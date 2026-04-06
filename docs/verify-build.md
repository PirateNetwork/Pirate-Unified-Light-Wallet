# Verify Builds

This document describes how to verify published Pirate Unified Wallet artifacts and how to reproduce repository outputs locally.

Release artifacts
-----------------

The project build scripts currently generate these artifact names:

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
  - `pirate-unified-wallet-android-V8.apk`
  - `pirate-unified-wallet-android-V8-unsigned.apk`
  - `pirate-unified-wallet-android-V7.apk`
  - `pirate-unified-wallet-android-V7-unsigned.apk`
  - `pirate-unified-wallet-android-x86.apk`
  - `pirate-unified-wallet-android-x86-unsigned.apk`
  - `pirate-unified-wallet-android.aab`
  - `pirate-unified-wallet-android-unsigned.aab`
- iOS
  - `pirate-unified-wallet-ios.ipa`
  - `pirate-unified-wallet-ios-unsigned.ipa`
- Backend
  - `piratewallet-cli`
  - `piratewallet-cli.exe`
  - `pirate-qortal-cli`
  - `pirate-qortal-cli.exe`
  - `libpirate_ffi_native.a`
  - `libpirate_ffi_native.so`
  - `pirate_ffi_native.dll`
  - `pirate_wallet_service.h`

Each release artifact should have a matching `.sha256` file or be covered by a published checksum bundle.

Verify an official release
--------------------------

1. Download the release assets.

```bash
gh release download <tag> -R PirateNetwork/Pirate-Unified-Light-Wallet
```

2. Locate the checksum source.

The repository currently supports either:

- per-artifact checksum files such as `pirate-unified-wallet-windows-installer.exe.sha256`
- checksum bundles whose filenames include `checksum` or `checksums`

3. Compare the local file hash to the published hash.

Linux:

```bash
expected="$(awk '{print $1}' pirate-unified-wallet-windows-installer.exe.sha256)"
actual="$(sha256sum pirate-unified-wallet-windows-installer.exe | awk '{print $1}')"
test "$expected" = "$actual" && echo MATCH || echo MISMATCH
```

macOS:

```bash
expected="$(awk '{print $1}' pirate-unified-wallet-macos-unsigned.dmg.sha256)"
actual="$(shasum -a 256 pirate-unified-wallet-macos-unsigned.dmg | awk '{print $1}')"
test "$expected" = "$actual" && echo MATCH || echo MISMATCH
```

Windows PowerShell:

```powershell
$expected = (Get-Content .\pirate-unified-wallet-windows-installer.exe.sha256 | Select-Object -First 1).Split()[0].ToLower()
$actual = (Get-FileHash .\pirate-unified-wallet-windows-installer.exe -Algorithm SHA256).Hash.ToLower()
if ($expected -eq $actual) { 'MATCH' } else { 'MISMATCH' }
```

Signed and unsigned outputs
---------------------------

Unsigned artifacts are the best fit for deterministic comparison because signing, notarization, and store packaging change the final bytes.

Use the unsigned variants when you want a close comparison with a locally reproduced build:

- `*-unsigned.exe`
- `*-unsigned.zip`
- `*-unsigned.dmg`
- `*-unsigned.apk`
- `*-unsigned.aab`
- `*-unsigned.ipa`

Reproduce repository outputs
----------------------------

Local platform scripts are the authoritative way to generate the packaged outputs listed above.

Examples:

```bash
bash scripts/build-windows.sh
bash scripts/build-linux.sh appimage
bash scripts/build-linux.sh flatpak
bash scripts/build-linux.sh deb
bash scripts/build-macos.sh
bash scripts/build-android.sh apk
bash scripts/build-android.sh bundle
bash scripts/build-ios.sh false
```

Nix flake builds
----------------

The checked-in flake exposes native build targets that follow the committed release scripts:

Linux hosts:

```bash
nix build .#linux-appimage
nix build .#linux-flatpak
nix build .#linux-deb
nix build .#android-apk
nix build .#android-bundle
```

macOS hosts:

```bash
nix build .#macos-dmg
nix build .#ios-ipa
```

Notes:

- The flake is host-native. It does not expose Windows packaging targets.
- The flake packages collect the outputs produced by the committed platform scripts.
- Use the platform scripts directly if you need a platform that is not exposed by the flake on your current host.

Compare local outputs
---------------------

After building locally, hash the artifact and compare it to the published checksum.

```bash
sha256sum dist/windows/pirate-unified-wallet-windows-portable-unsigned.zip
sha256sum dist/linux/pirate-unified-wallet-linux-x86_64.AppImage
shasum -a 256 dist/macos/pirate-unified-wallet-macos-unsigned.dmg
sha256sum dist/android/pirate-unified-wallet-android-V8-unsigned.apk
shasum -a 256 dist/ios/pirate-unified-wallet-ios-unsigned.ipa
```

SBOM and provenance
-------------------

To generate release metadata locally:

```bash
scripts/generate-sbom.sh dist/sbom
scripts/generate-provenance.sh <artifact> dist/provenance
```

The provenance script writes:

- `{artifact}.provenance.json`
- `{artifact}.provenance.json.sha256`
- optional Sigstore bundles if `cosign` is installed
- `{artifact}.VERIFY.md`

Verify Build screen
-------------------

The application includes a Verify Build screen that:

- fetches GitHub release metadata
- locates published checksums
- hashes local artifacts
- reports whether the local artifact matches a published checksum

That screen depends on outbound GitHub access being enabled in application settings.
