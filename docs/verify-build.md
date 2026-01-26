# Verify Builds

This guide explains how to verify official release artifacts and reproduce
builds from source.

## Verify an official release

1) Download the release asset and checksum file.

```bash
gh release download <tag> -R PirateNetwork/Pirate-Unified-Light-Wallet
# Or download manually:
curl -L -O <release-asset-url>
curl -L -O <checksums-url>
```

2) Verify the checksum.

```bash
# Linux
sha256sum -c <checksums-file>

# macOS
shasum -a 256 -c <checksums-file>

# Windows (PowerShell)
Get-FileHash <artifact> -Algorithm SHA256
Get-Content <checksums-file>
```

Compare the hash for your artifact filename to the printed checksum.

## Unsigned vs signed artifacts

Reproducible builds correspond to the `-unsigned` artifacts. Signed releases
(code-signed, notarized, or store-signed) will not be byte-for-byte identical.
We publish checksums for both. To reproduce locally, compare against the
`-unsigned` checksums.

## Reproduce with Nix

We provide a Nix flake for pinned, reproducible builds.

```bash
git clone https://github.com/PirateNetwork/Pirate-Unified-Light-Wallet.git
cd Pirate-Unified-Light-Wallet
git checkout <tag-or-commit>
```

Build the target you want:

```bash
nix build .#android-apk
nix build .#android-bundle
nix build .#ios-ipa
nix build .#linux-appimage
nix build .#linux-deb
nix build .#macos-dmg
nix build .#windows-msix
```

The build output is available under the `result/` symlink.
Look for `*-unsigned.*` artifacts when comparing reproducible builds.

## Compare build outputs

Hash the artifact you built and compare it to the published checksum.

```bash
# Linux
sha256sum result/<artifact>

# macOS
shasum -a 256 result/<artifact>

# Windows (PowerShell)
Get-FileHash result/<artifact> -Algorithm SHA256
```

## Generate SBOMs (optional)

```bash
scripts/generate-sbom.sh dist/sbom
```

Outputs include:
- `dist/sbom/rust-sbom.json`
- `dist/sbom/flutter-sbom.spdx.json`
- `dist/sbom/flutter-sbom.cdx.json`
- `dist/sbom/SBOM-SUMMARY.md`

## Generate provenance (optional)

```bash
scripts/generate-provenance.sh <artifact> dist/provenance
```

This produces a provenance JSON file, checksum, and optional Sigstore bundles
if `cosign` is installed.

## Notes

- The in-app verification page will compare your local hash to the published checksums automatically.
