"use strict";

const fs = require("fs");
const path = require("path");

const packageRoot = path.resolve(__dirname, "..");
const expectedName = "react-native-pirate-wallet-ios-simulator-x86_64";
const expectedArchitecture = "x86_64";
const expectedFiles = [
  "ios/Headers/module.modulemap",
  "ios/Headers/pirate_wallet_service.h",
  "ios/libpirate_ffi_native.a",
];

function fail(message) {
  console.error(`[${expectedName}] ${message}`);
  process.exitCode = 1;
}

function requireFile(relativePath) {
  const stat = fs.statSync(path.join(packageRoot, relativePath), {
    throwIfNoEntry: false,
  });
  if (!stat?.isFile()) {
    fail(`Required package file is missing: ${relativePath}`);
  } else if (stat.size === 0) {
    fail(`Required package file is empty: ${relativePath}`);
  }
}

const packageJson = JSON.parse(
  fs.readFileSync(path.join(packageRoot, "package.json"), "utf8")
);
if (packageJson.name !== expectedName) {
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
if (packageJson.os?.length !== 1 || packageJson.os[0] !== "darwin") {
  fail("The iOS package must only install on macOS hosts");
}
if (
  packageJson.pirateWalletNative?.platform !== "ios-simulator" ||
  JSON.stringify(packageJson.pirateWalletNative?.architectures) !==
    JSON.stringify([expectedArchitecture])
) {
  fail(
    `Native metadata must identify only ${expectedArchitecture} iOS simulator`
  );
}

["LICENSE-MIT", "README.md", "package.json", ...expectedFiles].forEach(
  requireFile
);
for (const unexpected of [
  "android",
  "example",
  "node_modules",
  "ios/Frameworks",
]) {
  if (fs.existsSync(path.join(packageRoot, unexpected))) {
    fail(`Unexpected path in iOS package: ${unexpected}`);
  }
}

const iosRoot = path.join(packageRoot, "ios");
if (fs.statSync(iosRoot, { throwIfNoEntry: false })?.isDirectory()) {
  const actualFiles = [];
  function visit(directory) {
    for (const entry of fs.readdirSync(directory, { withFileTypes: true })) {
      const absolutePath = path.join(directory, entry.name);
      if (entry.isDirectory()) {
        visit(absolutePath);
      } else if (entry.isFile()) {
        actualFiles.push(path.relative(packageRoot, absolutePath));
      } else {
        fail(`Unsupported entry in iOS package: ${absolutePath}`);
      }
    }
  }
  visit(iosRoot);
  const allowed = new Set(expectedFiles.map((file) => path.normalize(file)));
  for (const actualFile of actualFiles) {
    if (!allowed.has(path.normalize(actualFile))) {
      fail(`Unexpected file in iOS package: ${actualFile}`);
    }
  }
}

if (process.exitCode) {
  process.exit(process.exitCode);
}
