#!/usr/bin/env bash
set -euo pipefail

PROJECT_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
CRATES_DIR="$PROJECT_ROOT/crates"
JNI_DIR="$PROJECT_ROOT/app/android/app/src/main/jniLibs"

if [[ -n "${CARGO_HOME:-}" ]]; then
  CARGO_BIN="$(cygpath -u "$CARGO_HOME")/bin"
else
  CARGO_BIN="/c/Users/${USER}/.cargo/bin"
fi
export PATH="$CARGO_BIN:/c/msys64/usr/bin:$PATH"
export MSYS2_ARG_CONV_EXCL="*"

SDK_BASE=""
if [[ -n "${ANDROID_SDK_ROOT:-}" ]]; then
  SDK_BASE="$(cygpath -u "$ANDROID_SDK_ROOT")"
elif [[ -n "${ANDROID_HOME:-}" ]]; then
  SDK_BASE="$(cygpath -u "$ANDROID_HOME")"
else
  SDK_BASE="/c/Users/${USER}/AppData/Local/Android/Sdk"
fi

if [[ -n "${ANDROID_NDK_HOME:-}" ]]; then
  NDK_HOME="$(cygpath -u "$ANDROID_NDK_HOME")"
elif [[ -n "${ANDROID_NDK_ROOT:-}" ]]; then
  NDK_HOME="$(cygpath -u "$ANDROID_NDK_ROOT")"
elif [[ -d "$SDK_BASE/ndk" ]]; then
  NDK_HOME="$(ls -d "$SDK_BASE/ndk"/* | sort -r | head -n 1)"
elif [[ -d "$SDK_BASE/ndk-bundle" ]]; then
  NDK_HOME="$SDK_BASE/ndk-bundle"
else
  echo "Android NDK not found. Set ANDROID_NDK_HOME or ANDROID_SDK_ROOT." >&2
  exit 1
fi

NDK_HOST_TAG="${ANDROID_NDK_HOST_TAG:-windows-x86_64}"
CLANG_SUFFIX="${ANDROID_NDK_CLANG_SUFFIX:-.cmd}"
EXE_SUFFIX="${ANDROID_NDK_EXE_SUFFIX:-.exe}"

NDK_BIN="$NDK_HOME/toolchains/llvm/prebuilt/$NDK_HOST_TAG/bin"
if [[ ! -d "$NDK_BIN" ]]; then
  echo "NDK toolchain not found at: $NDK_BIN" >&2
  exit 1
fi

if [[ "$NDK_HOME" == /* ]]; then
  export ANDROID_NDK_HOME="$(cygpath -w "$NDK_HOME")"
else
  export ANDROID_NDK_HOME="$NDK_HOME"
fi
export ANDROID_NDK_ROOT="$ANDROID_NDK_HOME"

CLANG_CMD_WIN="$(cygpath -m "$NDK_BIN/aarch64-linux-android21-clang${CLANG_SUFFIX}")"
CLANGXX_CMD_WIN="$(cygpath -m "$NDK_BIN/aarch64-linux-android21-clang++${CLANG_SUFFIX}")"
CLANG_EXE_WIN="$(cygpath -m "$NDK_BIN/clang${EXE_SUFFIX}")"
CLANGXX_EXE_WIN="$(cygpath -m "$NDK_BIN/clang++${EXE_SUFFIX}")"
AR_EXE_WIN="$(cygpath -m "$NDK_BIN/llvm-ar${EXE_SUFFIX}")"
RANLIB_EXE_WIN="$(cygpath -m "$NDK_BIN/llvm-ranlib${EXE_SUFFIX}")"

# Windows executables for OpenSSL/cc-rs
export CC_aarch64_linux_android="$CLANG_EXE_WIN"
export CXX_aarch64_linux_android="$CLANGXX_EXE_WIN"
export AR_aarch64_linux_android="$AR_EXE_WIN"
export RANLIB_aarch64_linux_android="$RANLIB_EXE_WIN"
export CFLAGS_aarch64_linux_android="--target=aarch64-linux-android21"
export CXXFLAGS_aarch64_linux_android="--target=aarch64-linux-android21"

export CC_armv7_linux_androideabi="$CLANG_EXE_WIN"
export CXX_armv7_linux_androideabi="$CLANGXX_EXE_WIN"
export AR_armv7_linux_androideabi="$AR_EXE_WIN"
export RANLIB_armv7_linux_androideabi="$RANLIB_EXE_WIN"
export CFLAGS_armv7_linux_androideabi="--target=armv7a-linux-androideabi21"
export CXXFLAGS_armv7_linux_androideabi="--target=armv7a-linux-androideabi21"

export CC_x86_64_linux_android="$CLANG_EXE_WIN"
export CXX_x86_64_linux_android="$CLANGXX_EXE_WIN"
export AR_x86_64_linux_android="$AR_EXE_WIN"
export RANLIB_x86_64_linux_android="$RANLIB_EXE_WIN"
export CFLAGS_x86_64_linux_android="--target=x86_64-linux-android21"
export CXXFLAGS_x86_64_linux_android="--target=x86_64-linux-android21"

# Rust linker: use Windows .cmd wrappers so clang picks Android GNU linker mode.
export CARGO_TARGET_AARCH64_LINUX_ANDROID_LINKER="$CLANG_CMD_WIN"
export CARGO_TARGET_ARMV7_LINUX_ANDROIDEABI_LINKER="$(cygpath -m "$NDK_BIN/armv7a-linux-androideabi21-clang${CLANG_SUFFIX}")"
export CARGO_TARGET_X86_64_LINUX_ANDROID_LINKER="$(cygpath -m "$NDK_BIN/x86_64-linux-android21-clang${CLANG_SUFFIX}")"

mkdir -p "$JNI_DIR"

cd "$CRATES_DIR"

build_target() {
  local rust_target="$1"
  local abi="$2"
  echo "Building $rust_target ($abi)..."
  cargo build --release --target "$rust_target" --package pirate-ffi-frb --features frb --no-default-features
  local so_path="$CRATES_DIR/target/$rust_target/release/libpirate_ffi_frb.so"
  if [[ ! -f "$so_path" ]]; then
    echo "libpirate_ffi_frb.so not found at $so_path" >&2
    exit 1
  fi
  local dest_dir="$JNI_DIR/$abi"
  mkdir -p "$dest_dir"
  cp "$so_path" "$dest_dir/"
  echo "Copied to $dest_dir"
}

build_target "aarch64-linux-android" "arm64-v8a"
build_target "armv7-linux-androideabi" "armeabi-v7a"
build_target "x86_64-linux-android" "x86_64"
