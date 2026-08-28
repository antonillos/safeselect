"""Local checks based on LobeHub's manifest reference, not an official schema.

Reference: https://market.lobehub.com/s/publish-mcp/references/manifest
Reviewed 2026-08-28. No network access or remote schema resolution.
"""

import json
import re
from urllib.parse import urlsplit


class ManifestError(ValueError):
    """Messages contain only fixed field names, never manifest values."""


def check(condition, message):
    if not condition:
        raise ManifestError(message)


def load_manifest(text):
    def pairs(items):
        result = {}
        for key, value in items:
            check(key not in result, "Duplicate JSON object key; remove the duplicate.")
            result[key] = value
        return result

    def constant(_value):
        raise ManifestError("Non-finite numbers are not valid JSON.")

    try:
        return json.loads(text, object_pairs_hook=pairs, parse_constant=constant)
    except json.JSONDecodeError:
        raise ManifestError("Invalid JSON syntax in manifest.") from None


def string(value, field, nonempty=False):
    check(isinstance(value, str) and (not nonempty or bool(value.strip())),
          f"{field} must be {'a non-empty string' if nonempty else 'a string'}.")


def strings(value, field):
    check(isinstance(value, list) and all(isinstance(v, str) for v in value),
          f"{field} must be an array of strings.")


def objects(value, field):
    check(isinstance(value, list) and all(isinstance(v, dict) for v in value),
          f"{field} must be an array of objects.")


def validate_manifest(manifest):
    check(isinstance(manifest, dict), "Manifest must be a JSON object.")
    allowed = set("identifier name version author authorUrl category cloudEndpoint description "
                  "homepage icon localizations prompts resources tags tools".split())
    check(not (manifest.keys() - allowed),
          "Unknown manifest field; LobeHub may silently discard it. Review the documented contract.")
    for field in ("identifier", "name", "version"):
        string(manifest.get(field), field, nonempty=True)
    check(re.fullmatch(r"[0-9a-z][0-9_a-z-]*", manifest["identifier"]),
          "identifier must match [0-9a-z][0-9_a-z-]*.")
    for field in ("author", "category", "description", "icon"):
        if field in manifest:
            string(manifest[field], field)
    for field in ("homepage", "authorUrl", "cloudEndpoint"):
        if field not in manifest:
            continue
        string(manifest[field], field, nonempty=True)
        try:
            url = urlsplit(manifest[field])
            valid = (url.scheme in ("https", "http") and bool(url.hostname)
                     and url.username is None and url.password is None
                     and not any(c.isspace() or ord(c) < 32 for c in manifest[field]))
            _ = url.port  # Reject malformed/out-of-range ports.
        except ValueError:
            valid = False
        check(valid, f"{field} must be an absolute HTTP(S) URL without credentials.")
    if "tags" in manifest:
        strings(manifest["tags"], "tags")
    for field in ("tools", "resources", "prompts"):
        if field in manifest:
            objects(manifest[field], field)
    prompt_names = set()
    for prompt in manifest.get("prompts", []):
        string(prompt.get("name"), "prompts.name", nonempty=True)
        check(prompt["name"] not in prompt_names, "Duplicate prompts.name.")
        prompt_names.add(prompt["name"])
        for field in ("title", "description"):
            if field in prompt:
                string(prompt[field], "prompts." + field)
        if "arguments" in prompt:
            objects(prompt["arguments"], "prompts.arguments")
            for argument in prompt["arguments"]:
                string(argument.get("name"), "prompts.arguments.name", nonempty=True)
                if "description" in argument:
                    string(argument["description"], "prompts.arguments.description")
                if "required" in argument:
                    check(isinstance(argument["required"], bool),
                          "prompts.arguments.required must be a boolean.")
    resource_uris = set()
    for resource in manifest.get("resources", []):
        for field in ("uri", "name"):
            string(resource.get(field), "resources." + field, nonempty=True)
        check(resource["uri"] not in resource_uris, "Duplicate resources.uri.")
        resource_uris.add(resource["uri"])
        for field in ("description", "mimeType"):
            if field in resource:
                string(resource[field], "resources." + field)
    names = set()
    for tool in manifest.get("tools", []):
        string(tool.get("name"), "tools.name", nonempty=True)
        check(tool["name"] not in names, "Duplicate tools.name.")
        names.add(tool["name"])
        if "description" in tool:
            string(tool["description"], "tools.description")
        for field in ("inputSchema", "outputSchema"):
            if field == "outputSchema" and field not in tool:
                continue
            schema = tool.get(field)
            check(isinstance(schema, dict) and schema.get("type") == "object",
                  f"tools.{field} must be an object schema with type=object.")
    if "localizations" in manifest:
        objects(manifest["localizations"], "localizations")
        locales = set()
        for item in manifest["localizations"]:
            check(not (item.keys() - {"locale", "name", "description", "summary", "tags"}),
                  "Unknown localization field.")
            string(item.get("locale"), "localizations.locale", nonempty=True)
            check(item["locale"] not in locales, "Duplicate localizations.locale.")
            locales.add(item["locale"])
            for field in ("name", "description", "summary"):
                if field in item:
                    string(item[field], "localizations." + field)
            if "tags" in item:
                strings(item["tags"], "localizations.tags")
