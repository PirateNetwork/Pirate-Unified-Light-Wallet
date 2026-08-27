#!/usr/bin/env bash
set -euo pipefail

APP_PATH="${1:-}"
if [[ -z "$APP_PATH" || ! -d "$APP_PATH" ]]; then
  echo "Usage: $0 path/to/application.app" >&2
  exit 2
fi
if ! command -v codesign >/dev/null 2>&1; then
  echo "codesign not found (this script must run on macOS)" >&2
  exit 2
fi
if [[ ! -x /usr/libexec/PlistBuddy ]]; then
  echo "PlistBuddy not found (this script must run on macOS)" >&2
  exit 2
fi

ENTITLEMENTS_DUMP="$(mktemp)"
cleanup() {
  rm -f "$ENTITLEMENTS_DUMP"
}
trap cleanup EXIT

if ! codesign --display --entitlements - "$APP_PATH" \
  >"$ENTITLEMENTS_DUMP" 2>/dev/null; then
  echo "Could not read signed entitlements from: $APP_PATH" >&2
  exit 1
fi

if ! /usr/libexec/PlistBuddy \
  -c 'Print :keychain-access-groups' \
  "$ENTITLEMENTS_DUMP" >/dev/null 2>&1; then
  echo "Signed app is missing the keychain-access-groups entitlement: $APP_PATH" >&2
  exit 1
fi

echo "[verify-macos-entitlements] Keychain entitlement present: $APP_PATH"
