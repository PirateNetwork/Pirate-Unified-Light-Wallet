#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
DIST_DIR="${1:-$PROJECT_ROOT/dist/react-native-plugin}"
if [[ "$DIST_DIR" != /* ]]; then
  DIST_DIR="$PROJECT_ROOT/$DIST_DIR"
fi
MAX_TARBALL_BYTES="${MAX_NPM_TARBALL_BYTES:-195000000}"

package_names=(
  "react-native-pirate-wallet-android"
  "react-native-pirate-wallet-android-x86_64"
  "react-native-pirate-wallet-ios-device"
  "react-native-pirate-wallet-ios-simulator-arm64"
  "react-native-pirate-wallet-ios-simulator-x86_64"
  "react-native-pirate-wallet"
)

zip_directory() {
  local source_dir="$1"
  local destination="$2"
  if command -v zip >/dev/null 2>&1; then
    (cd "$source_dir" && zip -qr "$destination" .)
    return
  fi
  if command -v powershell.exe >/dev/null 2>&1 && command -v cygpath >/dev/null 2>&1; then
    local source_windows
    local destination_windows
    source_windows="$(cygpath -w "$source_dir")"
    destination_windows="$(cygpath -w "$destination")"
    powershell.exe -NoProfile -NonInteractive -Command \
      "Compress-Archive -Path '$source_windows\\*' -DestinationPath '$destination_windows' -CompressionLevel Optimal -Force"
    return
  fi
  echo "Missing a zip implementation" >&2
  return 1
}

rm -rf "$DIST_DIR"
mkdir -p "$DIST_DIR/packages"

for package_name in "${package_names[@]}"; do
  source_dir="$PROJECT_ROOT/bindings/$package_name"
  package_dir="$DIST_DIR/packages/$package_name"
  mkdir -p "$package_dir"
  if command -v rsync >/dev/null 2>&1; then
    rsync -a \
      --delete \
      --exclude '.git' \
      --exclude '.gradle' \
      --exclude 'build' \
      --exclude 'node_modules' \
      "$source_dir/" \
      "$package_dir/"
  else
    tar \
      --exclude='.git' \
      --exclude='.gradle' \
      --exclude='build' \
      --exclude='node_modules' \
      -C "$source_dir" -cf - . | tar -C "$package_dir" -xf -
  fi

  if [[ "$package_name" == "react-native-pirate-wallet" ]]; then
    rm -rf "$package_dir/ios/Frameworks/PirateWalletNative.xcframework"
  fi

  (
    cd "$package_dir"
    node scripts/verify-package.js --publish-layout
    npm pack --json --pack-destination "$DIST_DIR" \
      > "$DIST_DIR/$package_name-npm-pack.json"
    zip_directory "$package_dir" "$DIST_DIR/$package_name-package.zip"
  )

  package_version="$(node -e \
    'process.stdout.write(require(process.argv[1]).version)' \
    "$package_dir/package.json")"
  tarball="$DIST_DIR/$package_name-$package_version.tgz"
  if [[ ! -f "$tarball" ]]; then
    echo "Expected npm tarball was not produced: $tarball" >&2
    exit 1
  fi

  tarball_size="$(wc -c < "$tarball" | tr -d '[:space:]')"
  if (( tarball_size > MAX_TARBALL_BYTES )); then
    echo "$package_name tarball is too large for npm publish: $tarball_size bytes" >&2
    node - "$DIST_DIR/$package_name-npm-pack.json" <<'NODE'
const fs = require('fs');

const report = JSON.parse(fs.readFileSync(process.argv[2], 'utf8'))[0];
const files = [...(report?.files ?? [])]
  .sort((left, right) => right.size - left.size)
  .slice(0, 10);
console.error('Largest packed files:');
for (const file of files) {
  console.error(`  ${file.size} bytes  ${file.path}`);
}
NODE
    exit 1
  fi

  (
    cd "$DIST_DIR"
    sha256sum "$(basename "$tarball")" > "$(basename "$tarball").sha256"
    sha256sum "$package_name-package.zip" \
      > "$package_name-package.zip.sha256"
  )
  echo "Packed $package_name@$package_version ($tarball_size bytes)"
done

consumer_dir="$(mktemp -d)"
trap 'rm -rf "$consumer_dir"' EXIT
(
  cd "$consumer_dir"
  npm init --yes >/dev/null
  npm install \
    --force \
    --ignore-scripts \
    --legacy-peer-deps \
    --no-audit \
    --no-fund \
    "$DIST_DIR"/*.tgz
  node node_modules/react-native-pirate-wallet-android/scripts/verify-package.js \
    --publish-layout
  node node_modules/react-native-pirate-wallet-android-x86_64/scripts/verify-package.js
  node node_modules/react-native-pirate-wallet-ios-device/scripts/verify-package.js
  node node_modules/react-native-pirate-wallet-ios-simulator-arm64/scripts/verify-package.js
  node node_modules/react-native-pirate-wallet-ios-simulator-x86_64/scripts/verify-package.js
  node node_modules/react-native-pirate-wallet/scripts/verify-package.js
  node -e '
    const fs = require("fs");
    const {resolveAndroidJniLibsPaths} = require(
      "react-native-pirate-wallet/scripts/resolve-android-packages"
    );
    const jniLibsPaths = resolveAndroidJniLibsPaths();
    for (const jniLibs of jniLibsPaths) {
      if (!fs.statSync(jniLibs, {throwIfNoEntry: false})?.isDirectory()) {
        throw new Error(`Resolved Android JNI directory does not exist: ${jniLibs}`);
      }
    }
  '
  npm --prefix node_modules/react-native-pirate-wallet test
)
