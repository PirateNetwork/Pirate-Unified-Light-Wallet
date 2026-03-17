# Contributing

This repository contains a Flutter application, a Rust core, and platform build scripts. Changes should match what is committed in the repository and should not rely on local-only directories or developer-specific tooling paths.

General rules
-------------

- Keep documentation aligned with committed files and committed scripts.
- Do not document local-only directories that are not checked in.
- Do not edit generated files by hand unless the generation process requires a small header-only fix and that exception is documented.
- Keep platform-specific changes limited to the platforms they affect.
- Prefer small, reviewable changes over wide mechanical rewrites.

Generated outputs
-----------------

The following areas are generated and should normally be refreshed through tooling:

- `app/lib/core/ffi/generated/`
- `app/lib/l10n/app_localizations*.dart`
- Flutter plugin registrants under platform runner directories

If a change requires regenerated output, include the source change and the generated result in the same reviewable patch.

Local development checks
------------------------

Rust:

```bash
cd crates
cargo fmt --all
cargo clippy --all-targets --all-features --locked -- -D warnings
cargo test --all-features --locked
```

Flutter:

```bash
cd app
flutter pub get --enforce-lockfile
flutter analyze
```

Documentation style
-------------------

- Use direct technical language.
- Do not add marketing copy, placeholders, or internal planning notes to user-facing documentation.
- Keep documentation tied to committed code and scripts.
- Use plain Markdown and ASCII text unless a file already uses something else intentionally.

Build and packaging
-------------------

Use the committed scripts under `scripts/` for packaging work. Do not replace those instructions in documentation with local helper scripts that are not checked in.

Security and release changes
----------------------------

If a change affects:

- artifact verification
- updater behavior
- network transport
- signing
- wallet storage

then update the relevant documentation under `docs/` in the same change.
