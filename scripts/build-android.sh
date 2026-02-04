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

abi_label() {
    case "$1" in
        arm64-v8a)
            echo "V8"
            ;;
        armeabi-v7a)
            echo "V7"
            ;;
        x86_64)
            echo "x86"
            ;;
        *)
            echo "$1"
            ;;
    esac
}

# Parse arguments
BUILD_TYPE="${1:-apk}"  # apk or bundle
SIGN="${2:-false}"      # Sign the build
REPRODUCIBLE="${REPRODUCIBLE:-0}"
ANDROID_SPLIT_PER_ABI="${ANDROID_SPLIT_PER_ABI:-1}"
ANDROID_GRADLE_STACKTRACE="${ANDROID_GRADLE_STACKTRACE:-1}"

# Reproducible build settings
export SOURCE_DATE_EPOCH="${SOURCE_DATE_EPOCH:-$(git log -1 --format=%ct 2>/dev/null || date +%s)}"
export TZ=UTC
export FLUTTER_SUPPRESS_ANALYTICS=true
export DART_SUPPRESS_ANALYTICS=true
export CARGO_INCREMENTAL=0

log "Building Android $BUILD_TYPE (reproducible)"
log "SOURCE_DATE_EPOCH: $SOURCE_DATE_EPOCH"

if [ "$REPRODUCIBLE" = "1" ]; then
    SIGN=false
fi

cd "$APP_DIR"

# Clean previous builds
log "Cleaning previous builds..."
flutter clean

# Get dependencies
log "Fetching dependencies..."
flutter pub get --enforce-lockfile

# Build Rust FFI libraries for Android
log "Building Rust Android libraries..."
chmod +x "$SCRIPT_DIR/build-rust-android.sh"
bash "$SCRIPT_DIR/build-rust-android.sh"

# Build based on type
if [ "$BUILD_TYPE" = "bundle" ]; then
    log "Building Android App Bundle..."
    flutter build appbundle --release
    
    OUTPUT_FILE="$APP_DIR/build/app/outputs/bundle/release/app-release.aab"
    OUTPUT_NAME_BASE="pirate-unified-wallet-android"
else
    log "Building Android APK..."
    APK_MODE="split"
    APK_FILES=()
    if [ "$ANDROID_SPLIT_PER_ABI" = "1" ]; then
        if ! flutter build apk --release --split-per-abi; then
            warn "Split APK build failed."
            if [ "$ANDROID_GRADLE_STACKTRACE" = "1" ]; then
                warn "Retrying split build with Gradle --stacktrace --info..."
                (cd "$APP_DIR/android" && ./gradlew assembleRelease -Psplit-per-abi=true --stacktrace --info)
            else
                error "Split APK build failed. Set ANDROID_GRADLE_STACKTRACE=1 for diagnostics."
            fi
        fi
    else
        APK_MODE="arm64"
        flutter build apk --release --target-platform=android-arm64
    fi
    
    if [ "$APK_MODE" = "split" ]; then
        # Multiple ABIs
        ABIS=("arm64-v8a" "armeabi-v7a" "x86_64")
        for abi in "${ABIS[@]}"; do
            signed="$APP_DIR/build/app/outputs/flutter-apk/app-${abi}-release.apk"
            unsigned="$APP_DIR/build/app/outputs/flutter-apk/app-${abi}-release-unsigned.apk"
            if [ -f "$signed" ]; then
                APK_FILES+=("$signed")
            elif [ -f "$unsigned" ]; then
                APK_FILES+=("$unsigned")
            else
                warn "APK for $abi not found."
            fi
        done
    else
        ARM64_APK="$APP_DIR/build/app/outputs/flutter-apk/app-release.apk"
        ARM64_APK_UNSIGNED="$APP_DIR/build/app/outputs/flutter-apk/app-release-unsigned.apk"
        if [ -f "$ARM64_APK" ]; then
            APK_FILES+=("$ARM64_APK")
        elif [ -f "$ARM64_APK_UNSIGNED" ]; then
            APK_FILES+=("$ARM64_APK_UNSIGNED")
        fi
    fi

    if [ "${#APK_FILES[@]}" -eq 0 ]; then
        error "Build failed: no APK outputs found"
    fi
    OUTPUT_FILE="${APK_FILES[0]}"
fi

if [ ! -f "$OUTPUT_FILE" ]; then
    error "Build failed: $OUTPUT_FILE not found"
fi

SIGNED=false

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
            BUILD_TOOLS_VERSION="${ANDROID_BUILD_TOOLS_VERSION:-34.0.0}"
            APKSIGNER_PATH="$ANDROID_HOME/build-tools/$BUILD_TOOLS_VERSION/apksigner"
            if [ ! -f "$APKSIGNER_PATH" ]; then
                error "apksigner not found at $APKSIGNER_PATH"
            fi
            "$APKSIGNER_PATH" sign \
                --ks "$KEYSTORE_PATH" \
                --ks-key-alias "$KEY_ALIAS" \
                --ks-pass "pass:$KEYSTORE_PASSWORD" \
                --key-pass "pass:$KEY_PASSWORD" \
                "$OUTPUT_FILE"
        fi
        
        SIGNED=true
        log "Signed successfully"
    fi
fi

# Create output directory
OUTPUT_DIR="$PROJECT_ROOT/dist/android"
mkdir -p "$OUTPUT_DIR"

# Copy artifacts
log "Copying artifacts..."
if [ "$BUILD_TYPE" = "apk" ]; then
    for apk in "${APK_FILES[@]}"; do
        filename="$(basename "$apk")"
        if [[ "$filename" == *"-release-unsigned.apk" ]]; then
            abi="${filename#app-}"
            abi="${abi%-release-unsigned.apk}"
        else
            abi="${filename#app-}"
            abi="${abi%-release.apk}"
        fi
        if [ "$APK_MODE" != "split" ]; then
            abi="arm64-v8a"
        fi
        abi_tag="$(abi_label "$abi")"
        if [ "$SIGNED" = "true" ]; then
            OUTPUT_NAME="pirate-unified-wallet-android-${abi_tag}.apk"
        else
            OUTPUT_NAME="pirate-unified-wallet-android-${abi_tag}-unsigned.apk"
        fi
        cp "$apk" "$OUTPUT_DIR/$OUTPUT_NAME"
        sha256sum "$OUTPUT_DIR/$OUTPUT_NAME" > "$OUTPUT_DIR/$OUTPUT_NAME.sha256"
    done
else
    if [ "$SIGNED" = "true" ]; then
        OUTPUT_NAME="pirate-unified-wallet-android.aab"
    else
        OUTPUT_NAME="pirate-unified-wallet-android-unsigned.aab"
    fi
    cp "$OUTPUT_FILE" "$OUTPUT_DIR/$OUTPUT_NAME"
    sha256sum "$OUTPUT_DIR/$OUTPUT_NAME" > "$OUTPUT_DIR/$OUTPUT_NAME.sha256"
fi

log "Build complete!"
log "Output directory: $OUTPUT_DIR"
