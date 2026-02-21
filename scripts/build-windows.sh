#!/usr/bin/env bash
# Windows desktop build + packaging script (installer + portable).
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

if [ -d "/c/Strawberry/perl/bin" ]; then
    export PATH="/c/Strawberry/perl/bin:/c/Strawberry/c/bin:$PATH"
    export PERL="/c/Strawberry/perl/bin/perl"
fi

log "Building Windows desktop artifacts (reproducible)"
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
    (cd "$src" && normalize_mtime ".")
    if command -v zip &> /dev/null; then
        (cd "$src" && LC_ALL=C find . -type f -print | sort | zip -X -@ "$dest")
        return 0
    fi
    if command -v python &> /dev/null; then
        python - "$src" "$dest" "${SOURCE_DATE_EPOCH:-}" <<'PY'
import os
import sys
import time
import datetime
import zipfile
import shutil

src = sys.argv[1]
dest = sys.argv[2]
epoch_raw = sys.argv[3] if len(sys.argv) > 3 else ""
try:
    epoch = int(epoch_raw) if epoch_raw else int(time.time())
except ValueError:
    epoch = int(time.time())

dt = datetime.datetime.utcfromtimestamp(epoch)
zip_dt = (dt.year, dt.month, dt.day, dt.hour, dt.minute, dt.second)

if os.path.exists(dest):
    os.remove(dest)

with zipfile.ZipFile(dest, "w", compression=zipfile.ZIP_DEFLATED) as zf:
    for root, dirs, files in os.walk(src):
        dirs.sort()
        files.sort()
        for name in files:
            path = os.path.join(root, name)
            rel = os.path.relpath(path, src).replace(os.sep, "/")
            info = zipfile.ZipInfo(rel, date_time=zip_dt)
            info.compress_type = zipfile.ZIP_DEFLATED
            info.external_attr = 0o100644 << 16
            with open(path, "rb") as f, zf.open(info, "w") as out:
                shutil.copyfileobj(f, out, 1024 * 1024)
        # Only the entries matter, directories are implied.
PY
        return 0
    fi
    error "zip not found and python not available to create portable archive."
}

sign_windows_binaries() {
    local release_dir="$1"
    local cert_path="$2"
    local cert_password="$3"
    if [ -z "$cert_path" ] || [ ! -f "$cert_path" ]; then
        return 1
    fi
    if [ -z "$cert_password" ]; then
        warn "WINDOWS_SIGN_PASSWORD not set. Skipping binary signing."
        return 1
    fi
    local signtool_cmd
    signtool_cmd="$(resolve_signtool || true)"
    if [ -z "$signtool_cmd" ]; then
        warn "signtool not found. Skipping binary signing."
        return 1
    fi
    local signed_any=false
    local failed_any=false
    while IFS= read -r -d '' file; do
        if sign_windows_file "$file" "$cert_path" "$cert_password" "$signtool_cmd"; then
            signed_any=true
        else
            failed_any=true
            warn "Failed to sign $file"
        fi
    done < <(find "$release_dir" -maxdepth 1 -type f \( -name "*.exe" -o -name "*.dll" \) -print0)
    if [ "$failed_any" = "true" ]; then
        warn "One or more Windows binaries failed to sign."
        return 1
    fi
    if [ "$signed_any" = "true" ]; then
        return 0
    fi
    warn "No Windows binaries found to sign in $release_dir"
    return 1
}

resolve_signtool() {
    if command -v signtool.exe &> /dev/null; then
        echo "signtool.exe"
        return 0
    fi
    if command -v signtool &> /dev/null; then
        echo "signtool"
        return 0
    fi
    return 1
}

sign_windows_file() {
    local file="$1"
    local cert_path="$2"
    local cert_password="$3"
    local signtool_cmd="$4"
    local timestamp_url="${WINDOWS_SIGN_TIMESTAMP_URL:-}"
    if [ -n "$timestamp_url" ]; then
        "$signtool_cmd" sign /fd SHA256 /tr "$timestamp_url" /td SHA256 /f "$cert_path" /p "$cert_password" "$file"
        return $?
    fi
    "$signtool_cmd" sign /fd SHA256 /f "$cert_path" /p "$cert_password" "$file"
}

resolve_release_dir() {
    if [ -d "build/windows/runner/Release" ]; then
        (cd "build/windows/runner/Release" && pwd)
        return 0
    fi
    if [ -d "build/windows/x64/runner/Release" ]; then
        (cd "build/windows/x64/runner/Release" && pwd)
        return 0
    fi
    return 1
}

stage_rust_windows() {
    local release_dir="$1"
    log "Building Rust FFI library..."
    (cd "$PROJECT_ROOT/crates" && cargo build --release --target x86_64-pc-windows-msvc --package pirate-ffi-frb --features frb --no-default-features --locked)
    local dll_path="$PROJECT_ROOT/crates/target/x86_64-pc-windows-msvc/release/pirate_ffi_frb.dll"
    if [ ! -f "$dll_path" ]; then
        dll_path="$(find "$PROJECT_ROOT/crates/target/x86_64-pc-windows-msvc/release" -name "pirate_ffi_frb.dll" -print -quit)"
    fi
    if [ -z "$dll_path" ] || [ ! -f "$dll_path" ]; then
        error "Rust library not found under crates/target/x86_64-pc-windows-msvc/release"
    fi
    cp "$dll_path" "$release_dir/"
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

# On tag builds, align app version metadata with the git tag (vX.Y.Z).
bash "$SCRIPT_DIR/sync-version-from-tag.sh"

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
RELEASE_DIR="$(resolve_release_dir || true)"
if [ -z "$RELEASE_DIR" ]; then
    error "Build failed: Release directory not found"
fi

stage_rust_windows "$RELEASE_DIR"

# Sign binaries for portable distribution (optional)
SIGN_CERT="${WINDOWS_SIGN_CERT:-}"
SIGN_PASSWORD="${WINDOWS_SIGN_PASSWORD:-}"
BINARIES_SIGNED=false
if [ "$REPRODUCIBLE" = "1" ]; then
    warn "REPRODUCIBLE=1: skipping Windows binary signing."
else
    if sign_windows_binaries "$RELEASE_DIR" "$SIGN_CERT" "$SIGN_PASSWORD"; then
        BINARIES_SIGNED=true
    fi
fi

create_windows_installer() {
    local source_dir="$1"
    local output_dir="$2"
    local output_name="$3"

    local iscc_cmd=""
    if command -v iscc &> /dev/null; then
        iscc_cmd="iscc"
    elif command -v iscc.exe &> /dev/null; then
        iscc_cmd="iscc.exe"
    elif [ -n "${ISCC_PATH:-}" ] && [ -f "${ISCC_PATH:-}" ]; then
        iscc_cmd="$ISCC_PATH"
    fi

    if [ -z "$iscc_cmd" ]; then
        warn "Inno Setup compiler (iscc) not found. Skipping installer build."
        warn "Install Inno Setup or set ISCC_PATH to produce installer .exe."
        return 1
    fi

    local app_exe_name
    app_exe_name="$(find "$source_dir" -maxdepth 1 -type f -name "*.exe" -print | sed 's|.*/||' | LC_ALL=C sort | head -n 1)"
    if [ -z "$app_exe_name" ]; then
        warn "No runtime .exe found in $source_dir. Skipping installer build."
        return 1
    fi

    local app_version
    app_version="$(awk -F'[:+ ]+' '/^version:/ {print $2; exit}' "$APP_DIR/pubspec.yaml")"
    if [ -z "$app_version" ]; then
        app_version="0.0.0"
    fi

    local iss_file
    iss_file="$(mktemp --suffix=.iss)"
    cat > "$iss_file" <<'ISS'
[Setup]
AppId={{8A65B5A7-79A4-4EBF-A89E-9B8F745FA96F}
AppName=Pirate Wallet
AppVersion={#AppVersion}
DefaultDirName={localappdata}\PirateWallet
DefaultGroupName=Pirate Wallet
OutputDir={#OutputDir}
OutputBaseFilename={#OutputBaseFilename}
Compression=lzma2
SolidCompression=yes
WizardStyle=modern
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
DisableProgramGroupPage=yes
PrivilegesRequired=lowest
UninstallDisplayIcon={app}\{#AppExeName}

[Languages]
Name: "english"; MessagesFile: "compiler:Default.isl"

[Files]
Source: "{#SourceDir}\*"; DestDir: "{app}"; Flags: recursesubdirs createallsubdirs ignoreversion

[Icons]
Name: "{autoprograms}\Pirate Wallet"; Filename: "{app}\{#AppExeName}"
Name: "{autodesktop}\Pirate Wallet"; Filename: "{app}\{#AppExeName}"

[Run]
Filename: "{app}\{#AppExeName}"; Description: "Launch Pirate Wallet"; Flags: nowait postinstall skipifsilent
ISS

    "$iscc_cmd" \
        "/DSourceDir=$source_dir" \
        "/DOutputDir=$output_dir" \
        "/DOutputBaseFilename=$output_name" \
        "/DAppVersion=$app_version" \
        "/DAppExeName=$app_exe_name" \
        "$iss_file"

    rm -f "$iss_file"
    return 0
}

# Create output directory
OUTPUT_DIR="$PROJECT_ROOT/dist/windows"
mkdir -p "$OUTPUT_DIR"

PORTABLE_OUTPUT_NAME="pirate-unified-wallet-windows-portable"
if [ "$BINARIES_SIGNED" != "true" ]; then
    PORTABLE_OUTPUT_NAME="${PORTABLE_OUTPUT_NAME}-unsigned"
fi
PORTABLE_OUTPUT_NAME="${PORTABLE_OUTPUT_NAME}.zip"
INSTALLER_OUTPUT_NAME="pirate-unified-wallet-windows-installer"
if [ "$BINARIES_SIGNED" != "true" ]; then
    INSTALLER_OUTPUT_NAME="${INSTALLER_OUTPUT_NAME}-unsigned"
fi
INSTALLER_OUTPUT_NAME="${INSTALLER_OUTPUT_NAME}.exe"

# Copy artifacts
log "Creating portable version..."
cd "$RELEASE_DIR"
zip_dir_deterministic "." "$OUTPUT_DIR/$PORTABLE_OUTPUT_NAME"

log "Creating installer..."
if ! create_windows_installer "$RELEASE_DIR" "$OUTPUT_DIR" "${INSTALLER_OUTPUT_NAME%.exe}"; then
    warn "Installer artifact was not generated."
fi

# Generate SHA-256 checksums
log "Generating checksums..."
cd "$OUTPUT_DIR"
if [ -f "$INSTALLER_OUTPUT_NAME" ]; then
    sha256sum "$INSTALLER_OUTPUT_NAME" > "$INSTALLER_OUTPUT_NAME.sha256"
fi
sha256sum "$PORTABLE_OUTPUT_NAME" > "$PORTABLE_OUTPUT_NAME.sha256"

# Also publish checksums for top-level runtime executables so in-app
# verification can compare directly against a running binary (e.g. app.exe).
while IFS= read -r -d '' exe; do
    exe_name="$(basename "$exe")"
    sha256sum "$exe" | awk '{print $1}' > "$OUTPUT_DIR/$exe_name.sha256"
done < <(find "$RELEASE_DIR" -maxdepth 1 -type f -name "*.exe" -print0)

log "Build complete!"
if [ -f "$INSTALLER_OUTPUT_NAME" ]; then
    log "Installer: $OUTPUT_DIR/$INSTALLER_OUTPUT_NAME"
fi
log "Portable: $OUTPUT_DIR/$PORTABLE_OUTPUT_NAME"
