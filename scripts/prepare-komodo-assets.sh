#!/usr/bin/env bash
# Stabilize Komodo coin assets before Flutter's asset transformer runs.
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
APP_DIR="$PROJECT_ROOT/app"

log() {
    echo "[prepare-komodo-assets] $1"
}

error() {
    echo "[prepare-komodo-assets][ERROR] $1" >&2
    exit 1
}

resolve_tool() {
    local name="$1"
    local tool_path=""

    if tool_path="$(command -v "$name" 2>/dev/null)"; then
        case "$(uname -s 2>/dev/null || true)" in
            MINGW*|MSYS*|CYGWIN*)
                if [ -f "${tool_path}.bat" ]; then
                    echo "${tool_path}.bat"
                    return 0
                fi
                if tool_path="$(command -v "${name}.bat" 2>/dev/null)"; then
                    echo "$tool_path"
                    return 0
                fi
                ;;
        esac
        echo "$tool_path"
        return 0
    fi

    if tool_path="$(command -v "${name}.bat" 2>/dev/null)"; then
        echo "$tool_path"
        return 0
    fi

    return 1
}

FLUTTER_CMD="$(resolve_tool flutter)" || error "flutter is required; run this after Flutter is installed."

cd "$APP_DIR"

if [ ! -f ".dart_tool/package_config.json" ]; then
    error ".dart_tool/package_config.json not found; run flutter pub get first."
fi

if [ -z "${GITHUB_API_PUBLIC_READONLY_TOKEN:-}" ] && [ -n "${GITHUB_TOKEN:-}" ]; then
    export GITHUB_API_PUBLIC_READONLY_TOKEN="$GITHUB_TOKEN"
fi

tmp_root="${RUNNER_TEMP:-${TMPDIR:-${TEMP:-/tmp}}}"
if command -v cygpath >/dev/null 2>&1; then
    tmp_root="$(cygpath -u "$tmp_root")"
fi
mkdir -p "$tmp_root"
tmp_dir="$(mktemp -d "$tmp_root/komodo-assets.XXXXXX")"
trap 'rm -rf "$tmp_dir"' EXIT

input_file="$tmp_dir/transformer_input.txt"
output_file="$tmp_dir/transformer_output.txt"
printf 'prepare-komodo-assets\n' > "$input_file"

run_transformer_pass() {
    local attempt="$1"
    local log_file="$tmp_dir/pass-${attempt}.log"

    rm -f "$output_file"
    log "Running coin asset transformer preflight pass $attempt..."

    set +e
    "$FLUTTER_CMD" pub run komodo_wallet_build_transformer \
        --fetch_coin_assets \
        --artifact_output_package=komodo_defi_framework \
        --config_output_path=app_build/build_config.json \
        --input "$input_file" \
        --output "$output_file" \
        --log_level=info >"$log_file" 2>&1
    local status=$?
    set -e

    cat "$log_file"

    if [ "$status" -eq 0 ]; then
        [ -f "$output_file" ] || error "Transformer pass $attempt succeeded but did not write $output_file"
        return 0
    fi

    if grep -q "Coin assets were updated" "$log_file"; then
        return 2
    fi

    return "$status"
}

for attempt in 1 2 3; do
    set +e
    run_transformer_pass "$attempt"
    status=$?
    set -e

    if [ "$status" -eq 0 ]; then
        log "Coin assets are stable."
        exit 0
    fi

    if [ "$status" -eq 2 ]; then
        if [ "$attempt" -lt 3 ]; then
            log "Coin assets changed during preflight; verifying with another pass."
            continue
        fi
        error "Coin assets were still changing after $attempt preflight passes."
    fi

    error "Coin asset transformer preflight failed with exit code $status."
done
