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
  if [[ -f "$RELEASE_DIR/PirateWalletNative.xcframework.zip" ]] &&
    [[ -n "${GITHUB_REPOSITORY:-}" ]] &&
    [[ -n "${GITHUB_REF_NAME:-}" ]]; then
    checksum="$(sha256sum "$RELEASE_DIR/PirateWalletNative.xcframework.zip" | awk '{print $1}')"
    url="https://github.com/${GITHUB_REPOSITORY}/releases/download/${GITHUB_REF_NAME}/PirateWalletNative.xcframework.zip"
    chmod +x scripts/generate-ios-spm-release-manifest.sh
    scripts/generate-ios-spm-release-manifest.sh \
      "${GITHUB_REPOSITORY}" \
      "${GITHUB_REF_NAME}" \
      "$url" \
      "$checksum" \
      > "$RELEASE_DIR/PirateWalletSDK-Package.swift"
  fi
fi

# Developer-facing artifacts are grouped into one archive with folders instead
# of being exposed as dozens of top-level release assets.
if is_true "${CLI_CHANGED:-false}" || is_true "${QORTAL_CLI_CHANGED:-false}"; then
  copy_matching "$DEV_DIR/cli" \( \
    -name 'piratewallet-cli' \
    -o -name 'piratewallet-cli.exe' \
    -o -name 'pirate-qortal-cli' \
    -o -name 'pirate-qortal-cli.exe' \
  \)
fi

if is_true "${NATIVE_FFI_CHANGED:-false}" ||
  is_true "${IOS_SDK_CHANGED:-false}" ||
  is_true "${ANDROID_SDK_CHANGED:-false}"; then
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

copy_matching "$DEV_DIR/mobile-store-and-test-builds" \( \
  -name 'pirate-unified-wallet-android*.aab' \
  -o -name 'pirate-unified-wallet-android-*-unsigned.apk' \
  -o -name 'pirate-unified-wallet-ios-unsigned.ipa' \
  -o -name 'pirate-unified-wallet-ios-simulator.app.zip' \
\)

copy_matching "$DEV_DIR/unsigned-desktop-test-builds" \( \
  -name 'pirate-unified-wallet-windows-*-unsigned.*' \
  -o -name 'pirate-unified-wallet-macos-unsigned.dmg' \
\)

if find "$DEV_DIR" -type f -print -quit | grep -q .; then
  zip_directory "$DEV_DIR" "$RELEASE_DIR/pirate-unified-wallet-developer-artifacts.zip"
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

echo "Release files:"
ls -la "$RELEASE_DIR"
