#!/usr/bin/env bash
# iOS IPA build and signing script (TestFlight ready)
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

error() {
    echo -e "${RED}[ERROR]${NC} $1" >&2
    exit 1
}

warn() {
    echo -e "${YELLOW}[WARN]${NC} $1"
}

# Check if running on macOS
if [[ "$OSTYPE" != "darwin"* ]]; then
    error "iOS builds require macOS"
fi

# Reproducible build settings
export SOURCE_DATE_EPOCH="${SOURCE_DATE_EPOCH:-$(git log -1 --format=%ct 2>/dev/null || date +%s)}"
export TZ=UTC
export FLUTTER_SUPPRESS_ANALYTICS=true
export DART_SUPPRESS_ANALYTICS=true
export CARGO_INCREMENTAL=0

log "Building iOS IPA (reproducible)"
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

REPRODUCIBLE="${REPRODUCIBLE:-0}"

zip_dir_deterministic() {
    local src="$1"
    local dest="$2"
    (cd "$src" && normalize_mtime "." && LC_ALL=C find . -type f -print | sort | zip -X -@ "$dest")
}

cd "$APP_DIR"

# Clean previous builds
log "Cleaning previous builds..."
flutter clean

# Get dependencies
log "Fetching dependencies..."
flutter pub get --enforce-lockfile
cd ios && pod install --deployment && cd ..

# Build unsigned IPA first
log "Building iOS app..."
flutter build ios --release --no-codesign

# Check for signing configuration
SIGN="${1:-auto}"  # auto, true, or false
SIGNED=false
if [ "$REPRODUCIBLE" = "1" ]; then
    SIGN=false
fi

if [ "$SIGN" = "auto" ]; then
    # Check if we have signing certificates
    if security find-identity -v -p codesigning | grep -q "iPhone Distribution"; then
        SIGN=true
    else
        SIGN=false
        warn "No code signing identity found. Building unsigned IPA."
    fi
fi

if [ "$SIGN" = "true" ]; then
    log "Code signing iOS app..."
    
    # Export IPA with signing
    xcodebuild -workspace ios/Runner.xcworkspace \
        -scheme Runner \
        -sdk iphoneos \
        -configuration Release \
        archive -archivePath build/ios/Runner.xcarchive
    
    xcodebuild -exportArchive \
        -archivePath build/ios/Runner.xcarchive \
        -exportOptionsPlist ios/ExportOptions.plist \
        -exportPath build/ios/ipa
    
    IPA_FILE="build/ios/ipa/Runner.ipa"
    SIGNED=true
else
    # Create unsigned IPA
    log "Creating unsigned IPA..."
    
    cd build/ios/iphoneos
    mkdir -p Payload
    cp -r Runner.app Payload/
    zip_dir_deterministic "Payload" "Runner.ipa"
    IPA_FILE="$APP_DIR/build/ios/iphoneos/Runner.ipa"
    cd "$APP_DIR"
fi

if [ ! -f "$IPA_FILE" ]; then
    error "Build failed: $IPA_FILE not found"
fi

# Create output directory
OUTPUT_DIR="$PROJECT_ROOT/dist/ios"
mkdir -p "$OUTPUT_DIR"

OUTPUT_NAME="pirate-unified-wallet-ios"
if [ "$SIGNED" != "true" ]; then
    OUTPUT_NAME="${OUTPUT_NAME}-unsigned"
fi
OUTPUT_NAME="${OUTPUT_NAME}.ipa"

# Copy artifacts
log "Copying artifacts..."
cp "$IPA_FILE" "$OUTPUT_DIR/$OUTPUT_NAME"

# Generate SHA-256 checksum
log "Generating checksum..."
cd "$OUTPUT_DIR"
shasum -a 256 "$OUTPUT_NAME" > "$OUTPUT_NAME.sha256"

log "Build complete!"
log "Output: $OUTPUT_DIR/$OUTPUT_NAME"
log "SHA-256: $(cat "$OUTPUT_NAME.sha256")"

if [ "$SIGN" = "true" ]; then
    log "IPA is signed and ready for TestFlight upload"
else
    warn "IPA is unsigned. Sign before submitting to App Store."
fi
