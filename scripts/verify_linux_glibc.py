#!/usr/bin/env python3
"""Reject Linux ELF payloads that exceed the supported glibc ABI floor."""

from __future__ import annotations

import argparse
from pathlib import Path
import re
import subprocess
import sys
from typing import Iterable


GLIBC_PATTERN = re.compile(r"\bGLIBC_(\d+(?:\.\d+)+)\b")
DEFAULT_MAX_GLIBC = "2.35"


def version_tuple(version: str) -> tuple[int, ...]:
    try:
        return tuple(int(part) for part in version.split("."))
    except ValueError as exc:
        raise ValueError(f"Invalid numeric version: {version}") from exc


def parse_glibc_versions(readelf_output: str) -> set[str]:
    return set(GLIBC_PATTERN.findall(readelf_output))


def is_elf(path: Path) -> bool:
    try:
        with path.open("rb") as stream:
            return stream.read(4) == b"\x7fELF"
    except OSError:
        return False


def iter_elf_files(roots: Iterable[Path]) -> Iterable[Path]:
    for root in roots:
        if not root.exists():
            raise FileNotFoundError(f"Compatibility scan path does not exist: {root}")
        candidates = [root] if root.is_file() else root.rglob("*")
        for candidate in candidates:
            if candidate.is_file() and is_elf(candidate):
                yield candidate


def read_glibc_versions(path: Path, readelf: str) -> set[str]:
    result = subprocess.run(
        [readelf, "--version-info", "--wide", str(path)],
        check=False,
        capture_output=True,
        text=True,
    )
    if result.returncode != 0:
        detail = result.stderr.strip() or "readelf returned no diagnostic"
        raise RuntimeError(f"Unable to inspect {path}: {detail}")
    return parse_glibc_versions(result.stdout)


def audit(
    roots: Iterable[Path],
    maximum: str,
    readelf: str,
) -> tuple[list[tuple[Path, str | None]], list[tuple[Path, str]]]:
    maximum_tuple = version_tuple(maximum)
    inspected: list[tuple[Path, str | None]] = []
    violations: list[tuple[Path, str]] = []

    for path in iter_elf_files(roots):
        versions = read_glibc_versions(path, readelf)
        highest = max(versions, key=version_tuple) if versions else None
        inspected.append((path, highest))
        if highest is not None and version_tuple(highest) > maximum_tuple:
            violations.append((path, highest))

    return inspected, violations


def build_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(
        description=(
            "Scan every ELF beneath one or more paths and reject symbols newer "
            "than the supported glibc version."
        )
    )
    parser.add_argument("paths", nargs="+", type=Path)
    parser.add_argument(
        "--max-version",
        default=DEFAULT_MAX_GLIBC,
        help=f"newest permitted GLIBC symbol version (default: {DEFAULT_MAX_GLIBC})",
    )
    parser.add_argument("--readelf", default="readelf")
    return parser


def main(argv: list[str] | None = None) -> int:
    args = build_parser().parse_args(argv)
    try:
        inspected, violations = audit(args.paths, args.max_version, args.readelf)
    except (FileNotFoundError, RuntimeError, ValueError) as exc:
        print(f"[verify-linux-glibc][ERROR] {exc}", file=sys.stderr)
        return 2

    if not inspected:
        print(
            "[verify-linux-glibc][ERROR] No ELF files found; refusing an empty compatibility check.",
            file=sys.stderr,
        )
        return 2

    for path, highest in inspected:
        requirement = f"GLIBC_{highest}" if highest else "no dynamic GLIBC imports"
        print(f"[verify-linux-glibc] {requirement}: {path}")

    if violations:
        print(
            f"[verify-linux-glibc][ERROR] {len(violations)} ELF file(s) exceed "
            f"the Ubuntu 22.04 ceiling GLIBC_{args.max_version}:",
            file=sys.stderr,
        )
        for path, highest in violations:
            print(f"  GLIBC_{highest}: {path}", file=sys.stderr)
        return 1

    print(
        f"[verify-linux-glibc] Approved {len(inspected)} ELF file(s) for "
        f"GLIBC_{args.max_version} and older."
    )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
