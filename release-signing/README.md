# Pirate Unified Wallet release verification

This bundle contains checksums and supporting security metadata for the files
published with a Pirate Unified Wallet release.

## Official release key

- Identity: `Pirate Unified Wallet <dev@piratechainfoundation.com>`
- Fingerprint: `E4FB 2399 AECC F9B9 447D ED47 2CE6 5343 4015 53A6`
- Public key: `public-keys/pirate-unified-wallet-release-public-key.asc`

Confirm the complete fingerprint through an official Pirate Network source
before trusting the key. A name or email address printed by GPG is not, by
itself, proof that a key is official.

## Verify a checksum

`SHA256SUMS` covers every top-level release asset. Individual checksum files
are also available under `checksums/`. For example:

```bash
expected="$(awk '{print $1}' checksums/pirate-unified-wallet-linux-x86_64.AppImage.sha256)"
actual="$(sha256sum ../pirate-unified-wallet-linux-x86_64.AppImage | awk '{print $1}')"
test "$expected" = "$actual" && echo "Checksum verified" || echo "Checksum mismatch"
```

Replace the filename with the asset you downloaded. A checksum mismatch means
the file must not be used.

## Verify a Linux signature

Import the public key and confirm its fingerprint:

```bash
gpg --import public-keys/pirate-unified-wallet-release-public-key.asc
gpg --fingerprint E4FB2399AECCF9B9447DED472CE65343401553A6
```

Then verify a Linux artifact for which a matching `.asc` file is present:

```bash
gpg --verify \
  raw/linux-signatures/pirate-unified-wallet-linux-x86_64.AppImage.asc \
  ../pirate-unified-wallet-linux-x86_64.AppImage
```

The verification must report a good signature from the fingerprint shown
above. A warning that the key is not personally certified refers to GPG's
web-of-trust model; independently confirming the full fingerprint is what
connects the key to the project.

Checksums detect accidental or malicious changes to downloaded files. The PGP
signature additionally proves that the artifact was signed by the holder of
the official release key. Neither process decrypts an installer.

## Bundle layout

- `checksums/`: SHA-256 checksum files for every top-level release asset
- `SHA256SUMS`: a consolidated checksum manifest for the same assets
- `public-keys/`: public keys required to verify detached signatures
- `raw/`: detached signatures, SBOMs, provenance, verification notes, and
  optional scan reports produced by release jobs

The private release key is never included in this bundle or repository.
