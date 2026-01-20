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
  expect_prefix "Flutter" "$(flutter --version | head -n1)" "$FLUTTER_EXPECTED"
fi

JAVA_EXPECTED="${JAVA_VERSION:-}"
if [[ -n "$JAVA_EXPECTED" ]] && command -v java &> /dev/null; then
  expect_prefix "Java" "$(java -version 2>&1 | head -n1)" "$JAVA_EXPECTED"
fi

GRADLE_EXPECTED="${GRADLE_VERSION:-}"
if [[ -z "$GRADLE_EXPECTED" && -f "$PROJECT_ROOT/app/android/gradle/wrapper/gradle-wrapper.properties" ]]; then
  GRADLE_EXPECTED="$(awk -F 'gradle-' '/distributionUrl/ {print $2}' "$PROJECT_ROOT/app/android/gradle/wrapper/gradle-wrapper.properties" | awk -F '-' '{print $1}')"
fi
if [[ -n "$GRADLE_EXPECTED" && -x "$PROJECT_ROOT/app/android/gradlew" ]]; then
  expect_prefix "Gradle" "$("$PROJECT_ROOT/app/android/gradlew" --version | head -n3 | tr '\n' ' ')" "$GRADLE_EXPECTED"
fi

COCOAPODS_EXPECTED="${COCOAPODS_VERSION:-}"
if [[ -n "$COCOAPODS_EXPECTED" && "$(uname -s)" == "Darwin" ]]; then
  if ! command -v pod &> /dev/null; then
    fail "CocoaPods not found"
  fi
  expect_prefix "CocoaPods" "$(pod --version)" "$COCOAPODS_EXPECTED"
fi

echo "[INFO] Toolchain versions match pinned expectations."
