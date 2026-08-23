#!/usr/bin/env python3
"""Make the resolved Komodo asset transformer configuration hermetic.

The pinned coin configuration and icons are materialized separately by
``prefetch-komodo-assets.py``. Native KDF executables are fetched by
``prefetch-kdf-artifact.sh`` and verified against the checksums in the same SDK
configuration. This tool checks that all materialized assets are present, then
atomically disables every build-time network/update switch before Flutter
invokes the SDK transformer.
"""

from __future__ import annotations

import argparse
import json
import os
from pathlib import Path, PurePosixPath
import re
import stat
import sys
import tempfile
from typing import Any
from urllib.parse import unquote, urljoin, urlparse


PACKAGE_NAME = "komodo_defi_framework"
PIN_PATTERN = re.compile(r"^[0-9a-f]{40}$")


class ConfigurationError(RuntimeError):
    """Raised when the resolved SDK assets cannot support an offline build."""


def _load_json(path: Path) -> dict[str, Any]:
    try:
        value = json.loads(path.read_text(encoding="utf-8"))
    except FileNotFoundError as exc:
        raise ConfigurationError(f"Required file does not exist: {path}") from exc
    except json.JSONDecodeError as exc:
        raise ConfigurationError(f"Invalid JSON in {path}: {exc}") from exc
    if not isinstance(value, dict):
        raise ConfigurationError(f"Expected a JSON object in {path}")
    return value


def _validate_json(path: Path) -> None:
    try:
        json.loads(path.read_text(encoding="utf-8"))
    except json.JSONDecodeError as exc:
        raise ConfigurationError(f"Invalid JSON in {path}: {exc}") from exc


def _file_uri_to_path(uri: str) -> Path:
    parsed = urlparse(uri)
    if parsed.scheme != "file":
        raise ConfigurationError(f"Only file package roots are supported, got {uri!r}")

    decoded_path = unquote(parsed.path)
    if parsed.netloc and parsed.netloc != "localhost":
        decoded_path = f"//{parsed.netloc}{decoded_path}"

    drive_match = re.match(r"^/([A-Za-z]):/(.*)$", decoded_path)
    if drive_match:
        drive, remainder = drive_match.groups()
        if os.name == "nt":
            decoded_path = f"{drive}:/{remainder}"
        else:
            decoded_path = f"/mnt/{drive.lower()}/{remainder}"
    return Path(decoded_path)


def resolve_package_root(package_config_path: Path) -> Path:
    package_config_path = package_config_path.resolve()
    package_config = _load_json(package_config_path)
    packages = package_config.get("packages")
    if not isinstance(packages, list):
        raise ConfigurationError(
            f"Missing packages array in {package_config_path}",
        )

    for package in packages:
        if not isinstance(package, dict) or package.get("name") != PACKAGE_NAME:
            continue
        root_uri = package.get("rootUri")
        if not isinstance(root_uri, str) or not root_uri:
            raise ConfigurationError(
                f"{PACKAGE_NAME} has no rootUri in {package_config_path}",
            )
        absolute_uri = urljoin(package_config_path.as_uri(), root_uri)
        package_root = _file_uri_to_path(absolute_uri).resolve()
        if not package_root.is_dir():
            raise ConfigurationError(
                f"Resolved {PACKAGE_NAME} package root does not exist: {package_root}",
            )
        return package_root

    raise ConfigurationError(
        f"Package {PACKAGE_NAME!r} was not found in {package_config_path}",
    )


def _section(config: dict[str, Any], name: str, config_path: Path) -> dict[str, Any]:
    value = config.get(name)
    if not isinstance(value, dict):
        raise ConfigurationError(f"Missing {name!r} object in {config_path}")
    return value


def _require_bool(section: dict[str, Any], key: str, config_path: Path) -> None:
    if not isinstance(section.get(key), bool):
        raise ConfigurationError(
            f"Expected boolean {key!r} in {config_path}",
        )


def _require_pin(section: dict[str, Any], key: str, config_path: Path) -> None:
    value = section.get(key)
    if not isinstance(value, str) or not PIN_PATTERN.fullmatch(value):
        raise ConfigurationError(
            f"Expected a pinned 40-character commit in {key!r} in {config_path}",
        )


def _asset_path(package_root: Path, relative: str, config_path: Path) -> Path:
    posix_path = PurePosixPath(relative.rstrip("/"))
    if posix_path.is_absolute() or not posix_path.parts or ".." in posix_path.parts:
        raise ConfigurationError(
            f"Unsafe materialized asset path {relative!r} in {config_path}",
        )

    package_root = package_root.resolve()
    candidate = package_root.joinpath(*posix_path.parts).resolve()
    try:
        candidate.relative_to(package_root)
    except ValueError as exc:
        raise ConfigurationError(
            f"Materialized asset path escapes the package root: {relative!r}",
        ) from exc
    return candidate


def _validate_materialized_coin_assets(
    package_root: Path,
    coins: dict[str, Any],
    config_path: Path,
) -> tuple[int, int]:
    mapped_files = coins.get("mapped_files")
    if not isinstance(mapped_files, dict) or not mapped_files:
        raise ConfigurationError(f"No mapped_files configured in {config_path}")

    file_count = 0
    for relative in mapped_files:
        if not isinstance(relative, str):
            raise ConfigurationError(f"Non-string mapped file path in {config_path}")
        asset = _asset_path(package_root, relative, config_path)
        if not asset.is_file() or asset.stat().st_size == 0:
            raise ConfigurationError(f"Bundled coin asset is missing or empty: {asset}")
        if asset.suffix.lower() == ".json":
            _validate_json(asset)
        file_count += 1

    mapped_folders = coins.get("mapped_folders")
    if not isinstance(mapped_folders, dict) or not mapped_folders:
        raise ConfigurationError(f"No mapped_folders configured in {config_path}")

    folder_file_count = 0
    for relative in mapped_folders:
        if not isinstance(relative, str):
            raise ConfigurationError(f"Non-string mapped folder path in {config_path}")
        asset_dir = _asset_path(package_root, relative, config_path)
        if not asset_dir.is_dir():
            raise ConfigurationError(f"Bundled coin asset directory is missing: {asset_dir}")
        files = [
            path
            for path in asset_dir.rglob("*")
            if path.is_file()
            and not any(
                part.startswith(".") for part in path.relative_to(asset_dir).parts
            )
        ]
        if not files:
            raise ConfigurationError(f"Bundled coin asset directory is empty: {asset_dir}")
        empty = next((path for path in files if path.stat().st_size == 0), None)
        if empty is not None:
            raise ConfigurationError(f"Bundled coin asset is empty: {empty}")
        folder_file_count += len(files)

    return file_count, folder_file_count


def _write_json_atomically(path: Path, value: dict[str, Any]) -> None:
    mode = stat.S_IMODE(path.stat().st_mode)
    serialized = json.dumps(value, indent=4, ensure_ascii=False) + "\n"
    temporary_path: Path | None = None
    try:
        with tempfile.NamedTemporaryFile(
            mode="w",
            encoding="utf-8",
            newline="\n",
            prefix=f".{path.name}.",
            suffix=".tmp",
            dir=path.parent,
            delete=False,
        ) as temporary:
            temporary.write(serialized)
            temporary.flush()
            os.fsync(temporary.fileno())
            temporary_path = Path(temporary.name)
        temporary_path.chmod(mode)
        os.replace(temporary_path, path)
        temporary_path = None
    finally:
        if temporary_path is not None:
            temporary_path.unlink(missing_ok=True)


def configure(package_config_path: Path) -> tuple[Path, int, int, bool]:
    package_root = resolve_package_root(package_config_path)
    config_path = package_root / "app_build" / "build_config.json"
    config = _load_json(config_path)
    api = _section(config, "api", config_path)
    coins = _section(config, "coins", config_path)

    _require_pin(api, "api_commit_hash", config_path)
    _require_pin(coins, "bundled_coins_repo_commit", config_path)
    _require_bool(api, "fetch_at_build_enabled", config_path)
    _require_bool(coins, "fetch_at_build_enabled", config_path)
    _require_bool(coins, "update_commit_on_build", config_path)
    platforms = api.get("platforms")
    if not isinstance(platforms, dict) or not platforms:
        raise ConfigurationError(f"No pinned KDF platforms configured in {config_path}")

    mapped_files, folder_files = _validate_materialized_coin_assets(
        package_root,
        coins,
        config_path,
    )

    changed = any(
        (
            api["fetch_at_build_enabled"],
            coins["fetch_at_build_enabled"],
            coins["update_commit_on_build"],
        ),
    )
    api["fetch_at_build_enabled"] = False
    coins["fetch_at_build_enabled"] = False
    coins["update_commit_on_build"] = False
    if changed:
        _write_json_atomically(config_path, config)

    return config_path, mapped_files, folder_files, changed


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(
        description="Validate materialized Komodo assets and disable transformer downloads.",
    )
    parser.add_argument(
        "--package-config",
        type=Path,
        default=Path("app/.dart_tool/package_config.json"),
        help="Flutter package_config.json produced by flutter pub get",
    )
    args = parser.parse_args(argv)

    try:
        config_path, mapped_files, folder_files, changed = configure(
            args.package_config,
        )
    except ConfigurationError as exc:
        print(f"[configure-komodo-assets][ERROR] {exc}", file=sys.stderr)
        return 1

    action = "configured" if changed else "already configured"
    print(
        f"[configure-komodo-assets] {action}: {config_path} "
        f"({mapped_files} config files, {folder_files} folder assets validated)",
    )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
