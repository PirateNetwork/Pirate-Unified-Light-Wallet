Pirate Unified Wallet
=====================

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

2) Generate bindings

From repo root:

```
bash generate_ffi_bindings.sh
```

3) Build Rust libs (multi-target)

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

4) Build Flutter apps (multi-target)

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
