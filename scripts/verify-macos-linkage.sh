#!/usr/bin/env bash
set -euo pipefail

APP_PATH="${1:-}"
if [[ -z "$APP_PATH" ]]; then
  echo "Usage: $0 path/to/application.app" >&2
  exit 2
fi
if [[ ! -d "$APP_PATH/Contents" ]]; then
  echo "Invalid macOS app bundle: $APP_PATH" >&2
  exit 2
fi

for tool in file otool; do
  if ! command -v "$tool" >/dev/null 2>&1; then
    echo "$tool not found (this script must run on macOS)" >&2
    exit 2
  fi
done

mach_o_count=0
invalid_count=0

while IFS= read -r -d '' candidate; do
  if ! file -b "$candidate" | grep -q 'Mach-O'; then
    continue
  fi

  mach_o_count=$((mach_o_count + 1))
  if ! linkage="$(otool -L "$candidate")"; then
    echo "Unable to inspect Mach-O dependencies: $candidate" >&2
    invalid_count=$((invalid_count + 1))
    continue
  fi

  while IFS= read -r dependency; do
    [[ -n "$dependency" ]] || continue
    case "$dependency" in
      @rpath/*|@loader_path/*|@executable_path/*|/usr/lib/*|/System/Library/*|/System/iOSSupport/*)
        ;;
      *)
        echo "Non-portable dependency: $candidate -> $dependency" >&2
        invalid_count=$((invalid_count + 1))
        ;;
    esac
  done < <(
    printf '%s\n' "$linkage" |
      awk '/^[[:space:]]/ {
        sub(/^[[:space:]]+/, "")
        sub(/[[:space:]]+\(compatibility version.*$/, "")
        print
      }'
  )
done < <(find "$APP_PATH/Contents" -type f -print0)

if (( mach_o_count == 0 )); then
  echo "No Mach-O binaries found in app bundle: $APP_PATH" >&2
  exit 1
fi
if (( invalid_count != 0 )); then
  echo "Found $invalid_count Mach-O dependency issue(s)." >&2
  exit 1
fi

echo "[verify-macos-linkage] OK: checked $mach_o_count Mach-O binaries"
