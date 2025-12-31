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
export FLUTTER_SUPPRESS_ANALYTICS=true
export DART_SUPPRESS_ANALYTICS=true

log "Building iOS IPA (reproducible)"
log "SOURCE_DATE_EPOCH: $SOURCE_DATE_EPOCH"

cd "$APP_DIR"

# Clean previous builds
log "Cleaning previous builds..."
flutter clean

# Get dependencies
log "Fetching dependencies..."
flutter pub get
cd ios && pod install && cd ..

# Build unsigned IPA first
log "Building iOS app..."
flutter build ios --release --no-codesign

# Check for signing configuration
SIGN="${1:-auto}"  # auto, true, or false

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
else
    # Create unsigned IPA
    log "Creating unsigned IPA..."
    
    cd build/ios/iphoneos
    mkdir -p Payload
    cp -r Runner.app Payload/
    zip -r Runner.ipa Payload
    
    IPA_FILE="Runner.ipa"
    cd "$APP_DIR"
fi

if [ ! -f "$IPA_FILE" ]; then
    error "Build failed: $IPA_FILE not found"
fi

# Create output directory
OUTPUT_DIR="$PROJECT_ROOT/dist/ios"
mkdir -p "$OUTPUT_DIR"

OUTPUT_NAME="pirate-unified-wallet-ios.ipa"

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

