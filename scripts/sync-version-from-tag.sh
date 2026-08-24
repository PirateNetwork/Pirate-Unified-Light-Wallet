#!/usr/bin/env bash
# Sync app version metadata from a git tag like v1.2.3.
#
# Updates:
# - app/pubspec.yaml `version: X.Y.Z+N`
#
# Rules:
# - If no tag ref is available (non-tag builds), this script is a no-op.
# - Build number defaults to a monotonic MMmmpp encoding unless
#   VERSION_BUILD_NUMBER is set (major * 10000 + minor * 100 + patch).

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
PUBSPEC_PATH="${VERSION_PUBSPEC_PATH:-$PROJECT_ROOT/app/pubspec.yaml}"

log() {
  echo "[version-sync] $*"
}

resolve_tag() {
  local input="${1:-}"
  if [[ -n "$input" ]]; then
    echo "${input#refs/tags/}"
    return 0
  fi
  if [[ "${GITHUB_REF_TYPE:-}" == "tag" && -n "${GITHUB_REF_NAME:-}" ]]; then
    echo "$GITHUB_REF_NAME"
    return 0
  fi
  if [[ "${GITHUB_REF:-}" == refs/tags/* ]]; then
    echo "${GITHUB_REF#refs/tags/}"
    return 0
  fi
  echo ""
}

TAG_NAME="$(resolve_tag "${1:-}")"
if [[ -z "$TAG_NAME" ]]; then
  log "No git tag ref detected. Keeping existing version metadata."
  exit 0
fi

if [[ ! "$TAG_NAME" =~ ^v([0-9]+)\.([0-9]+)\.([0-9]+)([-+].*)?$ ]]; then
  echo "[version-sync] Tag '$TAG_NAME' must match vX.Y.Z or vX.Y.Z-suffix" >&2
  exit 1
fi

MAJOR="${BASH_REMATCH[1]}"
MINOR="${BASH_REMATCH[2]}"
PATCH="${BASH_REMATCH[3]}"
SEMVER="${MAJOR}.${MINOR}.${PATCH}"
if [[ "$MINOR" -ge 100 || "$PATCH" -ge 100 ]]; then
  echo "[version-sync] Tag '$TAG_NAME' exceeds the two-digit minor/patch build-number encoding" >&2
  exit 1
fi
DEFAULT_BUILD_NUMBER=$((10#$MAJOR * 10000 + 10#$MINOR * 100 + 10#$PATCH))
BUILD_NUMBER="${VERSION_BUILD_NUMBER:-$DEFAULT_BUILD_NUMBER}"
if [[ ! "$BUILD_NUMBER" =~ ^[1-9][0-9]*$ || "$BUILD_NUMBER" -gt 65535 ]]; then
  echo "[version-sync] Build number must be an integer from 1 through 65535: '$BUILD_NUMBER'" >&2
  exit 1
fi
PUBSPEC_VERSION="${SEMVER}+${BUILD_NUMBER}"
if [[ ! -f "$PUBSPEC_PATH" ]]; then
  echo "[version-sync] pubspec not found: $PUBSPEC_PATH" >&2
  exit 1
fi

tmp_file="$(mktemp)"
awk -v app_version="$PUBSPEC_VERSION" '
  BEGIN {
    version_done = 0
  }
  {
    if (!version_done && $0 ~ /^version:[[:space:]]*/) {
      print "version: " app_version
      version_done = 1
      next
    }
    print
  }
  END {
    if (!version_done) {
      exit 2
    }
  }
' "$PUBSPEC_PATH" > "$tmp_file"

mv "$tmp_file" "$PUBSPEC_PATH"

if ! grep -qxF "version: ${PUBSPEC_VERSION}" "$PUBSPEC_PATH"; then
  echo "[version-sync] Failed to verify synced pubspec version ${PUBSPEC_VERSION}" >&2
  exit 1
fi

log "Synced pubspec version to ${PUBSPEC_VERSION} from tag ${TAG_NAME}"
