#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
CRATE_DIR="$PROJECT_ROOT/crates/pirate-ffi-native"

python3 "$SCRIPT_DIR/verify_native_ffi_header.py" \
  --rust-source "$CRATE_DIR/src/lib.rs" \
  --header "$CRATE_DIR/pirate_wallet_service.h"

cd "$PROJECT_ROOT/crates"
cargo build --release --locked -p pirate-ffi-native
