#!/usr/bin/env python3
"""Make one visible, reproducible MCP tool call against the local demo."""

import json
import os
import subprocess
import sys


def send(process, payload):
    process.stdin.write(json.dumps(payload) + "\n")
    process.stdin.flush()
    line = process.stdout.readline()
    if not line:
        stderr = process.stderr.read()
        raise RuntimeError(f"SafeSelect MCP server exited unexpectedly: {stderr}")
    return json.loads(line)


def notify(process, payload):
    process.stdin.write(json.dumps(payload) + "\n")
    process.stdin.flush()


def main():
    if len(sys.argv) != 4:
        raise SystemExit("Usage: mcp_call.py <environment> <tool> <json-arguments>")

    environment, tool, arguments = sys.argv[1:]
    root = os.path.dirname(os.path.abspath(__file__))
    process = subprocess.Popen(
        ["safeselect", "serve", "--project", root, "--environment", environment],
        stdin=subprocess.PIPE,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        text=True,
        env=os.environ.copy(),
    )
    try:
        send(
            process,
            {
                "jsonrpc": "2.0",
                "id": 1,
                "method": "initialize",
                "params": {
                    "protocolVersion": "2024-11-05",
                    "clientInfo": {"name": "safeselect-demo-vhs"},
                },
            },
        )
        notify(process, {"jsonrpc": "2.0", "method": "notifications/initialized"})
        response = send(
            process,
            {
                "jsonrpc": "2.0",
                "id": 2,
                "method": "tools/call",
                "params": {"name": tool, "arguments": json.loads(arguments)},
            },
        )
        result = response.get("result", response.get("error", response))
        content = result.get("content", []) if isinstance(result, dict) else []
        for item in content:
            if item.get("type") == "text":
                print(item["text"])
        if not content:
            print(json.dumps(result, indent=2))
    finally:
        process.kill()
        process.wait()


if __name__ == "__main__":
    main()
