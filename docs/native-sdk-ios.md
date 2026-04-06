# iOS SDK

The iOS SDK in this repo is the Swift wrapper over the shared Rust wallet backend.

Relevant paths:

- `bindings/ios-sdk/`
- `crates/pirate-ffi-native/`
- `crates/pirate-wallet-service/`

The Swift code is a typed wrapper. Wallet behavior lives in the Rust service layer.

## Scope

The iOS SDK is intended to match the Android SDK boundary.

That includes:

- wallet create and restore
- watch-only wallet import
- receive addresses
- balances
- optional Sapling and Orchard split balances
- sync control
- a polling synchronizer wrapper
- send, build, sign, and broadcast
- shielded address validation
- consensus branch validation
- transaction details, recipients, and memo lookup
- viewing key export and watch-only import

There is also an advanced key-management surface for higher-risk operations:

- list key groups
- export Sapling and Orchard viewing keys
- export Sapling and Orchard spending keys
- import Sapling and Orchard spending keys
- raw seed export

Those live under `sdk.advancedKeyManagement`.

The boundary is the same one used for the Android SDK.

## Build

Generate the native header:

```bash
bash scripts/build-native-ffi.sh
```

Package the iOS SDK:

```bash
bash scripts/build-ios-sdk.sh
```

That script:

- builds `pirate-ffi-native` for device and simulator targets
- creates `PirateWalletNative.xcframework`
- stages the Swift wrapper sources
- writes release bundles under `dist/ios-sdk/`

This packaging step requires macOS and Xcode.

## Outputs

Release outputs:

- `bindings/ios-sdk/Frameworks/PirateWalletNative.xcframework`
- `dist/ios-sdk/PirateWalletNative.xcframework.zip`
- `dist/ios-sdk/PirateWalletSDK-package.zip`

The XCFramework zip is the binary artifact.

The package zip contains the Swift wrapper sources plus the XCFramework layout expected by the checked-in Swift package.

## Package layout

The Swift package is in:

- `bindings/ios-sdk/Package.swift`

The package currently keeps:

- `swift-tools-version: 5.9`

It is the minimum package-tools requirement.

The CI build can use a newer Xcode and newer Swift compiler without forcing the package manifest to require the newest toolchain.

The wrapper sources are in:

- `bindings/ios-sdk/Sources/PirateWalletSDK/`

Full method and type reference:

- `docs/native-sdk-ios-api.md`

Current wrapper files:

- `PirateWalletSDK.swift`
- `PirateWalletSDKModels.swift`
- `PirateWalletSynchronizer.swift`

## Public API

The public Swift surface mirrors the Android SDK structure:

- `PirateWalletSDK`
- `PirateWalletSynchronizer`
- `PirateWalletAdvancedKeyManagement`

The typed SDK now exposes both:

- synchronous methods, for simple tooling and tests
- async `...Async` counterparts, for app integrations that should avoid blocking the caller thread

For normal app usage, prefer the async methods over the synchronous ones.

Birthday height is exposed in two places:

- `sdk.getLatestBirthdayHeight(walletId:)`
- `PirateWalletSynchronizer.latestBirthdayHeight`

That value comes from the wallet metadata already stored by the backend.

The synchronizer keeps its published state on the main actor, but its wallet-service
polling work runs through a dedicated background invocation queue so sync refreshes
do not block the UI thread.

Examples:

```swift
let wallets = try await sdk.listWalletsAsync()
let balance = try await sdk.getBalanceAsync(walletId: walletId)
let txid = try await sdk.sendAsync(walletId: walletId, output: output)
let seed = try await sdk.advancedKeyManagement.exportSeedAsync(walletId: walletId)
```

## Maintenance notes

Source of truth:

- business logic and JSON method handling: `crates/pirate-wallet-service/src/service.rs`
- native C ABI: `crates/pirate-ffi-native/src/lib.rs`
- Swift typed wrapper: `bindings/ios-sdk/Sources/PirateWalletSDK/`

When adding a new iOS SDK method:

1. add or reuse the Rust service method in `pirate-wallet-service`
2. expose it in `crates/pirate-wallet-service/src/service.rs`
3. add the typed Swift wrapper and model decoding
4. keep the iOS and Android SDK boundaries aligned
5. rebuild the XCFramework package on macOS

## Verification

The Swift wrapper has not been host-verified yet because iOS packaging requires macOS and Xcode.

The matching backend and C ABI layers were verified from this repo:

```bash
cd crates
cargo check -p pirate-wallet-service -p pirate-ffi-native
```

On a macOS builder, the normal verification path is:

```bash
bash scripts/build-native-ffi.sh
bash scripts/build-ios-sdk.sh
```

## CI

The iOS SDK CI path now does two things on a macOS runner:

- builds the XCFramework package
- builds and tests the Swift package wrapper

The CI job also selects Xcode explicitly instead of relying on the runner default:

- prefers Xcode 26.3
- falls back to Xcode 26.2 if 26.3 is not installed on the runner image

That means CI should catch wrapper compile breakage and package layout problems after commit.
