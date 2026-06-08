#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"

fail() {
  echo "[ERROR] $1" >&2
  exit 1
}

expect_prefix() {
  local label="$1"
  local actual="$2"
  local expected="$3"
  if [[ -z "$expected" ]]; then
    return 0
  fi
  if [[ "$actual" != *"$expected"* ]]; then
    fail "$label version mismatch (expected $expected, got: $actual)"
  fi
}

is_windows_shell() {
  local uname_value
  uname_value="$(uname -s 2>/dev/null || true)"
  case "$uname_value" in
    MINGW*|MSYS*|CYGWIN*)
      return 0
      ;;
  esac
  case "${OSTYPE:-}" in
    msys*|win32*|cygwin*)
      return 0
      ;;
  esac
  return 1
}

RUST_EXPECTED="${RUST_VERSION:-}"
if [[ -z "$RUST_EXPECTED" && -f "$PROJECT_ROOT/rust-toolchain.toml" ]]; then
  RUST_EXPECTED="$(awk -F '\"' '/^channel/ {print $2}' "$PROJECT_ROOT/rust-toolchain.toml" || true)"
fi
if command -v rustc &> /dev/null; then
  expect_prefix "Rust" "$(rustc --version)" "$RUST_EXPECTED"
fi

if command -v rustup &> /dev/null; then
  echo "[INFO] Rustup version: $(rustup --version | head -n1)"
fi

FLUTTER_EXPECTED="${FLUTTER_VERSION:-}"
if [[ -n "$FLUTTER_EXPECTED" ]]; then
  if ! command -v flutter &> /dev/null; then
    fail "Flutter not found on PATH"
  fi
  # Skip Flutter version check on Windows in CI (already validated by setup-flutter action)
  # This avoids broken pipe issues during Flutter's first-time tool initialization
  if is_windows_shell && [[ "${CI:-false}" == "true" || "${GITHUB_ACTIONS:-false}" == "true" ]]; then
    echo "[INFO] Skipping Flutter version check on Windows CI (validated by setup action)"
  else
    # Flutter can print "Building flutter tool..." before the version line on
    # first run, so look for the actual version line instead of assuming line 1.
    FLUTTER_OUTPUT="$(flutter --version 2>&1 || true)"
    FLUTTER_VERSION_LINE="$(printf '%s\n' "$FLUTTER_OUTPUT" | awk '/^Flutter[[:space:]]+[0-9]+/ {print; exit}')"
    if [[ -z "$FLUTTER_VERSION_LINE" ]]; then
      FLUTTER_VERSION_LINE="$(printf '%s\n' "$FLUTTER_OUTPUT" | head -n1)"
    fi
    expect_prefix "Flutter" "$FLUTTER_VERSION_LINE" "$FLUTTER_EXPECTED"
  fi
fi

JAVA_EXPECTED="${JAVA_VERSION:-}"
if [[ -n "$JAVA_EXPECTED" ]] && command -v java &> /dev/null; then
  # Capture full output to avoid broken pipe on Windows
  JAVA_OUTPUT="$(java -version 2>&1 || true)"
  JAVA_FIRST_LINE="$(echo "$JAVA_OUTPUT" | head -n1)"
  expect_prefix "Java" "$JAVA_FIRST_LINE" "$JAVA_EXPECTED"
fi

GO_EXPECTED="${GO_VERSION:-}"
if [[ "$GO_EXPECTED" == *.x ]]; then
  GO_EXPECTED="${GO_EXPECTED%.x}"
fi
if [[ -n "$GO_EXPECTED" ]] && command -v go &> /dev/null; then
  GO_OUTPUT="$(go version 2>&1 || true)"
  expect_prefix "Go" "$GO_OUTPUT" "$GO_EXPECTED"
fi

GRADLE_EXPECTED="${GRADLE_VERSION:-}"
if [[ -z "$GRADLE_EXPECTED" && -f "$PROJECT_ROOT/app/android/gradle/wrapper/gradle-wrapper.properties" ]]; then
  GRADLE_EXPECTED="$(awk -F 'gradle-' '/distributionUrl/ {print $2}' "$PROJECT_ROOT/app/android/gradle/wrapper/gradle-wrapper.properties" | awk -F '-' '{print $1}')"
fi
if [[ -n "$GRADLE_EXPECTED" && -x "$PROJECT_ROOT/app/android/gradlew" ]]; then
  # Capture full output to avoid broken pipe on Windows
  GRADLE_OUTPUT="$("$PROJECT_ROOT/app/android/gradlew" --version 2>&1 || true)"
  GRADLE_FIRST_LINES="$(echo "$GRADLE_OUTPUT" | head -n3 | tr '\n' ' ')"
  expect_prefix "Gradle" "$GRADLE_FIRST_LINES" "$GRADLE_EXPECTED"
fi

COCOAPODS_EXPECTED="${COCOAPODS_VERSION:-}"
if [[ -n "$COCOAPODS_EXPECTED" && "$(uname -s)" == "Darwin" ]]; then
  if ! command -v pod &> /dev/null; then
    fail "CocoaPods not found"
  fi
  expect_prefix "CocoaPods" "$(pod --version)" "$COCOAPODS_EXPECTED"
fi

echo "[INFO] Toolchain versions match pinned expectations."
