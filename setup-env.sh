#!/bin/bash
# Source this script to set up the development environment for Pirate Unified Wallet
# Usage: source setup-env.sh

[ -f "$HOME/.cargo/env" ] && . "$HOME/.cargo/env"
export PATH="$HOME/apps/flutter/bin:$HOME/.cargo/bin:$HOME/.local/bin:$HOME/Android/Sdk/cmdline-tools/latest/bin:$PATH"

echo "Environment set up!"
echo "Rust: $(rustc --version)"
echo "Rustup: $(rustup --version | head -n 1)"
echo "Flutter: $(flutter --version | head -n 1)"
echo "FRB Codegen: $(flutter_rust_bridge_codegen --version)"
echo "Protoc: $(protoc --version)"
