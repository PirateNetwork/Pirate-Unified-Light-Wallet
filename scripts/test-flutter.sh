#!/usr/bin/env bash
# Flutter 3.47 widget tests need the application asset bundle for fonts, images,
# and framework shaders. Materialize the pinned Komodo assets before Flutter
# invokes its dependency transformer so the test build remains deterministic.
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"

resolve_flutter() {
    local requested="${FLUTTER:-}"
    local resolved=""
    if [ -n "$requested" ]; then
        resolved="$(command -v "$requested" 2>/dev/null || true)"
        if [ -n "$resolved" ]; then
            echo "$resolved"
            return 0
        fi
        if [ -f "$requested" ]; then
            echo "$requested"
            return 0
        fi
    fi

    resolved="$(command -v flutter 2>/dev/null || true)"
    if [ -n "$resolved" ]; then
        case "$(uname -s 2>/dev/null || true)" in
            MINGW*|MSYS*|CYGWIN*)
                if [ -f "${resolved}.bat" ]; then
                    echo "${resolved}.bat"
                    return 0
                fi
                ;;
        esac
        echo "$resolved"
        return 0
    fi

    resolved="$(command -v flutter.bat 2>/dev/null || true)"
    if [ -n "$resolved" ]; then
        echo "$resolved"
        return 0
    fi
    return 1
}

FLUTTER_BIN="$(resolve_flutter)" || {
    echo "Flutter was not found. Set FLUTTER to the Flutter executable." >&2
    exit 127
}

export OVERRIDE_DEFI_API_DOWNLOAD=false
bash "$PROJECT_ROOT/scripts/prepare-komodo-assets.sh"
cd "$PROJECT_ROOT/app"
exec "$FLUTTER_BIN" test "$@"
