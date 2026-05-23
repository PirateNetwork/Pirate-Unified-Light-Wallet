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
  unzip -l "$archive" | grep -Eiq '(^|/)(kdf(\.exe)?|kdf\.dll|libkdf\.(so|dylib)|komodo_defi_framework[^/]*|kdf_resources\.bundle)(/|$)'
}

find_kdf_file() {
  local search_dir="$1"
  [ -d "$search_dir" ] || return 1
  find "$search_dir" \
    \( -type f -o -type l \) \
    \( -name 'kdf' -o -name 'kdf.exe' -o -name 'kdf.dll' -o -name 'libkdf.so' -o -name 'libkdf.dylib' \) \
    -print -quit | grep -q .
}

verify_zip_artifact() {
  local artifact="$1"
  if contains_kdf_in_zip "$artifact"; then
    echo "KDF artifact found in $artifact"
    exit 0
  fi
  echo "Missing KDF artifact in $artifact" >&2
  exit 1
}

if [ -f "$target" ]; then
  case "$target" in
    *.apk|*.aab|*.ipa|*.zip)
      verify_zip_artifact "$target"
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
    candidates=(
      "$target/build/app"
      "$target/android/app/build"
      "$target"
    )
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
