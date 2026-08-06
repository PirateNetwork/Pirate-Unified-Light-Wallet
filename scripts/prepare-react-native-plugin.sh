#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
PLUGIN_DIR="$PROJECT_ROOT/bindings/react-native-pirate-wallet"
ANDROID_ARM_PACKAGE_DIR="$PROJECT_ROOT/bindings/react-native-pirate-wallet-android"
ANDROID_X86_PACKAGE_DIR="$PROJECT_ROOT/bindings/react-native-pirate-wallet-android-x86_64"
IOS_DEVICE_PACKAGE_DIR="$PROJECT_ROOT/bindings/react-native-pirate-wallet-ios-device"
IOS_SIMULATOR_PACKAGE_DIR="$PROJECT_ROOT/bindings/react-native-pirate-wallet-ios-simulator"

ANDROID_SRC="$PROJECT_ROOT/bindings/android-sdk/src/main/jniLibs"
ANDROID_ARM_DST="$ANDROID_ARM_PACKAGE_DIR/android/src/main/jniLibs"
ANDROID_X86_DST="$ANDROID_X86_PACKAGE_DIR/android/src/main/jniLibs"
IOS_SRC="$PROJECT_ROOT/bindings/ios-sdk/Frameworks/PirateWalletNative.xcframework"
IOS_DEVICE_DST="$IOS_DEVICE_PACKAGE_DIR/ios/Frameworks/PirateWalletNative.xcframework"
IOS_SIMULATOR_DST="$IOS_SIMULATOR_PACKAGE_DIR/ios/Frameworks/PirateWalletNative.xcframework"
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

rm -rf \
  "$ANDROID_ARM_DST" \
  "$ANDROID_X86_DST" \
  "$IOS_DEVICE_DST" \
  "$IOS_SIMULATOR_DST" \
  "$LEGACY_ANDROID_DST" \
  "$LEGACY_IOS_DST"
mkdir -p \
  "$ANDROID_ARM_DST" \
  "$ANDROID_X86_DST" \
  "$IOS_DEVICE_DST" \
  "$IOS_SIMULATOR_DST"

for abi in arm64-v8a armeabi-v7a; do
  cp -R "$ANDROID_SRC/$abi" "$ANDROID_ARM_DST/"
done
cp -R "$ANDROID_SRC/x86_64" "$ANDROID_X86_DST/"

cp "$IOS_SRC/Info.plist" "$IOS_DEVICE_DST/Info.plist"
cp -R "$IOS_SRC/ios-arm64" "$IOS_DEVICE_DST/"
cp "$IOS_SRC/Info.plist" "$IOS_SIMULATOR_DST/Info.plist"
cp -R \
  "$IOS_SRC/ios-arm64_x86_64-simulator" \
  "$IOS_SIMULATOR_DST/"

echo "Staged Android ARM JNI libraries into $ANDROID_ARM_DST"
echo "Staged Android x86_64 JNI library into $ANDROID_X86_DST"
echo "Staged iOS device XCFramework slice into $IOS_DEVICE_DST"
echo "Staged iOS simulator XCFramework slice into $IOS_SIMULATOR_DST"
