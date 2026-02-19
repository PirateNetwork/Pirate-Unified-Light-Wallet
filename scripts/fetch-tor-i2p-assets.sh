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

TOR_BROWSER_VERSION="${TOR_BROWSER_VERSION:-15.0.5}"
TOR_BROWSER_BASE_URL="${TOR_BROWSER_BASE_URL:-https://dist.torproject.org/torbrowser/${TOR_BROWSER_VERSION}}"
TOR_BROWSER_LINUX_FILE="${TOR_BROWSER_LINUX_FILE:-tor-browser-linux-x86_64-${TOR_BROWSER_VERSION}.tar.xz}"
TOR_BROWSER_LINUX_SHA256="${TOR_BROWSER_LINUX_SHA256:-403f7845dc5797a3fbb073d5c75d53aa9c8ebc8ceaf18dc53090bdd4fbb23916}"
TOR_BROWSER_MACOS_FILE="${TOR_BROWSER_MACOS_FILE:-tor-browser-macos-${TOR_BROWSER_VERSION}.dmg}"
TOR_BROWSER_MACOS_SHA256="${TOR_BROWSER_MACOS_SHA256:-8462a5dfa81bd86b2dec8ee1825c4d43592f033a1cdf9aa835dcf1a1b03bcda1}"

I2PD_VERSION_DEFAULT="2.59.0"
I2PD_VERSION="${I2PD_VERSION:-$I2PD_VERSION_DEFAULT}"
I2PD_BASE_URL="${I2PD_BASE_URL:-https://github.com/PurpleI2P/i2pd/releases/download/$I2PD_VERSION}"
I2PD_LINUX_AMD64_SHA512="${I2PD_LINUX_AMD64_SHA512:-bec531e3d97b6b397c936438c368677d34a29e1fc8268a67fb1b10675e011a4144d999150eb62fe6f73ff393a3d80dee38c7edea27a9a88030355a7d5ac4b635}"
I2PD_LINUX_ARM64_SHA512="${I2PD_LINUX_ARM64_SHA512:-dcd146b91441d161448fe77300d4806eecf01e5279d1159992065736221ec7bdef67f8aec8bb5552c4e99277e7f9f7afcfab49d3949c49bceef2807395075527}"
I2PD_MACOS_SHA512="${I2PD_MACOS_SHA512:-1c3c82b42d134b01fedad1c465cb6c6d04af70df9417c44441ff421779017a412b6537c4f23a2334daa2a121f4dd71344bfc4f91fea7cd455e2144914134500c}"

TOR_PT_SOURCE="${TOR_PT_SOURCE:-auto}"
SNOWFLAKE_REPO_URL="${SNOWFLAKE_REPO_URL:-https://gitlab.torproject.org/tpo/anti-censorship/pluggable-transports/snowflake.git}"
SNOWFLAKE_REF="${SNOWFLAKE_REF:-v2.11.0}"
SNOWFLAKE_COMMIT="${SNOWFLAKE_COMMIT:-6472bd86cdd5d13fe61dc851edcf83b03df7bda1}"
OBFS4_REPO_URL="${OBFS4_REPO_URL:-https://gitlab.com/yawning/obfs4.git}"
OBFS4_REF="${OBFS4_REF:-obfs4proxy-0.0.14}"
OBFS4_COMMIT="${OBFS4_COMMIT:-336a71d6e4cfd2d33e9c57797828007ad74975e9}"
SNOWFLAKE_GO_PACKAGE="${SNOWFLAKE_GO_PACKAGE:-gitlab.torproject.org/tpo/anti-censorship/pluggable-transports/snowflake/v2/client}"
SNOWFLAKE_GO_VERSION="${SNOWFLAKE_GO_VERSION:-$SNOWFLAKE_REF}"
OBFS4_GO_PACKAGE="${OBFS4_GO_PACKAGE:-gitlab.com/yawning/obfs4.git/obfs4proxy}"
OBFS4_GO_VERSION="${OBFS4_GO_VERSION:-$OBFS4_COMMIT}"
TOR_PT_GO_INSTALL_FALLBACK="${TOR_PT_GO_INSTALL_FALLBACK:-1}"
FETCH_RETRY_ATTEMPTS="${FETCH_RETRY_ATTEMPTS:-4}"
FETCH_RETRY_DELAY_SECONDS="${FETCH_RETRY_DELAY_SECONDS:-3}"

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

retry_cmd() {
  local attempts="$1"
  local delay="$2"
  local description="$3"
  shift 3

  local try=1
  local current_delay="$delay"
  while true; do
    if "$@"; then
      return 0
    fi
    if (( try >= attempts )); then
      warn "$description failed after ${attempts} attempts."
      return 1
    fi
    warn "$description failed (attempt ${try}/${attempts}); retrying in ${current_delay}s..."
    sleep "$current_delay"
    try=$((try + 1))
    current_delay=$((current_delay * 2))
  done
}

clone_repo() {
  local url="$1"
  local dest="$2"
  rm -rf "$dest"
  git clone "$url" "$dest"
}

normalize_mode() {
  echo "$1" | tr '[:upper:]' '[:lower:]'
}

tor_pt_source_enabled() {
  local mode
  mode="$(normalize_mode "$TOR_PT_SOURCE")"
  case "$mode" in
    1|true|yes|on|auto|only) return 0 ;;
    *) return 1 ;;
  esac
}

tor_pt_source_only() {
  local mode
  mode="$(normalize_mode "$TOR_PT_SOURCE")"
  case "$mode" in
    only) return 0 ;;
    *) return 1 ;;
  esac
}

resolve_path() {
  local path="$1"
  if [[ -L "$path" ]]; then
    if have_cmd realpath; then
      realpath "$path"
      return 0
    fi
    if have_cmd readlink; then
      readlink -f "$path"
      return 0
    fi
  fi
  echo "$path"
}

find_pt_binary() {
  local root="$1"
  local name="$2"
  find "$root" \( -type f -o -type l \) -iname "${name}*" | head -n 1
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

ensure_repo() {
  local url="$1"
  local ref="$2"
  local commit="$3"
  local dest="$4"

  if [[ ! -d "$dest/.git" ]]; then
    log "Cloning $url"
    if ! retry_cmd "$FETCH_RETRY_ATTEMPTS" "$FETCH_RETRY_DELAY_SECONDS" "clone $url" clone_repo "$url" "$dest"; then
      warn "Failed to clone $url"
      return 1
    fi
  fi
  if ! (cd "$dest" && retry_cmd "$FETCH_RETRY_ATTEMPTS" "$FETCH_RETRY_DELAY_SECONDS" "fetch $commit from $url" env GIT_TERMINAL_PROMPT=0 git fetch --depth 1 origin "$commit" >/dev/null 2>&1); then
    warn "Failed to fetch $commit from $url"
    return 1
  fi
  if ! (cd "$dest" && GIT_TERMINAL_PROMPT=0 git checkout -q "$commit"); then
    warn "Failed to checkout $commit in $dest"
    return 1
  fi
  local head
  head="$(cd "$dest" && git rev-parse HEAD)"
  if [[ "$head" != "$commit" ]]; then
    warn "Expected $ref ($commit) but found $head in $dest"
    return 1
  fi
}

go_arch_label() {
  case "$1" in
    amd64) echo "x86_64" ;;
    arm64) echo "aarch64" ;;
    *) echo "$1" ;;
  esac
}

go_build_pt() {
  local repo="$1"
  local pkg="$2"
  local goos="$3"
  local goarch="$4"
  local output="$5"

  log "Building $(basename "$output") (GOOS=$goos GOARCH=$goarch)"
  if ! (cd "$repo" && \
    CGO_ENABLED=0 GOWORK=off GOOS="$goos" GOARCH="$goarch" \
    go build -mod=readonly -buildvcs=false -trimpath -ldflags "-s -w -buildid=" \
    -o "$output" "$pkg"); then
    return 1
  fi
  [[ -f "$output" ]] || return 1
  chmod +x "$output" || return 1
}

go_install_pt_binary() {
  local package="$1"
  local version="$2"
  local binary_name="$3"
  local goos="$4"
  local goarch="$5"
  local output="$6"

  local tmpdir
  tmpdir="$(mktemp -d)"
  local gobin="$tmpdir/bin"
  mkdir -p "$gobin"

  if ! (cd "$tmpdir" && \
    GOBIN="$gobin" CGO_ENABLED=0 GOWORK=off GOOS="$goos" GOARCH="$goarch" \
    go install -trimpath -buildvcs=false -ldflags "-s -w -buildid=" \
    "${package}@${version}"); then
    rm -rf "$tmpdir"
    return 1
  fi

  local built="$gobin/$binary_name"
  if [[ ! -f "$built" && -f "$gobin/${binary_name}.exe" ]]; then
    built="$gobin/${binary_name}.exe"
  fi
  [[ -f "$built" ]] || {
    warn "Expected $binary_name from ${package}@${version}, but no binary was produced."
    rm -rf "$tmpdir"
    return 1
  }

  cp "$built" "$output"
  chmod +x "$output"
  rm -rf "$tmpdir"
}

build_tor_pt_via_go_install() {
  if [[ "$TOR_PT_GO_INSTALL_FALLBACK" == "0" ]]; then
    return 1
  fi
  if ! have_cmd go; then
    return 1
  fi

  mkdir -p "$TOR_PT_DIR/$PLATFORM"

  if [[ "$PLATFORM" == "linux" ]]; then
    local goarch
    case "$ARCH_LABEL" in
      x86_64) goarch="amd64" ;;
      aarch64) goarch="arm64" ;;
      *) error "Unsupported arch for PT build: $ARCH_LABEL" ;;
    esac
    local label
    label="$(go_arch_label "$goarch")"
    log "Building Tor pluggable transports via Go module fallback (linux/$label)"
    if ! go_install_pt_binary "$SNOWFLAKE_GO_PACKAGE" "$SNOWFLAKE_GO_VERSION" "client" "linux" "$goarch" "$TOR_PT_DIR/$PLATFORM/snowflake-client-$label"; then
      return 1
    fi
    if ! go_install_pt_binary "$OBFS4_GO_PACKAGE" "$OBFS4_GO_VERSION" "obfs4proxy" "linux" "$goarch" "$TOR_PT_DIR/$PLATFORM/obfs4proxy-$label"; then
      return 1
    fi
    return 0
  fi

  if [[ "$PLATFORM" == "macos" ]]; then
    local goarch
    for goarch in amd64 arm64; do
      local label
      label="$(go_arch_label "$goarch")"
      log "Building Tor pluggable transports via Go module fallback (darwin/$label)"
      if ! go_install_pt_binary "$SNOWFLAKE_GO_PACKAGE" "$SNOWFLAKE_GO_VERSION" "client" "darwin" "$goarch" "$TOR_PT_DIR/$PLATFORM/snowflake-client-$label"; then
        return 1
      fi
      if ! go_install_pt_binary "$OBFS4_GO_PACKAGE" "$OBFS4_GO_VERSION" "obfs4proxy" "darwin" "$goarch" "$TOR_PT_DIR/$PLATFORM/obfs4proxy-$label"; then
        return 1
      fi
    done
    return 0
  fi

  return 1
}

build_tor_pt_from_source() {
  if ! tor_pt_source_enabled; then
    return 1
  fi
  if ! have_cmd go; then
    warn "Go not found; skipping Tor PT source build."
    return 1
  fi
  if ! have_cmd git; then
    warn "Git not found; skipping Tor PT source build."
    return 1
  fi

  local build_root="$TOR_PT_DIR/.build"
  local snowflake_repo="$build_root/snowflake"
  local obfs4_repo="$build_root/obfs4"

  mkdir -p "$TOR_PT_DIR/$PLATFORM"
  mkdir -p "$build_root"

  if ! ensure_repo "$SNOWFLAKE_REPO_URL" "$SNOWFLAKE_REF" "$SNOWFLAKE_COMMIT" "$snowflake_repo"; then
    return 1
  fi
  if ! ensure_repo "$OBFS4_REPO_URL" "$OBFS4_REF" "$OBFS4_COMMIT" "$obfs4_repo"; then
    return 1
  fi

  if [[ "$PLATFORM" == "linux" ]]; then
    local goarch
    case "$ARCH_LABEL" in
      x86_64) goarch="amd64" ;;
      aarch64) goarch="arm64" ;;
      *) error "Unsupported arch for PT build: $ARCH_LABEL" ;;
    esac
    local label
    label="$(go_arch_label "$goarch")"
    if ! go_build_pt "$snowflake_repo" "./client" "linux" "$goarch" "$TOR_PT_DIR/$PLATFORM/snowflake-client-$label"; then
      return 1
    fi
    if ! go_build_pt "$obfs4_repo" "./obfs4proxy" "linux" "$goarch" "$TOR_PT_DIR/$PLATFORM/obfs4proxy-$label"; then
      return 1
    fi
    return 0
  fi

  if [[ "$PLATFORM" == "macos" ]]; then
    local goarch
    for goarch in amd64 arm64; do
      local label
      label="$(go_arch_label "$goarch")"
      if ! go_build_pt "$snowflake_repo" "./client" "darwin" "$goarch" "$TOR_PT_DIR/$PLATFORM/snowflake-client-$label"; then
        return 1
      fi
      if ! go_build_pt "$obfs4_repo" "./obfs4proxy" "darwin" "$goarch" "$TOR_PT_DIR/$PLATFORM/obfs4proxy-$label"; then
        return 1
      fi
    done
    return 0
  fi

  return 1
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

    # Extract once, then store as universal (i2pd) or arch-specific (i2pd-x86_64 / i2pd-aarch64).
    local extracted="$tmpdir/i2pd"
    extract_archive_or_copy "$archive" "i2pd" "$extracted" "$tmpdir/unpack"
    chmod +x "$extracted"

    local dest_name="i2pd"
    if have_cmd lipo; then
      local archs
      archs="$(lipo -archs "$extracted" 2>/dev/null || true)"
      case "$archs" in
        *x86_64*arm64*|*arm64*x86_64*) dest_name="i2pd" ;;
        *x86_64*) dest_name="i2pd-x86_64" ;;
        *arm64*) dest_name="i2pd-aarch64" ;;
        *) dest_name="i2pd" ;;
      esac
    else
      # Best-effort default: treat the upstream tarball as Intel-only.
      dest_name="i2pd-x86_64"
    fi

    # Avoid shipping a mismatched generic i2pd in universal builds.
    if [[ "$dest_name" != "i2pd" ]]; then
      rm -f "$I2P_DIR/macos/i2pd"
    else
      rm -f "$I2P_DIR/macos/i2pd-x86_64" "$I2P_DIR/macos/i2pd-aarch64"
    fi

    cp "$extracted" "$I2P_DIR/macos/$dest_name"
    chmod +x "$I2P_DIR/macos/$dest_name"
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
  if build_tor_pt_from_source; then
    log "Tor pluggable transports built from source."
    return 0
  fi
  if build_tor_pt_via_go_install; then
    log "Tor pluggable transports built via Go module fallback."
    return 0
  fi
  if tor_pt_source_only; then
    error "Tor PT source build requested but failed. Check upstream availability or set TOR_PT_SOURCE=off."
  fi

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

    local snowflake_dest="$TOR_PT_DIR/$PLATFORM/snowflake-client"
    local obfs4_dest="$TOR_PT_DIR/$PLATFORM/obfs4proxy"
    local pt_dir snowflake_src obfs4_src
    pt_dir="$(find "$extract_dir" -type d -name 'PluggableTransports' | head -n 1)"
    if [[ -n "$pt_dir" ]]; then
      snowflake_src="$pt_dir/snowflake-client"
      obfs4_src="$pt_dir/obfs4proxy"
    fi

    if [[ ! -f "${snowflake_src:-}" || ! -f "${obfs4_src:-}" ]]; then
      snowflake_src="$(find_pt_binary "$extract_dir" "snowflake-client")"
      obfs4_src="$(find_pt_binary "$extract_dir" "obfs4proxy")"
    fi

    [[ -f "${snowflake_src:-}" ]] || error "snowflake-client not found in Tor Browser bundle"
    [[ -f "${obfs4_src:-}" ]] || error "obfs4proxy not found in Tor Browser bundle"

    snowflake_src="$(resolve_path "$snowflake_src")"
    obfs4_src="$(resolve_path "$obfs4_src")"
    cp "$snowflake_src" "$snowflake_dest"
    cp "$obfs4_src" "$obfs4_dest"
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

    mount_point="$(hdiutil attach -nobrowse -readonly "$dmg" | awk -F '\t' '/\/Volumes\// {print $NF; exit}')"
    [[ -n "$mount_point" ]] || error "Failed to mount Tor Browser DMG"

    local snowflake_dest="$TOR_PT_DIR/$PLATFORM/snowflake-client"
    local obfs4_dest="$TOR_PT_DIR/$PLATFORM/obfs4proxy"
    local pt_dir snowflake_src obfs4_src
    pt_dir="$(find "$mount_point" -type d -name 'PluggableTransports' | head -n 1)"
    if [[ -n "$pt_dir" ]]; then
      snowflake_src="$pt_dir/snowflake-client"
      obfs4_src="$pt_dir/obfs4proxy"
    fi

    if [[ ! -f "${snowflake_src:-}" || ! -f "${obfs4_src:-}" ]]; then
      snowflake_src="$(find_pt_binary "$mount_point" "snowflake-client")"
      obfs4_src="$(find_pt_binary "$mount_point" "obfs4proxy")"
    fi

    [[ -f "${snowflake_src:-}" ]] || error "snowflake-client not found in Tor Browser DMG"
    [[ -f "${obfs4_src:-}" ]] || error "obfs4proxy not found in Tor Browser DMG"

    snowflake_src="$(resolve_path "$snowflake_src")"
    obfs4_src="$(resolve_path "$obfs4_src")"
    cp "$snowflake_src" "$snowflake_dest"
    cp "$obfs4_src" "$obfs4_dest"
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
