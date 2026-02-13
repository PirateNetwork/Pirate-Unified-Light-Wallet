#!/usr/bin/env bash
# macOS DMG build, signing, and notarization script.
#
# Goals:
# - Build a universal2 Flutter app (Apple Silicon + Intel).
# - Build a universal Rust FFI dylib via lipo.
# - Sign in the correct order (nested code first, app last) to avoid
#   DYLD library validation failures like:
#     "mapping process and mapped file (non-platform) have different Team IDs"
#
# Notes:
# - This script must be run on macOS.
# - For production distribution, you want Developer ID signing + notarization.

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
APP_DIR="$PROJECT_ROOT/app"

GREEN='\033[0;32m'
RED='\033[0;31m'
YELLOW='\033[1;33m'
NC='\033[0m'

log() {
  echo -e "${GREEN}[$(date +'%Y-%m-%d %H:%M:%S')]${NC} $1"
}

warn() {
  echo -e "${YELLOW}[WARN]${NC} $1"
}

autofail() {
  echo -e "${RED}[ERROR]${NC} $1" >&2
  exit 1
}

read_pubspec_version() {
  local pubspec="$APP_DIR/pubspec.yaml"
  local raw
  raw="$(sed -nE 's/^version:[[:space:]]*([0-9]+\.[0-9]+\.[0-9]+)\+([0-9]+).*/\1+\2/p' "$pubspec" | head -n1)"
  if [[ -z "$raw" ]]; then
    autofail "Unable to parse app version from $pubspec"
  fi
  APP_VERSION_SEMVER="${raw%%+*}"
  APP_VERSION_BUILD="${raw##*+}"
  APP_VERSION_FULL="$APP_VERSION_SEMVER+$APP_VERSION_BUILD"
}

# Check if running on macOS
if [[ "$OSTYPE" != "darwin"* ]]; then
  autofail "macOS builds require macOS"
fi

# Reproducible build settings
export SOURCE_DATE_EPOCH="${SOURCE_DATE_EPOCH:-$(git log -1 --format=%ct 2>/dev/null || date +%s)}"
export TZ=UTC
export FLUTTER_SUPPRESS_ANALYTICS=true
export DART_SUPPRESS_ANALYTICS=true
export CARGO_INCREMENTAL=0

REPRODUCIBLE="${REPRODUCIBLE:-0}"

log "Building macOS DMG"
log "SOURCE_DATE_EPOCH: $SOURCE_DATE_EPOCH"

normalize_mtime() {
  local target="$1"
  if [ -z "${SOURCE_DATE_EPOCH:-}" ]; then
    return 0
  fi
  local stamp
  stamp="$(date -u -d "@$SOURCE_DATE_EPOCH" +"%Y%m%d%H%M.%S" 2>/dev/null || date -u -r "$SOURCE_DATE_EPOCH" +"%Y%m%d%H%M.%S")"
  find "$target" -exec touch -t "$stamp" {} + 2>/dev/null || true
}

log "Fetching Tor/I2P assets..."
chmod +x "$SCRIPT_DIR/fetch-tor-i2p-assets.sh"
"$SCRIPT_DIR/fetch-tor-i2p-assets.sh"

stage_rust_macos_universal() {
  local app_path="$1"

  log "Building Rust FFI library (universal2)..."

  if command -v rustup >/dev/null 2>&1; then
    rustup target add aarch64-apple-darwin x86_64-apple-darwin >/dev/null
  fi

  local crate_dir="$PROJECT_ROOT/crates"

  (cd "$crate_dir" && cargo build --release --target aarch64-apple-darwin --package pirate-ffi-frb --features frb --no-default-features --locked)
  (cd "$crate_dir" && cargo build --release --target x86_64-apple-darwin --package pirate-ffi-frb --features frb --no-default-features --locked)

  local dylib_arm="$crate_dir/target/aarch64-apple-darwin/release/libpirate_ffi_frb.dylib"
  local dylib_x86="$crate_dir/target/x86_64-apple-darwin/release/libpirate_ffi_frb.dylib"
  [ -f "$dylib_arm" ] || autofail "Rust library not found: $dylib_arm"
  [ -f "$dylib_x86" ] || autofail "Rust library not found: $dylib_x86"

  local dest_dir="$app_path/Contents/Frameworks"
  mkdir -p "$dest_dir"

  # flutter_rust_bridge's default loader for iOS/macOS expects a framework at
  # "$stem.framework/$stem" (see flutter_rust_bridge's loadExternalLibraryRaw).
  # Therefore, we bundle the Rust dynamic library as a .framework in the app.
  #
  # Use a "flat" framework layout (no Versions/ symlinks) because it's the most
  # compatible format for embedded frameworks in app bundles.
  local fw_name="pirate_ffi_frb"
  local fw_dir="$dest_dir/${fw_name}.framework"
  rm -rf "$fw_dir"
  mkdir -p "$fw_dir"

  local out="$fw_dir/$fw_name"
  lipo -create -output "$out" "$dylib_arm" "$dylib_x86"
  chmod +x "$out" || true

  if command -v install_name_tool >/dev/null 2>&1; then
    # Ensure the install name is @rpath so the app can load it from Contents/Frameworks.
    install_name_tool -id "@rpath/${fw_name}.framework/${fw_name}" "$out" || warn "install_name_tool failed on $out"
  fi

  if command -v strip >/dev/null 2>&1; then
    strip -x "$out" || warn "Failed to strip $out"
  fi

  # Minimal Info.plist so codesign / Gatekeeper treat this as a framework bundle.
  cat > "$fw_dir/Info.plist" <<EOF
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>CFBundleDevelopmentRegion</key>
  <string>en</string>
  <key>CFBundleExecutable</key>
  <string>${fw_name}</string>
  <key>CFBundleIdentifier</key>
  <string>com.pirate.wallet.${fw_name}</string>
  <key>CFBundleInfoDictionaryVersion</key>
  <string>6.0</string>
  <key>CFBundleName</key>
  <string>${fw_name}</string>
  <key>CFBundlePackageType</key>
  <string>FMWK</string>
  <key>CFBundleShortVersionString</key>
  <string>${APP_VERSION_SEMVER}</string>
  <key>CFBundleVersion</key>
  <string>${APP_VERSION_BUILD}</string>
</dict>
</plist>
EOF
}

require_universal_macho() {
  local path="$1"
  if ! command -v lipo >/dev/null 2>&1; then
    warn "lipo not found; skipping universal check for $path"
    return 0
  fi
  if [ ! -f "$path" ]; then
    autofail "Expected file not found: $path"
  fi
  local archs
  archs="$(lipo -archs "$path" 2>/dev/null || true)"
  if [[ "$archs" != *x86_64* || "$archs" != *arm64* ]]; then
    autofail "Expected universal2 Mach-O at $path (arm64 + x86_64). Got: ${archs:-unknown}"
  fi
}

verify_universal_app_bundle() {
  local app_path="$1"
  local exe_name
  exe_name="$(basename "$app_path" .app)"

  require_universal_macho "$app_path/Contents/MacOS/$exe_name"
  require_universal_macho "$app_path/Contents/Frameworks/pirate_ffi_frb.framework/pirate_ffi_frb"

  local frameworks_dir="$app_path/Contents/Frameworks"
  if [ -d "$frameworks_dir" ]; then
    local fw
    for fw in "$frameworks_dir"/*.framework; do
      [ -d "$fw" ] || continue
      local name
      name="$(basename "$fw" .framework)"
      local bin="$fw/$name"
      if [ -f "$bin" ]; then
        require_universal_macho "$bin"
      fi
    done
  fi
}

# Sign nested code (frameworks, dylibs, helpers) before signing the app.
# Do NOT use --deep signing on the app bundle; it's a common cause of broken
# signatures when embedded frameworks are already signed differently.
sign_nested_code() {
  local app_path="$1"
  local identity="$2"

  local frameworks_dir="$app_path/Contents/Frameworks"
  local plugins_dir="$app_path/Contents/PlugIns"

  if [ -d "$frameworks_dir" ]; then
    # Sign dylibs first
    while IFS= read -r -d '' f; do
      codesign --force --sign "$identity" --timestamp "$f"
    done < <(find "$frameworks_dir" -type f -name "*.dylib" -print0 | LC_ALL=C sort -z)

    # Sign frameworks
    while IFS= read -r -d '' f; do
      codesign --force --sign "$identity" --timestamp "$f"
    done < <(find "$frameworks_dir" -type d -name "*.framework" -print0 | LC_ALL=C sort -z)

    # Sign any helper apps in Frameworks
    while IFS= read -r -d '' f; do
      codesign --force --sign "$identity" --timestamp "$f"
    done < <(find "$frameworks_dir" -type d -name "*.app" -print0 | LC_ALL=C sort -z)
  fi

  if [ -d "$plugins_dir" ]; then
    while IFS= read -r -d '' f; do
      codesign --force --sign "$identity" --timestamp "$f"
    done < <(find "$plugins_dir" -type d \( -name "*.appex" -o -name "*.plugin" -o -name "*.xpc" \) -print0 | LC_ALL=C sort -z)
  fi

  local resources_dir="$app_path/Contents/Resources"
  if [ -d "$resources_dir" ]; then
    # These are shipped as standalone executables and must be signed too under hardened runtime.
    local extra_dir
    for extra_dir in "$resources_dir/tor-pt" "$resources_dir/i2p"; do
      if [ -d "$extra_dir" ]; then
        while IFS= read -r -d '' f; do
          codesign --force --sign "$identity" --timestamp "$f"
        done < <(find "$extra_dir" -type f -perm -111 -print0 | LC_ALL=C sort -z)
      fi
    done
  fi
}

sign_nested_code_no_timestamp() {
  local app_path="$1"
  local identity="$2"

  local frameworks_dir="$app_path/Contents/Frameworks"
  local plugins_dir="$app_path/Contents/PlugIns"

  if [ -d "$frameworks_dir" ]; then
    while IFS= read -r -d '' f; do
      codesign --force --sign "$identity" "$f"
    done < <(find "$frameworks_dir" -type f -name "*.dylib" -print0 | LC_ALL=C sort -z)

    while IFS= read -r -d '' f; do
      codesign --force --sign "$identity" "$f"
    done < <(find "$frameworks_dir" -type d -name "*.framework" -print0 | LC_ALL=C sort -z)

    while IFS= read -r -d '' f; do
      codesign --force --sign "$identity" "$f"
    done < <(find "$frameworks_dir" -type d -name "*.app" -print0 | LC_ALL=C sort -z)
  fi

  if [ -d "$plugins_dir" ]; then
    while IFS= read -r -d '' f; do
      codesign --force --sign "$identity" "$f"
    done < <(find "$plugins_dir" -type d \( -name "*.appex" -o -name "*.plugin" -o -name "*.xpc" \) -print0 | LC_ALL=C sort -z)
  fi

  local resources_dir="$app_path/Contents/Resources"
  if [ -d "$resources_dir" ]; then
    local extra_dir
    for extra_dir in "$resources_dir/tor-pt" "$resources_dir/i2p"; do
      if [ -d "$extra_dir" ]; then
        while IFS= read -r -d '' f; do
          codesign --force --sign "$identity" "$f"
        done < <(find "$extra_dir" -type f -perm -111 -print0 | LC_ALL=C sort -z)
      fi
    done
  fi
}

adhoc_sign_app_bundle() {
  local app_path="$1"
  local identity="-"

  log "Ad-hoc signing nested code..."
  sign_nested_code_no_timestamp "$app_path" "$identity"

  log "Ad-hoc signing app bundle..."
  codesign --force --sign "$identity" "$app_path"

  log "Verifying signature..."
  codesign --verify --deep --strict --verbose=4 "$app_path"
}

sign_app_bundle() {
  local app_path="$1"
  local identity="$2"
  local entitlements_path="$3"

  [ -f "$entitlements_path" ] || autofail "Entitlements file not found: $entitlements_path"

  log "Signing nested code..."
  sign_nested_code "$app_path" "$identity"

  log "Signing app bundle..."
  codesign --force --sign "$identity" --timestamp \
    --options runtime \
    --entitlements "$entitlements_path" \
    "$app_path"

  log "Verifying signature..."
  codesign --verify --deep --strict --verbose=4 "$app_path"
}

cd "$APP_DIR"

# On tag builds, align app version metadata with the git tag (vX.Y.Z).
bash "$SCRIPT_DIR/sync-version-from-tag.sh"
read_pubspec_version
log "App version: $APP_VERSION_FULL"

log "Cleaning previous builds..."
flutter clean

log "Fetching dependencies..."
flutter pub get --enforce-lockfile

log "Building macOS app (universal2)..."
# On modern Flutter versions, `flutter build macos` produces a universal build by
# default. We validate the result below via lipo checks.
flutter build macos --release

APP_OUTPUT_DIR="build/macos/Build/Products/Release"
APP_PATH="$APP_OUTPUT_DIR/Pirate Unified Wallet.app"

if [ ! -d "$APP_PATH" ]; then
  APP_PATH="$(find "$APP_OUTPUT_DIR" -maxdepth 1 -type d -name "*.app" | LC_ALL=C sort | head -n 1)"
fi

if [ -z "$APP_PATH" ] || [ ! -d "$APP_PATH" ]; then
  autofail "Build failed: App not found in $APP_OUTPUT_DIR"
fi

stage_rust_macos_universal "$APP_PATH"

log "Verifying universal app bundle..."
verify_universal_app_bundle "$APP_PATH"

# Decide signing behavior
SIGN_ARG="${1:-auto}"
if [ "$REPRODUCIBLE" = "1" ]; then
  SIGN_ARG=false
fi

SIGN=false
case "$SIGN_ARG" in
  true|false)
    SIGN="$SIGN_ARG"
    ;;
  auto)
    if security find-identity -v -p codesigning | grep -q "Developer ID Application"; then
      SIGN=true
    else
      SIGN=false
      warn "No Developer ID Application signing identity found."
    fi
    ;;
  *)
    autofail "Invalid signing argument: $SIGN_ARG (expected: auto|true|false)"
    ;;
esac

SIGNED=false
SIGN_IDENTITY="${MACOS_SIGN_IDENTITY:-Developer ID Application}"
ENTITLEMENTS_PATH="${MACOS_ENTITLEMENTS_PATH:-$APP_DIR/macos/Runner/Distribution.entitlements}"

if [ "$SIGN" = "true" ]; then
  log "Code signing macOS app..."
  sign_app_bundle "$APP_PATH" "$SIGN_IDENTITY" "$ENTITLEMENTS_PATH"
  SIGNED=true
else
  # We modify the app bundle after the Flutter/Xcode build (e.g. adding the Rust
  # framework), which can invalidate the build-time ad-hoc signature. Re-sign
  # ad-hoc so the app can launch and load embedded frameworks consistently.
  log "Re-signing macOS app ad-hoc..."
  adhoc_sign_app_bundle "$APP_PATH"
fi

# Create DMG
log "Creating DMG..."

DMG_NAME="Pirate Unified Wallet"
OUTPUT_NAME="pirate-unified-wallet-macos"
if [ "$SIGNED" != "true" ]; then
  OUTPUT_NAME="${OUTPUT_NAME}-unsigned"
fi

DMG_FILE="$PROJECT_ROOT/dist/macos/${OUTPUT_NAME}.dmg"
mkdir -p "$PROJECT_ROOT/dist/macos"

TMP_DMG_DIR="$(mktemp -d)"
cp -R "$APP_PATH" "$TMP_DMG_DIR/"

if [ "$SIGNED" != "true" ]; then
  cat > "$TMP_DMG_DIR/README.txt" <<'EOF'
 Pirate Unified Wallet (test build)

 This build is not Developer ID code-signed or notarized yet. macOS may block it on first launch.

 How to run it:
 1) Drag "Pirate Unified Wallet.app" to /Applications
 2) Open Terminal and run:
   xattr -dr com.apple.quarantine "/Applications/Pirate Unified Wallet.app"
3) Then right-click the app and choose Open (first run).
   Alternatively: System Settings -> Privacy & Security -> Open Anyway.

Apple Silicon note: If you enable I2P and see "Bad CPU type in executable", install Rosetta:
   softwareupdate --install-rosetta --agree-to-license
If that fails:
   sudo softwareupdate --install-rosetta --agree-to-license
EOF
fi
normalize_mtime "$TMP_DMG_DIR"

ln -s /Applications "$TMP_DMG_DIR/Applications"

hdiutil create -volname "$DMG_NAME"   -srcfolder "$TMP_DMG_DIR"   -ov -format UDZO   "$DMG_FILE"

rm -rf "$TMP_DMG_DIR"

[ -f "$DMG_FILE" ] || autofail "DMG creation failed"

if [ "$SIGNED" = "true" ]; then
  log "Signing DMG..."
  codesign --force --sign "$SIGN_IDENTITY" --timestamp "$DMG_FILE"
fi

# Notarize if enabled
NOTARIZE="${MACOS_NOTARIZE:-false}"
if [ "$REPRODUCIBLE" = "1" ]; then
  NOTARIZE=false
fi

if [ "$NOTARIZE" = "true" ] && [ "$SIGNED" = "true" ]; then
  log "Notarizing DMG..."

  APPLE_ID="${MACOS_APPLE_ID:-}"
  TEAM_ID="${MACOS_TEAM_ID:-}"
  APP_PASSWORD="${MACOS_APP_PASSWORD:-}"

  if [ -z "$APPLE_ID" ] || [ -z "$TEAM_ID" ] || [ -z "$APP_PASSWORD" ]; then
    autofail "Notarization requested but credentials are missing. Set MACOS_APPLE_ID, MACOS_TEAM_ID, MACOS_APP_PASSWORD."
  fi

  xcrun notarytool submit "$DMG_FILE"     --apple-id "$APPLE_ID"     --team-id "$TEAM_ID"     --password "$APP_PASSWORD"     --wait

  xcrun stapler staple "$DMG_FILE"

  log "Notarization complete"
fi

log "Generating checksum..."
cd "$PROJECT_ROOT/dist/macos"
shasum -a 256 "${OUTPUT_NAME}.dmg" > "${OUTPUT_NAME}.dmg.sha256"

log "Build complete!"
log "DMG: $DMG_FILE"
log "SHA-256: $(cat "${OUTPUT_NAME}.dmg.sha256")"

if [ "$SIGNED" = "true" ]; then
  log "DMG is signed"
  if [ "$NOTARIZE" = "true" ]; then
    log "DMG is notarized and ready for distribution"
  fi
fi
