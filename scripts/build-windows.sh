#!/usr/bin/env bash
# Windows MSIX build and signing script
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

# Reproducible build settings
export SOURCE_DATE_EPOCH="${SOURCE_DATE_EPOCH:-$(git log -1 --format=%ct 2>/dev/null || date +%s)}"
export FLUTTER_SUPPRESS_ANALYTICS=true
export DART_SUPPRESS_ANALYTICS=true

log "Building Windows MSIX (reproducible)"
log "SOURCE_DATE_EPOCH: $SOURCE_DATE_EPOCH"

cd "$APP_DIR"

# Clean previous builds
log "Cleaning previous builds..."
flutter clean

# Get dependencies
log "Fetching dependencies..."
flutter pub get

# Build Windows app
log "Building Windows app..."
flutter build windows --release

# Check if build succeeded
if [ ! -d "build/windows/runner/Release" ]; then
    error "Build failed: Release directory not found"
fi

# Create MSIX package
log "Creating MSIX package..."

# Install flutter_distributor if not available
if ! flutter pub global list | grep -q flutter_distributor; then
    log "Installing flutter_distributor..."
    flutter pub global activate flutter_distributor
fi

# Package as MSIX
flutter pub run msix:create

MSIX_FILE="build/windows/runner/Release/pirate_unified_wallet.msix"

if [ ! -f "$MSIX_FILE" ]; then
    error "MSIX creation failed: $MSIX_FILE not found"
fi

# Sign if certificate is available
SIGN_CERT="${WINDOWS_SIGN_CERT:-}"
SIGN_PASSWORD="${WINDOWS_SIGN_PASSWORD:-}"

if [ -n "$SIGN_CERT" ] && [ -f "$SIGN_CERT" ]; then
    log "Signing MSIX..."
    
    if command -v signtool &> /dev/null; then
        signtool sign /f "$SIGN_CERT" /p "$SIGN_PASSWORD" /fd SHA256 "$MSIX_FILE"
        log "Signed successfully"
    else
        warn "signtool not found. Skipping signing."
        warn "Install Windows SDK to enable signing."
    fi
else
    warn "No signing certificate configured."
    warn "Set WINDOWS_SIGN_CERT and WINDOWS_SIGN_PASSWORD to sign."
fi

# Create output directory
OUTPUT_DIR="$PROJECT_ROOT/dist/windows"
mkdir -p "$OUTPUT_DIR"

OUTPUT_NAME="pirate-unified-wallet-windows.msix"

# Copy artifacts
log "Copying artifacts..."
cp "$MSIX_FILE" "$OUTPUT_DIR/$OUTPUT_NAME"

# Also copy portable version
log "Creating portable version..."
cd build/windows/runner/Release
zip -r "$PROJECT_ROOT/$OUTPUT_DIR/pirate-unified-wallet-windows-portable.zip" .

# Generate SHA-256 checksums
log "Generating checksums..."
cd "$OUTPUT_DIR"
sha256sum "$OUTPUT_NAME" > "$OUTPUT_NAME.sha256"
sha256sum "pirate-unified-wallet-windows-portable.zip" > "pirate-unified-wallet-windows-portable.zip.sha256"

log "Build complete!"
log "MSIX: $OUTPUT_DIR/$OUTPUT_NAME"
log "Portable: $OUTPUT_DIR/pirate-unified-wallet-windows-portable.zip"

