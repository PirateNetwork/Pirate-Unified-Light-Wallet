# Android SDK

The Android SDK in this repo is the Android wrapper over the shared Rust wallet backend.

Relevant paths:

- `bindings/android-sdk/`
- `crates/pirate-ffi-native/`
- `crates/pirate-wallet-service/`

The Android code is only a typed wrapper. Wallet behavior lives in the Rust service layer.

## What it covers

The SDK is built for the shielded wallet model used by the unified wallet.

That includes:

- wallet create and restore
- watch-only wallet import
- receive addresses
- balances
- optional Sapling and Orchard split balances
- sync control
- a polling synchronizer surface
- send, build, sign, and broadcast
- shielded address validation
- consensus branch validation
- transaction details, recipients, and memo lookup
- viewing key export and watch-only import

There is also a separate advanced key-management surface for higher-risk operations:

- list key groups
- export Sapling and Orchard viewing keys
- export Sapling and Orchard spending keys
- import Sapling and Orchard spending keys
- raw seed export

Those live under `sdk.advancedKeyManagement`.

It deliberately does not expose:

- transparent address and balance APIs
- transparent shielding flows
- rewind helpers
- old callback-style processor error hooks

## Build

Build the native header:

```bash
bash scripts/build-native-ffi.sh
```

Build the Android SDK:

```bash
bash scripts/build-android-sdk.sh
```

That script:

- builds the JNI libraries from `pirate-ffi-native`
- strips unneeded native symbols with the NDK toolchain before Gradle packages the AAR
- runs Android unit tests
- builds the release AAR
- writes release bundles under `dist/android-sdk/`

The script also defaults `GRADLE_USER_HOME` to a repo-local cache so local builds do not depend on a host-global Gradle cache.

## Outputs

Release outputs:

- `bindings/android-sdk/build/outputs/aar/pirate-android-sdk-release.aar`
- `dist/android-sdk/pirate-android-sdk-package.zip`

The AAR is the normal delivery artifact.

The package zip is there for teams that want to vendor the whole Gradle module instead of only consuming the AAR.

## Using it

If you only want the binary artifact, copy the AAR into the consuming Android project and reference it directly:

```gradle
dependencies {
    implementation(files("libs/pirate-android-sdk-release.aar"))
}
```

If you want the full module layout, use `pirate-android-sdk-package.zip`.

## Public entry points

The public Kotlin surface is in:

- `bindings/android-sdk/src/main/kotlin/com/pirate/wallet/sdk/PirateWalletSdk.kt`
- `bindings/android-sdk/src/main/kotlin/com/pirate/wallet/sdk/PirateWalletSdkModels.kt`
- `bindings/android-sdk/src/main/kotlin/com/pirate/wallet/sdk/PirateWalletSynchronizer.kt`

Full method and type reference:

- `docs/native-sdk-android-api.md`

Important entry points:

- `PirateWalletSdk`
- `PirateWalletSynchronizer`
- `PirateWalletSdk.advancedKeyManagement`

Birthday height is exposed in two places:

- `sdk.getLatestBirthdayHeight(walletId)`
- `PirateWalletSynchronizer.latestBirthdayHeight`

That value comes from the wallet metadata already stored by the backend.

## Differences from the old Android SDK

The older SDK had processor-heavy and transparent-wallet-heavy surfaces that are not part of this one.

Examples that are not included here:

- `processorInfo`
- rewind helpers
- callback-style processor and chain error hooks
- `new`, `newBlocking`, and `erase`
- transparent wallet flows

For the current unified-wallet design, this SDK is the supported Android surface.

## Working on it

When adding a new Android SDK method:

1. add or reuse the Rust service method in `pirate-wallet-service`
2. expose it in `crates/pirate-wallet-service/src/service.rs`
3. add the typed Kotlin wrapper and parsers
4. add or update JVM tests
5. rebuild the AAR

## Checks

Useful commands:

```bash
cd crates
cargo check -p pirate-wallet-service -p pirate-ffi-native

cd ../bindings/android-sdk
ANDROID_HOME=/opt/android-sdk \
ANDROID_SDK_ROOT=/opt/android-sdk \
JAVA_HOME=/usr/lib/jvm/java-21-openjdk-amd64 \
GRADLE_USER_HOME=/tmp/gradle-pirate-android-sdk \
./gradlew --no-daemon test compileReleaseKotlin

ANDROID_HOME=/opt/android-sdk \
ANDROID_SDK_ROOT=/opt/android-sdk \
JAVA_HOME=/usr/lib/jvm/java-21-openjdk-amd64 \
GRADLE_USER_HOME=/tmp/gradle-pirate-android-sdk \
./gradlew --no-daemon assembleRelease
```

## CI

The Android SDK CI path now checks three things:

- the Rust JNI library build
- the Android SDK unit tests and release AAR build
- a separate Android smoke-consumer app module that compiles against the public SDK surface

