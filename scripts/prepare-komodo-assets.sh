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
    local bat_path=""

    tool_path="$(command -v "$name" 2>/dev/null || true)"
    if [ -n "$tool_path" ]; then
        case "$(uname -s 2>/dev/null || true)" in
            MINGW*|MSYS*|CYGWIN*)
                if [ -f "${tool_path}.bat" ]; then
                    echo "${tool_path}.bat"
                    return 0
                fi
                bat_path="$(command -v "${name}.bat" 2>/dev/null || true)"
                if [ -n "$bat_path" ]; then
                    echo "$bat_path"
                    return 0
                fi
                ;;
        esac
        echo "$tool_path"
        return 0
    fi

    bat_path="$(command -v "${name}.bat" 2>/dev/null || true)"
    if [ -n "$bat_path" ]; then
        echo "$bat_path"
        return 0
    fi

    return 1
}

FLUTTER_CMD="$(resolve_tool flutter)" || error "flutter is required; run this after Flutter is installed."
MAX_ATTEMPTS="${KOMODO_ASSET_PREPARE_ATTEMPTS:-5}"
RETRY_BASE_SECONDS="${KOMODO_ASSET_RETRY_BASE_SECONDS:-10}"
CDN_FALLBACK_DISABLED=0
CDN_FALLBACK_DISABLE_ALLOWED="${KOMODO_ASSET_DISABLE_CDN_FALLBACK:-0}"

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

disable_coin_cdn_mirror() {
    local package_config="$APP_DIR/.dart_tool/package_config.json"
    local root_uri
    root_uri="$(awk '
        /"name"[[:space:]]*:[[:space:]]*"komodo_defi_framework"/ { found = 1; next }
        found && /"rootUri"[[:space:]]*:/ {
            line = $0
            sub(/^[^:]*:[[:space:]]*"/, "", line)
            sub(/",[[:space:]]*$/, "", line)
            print line
            exit
        }
    ' "$package_config")"
    if [ -z "$root_uri" ]; then
        log "Unable to locate komodo_defi_framework in package_config.json for CDN fallback."
        return 1
    fi

    local package_root="$root_uri"
    package_root="${package_root#file://}"
    package_root="${package_root//%20/ }"
    case "$(uname -s 2>/dev/null || true)" in
        MINGW*|MSYS*|CYGWIN*)
            if [[ "$package_root" =~ ^/[A-Za-z]:/ ]]; then
                package_root="${package_root#/}"
            fi
            if command -v cygpath >/dev/null 2>&1; then
                package_root="$(cygpath -u "$package_root")"
            fi
            ;;
    esac

    local build_config="$package_root/app_build/build_config.json"
    if [ ! -f "$build_config" ]; then
        log "Komodo build_config.json not found at $build_config; cannot disable CDN mirror."
        return 1
    fi

    if ! grep -q '"cdn_branch_mirrors"[[:space:]]*:' "$build_config"; then
        log "Komodo coin CDN mirrors already absent in $build_config."
        return 0
    fi

    local tmp_config="$tmp_dir/build_config.no-cdn.json"
    awk '
        /"cdn_branch_mirrors"[[:space:]]*:/ {
            indent = $0
            sub(/"cdn_branch_mirrors".*/, "", indent)
            replacement = indent "\"cdn_branch_mirrors\": {}"
            in_cdn = 1
            next
        }
        in_cdn && /^[[:space:]]*}[,]?[[:space:]]*$/ {
            if ($0 ~ /,/) {
                print replacement ","
            } else {
                print replacement
            }
            in_cdn = 0
            next
        }
        !in_cdn { print }
    ' "$build_config" > "$tmp_config"
    mv "$tmp_config" "$build_config"
    log "Disabled Komodo coin CDN mirrors in $build_config."
}

is_transient_transformer_failure() {
    local log_file="$1"
    grep -Eiq \
        "ClientException|SocketException|HttpException|Connection (closed|reset|refused)|Failed host lookup|timed out|TLS|status (408|429|5[0-9][0-9])|Service Unavailable|Too Many Requests|rate limit" \
        "$log_file"
}

run_transformer_pass() {
    local attempt="$1"
    local log_file="$tmp_dir/pass-${attempt}.log"
    LAST_TRANSFORMER_LOG="$log_file"

    rm -f "$output_file"
    log "Running coin asset transformer preflight pass $attempt..."

    local status
    if "$FLUTTER_CMD" pub run komodo_wallet_build_transformer \
        --fetch_coin_assets \
        --artifact_output_package=komodo_defi_framework \
        --config_output_path=app_build/build_config.json \
        --input "$input_file" \
        --output "$output_file" \
        --log_level=info >"$log_file" 2>&1; then
        status=0
    else
        status=$?
    fi

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

attempt=1
while [ "$attempt" -le "$MAX_ATTEMPTS" ]; do
    if run_transformer_pass "$attempt"; then
        log "Coin assets are stable."
        exit 0
    else
        status=$?
    fi

    if [ "$status" -eq 2 ]; then
        if [ "$attempt" -lt "$MAX_ATTEMPTS" ]; then
            log "Coin assets changed during preflight; verifying with another pass."
            attempt=$((attempt + 1))
            continue
        fi
        error "Coin assets were still changing after $attempt preflight passes."
    fi

    if is_transient_transformer_failure "$LAST_TRANSFORMER_LOG" && [ "$attempt" -lt "$MAX_ATTEMPTS" ]; then
        if [ "$CDN_FALLBACK_DISABLE_ALLOWED" = "1" ] &&
            [ "$CDN_FALLBACK_DISABLED" -eq 0 ] &&
            grep -q "kmdclassic.github.io" "$LAST_TRANSFORMER_LOG"; then
            log "Coin asset CDN returned a transient error; disabling CDN mirror for fallback."
            disable_coin_cdn_mirror || true
            CDN_FALLBACK_DISABLED=1
        fi
        delay=$((attempt * RETRY_BASE_SECONDS))
        log "Coin asset transformer failed with a transient download error; retrying in ${delay}s."
        sleep "$delay"
        attempt=$((attempt + 1))
        continue
    fi

    error "Coin asset transformer preflight failed with exit code $status."
done
