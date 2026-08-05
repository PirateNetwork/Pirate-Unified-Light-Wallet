#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
DIST_DIR="${1:-$PROJECT_ROOT/dist/react-native-plugin}"
MAX_TARBALL_BYTES="${MAX_NPM_TARBALL_BYTES:-390000000}"

package_names=(
  "react-native-pirate-wallet-android"
  "react-native-pirate-wallet"
)

rm -rf "$DIST_DIR"
mkdir -p "$DIST_DIR/packages"

for package_name in "${package_names[@]}"; do
  source_dir="$PROJECT_ROOT/bindings/$package_name"
  package_dir="$DIST_DIR/packages/$package_name"
  mkdir -p "$package_dir"
  rsync -a \
    --delete \
    --exclude '.git' \
    --exclude '.gradle' \
    --exclude 'build' \
    --exclude 'node_modules' \
    "$source_dir/" \
    "$package_dir/"

  (
    cd "$package_dir"
    node scripts/verify-package.js --publish-layout
    npm pack --json --pack-destination "$DIST_DIR" \
      > "$DIST_DIR/$package_name-npm-pack.json"
    zip -qr "$DIST_DIR/$package_name-package.zip" .
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
    --ignore-scripts \
    --legacy-peer-deps \
    --no-audit \
    --no-fund \
    "$DIST_DIR"/react-native-pirate-wallet-android-*.tgz \
    "$DIST_DIR"/react-native-pirate-wallet-[0-9]*.tgz
  node node_modules/react-native-pirate-wallet-android/scripts/verify-package.js \
    --publish-layout
  node node_modules/react-native-pirate-wallet/scripts/verify-package.js \
    --publish-layout
  node -e '
    const fs = require("fs");
    const {resolveAndroidJniLibsPath} = require(
      "react-native-pirate-wallet/scripts/resolve-android-package"
    );
    const jniLibs = resolveAndroidJniLibsPath();
    if (!fs.statSync(jniLibs, {throwIfNoEntry: false})?.isDirectory()) {
      throw new Error(`Resolved Android JNI directory does not exist: ${jniLibs}`);
    }
  '
  npm --prefix node_modules/react-native-pirate-wallet test
)
