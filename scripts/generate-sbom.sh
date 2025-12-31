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

OUTPUT_DIR="${1:-$PROJECT_ROOT/dist/sbom}"
mkdir -p "$OUTPUT_DIR"

log "Generating SBOM..."

# ============================================================================
# Rust SBOM with cargo auditable
# ============================================================================

log "Generating Rust SBOM..."

cd "$PROJECT_ROOT/crates"

# Check if cargo-auditable is installed
if ! command -v cargo-auditable &> /dev/null; then
    log "Installing cargo-auditable..."
    cargo install cargo-auditable
fi

# Generate auditable build metadata
cargo auditable build --release

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

# Check if syft is installed
if ! command -v syft &> /dev/null; then
    warn "Syft not found. Installing..."
    curl -sSfL https://raw.githubusercontent.com/anchore/syft/main/install.sh | sh -s -- -b /usr/local/bin
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
cat > "$OUTPUT_DIR/SBOM-SUMMARY.md" <<EOF
# Software Bill of Materials (SBOM)

Generated: $(date -u +"%Y-%m-%d %H:%M:%S UTC")
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
        sha256sum "$file" > "$file.sha256"
    fi
done

log "SBOM generation complete!"
log "Output: $OUTPUT_DIR"
log ""
log "Files generated:"
ls -lh "$OUTPUT_DIR"

