#!/usr/bin/env bash
# Verify KMDCL/KDF runtime artifacts are present in built app outputs.
set -euo pipefail

platform="${1:-}"
target="${2:-app}"

if [ -z "$platform" ]; then
  echo "Usage: $0 <windows|linux|macos|android|ios> [build-dir-or-artifact]" >&2
  exit 2
fi

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
if [[ "$target" != /* ]]; then
  target="$root/$target"
fi

contains_kdf_in_zip() {
  local archive="$1"
  unzip -l "$archive" | grep -Ei '(^|/)(kdf(\.exe)?|kdf\.dll|libkdf\.(so|dylib)|komodo_defi_framework[^/]*|kdf_resources\.bundle)(/|$)' >/dev/null
}

contains_android_kdf_in_zip() {
  local archive="$1"
  unzip -l "$archive" | grep -Ei '[[:space:]](base/)?lib/(arm64-v8a|armeabi-v7a)/(libkomodo_defi_framework\.so|libkdf\.so|libkdflib(_static)?\.so)$' >/dev/null
}

find_kdf_file() {
  local search_dir="$1"
  [ -d "$search_dir" ] || return 1
  find "$search_dir" \
    \( -type f -o -type l \) \
    \( \
      -name 'kdf' -o \
      -name 'kdf.exe' -o \
      -name 'kdf.dll' -o \
      -name 'libkdf.a' -o \
      -name 'libmm2.a' -o \
      -name 'libkdf.so' -o \
      -name 'libkdflib.so' -o \
      -name 'libkdflib_static.so' -o \
      -name 'libkomodo_defi_framework.so' -o \
      -name 'libkdf.dylib' \
    \) \
    -print -quit | grep -q .
}

verify_zip_artifact() {
  local platform="$1"
  local artifact="$2"
  if [ "$platform" = "android" ] && contains_android_kdf_in_zip "$artifact"; then
    echo "KDF artifact found in $artifact"
    exit 0
  fi
  if [ "$platform" = "android" ]; then
    echo "Missing KDF artifact in $artifact" >&2
    exit 1
  fi
  if contains_kdf_in_zip "$artifact"; then
    echo "KDF artifact found in $artifact"
    exit 0
  fi
  echo "Missing KDF artifact in $artifact" >&2
  exit 1
}

package_root() {
  local package_name="$1"
  local app_dir="$2"
  local package_config="$app_dir/.dart_tool/package_config.json"
  [ -f "$package_config" ] || return 1
  python3 - "$package_config" "$package_name" <<'PY'
import json
import pathlib
import sys
import urllib.parse

config_path = pathlib.Path(sys.argv[1]).resolve()
package_name = sys.argv[2]
data = json.loads(config_path.read_text(encoding="utf-8"))
for package in data.get("packages", []):
    if package.get("name") != package_name:
        continue
    root_uri = package.get("rootUri", "")
    if root_uri.startswith("file://"):
        print(urllib.parse.unquote(urllib.parse.urlparse(root_uri).path))
    else:
        print((config_path.parent / root_uri).resolve())
    sys.exit(0)
sys.exit(1)
PY
}

if [ -f "$target" ]; then
  case "$target" in
    *.apk|*.aab|*.ipa|*.zip)
      verify_zip_artifact "$platform" "$target"
      ;;
    *)
      echo "Unsupported KDF artifact verification target: $target" >&2
      exit 2
      ;;
  esac
fi

case "$platform" in
  windows)
    candidates=(
      "$target/build/windows/x64/runner/Release"
      "$target/build/windows/runner/Release"
      "$target"
    )
    ;;
  linux)
    candidates=(
      "$target/build/linux/x64/release/bundle/lib"
      "$target/build/linux/x64/release/bundle"
      "$target"
    )
    ;;
  macos)
    candidates=(
      "$target/build/macos/Build/Products/Release/komodo_defi_framework/kdf_resources.bundle/Contents/Resources"
      "$target/build/macos/Build/Products/Release"
      "$target"
    )
    ;;
  android)
    komodo_root="$(package_root "komodo_defi_framework" "$target" 2>/dev/null || true)"
    candidates=(
      "$target/build/app"
      "$target/android/app/build"
      "$target"
    )
    if [ -n "$komodo_root" ]; then
      candidates=(
        "$komodo_root/android/app/src/main/cpp/libs/arm64-v8a"
        "$komodo_root/android/app/src/main/cpp/libs/armeabi-v7a"
        "${candidates[@]}"
      )
    fi
    ;;
  ios)
    candidates=(
      "$target/build/ios"
      "$target/ios"
      "$target"
    )
    ;;
  *)
    echo "Unsupported platform for KDF verification: $platform" >&2
    exit 2
    ;;
esac

for candidate in "${candidates[@]}"; do
  if find_kdf_file "$candidate"; then
    echo "KDF artifact found under $candidate"
    exit 0
  fi
done

echo "Missing KDF artifact for $platform under $target" >&2
echo "Checked:" >&2
printf '  %s\n' "${candidates[@]}" >&2
exit 1
