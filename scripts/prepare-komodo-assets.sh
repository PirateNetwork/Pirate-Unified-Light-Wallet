#!/usr/bin/env bash
# Materialize pinned coin assets and make the SDK's Flutter transformer offline.
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
PACKAGE_CONFIG="$PROJECT_ROOT/app/.dart_tool/package_config.json"

error() {
    echo "[prepare-komodo-assets][ERROR] $1" >&2
    exit 1
}

resolve_python() {
    if [ -n "${PYTHON:-}" ]; then
        local resolved
        resolved="$(command -v "$PYTHON" 2>/dev/null || true)"
        if [ -n "$resolved" ]; then
            echo "$resolved"
            return 0
        fi
        if [ -f "$PYTHON" ]; then
            echo "$PYTHON"
            return 0
        fi
        return 1
    fi
    if command -v python3 >/dev/null 2>&1; then
        command -v python3
        return 0
    fi
    if command -v python >/dev/null 2>&1; then
        command -v python
        return 0
    fi
    if command -v py >/dev/null 2>&1; then
        command -v py
        return 0
    fi
    return 1
}

[ -f "$PACKAGE_CONFIG" ] || error \
    "$PACKAGE_CONFIG not found; run flutter pub get --enforce-lockfile first."

PYTHON_BIN="$(resolve_python)" || error "Python 3 is required."

"$PYTHON_BIN" "$SCRIPT_DIR/prefetch-komodo-assets.py" \
    --package-config "$PACKAGE_CONFIG" \
    --asset-lock "$SCRIPT_DIR/komodo-coin-assets.lock.json"

"$PYTHON_BIN" "$SCRIPT_DIR/configure-komodo-assets.py" \
    --package-config "$PACKAGE_CONFIG"

echo "[prepare-komodo-assets] Pinned coin assets are ready; transformer network fetches are disabled."
