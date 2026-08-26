#!/usr/bin/env python3
"""Validate and report the versioned adversarial case corpus."""

import argparse
import json
from pathlib import Path
import sys


REQUIRED_CASE_FIELDS = {
    "id",
    "backend",
    "operation",
    "payload",
    "expected_decision",
    "expected_state_unchanged",
    "controls",
}
ALLOWED_BACKENDS = {"postgresql", "mongodb", "both", "mcp"}
ALLOWED_DECISIONS = {"reject", "allow"}
ALLOWED_STATUSES = {"implemented", "planned"}


def load_manifest(path: Path) -> tuple[dict, list[str]]:
    errors: list[str] = []
    try:
        document = json.loads(path.read_text(encoding="utf-8"))
    except (OSError, json.JSONDecodeError) as error:
        return {}, [f"cannot read manifest: {error}"]

    if document.get("schema_version") != 1:
        errors.append("schema_version must be 1")
    cases = document.get("cases")
    if not isinstance(cases, list) or not cases:
        errors.append("cases must be a non-empty array")
        return document, errors

    seen_ids: set[str] = set()
    status_counts: dict[str, int] = {}
    for index, case in enumerate(cases):
        prefix = f"cases[{index}]"
        if not isinstance(case, dict):
            errors.append(f"{prefix} must be an object")
            continue
        missing = REQUIRED_CASE_FIELDS - case.keys()
        errors.extend(f"{prefix} missing {field}" for field in sorted(missing))
        case_id = case.get("id")
        if not isinstance(case_id, str) or not case_id:
            errors.append(f"{prefix}.id must be a non-empty string")
        elif case_id in seen_ids:
            errors.append(f"{prefix}.id is duplicated: {case_id}")
        else:
            seen_ids.add(case_id)
        if case.get("backend") not in ALLOWED_BACKENDS:
            errors.append(f"{prefix}.backend is unsupported")
        if case.get("expected_decision") not in ALLOWED_DECISIONS:
            errors.append(f"{prefix}.expected_decision is unsupported")
        if case.get("expected_state_unchanged") is not True:
            errors.append(f"{prefix}.expected_state_unchanged must be true")
        if not isinstance(case.get("controls"), list) or not case["controls"]:
            errors.append(f"{prefix}.controls must be a non-empty array")
        status = case.get("status", "implemented")
        if status not in ALLOWED_STATUSES:
            errors.append(f"{prefix}.status is unsupported")
        status_counts[status] = status_counts.get(status, 0) + 1
    document["_status_counts"] = status_counts
    return document, errors


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "--json", action="store_true", help="emit a machine-readable report"
    )
    parser.add_argument(
        "manifest",
        nargs="?",
        default=Path(__file__).with_name("adversarial-cases.json"),
        type=Path,
    )
    args = parser.parse_args()
    document, errors = load_manifest(args.manifest)
    cases = document.get("cases", [])
    report = {
        "manifest": str(args.manifest),
        "schema_version": document.get("schema_version"),
        "case_count": len(cases) if isinstance(cases, list) else 0,
        "status_counts": document.get("_status_counts", {}),
        "valid": not errors,
        "errors": errors,
    }
    if args.json:
        print(json.dumps(report, sort_keys=True))
    elif errors:
        print("Adversarial manifest: INVALID")
        print("\n".join(f"- {error}" for error in errors))
    else:
        print(f"Adversarial manifest: valid ({report['case_count']} cases)")
    return 0 if not errors else 1


if __name__ == "__main__":
    sys.exit(main())
