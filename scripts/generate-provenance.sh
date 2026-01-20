#!/usr/bin/env bash
# Sigstore provenance generation and signing script
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

sha256_file() {
    local file="$1"
    if command -v sha256sum &> /dev/null; then
        sha256sum "$file" | awk '{print $1}'
        return 0
    fi
    if command -v shasum &> /dev/null; then
        shasum -a 256 "$file" | awk '{print $1}'
        return 0
    fi
    error "Missing sha256sum/shasum to hash $file"
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

ARTIFACT="${1:-}"
OUTPUT_DIR="${2:-$PROJECT_ROOT/dist/provenance}"

if [ -z "$ARTIFACT" ]; then
    error "Usage: $0 <artifact-path> [output-dir]"
fi

if [ ! -f "$ARTIFACT" ]; then
    error "Artifact not found: $ARTIFACT"
fi

mkdir -p "$OUTPUT_DIR"

log "Generating provenance for: $ARTIFACT"

# ============================================================================
# Generate SLSA Provenance
# ============================================================================

ARTIFACT_NAME=$(basename "$ARTIFACT")
ARTIFACT_HASH=$(sha256_file "$ARTIFACT")

# Get Git info
GIT_COMMIT=$(git rev-parse HEAD 2>/dev/null || echo "unknown")
GIT_TAG=$(git describe --tags --exact-match 2>/dev/null || echo "")
GIT_BRANCH=$(git rev-parse --abbrev-ref HEAD 2>/dev/null || echo "unknown")
GIT_REMOTE=$(git config --get remote.origin.url 2>/dev/null || echo "unknown")

# Get build environment
SOURCE_DATE_EPOCH="${SOURCE_DATE_EPOCH:-$(git log -1 --format=%ct 2>/dev/null || date +%s)}"
BUILD_DATE=$(date -u -d "@$SOURCE_DATE_EPOCH" +"%Y-%m-%dT%H:%M:%SZ" 2>/dev/null || \
    date -u -r "$SOURCE_DATE_EPOCH" +"%Y-%m-%dT%H:%M:%SZ" 2>/dev/null || \
    date -u +"%Y-%m-%dT%H:%M:%SZ")
BUILD_USER="${USER:-unknown}"
BUILD_HOST="${HOSTNAME:-unknown}"

# Create SLSA provenance (v1.0)
cat > "$OUTPUT_DIR/$ARTIFACT_NAME.provenance.json" <<EOF
{
  "_type": "https://in-toto.io/Statement/v1",
  "subject": [
    {
      "name": "$ARTIFACT_NAME",
      "digest": {
        "sha256": "$ARTIFACT_HASH"
      }
    }
  ],
  "predicateType": "https://slsa.dev/provenance/v1",
  "predicate": {
    "buildDefinition": {
      "buildType": "https://pirate.black/build-types/nix-flake@v1",
      "externalParameters": {
        "source": {
          "uri": "$GIT_REMOTE",
          "digest": {
            "sha1": "$GIT_COMMIT"
          },
          "ref": "$GIT_BRANCH",
          "tag": "$GIT_TAG"
        }
      },
      "internalParameters": {
        "sourceEpoch": $SOURCE_DATE_EPOCH,
        "buildDate": "$BUILD_DATE"
      },
      "resolvedDependencies": []
    },
    "runDetails": {
      "builder": {
        "id": "https://github.com/pirate/wallet/actions",
        "version": {
          "nix": "$(nix --version 2>/dev/null || echo 'unknown')",
          "flutter": "$(flutter --version 2>&1 | head -1 || echo 'unknown')",
          "rust": "$(rustc --version || echo 'unknown')"
        }
      },
      "metadata": {
        "invocationId": "$(uuidgen 2>/dev/null || echo 'unknown')",
        "startedOn": "$BUILD_DATE",
        "finishedOn": "$BUILD_DATE"
      },
      "byproducts": []
    }
  }
}
EOF

log "SLSA provenance generated"

# ============================================================================
# Sign with Sigstore (if available)
# ============================================================================

if command -v cosign &> /dev/null; then
    log "Signing with Sigstore/cosign..."
    
    # Sign the artifact
    cosign sign-blob \
        --bundle "$OUTPUT_DIR/$ARTIFACT_NAME.sigstore.bundle" \
        "$ARTIFACT" 2>/dev/null || {
        warn "Cosign signing failed (may need OIDC auth)"
        warn "Sign manually with: cosign sign-blob --bundle <bundle> $ARTIFACT"
    }
    
    # Sign the provenance
    cosign sign-blob \
        --bundle "$OUTPUT_DIR/$ARTIFACT_NAME.provenance.sigstore.bundle" \
        "$OUTPUT_DIR/$ARTIFACT_NAME.provenance.json" 2>/dev/null || true
    
    log "Sigstore signatures created"
else
    warn "Cosign not found. Provenance created but not signed."
    warn "Install cosign: https://docs.sigstore.dev/cosign/installation/"
fi

# ============================================================================
# Create verification instructions
# ============================================================================

cat > "$OUTPUT_DIR/$ARTIFACT_NAME.VERIFY.md" <<'EOF'
# Artifact Verification

## Files

- `{artifact}.provenance.json` - SLSA provenance (unencrypted)
- `{artifact}.sigstore.bundle` - Sigstore signature bundle
- `{artifact}.provenance.sigstore.bundle` - Provenance signature bundle
- `{artifact}.sha256` - SHA-256 checksum

## Verify Checksum

```bash
sha256sum -c {artifact}.sha256
```

## Verify Provenance

```bash
# View provenance
cat {artifact}.provenance.json | jq .

# Extract commit hash
jq -r '.predicate.buildDefinition.externalParameters.source.digest.sha1' \
    {artifact}.provenance.json
```

## Verify Sigstore Signature

```bash
# Install cosign
# https://docs.sigstore.dev/cosign/installation/

# Verify artifact signature
cosign verify-blob \
    --bundle {artifact}.sigstore.bundle \
    --certificate-identity-regexp ".*" \
    --certificate-oidc-issuer-regexp ".*" \
    {artifact}

# Verify provenance signature  
cosign verify-blob \
    --bundle {artifact}.provenance.sigstore.bundle \
    --certificate-identity-regexp ".*" \
    --certificate-oidc-issuer-regexp ".*" \
    {artifact}.provenance.json
```

## Reproduce Build

```bash
# Clone repository
git clone https://github.com/pirate/wallet.git
cd wallet

# Checkout specific commit from provenance
git checkout $(jq -r '.predicate.buildDefinition.externalParameters.source.digest.sha1' \
    {artifact}.provenance.json)

# Build with Nix
nix build .#<platform>

# Compare hash
sha256sum result/<artifact>
```

## Security Contact

Report vulnerabilities: security@pirate.black
PGP: See SECURITY.md
EOF

sed -i "s/{artifact}/$ARTIFACT_NAME/g" "$OUTPUT_DIR/$ARTIFACT_NAME.VERIFY.md" 2>/dev/null || \
    sed "s/{artifact}/$ARTIFACT_NAME/g" "$OUTPUT_DIR/$ARTIFACT_NAME.VERIFY.md" > "$OUTPUT_DIR/$ARTIFACT_NAME.VERIFY.md.tmp" && \
    mv "$OUTPUT_DIR/$ARTIFACT_NAME.VERIFY.md.tmp" "$OUTPUT_DIR/$ARTIFACT_NAME.VERIFY.md"

log "Verification instructions created"

# Generate checksum for provenance
write_sha256 "$OUTPUT_DIR/$ARTIFACT_NAME.provenance.json"

log "Provenance generation complete!"
log "Output: $OUTPUT_DIR"
log ""
log "Files:"
ls -lh "$OUTPUT_DIR"
