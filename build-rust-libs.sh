#!/bin/bash
# Build Rust libraries for Android (use in WSL/Linux)
# This script automates building the Rust FFI libraries for Android.
# Prefer build-android-wsl.sh when running inside WSL.

set -e  # Exit on error

# Colors for output
BLUE='\033[0;34m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
RED='\033[0;31m'
NC='\033[0m' # No Color

# Get project root
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$SCRIPT_DIR"
CRATES_DIR="$PROJECT_ROOT/crates"
APP_DIR="$PROJECT_ROOT/app"
export CARGO_INCREMENTAL=0

# Check for cargo
if ! command -v cargo &> /dev/null; then
    echo -e "${RED}ERROR: Cargo not found${NC}"
    echo "   Please install Rust from https://rustup.rs"
    exit 1
fi

if command -v rustup &> /dev/null; then
    rustup target add aarch64-linux-android armv7-linux-androideabi x86_64-linux-android
fi

# Check for Android NDK
if [ -z "$ANDROID_NDK_HOME" ] && [ -z "$ANDROID_NDK_ROOT" ]; then
    echo -e "${YELLOW}WARN: Android NDK not found in environment${NC}"
    echo "   Set ANDROID_NDK_HOME or ANDROID_NDK_ROOT"
fi

echo -e "${BLUE}Building Rust libraries for Android...${NC}"

# Android architectures
ARCHS=("aarch64-linux-android" "armv7-linux-androideabi" "x86_64-linux-android")
ABIS=("arm64-v8a" "armeabi-v7a" "x86_64")

cd "$CRATES_DIR"

if command -v cargo-ndk &> /dev/null; then
    echo -e "${BLUE}Using cargo-ndk for multi-ABI build...${NC}"
    cargo ndk \
        -t arm64-v8a \
        -t armeabi-v7a \
        -t x86_64 \
        -o "$APP_DIR/android/app/src/main/jniLibs" \
        build --release -p pirate-ffi-frb --features frb --no-default-features --locked
    echo -e "${GREEN}OK. Android build complete!${NC}"
    exit 0
fi

for i in "${!ARCHS[@]}"; do
    ARCH="${ARCHS[$i]}"
    ABI="${ABIS[$i]}"

    echo -e "${BLUE}Building for $ARCH ($ABI)...${NC}"

    cargo build --release --target "$ARCH" --package pirate-ffi-frb --features frb --no-default-features --locked

    if [ $? -eq 0 ]; then
        SO_PATH="$CRATES_DIR/target/$ARCH/release/libpirate_ffi_frb.so"
        DEST_DIR="$APP_DIR/android/app/src/main/jniLibs/$ABI"

        if [ -f "$SO_PATH" ]; then
            mkdir -p "$DEST_DIR"
            cp "$SO_PATH" "$DEST_DIR/"
            echo -e "${GREEN}OK. $ABI library built and copied${NC}"
        else
            echo -e "${RED}ERROR: Library not found at: $SO_PATH${NC}"
        fi
    else
        echo -e "${RED}ERROR: Build failed for $ARCH${NC}"
        exit 1
    fi
done

echo -e "${GREEN}OK. Android build complete!${NC}"
