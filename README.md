Pirate Unified Wallet
=====================

Warning: This software is under active development and should not be used outside of testing

Cross-platform Pirate Chain wallet with Rust core and Flutter UI. Full README and
extended docs are coming soon.

Quick Build (Windows + Android)
-------------------------------

Prereqs:
- Rust toolchain (stable)
- Flutter SDK (stable)
- Android SDK/NDK (for Android builds)
- Visual Studio 2022 (Windows desktop build)
- OpenSSL (Windows desktop build)

1) Install dependencies

- Windows: install Rust + Flutter + VS Build Tools + OpenSSL.
- Android: install Android Studio + SDK/NDK, then run `flutter doctor`.

2) Fetch Tor/I2P assets (desktop builds only)

The build scripts pull official Tor Browser bundles + i2pd, verify pinned hashes,
extract Snowflake/obfs4, and bundle them into desktop builds.

- macOS/Linux:
```
bash scripts/fetch-tor-i2p-assets.sh
```

- Windows (requires 7-Zip or set `SEVEN_ZIP_PATH`):
```
powershell -ExecutionPolicy Bypass -File scripts\fetch-tor-i2p-assets.ps1
```

Optional overrides (advanced):
- Tor Browser:
  - `TOR_BROWSER_VERSION` (default 15.0.5)
  - `TOR_BROWSER_BASE_URL` (defaults to `https://dist.torproject.org/torbrowser/$TOR_BROWSER_VERSION`)
  - `TOR_BROWSER_LINUX_URL` / `TOR_BROWSER_MACOS_URL` / `TOR_BROWSER_WINDOWS_URL`
  - `TOR_BROWSER_LINUX_SHA256` / `TOR_BROWSER_MACOS_SHA256` / `TOR_BROWSER_WINDOWS_SHA256`
- i2pd:
  - `I2PD_VERSION` (default 2.58.0)
  - `I2PD_BASE_URL`
  - `I2PD_LINUX_AMD64_SHA512` / `I2PD_LINUX_ARM64_SHA512`
  - `I2PD_MACOS_SHA512` / `I2PD_WINDOWS_SHA512`
- Set `SKIP_TOR_I2P_FETCH=1` to skip fetching assets.

3) Generate bindings

From repo root:

```
bash generate_ffi_bindings.sh
```

4) Build Rust libs (multi-target)

- Windows (DLL):
```
powershell -ExecutionPolicy Bypass -File .\build-rust-libs.ps1 -Windows
```

- Android (MSYS2 shell) or WSL:
```
bash scripts/build-android-msys2.sh
```
or
```
bash build-android-wsl.sh
```

5) Build Flutter apps (multi-target)

- Windows:
```
cd app
flutter build windows --release
```

- Android (APK split per ABI):
```
cd app
flutter build apk --release --split-per-abi
```

Outputs
-------
- Windows: `app/build/windows/x64/runner/Release/`
- Android: `app/build/app/outputs/flutter-apk/`

Reproducible Packaging Notes
----------------------------
- Windows MSIX packaging: requires `msix_config` in `app/pubspec.yaml`. Set
  `MSIX_VERSION` to install a pinned `msix` tool if it is not already installed.
- Linux AppImage packaging: install `appimagetool` or set `APPIMAGETOOL_URL` and
  `APPIMAGETOOL_SHA256` to download a pinned binary.
- Android: release builds are unsigned by default for reproducibility. Use
  `scripts/build-android.sh <apk|bundle> true` to sign after building.
- Set `REPRODUCIBLE=1` to force unsigned outputs (skip signing/notarization).

Reproducible Toolchain Pins
---------------------------
- Rust: `1.90.0` (see `rust-toolchain.toml`)
- Flutter: `3.41.1` (stable)
- Java: `21`
- Gradle: `8.11.1` (see `app/android/gradle/wrapper/gradle-wrapper.properties`)
- Android build-tools: `34.0.0` (apksigner)
- Android platform: `36`
- CocoaPods: `1.16.2`
- flutter_rust_bridge_codegen: `2.11.1`
- Syft: `1.40.1`

Verify versions:
```
scripts/verify-toolchain.sh
```
