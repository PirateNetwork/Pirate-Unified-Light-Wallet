#!/usr/bin/env bash

set -euo pipefail

if [[ $# -lt 3 || $# -gt 4 ]]; then
  echo "Usage: $0 RELEASE_DIR OUTPUT_ZIP RELEASE_TAG [GPG_KEY]" >&2
  exit 64
fi

RELEASE_DIR="$(cd "$1" && pwd)"
OUTPUT_ZIP="$2"
RELEASE_TAG="$3"
GPG_KEY="${4:-${GPG_SIGNING_KEY:-}}"
PROJECT_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
README_SOURCE="$PROJECT_ROOT/release-signing/README"
PUBLIC_KEY_SOURCE="$PROJECT_ROOT/release-signing/public_key.asc"
EXPECTED_PRIMARY_FINGERPRINT="E4FB2399AECCF9B9447DED472CE65343401553A6"

if [[ -z "$GPG_KEY" ]]; then
  echo "A GPG signing key fingerprint is required." >&2
  exit 64
fi
if [[ ! -f "$README_SOURCE" || ! -f "$PUBLIC_KEY_SOURCE" ]]; then
  echo "Release verification instructions or public key are missing." >&2
  exit 1
fi

mapfile -t public_fingerprints < <(
  gpg --batch --with-colons --show-keys --fingerprint "$PUBLIC_KEY_SOURCE" |
    awk -F: '$1 == "fpr" { print $10 }'
)
mapfile -t secret_fingerprints < <(
  gpg --batch --with-colons --list-secret-keys --fingerprint "$GPG_KEY" |
    awk -F: '$1 == "fpr" { print $10 }'
)
if [[ " ${public_fingerprints[*]} " != *" $EXPECTED_PRIMARY_FINGERPRINT "* ]]; then
  echo "The repository public key is not the Stashi Wallet release key." >&2
  exit 1
fi
if [[ " ${secret_fingerprints[*]} " != *" $EXPECTED_PRIMARY_FINGERPRINT "* ]]; then
  echo "The selected private key cannot sign as the Stashi Wallet release key." >&2
  exit 1
fi

OUTPUT_DIR="$(mkdir -p "$(dirname "$OUTPUT_ZIP")" && cd "$(dirname "$OUTPUT_ZIP")" && pwd)"
OUTPUT_ZIP="$OUTPUT_DIR/$(basename "$OUTPUT_ZIP")"
rm -f "$OUTPUT_ZIP"

STAGE_DIR="$(mktemp -d)"
cleanup() {
  rm -rf -- "$STAGE_DIR"
}
trap cleanup EXIT

cp -f "$README_SOURCE" "$STAGE_DIR/README"
cp -f "$PUBLIC_KEY_SOURCE" "$STAGE_DIR/public_key.asc"

CHECKSUM_MANIFEST="$STAGE_DIR/sha256sum-${RELEASE_TAG}.txt"
: > "$CHECKSUM_MANIFEST"

mapfile -d '' RELEASE_FILES < <(
  find "$RELEASE_DIR" -maxdepth 1 -type f \
    ! -name 'signatures-*.zip' \
    ! -name '*.sig' \
    ! -name 'README' \
    ! -name 'public_key.asc' \
    ! -name 'sha256sum-*.txt' \
    ! -name 'build-payloads-*.txt' \
    -print0 | sort -z
)
if [[ ${#RELEASE_FILES[@]} -eq 0 ]]; then
  echo "No release files found in $RELEASE_DIR." >&2
  exit 1
fi

sign_file() {
  local input="$1"
  local output="$2"
  gpg --batch --yes --pinentry-mode loopback \
    --passphrase "${GPG_PASSPHRASE:-}" \
    --local-user "$GPG_KEY" \
    --digest-algo SHA512 \
    --detach-sign \
    --output "$output" \
    "$input"
}

for file in "${RELEASE_FILES[@]}"; do
  filename="$(basename "$file")"
  hash="$(sha256sum "$file" | awk '{print $1}')"
  printf '%s  %s\n' "$hash" "$filename" >> "$CHECKSUM_MANIFEST"
  sign_file "$file" "$STAGE_DIR/$filename.sig"
done

sign_file "$CHECKSUM_MANIFEST" "$CHECKSUM_MANIFEST.sig"

METADATA_ZIP="$RELEASE_DIR/Stashi-Wallet-release-metadata.zip"
PAYLOAD_MANIFEST="$STAGE_DIR/build-payloads-${RELEASE_TAG}.txt"
if [[ -f "$METADATA_ZIP" ]]; then
  python3 - "$METADATA_ZIP" "$PAYLOAD_MANIFEST" <<'PY'
import pathlib
import re
import sys
import zipfile

archive_path = pathlib.Path(sys.argv[1])
output_path = pathlib.Path(sys.argv[2])
line_pattern = re.compile(r"^([0-9a-fA-F]{64})[ \t]+[*]?([^\\/]+)$")
entries: dict[str, str] = {}

with zipfile.ZipFile(archive_path) as archive:
    candidates = sorted(
        name
        for name in archive.namelist()
        if pathlib.PurePosixPath(name).name.startswith("installed-payload-")
        and name.endswith(".txt")
    )
    by_platform: dict[str, list[str]] = {}
    for name in candidates:
        filename = pathlib.PurePosixPath(name).name
        match = re.fullmatch(r"installed-payload-([a-z0-9]+)(-unsigned)?\.txt", filename)
        if match is not None:
            by_platform.setdefault(match.group(1), []).append(name)
    names = []
    for platform, platform_names in sorted(by_platform.items()):
        signed_name = f"installed-payload-{platform}.txt"
        names.append(
            next(
                (name for name in platform_names if pathlib.PurePosixPath(name).name == signed_name),
                platform_names[0],
            )
        )
    for name in names:
        text = archive.read(name).decode("utf-8")
        for raw_line in text.splitlines():
            line = raw_line.strip()
            if not line or line.startswith("#"):
                continue
            match = line_pattern.fullmatch(line)
            if match is None:
                raise SystemExit(f"Invalid installed payload checksum in {name}: {raw_line!r}")
            digest, filename = match.groups()
            digest = digest.lower()
            previous = entries.setdefault(filename, digest)
            if previous != digest:
                raise SystemExit(f"Conflicting installed payload checksum for {filename}")

if entries:
    output_path.write_text(
        "".join(f"{digest}  {filename}\n" for filename, digest in sorted(entries.items())),
        encoding="utf-8",
        newline="\n",
    )
PY
fi

if [[ -s "$PAYLOAD_MANIFEST" ]]; then
  sign_file "$PAYLOAD_MANIFEST" "$PAYLOAD_MANIFEST.sig"
else
  rm -f "$PAYLOAD_MANIFEST"
fi

(
  cd "$RELEASE_DIR"
  sha256sum -c "$CHECKSUM_MANIFEST"
)
gpg --batch --verify "$CHECKSUM_MANIFEST.sig" "$CHECKSUM_MANIFEST"
if [[ -f "$PAYLOAD_MANIFEST" ]]; then
  gpg --batch --verify "$PAYLOAD_MANIFEST.sig" "$PAYLOAD_MANIFEST"
fi

if command -v zip >/dev/null 2>&1; then
  (
    cd "$STAGE_DIR"
    LC_ALL=C find . -maxdepth 1 -type f -printf '%P\n' | sort | zip -q "$OUTPUT_ZIP" -@
  )
else
  python3 - "$STAGE_DIR" "$OUTPUT_ZIP" <<'PY'
import pathlib
import sys
import zipfile

source = pathlib.Path(sys.argv[1])
output = pathlib.Path(sys.argv[2])
with zipfile.ZipFile(output, "w", compression=zipfile.ZIP_DEFLATED) as archive:
    for path in sorted(source.iterdir()):
        if path.is_file():
            archive.write(path, path.name)
PY
fi

while IFS= read -r -d '' verification_file; do
  cp -f "$verification_file" "$RELEASE_DIR/$(basename "$verification_file")"
done < <(find "$STAGE_DIR" -maxdepth 1 -type f -print0)

echo "Created $(basename "$OUTPUT_ZIP") with ${#RELEASE_FILES[@]} signed release files."
