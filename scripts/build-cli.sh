#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
CRATE_DIR="$PROJECT_ROOT/crates"

if [ -d "/c/Strawberry/perl/bin" ]; then
  export PATH="/c/Strawberry/perl/bin:/c/Strawberry/c/bin:$PATH"
  export PERL="/c/Strawberry/perl/bin/perl"
fi

build_package() {
  local package="$1"
  cargo build --release --locked -p "$package"
}

verify_binary() {
  local path="$1"
  if [ ! -f "$path" ]; then
    echo "Expected CLI binary was not produced: $path" >&2
    exit 1
  fi
}

build_macos_universal_package() {
  local package="$1"

  cargo build --release --locked --target aarch64-apple-darwin -p "$package"
  cargo build --release --locked --target x86_64-apple-darwin -p "$package"

  local arm_bin="$CRATE_DIR/target/aarch64-apple-darwin/release/$package"
  local x86_bin="$CRATE_DIR/target/x86_64-apple-darwin/release/$package"
  local out_bin="$CRATE_DIR/target/release/$package"

  verify_binary "$arm_bin"
  verify_binary "$x86_bin"
  mkdir -p "$(dirname "$out_bin")"
  lipo -create -output "$out_bin" "$arm_bin" "$x86_bin"
  chmod +x "$out_bin"
  lipo -info "$out_bin"
}

cd "$CRATE_DIR"

case "$(uname -s)" in
  Darwin)
    if command -v rustup >/dev/null 2>&1; then
      rustup target add aarch64-apple-darwin x86_64-apple-darwin >/dev/null
    fi
    build_macos_universal_package piratewallet-cli
    build_macos_universal_package pirate-qortal-cli
    ;;
  MINGW*|MSYS*|CYGWIN*)
    build_package piratewallet-cli
    build_package pirate-qortal-cli
    verify_binary "$CRATE_DIR/target/release/piratewallet-cli.exe"
    verify_binary "$CRATE_DIR/target/release/pirate-qortal-cli.exe"
    ;;
  *)
    build_package piratewallet-cli
    build_package pirate-qortal-cli
    verify_binary "$CRATE_DIR/target/release/piratewallet-cli"
    verify_binary "$CRATE_DIR/target/release/pirate-qortal-cli"
    ;;
esac
