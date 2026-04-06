# Migration Notes

This document tracks the migration path from the older Pirate light wallet deliverables to the unified wallet repo.

Scope
-----

This repository is intended to replace the older split surfaces over time:

- `piratewallet-light-cli`
- `qortal-piratewallet-light-cli`
- `PirateLightClientKit`
- `pirate-android-wallet-sdk`

Reference repos
---------------

The local compatibility references live under `reference_repos/`.

Use these branches as the current references:

- `reference_repos/piratewallet-light-cli`: `dev`
- `reference_repos/qortal-piratewallet-light-cli`: `master`
- `reference_repos/PirateLightClientKit`: `master`
- `reference_repos/pirate-android-wallet-sdk`: `master`
- `reference_repos/react-native-piratechain`: `master`

Current replacement status
--------------------------

Base CLI:

- new repo-owned binary: `crates/piratewallet-cli`
- shared backend: `crates/pirate-wallet-service`
- covers common wallet operations plus legacy-compatible key, seed, address, sync, export, notes, and clear aliases for most reference CLI workflows
- remaining CLI gaps: offline memo encryption/decryption, send progress, and price lookup

Flutter bridge:

- `crates/pirate-ffi-frb` now acts as a thin Flutter adapter over the shared backend surface
- app-facing wallet logic lives in `crates/pirate-wallet-service`

Qortal CLI:

- new repo-owned adapter: `crates/pirate-qortal-cli`
- current state: Qortal schema compatibility exists for `syncstatus`, `balance`, `list`, `sendp2sh`, and `redeemp2sh`
- current limitation: `sendp2sh` funds from wallet-owned shielded notes, not from a transparent wallet balance

iOS SDK:

- repo-owned native FFI: `crates/pirate-ffi-native`
- repo-owned wrapper/package path: `bindings/ios-sdk`
- typed Swift SDK surface now mirrors the Android SDK boundary
- local XCFramework packaging path exists
- remaining gap: host verification and release-time Swift Package Manager binary-target publication still need a finalized macOS-backed distribution flow

Android SDK:

- repo-owned native FFI: `crates/pirate-ffi-native`
- repo-owned Android module: `bindings/android-sdk`
- typed Kotlin SDK surface exists, JVM tests exist, and release AAR packaging works
- release artifacts: AAR plus module package zip
- intentional scope: shielded-first unified-wallet SDK, not a full transparent-wallet compatibility clone
- advanced APIs: the mobile SDK surfaces keep passphrase-gated seed export plus Sapling and Orchard spending-key import/export under advanced key management

- CLI keeps the legacy raw `seed` export command for operator and integration use, while the Flutter wallet uses the separate gated `seed-export` flow

Edge path
---------

Edge currently depends on the older split native SDK repos and combines them inside a React Native wrapper.

The replacement path from this repo is now:

1. use `pirate-ffi-native` as the shared native Rust layer
2. stage the repo-owned Android JNI libraries and iOS XCFramework into `bindings/react-native-pirate-wallet`
3. consume the repo-owned React Native package

React Native package:

- repo-owned package path: `bindings/react-native-pirate-wallet`
- JS wrapper: mirrors the shielded-first SDK surface
- Android native bridge: JNI-based bridge over `libpirate_ffi_native.so`
- iOS native bridge: Swift bridge over `PirateWalletNative.xcframework`

What is still not finished
--------------------------

- release-grade hosted SDK publication for Swift Package Manager
