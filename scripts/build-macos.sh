#!/usr/bin/env bash
# macOS universal DMG build, signing, and notarization script
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
    error "macOS builds require macOS"
fi

# Reproducible build settings
export SOURCE_DATE_EPOCH="${SOURCE_DATE_EPOCH:-$(git log -1 --format=%ct 2>/dev/null || date +%s)}"
export FLUTTER_SUPPRESS_ANALYTICS=true
export DART_SUPPRESS_ANALYTICS=true

log "Building macOS universal DMG (reproducible)"
log "SOURCE_DATE_EPOCH: $SOURCE_DATE_EPOCH"

cd "$APP_DIR"

# Clean previous builds
log "Cleaning previous builds..."
flutter clean

# Get dependencies
log "Fetching dependencies..."
flutter pub get

# Build macOS app
log "Building macOS app..."
flutter build macos --release

APP_PATH="build/macos/Build/Products/Release/Pirate Unified Wallet.app"

if [ ! -d "$APP_PATH" ]; then
    error "Build failed: App not found at $APP_PATH"
fi

# Sign the app if certificate is available
SIGN="${1:-auto}"
if [ "$SIGN" = "auto" ]; then
    if security find-identity -v -p codesigning | grep -q "Developer ID Application"; then
        SIGN=true
    else
        SIGN=false
        warn "No code signing identity found."
    fi
fi

if [ "$SIGN" = "true" ]; then
    log "Code signing macOS app..."
    
    SIGN_IDENTITY="${MACOS_SIGN_IDENTITY:-Developer ID Application}"
    
    # Sign the app
    codesign --force --deep --sign "$SIGN_IDENTITY" \
        --options runtime \
        --entitlements macos/Runner/Release.entitlements \
        "$APP_PATH"
    
    # Verify signature
    codesign --verify --verbose=4 "$APP_PATH"
    
    log "Code signing complete"
fi

# Create DMG
log "Creating DMG..."

DMG_NAME="Pirate Unified Wallet"
DMG_FILE="$PROJECT_ROOT/dist/macos/pirate-unified-wallet-macos.dmg"

mkdir -p "$PROJECT_ROOT/dist/macos"

# Create temporary directory for DMG contents
TMP_DMG_DIR=$(mktemp -d)
cp -R "$APP_PATH" "$TMP_DMG_DIR/"

# Create symbolic link to Applications
ln -s /Applications "$TMP_DMG_DIR/Applications"

# Create DMG
hdiutil create -volname "$DMG_NAME" \
    -srcfolder "$TMP_DMG_DIR" \
    -ov -format UDZO \
    "$DMG_FILE"

# Clean up
rm -rf "$TMP_DMG_DIR"

if [ ! -f "$DMG_FILE" ]; then
    error "DMG creation failed"
fi

# Sign DMG if requested
if [ "$SIGN" = "true" ]; then
    log "Signing DMG..."
    codesign --force --sign "$SIGN_IDENTITY" "$DMG_FILE"
fi

# Notarize if credentials are available
NOTARIZE="${MACOS_NOTARIZE:-false}"
if [ "$NOTARIZE" = "true" ] && [ "$SIGN" = "true" ]; then
    log "Notarizing DMG..."
    
    APPLE_ID="${MACOS_APPLE_ID:-}"
    TEAM_ID="${MACOS_TEAM_ID:-}"
    APP_PASSWORD="${MACOS_APP_PASSWORD:-}"
    
    if [ -z "$APPLE_ID" ] || [ -z "$TEAM_ID" ] || [ -z "$APP_PASSWORD" ]; then
        warn "Notarization credentials not set. Skipping notarization."
        warn "Set MACOS_APPLE_ID, MACOS_TEAM_ID, and MACOS_APP_PASSWORD to notarize."
    else
        # Upload for notarization
        xcrun notarytool submit "$DMG_FILE" \
            --apple-id "$APPLE_ID" \
            --team-id "$TEAM_ID" \
            --password "$APP_PASSWORD" \
            --wait
        
        # Staple the notarization ticket
        xcrun stapler staple "$DMG_FILE"
        
        log "Notarization complete"
    fi
fi

# Generate SHA-256 checksum
log "Generating checksum..."
cd "$PROJECT_ROOT/dist/macos"
shasum -a 256 "pirate-unified-wallet-macos.dmg" > "pirate-unified-wallet-macos.dmg.sha256"

log "Build complete!"
log "DMG: $DMG_FILE"
log "SHA-256: $(cat pirate-unified-wallet-macos.dmg.sha256)"

if [ "$SIGN" = "true" ]; then
    log "DMG is signed"
    if [ "$NOTARIZE" = "true" ]; then
        log "DMG is notarized and ready for distribution"
    fi
fi

