Pirate Unified Wallet
=====================

Pirate Unified Wallet is a cross-platform Pirate Chain wallet with a Flutter user interface and a Rust core. The repository includes the application, the Rust wallet and sync crates, build scripts, and the project-owned documentation used for release and verification work.

This repository is under active development. Before distributing builds, review the release notes, the security notes, and the verification instructions in `docs/`.

Repository layout
-----------------

- `app/`  
  Flutter application code, desktop packaging hooks, generated localization files, and the desktop updater.
- `crates/`  
  Rust wallet, storage, sync, FFI, and supporting crates.
- `docs/`  
  Project-owned documentation for security, build verification, and localization.
- `scripts/`  
  Platform build, packaging, SBOM, provenance, and asset-fetch scripts.
- `generate_ffi_bindings.sh`  
  Generates Flutter Rust Bridge bindings for the app and Rust FFI layer.

Supported build outputs
-----------------------

Current build scripts produce the following release artifacts:

- Windows: installer `.exe` and portable `.zip`
- Linux: `.AppImage`, `.flatpak`, and `.deb`
- macOS: `.dmg`
- Android: split APKs and `.aab`
- iOS: `.ipa`

Platform packaging is handled by the scripts in `scripts/`. A plain `flutter build` is useful for development, but it does not replace the release packaging scripts.

Toolchain
---------

The project is built and tested in CI with these pinned versions:

- Rust `1.90.0`
- Flutter `3.41.1`
- `flutter_rust_bridge_codegen` `2.11.1`
- CocoaPods `1.16.2` for macOS and iOS builds

The Rust pin is defined in `rust-toolchain.toml`. CI pins are defined in `.github/workflows/ci.yml`.

To check local tools against the current pins:

```bash
FLUTTER_VERSION=3.41.1 \
COCOAPODS_VERSION=1.16.2 \
scripts/verify-toolchain.sh
```

Nix flake
---------

The repository includes a checked-in flake that mirrors the committed native build scripts.

Development shells:

```bash
nix develop
nix develop .#ci
nix develop .#build
```

Native flake package outputs:

- Linux hosts:
  - `.#linux-appimage`
  - `.#linux-flatpak`
  - `.#linux-deb`
  - `.#android-apk`
  - `.#android-bundle`
- macOS hosts:
  - `.#macos-dmg`
  - `.#ios-ipa`

Windows packaging is not exposed through the flake. Use `scripts/build-windows.sh` for Windows release artifacts.

Build prerequisites
-------------------

Common requirements:

- Rust toolchain with `rustfmt` and `clippy`
- Flutter stable SDK
- `flutter_rust_bridge_codegen`
- `protoc`

Platform-specific requirements:

- Windows:
  - Visual Studio 2022 C++ build tools
  - OpenSSL
  - PowerShell
  - Inno Setup if you want the installer artifact
- Linux:
  - Flutter Linux desktop dependencies
  - `flatpak-builder` for Flatpak output
  - `dpkg-deb` and `dpkg-scanpackages` for Debian and APT output
  - `appimagetool` or pinned `APPIMAGETOOL_URL` and `APPIMAGETOOL_SHA256`
- macOS:
  - Xcode and CocoaPods
  - Apple signing and notarization credentials for signed distribution
- Android:
  - Android SDK and NDK
  - Java runtime compatible with your Android toolchain
- iOS:
  - macOS
  - Xcode
  - CocoaPods
  - Apple signing configuration for signed IPA export

Getting started
---------------

1. Fetch Flutter dependencies:

```bash
cd app
flutter pub get --enforce-lockfile
cd ..
```

2. Generate Flutter Rust Bridge bindings:

```bash
bash generate_ffi_bindings.sh
```

3. Build the target you need.

Release build commands
----------------------

Windows:

```bash
bash scripts/build-windows.sh
```

Linux:

```bash
bash scripts/build-linux.sh appimage
bash scripts/build-linux.sh flatpak
bash scripts/build-linux.sh deb
```

macOS:

```bash
bash scripts/build-macos.sh
```

Android:

```bash
bash scripts/build-android.sh apk
bash scripts/build-android.sh bundle
```

iOS:

```bash
bash scripts/build-ios.sh false
```

The desktop build scripts fetch and verify the pinned Tor Browser and i2pd assets before packaging. If you intentionally want to skip that step for a local build, set `SKIP_TOR_I2P_FETCH=1`.

Development notes
-----------------

- The generated Flutter FFI files live under `app/lib/core/ffi/generated/`.
- The generated localization files live under `app/lib/l10n/`.
- Rust quality checks:

```bash
cd crates
cargo fmt --all
cargo clippy --all-targets --all-features --locked -- -D warnings
cargo test --all-features --locked
```

Documentation index
-------------------

- Build verification: `docs/verify-build.md`
- Security notes: `docs/security.md`
- Release process: `docs/release-process.md`
- Translation workflow: `docs/localization/TRANSLATION_WORKFLOW.md`
- Contribution guide: `CONTRIBUTING.md`
- Flutter app notes: `app/README.md`
- UI structure: `app/DESIGN_SYSTEM.md`

Release and verification outputs
--------------------------------

Project scripts also generate or consume:

- SHA-256 checksum files for release artifacts
- SBOMs via `scripts/generate-sbom.sh`
- provenance files via `scripts/generate-provenance.sh`

For verification and release handling details, use the documents under `docs/` rather than this README.
