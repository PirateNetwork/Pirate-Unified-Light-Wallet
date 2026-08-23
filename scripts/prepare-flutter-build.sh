#!/usr/bin/env bash
# Prepare all dependency-owned assets before a supported Flutter build.
set -euo pipefail

if [ "$#" -ne 1 ]; then
    echo "Usage: $0 <android|ios|linux|macos|windows>" >&2
    exit 64
fi

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
PLATFORM="$1"

case "$PLATFORM" in
    android|ios|linux|macos|windows)
        ;;
    *)
        echo "Unsupported Flutter build platform: $PLATFORM" >&2
        exit 64
        ;;
esac

if [ ! -f "$PROJECT_ROOT/app/.dart_tool/package_config.json" ]; then
    echo "Run flutter pub get --enforce-lockfile before the asset preflight." >&2
    exit 1
fi

echo "[prepare-flutter-build] Prefetching checksummed KDF assets for $PLATFORM..."
bash "$SCRIPT_DIR/prefetch-kdf-artifact.sh" "$PLATFORM"

echo "[prepare-flutter-build] Materializing pinned Komodo assets and disabling transformer fetches..."
bash "$SCRIPT_DIR/prepare-komodo-assets.sh"

echo "[prepare-flutter-build] Hermetic Flutter asset preflight complete for $PLATFORM."
