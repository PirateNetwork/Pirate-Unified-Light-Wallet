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

RUST_EXPECTED="${RUST_VERSION:-}"
if [[ -z "$RUST_EXPECTED" && -f "$PROJECT_ROOT/rust-toolchain.toml" ]]; then
  RUST_EXPECTED="$(awk -F '\"' '/^channel/ {print $2}' "$PROJECT_ROOT/rust-toolchain.toml" || true)"
fi
if command -v rustc &> /dev/null; then
  expect_prefix "Rust" "$(rustc --version)" "$RUST_EXPECTED"
fi

FLUTTER_EXPECTED="${FLUTTER_VERSION:-}"
if [[ -n "$FLUTTER_EXPECTED" ]]; then
  if ! command -v flutter &> /dev/null; then
    fail "Flutter not found on PATH"
  fi
  # Skip Flutter version check on Windows in CI (already validated by setup-flutter action)
  # This avoids broken pipe issues during Flutter's first-time tool initialization
  if [[ "$OSTYPE" == "msys" || "$OSTYPE" == "win32" ]] && [[ "${CI:-false}" == "true" || "${GITHUB_ACTIONS:-false}" == "true" ]]; then
    echo "[INFO] Skipping Flutter version check on Windows CI (validated by setup action)"
  else
    # Capture full output to avoid broken pipe on Windows
    FLUTTER_OUTPUT="$(flutter --version 2>&1 || true)"
    FLUTTER_FIRST_LINE="$(echo "$FLUTTER_OUTPUT" | head -n1)"
    expect_prefix "Flutter" "$FLUTTER_FIRST_LINE" "$FLUTTER_EXPECTED"
  fi
fi

JAVA_EXPECTED="${JAVA_VERSION:-}"
if [[ -n "$JAVA_EXPECTED" ]] && command -v java &> /dev/null; then
  # Capture full output to avoid broken pipe on Windows
  JAVA_OUTPUT="$(java -version 2>&1 || true)"
  JAVA_FIRST_LINE="$(echo "$JAVA_OUTPUT" | head -n1)"
  expect_prefix "Java" "$JAVA_FIRST_LINE" "$JAVA_EXPECTED"
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
