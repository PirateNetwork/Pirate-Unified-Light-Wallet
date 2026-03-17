# Flutter Application

This directory contains the Flutter user interface for Pirate Unified Wallet.

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
- `lib/l10n/app_localizations*.dart`
- platform plugin registrants under the Flutter runner directories

Common commands
---------------

Install dependencies:

```bash
flutter pub get --enforce-lockfile
```

Generate localization code:

```bash
flutter gen-l10n
```

Build app-only outputs for development:

```bash
flutter build windows --release
flutter build linux --release
flutter build macos --release
flutter build apk --release --split-per-abi
flutter build appbundle --release
flutter build ios --release --no-codesign
```

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

- root build and repository notes: `../README.md`
- security notes: `../docs/security.md`
- build verification: `../docs/verify-build.md`
- translation workflow: `../docs/localization/TRANSLATION_WORKFLOW.md`
- UI structure: `DESIGN_SYSTEM.md`
