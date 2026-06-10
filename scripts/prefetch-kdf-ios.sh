#!/usr/bin/env bash
# Fetch the KMDCL/KDF iOS static library before CocoaPods/Xcode link the plugin.
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
APP_DIR="$PROJECT_ROOT/app"

cd "$APP_DIR"

tmp_dir="$(mktemp -d)"
cleanup() {
    rm -rf "$tmp_dir"
}
trap cleanup EXIT

input_file="$tmp_dir/transformer-input.txt"
output_file="$tmp_dir/transformer-output.txt"
printf 'prefetch kdf ios\n' > "$input_file"

if [ -z "${GITHUB_API_PUBLIC_READONLY_TOKEN:-}" ] && [ -n "${GITHUB_TOKEN:-}" ]; then
    export GITHUB_API_PUBLIC_READONLY_TOKEN="$GITHUB_TOKEN"
fi

TARGET_DEVICE_PLATFORM_NAME=iphoneos \
SWIFT_PLATFORM_TARGET_PREFIX=ios \
OVERRIDE_DEFI_API_DOWNLOAD=true \
dart run komodo_wallet_build_transformer \
    --input="$input_file" \
    --output="$output_file" \
    --fetch_defi_api \
    --artifact_output_package=komodo_defi_framework \
    --config_output_path=app_build/build_config.json

kdf_ios_lib="$(python3 - <<'PY'
import json
import pathlib
from urllib.parse import unquote, urlparse

config = json.loads(pathlib.Path(".dart_tool/package_config.json").read_text(encoding="utf-8"))
for package in config["packages"]:
    if package["name"] != "komodo_defi_framework":
        continue
    uri = package["rootUri"]
    if uri.startswith("file://"):
        raw_path = unquote(urlparse(uri).path)
        if len(raw_path) > 3 and raw_path[0] == "/" and raw_path[2] == ":":
            raw_path = f"/mnt/{raw_path[1].lower()}/{raw_path[4:]}"
        path = pathlib.Path(raw_path)
    else:
        path = pathlib.Path(uri)
    print(path / "ios" / "libkdf.a")
    break
PY
)"

if [ -z "$kdf_ios_lib" ] || [ ! -f "$kdf_ios_lib" ]; then
    echo "KDF iOS library was not fetched: ${kdf_ios_lib:-unknown path}" >&2
    exit 1
fi

echo "KDF iOS library ready at $kdf_ios_lib"
