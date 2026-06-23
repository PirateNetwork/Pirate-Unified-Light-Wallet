#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
CRATES_DIR="$PROJECT_ROOT/crates"
TARGET="${QORTAL_JNI_TARGET:-}"
DIST_DIR="${QORTAL_JNI_DIST_DIR:-$PROJECT_ROOT/dist/qortal-jni}"
CARGO_BIN="${CARGO:-cargo}"

if [ -d "/c/Strawberry/perl/bin" ]; then
  export PATH="/c/Strawberry/perl/bin:/c/Strawberry/c/bin:$PATH"
  export PERL="/c/Strawberry/perl/bin/perl"
fi

case "$TARGET" in
  x86_64-unknown-linux-gnu)
    source_name="librust.so"
    output_name="librust-linux-x86_64.so"
    ;;
  aarch64-unknown-linux-gnu)
    source_name="librust.so"
    output_name="librust-linux-aarch64.so"
    ;;
  x86_64-apple-darwin)
    source_name="librust.dylib"
    output_name="librust-macos-x86_64.dylib"
    ;;
  aarch64-apple-darwin)
    source_name="librust.dylib"
    output_name="librust-macos-aarch64.dylib"
    ;;
  x86_64-pc-windows-msvc)
    source_name="rust.dll"
    output_name="librust-windows-x86_64.dll"
    ;;
  "")
    case "$(uname -s)-$(uname -m)" in
      Linux-x86_64) TARGET="x86_64-unknown-linux-gnu" ;;
      Linux-aarch64|Linux-arm64) TARGET="aarch64-unknown-linux-gnu" ;;
      Darwin-x86_64) TARGET="x86_64-apple-darwin" ;;
      Darwin-arm64) TARGET="aarch64-apple-darwin" ;;
      MINGW*-x86_64|MSYS*-x86_64|CYGWIN*-x86_64) TARGET="x86_64-pc-windows-msvc" ;;
      *) echo "Unsupported Qortal JNI host: $(uname -s)-$(uname -m)" >&2; exit 1 ;;
    esac
    exec env QORTAL_JNI_TARGET="$TARGET" QORTAL_JNI_DIST_DIR="$DIST_DIR" "$0"
    ;;
  *)
    echo "Unsupported Qortal JNI target: $TARGET" >&2
    exit 1
    ;;
esac

cd "$CRATES_DIR"
"$CARGO_BIN" build --release --locked --target "$TARGET" -p pirate-qortal-jni

source_path="$CRATES_DIR/target/$TARGET/release/$source_name"
if [ ! -f "$source_path" ]; then
  echo "Expected Qortal JNI library was not produced: $source_path" >&2
  exit 1
fi

case "$DIST_DIR" in
  ""|"/"|"$PROJECT_ROOT"|"$CRATES_DIR")
    echo "Refusing to clean unsafe Qortal JNI dist directory: $DIST_DIR" >&2
    exit 1
    ;;
esac

rm -rf "$DIST_DIR"
mkdir -p "$DIST_DIR"
cp -f "$source_path" "$DIST_DIR/$output_name"
cp -R "$PROJECT_ROOT/bindings/qortal-jni/src" "$DIST_DIR/"
cp -f "$PROJECT_ROOT/docs/qortal-handoff.md" "$DIST_DIR/"
cp -f "$PROJECT_ROOT/LICENSE-MIT" "$DIST_DIR/LICENSE-qortal-jni.txt"
echo "$DIST_DIR/$output_name"
