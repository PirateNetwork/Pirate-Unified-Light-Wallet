#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
PLUGIN_DIR="$PROJECT_ROOT/bindings/react-native-pirate-wallet"
ANDROID_ARM_PACKAGE_DIR="$PROJECT_ROOT/bindings/react-native-pirate-wallet-android"
ANDROID_X86_PACKAGE_DIR="$PROJECT_ROOT/bindings/react-native-pirate-wallet-android-x86_64"
IOS_DEVICE_PACKAGE_DIR="$PROJECT_ROOT/bindings/react-native-pirate-wallet-ios-device"
IOS_SIMULATOR_ARM64_PACKAGE_DIR="$PROJECT_ROOT/bindings/react-native-pirate-wallet-ios-simulator-arm64"
IOS_SIMULATOR_X86_64_PACKAGE_DIR="$PROJECT_ROOT/bindings/react-native-pirate-wallet-ios-simulator-x86_64"

ANDROID_SRC="$PROJECT_ROOT/bindings/android-sdk/src/main/jniLibs"
ANDROID_ARM_DST="$ANDROID_ARM_PACKAGE_DIR/android/src/main/jniLibs"
ANDROID_X86_DST="$ANDROID_X86_PACKAGE_DIR/android/src/main/jniLibs"
IOS_SRC="$PROJECT_ROOT/bindings/ios-sdk/Frameworks/PirateWalletNative.xcframework"
IOS_SIMULATOR_SLICES_SRC="$PROJECT_ROOT/dist/ios-sdk/react-native"
IOS_DEVICE_DST="$IOS_DEVICE_PACKAGE_DIR/ios/Frameworks/PirateWalletNative.xcframework"
IOS_SIMULATOR_ARM64_DST="$IOS_SIMULATOR_ARM64_PACKAGE_DIR/ios"
IOS_SIMULATOR_X86_64_DST="$IOS_SIMULATOR_X86_64_PACKAGE_DIR/ios"
LEGACY_IOS_DST="$PLUGIN_DIR/ios/Frameworks/PirateWalletNative.xcframework"
LEGACY_ANDROID_DST="$PLUGIN_DIR/android/src/main/jniLibs"

if [[ ! -d "$ANDROID_SRC" ]]; then
  echo "Missing Android JNI libraries: $ANDROID_SRC" >&2
  exit 1
fi

if [[ ! -d "$IOS_SRC" ]]; then
  echo "Missing iOS XCFramework: $IOS_SRC" >&2
  exit 1
fi

for archive in ios-simulator-arm64.a ios-simulator-x86_64.a; do
  if [[ ! -s "$IOS_SIMULATOR_SLICES_SRC/$archive" ]]; then
    echo "Missing thin iOS simulator archive: $IOS_SIMULATOR_SLICES_SRC/$archive" >&2
    exit 1
  fi
done

rm -rf \
  "$ANDROID_ARM_DST" \
  "$ANDROID_X86_DST" \
  "$IOS_DEVICE_DST" \
  "$IOS_SIMULATOR_ARM64_DST" \
  "$IOS_SIMULATOR_X86_64_DST" \
  "$LEGACY_ANDROID_DST" \
  "$LEGACY_IOS_DST"
mkdir -p \
  "$ANDROID_ARM_DST" \
  "$ANDROID_X86_DST" \
  "$IOS_DEVICE_DST" \
  "$IOS_SIMULATOR_ARM64_DST/Headers" \
  "$IOS_SIMULATOR_X86_64_DST/Headers"

for abi in arm64-v8a armeabi-v7a; do
  cp -R "$ANDROID_SRC/$abi" "$ANDROID_ARM_DST/"
done
cp -R "$ANDROID_SRC/x86_64" "$ANDROID_X86_DST/"

cp "$IOS_SRC/Info.plist" "$IOS_DEVICE_DST/Info.plist"
cp -R "$IOS_SRC/ios-arm64" "$IOS_DEVICE_DST/"

SIMULATOR_HEADERS="$IOS_SRC/ios-arm64_x86_64-simulator/Headers"
for destination in "$IOS_SIMULATOR_ARM64_DST" "$IOS_SIMULATOR_X86_64_DST"; do
  cp "$SIMULATOR_HEADERS/module.modulemap" "$destination/Headers/"
  cp "$SIMULATOR_HEADERS/pirate_wallet_service.h" "$destination/Headers/"
done
cp \
  "$IOS_SIMULATOR_SLICES_SRC/ios-simulator-arm64.a" \
  "$IOS_SIMULATOR_ARM64_DST/libpirate_ffi_native.a"
cp \
  "$IOS_SIMULATOR_SLICES_SRC/ios-simulator-x86_64.a" \
  "$IOS_SIMULATOR_X86_64_DST/libpirate_ffi_native.a"

echo "Staged Android ARM JNI libraries into $ANDROID_ARM_DST"
echo "Staged Android x86_64 JNI library into $ANDROID_X86_DST"
echo "Staged iOS device XCFramework slice into $IOS_DEVICE_DST"
echo "Staged iOS arm64 simulator archive into $IOS_SIMULATOR_ARM64_DST"
echo "Staged iOS x86_64 simulator archive into $IOS_SIMULATOR_X86_64_DST"
