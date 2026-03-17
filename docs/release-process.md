# Release Process

This document describes the release process for Pirate Unified Wallet as it is implemented in this repository.

Release inputs
--------------

Before building release artifacts:

- update the application version in `app/pubspec.yaml`
- ensure Rust, Flutter, and dependency checks pass
- ensure platform signing inputs are available where required
- ensure release notes and published checksums will be prepared with the artifacts

Versioning from tags
--------------------

Release builds use `scripts/sync-version-from-tag.sh` before platform packaging.

For a tag such as:

```text
v1.1.1
```

the script updates `app/pubspec.yaml` for the build so that Flutter platform metadata uses:

- build name: `1.1.1`
- build number: `1` by default, unless `VERSION_BUILD_NUMBER` is set

That version then flows into:

- Android `versionName` and `versionCode`
- iOS `CFBundleShortVersionString` and `CFBundleVersion`
- macOS `CFBundleShortVersionString` and `CFBundleVersion`
- Windows `FileVersion` and `ProductVersion`
- the in-app settings version display via `package_info_plus`

Rust build info used by the Verify Build screen is also resolved from `app/pubspec.yaml`, so it matches the app release version instead of the crate workspace version.

Required checks
---------------

Run the checks appropriate to the platform and the changes in the release:

```bash
cd crates
cargo fmt --all -- --check
cargo clippy --all-targets --all-features --locked -- -D warnings
cargo test --all-features --locked
cd ..

cd app
flutter pub get --enforce-lockfile
flutter analyze
cd ..
```

Platform build scripts
----------------------

Use the committed build scripts under `scripts/` for release packaging.

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
bash scripts/build-ios.sh true
```

Nix-backed native entry points
------------------------------

The repository flake exposes the same native packaging paths through Nix:

- Linux hosts
  - `nix build .#linux-appimage`
  - `nix build .#linux-flatpak`
  - `nix build .#linux-deb`
  - `nix build .#android-apk`
  - `nix build .#android-bundle`
- macOS hosts
  - `nix build .#macos-dmg`
  - `nix build .#ios-ipa`

Windows packaging remains script-driven through `scripts/build-windows.sh`.

Signing behavior
----------------

Signing behavior depends on platform and environment:

- Windows
  - signing is controlled by the variables consumed by `scripts/build-windows.sh`
  - unsigned artifacts are produced when signing inputs are not present
- macOS
  - `scripts/build-macos.sh` supports Developer ID signing and optional notarization
- Android
  - `scripts/build-android.sh` signs only when keystore inputs are provided
- iOS
  - `scripts/build-ios.sh true` requires a valid Xcode signing configuration

Artifact naming
---------------

Current script outputs are:

- Windows
  - `pirate-unified-wallet-windows-installer.exe`
  - `pirate-unified-wallet-windows-installer-unsigned.exe`
  - `pirate-unified-wallet-windows-portable.zip`
  - `pirate-unified-wallet-windows-portable-unsigned.zip`
- Linux
  - `pirate-unified-wallet-linux-x86_64.AppImage`
  - `pirate-unified-wallet.flatpak`
  - `pirate-unified-wallet-amd64.deb`
- macOS
  - `pirate-unified-wallet-macos.dmg`
  - `pirate-unified-wallet-macos-unsigned.dmg`
- Android
  - split APK outputs named by ABI
  - signed and unsigned variants
  - `pirate-unified-wallet-android.aab`
  - `pirate-unified-wallet-android-unsigned.aab`
- iOS
  - `pirate-unified-wallet-ios.ipa`
  - `pirate-unified-wallet-ios-unsigned.ipa`

Checksums, SBOMs, and provenance
--------------------------------

After packaging, generate or verify release metadata:

```bash
scripts/generate-sbom.sh dist/sbom
scripts/generate-provenance.sh <artifact> dist/provenance
```

Each published release should include readable checksum data for the distributed artifacts. The Verify Build screen and desktop updater depend on that.

Release publication checklist
-----------------------------

- artifacts built from committed sources
- checksums published
- release notes prepared
- signed artifacts used where intended
- unsigned artifacts retained where deterministic verification is needed
- updater asset names match the published artifact names
- Verify Build can resolve published checksums for the release
