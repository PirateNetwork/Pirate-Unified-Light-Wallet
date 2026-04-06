#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
CRATE_DIR="$PROJECT_ROOT/crates/pirate-ffi-native"

cd "$PROJECT_ROOT/crates"
cargo build --release -p pirate-ffi-native

if command -v cbindgen >/dev/null 2>&1; then
  cbindgen "$CRATE_DIR" --config "$CRATE_DIR/cbindgen.toml" --output "$CRATE_DIR/pirate_wallet_service.h"
else
  echo "cbindgen not installed; using checked-in header at $CRATE_DIR/pirate_wallet_service.h" >&2
fi
