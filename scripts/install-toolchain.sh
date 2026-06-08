#!/usr/bin/env bash
# Automated toolchain installation for Pirate Unified Wallet
# This script installs Rust, Flutter, and other required tools.

set -euo pipefail

# Pinned versions
RUST_VERSION="1.90.0"
FLUTTER_VERSION="3.41.1"
FRB_CODEGEN_VERSION="2.11.1"
PROTOC_VERSION="25.1"
ANDROID_CMDLINE_TOOLS_VERSION="11076708"

echo "=== Installing toolchain for Pirate Unified Wallet ==="

# 1. Install Rust via rustup
if ! command -v rustup &> /dev/null; then
    echo "Installing rustup..."
    curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y --default-toolchain "$RUST_VERSION"
else
    echo "Rustup already installed. Ensuring toolchain $RUST_VERSION..."
    rustup toolchain install "$RUST_VERSION"
    rustup default "$RUST_VERSION"
fi

# Load cargo environment
[ -f "$HOME/.cargo/env" ] && . "$HOME/.cargo/env"

# 2. Install Flutter
mkdir -p "$HOME/apps"
if [ ! -d "$HOME/apps/flutter" ]; then
    echo "Installing Flutter $FLUTTER_VERSION..."
    cd "$HOME/apps"
    curl -O "https://storage.googleapis.com/flutter_infra_release/releases/stable/linux/flutter_linux_$FLUTTER_VERSION-stable.tar.xz"
    tar xf "flutter_linux_$FLUTTER_VERSION-stable.tar.xz"
    rm "flutter_linux_$FLUTTER_VERSION-stable.tar.xz"
    cd -
else
    echo "Flutter already exists at $HOME/apps/flutter"
fi

export PATH="$HOME/apps/flutter/bin:$PATH"

# 3. Install flutter_rust_bridge_codegen
if ! command -v flutter_rust_bridge_codegen &> /dev/null; then
    echo "Installing flutter_rust_bridge_codegen $FRB_CODEGEN_VERSION..."
    mkdir -p "$HOME/.cargo/bin"
    curl -LO "https://github.com/fzyzcjy/flutter_rust_bridge/releases/download/v$FRB_CODEGEN_VERSION/flutter_rust_bridge_codegen-x86_64-unknown-linux-gnu-v$FRB_CODEGEN_VERSION.tgz"
    tar xf "flutter_rust_bridge_codegen-x86_64-unknown-linux-gnu-v$FRB_CODEGEN_VERSION.tgz"
    mv flutter_rust_bridge_codegen "$HOME/.cargo/bin/"
    rm "flutter_rust_bridge_codegen-x86_64-unknown-linux-gnu-v$FRB_CODEGEN_VERSION.tgz"
else
    echo "flutter_rust_bridge_codegen already installed."
fi

# 4. Install protoc
mkdir -p "$HOME/.local/bin"
mkdir -p "$HOME/.local/include"
if ! command -v protoc &> /dev/null; then
    echo "Installing protoc $PROTOC_VERSION..."
    cd "$HOME/apps"
    curl -LO "https://github.com/protocolbuffers/protobuf/releases/download/v$PROTOC_VERSION/protoc-$PROTOC_VERSION-linux-x86_64.zip"
    unzip -o "protoc-$PROTOC_VERSION-linux-x86_64.zip" -d protoc_temp
    mv protoc_temp/bin/protoc "$HOME/.local/bin/"
    cp -r protoc_temp/include/* "$HOME/.local/include/"
    rm -rf protoc_temp "protoc-$PROTOC_VERSION-linux-x86_64.zip"
    cd -
else
    echo "protoc already installed."
fi

# 5. Android Command Line Tools (minimal setup)
if [ ! -d "$HOME/Android/Sdk/cmdline-tools/latest" ]; then
    echo "Installing Android Command Line Tools..."
    mkdir -p "$HOME/Android/Sdk/cmdline-tools"
    cd "$HOME/apps"
    curl -LO "https://dl.google.com/android/repository/commandlinetools-linux-${ANDROID_CMDLINE_TOOLS_VERSION}_latest.zip"
    unzip -o "commandlinetools-linux-${ANDROID_CMDLINE_TOOLS_VERSION}_latest.zip" -d cmdline-tools-temp
    mkdir -p "$HOME/Android/Sdk/cmdline-tools/latest"
    cp -r cmdline-tools-temp/cmdline-tools/* "$HOME/Android/Sdk/cmdline-tools/latest/"
    rm -rf cmdline-tools-temp "commandlinetools-linux-${ANDROID_CMDLINE_TOOLS_VERSION}_latest.zip"
    cd -
else
    echo "Android Command Line Tools already installed."
fi

echo "=== Toolchain installation complete! ==="
echo "Now run: source setup-env.sh"
echo "Then run: flutter doctor --android-licenses"
