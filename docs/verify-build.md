# Verify Builds

This document describes how to verify published Stashi Wallet artifacts and how to reproduce repository outputs locally.

Release artifacts
-----------------

The project build scripts currently generate these artifact names:

- Windows release installers
  - `Stashi-Wallet-windows-installer.exe`
  - `Stashi-Wallet-windows-installer-unsigned.exe`
- Windows CI/test build
  - `Stashi-Wallet-windows-portable-unsigned.zip`
- Linux
  - `Stashi-Wallet-linux-x86_64.AppImage`
  - `Stashi-Wallet.flatpak`
  - `Stashi-Wallet-amd64.deb`
- macOS
  - `Stashi-Wallet-macos.dmg`
  - `Stashi-Wallet-macos-unsigned.dmg`
- Android
  - `Stashi-Wallet-android-V8.apk`
  - `Stashi-Wallet-android-V8-unsigned.apk`
  - `Stashi-Wallet-android-V7.apk`
  - `Stashi-Wallet-android-V7-unsigned.apk`
  - `Stashi-Wallet-android-x86.apk`
  - `Stashi-Wallet-android-x86-unsigned.apk`
  - `Stashi-Wallet-android.aab`
  - `Stashi-Wallet-android-unsigned.aab`
- iOS
  - `Stashi-Wallet-ios.ipa`
  - `Stashi-Wallet-ios-unsigned.ipa`
- Backend
  - `piratewallet-cli`
  - `piratewallet-cli.exe`
  - `pirate-qortal-cli`
  - `pirate-qortal-cli.exe`
  - Qortal JNI libraries named `librust-<platform>-<architecture>`
  - `libpirate_ffi_native.a`
  - `libpirate_ffi_native.so`
  - `pirate_ffi_native.dll`
  - `pirate_wallet_service.h`

Official GitHub Releases keep user installables at the top level. Developer artifacts, store/test builds, SBOMs, provenance, signatures, and checksums are grouped into release bundles so the download list stays readable.

The Windows installer is the normal-user release artifact. The unsigned
portable build is retained inside
`Stashi-Wallet-unsigned-desktop-test-builds.zip` for testing and
reproducible comparison.

Each release also publishes `signatures-<tag>.zip`. Its filenames and command
flow intentionally match Treasure Chest's verification bundle, while every
signature is made with Stashi Wallet's own release key. Never use a
Treasure Chest maintainer key to authenticate a Stashi Wallet release.

The established key UID is
`Pirate Unified Wallet <dev@piratechainfoundation.com>`. The authoritative
primary fingerprint is:

```text
E4FB 2399 AECC F9B9 447D ED47 2CE6 5343 4015 53A6
```

Verify an official release
--------------------------

1. Download the release asset you want and the signature bundle for the same
   tag.

```bash
gh release download <tag> -R PirateNetwork/Pirate-Unified-Light-Wallet \
  -p Stashi-Wallet-windows-installer.exe \
  -p 'signatures-*.zip'
```

2. Extract the signature bundle, import its key, and independently confirm the
   complete fingerprint above. Importing a key does not establish that it is
   the correct key.

```bash
unzip signatures-<tag>.zip -d signatures-<tag>
gpg --import signatures-<tag>/public_key.asc
gpg --fingerprint E4FB2399AECCF9B9447DED472CE65343401553A6
```

3. Verify the signature on the checksum manifest, then verify the downloaded
   files against it.

```bash
gpg --verify \
  signatures-<tag>/sha256sum-<tag>.txt.sig \
  signatures-<tag>/sha256sum-<tag>.txt

(cd . && sha256sum -c signatures-<tag>/sha256sum-<tag>.txt)
```

Run the checksum command from the directory containing all files listed in the
manifest, or verify one file by comparing its locally calculated SHA-256 with
that file's manifest entry.

4. Optionally verify the downloaded asset directly. Every top-level release
   asset has a binary detached `.sig` file in the signature bundle.

```bash
gpg --verify \
  signatures-<tag>/Stashi-Wallet-linux-x86_64.AppImage.sig \
  Stashi-Wallet-linux-x86_64.AppImage
```

PGP verification does not decrypt a package. It confirms that the exact bytes
were signed by the holder of the matching private key. A public key obtained
from the same release page is only an identity proof after its complete
fingerprint has been confirmed through an independent official Pirate Network
channel.

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
sha256sum dist/windows/Stashi-Wallet-windows-portable-unsigned.zip
sha256sum dist/linux/Stashi-Wallet-linux-x86_64.AppImage
shasum -a 256 dist/macos/Stashi-Wallet-macos-unsigned.dmg
sha256sum dist/android/Stashi-Wallet-android-V8-unsigned.apk
shasum -a 256 dist/ios/Stashi-Wallet-ios-unsigned.ipa
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

- downloads the deterministic `signatures-<tag>.zip` asset for its exact build
- verifies the signed manifest with the embedded Stashi Wallet public key and
  pinned primary key identity
- hashes the distributed desktop artifact when available, otherwise the
  installed desktop executable recorded during packaging
- reports a match only when the PGP signature and SHA-256 comparison both pass

The screen does not treat a checksum by itself as proof of publisher identity,
and it does not silently fall back to a different release tag.

That screen depends on outbound GitHub access being enabled in application settings.
