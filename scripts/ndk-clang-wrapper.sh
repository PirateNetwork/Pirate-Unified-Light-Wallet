#!/usr/bin/env bash
set -euo pipefail

NDK_HOME="${ANDROID_NDK_HOME:-${ANDROID_NDK_ROOT:-}}"
if [[ -z "$NDK_HOME" ]]; then
  echo "ANDROID_NDK_HOME not set." >&2
  exit 1
fi

HOST_TAG="${ANDROID_NDK_HOST_TAG:-linux-x86_64}"
NDK_BASE="${NDK_HOME}/toolchains/llvm/prebuilt/${HOST_TAG}"
CLANG_VERSION="${ANDROID_NDK_CLANG_VERSION:-}"

if [[ -n "$CLANG_VERSION" ]]; then
  CLANG_BIN="${NDK_BASE}/bin/clang-${CLANG_VERSION}"
  RESOURCE_DIR="${NDK_BASE}/lib/clang/${CLANG_VERSION}"
else
  CLANG_BIN="${NDK_BASE}/bin/clang"
  RESOURCE_ROOT="${NDK_BASE}/lib/clang"
  if [[ -d "$RESOURCE_ROOT" ]]; then
    RESOURCE_DIR="$(ls -d "$RESOURCE_ROOT"/* 2>/dev/null | sort -V | tail -n 1)"
  else
    RESOURCE_DIR=""
  fi
fi

if [[ ! -x "$CLANG_BIN" ]]; then
  echo "clang not found at $CLANG_BIN" >&2
  exit 1
fi

if [[ -z "$RESOURCE_DIR" || ! -d "$RESOURCE_DIR" ]]; then
  echo "clang resource dir not found under ${NDK_BASE}/lib/clang" >&2
  exit 1
fi

LD_LINUX="${ANDROID_NDK_LD_LINUX:-/lib64/ld-linux-x86-64.so.2}"
exec "$LD_LINUX" "$CLANG_BIN" -resource-dir "$RESOURCE_DIR" "$@"
