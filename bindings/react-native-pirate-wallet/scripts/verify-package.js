'use strict';

const fs = require('fs');
const path = require('path');

const packageRoot = path.resolve(__dirname, '..');

function fail(message) {
  console.error(`[react-native-pirate-wallet] ${message}`);
  process.exitCode = 1;
}

function requireFile(relativePath) {
  const absolutePath = path.join(packageRoot, relativePath);
  if (!fs.statSync(absolutePath, {throwIfNoEntry: false})?.isFile()) {
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
  if (!fs.statSync(directory, {throwIfNoEntry: false})?.isDirectory()) {
    return [];
  }

  return fs.readdirSync(directory, {withFileTypes: true}).flatMap(entry => {
    const entryPath = path.join(directory, entry.name);
    return entry.isDirectory() ? collectFiles(entryPath) : [entryPath];
  });
}

const packageJson = JSON.parse(
  fs.readFileSync(path.join(packageRoot, 'package.json'), 'utf8'),
);

if (packageJson.name !== 'react-native-pirate-wallet') {
  fail(`Unexpected package name: ${packageJson.name}`);
}
if (!/^\d+\.\d+\.\d+(?:[-+][0-9A-Za-z.-]+)?$/.test(packageJson.version)) {
  fail(`Package version is not valid semantic versioning: ${packageJson.version}`);
}
if (packageJson.private === true) {
  fail('The publishable package must not be marked private');
}
if (packageJson.repository?.url !== 'https://github.com/PirateNetwork/Pirate-Unified-Light-Wallet.git') {
  fail('The repository URL must match the GitHub repository used for npm provenance');
}
if (packageJson.publishConfig?.access !== 'public') {
  fail('publishConfig.access must remain public');
}

[
  'LICENSE-MIT',
  'README.md',
  'react-native.config.js',
  'react-native-pirate-wallet.podspec',
  'scripts/resolve-android-package.js',
  'test/smoke.js',
  'src/index.js',
  'src/index.d.ts',
  'android/src/main/AndroidManifest.xml',
  'android/src/main/java/com/pirate/wallet/reactnative/PirateWalletReactNativeModule.kt',
  'ios/PirateWalletReactNative.m',
  'ios/PirateWalletReactNative.swift',
  'ios/Frameworks/PirateWalletNative.xcframework/Info.plist',
].forEach(requireFile);

if (process.argv.includes('--publish-layout')) {
  [
    'android/.gradle',
    'android/build',
    'android/src/main/jniLibs',
  ].forEach(rejectPath);
}

const androidPackageName = 'react-native-pirate-wallet-android';
if (
  packageJson.optionalDependencies?.[androidPackageName] !== packageJson.version
) {
  fail(`${androidPackageName} must use the same exact version as the wrapper`);
}

const staticLibraries = collectFiles(
  path.join(packageRoot, 'ios', 'Frameworks', 'PirateWalletNative.xcframework'),
).filter(file => file.endsWith('.a'));
if (staticLibraries.length < 2) {
  fail('The iOS XCFramework must contain device and simulator static libraries');
}
for (const library of staticLibraries) {
  if (fs.statSync(library).size === 0) {
    fail(`The iOS static library is empty: ${path.relative(packageRoot, library)}`);
  }
}

try {
  const {resolveAndroidJniLibsPath} = require('./resolve-android-package');
  resolveAndroidJniLibsPath();
} catch (error) {
  fail(error.message);
}

if (process.exitCode) {
  process.exit(process.exitCode);
}
