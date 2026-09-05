#!/usr/bin/env bash
# Install into an isolated, cacheable prefix; never accept an unrelated PATH tool.
set -euo pipefail
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
TOOLS="$PROJECT_ROOT/.tools/rust-sbom"
# Keep these versions and the CI cache key in sync.
for spec in cargo-auditable:0.7.2 rust-audit-info:0.5.4; do
    name="${spec%:*}"
    version="${spec#*:}"
    suffix=""
    case "$(uname -s)" in MINGW*|MSYS*|CYGWIN*) suffix=".exe" ;; esac
    if [[ ! -f "$TOOLS/bin/$name$suffix" || ! -f "$TOOLS/$name-$version.installed" ]]; then
        cargo install "$name" --version "=$version" --locked --root "$TOOLS" --force
        touch "$TOOLS/$name-$version.installed"
    fi
done
