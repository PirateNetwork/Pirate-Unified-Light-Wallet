#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
CRATE_DIR="$PROJECT_ROOT/crates/pirate-ffi-native"
HEADER="$CRATE_DIR/pirate_wallet_service.h"
CBINDGEN_VERSION="${CBINDGEN_VERSION:-0.29.3}"
MODE="${1:---write}"

case "$MODE" in
  --write|--check) ;;
  *) echo "Usage: $0 [--write|--check]" >&2; exit 64 ;;
esac

if ! command -v cbindgen >/dev/null 2>&1; then
  echo "cbindgen $CBINDGEN_VERSION is required." >&2
  echo "Install it with: cargo install cbindgen --locked --version $CBINDGEN_VERSION" >&2
  exit 1
fi

actual_version="$(cbindgen --version | awk '{print $2}')"
if [[ "$actual_version" != "$CBINDGEN_VERSION" ]]; then
  echo "Expected cbindgen $CBINDGEN_VERSION, found $actual_version." >&2
  exit 1
fi

tmpdir="$(mktemp -d)"
trap 'rm -rf "$tmpdir"' EXIT
generated="$tmpdir/pirate_wallet_service.h"

cbindgen "$CRATE_DIR" \
  --config "$CRATE_DIR/cbindgen.toml" \
  --lockfile "$PROJECT_ROOT/crates/Cargo.lock" \
  --only-target-dependencies \
  --output "$generated"
python3 "$SCRIPT_DIR/verify_native_ffi_header.py" \
  --rust-source "$CRATE_DIR/src/lib.rs" \
  --header "$generated"

if [[ "$MODE" == "--check" ]]; then
  if ! cmp -s "$generated" "$HEADER"; then
    echo "Checked-in native FFI header is stale. Regenerate it with:" >&2
    echo "  bash scripts/generate-native-ffi-header.sh --write" >&2
    diff -u "$HEADER" "$generated" || true
    exit 1
  fi
  echo "Checked-in native FFI header matches cbindgen $CBINDGEN_VERSION."
else
  cp "$generated" "$HEADER"
  echo "Updated $HEADER with cbindgen $CBINDGEN_VERSION."
fi
