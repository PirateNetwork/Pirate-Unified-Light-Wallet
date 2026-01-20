#!/usr/bin/env bash
set -euo pipefail

PROJECT_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
CRATES_DIR="$PROJECT_ROOT/crates"
JNI_DIR="$PROJECT_ROOT/app/android/app/src/main/jniLibs"

NDK_HOME="${ANDROID_NDK_HOME:-$HOME/android-ndk-r26d-clean}"

if [[ ! -d "$NDK_HOME" ]]; then
  echo "NDK not found at: $NDK_HOME" >&2
  echo "Expected the Linux NDK to be unpacked in WSL." >&2
  exit 1
fi

if ! command -v cargo >/dev/null 2>&1; then
  echo "cargo not found in WSL. Install Rust (rustup) first." >&2
  exit 1
fi

if command -v rustup >/dev/null 2>&1; then
  rustup target add aarch64-linux-android armv7-linux-androideabi x86_64-linux-android
fi

export ANDROID_NDK_HOME="$NDK_HOME"
export ANDROID_NDK_ROOT="$NDK_HOME"
export CARGO_INCREMENTAL=0

BIN_DIR="$NDK_HOME/toolchains/llvm/prebuilt/linux-x86_64/bin"
SYSROOT="$NDK_HOME/toolchains/llvm/prebuilt/linux-x86_64/sysroot"
WRAP_CLANG="$BIN_DIR/clang-wsl"
WRAP_CLANGXX="$BIN_DIR/clang++-wsl"

if [[ ! -f "$WRAP_CLANG" ]]; then
  cat > "$WRAP_CLANG" <<EOF
#!/usr/bin/env bash
exec /lib64/ld-linux-x86-64.so.2 "$BIN_DIR/clang-17" "\$@"
EOF
  chmod +x "$WRAP_CLANG"
fi

if [[ ! -f "$WRAP_CLANGXX" ]]; then
  cat > "$WRAP_CLANGXX" <<EOF
#!/usr/bin/env bash
exec /lib64/ld-linux-x86-64.so.2 "$BIN_DIR/clang-17" "\$@"
EOF
  chmod +x "$WRAP_CLANGXX"
fi

echo "Using ANDROID_NDK_HOME=$ANDROID_NDK_HOME"
echo "Using clang wrapper: $WRAP_CLANG"
echo "Using clang++ wrapper: $WRAP_CLANGXX"
echo "Using sysroot: $SYSROOT"
echo "Project root: $PROJECT_ROOT"

mkdir -p "$JNI_DIR"

cd "$CRATES_DIR"

echo "Building Android Rust libs (arm64-v8a, armeabi-v7a, x86_64)..."
env \
  "CFLAGS=--sysroot=$SYSROOT" \
  "CXXFLAGS=--sysroot=$SYSROOT" \
  "CC_aarch64-linux-android=$WRAP_CLANG" \
  "CXX_aarch64-linux-android=$WRAP_CLANGXX" \
  "CC_armv7-linux-androideabi=$WRAP_CLANG" \
  "CXX_armv7-linux-androideabi=$WRAP_CLANGXX" \
  "CC_i686-linux-android=$WRAP_CLANG" \
  "CXX_i686-linux-android=$WRAP_CLANGXX" \
  "CC_x86_64-linux-android=$WRAP_CLANG" \
  "CXX_x86_64-linux-android=$WRAP_CLANGXX" \
  cargo ndk \
  -t arm64-v8a \
  -t armeabi-v7a \
  -t x86_64 \
  -o "$JNI_DIR" \
  build --release -p pirate-ffi-frb --features frb --no-default-features --locked

echo "Built libs:"
find "$JNI_DIR" -type f -name "libpirate_ffi_frb.so" -print
