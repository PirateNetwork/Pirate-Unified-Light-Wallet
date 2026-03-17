# Security Notes

This document summarizes the security controls that are implemented in this repository today. It is not a threat model and it is not a substitute for independent review.

Build and release integrity
---------------------------

The repository includes build and release controls intended to make shipped artifacts inspectable and reproducible:

- Release scripts write SHA-256 checksum files next to generated artifacts.
- `scripts/generate-sbom.sh` generates SBOM outputs for the Rust and Flutter dependency sets.
- `scripts/generate-provenance.sh` generates provenance metadata and optional Sigstore bundles when `cosign` is available.
- Desktop Tor Browser and i2pd assets are fetched through project scripts that verify pinned hashes before bundling.
- The checked-in Nix flake exposes native build shells and native package targets for Linux and macOS hosts.
For release verification procedures, use `verify-build.md`.

Code quality and dependency checks
----------------------------------

CI and local tooling cover the following checks:

- `cargo fmt`
- `cargo clippy`
- Rust unit and property tests
- `cargo audit`
- `cargo deny`
- `flutter analyze`
- Flutter tests
- Semgrep in CI

These checks reduce risk, but they do not prove the absence of security issues.

Local storage and secrets
-------------------------

The project uses layered local storage:

- Sensitive Flutter-side preferences and cached secrets use `flutter_secure_storage`.
- Wallet databases are managed by the Rust storage layer.
- The Rust storage layer can use the native OS keystore when the `native-keystore` feature is enabled.

Local device compromise is out of scope for these protections. If the operating system account or device is already compromised, application-level storage protections are limited.

Network and external services
-----------------------------

The application can make outbound requests to third-party services for release checks, build verification, pricing, and desktop update metadata. These outbound calls are controlled by user settings in the application.

Current release and update verification behavior:

- The Verify Build screen downloads published release checksums from GitHub and compares them against local artifacts.
- The desktop updater requires published checksums before applying an update.
- On Windows installer updates, the updater also checks Authenticode status before launch.
- On macOS DMG updates, the updater verifies the mounted application bundle signature before replacement.
- On Linux, the updater currently relies on published checksum verification.

If a release is published without readable checksums, the Verify Build screen and updater will refuse to treat that release as verified.

Privacy-related controls
------------------------

Network transport defaults to Tor. The application also supports direct connections, SOCKS5 proxies, and I2P. The default configuration is intended to protect privacy, but the effective privacy level depends on the transport mode selected by the user.

Reporting vulnerabilities
-------------------------

If you find a security issue, contact the Pirate Chain engineering team:

- https://piratechain.com/team

When reporting an issue, include:

- the release version or commit
- the affected platform
- clear reproduction steps
- whether the issue affects stored keys, network privacy, update delivery, or transaction construction
