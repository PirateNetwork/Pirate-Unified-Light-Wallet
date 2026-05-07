#!/usr/bin/env bash
# iOS IPA build and signing script (TestFlight ready)
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
APP_DIR="$PROJECT_ROOT/app"
IOS_DIR="$APP_DIR/ios"

GREEN='\033[0;32m'
RED='\033[0;31m'
YELLOW='\033[1;33m'
NC='\033[0m'

log() {
    echo -e "${GREEN}[$(date +'%Y-%m-%d %H:%M:%S')]${NC} $1"
}

error() {
    echo -e "${RED}[ERROR]${NC} $1" >&2
    exit 1
}

warn() {
    echo -e "${YELLOW}[WARN]${NC} $1"
}

# Check if running on macOS
if [[ "$OSTYPE" != "darwin"* ]]; then
    error "iOS builds require macOS"
fi

# Reproducible build settings
export SOURCE_DATE_EPOCH="${SOURCE_DATE_EPOCH:-$(git log -1 --format=%ct 2>/dev/null || date +%s)}"
export TZ=UTC
export FLUTTER_SUPPRESS_ANALYTICS=true
export DART_SUPPRESS_ANALYTICS=true
export CARGO_INCREMENTAL=0

log "Building iOS IPA (reproducible)"
log "SOURCE_DATE_EPOCH: $SOURCE_DATE_EPOCH"

normalize_mtime() {
    local target="$1"
    if [ -z "${SOURCE_DATE_EPOCH:-}" ]; then
        return 0
    fi
    local stamp
    stamp="$(date -u -d "@$SOURCE_DATE_EPOCH" +"%Y%m%d%H%M.%S" 2>/dev/null || date -u -r "$SOURCE_DATE_EPOCH" +"%Y%m%d%H%M.%S")"
    find "$target" -exec touch -t "$stamp" {} + 2>/dev/null || true
}

REPRODUCIBLE="${REPRODUCIBLE:-0}"

zip_dir_deterministic() {
    local src="$1"
    local dest="$2"
    (cd "$src" && normalize_mtime "." && LC_ALL=C find . -type f -print | sort | zip -X -@ "$dest")
}

zip_payload_deterministic() {
    local payload_dir="$1"
    local dest="$2"
    local parent
    parent="$(dirname "$payload_dir")"
    local base
    base="$(basename "$payload_dir")"
    (cd "$parent" && normalize_mtime "$base" && LC_ALL=C find "$base" -type f -print | sort | zip -X -@ "$dest")
}

configure_runner_signing() {
    local project_file="$1"

    if [ -z "${IOS_TEAM_ID:-}" ]; then
        error "IOS_TEAM_ID is required for signed iOS builds"
    fi
    if [ -z "${IOS_PROVISIONING_PROFILE_SPECIFIER:-}" ]; then
        error "IOS_PROVISIONING_PROFILE_SPECIFIER is required for signed iOS builds"
    fi

    IOS_CODE_SIGN_IDENTITY="${IOS_CODE_SIGN_IDENTITY:-Apple Distribution}" \
    python3 - "$project_file" <<'PY'
import os
import re
import sys
from pathlib import Path

project_file = Path(sys.argv[1])
text = project_file.read_text(encoding="utf-8")

team_id = os.environ["IOS_TEAM_ID"]
profile = os.environ["IOS_PROVISIONING_PROFILE_SPECIFIER"]
identity = os.environ.get("IOS_CODE_SIGN_IDENTITY", "Apple Distribution")

settings_to_apply = {
    "CODE_SIGN_IDENTITY": identity,
    "CODE_SIGN_IDENTITY[sdk=iphoneos*]": identity,
    "CODE_SIGN_STYLE": "Manual",
    "DEVELOPMENT_TEAM": team_id,
    "PROVISIONING_PROFILE_SPECIFIER": profile,
}


def render_value(value: str) -> str:
    if re.search(r"[\s$()]", value):
        return f'"{value}"'
    return value


def render_key(key: str) -> str:
    if re.search(r"[\[\]*]", key):
        return f'"{key}"'
    return key


def set_build_setting(settings: str, key: str, value: str) -> str:
    rendered_key = render_key(key)
    rendered_line = f"\t\t\t\t{rendered_key} = {render_value(value)};"
    key_pattern = re.escape(key)
    quoted_key_pattern = re.escape(f'"{key}"')
    line_pattern = re.compile(rf"^\t\t\t\t(?:{key_pattern}|{quoted_key_pattern}) = .+?;$")

    lines = settings.splitlines()
    for index, line in enumerate(lines):
        if line_pattern.match(line):
            lines[index] = rendered_line
            return "\n".join(lines) + "\n"

    insert_at = next(
        (index for index, line in enumerate(lines) if "PRODUCT_BUNDLE_IDENTIFIER = com.pirate.wallet;" in line),
        len(lines),
    )
    lines.insert(insert_at, rendered_line)
    return "\n".join(lines) + "\n"


block_pattern = re.compile(
    r"(?P<head>\t\t[0-9A-F]+ /\* (?P<name>Debug|Release|Profile) \*/ = \{\n"
    r"\t\t\tisa = XCBuildConfiguration;\n"
    r"(?:\t\t\tbaseConfigurationReference = [^\n]+;\n)?"
    r"\t\t\tbuildSettings = \{\n)"
    r"(?P<settings>.*?)"
    r"(?P<tail>\t\t\t\};\n\t\t\tname = (?P=name);\n\t\t\};)",
    re.S,
)

updated_count = 0


def update_block(match: re.Match[str]) -> str:
    global updated_count
    settings = match.group("settings")
    if "PRODUCT_BUNDLE_IDENTIFIER = com.pirate.wallet;" not in settings:
        return match.group(0)

    for key, value in settings_to_apply.items():
        settings = set_build_setting(settings, key, value)
    updated_count += 1
    return f"{match.group('head')}{settings}{match.group('tail')}"


updated = block_pattern.sub(update_block, text)
if updated_count != 3:
    raise SystemExit(f"Expected to update 3 Runner signing configurations, updated {updated_count}")

project_file.write_text(updated, encoding="utf-8")
PY
}

cd "$APP_DIR"

# On tag builds, align app version metadata with the git tag (vX.Y.Z).
bash "$SCRIPT_DIR/sync-version-from-tag.sh"

# Clean previous builds
log "Cleaning previous builds..."
flutter clean

# Get dependencies
log "Fetching dependencies..."
flutter pub get --enforce-lockfile
pushd "$IOS_DIR" >/dev/null
if [ -f Podfile.lock ]; then
    pod install --deployment
else
    pod install
fi
popd >/dev/null

# Build unsigned IPA first
log "Building iOS app..."
flutter build ios --release --no-codesign

# Resolve build output paths
APP_BUILD_DIR="$APP_DIR/build/ios/iphoneos"
RUNNER_APP="$APP_BUILD_DIR/Runner.app"
if [ ! -d "$RUNNER_APP" ]; then
    RUNNER_APP="$(find "$APP_DIR/build" -path "*/ios/iphoneos/Runner.app" -type d | head -n 1 || true)"
    if [ -n "$RUNNER_APP" ]; then
        APP_BUILD_DIR="$(dirname "$RUNNER_APP")"
    fi
fi

# Check for signing configuration
SIGN="${1:-auto}"  # auto, true, or false
SIGNED=false
if [ "$REPRODUCIBLE" = "1" ]; then
    SIGN=false
fi

if [ "$SIGN" = "auto" ]; then
    # Check if we have signing certificates
    if security find-identity -v -p codesigning | grep -q "iPhone Distribution"; then
        SIGN=true
    else
        SIGN=false
        warn "No code signing identity found. Building unsigned IPA."
    fi
fi

if [ "$SIGN" = "true" ]; then
    log "Code signing iOS app..."

    WORKSPACE_PATH="$IOS_DIR/Runner.xcworkspace"
    EXPORT_OPTIONS_PLIST="$IOS_DIR/ExportOptions.plist"
    ARCHIVE_PATH="$APP_DIR/build/ios/Runner.xcarchive"
    EXPORT_PATH="$APP_DIR/build/ios/ipa"

    if [ ! -d "$WORKSPACE_PATH" ]; then
        error "iOS workspace not found: $WORKSPACE_PATH"
    fi
    if [ ! -f "$EXPORT_OPTIONS_PLIST" ]; then
        error "iOS export options not found: $EXPORT_OPTIONS_PLIST"
    fi

    configure_runner_signing "$IOS_DIR/Runner.xcodeproj/project.pbxproj"

    archive_signing_args=()
    if [ -n "${IOS_SIGN_KEYCHAIN:-}" ]; then
        archive_signing_args+=(OTHER_CODE_SIGN_FLAGS="--keychain $IOS_SIGN_KEYCHAIN")
    fi
    
    # Export IPA with signing
    xcodebuild -workspace "$WORKSPACE_PATH" \
        -scheme Runner \
        -sdk iphoneos \
        -configuration Release \
        -destination "generic/platform=iOS" \
        archive -archivePath "$ARCHIVE_PATH" \
        "${archive_signing_args[@]}"
    
    xcodebuild -exportArchive \
        -archivePath "$ARCHIVE_PATH" \
        -exportOptionsPlist "$EXPORT_OPTIONS_PLIST" \
        -exportPath "$EXPORT_PATH"

    mapfile -t exported_ipas < <(find "$EXPORT_PATH" -maxdepth 1 -type f -name "*.ipa" | sort)
    if [ "${#exported_ipas[@]}" -ne 1 ]; then
        find "$EXPORT_PATH" -maxdepth 2 -print >&2 || true
        error "Expected exactly one exported IPA under $EXPORT_PATH, found ${#exported_ipas[@]}"
    fi

    IPA_FILE="${exported_ipas[0]}"
    SIGNED=true
else
    # Create unsigned IPA
    log "Creating unsigned IPA..."

    if [ -z "$RUNNER_APP" ] || [ ! -d "$RUNNER_APP" ]; then
        error "Build failed: Runner.app not found under $APP_DIR/build"
    fi

    PAYLOAD_DIR="$APP_BUILD_DIR/Payload"
    rm -rf "$PAYLOAD_DIR"
    mkdir -p "$PAYLOAD_DIR"
    cp -R "$RUNNER_APP" "$PAYLOAD_DIR/"
    zip_payload_deterministic "$PAYLOAD_DIR" "$APP_BUILD_DIR/Runner.ipa"
    IPA_FILE="$APP_BUILD_DIR/Runner.ipa"
fi

if [ ! -f "$IPA_FILE" ]; then
    error "Build failed: $IPA_FILE not found"
fi

# Create output directory
OUTPUT_DIR="$PROJECT_ROOT/dist/ios"
mkdir -p "$OUTPUT_DIR"

OUTPUT_NAME="pirate-unified-wallet-ios"
if [ "$SIGNED" != "true" ]; then
    OUTPUT_NAME="${OUTPUT_NAME}-unsigned"
fi
OUTPUT_NAME="${OUTPUT_NAME}.ipa"

# Copy artifacts
log "Copying artifacts..."
cp "$IPA_FILE" "$OUTPUT_DIR/$OUTPUT_NAME"

# Generate SHA-256 checksum
log "Generating checksum..."
cd "$OUTPUT_DIR"
shasum -a 256 "$OUTPUT_NAME" > "$OUTPUT_NAME.sha256"

log "Build complete!"
log "Output: $OUTPUT_DIR/$OUTPUT_NAME"
log "SHA-256: $(cat "$OUTPUT_NAME.sha256")"

if [ "$SIGN" = "true" ]; then
    log "IPA is signed and ready for TestFlight upload"
else
    warn "IPA is unsigned. Sign before submitting to App Store."
fi
