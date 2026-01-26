#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
CRATES_DIR="$PROJECT_ROOT/crates"
JNI_DIR="$PROJECT_ROOT/app/android/app/src/main/jniLibs"

GREEN='\033[0;32m'
RED='\033[0;31m'
YELLOW='\033[1;33m'
NC='\033[0m'

log() {
    echo -e "${GREEN}[$(date +'%Y-%m-%d %H:%M:%S')]${NC} $1"
}

warn() {
    echo -e "${YELLOW}[WARN]${NC} $1"
}

error() {
    echo -e "${RED}[ERROR]${NC} $1" >&2
    exit 1
}

resolve_ndk() {
    if [[ -n "${ANDROID_NDK_HOME:-}" ]]; then
        echo "$ANDROID_NDK_HOME"
        return 0
    fi
    if [[ -n "${ANDROID_NDK_ROOT:-}" ]]; then
        echo "$ANDROID_NDK_ROOT"
        return 0
    fi
    local sdk="${ANDROID_SDK_ROOT:-${ANDROID_HOME:-}}"
    if [[ -z "$sdk" ]]; then
        return 1
    fi
    if [[ -n "${ANDROID_NDK_VERSION:-}" && -d "$sdk/ndk/$ANDROID_NDK_VERSION" ]]; then
        echo "$sdk/ndk/$ANDROID_NDK_VERSION"
        return 0
    fi
    if [[ -d "$sdk/ndk" ]]; then
        ls -d "$sdk/ndk"/* 2>/dev/null | sort -r | head -n 1
        return 0
    fi
    if [[ -d "$sdk/ndk-bundle" ]]; then
        echo "$sdk/ndk-bundle"
        return 0
    fi
    return 1
}

NDK_HOME="$(resolve_ndk || true)"
if [[ -z "$NDK_HOME" ]]; then
    error "Android NDK not found. Set ANDROID_NDK_HOME or install NDK via sdkmanager."
fi

HOST_TAG="linux-x86_64"
if [[ "$(uname -s)" == "Darwin" ]]; then
    if [[ "$(uname -m)" == "arm64" ]]; then
        HOST_TAG="darwin-arm64"
    else
        HOST_TAG="darwin-x86_64"
    fi
fi

NDK_BIN="$NDK_HOME/toolchains/llvm/prebuilt/$HOST_TAG/bin"
if [[ ! -d "$NDK_BIN" ]]; then
    error "NDK toolchain not found at: $NDK_BIN"
fi

export ANDROID_NDK_HOME="$NDK_HOME"
export ANDROID_NDK_ROOT="$NDK_HOME"
export PATH="$NDK_BIN:$PATH"

if command -v rustup >/dev/null 2>&1; then
    rustup target add aarch64-linux-android armv7-linux-androideabi x86_64-linux-android
fi

export CARGO_TARGET_AARCH64_LINUX_ANDROID_LINKER="$NDK_BIN/aarch64-linux-android21-clang"
export CARGO_TARGET_ARMV7_LINUX_ANDROIDEABI_LINKER="$NDK_BIN/armv7a-linux-androideabi21-clang"
export CARGO_TARGET_X86_64_LINUX_ANDROID_LINKER="$NDK_BIN/x86_64-linux-android21-clang"
export CC_aarch64_linux_android="$NDK_BIN/aarch64-linux-android21-clang"
export CC_armv7_linux_androideabi="$NDK_BIN/armv7a-linux-androideabi21-clang"
export CC_x86_64_linux_android="$NDK_BIN/x86_64-linux-android21-clang"

strip_binary() {
    local bin="$1"
    local strip_tool="$NDK_BIN/llvm-strip"
    if [[ -x "$strip_tool" ]]; then
        "$strip_tool" --strip-unneeded "$bin" || warn "Failed to strip $bin"
    else
        warn "llvm-strip not found at $strip_tool; skipping strip"
    fi
}

build_target() {
    local rust_target="$1"
    local abi="$2"
    log "Building Rust FFI for $rust_target ($abi)..."
    cargo build --release --target "$rust_target" --package pirate-ffi-frb --features frb --no-default-features --locked
    local so_path="$CRATES_DIR/target/$rust_target/release/libpirate_ffi_frb.so"
    if [[ ! -f "$so_path" ]]; then
        error "libpirate_ffi_frb.so not found at $so_path"
    fi
    strip_binary "$so_path"
    local dest_dir="$JNI_DIR/$abi"
    mkdir -p "$dest_dir"
    cp "$so_path" "$dest_dir/"
}

mkdir -p "$JNI_DIR"
cd "$CRATES_DIR"

build_target "aarch64-linux-android" "arm64-v8a"
build_target "armv7-linux-androideabi" "armeabi-v7a"
build_target "x86_64-linux-android" "x86_64"

log "Rust Android libraries ready in $JNI_DIR"
