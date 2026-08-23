#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
CRATES_DIR="$PROJECT_ROOT/crates"
SDK_DIR="$PROJECT_ROOT/bindings/ios-sdk"
FRAMEWORKS_DIR="$SDK_DIR/Frameworks"
CRATE_DIR="$CRATES_DIR/pirate-ffi-native"
HEADER="$CRATE_DIR/pirate_wallet_service.h"
IOS_MIN_DEPLOYMENT_TARGET="${IOS_MIN_DEPLOYMENT_TARGET:-15.0}"
IOS_MAX_COMPRESSED_ARCHIVE_BYTES="${IOS_MAX_COMPRESSED_ARCHIVE_BYTES:-190000000}"

if [[ "$OSTYPE" != "darwin"* ]]; then
  echo "iOS SDK packaging requires macOS." >&2
  exit 1
fi

if [[ ! -f "$HEADER" ]]; then
  echo "Missing header: $HEADER" >&2
  exit 1
fi

export CARGO_INCREMENTAL=0
export IPHONEOS_DEPLOYMENT_TARGET="$IOS_MIN_DEPLOYMENT_TARGET"
# Keep release-only compiler data out of the static libraries at the source.
# The XCFramework is consumed as a binary dependency, so Cargo debug metadata
# and incremental state provide no value to downstream applications.
#
# Do not enable LTO or collapse this build to one codegen unit. Those settings
# are useful when producing a final executable, but this output is an
# intermediate static archive. On Xcode 26 they more than doubled the packaged
# archive by coalescing dependency code into large members. Preserve normal
# release archive granularity so the consumer linker can load only the members
# reachable from the two exported C entry points.
export CARGO_PROFILE_RELEASE_DEBUG=0
export CARGO_PROFILE_RELEASE_INCREMENTAL=false
export CARGO_PROFILE_RELEASE_LTO=false
export CARGO_PROFILE_RELEASE_CODEGEN_UNITS=16
export CARGO_PROFILE_RELEASE_STRIP=debuginfo
rustup target add aarch64-apple-ios aarch64-apple-ios-sim x86_64-apple-ios

TMP_DIR="$(mktemp -d)"
trap 'rm -rf "$TMP_DIR"' EXIT

strip_static_archive() {
  local archive="$1"
  local target_name="$2"
  local original_size
  local stripped_size
  local symbols_file="$TMP_DIR/$target_name-exported-symbols.txt"

  original_size="$(stat -f%z "$archive")"
  # -S drops DWARF/debug data. -x drops local linker symbols while retaining
  # the external and undefined symbols a static archive needs at link time.
  # Keep this as a post-build defence because vendored C archives do not
  # necessarily inherit Cargo's Rust code-generation settings.
  xcrun strip -S -x "$archive"
  xcrun ranlib "$archive"
  stripped_size="$(stat -f%z "$archive")"

  if (( stripped_size <= 0 || stripped_size > original_size )); then
    echo "Invalid stripped archive size for $target_name: $stripped_size (was $original_size)" >&2
    exit 1
  fi

  xcrun nm -gjU "$archive" > "$symbols_file"
  for symbol in \
    _pirate_wallet_service_invoke_json \
    _pirate_wallet_service_free_string; do
    if ! grep -Fxq "$symbol" "$symbols_file"; then
      echo "Stripping $target_name removed required export: $symbol" >&2
      exit 1
    fi
  done

  echo "Stripped iOS debug metadata and local symbols for $target_name ($original_size -> $stripped_size bytes)"
}

verify_compressed_archive_budget() {
  local archive="$1"
  local target_name="$2"
  local raw_size
  local compressed_size

  raw_size="$(stat -f%z "$archive")"
  compressed_size="$(gzip -9 -c "$archive" | wc -c | tr -d '[:space:]')"
  if (( compressed_size <= 0 )); then
    echo "Could not measure compressed archive size for $target_name" >&2
    exit 1
  fi
  if (( compressed_size > IOS_MAX_COMPRESSED_ARCHIVE_BYTES )); then
    echo "$target_name exceeds the compressed iOS package budget: $compressed_size bytes (raw: $raw_size bytes, limit: $IOS_MAX_COMPRESSED_ARCHIVE_BYTES bytes)" >&2
    exit 1
  fi

  echo "Verified compressed iOS package budget for $target_name ($compressed_size compressed bytes; $raw_size raw bytes)"
}

verify_architectures() {
  local archive="$1"
  local label="$2"
  shift 2
  local actual
  local expected
  actual="$(lipo -archs "$archive")"

  if (( $(wc -w <<< "$actual") != $# )); then
    echo "$label has unexpected architectures: $actual" >&2
    exit 1
  fi
  for expected in "$@"; do
    if [[ " $actual " != *" $expected "* ]]; then
      echo "$label is missing architecture $expected: $actual" >&2
      exit 1
    fi
  done
  echo "Verified $label architectures: $actual"
}

cd "$CRATES_DIR"
# The XCFramework packages static libraries only. Build just the staticlib
# artifact so iOS packaging does not waste time or fail linking an unused cdylib.
cargo rustc --release --locked --target aarch64-apple-ios --package pirate-ffi-native --lib -- --crate-type staticlib
cargo rustc --release --locked --target aarch64-apple-ios-sim --package pirate-ffi-native --lib -- --crate-type staticlib
cargo rustc --release --locked --target x86_64-apple-ios --package pirate-ffi-native --lib -- --crate-type staticlib

strip_static_archive \
  "$CRATES_DIR/target/aarch64-apple-ios/release/libpirate_ffi_native.a" \
  "ios-arm64"
strip_static_archive \
  "$CRATES_DIR/target/aarch64-apple-ios-sim/release/libpirate_ffi_native.a" \
  "ios-simulator-arm64"
strip_static_archive \
  "$CRATES_DIR/target/x86_64-apple-ios/release/libpirate_ffi_native.a" \
  "ios-simulator-x86_64"
verify_compressed_archive_budget \
  "$CRATES_DIR/target/aarch64-apple-ios/release/libpirate_ffi_native.a" \
  "ios-arm64"

HEADERS_DIR="$TMP_DIR/include"
mkdir -p "$HEADERS_DIR"
cp "$HEADER" "$HEADERS_DIR/"
cat > "$HEADERS_DIR/module.modulemap" <<'EOF'
module PirateWalletNative {
  header "pirate_wallet_service.h"
  export *
}
EOF

SIM_DIR="$TMP_DIR/sim"
mkdir -p "$SIM_DIR"
SIM_LIB="$SIM_DIR/libpirate_ffi_native.a"
lipo -create \
  "$CRATES_DIR/target/aarch64-apple-ios-sim/release/libpirate_ffi_native.a" \
  "$CRATES_DIR/target/x86_64-apple-ios/release/libpirate_ffi_native.a" \
  -output "$SIM_LIB"
verify_compressed_archive_budget \
  "$SIM_LIB" \
  "ios-arm64_x86_64-simulator"

verify_architectures \
  "$CRATES_DIR/target/aarch64-apple-ios/release/libpirate_ffi_native.a" \
  "iOS device archive" \
  arm64
verify_architectures \
  "$SIM_LIB" \
  "iOS simulator archive" \
  arm64 x86_64

mkdir -p "$FRAMEWORKS_DIR"
rm -rf "$FRAMEWORKS_DIR/PirateWalletNative.xcframework"
xcodebuild -create-xcframework \
  -library "$CRATES_DIR/target/aarch64-apple-ios/release/libpirate_ffi_native.a" -headers "$HEADERS_DIR" \
  -library "$SIM_LIB" -headers "$HEADERS_DIR" \
  -output "$FRAMEWORKS_DIR/PirateWalletNative.xcframework"

DIST_DIR="$PROJECT_ROOT/dist/ios-sdk"
mkdir -p "$DIST_DIR"
ZIP_PATH="$DIST_DIR/PirateWalletNative.xcframework.zip"
rm -f "$ZIP_PATH" "$ZIP_PATH.sha256"
(cd "$FRAMEWORKS_DIR" && ditto -c -k --sequesterRsrc --keepParent PirateWalletNative.xcframework "$ZIP_PATH")
(cd "$DIST_DIR" && shasum -a 256 "$(basename "$ZIP_PATH")" > "$(basename "$ZIP_PATH").sha256")

PACKAGE_STAGING="$DIST_DIR/PirateWalletSDK-package"
rm -rf "$PACKAGE_STAGING"
mkdir -p "$PACKAGE_STAGING/Sources/PirateWalletSDK" "$PACKAGE_STAGING/Frameworks"
cp "$SDK_DIR/Package.swift" "$PACKAGE_STAGING/"
cp "$SDK_DIR"/Sources/PirateWalletSDK/*.swift "$PACKAGE_STAGING/Sources/PirateWalletSDK/"
cp -R "$FRAMEWORKS_DIR/PirateWalletNative.xcframework" "$PACKAGE_STAGING/Frameworks/"

PACKAGE_ZIP="$DIST_DIR/PirateWalletSDK-package.zip"
rm -f "$PACKAGE_ZIP" "$PACKAGE_ZIP.sha256"
(cd "$DIST_DIR" && ditto -c -k --sequesterRsrc --keepParent PirateWalletSDK-package "$PACKAGE_ZIP")
(cd "$DIST_DIR" && shasum -a 256 "$(basename "$PACKAGE_ZIP")" > "$(basename "$PACKAGE_ZIP").sha256")
rm -rf "$PACKAGE_STAGING"

echo "Built iOS SDK XCFramework at $FRAMEWORKS_DIR/PirateWalletNative.xcframework"
echo "Packaged $ZIP_PATH"
echo "Packaged $PACKAGE_ZIP"
echo "Rust iOS build deployment target: $IPHONEOS_DEPLOYMENT_TARGET"
