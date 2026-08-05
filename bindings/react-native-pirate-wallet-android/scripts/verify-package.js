'use strict';

const fs = require('fs');
const path = require('path');

const packageRoot = path.resolve(__dirname, '..');

function fail(message) {
  console.error(`[react-native-pirate-wallet-android] ${message}`);
  process.exitCode = 1;
}

function requireFile(relativePath) {
  const absolutePath = path.join(packageRoot, relativePath);
  const stat = fs.statSync(absolutePath, {throwIfNoEntry: false});
  if (!stat?.isFile()) {
    fail(`Required package file is missing: ${relativePath}`);
    return;
  }
  if (stat.size === 0) {
    fail(`Required package file is empty: ${relativePath}`);
  }
}

function rejectPath(relativePath) {
  if (fs.existsSync(path.join(packageRoot, relativePath))) {
    fail(`Unexpected path in Android package: ${relativePath}`);
  }
}

const packageJson = JSON.parse(
  fs.readFileSync(path.join(packageRoot, 'package.json'), 'utf8'),
);

if (packageJson.name !== 'react-native-pirate-wallet-android') {
  fail(`Unexpected package name: ${packageJson.name}`);
}
if (!/^\d+\.\d+\.\d+(?:[-+][0-9A-Za-z.-]+)?$/.test(packageJson.version)) {
  fail(`Package version is not valid semantic versioning: ${packageJson.version}`);
}
if (packageJson.private === true) {
  fail('The publishable package must not be marked private');
}
if (
  packageJson.repository?.url !==
  'https://github.com/PirateNetwork/Pirate-Unified-Light-Wallet.git'
) {
  fail('The repository URL must match the GitHub repository used for npm provenance');
}
if (packageJson.publishConfig?.access !== 'public') {
  fail('publishConfig.access must remain public');
}

['LICENSE-MIT', 'README.md', 'package.json'].forEach(requireFile);
['ios', 'example', 'node_modules'].forEach(rejectPath);

for (const abi of ['arm64-v8a', 'armeabi-v7a', 'x86_64']) {
  requireFile(`android/src/main/jniLibs/${abi}/libpirate_ffi_native.so`);
}

if (process.exitCode) {
  process.exit(process.exitCode);
}
