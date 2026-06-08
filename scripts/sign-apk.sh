#!/usr/bin/env bash
# Helper script to sign an unsigned Android APK for local development.
# Requirements: apksigner and zipalign (from Android Build Tools)
set -euo pipefail

GREEN='\033[0;32m'
RED='\033[0;31m'
YELLOW='\033[1;33m'
NC='\033[0m'

log() {
    echo -e "${GREEN}[INFO]${NC} $1"
}

warn() {
    echo -e "${YELLOW}[WARN]${NC} $1"
}

error() {
    echo -e "${RED}[ERROR]${NC} $1" >&2
    exit 1
}

# Try to find apksigner and zipalign if not in PATH
find_tool() {
    local tool_name="$1"
    if command -v "$tool_name" >/dev/null 2>&1; then
        command -v "$tool_name"
        return 0
    fi

    # Check common Android SDK locations
    local sdk="${ANDROID_HOME:-${ANDROID_SDK_ROOT:-$HOME/Android/Sdk}}"
    if [ -d "$sdk/build-tools" ]; then
        # Find the latest build-tools version
        local latest_bt
        latest_bt=$(ls -1 "$sdk/build-tools" | sort -V | tail -n 1)
        if [ -n "$latest_bt" ] && [ -f "$sdk/build-tools/$latest_bt/$tool_name" ]; then
            echo "$sdk/build-tools/$latest_bt/$tool_name"
            return 0
        fi
    fi
    return 1
}

APKSIGNER=$(find_tool "apksigner" || true)
ZIPALIGN=$(find_tool "zipalign" || true)

if [ -z "$APKSIGNER" ]; then
    warn "apksigner not found in PATH or Android SDK."
fi
if [ -z "$ZIPALIGN" ]; then
    warn "zipalign not found in PATH or Android SDK."
fi

if [ "$#" -lt 2 ]; then
    echo "Usage: $0 <unsigned-apk> <keystore> [alias]"
    echo ""
    echo "Example:"
    echo "  $0 pirate-unified-wallet-android-V8-unsigned.apk my-release-key.keystore"
    exit 1
fi

INPUT_APK="$1"
KEYSTORE="$2"
ALIAS="${3:-}"

if [ ! -f "$INPUT_APK" ]; then
    error "Input APK not found: $INPUT_APK"
fi
if [ ! -f "$KEYSTORE" ]; then
    error "Keystore not found: $KEYSTORE"
fi

# Determine output name
BASENAME=$(basename "$INPUT_APK" "-unsigned.apk")
BASENAME=$(basename "$BASENAME" ".apk")
OUTPUT_APK="${BASENAME}-signed.apk"
ALIGNED_APK="${BASENAME}-aligned.apk"

# 1. Zip Align (if zipalign is available)
if [ -n "$ZIPALIGN" ]; then
    log "Aligning APK..."
    "$ZIPALIGN" -f -p 4 "$INPUT_APK" "$ALIGNED_APK"
    SIGN_TARGET="$ALIGNED_APK"
else
    warn "Skipping alignment because zipalign was not found."
    SIGN_TARGET="$INPUT_APK"
fi

# 2. Sign with apksigner (if available)
if [ -n "$APKSIGNER" ]; then
    log "Signing with apksigner..."

    SIGN_ARGS=("--ks" "$KEYSTORE")
    if [ -n "$ALIAS" ]; then
        SIGN_ARGS+=("--ks-key-alias" "$ALIAS")
    fi

    # We use a temporary file for signing to avoid overwriting if alignment didn't happen
    cp "$SIGN_TARGET" "$OUTPUT_APK"
    "$APKSIGNER" sign "${SIGN_ARGS[@]}" "$OUTPUT_APK"

    log "Verifying signature..."
    "$APKSIGNER" verify "$OUTPUT_APK"

    log "Success! Signed APK is at: $OUTPUT_APK"

    # Cleanup alignment temporary file
    if [ -f "$ALIGNED_APK" ] && [ "$ALIGNED_APK" != "$OUTPUT_APK" ]; then
        rm "$ALIGNED_APK"
    fi
else
    error "apksigner is required for proper V2/V3 signing. jarsigner is NOT sufficient for modern Android."
fi
