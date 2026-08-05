#!/usr/bin/env python3
import argparse
import json
import re
import subprocess
import sys
from pathlib import Path


MANIFEST_PATH = Path("release-artifacts.toml")
REACT_NATIVE_PACKAGE_PATH = Path("bindings/react-native-pirate-wallet/package.json")
REACT_NATIVE_ANDROID_PACKAGE_PATH = Path(
    "bindings/react-native-pirate-wallet-android/package.json"
)
TRACKED = (
    "cli",
    "qortal_cli",
    "qortal_jni",
    "native_ffi",
    "ios_sdk",
    "android_sdk",
    "react_native_plugin",
)


def load_manifest(ref: str | None) -> dict:
    if ref is None:
        raw = MANIFEST_PATH.read_text(encoding="utf-8")
    else:
        try:
            raw = subprocess.check_output(
                ["git", "show", f"{ref}:{MANIFEST_PATH.as_posix()}"], stderr=subprocess.DEVNULL
            ).decode("utf-8")
        except subprocess.CalledProcessError:
            return {}
    return parse_manifest(raw)


def parse_manifest(raw: str) -> dict:
    section_name = None
    data: dict[str, dict[str, str]] = {}
    section_pattern = re.compile(r"^\[([A-Za-z0-9_]+)\]\s*$")
    value_pattern = re.compile(r'^([A-Za-z0-9_]+)\s*=\s*"([^"]*)"\s*$')

    for line in raw.splitlines():
        line = line.strip()
        if not line or line.startswith("#"):
            continue

        section_match = section_pattern.match(line)
        if section_match:
            section_name = section_match.group(1)
            data.setdefault(section_name, {})
            continue

        value_match = value_pattern.match(line)
        if value_match and section_name is not None:
            key, value = value_match.groups()
            data[section_name][key] = value

    return data


def previous_tag() -> str | None:
    try:
        raw = subprocess.check_output(
            ["git", "describe", "--tags", "--abbrev=0", "HEAD^"],
            stderr=subprocess.DEVNULL,
        )
    except subprocess.CalledProcessError:
        return None
    tag = raw.decode("utf-8").strip()
    return tag or None


def changed_artifacts(current: dict, previous: dict | None) -> dict:
    result = {}
    for name in TRACKED:
        current_version = current.get(name, {}).get("version")
        previous_version = None if previous is None else previous.get(name, {}).get("version")
        result[name] = {
            "version": current_version,
            "previous_version": previous_version,
            "changed": previous is None or current_version != previous_version,
        }
    return result


def validate_source_versions(current: dict) -> None:
    package = json.loads(REACT_NATIVE_PACKAGE_PATH.read_text(encoding="utf-8"))
    android_package = json.loads(
        REACT_NATIVE_ANDROID_PACKAGE_PATH.read_text(encoding="utf-8")
    )
    package_version = package.get("version")
    android_package_version = android_package.get("version")
    release_version = current.get("react_native_plugin", {}).get("version")
    if package_version != release_version:
        raise ValueError(
            "React Native package version does not match release-artifacts.toml "
            f"({package_version!r} != {release_version!r})"
        )
    if android_package_version != release_version:
        raise ValueError(
            "React Native Android package version does not match release-artifacts.toml "
            f"({android_package_version!r} != {release_version!r})"
        )
    android_dependency_version = package.get("optionalDependencies", {}).get(
        "react-native-pirate-wallet-android"
    )
    if android_dependency_version != release_version:
        raise ValueError(
            "React Native Android dependency must use the exact release version "
            f"({android_dependency_version!r} != {release_version!r})"
        )


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--base-ref", default=None)
    parser.add_argument("--github-output", default=None)
    args = parser.parse_args()

    current = load_manifest(None)
    validate_source_versions(current)
    base_ref = args.base_ref or previous_tag()
    previous = load_manifest(base_ref) if base_ref else None
    result = changed_artifacts(current, previous)

    if args.github_output:
        with open(args.github_output, "a", encoding="utf-8") as handle:
            for name, data in result.items():
                handle.write(f"{name}_changed={'true' if data['changed'] else 'false'}\n")
                handle.write(f"{name}_version={data['version']}\n")

    json.dump(result, sys.stdout, indent=2, sort_keys=True)
    sys.stdout.write("\n")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
