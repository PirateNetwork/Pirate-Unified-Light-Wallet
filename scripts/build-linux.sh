#!/usr/bin/env bash
# Linux AppImage, Flatpak, and Debian package build script
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

# Parse arguments
FORMAT="${1:-appimage}"  # appimage, flatpak, or deb

# Reproducible build settings
export SOURCE_DATE_EPOCH="${SOURCE_DATE_EPOCH:-$(git log -1 --format=%ct 2>/dev/null || date +%s)}"
export FLUTTER_SUPPRESS_ANALYTICS=true
export DART_SUPPRESS_ANALYTICS=true

log "Building Linux $FORMAT (reproducible)"
log "SOURCE_DATE_EPOCH: $SOURCE_DATE_EPOCH"

cd "$APP_DIR"

# Clean previous builds
log "Cleaning previous builds..."
flutter clean

# Get dependencies
log "Fetching dependencies..."
flutter pub get

# Build Linux app
log "Building Linux app..."
flutter build linux --release

BUNDLE_DIR="build/linux/x64/release/bundle"

if [ ! -d "$BUNDLE_DIR" ]; then
    error "Build failed: Bundle directory not found"
fi

OUTPUT_DIR="$PROJECT_ROOT/dist/linux"
mkdir -p "$OUTPUT_DIR"

case "$FORMAT" in
    appimage)
        build_appimage
        ;;
    flatpak)
        build_flatpak
        ;;
    deb)
        build_deb
        ;;
    *)
        error "Unknown format: $FORMAT"
        ;;
esac

build_appimage() {
    log "Creating AppImage..."
    
    # Install appimagetool if not available
    if ! command -v appimagetool &> /dev/null; then
        warn "appimagetool not found. Downloading..."
        wget -q https://github.com/AppImage/AppImageKit/releases/download/continuous/appimagetool-x86_64.AppImage \
            -O /tmp/appimagetool
        chmod +x /tmp/appimagetool
        APPIMAGETOOL=/tmp/appimagetool
    else
        APPIMAGETOOL=appimagetool
    fi
    
    # Create AppDir structure
    APPDIR="$OUTPUT_DIR/AppDir"
    rm -rf "$APPDIR"
    mkdir -p "$APPDIR/usr/bin"
    mkdir -p "$APPDIR/usr/lib"
    mkdir -p "$APPDIR/usr/share/applications"
    mkdir -p "$APPDIR/usr/share/icons/hicolor/256x256/apps"
    
    # Copy application files
    cp -r "$BUNDLE_DIR"/* "$APPDIR/usr/bin/"
    
    # Create desktop entry
    cat > "$APPDIR/pirate-unified-wallet.desktop" <<EOF
[Desktop Entry]
Name=Pirate Unified Wallet
Exec=pirate_unified_wallet
Icon=pirate-unified-wallet
Type=Application
Categories=Finance;Utility;
Comment=Privacy-first cryptocurrency wallet for Pirate Chain
Terminal=false
EOF
    
    cp "$APPDIR/pirate-unified-wallet.desktop" "$APPDIR/usr/share/applications/"
    
    # Copy icon (assuming it exists)
    if [ -f "$PROJECT_ROOT/assets/icon.png" ]; then
        cp "$PROJECT_ROOT/assets/icon.png" \
            "$APPDIR/usr/share/icons/hicolor/256x256/apps/pirate-unified-wallet.png"
        cp "$PROJECT_ROOT/assets/icon.png" "$APPDIR/pirate-unified-wallet.png"
    fi
    
    # Create AppRun script
    cat > "$APPDIR/AppRun" <<'EOF'
#!/bin/bash
APPDIR="$(dirname "$(readlink -f "$0")")"
exec "$APPDIR/usr/bin/pirate_unified_wallet" "$@"
EOF
    chmod +x "$APPDIR/AppRun"
    
    # Build AppImage
    ARCH=x86_64 $APPIMAGETOOL "$APPDIR" "$OUTPUT_DIR/pirate-unified-wallet-linux-x86_64.AppImage"
    
    # Generate checksum
    cd "$OUTPUT_DIR"
    sha256sum "pirate-unified-wallet-linux-x86_64.AppImage" > "pirate-unified-wallet-linux-x86_64.AppImage.sha256"
    
    log "AppImage created: $OUTPUT_DIR/pirate-unified-wallet-linux-x86_64.AppImage"
}

build_flatpak() {
    log "Creating Flatpak manifest..."
    
    # Create Flatpak manifest
    FLATPAK_MANIFEST="$PROJECT_ROOT/black.pirate.wallet.yml"
    
    cat > "$FLATPAK_MANIFEST" <<EOF
app-id: black.pirate.wallet
runtime: org.freedesktop.Platform
runtime-version: '23.08'
sdk: org.freedesktop.Sdk
command: pirate-unified-wallet
finish-args:
  - --share=network
  - --socket=wayland
  - --socket=fallback-x11
  - --device=dri
  - --filesystem=xdg-data/pirate-wallet:create
modules:
  - name: pirate-unified-wallet
    buildsystem: simple
    build-commands:
      - cp -r bundle /app/
      - install -Dm755 bundle/pirate_unified_wallet /app/bin/pirate-unified-wallet
      - install -Dm644 pirate-unified-wallet.desktop /app/share/applications/black.pirate.wallet.desktop
      - install -Dm644 icon.png /app/share/icons/hicolor/256x256/apps/black.pirate.wallet.png
    sources:
      - type: dir
        path: $BUNDLE_DIR
        dest: bundle
EOF
    
    log "Flatpak manifest created: $FLATPAK_MANIFEST"
    log "Build with: flatpak-builder build-dir $FLATPAK_MANIFEST"
    
    # Check if flatpak-builder is available
    if command -v flatpak-builder &> /dev/null; then
        log "Building Flatpak..."
        flatpak-builder --force-clean "$OUTPUT_DIR/flatpak-build" "$FLATPAK_MANIFEST"
        
        log "Creating Flatpak bundle..."
        flatpak build-bundle "$OUTPUT_DIR/flatpak-build" \
            "$OUTPUT_DIR/pirate-unified-wallet.flatpak" \
            black.pirate.wallet
        
        log "Flatpak created: $OUTPUT_DIR/pirate-unified-wallet.flatpak"
    else
        warn "flatpak-builder not found. Manifest created but not built."
    fi
}

build_deb() {
    log "Creating Debian package..."
    
    DEB_DIR="$OUTPUT_DIR/deb"
    rm -rf "$DEB_DIR"
    mkdir -p "$DEB_DIR/DEBIAN"
    mkdir -p "$DEB_DIR/usr/bin"
    mkdir -p "$DEB_DIR/usr/share/applications"
    mkdir -p "$DEB_DIR/usr/share/icons/hicolor/256x256/apps"
    mkdir -p "$DEB_DIR/usr/share/doc/pirate-unified-wallet"
    
    # Copy application files
    cp -r "$BUNDLE_DIR"/* "$DEB_DIR/usr/bin/"
    
    # Create control file
    cat > "$DEB_DIR/DEBIAN/control" <<EOF
Package: pirate-unified-wallet
Version: 1.0.0
Section: utils
Priority: optional
Architecture: amd64
Maintainer: Pirate Chain <dev@pirate.black>
Description: Privacy-first cryptocurrency wallet for Pirate Chain
 Pirate Unified Wallet is a production-grade, privacy-first wallet for
 Pirate Chain (ARRR) with Sapling shielded transactions, Tor routing,
 and watch-only capabilities.
Depends: libgtk-3-0, libglib2.0-0, libsqlite3-0
Homepage: https://pirate.black
EOF
    
    # Create desktop entry
    cat > "$DEB_DIR/usr/share/applications/pirate-unified-wallet.desktop" <<EOF
[Desktop Entry]
Name=Pirate Unified Wallet
Exec=/usr/bin/pirate_unified_wallet
Icon=pirate-unified-wallet
Type=Application
Categories=Finance;Utility;
Comment=Privacy-first cryptocurrency wallet for Pirate Chain
Terminal=false
EOF
    
    # Copy icon
    if [ -f "$PROJECT_ROOT/assets/icon.png" ]; then
        cp "$PROJECT_ROOT/assets/icon.png" \
            "$DEB_DIR/usr/share/icons/hicolor/256x256/apps/pirate-unified-wallet.png"
    fi
    
    # Copy documentation
    cat > "$DEB_DIR/usr/share/doc/pirate-unified-wallet/copyright" <<EOF
Format: https://www.debian.org/doc/packaging-manuals/copyright-format/1.0/
Upstream-Name: pirate-unified-wallet
Source: https://github.com/pirate/wallet

Files: *
Copyright: 2025 Pirate Chain
License: MIT or Apache-2.0
EOF
    
    # Build deb package
    dpkg-deb --build "$DEB_DIR" "$OUTPUT_DIR/pirate-unified-wallet_1.0.0_amd64.deb"
    
    # Generate checksum
    cd "$OUTPUT_DIR"
    sha256sum "pirate-unified-wallet_1.0.0_amd64.deb" > "pirate-unified-wallet_1.0.0_amd64.deb.sha256"
    
    log "Debian package created: $OUTPUT_DIR/pirate-unified-wallet_1.0.0_amd64.deb"
    
    # Create repository metadata (for apt install pirate-unified-wallet)
    create_apt_repo_metadata
}

create_apt_repo_metadata() {
    log "Creating APT repository metadata..."
    
    REPO_DIR="$OUTPUT_DIR/apt-repo"
    mkdir -p "$REPO_DIR/pool/main"
    mkdir -p "$REPO_DIR/dists/stable/main/binary-amd64"
    
    # Copy deb to pool
    cp "$OUTPUT_DIR/pirate-unified-wallet_1.0.0_amd64.deb" "$REPO_DIR/pool/main/"
    
    # Create Packages file
    cd "$REPO_DIR"
    dpkg-scanpackages pool/main /dev/null | gzip -9c > dists/stable/main/binary-amd64/Packages.gz
    dpkg-scanpackages pool/main /dev/null > dists/stable/main/binary-amd64/Packages
    
    # Create Release file
    cat > "dists/stable/Release" <<EOF
Origin: Pirate Chain
Label: Pirate Chain
Suite: stable
Codename: stable
Architectures: amd64
Components: main
Description: Pirate Chain official package repository
Date: $(date -u +"%a, %d %b %Y %H:%M:%S %Z")
EOF
    
    # Generate hashes
    cd "dists/stable"
    {
        echo "MD5Sum:"
        find . -type f -exec md5sum {} \; | sed 's,\./,,'
        echo "SHA1:"
        find . -type f -exec sha1sum {} \; | sed 's,\./,,'
        echo "SHA256:"
        find . -type f -exec sha256sum {} \; | sed 's,\./,,'
    } >> Release
    
    # Create installation instructions
    cat > "$REPO_DIR/INSTALL.md" <<'EOF'
# Pirate Unified Wallet - APT Repository

## Installation

Add the repository (once hosted):

```bash
echo "deb [trusted=yes] https://apt.pirate.black stable main" | sudo tee /etc/apt/sources.list.d/pirate.list
sudo apt update
sudo apt install pirate-unified-wallet
```

## Local Installation

For local installation from the .deb file:

```bash
sudo dpkg -i pirate-unified-wallet_1.0.0_amd64.deb
sudo apt-get install -f  # Install dependencies
```

## Verification

Verify the package signature:

```bash
sha256sum -c pirate-unified-wallet_1.0.0_amd64.deb.sha256
```
EOF
    
    log "APT repository metadata created at: $REPO_DIR"
    log "See $REPO_DIR/INSTALL.md for installation instructions"
}

log "Build complete!"

