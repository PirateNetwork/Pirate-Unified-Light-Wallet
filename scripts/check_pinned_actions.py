#!/usr/bin/env python3
from __future__ import annotations

import re
import sys
from pathlib import Path


WORKFLOWS_DIR = Path(".github/workflows")
USES_PATTERN = re.compile(r"^(?P<indent>\s*-?\s*uses:\s*)(?P<value>\S+)")
SHA_PATTERN = re.compile(r"^[0-9a-f]{40}$")


def iter_workflow_files() -> list[Path]:
    return sorted(
        path
        for path in WORKFLOWS_DIR.rglob("*")
        if path.is_file() and path.suffix in {".yml", ".yaml"}
    )


def is_external_action(value: str) -> bool:
    if value.startswith("./") or value.startswith("docker://"):
        return False
    return "@" in value and "/" in value


def validate_action_ref(path: Path, line_number: int, value: str) -> str | None:
    if not is_external_action(value):
        return None

    action, ref = value.rsplit("@", 1)
    if not SHA_PATTERN.fullmatch(ref):
        return (
            f"{path}:{line_number}: mutable GitHub Action ref '{value}' found. "
            f"Pin '{action}' to a full 40-character commit SHA."
        )
    return None


def main() -> int:
    failures: list[str] = []

    for workflow_path in iter_workflow_files():
        for line_number, line in enumerate(workflow_path.read_text().splitlines(), start=1):
            match = USES_PATTERN.match(line)
            if not match:
                continue
            value = match.group("value").strip().strip("'\"")
            failure = validate_action_ref(workflow_path, line_number, value)
            if failure:
                failures.append(failure)

    if failures:
        print("Found mutable GitHub Action references:\n", file=sys.stderr)
        for failure in failures:
            print(f"  - {failure}", file=sys.stderr)
        return 1

    print("All external GitHub Actions are pinned to immutable commit SHAs.")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
