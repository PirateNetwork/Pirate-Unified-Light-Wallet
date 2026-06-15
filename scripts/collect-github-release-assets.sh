#!/usr/bin/env bash
set -euo pipefail

ARTIFACTS_DIR="${1:-release-artifacts}"
RELEASE_DIR="${2:-release}"
META_DIR="${3:-release-meta}"
DEV_DIR="${4:-release-developer-artifacts}"

rm -rf "$RELEASE_DIR" "$META_DIR" "$DEV_DIR"
mkdir -p "$RELEASE_DIR" "$META_DIR/raw" "$META_DIR/checksums" "$DEV_DIR"

is_true() {
  [[ "${1:-false}" == "true" ]]
}

current_tag() {
  printf '%s' "${GITHUB_REF_NAME:-local}"
}

versioned_zip_name() {
  local base="$1"
  local tag="${2:-$(current_tag)}"
  printf '%s-%s.zip' "$base" "$tag"
}

copy_first() {
  local pattern="$1"
  local destination="$2"
  local source
  mkdir -p "$destination"
  source="$(find "$ARTIFACTS_DIR" -type f -name "$pattern" | sort | head -n 1 || true)"
  if [[ -n "$source" ]]; then
    cp -f "$source" "$destination/"
  fi
}

copy_matching() {
  local destination="$1"
  shift
  mkdir -p "$destination"
  find "$ARTIFACTS_DIR" -type f "$@" -print0 |
    while IFS= read -r -d '' file; do
      cp -f "$file" "$destination/"
    done
}

copy_preserving_path() {
  local file="$1"
  local destination_root="$2"
  local relative="${file#"$ARTIFACTS_DIR"/}"
  local destination="$destination_root/$relative"
  mkdir -p "$(dirname "$destination")"
  cp -f "$file" "$destination"
}

zip_directory() {
  local source_dir="$1"
  local output_zip="$2"
  local output_dir
  local output_abs
  output_dir="$(mkdir -p "$(dirname "$output_zip")" && cd "$(dirname "$output_zip")" && pwd)"
  output_abs="$output_dir/$(basename "$output_zip")"
  rm -f "$output_abs"
  if command -v zip >/dev/null 2>&1; then
    (cd "$source_dir" && zip -qr "$output_abs" .)
    return 0
  fi
  python3 - "$source_dir" "$output_abs" <<'PY'
import pathlib
import sys
import zipfile

source = pathlib.Path(sys.argv[1])
output = pathlib.Path(sys.argv[2])
with zipfile.ZipFile(output, "w", compression=zipfile.ZIP_DEFLATED) as archive:
    for path in sorted(source.rglob("*")):
        if path.is_file():
            archive.write(path, path.relative_to(source).as_posix())
PY
}

zip_nonempty_dir() {
  local source_dir="$1"
  local output_zip="$2"

  if [[ -d "$source_dir" ]] && find "$source_dir" -type f -print -quit | grep -q .; then
    zip_directory "$source_dir" "$output_zip"
  fi
}

DEVELOPER_ARTIFACT_SOURCE_LOG="$META_DIR/raw/developer-artifact-sources.tsv"

record_developer_artifact_source() {
  local group="$1"
  local source_tag="$2"
  local source_asset="$3"
  local release_asset="$4"
  local source_type="$5"

  if [[ ! -f "$DEVELOPER_ARTIFACT_SOURCE_LOG" ]]; then
    printf 'group\tsource_tag\tsource_asset\trelease_asset\tsource_type\n' \
      > "$DEVELOPER_ARTIFACT_SOURCE_LOG"
  fi
  printf '%s\t%s\t%s\t%s\t%s\n' \
    "$group" "$source_tag" "$source_asset" "$release_asset" "$source_type" \
    >> "$DEVELOPER_ARTIFACT_SOURCE_LOG"
}

github_release_download_available() {
  command -v gh >/dev/null 2>&1 &&
    [[ -n "${GITHUB_REPOSITORY:-}" ]] &&
    [[ -n "${GH_TOKEN:-${GITHUB_TOKEN:-}}" ]]
}

find_release_asset_for_base() {
  local tag="$1"
  local base="$2"
  local assets

  assets="$(gh release view "$tag" --repo "$GITHUB_REPOSITORY" --json assets --jq '.assets[].name' 2>/dev/null || true)"
  while IFS= read -r asset; do
    [[ -z "$asset" ]] && continue
    if [[ "$asset" == "$base"-v*.zip || "$asset" == "$base".zip ]]; then
      printf '%s\n' "$asset"
      return 0
    fi
  done <<< "$assets"

  return 1
}

download_previous_release_asset() {
  local base="$1"
  local destination="$2"
  local group="$3"

  if ! github_release_download_available; then
    echo "GitHub release download is unavailable; cannot backfill $group artifact '$base'." >&2
    return 1
  fi

  local releases
  releases="$(gh release list --repo "$GITHUB_REPOSITORY" --limit 100 --json tagName --jq '.[].tagName')"

  local tag
  while IFS= read -r tag; do
    [[ -z "$tag" ]] && continue
    [[ "$tag" == "$(current_tag)" ]] && continue

    local asset
    asset="$(find_release_asset_for_base "$tag" "$base" || true)"
    if [[ -z "$asset" ]]; then
      continue
    fi

    local tmp_dir
    tmp_dir="$(mktemp -d)"
    gh release download "$tag" \
      --repo "$GITHUB_REPOSITORY" \
      --pattern "$asset" \
      --dir "$tmp_dir" >/dev/null

    local downloaded
    downloaded="$tmp_dir/$asset"
    if [[ ! -f "$downloaded" ]]; then
      downloaded="$(find "$tmp_dir" -maxdepth 1 -type f -name "$asset" -print -quit || true)"
    fi
    if [[ -z "$downloaded" || ! -f "$downloaded" ]]; then
      rm -rf "$tmp_dir"
      echo "Downloaded $asset from $tag, but the file was not found locally." >&2
      return 1
    fi

    local artifact_source_tag="$tag"
    local release_asset="$asset"
    if [[ "$asset" == "$base.zip" ]]; then
      release_asset="$(versioned_zip_name "$base" "$tag")"
    elif [[ "$asset" == "$base"-v*.zip ]]; then
      artifact_source_tag="${asset#"$base"-}"
      artifact_source_tag="${artifact_source_tag%.zip}"
    fi

    mkdir -p "$destination"
    cp -f "$downloaded" "$destination/$release_asset"
    rm -rf "$tmp_dir"

    record_developer_artifact_source "$group" "$artifact_source_tag" "$asset" "$release_asset" "backfilled"
    echo "Backfilled $group artifact from $tag as $release_asset (artifact source $artifact_source_tag)"
    return 0
  done <<< "$releases"

  echo "No previous release asset found for $group artifact '$base'." >&2
  return 1
}

stage_developer_archive() {
  local changed="$1"
  local source_dir="$2"
  local base="$3"
  local group="$4"

  if is_true "$changed"; then
    local release_asset
    release_asset="$(versioned_zip_name "$base")"
    zip_nonempty_dir "$source_dir" "$RELEASE_DIR/$release_asset"
    if [[ ! -f "$RELEASE_DIR/$release_asset" ]]; then
      echo "Expected current $group artifact was not produced: $release_asset" >&2
      return 1
    fi
    record_developer_artifact_source "$group" "$(current_tag)" "(current build)" "$release_asset" "current"
    return 0
  fi

  download_previous_release_asset "$base" "$RELEASE_DIR" "$group"
}

stage_ios_spm_binary() {
  local base="PirateWalletNative.xcframework"
  local legacy="$RELEASE_DIR/$base.zip"
  if [[ -f "$legacy" ]]; then
    local release_asset
    release_asset="$(versioned_zip_name "$base")"
    mv -f "$legacy" "$RELEASE_DIR/$release_asset"
    record_developer_artifact_source "ios-spm-binary" "$(current_tag)" "(current build)" "$release_asset" "current"
  fi

  if ! find "$RELEASE_DIR" -maxdepth 1 -type f -name "$base-v*.zip" -print -quit | grep -q .; then
    download_previous_release_asset "$base" "$RELEASE_DIR" "ios-spm-binary"
  fi

  local binary
  binary="$(find "$RELEASE_DIR" -maxdepth 1 -type f -name "$base-v*.zip" | sort | head -n 1 || true)"
  if [[ -z "$binary" || ! -f "$binary" ]]; then
    echo "No iOS SPM binary asset is available for release packaging." >&2
    return 1
  fi

  if [[ -n "${GITHUB_REPOSITORY:-}" && -n "${GITHUB_REF_NAME:-}" ]]; then
    local checksum
    local url
    checksum="$(sha256sum "$binary" | awk '{print $1}')"
    url="https://github.com/${GITHUB_REPOSITORY}/releases/download/${GITHUB_REF_NAME}/$(basename "$binary")"
    chmod +x scripts/generate-ios-spm-release-manifest.sh
    scripts/generate-ios-spm-release-manifest.sh \
      "${GITHUB_REPOSITORY}" \
      "${GITHUB_REF_NAME}" \
      "$url" \
      "$checksum" \
      > "$RELEASE_DIR/PirateWalletSDK-Package.swift"
  fi
}

macos_notary_pending=false
if find "$ARTIFACTS_DIR" -type f -path '*macos-dmg-notary-pending*' -name '*.notary.json' -print -quit | grep -q .; then
  macos_notary_pending=true
fi

# Top-level release assets should be normal-user installables only.
find "$ARTIFACTS_DIR" -type f \( \
  -name 'pirate-unified-wallet-android-*.apk' \
  -o -name 'pirate-unified-wallet-windows-installer.exe' \
  -o -name 'pirate-unified-wallet-windows-portable.zip' \
  -o -name 'pirate-unified-wallet-macos.dmg' \
  -o -name 'pirate-unified-wallet-ios.ipa' \
\) ! -name '*-unsigned*' -print0 |
  while IFS= read -r -d '' file; do
    cp -f "$file" "$RELEASE_DIR/"
  done

# If signing is unavailable, publish unsigned desktop builds so testers still
# receive installable artifacts. Unsigned mobile/store builds stay in the
# developer bundle because regular users cannot install them safely.
if [[ ! -f "$RELEASE_DIR/pirate-unified-wallet-windows-installer.exe" ]]; then
  copy_first 'pirate-unified-wallet-windows-installer-unsigned.exe' "$RELEASE_DIR"
fi
if [[ ! -f "$RELEASE_DIR/pirate-unified-wallet-windows-portable.zip" ]]; then
  copy_first 'pirate-unified-wallet-windows-portable-unsigned.zip' "$RELEASE_DIR"
fi
if [[ ! -f "$RELEASE_DIR/pirate-unified-wallet-macos.dmg" ]]; then
  if [[ "$macos_notary_pending" == "true" ]]; then
    echo "Signed macOS notarization is pending; publishing explicit unsigned macOS fallback."
  fi
  copy_first 'pirate-unified-wallet-macos-unsigned.dmg' "$RELEASE_DIR"
fi

copy_matching "$RELEASE_DIR" \( \
  -name '*.AppImage' \
  -o -name '*.deb' \
  -o -name '*.flatpak' \
\)

# Keep the iOS SDK binary as a direct asset when published; Swift Package
# Manager binary targets require a direct release URL to the xcframework zip.
if is_true "${IOS_SDK_CHANGED:-false}"; then
  copy_first 'PirateWalletNative.xcframework.zip' "$RELEASE_DIR"
fi
stage_ios_spm_binary

# Developer-facing artifacts are grouped into purpose-specific archives instead
# of being exposed as dozens of top-level release assets. Keep each archive
# comfortably under GitHub's 2 GiB per-release-asset upload limit.
cli_archive_changed=false
if is_true "${CLI_CHANGED:-false}" || is_true "${QORTAL_CLI_CHANGED:-false}"; then
  cli_archive_changed=true
  copy_matching "$DEV_DIR/cli" \( \
    -name 'piratewallet-cli' \
    -o -name 'piratewallet-cli.exe' \
    -o -name 'pirate-qortal-cli' \
    -o -name 'pirate-qortal-cli.exe' \
  \)
fi

native_ffi_archive_changed=false
if is_true "${NATIVE_FFI_CHANGED:-false}" ||
  is_true "${IOS_SDK_CHANGED:-false}" ||
  is_true "${ANDROID_SDK_CHANGED:-false}"; then
  native_ffi_archive_changed=true
  copy_matching "$DEV_DIR/native-ffi" \( \
    -name 'libpirate_ffi_native.a' \
    -o -name 'libpirate_ffi_native.so' \
    -o -name 'pirate_ffi_native.dll' \
    -o -name 'pirate_ffi_native.lib' \
    -o -name 'pirate_wallet_service.h' \
  \)
fi

if is_true "${IOS_SDK_CHANGED:-false}"; then
  copy_matching "$DEV_DIR/sdk/ios" \( \
    -name 'PirateWalletNative.xcframework.zip' \
    -o -name 'PirateWalletSDK-package.zip' \
    -o -name 'PirateWalletSDK-Package.swift' \
  \)
fi

if is_true "${ANDROID_SDK_CHANGED:-false}"; then
  copy_matching "$DEV_DIR/sdk/android" \( \
    -name '*.aar' \
    -o -name 'pirate-android-sdk-package.zip' \
  \)
fi

if is_true "${REACT_NATIVE_PLUGIN_CHANGED:-false}"; then
  copy_matching "$DEV_DIR/sdk/react-native" \( \
    -name 'react-native-pirate-wallet-package.zip' \
  \)
fi

stage_developer_archive "$cli_archive_changed" \
  "$DEV_DIR/cli" \
  "pirate-unified-wallet-cli-artifacts" \
  "cli"
stage_developer_archive "$native_ffi_archive_changed" \
  "$DEV_DIR/native-ffi" \
  "pirate-unified-wallet-native-ffi-artifacts" \
  "native-ffi"
stage_developer_archive "${IOS_SDK_CHANGED:-false}" \
  "$DEV_DIR/sdk/ios" \
  "pirate-unified-wallet-ios-sdk-artifacts" \
  "ios-sdk"
stage_developer_archive "${ANDROID_SDK_CHANGED:-false}" \
  "$DEV_DIR/sdk/android" \
  "pirate-unified-wallet-android-sdk-artifacts" \
  "android-sdk"
stage_developer_archive "${REACT_NATIVE_PLUGIN_CHANGED:-false}" \
  "$DEV_DIR/sdk/react-native" \
  "pirate-unified-wallet-react-native-plugin-artifacts" \
  "react-native-plugin"

copy_matching "$DEV_DIR/mobile-store-and-test-builds" \( \
  -name 'pirate-unified-wallet-android*.aab' \
  -o -name 'pirate-unified-wallet-android-*-unsigned.apk' \
  -o -name 'pirate-unified-wallet-ios-unsigned.ipa' \
\)

copy_matching "$DEV_DIR/unsigned-desktop-test-builds" \( \
  -name 'pirate-unified-wallet-windows-*-unsigned.*' \
  -o -name 'pirate-unified-wallet-macos-unsigned.dmg' \
\)

if find "$DEV_DIR" -type f -print -quit | grep -q .; then
  zip_nonempty_dir "$DEV_DIR/mobile-store-and-test-builds" "$RELEASE_DIR/pirate-unified-wallet-mobile-store-test-builds.zip"
  zip_nonempty_dir "$DEV_DIR/unsigned-desktop-test-builds" "$RELEASE_DIR/pirate-unified-wallet-unsigned-desktop-test-builds.zip"
else
  rm -rf "$DEV_DIR"
fi

# Preserve full security/build metadata in the metadata bundle. This includes
# checksums, detached signatures, SBOMs, provenance, and optional VirusTotal
# reports without flooding the top-level GitHub Assets list.
find "$ARTIFACTS_DIR" -type f \( \
  -name '*.sha256' \
  -o -name '*.asc' \
  -o -name '*.spdx.json' \
  -o -name '*.cdx.json' \
  -o -name '*sbom*.json' \
  -o -name '*dependencies*.json' \
  -o -name '*dependencies*.txt' \
  -o -name '*dependency-tree*.txt' \
  -o -name 'SBOM-SUMMARY.md' \
  -o -name '*.provenance.json' \
  -o -name '*.sigstore.bundle' \
  -o -name '*.VERIFY.md' \
  -o -name '*.notary.json' \
  -o -name 'virustotal-*' \
\) -print0 |
  while IFS= read -r -d '' file; do
    copy_preserving_path "$file" "$META_DIR/raw"
  done

# Ensure every top-level release asset has a checksum in the metadata bundle,
# including grouped developer artifacts and generated Swift package manifests.
find "$RELEASE_DIR" -maxdepth 1 -type f ! -name 'pirate-unified-wallet-release-metadata.zip' -print0 |
  while IFS= read -r -d '' file; do
    hash="$(sha256sum "$file" | awk '{print $1}')"
    printf '%s  %s\n' "$hash" "$(basename "$file")" > "$META_DIR/checksums/$(basename "$file").sha256"
  done

if find "$META_DIR" -type f -print -quit | grep -q .; then
  zip_directory "$META_DIR" "$RELEASE_DIR/pirate-unified-wallet-release-metadata.zip"
else
  echo "No release metadata files found to package."
fi

python3 - "$RELEASE_DIR" <<'PY'
import pathlib
import sys

release_dir = pathlib.Path(sys.argv[1])
limit = 2_147_483_648
oversized = [
    path
    for path in sorted(release_dir.iterdir())
    if path.is_file() and path.stat().st_size >= limit
]
if oversized:
    print("Release assets must be smaller than GitHub's 2 GiB upload limit:", file=sys.stderr)
    for path in oversized:
        print(f"- {path.name}: {path.stat().st_size} bytes", file=sys.stderr)
    sys.exit(1)
PY

echo "Release files:"
ls -la "$RELEASE_DIR"
