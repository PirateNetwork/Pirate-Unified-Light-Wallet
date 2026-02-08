#!/usr/bin/env bash
set -euo pipefail

DMG_PATH="${1:-}"
if [[ -z "$DMG_PATH" ]]; then
  echo "Usage: $0 path/to/app.dmg" >&2
  exit 2
fi
if [[ ! -f "$DMG_PATH" ]]; then
  echo "DMG not found: $DMG_PATH" >&2
  exit 2
fi

if ! command -v hdiutil >/dev/null 2>&1; then
  echo "hdiutil not found (this script must run on macOS)" >&2
  exit 2
fi
if ! command -v codesign >/dev/null 2>&1; then
  echo "codesign not found (this script must run on macOS)" >&2
  exit 2
fi
if ! command -v lipo >/dev/null 2>&1; then
  echo "lipo not found (this script must run on macOS)" >&2
  exit 2
fi

MOUNT_DIR="$(mktemp -d)"
cleanup() {
  # Best-effort detach; ignore failures to avoid masking root cause.
  hdiutil detach "$MOUNT_DIR" -quiet >/dev/null 2>&1 || true
  rm -rf "$MOUNT_DIR" || true
}
trap cleanup EXIT

echo "[verify-macos-dmg] Mounting DMG: $DMG_PATH"
hdiutil attach -nobrowse -readonly -mountpoint "$MOUNT_DIR" "$DMG_PATH" -quiet

APP_PATH="$(find "$MOUNT_DIR" -maxdepth 2 -type d -name "*.app" | LC_ALL=C sort | head -n 1 || true)"
if [[ -z "$APP_PATH" || ! -d "$APP_PATH" ]]; then
  echo "No .app found in DMG mount: $MOUNT_DIR" >&2
  find "$MOUNT_DIR" -maxdepth 3 -print >&2 || true
  exit 1
fi

APP_NAME="$(basename "$APP_PATH" .app)"
MAIN_EXE="$APP_PATH/Contents/MacOS/$APP_NAME"
if [[ ! -f "$MAIN_EXE" ]]; then
  echo "Main executable not found: $MAIN_EXE" >&2
  exit 1
fi

require_universal() {
  local path="$1"
  if [[ ! -f "$path" ]]; then
    echo "Missing expected file: $path" >&2
    exit 1
  fi
  local archs
  archs="$(lipo -archs "$path" 2>/dev/null || true)"
  if [[ "$archs" != *x86_64* || "$archs" != *arm64* ]]; then
    echo "Expected universal2 Mach-O at $path (arm64 + x86_64). Got: ${archs:-unknown}" >&2
    exit 1
  fi
}

echo "[verify-macos-dmg] Checking universal2 binaries..."
require_universal "$MAIN_EXE"

# FRB loader expects this framework by default.
require_universal "$APP_PATH/Contents/Frameworks/pirate_ffi_frb.framework/pirate_ffi_frb"

FRAMEWORKS_DIR="$APP_PATH/Contents/Frameworks"
if [[ -d "$FRAMEWORKS_DIR" ]]; then
  for fw in "$FRAMEWORKS_DIR"/*.framework; do
    [[ -d "$fw" ]] || continue
    name="$(basename "$fw" .framework)"
    bin="$fw/$name"
    [[ -f "$bin" ]] || continue
    require_universal "$bin"
  done
fi

echo "[verify-macos-dmg] Verifying code signature..."
codesign --verify --deep --strict --verbose=4 "$APP_PATH" >/dev/null

echo "[verify-macos-dmg] OK: $APP_PATH"

