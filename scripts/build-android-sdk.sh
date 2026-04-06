#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
CRATES_DIR="$PROJECT_ROOT/crates"
SDK_DIR="$PROJECT_ROOT/bindings/android-sdk"
JNI_DIR="$SDK_DIR/src/main/jniLibs"
TOOLCHAIN_WRAPPER_DIR=""

cleanup() {
  if [[ -n "${TOOLCHAIN_WRAPPER_DIR:-}" && -d "$TOOLCHAIN_WRAPPER_DIR" ]]; then
    rm -rf "$TOOLCHAIN_WRAPPER_DIR"
  fi
}

trap cleanup EXIT

HOST_TAG="linux-x86_64"
if [[ "$(uname -s)" == "Darwin" ]]; then
  if [[ "$(uname -m)" == "arm64" ]]; then
    HOST_TAG="darwin-arm64"
  else
    HOST_TAG="darwin-x86_64"
  fi
fi

toolchain_dir_valid() {
  local ndk_dir="$1"
  [[ -d "$ndk_dir/toolchains/llvm/prebuilt/$HOST_TAG/bin" ]] &&
    [[ -d "$ndk_dir/toolchains/llvm/prebuilt/$HOST_TAG/sysroot" ]]
}

resolve_ndk() {
  if [[ -n "${ANDROID_NDK_HOME:-}" ]]; then
    if toolchain_dir_valid "$ANDROID_NDK_HOME"; then
      echo "$ANDROID_NDK_HOME"
      return 0
    fi
  fi
  if [[ -n "${ANDROID_NDK_ROOT:-}" ]]; then
    if toolchain_dir_valid "$ANDROID_NDK_ROOT"; then
      echo "$ANDROID_NDK_ROOT"
      return 0
    fi
  fi
  local sdk="${ANDROID_SDK_ROOT:-${ANDROID_HOME:-}}"
  if [[ -z "$sdk" ]]; then
    return 1
  fi
  if [[ -n "${ANDROID_NDK_VERSION:-}" && -d "$sdk/ndk/$ANDROID_NDK_VERSION" ]]; then
    if toolchain_dir_valid "$sdk/ndk/$ANDROID_NDK_VERSION"; then
      echo "$sdk/ndk/$ANDROID_NDK_VERSION"
      return 0
    fi
  fi
  if [[ -d "$sdk/ndk" ]]; then
    while IFS= read -r candidate; do
      if toolchain_dir_valid "$candidate"; then
        echo "$candidate"
        return 0
      fi
    done < <(ls -d "$sdk/ndk"/* 2>/dev/null | sort -Vr)
  fi
  if [[ -d "$sdk/ndk-bundle" ]]; then
    if toolchain_dir_valid "$sdk/ndk-bundle"; then
      echo "$sdk/ndk-bundle"
      return 0
    fi
  fi
  return 1
}

toolchain_executable_works() {
  local exe="$1"
  "$exe" --version >/dev/null 2>&1
}

find_clang_resource_dir() {
  local prebuilt_root="$1"
  find "$prebuilt_root/lib/clang" -mindepth 1 -maxdepth 1 -type d 2>/dev/null | sort -V | tail -n 1
}

find_host_clang() {
  local ndk_bin="$1"
  local clang_major="$2"
  local candidate=""
  local preferred_names=()

  if [[ -n "$clang_major" ]]; then
    preferred_names+=("clang-$clang_major")
  fi
  preferred_names+=("clang")

  for name in "${preferred_names[@]}"; do
    while IFS= read -r candidate; do
      if [[ -n "$candidate" && "$candidate" != "$ndk_bin/"* ]]; then
        echo "$candidate"
        return 0
      fi
    done < <(which -a "$name" 2>/dev/null || true)
  done
  return 1
}

make_system_clang_wrapper() {
  local wrapper_path="$1"
  local target_triple="$2"
  local system_clang="$3"
  local sysroot="$4"
  local resource_dir="$5"
  local linker="$6"
  cat > "$wrapper_path" <<EOF
#!/usr/bin/env bash
set -euo pipefail
compile_only=0
for arg in "\$@"; do
  case "\$arg" in
    -c|-E|-S|-M|-MM|-fsyntax-only)
      compile_only=1
      break
      ;;
  esac
done

args=(
  --target="$target_triple"
  --sysroot="$sysroot"
)

if [[ "\$compile_only" -eq 0 ]]; then
  args+=(
    -resource-dir="$resource_dir"
    -rtlib=compiler-rt
    -unwindlib=libunwind
    -fuse-ld="$linker"
  )
fi

exec "$system_clang" "\${args[@]}" "\$@"
EOF
  chmod +x "$wrapper_path"
}

configure_linkers() {
  local ndk_bin="$1"
  local prebuilt_root="$2"
  local aarch64_linker="$ndk_bin/aarch64-linux-android21-clang"
  local armv7_linker="$ndk_bin/armv7a-linux-androideabi21-clang"
  local x86_64_linker="$ndk_bin/x86_64-linux-android21-clang"

  if ! toolchain_executable_works "$ndk_bin/clang"; then
    local system_clang
    local sysroot
    local resource_dir
    local lld
    local clang_major

    sysroot="$prebuilt_root/sysroot"
    resource_dir="$(find_clang_resource_dir "$prebuilt_root")"
    lld="$ndk_bin/ld.lld"
    clang_major="$(basename "$resource_dir" 2>/dev/null || true)"

    if [[ -z "$resource_dir" || ! -x "$lld" ]]; then
      echo "Android NDK fallback toolchain is incomplete under $prebuilt_root" >&2
      exit 1
    fi

    system_clang="$(find_host_clang "$ndk_bin" "$clang_major" || true)"
    if [[ -z "$system_clang" ]]; then
      echo "Android NDK clang is not runnable on this host and no compatible system clang fallback was found." >&2
      exit 1
    fi

    TOOLCHAIN_WRAPPER_DIR="$(mktemp -d)"

    make_system_clang_wrapper "$TOOLCHAIN_WRAPPER_DIR/aarch64-linux-android21-clang" "aarch64-linux-android21" "$system_clang" "$sysroot" "$resource_dir" "$lld"
    make_system_clang_wrapper "$TOOLCHAIN_WRAPPER_DIR/armv7a-linux-androideabi21-clang" "armv7a-linux-androideabi21" "$system_clang" "$sysroot" "$resource_dir" "$lld"
    make_system_clang_wrapper "$TOOLCHAIN_WRAPPER_DIR/x86_64-linux-android21-clang" "x86_64-linux-android21" "$system_clang" "$sysroot" "$resource_dir" "$lld"

    aarch64_linker="$TOOLCHAIN_WRAPPER_DIR/aarch64-linux-android21-clang"
    armv7_linker="$TOOLCHAIN_WRAPPER_DIR/armv7a-linux-androideabi21-clang"
    x86_64_linker="$TOOLCHAIN_WRAPPER_DIR/x86_64-linux-android21-clang"
    export PATH="$TOOLCHAIN_WRAPPER_DIR:$PATH"
  fi

  export CARGO_TARGET_AARCH64_LINUX_ANDROID_LINKER="$aarch64_linker"
  export CARGO_TARGET_ARMV7_LINUX_ANDROIDEABI_LINKER="$armv7_linker"
  export CARGO_TARGET_X86_64_LINUX_ANDROID_LINKER="$x86_64_linker"
  export CARGO_TARGET_AARCH64_LINUX_ANDROID_AR="$ndk_bin/llvm-ar"
  export CARGO_TARGET_ARMV7_LINUX_ANDROIDEABI_AR="$ndk_bin/llvm-ar"
  export CARGO_TARGET_X86_64_LINUX_ANDROID_AR="$ndk_bin/llvm-ar"
  export CC_aarch64_linux_android="$aarch64_linker"
  export CC_armv7_linux_androideabi="$armv7_linker"
  export CC_x86_64_linux_android="$x86_64_linker"
  export CXX_aarch64_linux_android="$aarch64_linker"
  export CXX_armv7_linux_androideabi="$armv7_linker"
  export CXX_x86_64_linux_android="$x86_64_linker"
  export AR_aarch64_linux_android="$ndk_bin/llvm-ar"
  export AR_armv7_linux_androideabi="$ndk_bin/llvm-ar"
  export AR_x86_64_linux_android="$ndk_bin/llvm-ar"
}

find_gradle() {
  if [[ -x "$SDK_DIR/gradlew" ]]; then
    echo "./gradlew"
    return 0
  fi
  if command -v gradle >/dev/null 2>&1; then
    command -v gradle
    return 0
  fi
  local bundled_gradle
  bundled_gradle="$(ls -d /opt/gradle/gradle-*/bin/gradle 2>/dev/null | sort -V | tail -n 1 || true)"
  if [[ -n "$bundled_gradle" ]]; then
    echo "$bundled_gradle"
    return 0
  fi
  return 1
}

resolve_java_home() {
  if [[ -n "${JAVA_HOME:-}" && -x "${JAVA_HOME}/bin/javac" ]]; then
    echo "$JAVA_HOME"
    return 0
  fi
  if command -v javac >/dev/null 2>&1; then
    local javac_path
    javac_path="$(readlink -f "$(command -v javac)")"
    dirname "$(dirname "$javac_path")"
    return 0
  fi
  return 1
}

NDK_HOME="$(resolve_ndk || true)"
if [[ -z "$NDK_HOME" ]]; then
  echo "Android NDK not found. Set ANDROID_NDK_HOME or install the NDK first." >&2
  exit 1
fi

PREBUILT_ROOT="$NDK_HOME/toolchains/llvm/prebuilt/$HOST_TAG"
NDK_BIN="$PREBUILT_ROOT/bin"
if [[ ! -d "$NDK_BIN" ]]; then
  echo "Android NDK toolchain not found at $NDK_BIN" >&2
  exit 1
fi

export ANDROID_NDK_HOME="$NDK_HOME"
export ANDROID_NDK_ROOT="$NDK_HOME"
export PATH="$NDK_BIN:$PATH"

if command -v rustup >/dev/null 2>&1; then
  rustup target add aarch64-linux-android armv7-linux-androideabi x86_64-linux-android
fi

configure_linkers "$NDK_BIN" "$PREBUILT_ROOT"

build_rust_target() {
  local rust_target="$1"
  local abi="$2"
  local source_lib
  local staged_lib

  cargo build --release --target "$rust_target" --package pirate-ffi-native --locked
  mkdir -p "$JNI_DIR/$abi"
  source_lib="$CRATES_DIR/target/$rust_target/release/libpirate_ffi_native.so"
  staged_lib="$JNI_DIR/$abi/libpirate_ffi_native.so"
  cp "$source_lib" "$staged_lib"

  # Strip unneeded symbols before Gradle packages the AAR so release builds do not ship
  # oversized native libraries and do not rely on AGP to do heavy stripping work later.
  "$NDK_BIN/llvm-strip" --strip-unneeded "$staged_lib"
}

mkdir -p "$JNI_DIR"
cd "$CRATES_DIR"
build_rust_target "aarch64-linux-android" "arm64-v8a"
build_rust_target "armv7-linux-androideabi" "armeabi-v7a"
build_rust_target "x86_64-linux-android" "x86_64"

GRADLE_CMD="$(find_gradle || true)"
if [[ -n "$GRADLE_CMD" ]]; then
  export GRADLE_USER_HOME="${GRADLE_USER_HOME:-$PROJECT_ROOT/.gradle-android-sdk}"
  mkdir -p "$GRADLE_USER_HOME"
  if JAVA_HOME_RESOLVED="$(resolve_java_home || true)"; then
    export JAVA_HOME="$JAVA_HOME_RESOLVED"
  fi
  (cd "$SDK_DIR" && "$GRADLE_CMD" --no-daemon test assembleRelease)
else
  echo "Gradle wrapper not present and gradle is not installed. Rust JNI libraries have been staged in $JNI_DIR" >&2
fi

DIST_DIR="$PROJECT_ROOT/dist/android-sdk"
mkdir -p "$DIST_DIR"

if compgen -G "$SDK_DIR/build/outputs/aar/*.aar" > /dev/null; then
  cp "$SDK_DIR"/build/outputs/aar/*.aar "$DIST_DIR"/
fi

SDK_PACKAGE_DIR="$DIST_DIR/pirate-android-sdk-package"
rm -rf "$SDK_PACKAGE_DIR"
mkdir -p "$SDK_PACKAGE_DIR"
cp -R "$SDK_DIR/src" "$SDK_PACKAGE_DIR/"
cp "$SDK_DIR/build.gradle.kts" "$SDK_DIR/settings.gradle.kts" "$SDK_DIR/gradle.properties" "$SDK_DIR/consumer-rules.pro" "$SDK_PACKAGE_DIR/"

PACKAGE_ZIP="$DIST_DIR/pirate-android-sdk-package.zip"
rm -f "$PACKAGE_ZIP" "$PACKAGE_ZIP.sha256"
(cd "$DIST_DIR" && zip -qr "$(basename "$PACKAGE_ZIP")" "$(basename "$SDK_PACKAGE_DIR")")
(cd "$DIST_DIR" && sha256sum "$(basename "$PACKAGE_ZIP")" > "$(basename "$PACKAGE_ZIP").sha256")

echo "Android SDK AAR output, if Gradle was available, was written under $SDK_DIR/build"
echo "Android SDK release bundles were written to $DIST_DIR"
