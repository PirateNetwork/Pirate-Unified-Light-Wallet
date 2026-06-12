#!/usr/bin/env bash
# Fetch the KMDCL/KDF iOS static library before CocoaPods/Xcode link the plugin.
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
exec bash "$SCRIPT_DIR/prefetch-kdf-artifact.sh" ios
