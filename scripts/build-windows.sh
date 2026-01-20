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
export TZ=UTC
export FLUTTER_SUPPRESS_ANALYTICS=true
export DART_SUPPRESS_ANALYTICS=true
export CARGO_INCREMENTAL=0

log "Building Windows MSIX (reproducible)"
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

log "Fetching Tor/I2P assets..."
if command -v powershell.exe &> /dev/null; then
    powershell.exe -NoProfile -ExecutionPolicy Bypass -File "$SCRIPT_DIR/fetch-tor-i2p-assets.ps1"
elif command -v pwsh &> /dev/null; then
    pwsh -NoProfile -ExecutionPolicy Bypass -File "$SCRIPT_DIR/fetch-tor-i2p-assets.ps1"
else
    error "PowerShell not found. Run scripts/fetch-tor-i2p-assets.ps1 manually."
fi

cd "$APP_DIR"

# Clean previous builds
log "Cleaning previous builds..."
flutter clean

# Get dependencies
log "Fetching dependencies..."
flutter pub get --enforce-lockfile

# Build Windows app
log "Building Windows app..."
flutter build windows --release

# Check if build succeeded
if [ ! -d "build/windows/runner/Release" ]; then
    error "Build failed: Release directory not found"
fi

# Create MSIX package (optional)
log "Creating MSIX package..."

MSIX_FILE=""
if grep -q "msix_config" "$APP_DIR/pubspec.yaml"; then
    if command -v dart &> /dev/null; then
        if ! dart pub global list | grep -q "msix"; then
            if [ -n "${MSIX_VERSION:-}" ]; then
                log "Installing msix $MSIX_VERSION..."
                dart pub global activate msix "$MSIX_VERSION"
            else
                warn "msix tool not installed. Set MSIX_VERSION to install a pinned version."
            fi
        fi
        if dart pub global list | grep -q "msix"; then
            dart pub global run msix:create
            MSIX_FILE="$(find build/windows/runner/Release -maxdepth 1 -name "*.msix" -print -quit)"
            if [ -z "$MSIX_FILE" ]; then
                warn "MSIX creation did not produce an output. Skipping."
            fi
        fi
    else
        warn "dart not found on PATH. Skipping MSIX packaging."
    fi
else
    warn "msix_config not found in pubspec.yaml. Skipping MSIX packaging."
fi

# Sign if certificate is available
SIGN_CERT="${WINDOWS_SIGN_CERT:-}"
SIGN_PASSWORD="${WINDOWS_SIGN_PASSWORD:-}"

if [ -n "$MSIX_FILE" ] && [ -f "$MSIX_FILE" ]; then
    if [ "$REPRODUCIBLE" = "1" ]; then
        warn "REPRODUCIBLE=1: skipping MSIX signing."
    elif [ -n "$SIGN_CERT" ] && [ -f "$SIGN_CERT" ]; then
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
else
    warn "MSIX not created. Skipping signing."
fi

# Create output directory
OUTPUT_DIR="$PROJECT_ROOT/dist/windows"
mkdir -p "$OUTPUT_DIR"

OUTPUT_NAME="pirate-unified-wallet-windows.msix"

# Copy artifacts
log "Copying artifacts..."
if [ -n "$MSIX_FILE" ] && [ -f "$MSIX_FILE" ]; then
    cp "$MSIX_FILE" "$OUTPUT_DIR/$OUTPUT_NAME"
else
    warn "MSIX not created. Skipping MSIX artifact copy."
fi

# Also copy portable version
log "Creating portable version..."
cd build/windows/runner/Release
zip_dir_deterministic "." "$PROJECT_ROOT/$OUTPUT_DIR/pirate-unified-wallet-windows-portable.zip"

# Generate SHA-256 checksums
log "Generating checksums..."
cd "$OUTPUT_DIR"
if [ -f "$OUTPUT_NAME" ]; then
    sha256sum "$OUTPUT_NAME" > "$OUTPUT_NAME.sha256"
fi
sha256sum "pirate-unified-wallet-windows-portable.zip" > "pirate-unified-wallet-windows-portable.zip.sha256"

log "Build complete!"
if [ -f "$OUTPUT_NAME" ]; then
    log "MSIX: $OUTPUT_DIR/$OUTPUT_NAME"
fi
log "Portable: $OUTPUT_DIR/pirate-unified-wallet-windows-portable.zip"
