# Security Practices

This document summarizes current security practices for the Pirate Unified
Wallet project.

## Build integrity

- Reproducible build effort using a Nix flake (`flake.nix` + `flake.lock`).
- Release verification via published SHA-256 checksums (see `docs/verify-build.md`).
- SBOM generation with `scripts/generate-sbom.sh`.
- Provenance generation with `scripts/generate-provenance.sh` (optional Sigstore).

## Dependency hygiene

CI is configured to run:
- `cargo audit` for known Rust vulnerabilities.
- `cargo deny` for license and advisory checks.
- Semgrep for static analysis.
- Flutter analyze/tests for Dart/Flutter issues.

## Secrets and sensitive data

- The Flutter app uses platform secure storage (`flutter_secure_storage`).
- The Rust storage layer can optionally use the native OS keystore when the
  `native-keystore` feature is enabled.

## Reporting vulnerabilities

If you find a security issue, please contact the Pirate Chain engineering team https://piratechain.com/team
