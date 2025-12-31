#!/usr/bin/env bash
# Android APK/AAB build and signing script
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
APP_DIR="$PROJECT_ROOT/app"

# Colors for output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
NC='\033[0m' # No Color

log() {
    echo -e "${GREEN}[$(date +'%Y-%m-%d %H:%M:%S')]${NC} $1"
}

error() {
    echo -e "${RED}[ERROR]${NC} $1" >&2
    exit 1
}

warn() {
    echo -e "${YELLOW}[WARN]${NC} $1"
}

# Parse arguments
BUILD_TYPE="${1:-apk}"  # apk or bundle
SIGN="${2:-false}"      # Sign the build

# Reproducible build settings
export SOURCE_DATE_EPOCH="${SOURCE_DATE_EPOCH:-$(git log -1 --format=%ct 2>/dev/null || date +%s)}"
export FLUTTER_SUPPRESS_ANALYTICS=true
export DART_SUPPRESS_ANALYTICS=true

log "Building Android $BUILD_TYPE (reproducible)"
log "SOURCE_DATE_EPOCH: $SOURCE_DATE_EPOCH"

cd "$APP_DIR"

# Clean previous builds
log "Cleaning previous builds..."
flutter clean

# Get dependencies
log "Fetching dependencies..."
flutter pub get

# Build based on type
if [ "$BUILD_TYPE" = "bundle" ]; then
    log "Building Android App Bundle..."
    flutter build appbundle --release
    
    OUTPUT_FILE="$APP_DIR/build/app/outputs/bundle/release/app-release.aab"
    OUTPUT_NAME="pirate-unified-wallet-android.aab"
else
    log "Building Android APK..."
    flutter build apk --release --split-per-abi
    
    # Multiple ABIs
    ARM64_APK="$APP_DIR/build/app/outputs/flutter-apk/app-arm64-v8a-release.apk"
    ARMV7_APK="$APP_DIR/build/app/outputs/flutter-apk/app-armeabi-v7a-release.apk"
    X86_64_APK="$APP_DIR/build/app/outputs/flutter-apk/app-x86_64-release.apk"
    
    OUTPUT_FILE="$ARM64_APK"
    OUTPUT_NAME="pirate-unified-wallet-android-arm64-v8a.apk"
fi

if [ ! -f "$OUTPUT_FILE" ]; then
    error "Build failed: $OUTPUT_FILE not found"
fi

# Sign if requested and keystore is available
if [ "$SIGN" = "true" ]; then
    log "Signing $BUILD_TYPE..."
    
    KEYSTORE_PATH="${ANDROID_KEYSTORE_PATH:-$HOME/.android/pirate-wallet-release.keystore}"
    KEYSTORE_PASSWORD="${ANDROID_KEYSTORE_PASSWORD:-}"
    KEY_ALIAS="${ANDROID_KEY_ALIAS:-pirate-wallet}"
    KEY_PASSWORD="${ANDROID_KEY_PASSWORD:-$KEYSTORE_PASSWORD}"
    
    if [ ! -f "$KEYSTORE_PATH" ]; then
        warn "Keystore not found at $KEYSTORE_PATH"
        warn "Skipping signing. Set ANDROID_KEYSTORE_PATH to sign."
    elif [ -z "$KEYSTORE_PASSWORD" ]; then
        warn "ANDROID_KEYSTORE_PASSWORD not set. Skipping signing."
    else
        if [ "$BUILD_TYPE" = "bundle" ]; then
            # AAB signing
            jarsigner -verbose \
                -sigalg SHA256withRSA \
                -digestalg SHA-256 \
                -keystore "$KEYSTORE_PATH" \
                -storepass "$KEYSTORE_PASSWORD" \
                -keypass "$KEY_PASSWORD" \
                "$OUTPUT_FILE" \
                "$KEY_ALIAS"
        else
            # APK signing with apksigner
            "$ANDROID_HOME/build-tools/34.0.0/apksigner" sign \
                --ks "$KEYSTORE_PATH" \
                --ks-key-alias "$KEY_ALIAS" \
                --ks-pass "pass:$KEYSTORE_PASSWORD" \
                --key-pass "pass:$KEY_PASSWORD" \
                "$OUTPUT_FILE"
        fi
        
        log "Signed successfully"
    fi
fi

# Create output directory
OUTPUT_DIR="$PROJECT_ROOT/dist/android"
mkdir -p "$OUTPUT_DIR"

# Copy artifacts
log "Copying artifacts..."
cp "$OUTPUT_FILE" "$OUTPUT_DIR/$OUTPUT_NAME"

# Generate SHA-256 checksum
log "Generating checksum..."
cd "$OUTPUT_DIR"
sha256sum "$OUTPUT_NAME" > "$OUTPUT_NAME.sha256"

log "Build complete!"
log "Output: $OUTPUT_DIR/$OUTPUT_NAME"
log "SHA-256: $(cat "$OUTPUT_NAME.sha256")"

