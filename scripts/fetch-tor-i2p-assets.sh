#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
APP_DIR="$PROJECT_ROOT/app"
I2P_DIR="$APP_DIR/i2p"
TOR_PT_DIR="$APP_DIR/tor-pt"

GREEN='\033[0;32m'
RED='\033[0;31m'
YELLOW='\033[1;33m'
NC='\033[0m'

TOR_BROWSER_VERSION="${TOR_BROWSER_VERSION:-15.0.4}"
TOR_BROWSER_BASE_URL="${TOR_BROWSER_BASE_URL:-https://dist.torproject.org/torbrowser/${TOR_BROWSER_VERSION}}"
TOR_BROWSER_LINUX_FILE="${TOR_BROWSER_LINUX_FILE:-tor-browser-linux-x86_64-${TOR_BROWSER_VERSION}.tar.xz}"
TOR_BROWSER_LINUX_SHA256="${TOR_BROWSER_LINUX_SHA256:-4deec4fb1e09aabd60eb9f8575566650c6300f3f7b66726a26f271e3c4694add}"
TOR_BROWSER_MACOS_FILE="${TOR_BROWSER_MACOS_FILE:-tor-browser-macos-${TOR_BROWSER_VERSION}.dmg}"
TOR_BROWSER_MACOS_SHA256="${TOR_BROWSER_MACOS_SHA256:-c6d21dd2d67d752af6d8c22ddc2cc515021f9805ff570c00e374201ed97433a7}"

I2PD_VERSION_DEFAULT="2.58.0"
I2PD_VERSION="${I2PD_VERSION:-$I2PD_VERSION_DEFAULT}"
I2PD_BASE_URL="${I2PD_BASE_URL:-https://github.com/PurpleI2P/i2pd/releases/download/$I2PD_VERSION}"
I2PD_LINUX_AMD64_SHA512="${I2PD_LINUX_AMD64_SHA512:-a034077c1261a1f9004c340bcab0d9662c21eafd15a7ddd5da18fb46c754e2ca3e89ccf2076d4e99134749c233f7fa69d5e75c98a17fa64e58d6b128578923a1}"
I2PD_LINUX_ARM64_SHA512="${I2PD_LINUX_ARM64_SHA512:-43e8569e78dc738298530017c6824d6fc1187ff365476817c82ebf21c5ed1cdaa141882b787f1ba8fbf043fe0c736e72d5825628ac5d9bf8db8f026a8a3bb4d0}"
I2PD_MACOS_SHA512="${I2PD_MACOS_SHA512:-db0fef0399e78f1080dab150ffb02ece73cedffd59766502635ae7c397301fda05acbd2979890aa680eecf46d70965c9b7ffff49fc88cc0240e2bec3668daf25}"

log() {
  echo -e "${GREEN}[$(date +'%Y-%m-%d %H:%M:%S')]${NC} $1"
}

warn() {
  echo -e "${YELLOW}[WARN]${NC} $1"
}

error() {
  echo -e "${RED}[ERROR]${NC} $1" >&2
  exit 1
}

if [[ "${SKIP_TOR_I2P_FETCH:-}" == "1" ]]; then
  log "Skipping Tor/I2P asset fetch (SKIP_TOR_I2P_FETCH=1)."
  exit 0
fi

OS="$(uname -s)"
case "$OS" in
  Darwin) PLATFORM="macos" ;;
  Linux) PLATFORM="linux" ;;
  MINGW*|MSYS*|CYGWIN*) error "Use scripts/fetch-tor-i2p-assets.ps1 on Windows." ;;
  *) error "Unsupported OS: $OS" ;;
esac

ARCH_RAW="$(uname -m)"
case "$ARCH_RAW" in
  x86_64|amd64) ARCH_LABEL="x86_64" ;;
  aarch64|arm64) ARCH_LABEL="aarch64" ;;
  *) error "Unsupported architecture: $ARCH_RAW" ;;
esac

have_cmd() {
  command -v "$1" >/dev/null 2>&1
}

download_file() {
  local url="$1"
  local dest="$2"

  if have_cmd curl; then
    curl -fsSL --retry 3 --retry-delay 2 -o "$dest" "$url"
    return 0
  fi
  if have_cmd wget; then
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
  if have_cmd sha256sum; then
    local actual
    actual="$(sha256sum "$file" | awk '{print $1}')"
    [[ "$actual" == "$expected" ]] || error "SHA256 mismatch for $file"
    return 0
  fi
  if have_cmd shasum; then
    local actual
    actual="$(shasum -a 256 "$file" | awk '{print $1}')"
    [[ "$actual" == "$expected" ]] || error "SHA256 mismatch for $file"
    return 0
  fi
  error "Missing sha256sum/shasum to verify $file"
}

sha512_check() {
  local file="$1"
  local expected="$2"
  if [[ -z "$expected" ]]; then
    error "Missing expected SHA512 for $file"
  fi
  if have_cmd sha512sum; then
    local actual
    actual="$(sha512sum "$file" | awk '{print $1}')"
    [[ "$actual" == "$expected" ]] || error "SHA512 mismatch for $file"
    return 0
  fi
  if have_cmd shasum; then
    local actual
    actual="$(shasum -a 512 "$file" | awk '{print $1}')"
    [[ "$actual" == "$expected" ]] || error "SHA512 mismatch for $file"
    return 0
  fi
  error "Missing sha512sum/shasum to verify $file"
}

extract_archive_or_copy() {
  local archive="$1"
  local target="$2"
  local dest="$3"
  local tmpdir="$4"

  if [[ -n "$tmpdir" ]]; then
    mkdir -p "$tmpdir"
  fi

  case "$archive" in
    *.zip)
      have_cmd unzip || error "Missing unzip to extract $archive"
      unzip -q "$archive" -d "$tmpdir"
      ;;
    *.tar.gz|*.tgz|*.tar.xz|*.tar.bz2)
      have_cmd tar || error "Missing tar to extract $archive"
      tar -xf "$archive" -C "$tmpdir"
      ;;
    *)
      cp "$archive" "$dest"
      return 0
      ;;
  esac

  local found
  found="$(find "$tmpdir" -type f \( -name "$target" -o -name "${target}.exe" \) | head -n 1)"
  if [[ -z "$found" ]]; then
    error "Could not find $target inside $archive"
  fi
  cp "$found" "$dest"
}

fetch_i2p() {
  local version="$I2PD_VERSION"
  local base_url="$I2PD_BASE_URL"
  local tmpdir
  tmpdir="$(mktemp -d)"

  mkdir -p "$I2P_DIR/$PLATFORM"

  if [[ "$PLATFORM" == "macos" ]]; then
    local file="i2pd_${version}_osx.tar.gz"
    local url="$base_url/$file"
    local archive="$tmpdir/$file"
    log "Downloading i2pd (macOS): $url"
    download_file "$url" "$archive"
    sha512_check "$archive" "$I2PD_MACOS_SHA512"
    extract_archive_or_copy "$archive" "i2pd" "$I2P_DIR/macos/i2pd" "$tmpdir/unpack"
    chmod +x "$I2P_DIR/macos/i2pd"
    rm -rf "$tmpdir"
    return 0
  fi

  if [[ "$PLATFORM" == "linux" ]]; then
    have_cmd ar || error "Missing 'ar' for extracting .deb packages"
    local deb_arch
    local dest_name
    case "$ARCH_LABEL" in
      x86_64) deb_arch="amd64"; dest_name="i2pd-x86_64" ;;
      aarch64) deb_arch="arm64"; dest_name="i2pd-aarch64" ;;
    esac
    local file="i2pd_${version}-1_${deb_arch}.deb"
    local url="$base_url/$file"
    local deb="$tmpdir/$file"
    log "Downloading i2pd (Linux ${deb_arch}): $url"
    download_file "$url" "$deb"
    if [[ "$deb_arch" == "amd64" ]]; then
      sha512_check "$deb" "$I2PD_LINUX_AMD64_SHA512"
    else
      sha512_check "$deb" "$I2PD_LINUX_ARM64_SHA512"
    fi
    (cd "$tmpdir" && ar x "$deb")
    local data_tar
    data_tar="$(find "$tmpdir" -maxdepth 1 -name 'data.tar.*' | head -n 1)"
    [[ -n "$data_tar" ]] || error "Failed to locate data.tar in $deb"
    mkdir -p "$tmpdir/extract"
    tar -xf "$data_tar" -C "$tmpdir/extract"
    local src="$tmpdir/extract/usr/bin/i2pd"
    [[ -f "$src" ]] || error "i2pd not found in $deb"
    cp "$src" "$I2P_DIR/linux/$dest_name"
    chmod +x "$I2P_DIR/linux/$dest_name"
    rm -rf "$tmpdir"
    return 0
  fi
}

fetch_tor_pt() {
  mkdir -p "$TOR_PT_DIR/$PLATFORM"
  local tmpdir
  tmpdir="$(mktemp -d)"

  if [[ "$PLATFORM" == "linux" ]]; then
    if [[ "$ARCH_LABEL" != "x86_64" ]]; then
      error "Tor Browser bundle is not published for linux/$ARCH_LABEL. Set TOR_BROWSER_LINUX_URL and TOR_BROWSER_LINUX_SHA256 for a custom build."
    fi
    have_cmd tar || error "Missing tar to extract Tor Browser bundle"
    local file="${TOR_BROWSER_LINUX_FILE}"
    local url="${TOR_BROWSER_LINUX_URL:-$TOR_BROWSER_BASE_URL/$file}"
    local archive="$tmpdir/$file"

    log "Downloading Tor Browser bundle: $url"
    download_file "$url" "$archive"
    sha256_check "$archive" "$TOR_BROWSER_LINUX_SHA256"

    local extract_dir="$tmpdir/unpack"
    mkdir -p "$extract_dir"
    tar -xf "$archive" -C "$extract_dir"

    local pt_dir
    pt_dir="$(find "$extract_dir" -type d -name 'PluggableTransports' | head -n 1)"
    [[ -n "$pt_dir" ]] || error "PluggableTransports not found in Tor Browser bundle"

    local snowflake_dest="$TOR_PT_DIR/$PLATFORM/snowflake-client"
    local obfs4_dest="$TOR_PT_DIR/$PLATFORM/obfs4proxy"
    cp "$pt_dir/snowflake-client" "$snowflake_dest"
    cp "$pt_dir/obfs4proxy" "$obfs4_dest"
    chmod +x "$snowflake_dest" "$obfs4_dest"
    rm -rf "$tmpdir"
    return 0
  fi

  if [[ "$PLATFORM" == "macos" ]]; then
    have_cmd hdiutil || error "Missing hdiutil to mount Tor Browser DMG"
    local file="${TOR_BROWSER_MACOS_FILE}"
    local url="${TOR_BROWSER_MACOS_URL:-$TOR_BROWSER_BASE_URL/$file}"
    local dmg="$tmpdir/$file"

    log "Downloading Tor Browser bundle: $url"
    download_file "$url" "$dmg"
    sha256_check "$dmg" "$TOR_BROWSER_MACOS_SHA256"

    local mount_point=""
    cleanup_mount() {
      if [[ -n "$mount_point" ]]; then
        hdiutil detach "$mount_point" >/dev/null 2>&1 || true
      fi
    }
    trap cleanup_mount RETURN

    mount_point="$(hdiutil attach -nobrowse -readonly "$dmg" | awk '/\\/Volumes\\// {print $NF; exit}')"
    [[ -n "$mount_point" ]] || error "Failed to mount Tor Browser DMG"

    local pt_dir
    pt_dir="$(find "$mount_point" -type d -name 'PluggableTransports' | head -n 1)"
    [[ -n "$pt_dir" ]] || error "PluggableTransports not found in Tor Browser DMG"

    local snowflake_dest="$TOR_PT_DIR/$PLATFORM/snowflake-client"
    local obfs4_dest="$TOR_PT_DIR/$PLATFORM/obfs4proxy"
    cp "$pt_dir/snowflake-client" "$snowflake_dest"
    cp "$pt_dir/obfs4proxy" "$obfs4_dest"
    chmod +x "$snowflake_dest" "$obfs4_dest"
    rm -rf "$tmpdir"
    return 0
  fi

  error "Unsupported platform for Tor Browser pluggable transports: $PLATFORM"
}

log "Fetching Tor/I2P assets for $PLATFORM ($ARCH_LABEL)"
fetch_i2p
fetch_tor_pt
log "Tor/I2P assets downloaded."
