#!/usr/bin/env bash
# Preserve the caller's working directory, target, features and rustc arguments.
set -euo pipefail
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
bash "$SCRIPT_DIR/setup-rust-sbom-tools.sh"
exec "$SCRIPT_DIR/../.tools/rust-sbom/bin/cargo-auditable" auditable "$@"
