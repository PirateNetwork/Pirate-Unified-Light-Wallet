#!/usr/bin/env python3
"""Materialize pinned Komodo coin assets without invoking its transformer.

The upstream Flutter SDK intentionally keeps generated coin JSON and icons out
of Git. Fresh Pub checkouts therefore contain only ``.gitkeep`` placeholders.
This tool downloads one immutable coins-repository archive, verifies its
checked-in SHA-256, and copies only the paths declared by the SDK build config.
It records hashes for every materialized file so later builds can validate and
reuse the cache without trusting file existence alone.
"""

from __future__ import annotations

import argparse
import hashlib
import json
import os
from pathlib import Path, PurePosixPath
import re
import shutil
import stat
import sys
import tempfile
import time
from typing import Any
from urllib.error import HTTPError, URLError
from urllib.parse import unquote, urljoin, urlparse
from urllib.request import Request, urlopen
import zipfile


PACKAGE_NAME = "komodo_defi_framework"
PIN_PATTERN = re.compile(r"^[0-9a-f]{40}$")
SHA256_PATTERN = re.compile(r"^[0-9a-f]{64}$")
STAMP_VERSION = 1
STAMP_RELATIVE_PATH = Path("app_build") / "pulw_coin_assets.lock.json"
MAX_ARCHIVE_BYTES = 128 * 1024 * 1024
MAX_MATERIALIZED_BYTES = 64 * 1024 * 1024
RETRIABLE_HTTP_CODES = {408, 429, 500, 502, 503, 504}


class AssetPreparationError(RuntimeError):
    """Raised when pinned coin assets cannot be prepared safely."""


def _load_json(path: Path) -> dict[str, Any]:
    try:
        value = json.loads(path.read_text(encoding="utf-8"))
    except FileNotFoundError as exc:
        raise AssetPreparationError(f"Required file does not exist: {path}") from exc
    except json.JSONDecodeError as exc:
        raise AssetPreparationError(f"Invalid JSON in {path}: {exc}") from exc
    if not isinstance(value, dict):
        raise AssetPreparationError(f"Expected a JSON object in {path}")
    return value


def _file_uri_to_path(uri: str) -> Path:
    parsed = urlparse(uri)
    if parsed.scheme != "file":
        raise AssetPreparationError(f"Only file package roots are supported, got {uri!r}")
    decoded = unquote(parsed.path)
    if parsed.netloc and parsed.netloc != "localhost":
        decoded = f"//{parsed.netloc}{decoded}"
    drive_match = re.match(r"^/([A-Za-z]):/(.*)$", decoded)
    if drive_match:
        drive, remainder = drive_match.groups()
        decoded = (
            f"{drive}:/{remainder}"
            if os.name == "nt"
            else f"/mnt/{drive.lower()}/{remainder}"
        )
    return Path(decoded)


def resolve_package_root(package_config_path: Path) -> Path:
    package_config_path = package_config_path.resolve()
    packages = _load_json(package_config_path).get("packages")
    if not isinstance(packages, list):
        raise AssetPreparationError(f"Missing packages array in {package_config_path}")
    for package in packages:
        if not isinstance(package, dict) or package.get("name") != PACKAGE_NAME:
            continue
        root_uri = package.get("rootUri")
        if not isinstance(root_uri, str) or not root_uri:
            raise AssetPreparationError(f"{PACKAGE_NAME} has no rootUri")
        root = _file_uri_to_path(urljoin(package_config_path.as_uri(), root_uri)).resolve()
        if not root.is_dir():
            raise AssetPreparationError(f"Resolved package root does not exist: {root}")
        return root
    raise AssetPreparationError(f"Package {PACKAGE_NAME!r} was not found")


def _safe_relative(value: str, *, label: str) -> PurePosixPath:
    if "\\" in value:
        raise AssetPreparationError(f"Unsafe {label} path: {value!r}")
    relative = PurePosixPath(value.rstrip("/"))
    if relative.is_absolute() or not relative.parts or ".." in relative.parts:
        raise AssetPreparationError(f"Unsafe {label} path: {value!r}")
    return relative


def _destination(package_root: Path, relative: str) -> Path:
    parts = _safe_relative(relative, label="destination").parts
    package_root = package_root.resolve()
    candidate = package_root.joinpath(*parts).resolve()
    try:
        candidate.relative_to(package_root)
    except ValueError as exc:
        raise AssetPreparationError(f"Destination escapes package root: {relative!r}") from exc
    return candidate


def _sha256(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as stream:
        for chunk in iter(lambda: stream.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def _write_bytes_atomically(path: Path, content: bytes) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    temporary: Path | None = None
    try:
        with tempfile.NamedTemporaryFile(
            mode="wb",
            prefix=f".{path.name}.",
            suffix=".tmp",
            dir=path.parent,
            delete=False,
        ) as output:
            output.write(content)
            output.flush()
            os.fsync(output.fileno())
            temporary = Path(output.name)
        os.replace(temporary, path)
        temporary = None
    finally:
        if temporary is not None:
            temporary.unlink(missing_ok=True)


def _write_json_atomically(path: Path, value: dict[str, Any]) -> None:
    _write_bytes_atomically(
        path,
        (json.dumps(value, indent=2, sort_keys=True) + "\n").encode("utf-8"),
    )


def _load_lock(lock_path: Path) -> dict[str, str]:
    value = _load_json(lock_path.resolve())
    required = ("repository", "commit", "archive_url", "archive_sha256")
    if any(not isinstance(value.get(key), str) for key in required):
        raise AssetPreparationError(f"Incomplete coin asset lock: {lock_path}")
    lock = {key: value[key] for key in required}
    if lock["repository"] != "kmdclassic/coins":
        raise AssetPreparationError(f"Unexpected coin repository: {lock['repository']}")
    if not PIN_PATTERN.fullmatch(lock["commit"]):
        raise AssetPreparationError("Coin asset lock commit is not a full SHA")
    if not SHA256_PATTERN.fullmatch(lock["archive_sha256"]):
        raise AssetPreparationError("Coin archive SHA-256 is invalid")
    expected_url = f"https://codeload.github.com/kmdclassic/coins/zip/{lock['commit']}"
    if lock["archive_url"] != expected_url:
        raise AssetPreparationError("Coin archive URL does not match its pinned commit")
    return lock


def _download(url: str, destination: Path) -> None:
    for attempt in range(1, 6):
        try:
            request = Request(url, headers={"User-Agent": "pirate-wallet-hermetic-build"})
            with urlopen(request, timeout=120) as response, destination.open("wb") as output:
                written = 0
                while chunk := response.read(1024 * 1024):
                    written += len(chunk)
                    if written > MAX_ARCHIVE_BYTES:
                        raise AssetPreparationError("Coin archive exceeds safety limit")
                    output.write(chunk)
            if written == 0:
                raise AssetPreparationError("Coin archive download was empty")
            return
        except HTTPError as exc:
            if exc.code not in RETRIABLE_HTTP_CODES or attempt == 5:
                raise AssetPreparationError(f"Coin archive request failed: {exc}") from exc
            error: Exception = exc
        except (URLError, TimeoutError, ConnectionError, OSError) as exc:
            if attempt == 5:
                raise AssetPreparationError(f"Coin archive download failed: {exc}") from exc
            error = exc
        destination.unlink(missing_ok=True)
        delay = min(2 ** (attempt - 1), 8)
        print(
            f"[prefetch-komodo-assets] download attempt {attempt}/5 failed: "
            f"{error}; retrying in {delay}s",
            file=sys.stderr,
            flush=True,
        )
        time.sleep(delay)


def _config_maps(package_root: Path) -> tuple[dict[str, str], dict[str, str], str]:
    config_path = package_root / "app_build" / "build_config.json"
    coins = _load_json(config_path).get("coins")
    if not isinstance(coins, dict):
        raise AssetPreparationError(f"Missing coins configuration in {config_path}")
    commit = coins.get("bundled_coins_repo_commit")
    mapped_files = coins.get("mapped_files")
    mapped_folders = coins.get("mapped_folders")
    if not isinstance(commit, str) or not PIN_PATTERN.fullmatch(commit):
        raise AssetPreparationError(f"Invalid bundled coin commit in {config_path}")
    if not isinstance(mapped_files, dict) or not mapped_files:
        raise AssetPreparationError(f"No mapped_files configured in {config_path}")
    if not isinstance(mapped_folders, dict) or not mapped_folders:
        raise AssetPreparationError(f"No mapped_folders configured in {config_path}")
    if any(
        not isinstance(key, str) or not isinstance(value, str)
        for key, value in mapped_files.items()
    ):
        raise AssetPreparationError(f"Invalid mapped_files in {config_path}")
    if any(
        not isinstance(key, str) or not isinstance(value, str)
        for key, value in mapped_folders.items()
    ):
        raise AssetPreparationError(f"Invalid mapped_folders in {config_path}")
    return dict(mapped_files), dict(mapped_folders), commit


def _allowed_manifest_path(
    relative: str,
    mapped_files: dict[str, str],
    mapped_folders: dict[str, str],
) -> bool:
    path = _safe_relative(relative, label="manifest")
    if relative in mapped_files:
        return True
    for destination in mapped_folders:
        folder = _safe_relative(destination, label="mapped folder")
        try:
            path.relative_to(folder)
            return len(path.parts) > len(folder.parts)
        except ValueError:
            continue
    return False


def _validate_stamp(
    package_root: Path,
    lock: dict[str, str],
    mapped_files: dict[str, str],
    mapped_folders: dict[str, str],
) -> tuple[int, int] | None:
    stamp_path = package_root / STAMP_RELATIVE_PATH
    try:
        stamp = _load_json(stamp_path)
    except AssetPreparationError:
        return None
    if (
        stamp.get("version") != STAMP_VERSION
        or stamp.get("commit") != lock["commit"]
        or stamp.get("archive_sha256") != lock["archive_sha256"]
    ):
        return None
    files = stamp.get("files")
    if not isinstance(files, dict) or not files:
        return None
    if any(destination.rstrip("/") not in files for destination in mapped_files):
        return None
    for destination in mapped_folders:
        prefix = destination.rstrip("/") + "/"
        if not any(relative.startswith(prefix) for relative in files):
            return None
        destination_root = _destination(package_root, destination)
        expected_folder_files = {
            relative for relative in files if relative.startswith(prefix)
        }
        actual_folder_files = {
            PurePosixPath(
                destination.rstrip("/"),
                *path.relative_to(destination_root).parts,
            ).as_posix()
            for path in destination_root.rglob("*")
            if path.is_file()
            and not any(
                part.startswith(".")
                for part in path.relative_to(destination_root).parts
            )
        }
        if actual_folder_files != expected_folder_files:
            return None
    folder_count = 0
    for relative, expected_hash in files.items():
        if (
            not isinstance(relative, str)
            or not isinstance(expected_hash, str)
            or not SHA256_PATTERN.fullmatch(expected_hash)
            or not _allowed_manifest_path(relative, mapped_files, mapped_folders)
        ):
            return None
        asset = _destination(package_root, relative)
        if not asset.is_file() or asset.stat().st_size == 0 or _sha256(asset) != expected_hash:
            return None
        if relative not in mapped_files:
            folder_count += 1
    return len(mapped_files), folder_count


def _validated_members(archive: zipfile.ZipFile) -> tuple[str, dict[str, zipfile.ZipInfo]]:
    members: dict[str, zipfile.ZipInfo] = {}
    roots: set[str] = set()
    for info in archive.infolist():
        path = _safe_relative(info.filename, label="coin archive")
        roots.add(path.parts[0])
        mode = (info.external_attr >> 16) & 0xFFFF
        if stat.S_ISLNK(mode):
            raise AssetPreparationError(f"Symlink in coin archive: {info.filename!r}")
        if not info.is_dir():
            normalized = path.as_posix()
            if normalized in members:
                raise AssetPreparationError(f"Duplicate path in coin archive: {normalized}")
            members[normalized] = info
    if len(roots) != 1:
        raise AssetPreparationError("Coin archive must contain one top-level directory")
    return next(iter(roots)), members


def _read_member(
    archive: zipfile.ZipFile,
    info: zipfile.ZipInfo,
    *,
    total: list[int],
) -> bytes:
    if info.file_size <= 0:
        raise AssetPreparationError(f"Empty coin archive member: {info.filename}")
    total[0] += info.file_size
    if total[0] > MAX_MATERIALIZED_BYTES:
        raise AssetPreparationError("Configured coin assets exceed safety limit")
    content = archive.read(info)
    if len(content) != info.file_size:
        raise AssetPreparationError(f"Truncated coin archive member: {info.filename}")
    return content


def _materialize(
    package_root: Path,
    archive_path: Path,
    lock: dict[str, str],
    mapped_files: dict[str, str],
    mapped_folders: dict[str, str],
) -> tuple[int, int]:
    manifest: dict[str, str] = {}
    total = [0]
    try:
        archive = zipfile.ZipFile(archive_path)
    except zipfile.BadZipFile as exc:
        raise AssetPreparationError("Coin archive is not a valid ZIP") from exc
    with archive:
        root, members = _validated_members(archive)
        for destination, source in mapped_files.items():
            destination_relative = _safe_relative(destination, label="mapped file").as_posix()
            source_relative = _safe_relative(source, label="archive source").as_posix()
            member_name = f"{root}/{source_relative}"
            info = members.get(member_name)
            if info is None:
                raise AssetPreparationError(f"Mapped coin source is missing: {source}")
            content = _read_member(archive, info, total=total)
            if destination_relative.endswith(".json"):
                try:
                    json.loads(content.decode("utf-8"))
                except (UnicodeDecodeError, json.JSONDecodeError) as exc:
                    raise AssetPreparationError(f"Mapped coin JSON is invalid: {source}") from exc
            _write_bytes_atomically(_destination(package_root, destination_relative), content)
            manifest[destination_relative] = hashlib.sha256(content).hexdigest()

        folder_files = 0
        for destination, source in mapped_folders.items():
            destination_root = _destination(package_root, destination)
            source_relative = _safe_relative(source, label="archive folder").as_posix()
            prefix = f"{root}/{source_relative}/"
            selected = {
                name[len(prefix) :]: info
                for name, info in members.items()
                if name.startswith(prefix) and name != prefix
            }
            if not selected:
                raise AssetPreparationError(f"Mapped coin folder is empty: {source}")
            expected: set[Path] = set()
            for relative, info in sorted(selected.items()):
                relative_path = _safe_relative(relative, label="archive folder member")
                destination_path = destination_root.joinpath(*relative_path.parts)
                content = _read_member(archive, info, total=total)
                _write_bytes_atomically(destination_path, content)
                manifest_path = PurePosixPath(
                    destination.rstrip("/"),
                    *relative_path.parts,
                ).as_posix()
                manifest[manifest_path] = hashlib.sha256(content).hexdigest()
                expected.add(destination_path.resolve())
                folder_files += 1

            if destination_root.is_dir():
                for existing in destination_root.rglob("*"):
                    if (
                        existing.is_file()
                        and existing.name != ".gitkeep"
                        and existing.resolve() not in expected
                    ):
                        existing.unlink()
                for existing in sorted(destination_root.rglob("*"), reverse=True):
                    if existing.is_dir() and not any(existing.iterdir()):
                        existing.rmdir()

    _write_json_atomically(
        package_root / STAMP_RELATIVE_PATH,
        {
            "version": STAMP_VERSION,
            "repository": lock["repository"],
            "commit": lock["commit"],
            "archive_sha256": lock["archive_sha256"],
            "files": manifest,
        },
    )
    return len(mapped_files), folder_files


def prepare(
    package_config_path: Path,
    lock_path: Path,
    *,
    archive_override: Path | None = None,
) -> tuple[Path, int, int, bool]:
    package_root = resolve_package_root(package_config_path)
    mapped_files, mapped_folders, configured_commit = _config_maps(package_root)
    lock = _load_lock(lock_path)
    if configured_commit != lock["commit"]:
        raise AssetPreparationError(
            "SDK bundled_coins_repo_commit does not match the checked-in asset lock",
        )

    cached = _validate_stamp(package_root, lock, mapped_files, mapped_folders)
    if cached is not None:
        return package_root, cached[0], cached[1], True

    with tempfile.TemporaryDirectory(prefix="pulw-komodo-assets-") as temporary:
        archive_path = Path(temporary) / "coins.zip"
        if archive_override is None:
            print(
                f"[prefetch-komodo-assets] Downloading pinned {lock['repository']} "
                f"snapshot {lock['commit']}...",
                flush=True,
            )
            _download(lock["archive_url"], archive_path)
        else:
            shutil.copyfile(archive_override, archive_path)
        actual_hash = _sha256(archive_path)
        if actual_hash != lock["archive_sha256"]:
            raise AssetPreparationError(
                f"Coin archive checksum mismatch: got {actual_hash}, "
                f"expected {lock['archive_sha256']}",
            )
        mapped_count, folder_count = _materialize(
            package_root,
            archive_path,
            lock,
            mapped_files,
            mapped_folders,
        )

    validated = _validate_stamp(package_root, lock, mapped_files, mapped_folders)
    if validated != (mapped_count, folder_count):
        raise AssetPreparationError("Materialized coin asset manifest did not validate")
    return package_root, mapped_count, folder_count, False


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(
        description="Materialize checksum-pinned Komodo coin assets.",
    )
    parser.add_argument(
        "--package-config",
        type=Path,
        default=Path("app/.dart_tool/package_config.json"),
    )
    parser.add_argument(
        "--asset-lock",
        type=Path,
        default=Path(__file__).with_name("komodo-coin-assets.lock.json"),
    )
    args = parser.parse_args(argv)
    try:
        package_root, mapped_files, folder_files, cached = prepare(
            args.package_config,
            args.asset_lock,
        )
    except AssetPreparationError as exc:
        print(f"[prefetch-komodo-assets][ERROR] {exc}", file=sys.stderr)
        return 1
    action = "validated cached" if cached else "materialized"
    print(
        f"[prefetch-komodo-assets] {action} assets in {package_root} "
        f"({mapped_files} mapped files, {folder_files} folder files)",
    )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
