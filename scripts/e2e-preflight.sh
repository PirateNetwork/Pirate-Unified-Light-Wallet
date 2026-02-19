#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "${SCRIPT_DIR}/.." && pwd)"

if [[ "${E2E_FORCE_SKIP:-0}" == "1" ]]; then
  echo "[e2e-preflight] SKIP: E2E_FORCE_SKIP=1"
  exit 1
fi

if [[ "${E2E_FORCE_RUN:-0}" == "1" ]]; then
  echo "[e2e-preflight] OK: forced run (E2E_FORCE_RUN=1)"
  exit 0
fi

endpoint="${LIGHTWALLETD_URL:-${LIGHTWALLETD_ENDPOINT:-${E2E_LIGHTWALLETD_URL:-}}}"
missing=()

if [[ -z "${endpoint}" ]]; then
  missing+=("LIGHTWALLETD_URL (or LIGHTWALLETD_ENDPOINT / E2E_LIGHTWALLETD_URL)")
fi

platform="unknown"
case "$(uname -s)" in
  MINGW*|MSYS*|CYGWIN*)
    platform="windows"
    ;;
  Darwin)
    platform="macos"
    ;;
  Linux)
    platform="linux"
    ;;
esac

has_library=1
case "${platform}" in
  windows)
    if [[ -f "${REPO_ROOT}/app/build/windows/x64/runner/Debug/pirate_ffi_frb.dll" ]] || \
       [[ -f "${REPO_ROOT}/app/build/windows/x64/runner/Release/pirate_ffi_frb.dll" ]] || \
       [[ -f "${REPO_ROOT}/app/windows/pirate_ffi_frb.dll" ]]; then
      has_library=0
    fi
    ;;
  macos)
    if [[ -f "${REPO_ROOT}/app/build/macos/Build/Products/Debug/libpirate_ffi_frb.dylib" ]] || \
       [[ -f "${REPO_ROOT}/app/build/macos/Build/Products/Release/libpirate_ffi_frb.dylib" ]]; then
      has_library=0
    fi
    ;;
  linux)
    if [[ -f "${REPO_ROOT}/app/build/linux/x64/debug/bundle/lib/libpirate_ffi_frb.so" ]] || \
       [[ -f "${REPO_ROOT}/app/build/linux/x64/release/bundle/lib/libpirate_ffi_frb.so" ]]; then
      has_library=0
    fi
    ;;
esac

if [[ "${has_library}" -ne 0 ]]; then
  missing+=("Rust FFI library for ${platform} desktop runtime")
fi

if (( ${#missing[@]} > 0 )); then
  echo "[e2e-preflight] SKIP: missing prerequisites"
  for item in "${missing[@]}"; do
    echo "[e2e-preflight]   - ${item}"
  done
  echo "[e2e-preflight] set E2E_FORCE_RUN=1 to bypass this gate"
  exit 1
fi

echo "[e2e-preflight] OK: prerequisites detected"
echo "[e2e-preflight] endpoint=${endpoint}"
exit 0
