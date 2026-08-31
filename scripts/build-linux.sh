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

APPIMAGE_WORK_DIR=""

cleanup_build_temp() {
    if [[ -n "$APPIMAGE_WORK_DIR" && -d "$APPIMAGE_WORK_DIR" ]]; then
        rm -rf -- "$APPIMAGE_WORK_DIR"
    fi
}

trap cleanup_build_temp EXIT

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

flutter_build_linux_release() {
    local status=1
    for attempt in 1 2 3; do
        if OVERRIDE_DEFI_API_DOWNLOAD=false flutter build linux --release \
            --dart-define="PIRATE_RELEASE_TAG=${GITHUB_REF_NAME:-}"; then
            return 0
        else
            status=$?
        fi
        if [ "$attempt" -lt 3 ]; then
            warn "Linux build attempt $attempt failed; retrying..."
            sleep $((attempt * 20))
        fi
    done
    return "$status"
}

read_pubspec_version() {
    local pubspec="$APP_DIR/pubspec.yaml"
    local raw
    raw="$(sed -nE 's/^version:[[:space:]]*([0-9]+\.[0-9]+\.[0-9]+)\+([0-9]+).*/\1+\2/p' "$pubspec" | head -n1)"
    if [[ -z "$raw" ]]; then
        error "Unable to parse app version from $pubspec"
    fi
    APP_VERSION_SEMVER="${raw%%+*}"
    APP_VERSION_BUILD="${raw##*+}"
    APP_VERSION_FULL="$APP_VERSION_SEMVER+$APP_VERSION_BUILD"
}

# Parse arguments
FORMAT="${1:-appimage}"  # appimage, flatpak, or deb

# Reproducible build settings
export SOURCE_DATE_EPOCH="${SOURCE_DATE_EPOCH:-$(git log -1 --format=%ct 2>/dev/null || date +%s)}"
export TZ=UTC
export FLUTTER_SUPPRESS_ANALYTICS=true
export DART_SUPPRESS_ANALYTICS=true
export CARGO_INCREMENTAL=0

log "Building Linux $FORMAT (reproducible)"
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

download_file() {
    local url="$1"
    local dest="$2"
    if command -v curl &> /dev/null; then
        curl -fsSL --retry 3 --retry-delay 2 -o "$dest" "$url"
        return 0
    fi
    if command -v wget &> /dev/null; then
        wget -q -O "$dest" "$url"
        return 0
    fi
    error "Missing download tool: install curl or wget."
}

sha256_check() {
    local file="$1"
    local expected="$2"
    if [[ -z "$expected" ]]; then
        error "Missing expected SHA256 for $file"
    fi
    if command -v sha256sum &> /dev/null; then
        local actual
        actual="$(sha256sum "$file" | awk '{print $1}')"
        [[ "$actual" == "$expected" ]] || error "SHA256 mismatch for $file"
        return 0
    fi
    if command -v shasum &> /dev/null; then
        local actual
        actual="$(shasum -a 256 "$file" | awk '{print $1}')"
        [[ "$actual" == "$expected" ]] || error "SHA256 mismatch for $file"
        return 0
    fi
    error "Missing sha256sum/shasum to verify $file"
}

verify_static_appimage_runtime() {
    local runtime_file="$1"
    local dynamic_section

    if ! command -v readelf &> /dev/null; then
        error "readelf is required to verify the AppImage runtime."
    fi
    if ! dynamic_section="$(readelf --dynamic --wide "$runtime_file" 2>&1)"; then
        error "Unable to inspect AppImage runtime ELF metadata: $dynamic_section"
    fi
    if grep -Fq '(NEEDED)' <<<"$dynamic_section"; then
        error "AppImage runtime has shared-library dependencies; refusing a non-portable artifact."
    fi
    if LC_ALL=C grep -aFq 'libfuse.so.2' "$runtime_file"; then
        error "AppImage runtime still loads legacy libfuse.so.2."
    fi
}

ensure_flathub_remote() {
    if ! command -v flatpak &> /dev/null; then
        return 0
    fi
    if flatpak remotes --system 2>/dev/null | awk '{print $1}' | grep -q '^flathub$'; then
        return 0
    fi
    if flatpak remotes --user 2>/dev/null | awk '{print $1}' | grep -q '^flathub$'; then
        return 0
    fi
    log "Adding Flathub remote (user)..."
    if ! flatpak remote-add --user --if-not-exists flathub https://flathub.org/repo/flathub.flatpakrepo; then
        warn "Unable to add Flathub remote (user)."
        return 1
    fi
}

ensure_flatpak_runtime() {
    if ! command -v flatpak &> /dev/null; then
        return 0
    fi
    local sdk="org.freedesktop.Sdk//25.08"
    local platform="org.freedesktop.Platform//25.08"
    if flatpak info --user "$sdk" &> /dev/null && flatpak info --user "$platform" &> /dev/null; then
        return 0
    fi
    log "Installing Flatpak runtime (user)..."
    if ! flatpak install --user -y flathub "$platform" "$sdk"; then
        error "Failed to install Flatpak runtime: $platform / $sdk"
    fi
}
stage_rust_linux() {
    local bundle_dir="$1"
    log "Building Rust FFI library..."
    (cd "$PROJECT_ROOT/crates" && cargo build --release --package pirate-ffi-frb --features frb --no-default-features --locked)
    local so_path="$PROJECT_ROOT/crates/target/release/libpirate_ffi_frb.so"
    if [ ! -f "$so_path" ]; then
        error "Rust library not found at $so_path"
    fi
    if command -v strip &> /dev/null; then
        strip --strip-unneeded "$so_path" || warn "Failed to strip $so_path"
    fi
    local dest_dir="$bundle_dir/lib"
    mkdir -p "$dest_dir"
    cp "$so_path" "$dest_dir/"
}

log "Fetching Tor/I2P assets..."
chmod +x "$SCRIPT_DIR/fetch-tor-i2p-assets.sh"
"$SCRIPT_DIR/fetch-tor-i2p-assets.sh"

cd "$APP_DIR"

# On tag builds, align app version metadata with the git tag (vX.Y.Z).
bash "$SCRIPT_DIR/sync-version-from-tag.sh"
read_pubspec_version
log "App version: $APP_VERSION_FULL"

# Clean previous builds
log "Cleaning previous builds..."
flutter clean

# Get dependencies
log "Fetching dependencies..."
flutter pub get --enforce-lockfile

log "Preparing hermetic Flutter assets..."
bash "$SCRIPT_DIR/prepare-flutter-build.sh" linux

# Build Linux app
log "Building Linux app..."
flutter_build_linux_release

BUNDLE_DIR="$APP_DIR/build/linux/x64/release/bundle"

if [ ! -d "$BUNDLE_DIR" ]; then
    error "Build failed: Bundle directory not found"
fi

stage_rust_linux "$BUNDLE_DIR"

log "Verifying bundled KDF artifacts..."
bash "$SCRIPT_DIR/verify-kdf-artifacts.sh" linux "$BUNDLE_DIR"

log "Verifying Ubuntu 22.04 binary compatibility..."
python3 "$SCRIPT_DIR/verify_linux_glibc.py" \
    --max-version 2.35 \
    "$BUNDLE_DIR"

OUTPUT_DIR="$PROJECT_ROOT/dist/linux"
mkdir -p "$OUTPUT_DIR"

runtime_executable="$BUNDLE_DIR/stashi-wallet"
[ -f "$runtime_executable" ] || error "Installed Linux executable not found: $runtime_executable"
runtime_hash="$(sha256sum "$runtime_executable" | awk '{print $1}')"
printf '%s  %s\n' "$runtime_hash" 'stashi-wallet' \
    > "$OUTPUT_DIR/installed-payload-linux.txt"

build_appimage() {
    log "Creating AppImage..."

    APPIMAGE_WORK_DIR="$(mktemp -d)"
    local appimage_runtime="$APPIMAGE_WORK_DIR/runtime-x86_64"

    # Prefer the pinned tool when configured. A system tool remains useful for
    # local builds, but it must support selecting the separately pinned runtime.
    if [[ -n "${APPIMAGETOOL_URL:-}" || -n "${APPIMAGETOOL_SHA256:-}" ]]; then
        if [[ -z "${APPIMAGETOOL_URL:-}" || -z "${APPIMAGETOOL_SHA256:-}" ]]; then
            error "Set both APPIMAGETOOL_URL and APPIMAGETOOL_SHA256."
        fi
        log "Downloading pinned appimagetool..."
        local appimagetool_tmp="$APPIMAGE_WORK_DIR/appimagetool.AppImage"
        download_file "$APPIMAGETOOL_URL" "$appimagetool_tmp"
        sha256_check "$appimagetool_tmp" "$APPIMAGETOOL_SHA256"
        chmod +x "$appimagetool_tmp"
        local appimagetool_extract="$APPIMAGE_WORK_DIR/appimagetool-extracted"
        mkdir -p "$appimagetool_extract"
        if ! (cd "$appimagetool_extract" && "$appimagetool_tmp" --appimage-extract >/dev/null 2>&1); then
            error "Failed to extract the pinned appimagetool AppImage."
        fi
        if [ ! -x "$appimagetool_extract/squashfs-root/AppRun" ]; then
            error "Extracted appimagetool AppRun not found"
        fi
        APPIMAGETOOL="$appimagetool_extract/squashfs-root/AppRun"
    elif command -v appimagetool &> /dev/null; then
        APPIMAGETOOL="$(command -v appimagetool)"
    else
        error "appimagetool not found. Set pinned APPIMAGETOOL_URL and APPIMAGETOOL_SHA256 values."
    fi

    if ! "$APPIMAGETOOL" --help 2>&1 | grep -Fq -- '--runtime-file'; then
        error "appimagetool does not support --runtime-file; use the pinned modern release."
    fi

    if [[ -z "${APPIMAGE_RUNTIME_URL:-}" || -z "${APPIMAGE_RUNTIME_SHA256:-}" ]]; then
        error "Set APPIMAGE_RUNTIME_URL and APPIMAGE_RUNTIME_SHA256 for a reproducible AppImage build."
    fi
    log "Downloading pinned static AppImage runtime..."
    download_file "$APPIMAGE_RUNTIME_URL" "$appimage_runtime"
    sha256_check "$appimage_runtime" "$APPIMAGE_RUNTIME_SHA256"
    verify_static_appimage_runtime "$appimage_runtime"

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
    cat > "$APPDIR/stashi-wallet.desktop" <<EOF
[Desktop Entry]
Name=Stashi Wallet
Exec=stashi-wallet
Icon=stashi-wallet
Type=Application
Categories=Finance;Utility;
Comment=Privacy-first cryptocurrency wallet for Pirate Chain
Terminal=false
EOF
    
    cp "$APPDIR/stashi-wallet.desktop" "$APPDIR/usr/share/applications/"
    
    # Copy icon
    if [ -f "$PROJECT_ROOT/app/assets/icons/stashi-wallet-app-icon.png" ]; then
        cp "$PROJECT_ROOT/app/assets/icons/stashi-wallet-app-icon.png" \
            "$APPDIR/usr/share/icons/hicolor/256x256/apps/stashi-wallet.png"
        cp "$PROJECT_ROOT/app/assets/icons/stashi-wallet-app-icon.png" "$APPDIR/stashi-wallet.png"
    fi
    
    # Create AppRun script
    cat > "$APPDIR/AppRun" <<'EOF'
#!/bin/bash
APPDIR="$(dirname "$(readlink -f "$0")")"
exec "$APPDIR/usr/bin/stashi-wallet" "$@"
EOF
    chmod +x "$APPDIR/AppRun"
    
    # Build AppImage
    normalize_mtime "$APPDIR"
    local appimage_epoch="${SOURCE_DATE_EPOCH:-}"
    if [ -n "$appimage_epoch" ]; then
        # Avoid mksquashfs conflict when SOURCE_DATE_EPOCH is set
        # and appimagetool passes timestamp flags internally.
        env -u SOURCE_DATE_EPOCH \
            APPIMAGE_SQUASHFS_OPTIONS="-all-time $appimage_epoch" \
            ARCH=x86_64 \
            "$APPIMAGETOOL" --runtime-file "$appimage_runtime" \
            "$APPDIR" "$OUTPUT_DIR/Stashi-Wallet-linux-x86_64.AppImage"
    else
        ARCH=x86_64 "$APPIMAGETOOL" --runtime-file "$appimage_runtime" \
            "$APPDIR" "$OUTPUT_DIR/Stashi-Wallet-linux-x86_64.AppImage"
    fi

    python3 "$SCRIPT_DIR/verify_appimage_runtime.py" \
        "$OUTPUT_DIR/Stashi-Wallet-linux-x86_64.AppImage" \
        "$appimage_runtime"

    python3 "$SCRIPT_DIR/verify_linux_glibc.py" \
        --max-version 2.35 \
        "$OUTPUT_DIR/Stashi-Wallet-linux-x86_64.AppImage"
    
    # Generate checksum
    cd "$OUTPUT_DIR"
    sha256sum "Stashi-Wallet-linux-x86_64.AppImage" > "Stashi-Wallet-linux-x86_64.AppImage.sha256"
    
    log "AppImage created: $OUTPUT_DIR/Stashi-Wallet-linux-x86_64.AppImage"
}

build_flatpak() {
    log "Creating Flatpak manifest..."
    
    # Create Flatpak manifest
    FLATPAK_MANIFEST="$PROJECT_ROOT/com.pirate.wallet.yml"
    
    cat > "$FLATPAK_MANIFEST" <<EOF
app-id: com.pirate.wallet
runtime: org.freedesktop.Platform
runtime-version: '25.08'
sdk: org.freedesktop.Sdk
command: stashi-wallet
finish-args:
  - --share=network
  - --socket=wayland
  - --socket=fallback-x11
  - --device=dri
  - --talk-name=org.freedesktop.secrets
  - --filesystem=xdg-data/pirate-wallet:create
modules:
  - name: stashi-wallet
    buildsystem: simple
    build-commands:
      - cp -r bundle /app/
      - |
          install -d /app/bin
          cat > /app/bin/stashi-wallet <<'LAUNCHER'
          #!/bin/sh
          exec /app/bundle/stashi-wallet "\$@"
          LAUNCHER
          chmod 0755 /app/bin/stashi-wallet
      - |
          cat > com.pirate.wallet.desktop <<'DESKTOP'
          [Desktop Entry]
          Name=Stashi Wallet
          Exec=stashi-wallet
          Icon=com.pirate.wallet
          Type=Application
          Categories=Finance;Utility;
          Comment=Privacy-first cryptocurrency wallet for Pirate Chain
          Terminal=false
          DESKTOP
      - install -Dm644 com.pirate.wallet.desktop /app/share/applications/com.pirate.wallet.desktop
      - install -Dm644 assets/app_icon_256.png /app/share/icons/hicolor/256x256/apps/com.pirate.wallet.png
    sources:
      - type: dir
        path: $BUNDLE_DIR
        dest: bundle
      - type: file
        path: $PROJECT_ROOT/app/macos/Runner/Assets.xcassets/AppIcon.appiconset/app_icon_256.png
        dest: assets
EOF
    
    log "Flatpak manifest created: $FLATPAK_MANIFEST"
    log "Build with: flatpak-builder build-dir $FLATPAK_MANIFEST"
    
    # Check if flatpak-builder is available
    if command -v flatpak-builder &> /dev/null; then
        if ! ensure_flathub_remote; then
            error "Flathub remote unavailable; cannot build Flatpak."
        fi
        ensure_flatpak_runtime
        log "Building Flatpak..."
        local flatpak_build_dir="$OUTPUT_DIR/flatpak-build"
        local flatpak_repo_dir="$OUTPUT_DIR/flatpak-repo"
        rm -rf "$flatpak_build_dir" "$flatpak_repo_dir"
        flatpak-builder --user --install-deps-from=flathub --force-clean \
            --repo="$flatpak_repo_dir" \
            "$flatpak_build_dir" \
            "$FLATPAK_MANIFEST"

        log "Verifying Flatpak runtime layout..."
        flatpak build "$flatpak_build_dir" sh -eu -c \
            'test -x /app/bin/stashi-wallet; test -x /app/bundle/stashi-wallet; test -d /app/bundle/data; test -d /app/bundle/lib; grep -Fq "/app/bundle/stashi-wallet" /app/bin/stashi-wallet'

        log "Creating Flatpak bundle..."
        flatpak build-bundle "$flatpak_repo_dir" \
            "$OUTPUT_DIR/Stashi-Wallet.flatpak" \
            com.pirate.wallet
        
        log "Flatpak created: $OUTPUT_DIR/Stashi-Wallet.flatpak"
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
    mkdir -p "$DEB_DIR/usr/share/doc/stashi-wallet"
    
    # Copy application files
    cp -r "$BUNDLE_DIR"/* "$DEB_DIR/usr/bin/"
    
    # Create control file
    cat > "$DEB_DIR/DEBIAN/control" <<EOF
Package: stashi-wallet
Version: $APP_VERSION_SEMVER
Section: utils
Priority: optional
Architecture: amd64
Maintainer: Pirate Chain Foundation <dev@piratechainfoundation.com>
Provides: pirate-unified-wallet
Replaces: pirate-unified-wallet
Conflicts: pirate-unified-wallet
Description: Privacy-first cryptocurrency wallet for Pirate Chain
 Stashi Wallet is a privacy-first wallet for
 Pirate Chain (ARRR) with Sapling shielded transactions, Tor routing,
 and watch-only capabilities.
Depends: libgtk-3-0, libglib2.0-0, libsqlite3-0
Homepage: https://piratechain.com
EOF
    
    # Create desktop entry
    cat > "$DEB_DIR/usr/share/applications/stashi-wallet.desktop" <<EOF
[Desktop Entry]
Name=Stashi Wallet
Exec=/usr/bin/stashi-wallet
Icon=stashi-wallet
Type=Application
Categories=Finance;Utility;
Comment=Privacy-first cryptocurrency wallet for Pirate Chain
Terminal=false
EOF
    
    # Copy icon
    if [ -f "$PROJECT_ROOT/app/assets/icons/stashi-wallet-app-icon.png" ]; then
        cp "$PROJECT_ROOT/app/assets/icons/stashi-wallet-app-icon.png" \
            "$DEB_DIR/usr/share/icons/hicolor/256x256/apps/stashi-wallet.png"
    fi
    
    # Copy documentation
    cat > "$DEB_DIR/usr/share/doc/stashi-wallet/copyright" <<EOF
Format: https://www.debian.org/doc/packaging-manuals/copyright-format/1.0/
Package-Name: stashi-wallet
Source: https://github.com/PirateNetwork/Pirate-Unified-Light-Wallet

Files: *
Copyright: 2026 Pirate Chain Foundation
License: MIT or Apache-2.0
EOF
    
    # Build deb package
    normalize_mtime "$DEB_DIR"
    dpkg-deb --build "$DEB_DIR" "$OUTPUT_DIR/Stashi-Wallet-amd64.deb"
    
    # Generate checksum
    cd "$OUTPUT_DIR"
    sha256sum "Stashi-Wallet-amd64.deb" > "Stashi-Wallet-amd64.deb.sha256"
    
    log "Debian package created: $OUTPUT_DIR/Stashi-Wallet-amd64.deb"
    
    # Create repository metadata (for apt install stashi-wallet)
    create_apt_repo_metadata
}

create_apt_repo_metadata() {
    log "Creating APT repository metadata..."
    
    REPO_DIR="$OUTPUT_DIR/apt-repo"
    mkdir -p "$REPO_DIR/pool/main"
    mkdir -p "$REPO_DIR/dists/stable/main/binary-amd64"
    
    # Copy deb to pool
    cp "$OUTPUT_DIR/Stashi-Wallet-amd64.deb" "$REPO_DIR/pool/main/"
    
    if ! command -v dpkg-scanpackages &> /dev/null; then
        error "dpkg-scanpackages not found. Install dpkg-dev to generate apt metadata."
    fi

    # Create Packages file
    cd "$REPO_DIR"
    dpkg-scanpackages pool/main /dev/null | gzip -9c > dists/stable/main/binary-amd64/Packages.gz
    dpkg-scanpackages pool/main /dev/null > dists/stable/main/binary-amd64/Packages
    
    # Create Release file
    local release_date
    release_date="$(date -u -d "@$SOURCE_DATE_EPOCH" +"%a, %d %b %Y %H:%M:%S %Z" 2>/dev/null || date -u +"%a, %d %b %Y %H:%M:%S %Z")"
    cat > "dists/stable/Release" <<EOF
Origin: Pirate Chain
Label: Pirate Chain
Suite: stable
Codename: stable
Architectures: amd64
Components: main
Description: Pirate Chain official package repository
Date: $release_date
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
# Stashi Wallet APT Repository

## Installation

Add the repository (once hosted):

```bash
echo "deb [trusted=yes] <APT_REPO_URL> stable main" | sudo tee /etc/apt/sources.list.d/pirate.list
sudo apt update
sudo apt install stashi-wallet
```

## Local Installation

Replace `<APT_REPO_URL>` with the official repository URL when published.

For local installation from the .deb file:

```bash
sudo dpkg -i Stashi-Wallet-amd64.deb
sudo apt-get install -f  # Install dependencies
```

## Verification

Verify the package signature:

```bash
sha256sum -c Stashi-Wallet-amd64.deb.sha256
```
EOF
    
    log "APT repository metadata created at: $REPO_DIR"
    log "See $REPO_DIR/INSTALL.md for installation instructions"
}

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

log "Build complete!"
