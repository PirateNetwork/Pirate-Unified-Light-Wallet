'use strict';

const fs = require('fs');
const path = require('path');

const packageRoot = path.resolve(__dirname, '..');
const expectedName = 'react-native-pirate-wallet-android-x86_64';

function fail(message) {
  console.error(`[${expectedName}] ${message}`);
  process.exitCode = 1;
}

function requireFile(relativePath) {
  const absolutePath = path.join(packageRoot, relativePath);
  const stat = fs.statSync(absolutePath, {throwIfNoEntry: false});
  if (!stat?.isFile()) {
    fail(`Required package file is missing: ${relativePath}`);
  } else if (stat.size === 0) {
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
if (packageJson.name !== expectedName) {
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

[
  'LICENSE-MIT',
  'README.md',
  'package.json',
  'android/src/main/jniLibs/x86_64/libpirate_ffi_native.so',
].forEach(requireFile);
[
  'ios',
  'example',
  'node_modules',
  'android/src/main/jniLibs/arm64-v8a',
  'android/src/main/jniLibs/armeabi-v7a',
].forEach(rejectPath);

if (process.exitCode) {
  process.exit(process.exitCode);
}
