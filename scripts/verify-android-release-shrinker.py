#!/usr/bin/env python3
"""Verify reflection-only Room database constructors survive release shrinking."""

from __future__ import annotations

import re
import sys
from pathlib import Path


WORK_DATABASE_IMPL = "androidx.work.impl.WorkDatabase_Impl ->"
REFLECTIVE_ROOM_FACTORIES = (
    "androidx.room.Room.getGeneratedImplementation(",
    "androidx.room.util.KClassUtil.findAndInstantiateDatabaseImpl(",
)
CLASS_HEADER = re.compile(r"^(?!#)\S.* -> .*:$")
NO_ARG_CONSTRUCTOR = re.compile(r"\bvoid <init>\(\)")


def main() -> int:
    if len(sys.argv) != 2:
        print(f"Usage: {Path(sys.argv[0]).name} <mapping.txt>", file=sys.stderr)
        return 2

    mapping_path = Path(sys.argv[1])
    try:
        lines = mapping_path.read_text(encoding="utf-8").splitlines()
    except OSError as error:
        print(f"Unable to read R8 mapping {mapping_path}: {error}", file=sys.stderr)
        return 1

    uses_reflection = any(
        marker in line for marker in REFLECTIVE_ROOM_FACTORIES for line in lines
    )
    if not uses_reflection:
        print(
            "Room does not use a reflective generated-database factory; "
            "no constructor check needed."
        )
        return 0

    try:
        class_start = next(
            index for index, line in enumerate(lines) if line.startswith(WORK_DATABASE_IMPL)
        )
    except StopIteration:
        print("R8 mapping does not contain WorkDatabase_Impl.", file=sys.stderr)
        return 1

    class_end = len(lines)
    for index in range(class_start + 1, len(lines)):
        if CLASS_HEADER.match(lines[index]):
            class_end = index
            break

    if not any(NO_ARG_CONSTRUCTOR.search(line) for line in lines[class_start:class_end]):
        print(
            "R8 removed WorkDatabase_Impl's no-argument constructor, "
            "which makes WorkManager crash during AndroidX Startup.",
            file=sys.stderr,
        )
        return 1

    print("Verified WorkDatabase_Impl is constructible after R8 shrinking.")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
