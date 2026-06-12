#!/usr/bin/env bash
# Fetch one KMDCL/KDF platform artifact from the SDK build config.
set -euo pipefail

if [ "$#" -ne 1 ]; then
    echo "Usage: $0 <platform>" >&2
    echo "Example: $0 linux" >&2
    echo "Use '$0 native' to prefetch all native KDF artifacts." >&2
    exit 64
fi

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
APP_DIR="$PROJECT_ROOT/app"
PLATFORM="$1"

if [ "$PLATFORM" = "native" ] || [ "$PLATFORM" = "all" ]; then
    for native_platform in ios macos windows linux android-aarch64 android-armv7; do
        bash "$0" "$native_platform"
    done
    exit 0
fi

cd "$APP_DIR"

PYTHON_BIN="${PYTHON:-}"
if [ -z "$PYTHON_BIN" ]; then
    if command -v python3 >/dev/null 2>&1; then
        PYTHON_BIN="python3"
    elif command -v python >/dev/null 2>&1; then
        PYTHON_BIN="python"
    else
        echo "python3 or python is required to prefetch KDF artifacts" >&2
        exit 127
    fi
fi

if [ -z "${GITHUB_API_PUBLIC_READONLY_TOKEN:-}" ] && [ -n "${GITHUB_TOKEN:-}" ]; then
    export GITHUB_API_PUBLIC_READONLY_TOKEN="$GITHUB_TOKEN"
fi

"$PYTHON_BIN" - "$PLATFORM" <<'PY'
import hashlib
import html.parser
import json
import os
import pathlib
import re
import shutil
import sys
import tempfile
import time
import urllib.parse
import urllib.request
import zipfile

platform = sys.argv[1]


def package_root(package_name):
    config = json.loads(pathlib.Path(".dart_tool/package_config.json").read_text(encoding="utf-8"))
    for package in config["packages"]:
        if package["name"] != package_name:
            continue
        uri = package["rootUri"]
        if uri.startswith("file://"):
            raw_path = urllib.parse.unquote(urllib.parse.urlparse(uri).path)
            if len(raw_path) > 3 and raw_path[0] == "/" and raw_path[2] == ":":
                if os.name == "nt":
                    raw_path = raw_path[1:]
                else:
                    raw_path = f"/mnt/{raw_path[1].lower()}/{raw_path[4:]}"
            return pathlib.Path(raw_path)
        return pathlib.Path(uri)
    raise SystemExit(f"Package {package_name} was not found in package_config.json")


def expected_artifacts(root, platform_name):
    return {
        "linux": [
            root / "linux" / "bin" / "kdf",
            root / "linux" / "bin" / "mm2",
        ],
        "windows": [
            root / "windows" / "bin" / "kdf.exe",
            root / "windows" / "bin" / "mm2.exe",
        ],
        "macos": [
            root / "macos" / "bin" / "kdf",
            root / "macos" / "bin" / "mm2",
        ],
        "ios": [
            root / "ios" / "libkdf.a",
            root / "ios" / "libmm2.a",
        ],
        "android-armv7": [
            root / "android" / "app" / "src" / "main" / "cpp" / "libs" / "armeabi-v7a" / "libkdf.a",
            root / "android" / "app" / "src" / "main" / "cpp" / "libs" / "armeabi-v7a" / "libmm2.a",
        ],
        "android-aarch64": [
            root / "android" / "app" / "src" / "main" / "cpp" / "libs" / "arm64-v8a" / "libkdf.a",
            root / "android" / "app" / "src" / "main" / "cpp" / "libs" / "arm64-v8a" / "libmm2.a",
        ],
    }.get(platform_name, [])


def fallback_artifact_names(platform_name):
    return {
        "linux": ["kdf", "mm2"],
        "windows": ["kdf.exe", "mm2.exe"],
        "macos": ["kdf", "mm2"],
        "ios": ["libkdf.a", "libmm2.a"],
        "android-armv7": ["libkdf.a", "libmm2.a"],
        "android-aarch64": ["libkdf.a", "libmm2.a"],
    }.get(platform_name, [])


def chmod_if_executable(path):
    if path.name in {"kdf", "mm2", "kdf.exe", "mm2.exe"}:
        path.chmod(path.stat().st_mode | 0o111)


def normalize_extracted_artifacts(destination, artifacts, platform_name):
    if not artifacts:
        return

    candidates = [artifact for artifact in artifacts if artifact.exists()]
    for name in fallback_artifact_names(platform_name):
        candidates.extend(path for path in sorted(destination.rglob(name)) if path.is_file())

    if not candidates:
        return

    source = candidates[0]
    for artifact in artifacts:
        artifact.parent.mkdir(parents=True, exist_ok=True)
        if not artifact.exists():
            shutil.copy2(source, artifact)
        chmod_if_executable(artifact)


def missing_artifacts(artifacts):
    return [artifact for artifact in artifacts if not artifact.exists()]


class LinkParser(html.parser.HTMLParser):
    def __init__(self):
        super().__init__()
        self.links = []

    def handle_starttag(self, tag, attrs):
        if tag != "a":
            return
        for key, value in attrs:
            if key == "href" and value:
                self.links.append(value)


def http_get(url):
    headers = {"User-Agent": "pirate-wallet-ci"}
    token = os.environ.get("GITHUB_API_PUBLIC_READONLY_TOKEN")
    if token and urllib.parse.urlparse(url).netloc == "api.github.com":
        headers["Authorization"] = f"Bearer {token}"
    request = urllib.request.Request(url, headers=headers)
    with urllib.request.urlopen(request, timeout=60) as response:
        return response.read()


def download_file(url, path):
    headers = {"User-Agent": "pirate-wallet-ci"}
    token = os.environ.get("GITHUB_API_PUBLIC_READONLY_TOKEN")
    if token and urllib.parse.urlparse(url).netloc == "api.github.com":
        headers["Authorization"] = f"Bearer {token}"
    request = urllib.request.Request(url, headers=headers)
    with urllib.request.urlopen(request, timeout=60) as response, path.open("wb") as output:
        shutil.copyfileobj(response, output)


def choose_preferred(names, preferences):
    if not preferences:
        return sorted(names)[0]
    for preference in preferences:
        preferred = [name for name in names if preference in name]
        if preferred:
            return sorted(preferred)[0]
    return sorted(names)[0]


def find_in_github(api_url, pattern, preferences, full_hash, short_hash):
    releases_url = api_url.rstrip("/") + "/releases"
    releases = json.loads(http_get(releases_url).decode("utf-8"))
    candidates: dict[str, str] = {}
    for release in releases:
        for asset in release.get("assets", []):
            name = asset.get("name") or pathlib.PurePosixPath(
                urllib.parse.urlparse(asset.get("browser_download_url", "")).path,
            ).name
            if not name or not pattern.match(name):
                continue
            if full_hash not in name and short_hash not in name:
                continue
            url = asset.get("browser_download_url")
            if url:
                candidates[name] = url
    if not candidates:
        return None
    return candidates[choose_preferred(list(candidates), preferences)]


def find_in_directory_listing(source_url, branch, pattern, preferences, full_hash, short_hash):
    base = source_url if source_url.endswith("/") else f"{source_url}/"
    listing_urls = []
    if branch:
        listing_urls.append(urllib.parse.urljoin(base, f"{branch}/"))
    listing_urls.append(base)

    for listing_url in dict.fromkeys(listing_urls):
        try:
            parser = LinkParser()
            parser.feed(http_get(listing_url).decode("utf-8", errors="replace"))
        except Exception:
            continue

        candidates: dict[str, str] = {}
        for href in parser.links:
            href_path = urllib.parse.urlparse(href).path or href
            name = pathlib.PurePosixPath(href_path).name
            if "wallet" in name or not name.endswith(".zip"):
                continue
            if not pattern.match(name):
                continue
            if full_hash not in href_path and short_hash not in href_path:
                continue
            candidates[name] = urllib.parse.urljoin(listing_url, href)
        if candidates:
            return candidates[choose_preferred(list(candidates), preferences)]
    return None


root = package_root("komodo_defi_framework")
config_path = root / "app_build" / "build_config.json"
config = json.loads(config_path.read_text(encoding="utf-8"))
api = config["api"]
platforms = api["platforms"]
if platform not in platforms:
    raise SystemExit(f"KDF platform {platform!r} is not configured in {config_path}")

artifacts = expected_artifacts(root, platform)
if artifacts:
    normalize_extracted_artifacts(artifacts[0].parent, artifacts, platform)
if artifacts and not missing_artifacts(artifacts):
    print(f"KDF {platform} artifacts already present: {', '.join(str(artifact) for artifact in artifacts)}")
    sys.exit(0)

platform_config = platforms[platform]
destination = root / platform_config["path"]
destination.mkdir(parents=True, exist_ok=True)
pattern = re.compile(platform_config["matching_pattern"])
preferences = list(platform_config.get("matching_preference", []))
valid_checksums = set(platform_config["valid_zip_sha256_checksums"])
full_hash = api["api_commit_hash"]
short_hash = full_hash[:7]
download_url = None
source_urls = []

# The KMDCL SDK config points at the KMDCL repo first. If that repo has no
# release artifacts yet, use our fork-owned GitHub release before trying any
# optional sources supplied by CI.
for configured_source in api["source_urls"]:
    source_urls.append(configured_source)
    if "api.github.com/repos/kmdclassic/komodo-defi-framework" in configured_source.lower():
        source_urls.append("https://api.github.com/repos/OswaldKardingson/komodo-defi-framework")
extra_sources = os.environ.get("KDF_EXTRA_SOURCE_URLS", "")
source_urls.extend(source.strip() for source in extra_sources.split(",") if source.strip())

for source_url in dict.fromkeys(source_urls):
    try:
        if urllib.parse.urlparse(source_url).netloc == "api.github.com":
            download_url = find_in_github(source_url, pattern, preferences, full_hash, short_hash)
        else:
            download_url = find_in_directory_listing(source_url, api.get("branch", ""), pattern, preferences, full_hash, short_hash)
    except Exception as exc:
        print(f"Failed to query {source_url}: {exc}", file=sys.stderr)
        download_url = None
    if download_url:
        break

if not download_url:
    raise SystemExit(f"No KDF {platform} zip found for {full_hash}")

print(f"Downloading KDF {platform} artifact from {download_url}", flush=True)
with tempfile.TemporaryDirectory() as tmp:
    zip_path = pathlib.Path(tmp) / pathlib.PurePosixPath(urllib.parse.urlparse(download_url).path).name
    download_file(download_url, zip_path)
    digest = hashlib.sha256(zip_path.read_bytes()).hexdigest()
    if digest not in valid_checksums:
        raise SystemExit(f"KDF {platform} checksum mismatch: got {digest}, expected one of {sorted(valid_checksums)}")
    with zipfile.ZipFile(zip_path) as archive:
        archive.extractall(destination)

normalize_extracted_artifacts(destination, artifacts, platform)

last_updated = destination / f".api_last_updated_{platform}"
last_updated.write_text(
    json.dumps(
        {
            "api_commit_hash": full_hash,
            "timestamp": time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime()),
            "checksums": sorted(valid_checksums),
        },
        separators=(",", ":"),
    ),
    encoding="utf-8",
)

missing = missing_artifacts(artifacts)
if missing:
    found = sorted(path for path in destination.rglob("*") if path.is_file())
    raise SystemExit(f"KDF {platform} artifacts did not appear at {missing}; extracted files: {found}")

if artifacts:
    print(f"KDF {platform} artifacts ready: {', '.join(str(artifact) for artifact in artifacts)}")
else:
    print(f"KDF {platform} artifact extracted to {destination}")
PY
