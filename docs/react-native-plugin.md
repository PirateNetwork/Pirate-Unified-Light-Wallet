# React Native Plugin

The React Native packages in this repo live in:

- `bindings/react-native-pirate-wallet/`
- `bindings/react-native-pirate-wallet-android/`
- `bindings/react-native-pirate-wallet-android-x86_64/`
- `bindings/react-native-pirate-wallet-ios-device/`
- `bindings/react-native-pirate-wallet-ios-simulator-arm64/`
- `bindings/react-native-pirate-wallet-ios-simulator-x86_64/`

The public package contains the JavaScript API and platform bridges. The five
companion packages contain the native Android and iOS binaries. The simulator
architectures are published separately so no npm tarball carries two copies of
the Rust dependency graph; CocoaPods combines them into a universal simulator
slice during installation.

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
  - Objective-C bridge over `PirateWalletNative.xcframework`
- JavaScript
  - typed wrapper and polling synchronizer

The JavaScript surface mirrors the shielded-first SDK boundary used by the native Android and iOS SDKs.

Amount values that cross the React Native JSON boundary are decimal strings,
not JSON numbers. This applies to balances, fees, transaction amounts, pending
transaction totals, payment disclosure amounts, and `parseAmount()` results.
The JS wrapper accepts decimal strings, safe integer numbers, or `bigint` for
amount request fields and serializes them as strings before native invocation.

## Wallet and sync model

Wallet metadata lives in the backend registry for the configured storage
namespace. The registry stores an active wallet ID for flows that need a
current-wallet pointer, while most React Native SDK methods remain explicitly
wallet-scoped through `walletId`.

React Native apps should call `configureSecureAccountStorage()` before any
wallet operation:

```js
await sdk.configureSecureAccountStorage({
  accountId: edgeAccountIdHash
})
```

The account ID is used only to derive an app-private storage directory name and
platform credential identifier. The native bridge generates a random registry
credential, protects it with iOS Keychain or Android Keystore, and does not
return it to JavaScript. `configureAccountStorage()` remains available for
hosts that already provide equivalently protected credential storage.

The selected account namespace contains the wallet registry, per-wallet
databases, salts, and sealed database key files. Switching namespaces clears the
loaded registry state, active-wallet state, database caches, endpoint caches,
and sync caches before opening the requested account namespace.

`switchWallet(walletId)` updates the active-wallet pointer and stops sync for
the previously active wallet. Apps that sync more than one wallet should create
separate synchronizers by wallet ID.

Each wallet has independent sync state. Compact block ranges are cached per
endpoint, so later scans for another wallet on the same endpoint can reuse
previously fetched ranges, while concurrent sync still shares device, network,
and lightwalletd resources.

Endpoint selection is also wallet-scoped. React Native consumers have typed
methods to read the effective endpoint, inspect its complete configuration,
probe the active endpoint and pool health, test a candidate, set a pinned or unpinned single endpoint, and set an explicit
failover pool. The pool setter accepts one primary and at most 16 alternates;
the Rust service remains authoritative for same-chain, route, TLS, pin, and
duplicate validation. Saving a new endpoint cancels stale sync work, so the
consumer must restart that wallet's synchronizer after the setter succeeds.

The public package README is the normative JavaScript reference for
`getLightdEndpoint`, `getLightdEndpointConfig`,
`getLightdEndpointPoolDiagnostics`, `testLightdEndpoint`,
`setLightdEndpoint`, and `setLightdEndpointPool`, including their TypeScript
request and response shapes.

Receive-address access is split into `getCurrentAddress(walletId)`,
`getNextAddress(walletId)`, `listAddresses(walletId)`, and
`listAddressBalances(walletId, keyId?)`. These APIs return shielded receive
addresses. Newly generated addresses use Sapling before Ironwood activation and
Ironwood after activation. At activation, current- and next-address calls both
select Ironwood while existing Sapling addresses remain valid.

Without `keyId`, `listAddressBalances` lists external receive-address rows and
omits internal change-address rows. Supplying a key ID includes both scopes for
that key group. Internal change remains part of `getBalance(walletId)`, which is
the wallet-total API.

Transaction helpers are wallet-scoped, including
`broadcastTransaction(walletId, signed)`. This keeps endpoint failover, unknown
anchor repair, and post-broadcast persistence attached to the wallet that
created the transaction. A wallet ID is required for every broadcast.

Spendability has four typed reason codes: `OK`, `ERR_SYNC_FINALIZING`,
`ERR_WITNESS_REPAIR_QUEUED`, and `ERR_RESCAN_REQUIRED`. Repair remains durable
while queued or processing and clears only after rebuilt witnesses and the
selected anchor validate.

Hosts that need account-level signing isolation can opt in with
`enableWalletSigningProtection()`. The backend then wraps spend-capable material
per wallet while viewing data and compact-block cache access continue normally.
After each account unlock, call `unlockWalletSigning()`. When the account locks,
call `lockWalletSigning()` or `lockAllWalletSigning()` to clear session keys and
cached wallet database handles.

Payment disclosure helpers are also wallet-scoped. `exportPaymentDisclosures`
returns the Bech32 disclosure keys the wallet can derive for a sent transaction.
Each disclosure is scoped to one Sapling output or Ironwood action, so sharing it
lets a third party verify that specific payment without exposing the wallet's
other transactions. `verifyPaymentDisclosure` uses the selected wallet's
lightwalletd endpoint to fetch the transaction and decrypt the disclosed output.

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

- the Android ARM and x86_64 companion packages
- the iOS device and two architecture-specific simulator companion packages

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
- `bindings/react-native-pirate-wallet-android/package.json`
- `bindings/react-native-pirate-wallet-android-x86_64/package.json`
- `bindings/react-native-pirate-wallet-ios-device/package.json`
- `bindings/react-native-pirate-wallet-ios-simulator-arm64/package.json`
- `bindings/react-native-pirate-wallet-ios-simulator-x86_64/package.json`

The package README carries the JavaScript API and RPC reference:

- `bindings/react-native-pirate-wallet/README.md`

The example app is the minimal real consumer used by CI:

- `bindings/react-native-pirate-wallet/example/`

Release CI creates and tests npm tarballs for the public wrapper and five
native companions. They use the `react_native_plugin` version from
`release-artifacts.toml`; publication sends the native packages before the
wrapper.

The iOS SDK build uses a release-only Cargo configuration with debug metadata
and incremental compilation disabled. It deliberately keeps LTO disabled and
uses normal release code-generation granularity because the SDK output is an
intermediate static archive, not a final executable. This lets downstream
linkers load only reachable archive members and avoids the large package
regression caused by coalescing the Rust dependency graph into one unit.

The build removes DWARF data and local symbols from each thin archive, verifies
the exact device and simulator architectures, and measures each publishable
device or simulator archive against the compressed npm package budget. It also
creates the universal simulator XCFramework required by SwiftPM, but that
combined archive is not an npm publish unit. Package verifiers reject
unrecognized files, so debug bundles and other build by-products cannot
silently enter an npm release. Temporary Swift package staging is deleted
after its archive is created instead of being uploaded as a duplicate SDK
payload.

### Bootstrapping a new npm companion

npm trusted publishing can update an existing package, but the package must
already exist before its GitHub Actions trust relationship can be configured.
When a new native companion name is introduced:

1. let the tagged CI run build and verify the complete npm artifact
2. download that run's exact `.tgz` and checksum
3. publish the new public package once with a maintainer account and 2FA
4. configure its npm trusted publisher for `ci.yml`, the
   `PirateNetwork/Pirate-Unified-Light-Wallet` repository, and the
   `npm-publish` environment with `npm publish` permission
5. rerun the tagged npm publication job

The release job compares registry integrity with its local tarball. It skips an
identical bootstrap publication and continues with the remaining packages; it
fails instead of accepting different contents under the same version.

## Installing in a React Native app

Typical install flow:

```bash
npm install react-native-pirate-wallet
cd ios && pod install
```

Android:

- npm installs the exact-version ARM and x86_64 companions automatically
- the module autolinks like a normal React Native native module
- `configureSecureAccountStorage()` derives account directories under
  `Context.filesDir/pirate_wallet/accounts/<sanitized-account-id>` unless the
  caller provides `storagePath`

iOS:

- npm installs the exact-version device and simulator companion packages
- CocoaPods assembles and links `PirateWalletNative.xcframework` during
  `pod install`
- `configureSecureAccountStorage()` derives account directories under
  `Application Support/PirateWallet/accounts/<sanitized-account-id>` unless the
  caller provides `storagePath`

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

## Change-address policy

The React Native bridge does not expose a change-address override. Send helpers
inherit the shared backend policy automatically: Sapling-only change uses legacy
same-address change before Ironwood activation and Sapling internal change after
activation; Ironwood spends or outputs use Ironwood internal change.

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
6. update the package README and integration guide
7. rerun the staging and smoke checks

Keep the React Native layer thin. If a change belongs in the shared wallet backend, put it there first.
