"use strict";

const fs = require("fs");
const path = require("path");

const packageRoot = path.resolve(__dirname, "..");

function fail(message) {
  console.error(`[react-native-pirate-wallet] ${message}`);
  process.exitCode = 1;
}

function requireFile(relativePath) {
  const absolutePath = path.join(packageRoot, relativePath);
  if (!fs.statSync(absolutePath, { throwIfNoEntry: false })?.isFile()) {
    fail(`Required package file is missing: ${relativePath}`);
    return;
  }
  if (fs.statSync(absolutePath).size === 0) {
    fail(`Required package file is empty: ${relativePath}`);
  }
}

function rejectPath(relativePath) {
  if (fs.existsSync(path.join(packageRoot, relativePath))) {
    fail(`Generated build path must not be published: ${relativePath}`);
  }
}

function collectFiles(directory) {
  if (!fs.statSync(directory, { throwIfNoEntry: false })?.isDirectory()) {
    return [];
  }

  return fs.readdirSync(directory, { withFileTypes: true }).flatMap((entry) => {
    const entryPath = path.join(directory, entry.name);
    return entry.isDirectory() ? collectFiles(entryPath) : [entryPath];
  });
}

const packageJson = JSON.parse(
  fs.readFileSync(path.join(packageRoot, "package.json"), "utf8")
);

if (packageJson.name !== "react-native-pirate-wallet") {
  fail(`Unexpected package name: ${packageJson.name}`);
}
if (!/^\d+\.\d+\.\d+(?:[-+][0-9A-Za-z.-]+)?$/.test(packageJson.version)) {
  fail(
    `Package version is not valid semantic versioning: ${packageJson.version}`
  );
}
if (packageJson.private === true) {
  fail("The publishable package must not be marked private");
}
if (
  packageJson.repository?.url !==
  "https://github.com/PirateNetwork/Pirate-Unified-Light-Wallet.git"
) {
  fail(
    "The repository URL must match the GitHub repository used for npm provenance"
  );
}
if (packageJson.publishConfig?.access !== "public") {
  fail("publishConfig.access must remain public");
}

[
  "LICENSE-MIT",
  "README.md",
  "react-native.config.js",
  "react-native-pirate-wallet.podspec",
  "scripts/assemble-ios-framework.js",
  "scripts/resolve-android-packages.js",
  "test/smoke.js",
  "src/index.js",
  "src/index.d.ts",
  "android/src/main/AndroidManifest.xml",
  "android/src/main/java/com/pirate/wallet/reactnative/PirateWalletReactNativeModule.kt",
  "ios/PirateWalletReactNative.m",
  "ios/PirateWalletReactNative.swift",
].forEach(requireFile);

if (process.argv.includes("--publish-layout")) {
  [
    "android/.gradle",
    "android/build",
    "android/src/main/jniLibs",
    "ios/Frameworks/PirateWalletNative.xcframework",
  ].forEach(rejectPath);
}

const binaryPackageNames = [
  "react-native-pirate-wallet-android",
  "react-native-pirate-wallet-android-x86_64",
  "react-native-pirate-wallet-ios-device",
  "react-native-pirate-wallet-ios-simulator-arm64",
  "react-native-pirate-wallet-ios-simulator-x86_64",
];
for (const binaryPackageName of binaryPackageNames) {
  if (
    packageJson.optionalDependencies?.[binaryPackageName] !==
    packageJson.version
  ) {
    fail(`${binaryPackageName} must use the same exact version as the wrapper`);
  }
}

if (
  !process.argv.includes("--publish-layout") &&
  (process.platform === "darwin" || process.argv.includes("--all-platforms"))
) {
  const staticLibraries = collectFiles(
    path.join(
      packageRoot,
      "ios",
      "Frameworks",
      "PirateWalletNative.xcframework"
    )
  ).filter((file) => file.endsWith(".a"));
  if (staticLibraries.length !== 2) {
    fail(
      "The iOS XCFramework must contain device and simulator static libraries"
    );
  }
  for (const library of staticLibraries) {
    if (fs.statSync(library).size === 0) {
      fail(
        `The iOS static library is empty: ${path.relative(
          packageRoot,
          library
        )}`
      );
    }
  }
}

try {
  const { resolveAndroidJniLibsPaths } = require("./resolve-android-packages");
  if (resolveAndroidJniLibsPaths().length !== 2) {
    fail("Both Android binary packages must resolve");
  }
} catch (error) {
  fail(error.message);
}

if (process.exitCode) {
  process.exit(process.exitCode);
}
