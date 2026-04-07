#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
CRATES_DIR="$PROJECT_ROOT/crates"
SDK_DIR="$PROJECT_ROOT/bindings/ios-sdk"
FRAMEWORKS_DIR="$SDK_DIR/Frameworks"
CRATE_DIR="$CRATES_DIR/pirate-ffi-native"
HEADER="$CRATE_DIR/pirate_wallet_service.h"
IOS_MIN_DEPLOYMENT_TARGET="${IOS_MIN_DEPLOYMENT_TARGET:-13.0}"

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
rustup target add aarch64-apple-ios aarch64-apple-ios-sim x86_64-apple-ios

cd "$CRATES_DIR"
# The XCFramework packages static libraries only. Build just the staticlib
# artifact so iOS packaging does not waste time or fail linking an unused cdylib.
cargo rustc --release --target aarch64-apple-ios --package pirate-ffi-native --lib -- --crate-type staticlib
cargo rustc --release --target aarch64-apple-ios-sim --package pirate-ffi-native --lib -- --crate-type staticlib
cargo rustc --release --target x86_64-apple-ios --package pirate-ffi-native --lib -- --crate-type staticlib

TMP_DIR="$(mktemp -d)"
trap 'rm -rf "$TMP_DIR"' EXIT

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

echo "Built iOS SDK XCFramework at $FRAMEWORKS_DIR/PirateWalletNative.xcframework"
echo "Packaged $ZIP_PATH"
echo "Packaged $PACKAGE_ZIP"
echo "Rust iOS build deployment target: $IPHONEOS_DEPLOYMENT_TARGET"
