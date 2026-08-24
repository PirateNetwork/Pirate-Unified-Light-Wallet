#!/usr/bin/env python3
"""Verify that the checked-in native FFI header matches the public Rust ABI."""

from __future__ import annotations

import argparse
from pathlib import Path
import re
import sys


ABI = {
    "pirate_wallet_service_free_string": {
        "rust": re.compile(
            r'pub\s+unsafe\s+extern\s+"C"\s+fn\s+'
            r"pirate_wallet_service_free_string\s*\(\s*ptr\s*:\s*\*mut\s+c_char\s*\)",
            re.MULTILINE,
        ),
        "c": "void pirate_wallet_service_free_string(char *ptr);",
    },
    "pirate_wallet_service_invoke_json": {
        "rust": re.compile(
            r'pub\s+unsafe\s+extern\s+"C"\s+fn\s+'
            r"pirate_wallet_service_invoke_json\s*\(\s*"
            r"request_json\s*:\s*\*const\s+c_char\s*,\s*"
            r"pretty\s*:\s*bool\s*,?\s*\)\s*->\s*\*mut\s+c_char",
            re.MULTILINE,
        ),
        "c": (
            "char *pirate_wallet_service_invoke_json("
            "const char *request_json, bool pretty);"
        ),
    },
}

RUST_EXPORT = re.compile(
    r"#\[unsafe\(no_mangle\)\]"
    r"(?:(?:\s*///[^\n]*)|\s)*?"
    r'pub\s+(?:unsafe\s+)?extern\s+"C"\s+fn\s+'
    r"(pirate_wallet_service_[A-Za-z0-9_]+)",
    re.MULTILINE,
)
HEADER_EXPORT = re.compile(
    r"^[^#\n;]*\b(pirate_wallet_service_[A-Za-z0-9_]+)\s*\([^;]*\);",
    re.MULTILINE,
)


def normalize_declaration(value: str) -> str:
    value = re.sub(r"\s+", " ", value.strip())
    value = re.sub(r"\s*\*\s*", " *", value)
    value = re.sub(r"\s*\(\s*", "(", value)
    value = re.sub(r"\s*\)\s*", ")", value)
    value = re.sub(r"\s*,\s*", ", ", value)
    return value


def verify(rust_source: str, header: str) -> list[str]:
    errors: list[str] = []
    expected = set(ABI)
    rust_exports = set(RUST_EXPORT.findall(rust_source))
    header_exports = set(HEADER_EXPORT.findall(header))

    if rust_exports != expected:
        errors.append(
            "Rust C exports differ from the reviewed ABI: "
            f"expected {sorted(expected)}, found {sorted(rust_exports)}"
        )
    if header_exports != expected:
        errors.append(
            "Header exports differ from the reviewed ABI: "
            f"expected {sorted(expected)}, found {sorted(header_exports)}"
        )

    normalized_header = normalize_declaration(header)
    for name, contract in ABI.items():
        if contract["rust"].search(rust_source) is None:
            errors.append(f"Rust signature drifted for {name}")
        expected_declaration = normalize_declaration(contract["c"])
        if expected_declaration not in normalized_header:
            errors.append(f"C declaration drifted for {name}")

    required_fragments = (
        "#include <stdbool.h>",
        '#ifdef __cplusplus extern "C" {',
    )
    for fragment in required_fragments:
        if normalize_declaration(fragment) not in normalized_header:
            errors.append(f"Header is missing required C ABI fragment: {fragment}")

    return errors


def build_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser()
    parser.add_argument("--rust-source", required=True, type=Path)
    parser.add_argument("--header", required=True, type=Path)
    return parser


def main(argv: list[str] | None = None) -> int:
    args = build_parser().parse_args(argv)
    try:
        rust_source = args.rust_source.read_text(encoding="utf-8")
        header = args.header.read_text(encoding="utf-8")
    except OSError as exc:
        print(f"[verify-native-ffi-header][ERROR] {exc}", file=sys.stderr)
        return 2

    errors = verify(rust_source, header)
    if errors:
        for error in errors:
            print(f"[verify-native-ffi-header][ERROR] {error}", file=sys.stderr)
        return 1

    print(
        f"[verify-native-ffi-header] Approved {len(ABI)} checked-in C ABI declarations."
    )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
