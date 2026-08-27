#!/usr/bin/env python3
"""Verify that an AppImage contains a pinned runtime after finalization."""

from __future__ import annotations

import argparse
import mmap
from dataclasses import dataclass
from pathlib import Path
import struct
import sys
from typing import Sequence


APPIMAGE_TYPE_2_MAGIC = b"AI\x02"
DIGEST_SECTION_NAME = b".digest_md5"
DIGEST_WRITE_SIZE = 16
SQUASHFS_MAGIC = b"hsqs"


class VerificationError(RuntimeError):
    """Raised when a packaged AppImage violates the runtime policy."""


@dataclass(frozen=True)
class ElfSection:
    """The file range occupied by one ELF section."""

    offset: int
    size: int


def _unpack(data: mmap.mmap, fmt: str, offset: int) -> int:
    size = struct.calcsize(fmt)
    if offset < 0 or offset + size > len(data):
        raise VerificationError("ELF metadata extends beyond the runtime")
    return int(struct.unpack_from(fmt, data, offset)[0])


def _section_headers(data: mmap.mmap) -> tuple[str, int, int, int, int]:
    if len(data) < 16 or data[:4] != b"\x7fELF":
        raise VerificationError("Pinned runtime is not an ELF executable")

    elf_class = data[4]
    encoding = data[5]
    if encoding == 1:
        byte_order = "<"
    elif encoding == 2:
        byte_order = ">"
    else:
        raise VerificationError("Pinned runtime uses an unsupported ELF encoding")

    if elf_class == 2:
        section_offset = _unpack(data, f"{byte_order}Q", 40)
        entry_size = _unpack(data, f"{byte_order}H", 58)
        entry_count = _unpack(data, f"{byte_order}H", 60)
        names_index = _unpack(data, f"{byte_order}H", 62)
        minimum_entry_size = 64
    elif elf_class == 1:
        section_offset = _unpack(data, f"{byte_order}I", 32)
        entry_size = _unpack(data, f"{byte_order}H", 46)
        entry_count = _unpack(data, f"{byte_order}H", 48)
        names_index = _unpack(data, f"{byte_order}H", 50)
        minimum_entry_size = 40
    else:
        raise VerificationError("Pinned runtime uses an unsupported ELF class")

    if entry_count == 0:
        raise VerificationError("Extended ELF section counts are not supported")
    if entry_size < minimum_entry_size or names_index >= entry_count:
        raise VerificationError("Pinned runtime has invalid ELF section metadata")
    if section_offset + entry_size * entry_count > len(data):
        raise VerificationError("ELF section table extends beyond the runtime")
    return byte_order, elf_class, section_offset, entry_size, names_index


def _read_section(
    data: mmap.mmap,
    byte_order: str,
    elf_class: int,
    section_offset: int,
    entry_size: int,
    index: int,
) -> tuple[int, ElfSection]:
    entry = section_offset + entry_size * index
    name_offset = _unpack(data, f"{byte_order}I", entry)
    if elf_class == 2:
        file_offset = _unpack(data, f"{byte_order}Q", entry + 24)
        size = _unpack(data, f"{byte_order}Q", entry + 32)
    else:
        file_offset = _unpack(data, f"{byte_order}I", entry + 16)
        size = _unpack(data, f"{byte_order}I", entry + 20)
    if file_offset + size > len(data):
        raise VerificationError("ELF section extends beyond the runtime")
    return name_offset, ElfSection(file_offset, size)


def find_elf_section(data: mmap.mmap, name: bytes) -> ElfSection:
    """Locate an ELF section by name without depending on host binutils."""

    byte_order, elf_class, section_offset, entry_size, names_index = (
        _section_headers(data)
    )
    entry_count_offset = 60 if elf_class == 2 else 48
    entry_count = _unpack(data, f"{byte_order}H", entry_count_offset)
    _, names = _read_section(
        data,
        byte_order,
        elf_class,
        section_offset,
        entry_size,
        names_index,
    )
    names_data = data[names.offset : names.offset + names.size]

    for index in range(entry_count):
        name_offset, section = _read_section(
            data,
            byte_order,
            elf_class,
            section_offset,
            entry_size,
            index,
        )
        if name_offset >= len(names_data):
            raise VerificationError("ELF section name extends beyond its string table")
        terminator = names_data.find(b"\x00", name_offset)
        if terminator < 0:
            raise VerificationError("ELF section name is not terminated")
        if names_data[name_offset:terminator] == name:
            return section

    raise VerificationError(f"Pinned runtime has no {name.decode()} section")


def _require_equal(
    runtime: mmap.mmap,
    appimage: mmap.mmap,
    start: int,
    end: int,
) -> None:
    if runtime[start:end] == appimage[start:end]:
        return
    for offset in range(start, end):
        if runtime[offset] != appimage[offset]:
            raise VerificationError(
                "Packaged runtime differs from the pinned runtime at "
                f"unauthorized offset {offset}"
            )
    raise VerificationError("Packaged runtime differs outside the digest section")


def verify(appimage_path: Path, runtime_path: Path) -> ElfSection:
    """Verify runtime identity while allowing appimagetool's MD5 finalization."""

    runtime_size = runtime_path.stat().st_size
    appimage_size = appimage_path.stat().st_size
    if runtime_size <= 0:
        raise VerificationError("Pinned AppImage runtime is empty")
    if appimage_size < runtime_size + len(SQUASHFS_MAGIC):
        raise VerificationError("AppImage has no payload after its runtime")

    with runtime_path.open("rb") as runtime_file, appimage_path.open(
        "rb"
    ) as appimage_file:
        with mmap.mmap(runtime_file.fileno(), 0, access=mmap.ACCESS_READ) as runtime:
            with mmap.mmap(
                appimage_file.fileno(), 0, access=mmap.ACCESS_READ
            ) as appimage:
                digest = find_elf_section(runtime, DIGEST_SECTION_NAME)
                if digest.size < DIGEST_WRITE_SIZE:
                    raise VerificationError(
                        ".digest_md5 is smaller than appimagetool's 16-byte write"
                    )
                digest_end = digest.offset + DIGEST_WRITE_SIZE

                if appimage[8:11] != APPIMAGE_TYPE_2_MAGIC:
                    raise VerificationError("Output is not a type-2 AppImage")
                _require_equal(runtime, appimage, 0, digest.offset)
                _require_equal(runtime, appimage, digest_end, runtime_size)

                if runtime[digest.offset:digest_end] == appimage[
                    digest.offset:digest_end
                ]:
                    raise VerificationError(
                        "appimagetool did not finalize the .digest_md5 section"
                    )
                if appimage[runtime_size : runtime_size + 4] != SQUASHFS_MAGIC:
                    raise VerificationError(
                        "SquashFS payload does not begin at the runtime boundary"
                    )
                return digest


def main(argv: Sequence[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("appimage", type=Path)
    parser.add_argument("runtime", type=Path)
    args = parser.parse_args(argv)

    try:
        digest = verify(args.appimage, args.runtime)
    except (OSError, VerificationError) as error:
        print(f"[verify-appimage-runtime] ERROR: {error}", file=sys.stderr)
        return 1

    runtime_size = args.runtime.stat().st_size
    print(
        "[verify-appimage-runtime] verified pinned runtime "
        f"({runtime_size} bytes; .digest_md5 offset {digest.offset})"
    )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
