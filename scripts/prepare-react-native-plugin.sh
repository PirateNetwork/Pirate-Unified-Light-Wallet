#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
PLUGIN_DIR="$PROJECT_ROOT/bindings/react-native-pirate-wallet"

ANDROID_SRC="$PROJECT_ROOT/bindings/android-sdk/src/main/jniLibs"
ANDROID_DST="$PLUGIN_DIR/android/src/main/jniLibs"
IOS_SRC="$PROJECT_ROOT/bindings/ios-sdk/Frameworks/PirateWalletNative.xcframework"
IOS_DST_DIR="$PLUGIN_DIR/ios/Frameworks"

if [[ ! -d "$ANDROID_SRC" ]]; then
  echo "Missing Android JNI libraries: $ANDROID_SRC" >&2
  exit 1
fi

if [[ ! -d "$IOS_SRC" ]]; then
  echo "Missing iOS XCFramework: $IOS_SRC" >&2
  exit 1
fi

mkdir -p "$ANDROID_DST" "$IOS_DST_DIR"
rm -rf "$ANDROID_DST"/*
cp -R "$ANDROID_SRC"/. "$ANDROID_DST"/

rm -rf "$IOS_DST_DIR/PirateWalletNative.xcframework"
cp -R "$IOS_SRC" "$IOS_DST_DIR/"

echo "Staged Android JNI libraries into $ANDROID_DST"
echo "Staged iOS XCFramework into $IOS_DST_DIR"
