# Flutter Application

This directory contains the Flutter user interface for Stashi Wallet.

Contents
--------

- `lib/`
  - application features
  - routing
  - desktop integration
  - build verification UI
  - localization files
  - generated Flutter Rust Bridge bindings
- `android/`, `ios/`, `linux/`, `macos/`, `windows/`
  - platform runners and packaging integration
- `assets/`
  - icons, fonts, and other packaged resources

Generated files
---------------

These files are generated and should be refreshed through project tooling rather than edited by hand:

- `lib/core/ffi/generated/`
- platform plugin registrants under the Flutter runner directories

Refresh the runtime English catalog with
`dart run tool/audit_runtime_translations.dart --write`; see the translation
workflow before adding a locale.

Common commands
---------------

Install dependencies:

```bash
flutter pub get --enforce-lockfile
```

Audit translations:

```bash
dart run tool/audit_runtime_translations.dart
```

Build app-only outputs for development from the repository root. The preflight
checksum-prefetches the selected KDF binary and SDK-pinned coin snapshot,
materializes the required configuration and images, and disables dependency
asset-transformer network access:

```bash
(cd app && flutter pub get --enforce-lockfile)
bash scripts/prepare-flutter-build.sh windows
(cd app && flutter build windows --release)
```

For desktop and iOS builds, replace both occurrences of `windows` with
`linux`, `macos`, or `ios`. For Android, pass `android` to the preflight and
then build either the `apk` or `appbundle` Flutter target. Do not invoke a raw
Flutter build after dependency resolution without rerunning the preflight.

Run unit and widget tests through `bash scripts/test-flutter.sh`; it bypasses
asset transformers without removing any assets from packaged applications.

Release packaging
-----------------

Release packaging is driven from the repository root through the scripts in `../scripts/`.

Use the root-level build scripts when you need the packaged outputs that are published in releases:

- `../scripts/build-windows.sh`
- `../scripts/build-linux.sh`
- `../scripts/build-macos.sh`
- `../scripts/build-android.sh`
- `../scripts/build-ios.sh`

Related documentation
---------------------

- Stashi Wallet user guide: `../docs/user-guide/README.md`
- root build and repository notes: `../README.md`
- security notes: `../docs/security.md`
- build verification: `../docs/verify-build.md`
- translation workflow: `../docs/localization/TRANSLATION_WORKFLOW.md`
- UI structure: `DESIGN_SYSTEM.md`
