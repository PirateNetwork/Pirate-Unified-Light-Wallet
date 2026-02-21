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
MOUNT_POINT="$MOUNT_DIR"
MOUNT_DEVICE=""
cleanup() {
  # Best-effort detach; ignore failures to avoid masking root cause.
  if [[ -n "${MOUNT_DEVICE:-}" ]]; then
    hdiutil detach "$MOUNT_DEVICE" -force -quiet >/dev/null 2>&1 || true
  fi
  if [[ -n "${MOUNT_POINT:-}" ]]; then
    hdiutil detach "$MOUNT_POINT" -force -quiet >/dev/null 2>&1 || true
  fi
  hdiutil detach "$MOUNT_DIR" -force -quiet >/dev/null 2>&1 || true
  rm -rf "$MOUNT_DIR" || true
}
trap cleanup EXIT

echo "[verify-macos-dmg] Mounting DMG: $DMG_PATH"

if ! hdiutil imageinfo "$DMG_PATH" >/dev/null 2>&1; then
  echo "DMG imageinfo check failed: $DMG_PATH" >&2
  hdiutil imageinfo "$DMG_PATH" >&2 || true
  exit 1
fi

attach_with_retries() {
  local max_attempts=4
  local attempt=1
  local delay=2
  local out=""
  local rc=0

  while (( attempt <= max_attempts )); do
    echo "[verify-macos-dmg] Attach attempt $attempt/$max_attempts (explicit mountpoint)..."
    set +e
    out="$(hdiutil attach -nobrowse -readonly -mountpoint "$MOUNT_DIR" "$DMG_PATH" 2>&1)"
    rc=$?
    set -e
    if (( rc == 0 )); then
      echo "$out"
      MOUNT_POINT="$MOUNT_DIR"
      MOUNT_DEVICE="$(printf '%s\n' "$out" | awk '/^\/dev\// {print $1; exit}')"
      return 0
    fi

    echo "[verify-macos-dmg] Explicit mountpoint attach failed (attempt $attempt):" >&2
    echo "$out" >&2

    echo "[verify-macos-dmg] Retrying without explicit mountpoint..." >&2
    set +e
    out="$(hdiutil attach -nobrowse -readonly "$DMG_PATH" 2>&1)"
    rc=$?
    set -e
    if (( rc == 0 )); then
      echo "$out"
      MOUNT_DEVICE="$(printf '%s\n' "$out" | awk '/^\/dev\// {print $1; exit}')"
      MOUNT_POINT="$(printf '%s\n' "$out" | awk '/^\/dev\// {mp=$NF} END{print mp}')"
      if [[ -n "$MOUNT_POINT" && -d "$MOUNT_POINT" ]]; then
        return 0
      fi
      echo "[verify-macos-dmg] Fallback attach succeeded but mountpoint parse failed." >&2
      echo "$out" >&2
    else
      echo "[verify-macos-dmg] Fallback attach failed (attempt $attempt):" >&2
      echo "$out" >&2
    fi

    if (( attempt < max_attempts )); then
      sleep "$delay"
    fi
    attempt=$((attempt + 1))
  done

  return 1
}

if ! attach_with_retries; then
  echo "[verify-macos-dmg] Failed to mount DMG after retries: $DMG_PATH" >&2
  exit 1
fi

APP_PATH="$(find "$MOUNT_POINT" -maxdepth 2 -type d -name "*.app" | LC_ALL=C sort | head -n 1 || true)"
if [[ -z "$APP_PATH" || ! -d "$APP_PATH" ]]; then
  echo "No .app found in DMG mount: $MOUNT_POINT" >&2
  find "$MOUNT_POINT" -maxdepth 3 -print >&2 || true
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
