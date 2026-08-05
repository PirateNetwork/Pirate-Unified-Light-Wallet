#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
PLUGIN_DIR="$PROJECT_ROOT/bindings/react-native-pirate-wallet"
ANDROID_PACKAGE_DIR="$PROJECT_ROOT/bindings/react-native-pirate-wallet-android"

ANDROID_SRC="$PROJECT_ROOT/bindings/android-sdk/src/main/jniLibs"
ANDROID_DST="$ANDROID_PACKAGE_DIR/android/src/main/jniLibs"
IOS_SRC="$PROJECT_ROOT/bindings/ios-sdk/Frameworks/PirateWalletNative.xcframework"
IOS_DST="$PLUGIN_DIR/ios/Frameworks/PirateWalletNative.xcframework"
LEGACY_ANDROID_DST="$PLUGIN_DIR/android/src/main/jniLibs"

if [[ ! -d "$ANDROID_SRC" ]]; then
  echo "Missing Android JNI libraries: $ANDROID_SRC" >&2
  exit 1
fi

if [[ ! -d "$IOS_SRC" ]]; then
  echo "Missing iOS XCFramework: $IOS_SRC" >&2
  exit 1
fi

rm -rf "$ANDROID_DST" "$LEGACY_ANDROID_DST" "$IOS_DST"
mkdir -p "$ANDROID_DST" "$(dirname "$IOS_DST")"
cp -R "$ANDROID_SRC"/. "$ANDROID_DST"/

cp -R "$IOS_SRC" "$IOS_DST"

echo "Staged Android JNI libraries into $ANDROID_DST"
echo "Staged iOS XCFramework into $IOS_DST"
