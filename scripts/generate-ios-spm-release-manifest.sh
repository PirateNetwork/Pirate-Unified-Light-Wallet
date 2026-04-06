#!/usr/bin/env bash
set -euo pipefail

if [[ $# -ne 4 ]]; then
  echo "usage: $0 <owner/repo> <tag> <artifact-url> <checksum>" >&2
  exit 1
fi

REPOSITORY="$1"
TAG_NAME="$2"
ARTIFACT_URL="$3"
CHECKSUM="$4"

cat <<EOF
// swift-tools-version: 5.9
import PackageDescription

let package = Package(
    name: "PirateWalletSDK",
    platforms: [
        .iOS(.v15),
    ],
    products: [
        .library(
            name: "PirateWalletSDK",
            targets: ["PirateWalletSDK"]
        ),
    ],
    targets: [
        .binaryTarget(
            name: "PirateWalletNative",
            url: "$ARTIFACT_URL",
            checksum: "$CHECKSUM"
        ),
        .target(
            name: "PirateWalletSDK",
            dependencies: ["PirateWalletNative"],
            path: "Sources/PirateWalletSDK"
        ),
    ]
)
EOF
