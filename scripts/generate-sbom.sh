#!/usr/bin/env bash
# SBOM (Software Bill of Materials) generation script
# Uses Syft for Flutter/Dart and cargo auditable for Rust
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"

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

write_sha256() {
    local file="$1"
    if command -v sha256sum &> /dev/null; then
        sha256sum "$file" > "$file.sha256"
        return 0
    fi
    if command -v shasum &> /dev/null; then
        shasum -a 256 "$file" > "$file.sha256"
        return 0
    fi
    error "Missing sha256sum/shasum to write checksum for $file"
}

OUTPUT_DIR="${1:-$PROJECT_ROOT/dist/sbom}"
mkdir -p "$OUTPUT_DIR"

log "Generating SBOM..."

export CARGO_INCREMENTAL=0

SYFT_VERSION="${SYFT_VERSION:-1.40.1}"
SYFT_SHA256_LINUX_AMD64="${SYFT_SHA256_LINUX_AMD64:-c229137c919f22aa926c1c015388db5ec64e99c078e0baac053808e8f36e2e00}"
SYFT_SHA256_DARWIN_ARM64="${SYFT_SHA256_DARWIN_ARM64:-c0f6a4fc0563ef1dfe1acf9a4518db66cb37bbb1391889aba3be773dff3487dd}"
SYFT_SHA256_DARWIN_AMD64="${SYFT_SHA256_DARWIN_AMD64:-9e84d1f152ef9d3bb541cc7cedf81ed4c7ed78f6cc2e4c8f0db9e052b64cd7be}"
SYFT_SHA256_WINDOWS_AMD64="${SYFT_SHA256_WINDOWS_AMD64:-eedac363e277dfecac420b6e4ed0a861bc2c9c84a7544157f52807a99bff07cd}"
CARGO_AUDITABLE_VERSION="${CARGO_AUDITABLE_VERSION:-0.7.2}"

# ============================================================================
# Rust SBOM with cargo auditable
# ============================================================================

log "Generating Rust SBOM..."

cd "$PROJECT_ROOT/crates"

# Check if cargo-auditable is installed
if ! command -v cargo-auditable &> /dev/null; then
    log "Installing cargo-auditable..."
    cargo install cargo-auditable --locked --version "$CARGO_AUDITABLE_VERSION"
fi

# Generate auditable build metadata
cargo auditable build --release --locked

# Extract SBOM from binary
if command -v rust-audit-info &> /dev/null; then
    rust-audit-info "$PROJECT_ROOT/crates/target/release/pirate-ffi-frb" \
        > "$OUTPUT_DIR/rust-sbom.json" 2>/dev/null || true
fi

# Also generate Cargo.lock SBOM
cargo tree --prefix none --edges normal --format "{p}" \
    | sort -u \
    > "$OUTPUT_DIR/rust-dependencies.txt"

# Generate detailed dependency tree
cargo tree --format "{p} {l}" \
    > "$OUTPUT_DIR/rust-dependency-tree.txt"

log "Rust SBOM generated"

# ============================================================================
# Flutter/Dart SBOM with Syft
# ============================================================================

log "Generating Flutter/Dart SBOM..."

cd "$PROJECT_ROOT/app"

# Ensure Dart dependencies are locked
flutter pub get --enforce-lockfile

# Check if syft is installed
if ! command -v syft &> /dev/null; then
    warn "Syft not found. Installing pinned binary..."
    OS="$(uname -s)"
    ARCH="$(uname -m)"
    SYFT_TMP_DIR="$(mktemp -d)"
    SYFT_TGZ=""
    SYFT_URL=""
    SYFT_SHA256=""

    case "$OS" in
        Linux)
            if [[ "$ARCH" != "x86_64" && "$ARCH" != "amd64" ]]; then
                error "Unsupported Linux arch for syft: $ARCH"
            fi
            SYFT_TGZ="syft_${SYFT_VERSION}_linux_amd64.tar.gz"
            SYFT_SHA256="$SYFT_SHA256_LINUX_AMD64"
            ;;
        Darwin)
            if [[ "$ARCH" == "arm64" ]]; then
                SYFT_TGZ="syft_${SYFT_VERSION}_darwin_arm64.tar.gz"
                SYFT_SHA256="$SYFT_SHA256_DARWIN_ARM64"
            else
                SYFT_TGZ="syft_${SYFT_VERSION}_darwin_amd64.tar.gz"
                SYFT_SHA256="$SYFT_SHA256_DARWIN_AMD64"
            fi
            ;;
        MINGW*|MSYS*|CYGWIN*)
            SYFT_TGZ="syft_${SYFT_VERSION}_windows_amd64.zip"
            SYFT_SHA256="$SYFT_SHA256_WINDOWS_AMD64"
            ;;
        *)
            error "Unsupported OS for syft: $OS"
            ;;
    esac

    SYFT_URL="https://github.com/anchore/syft/releases/download/v${SYFT_VERSION}/${SYFT_TGZ}"
    SYFT_ARCHIVE="$SYFT_TMP_DIR/$SYFT_TGZ"

    if command -v curl &> /dev/null; then
        curl -fsSL -o "$SYFT_ARCHIVE" "$SYFT_URL"
    elif command -v wget &> /dev/null; then
        wget -q -O "$SYFT_ARCHIVE" "$SYFT_URL"
    else
        error "Missing download tool: install curl or wget."
    fi

    if command -v sha256sum &> /dev/null; then
        echo "$SYFT_SHA256  $SYFT_ARCHIVE" | sha256sum -c - >/dev/null
    elif command -v shasum &> /dev/null; then
        echo "$SYFT_SHA256  $SYFT_ARCHIVE" | shasum -a 256 -c - >/dev/null
    else
        error "Missing sha256sum/shasum to verify syft download."
    fi

    SYFT_BIN_DIR="$PROJECT_ROOT/.tools/syft"
    mkdir -p "$SYFT_BIN_DIR"
    if [[ "$SYFT_TGZ" == *.zip ]]; then
        if command -v unzip &> /dev/null; then
            unzip -q "$SYFT_ARCHIVE" -d "$SYFT_BIN_DIR"
        else
            powershell.exe -NoProfile -Command "Expand-Archive -Path '$SYFT_ARCHIVE' -DestinationPath '$SYFT_BIN_DIR'" \
                || error "Missing unzip to extract syft."
        fi
    else
        tar -xf "$SYFT_ARCHIVE" -C "$SYFT_BIN_DIR"
    fi
    export PATH="$SYFT_BIN_DIR:$PATH"
    rm -rf "$SYFT_TMP_DIR"
fi

# Generate SBOM for Flutter app
syft "$PROJECT_ROOT/app" \
    --output spdx-json="$OUTPUT_DIR/flutter-sbom.spdx.json" \
    --output cyclonedx-json="$OUTPUT_DIR/flutter-sbom.cdx.json"

# Generate Dart dependency list
flutter pub deps --json > "$OUTPUT_DIR/flutter-dependencies.json"
flutter pub deps --style=compact > "$OUTPUT_DIR/flutter-dependencies.txt"

log "Flutter/Dart SBOM generated"

# ============================================================================
# Combined SBOM
# ============================================================================

log "Creating combined SBOM..."

# Create a combined summary
SBOM_DATE="$(date -u +"%Y-%m-%d %H:%M:%S UTC")"
if [[ -n "${SOURCE_DATE_EPOCH:-}" ]]; then
    SBOM_DATE="$(date -u -d "@$SOURCE_DATE_EPOCH" +"%Y-%m-%d %H:%M:%S UTC" 2>/dev/null || date -u +"%Y-%m-%d %H:%M:%S UTC")"
fi
cat > "$OUTPUT_DIR/SBOM-SUMMARY.md" <<EOF
# Software Bill of Materials (SBOM)

Generated: $SBOM_DATE
Project: Pirate Unified Wallet
Version: 1.0.0

## Files

- \`rust-sbom.json\` - Rust dependencies (cargo-auditable format)
- \`rust-dependencies.txt\` - Rust dependency list
- \`rust-dependency-tree.txt\` - Rust dependency tree with licenses
- \`flutter-sbom.spdx.json\` - Flutter/Dart SBOM (SPDX format)
- \`flutter-sbom.cdx.json\` - Flutter/Dart SBOM (CycloneDX format)
- \`flutter-dependencies.json\` - Flutter dependency details (JSON)
- \`flutter-dependencies.txt\` - Flutter dependency summary

## Rust Dependencies

\`\`\`
$(head -20 "$OUTPUT_DIR/rust-dependencies.txt")
... ($(wc -l < "$OUTPUT_DIR/rust-dependencies.txt") total)
\`\`\`

## Flutter Dependencies

\`\`\`
$(head -20 "$OUTPUT_DIR/flutter-dependencies.txt")
... (see flutter-dependencies.txt for full list)
\`\`\`

## License Summary

### Rust
$(grep -o 'license: [^"]*' "$OUTPUT_DIR/rust-dependency-tree.txt" | sort | uniq -c | sort -rn || echo "N/A")

### Flutter
$(grep -o '"license": "[^"]*"' "$OUTPUT_DIR/flutter-dependencies.json" | sort | uniq -c | sort -rn || echo "N/A")

## Verification

All SBOMs are generated from source code at build time.
Verify authenticity using Sigstore provenance (see provenance.json).

## Security

Run security audit:
\`\`\`bash
# Rust
cd crates && cargo audit

# Flutter
cd app && flutter pub outdated
\`\`\`
EOF

log "Combined SBOM summary created"

# ============================================================================
# Generate checksums
# ============================================================================

log "Generating checksums..."

cd "$OUTPUT_DIR"
for file in *.json *.txt *.md; do
    if [ -f "$file" ]; then
        write_sha256 "$file"
    fi
done

log "SBOM generation complete!"
log "Output: $OUTPUT_DIR"
log ""
log "Files generated:"
ls -lh "$OUTPUT_DIR"
