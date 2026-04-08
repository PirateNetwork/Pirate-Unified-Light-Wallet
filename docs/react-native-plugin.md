# React Native Plugin

The React Native package in this repo lives in:

- `bindings/react-native-pirate-wallet/`

It wraps the same native backend used by the Android SDK and iOS SDK.

Related paths:

- `bindings/android-sdk/`
- `bindings/ios-sdk/`
- `crates/pirate-ffi-native/`
- `crates/pirate-wallet-service/`

## What it is

The package provides one JavaScript API for React Native apps that want access to the Pirate unified wallet backend.

Platform layers:

- Android
  - Kotlin bridge over `libpirate_ffi_native.so`
- iOS
  - Swift bridge over `PirateWalletNative.xcframework`
- JavaScript
  - typed wrapper and polling synchronizer

The JavaScript surface mirrors the shielded-first SDK boundary used by the native Android and iOS SDKs.

## What it does not do

The package does not contain the wallet logic itself.

Wallet behavior stays in the Rust service layer:

- `crates/pirate-wallet-service/`

The React Native package is a bridge and packaging layer on top of the native SDK outputs.

## Preparing native artifacts

Before testing or packaging the React Native plugin from this monorepo, stage the native artifacts:

```bash
bash scripts/prepare-react-native-plugin.sh
```

That script copies:

- Android JNI libraries from `bindings/android-sdk/src/main/jniLibs/`
- iOS XCFramework output from `bindings/ios-sdk/Frameworks/`

into:

- `bindings/react-native-pirate-wallet/android/`
- `bindings/react-native-pirate-wallet/ios/`

If those native artifacts are missing, the React Native package will not build correctly.

## Package files

Important files:

- `bindings/react-native-pirate-wallet/package.json`
- `bindings/react-native-pirate-wallet/react-native-pirate-wallet.podspec`
- `bindings/react-native-pirate-wallet/react-native.config.js`
- `bindings/react-native-pirate-wallet/src/index.js`
- `bindings/react-native-pirate-wallet/src/index.d.ts`
- `bindings/react-native-pirate-wallet/README.md`
- `bindings/react-native-pirate-wallet/example/`

The package README carries the JavaScript API and RPC reference:

- `bindings/react-native-pirate-wallet/README.md`

The example app is the minimal real consumer used by CI:

- `bindings/react-native-pirate-wallet/example/`

## Installing in a React Native app

Typical install flow:

```bash
npm install react-native-pirate-wallet
cd ios && pod install
```

Android:

- the module autolinks like a normal React Native native module

iOS:

- CocoaPods links the vendored `PirateWalletNative.xcframework`

## Mnemonic language support

The React Native plugin now supports explicit BIP39 seed phrase language
handling for:

- wallet creation
- wallet restore
- mnemonic generation
- mnemonic validation
- mnemonic inspection
- advanced seed export

Those additions live on the same broad JS SDK surface as the rest of the
wallet operations.

## Build checks in this repo

The React Native plugin CI path stages the native SDK artifacts and then checks:

- JavaScript smoke test
- Android native bridge build
- React Native example app test
- React Native example app Android build
- React Native example app iOS build on macOS

The workflow is defined in:

- `.github/workflows/ci.yml`

The staging step is:

- `scripts/prepare-react-native-plugin.sh`

## Local checks

Useful commands:

```bash
bash scripts/prepare-react-native-plugin.sh
node bindings/react-native-pirate-wallet/test/smoke.js

cd bindings/react-native-pirate-wallet/android
gradle --no-daemon assembleDebug

cd ../example
npm install
npm test -- --runInBand

cd android
./gradlew --no-daemon assembleDebug
```

## Maintenance notes

When adding a new React Native API:

1. add or reuse the Rust backend method in `pirate-wallet-service`
2. expose it in the native SDKs if the platform wrappers need changes
3. update the React Native bridge code
4. update `src/index.js`
5. update `src/index.d.ts`
6. update `bindings/react-native-pirate-wallet/README.md`
7. rerun the staging and smoke checks

Keep the React Native layer thin. If a change belongs in the shared wallet backend, put it there first.
