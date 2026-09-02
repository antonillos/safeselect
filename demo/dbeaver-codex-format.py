#!/usr/bin/env python3
"""Render safe, colorized Codex JSONL events without exposing raw payloads."""
from __future__ import annotations

import json
import os
import re
import sys

RESET = "\033[0m"
DIM = "\033[2m"
CYAN = "\033[96m"
GREEN = "\033[92m"
YELLOW = "\033[93m"
MAGENTA = "\033[95m"
ANSI = re.compile(r"\033\[[0-9;]*m")
PRIVATE_KEY = re.compile(
    r"-----BEGIN [^-]*PRIVATE KEY-----.+?-----END [^-]*PRIVATE KEY-----",
    re.DOTALL,
)
SECRET_ASSIGNMENT = re.compile(
    r"(?i)\b(password|passwd|secret|token|api[_-]?key|authorization)\b\s*[:=]\s*[^\s,;]+"
)
PERSONAL_PATH = re.compile(r"/Users/[^\s\"']+")
SHOW_ALL = os.environ.get("SAFESELECT_SHOW_ALL") == "1"


def text_from(value: object) -> str:
    if isinstance(value, str):
        return value.strip()
    if isinstance(value, list):
        return " ".join(text_from(item) for item in value if text_from(item))
    if isinstance(value, dict):
        for key in ("text", "content", "summary"):
            result = text_from(value.get(key))
            if result:
                return result
    return ""


def first_value(value: object, keys: tuple[str, ...]) -> str:
    """Find a useful display field without printing untrusted event payloads."""
    if isinstance(value, dict):
        for key in keys:
            candidate = value.get(key)
            if isinstance(candidate, str) and candidate.strip():
                return candidate.strip()
        for child in value.values():
            found = first_value(child, keys)
            if found:
                return found
    elif isinstance(value, list):
        for child in value:
            found = first_value(child, keys)
            if found:
                return found
    return ""


def emit(color: str, label: str, message: str) -> None:
    message = ANSI.sub("", message).replace("\n", " ").strip()
    message = PRIVATE_KEY.sub("[private-key-redacted]", message)
    message = SECRET_ASSIGNMENT.sub(
        lambda match: f"{match.group(1)}=[redacted]", message
    )
    message = message.replace("demo-password", "[db-password-redacted]")
    message = PERSONAL_PATH.sub("[personal-path]", message)
    if message:
        print(f"{color}{label}{RESET} {message[:700]}", flush=True)


def render(line: str) -> None:
    clean = ANSI.sub("", line).strip()
    try:
        event = json.loads(clean)
    except json.JSONDecodeError:
        if (
            not SHOW_ALL
            and (
                "guardian::review_session" in clean
                or "trunk rollout snapshot" in clean
            )
        ):
            return
        if clean:
            emit(DIM, "codex", clean)
        return
    kind = event.get("type", "")
    item = event.get("item") or {}
    item_kind = item.get("type", "") if isinstance(item, dict) else ""
    if kind == "item.completed" and item_kind == "reasoning":
        summary = text_from(item.get("summary"))
        emit(CYAN, "thinking ›", summary or "summary redacted by Codex")
    elif kind == "item.completed" and item_kind in {"agent_message", "message"}:
        emit(GREEN, "codex ›", text_from(item))
    elif "mcp" in kind.lower() or "mcp" in item_kind.lower():
        server = first_value(event, ("server_label", "server", "mcp_server"))
        name = first_value(
            event,
            ("tool_name", "mcp_tool_name", "function", "method", "operation", "name", "tool"),
        )
        if name in {"mcp_tool_call", "mcp_tool_result"}:
            name = ""
        emit(MAGENTA, "mcp ›", "/".join(part for part in (server, name) if part) or "call")
    elif kind == "item.completed" and ("tool" in item_kind or "command" in item_kind):
        # Command payloads are noisy and can contain sensitive paths; show only
        # the phase marker rather than echoing the command itself.
        if item_kind != "command_execution" or SHOW_ALL:
            name = first_value(item, ("name", "tool_name", "function")) or item_kind
            emit(YELLOW, "tool ›", name)
    elif kind in {"turn.started", "turn.completed", "thread.started"}:
        emit(DIM, "event ›", kind)
    elif SHOW_ALL:
        label = "/".join(part for part in (kind, item_kind) if part)
        emit(DIM, "event ›", label or "unclassified")


for line in sys.stdin:
    render(line)
