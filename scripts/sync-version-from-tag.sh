#!/usr/bin/env bash
# Sync app version metadata from a git tag like v1.2.3.
#
# Updates:
# - app/pubspec.yaml `version: X.Y.Z+N`
# - app/pubspec.yaml `msix_version: X.Y.Z.0`
#
# Rules:
# - If no tag ref is available (non-tag builds), this script is a no-op.
# - Build number defaults to patch (Z) unless VERSION_BUILD_NUMBER is set.

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
PUBSPEC_PATH="$PROJECT_ROOT/app/pubspec.yaml"

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
  log "Tag '$TAG_NAME' does not match vX.Y.Z or vX.Y.Z-suffix. Keeping existing version metadata."
  exit 0
fi

MAJOR="${BASH_REMATCH[1]}"
MINOR="${BASH_REMATCH[2]}"
PATCH="${BASH_REMATCH[3]}"
SEMVER="${MAJOR}.${MINOR}.${PATCH}"
BUILD_NUMBER="${VERSION_BUILD_NUMBER:-$PATCH}"
PUBSPEC_VERSION="${SEMVER}+${BUILD_NUMBER}"
MSIX_VERSION="${SEMVER}.0"

if [[ ! -f "$PUBSPEC_PATH" ]]; then
  echo "[version-sync] pubspec not found: $PUBSPEC_PATH" >&2
  exit 1
fi

tmp_file="$(mktemp)"
awk -v app_version="$PUBSPEC_VERSION" -v msix_version="$MSIX_VERSION" '
  BEGIN {
    version_done = 0
    msix_done = 0
  }
  {
    if (!version_done && $0 ~ /^version:[[:space:]]*/) {
      print "version: " app_version
      version_done = 1
      next
    }
    if ($0 ~ /^[[:space:]]*msix_version:[[:space:]]*/) {
      sub(/msix_version:[[:space:]]*.*/, "msix_version: " msix_version)
      msix_done = 1
      print
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

log "Synced pubspec version to ${PUBSPEC_VERSION} from tag ${TAG_NAME}"
log "Synced msix_version to ${MSIX_VERSION}"
