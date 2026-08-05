'use strict';

const fs = require('fs');
const path = require('path');

const androidPackageName = 'react-native-pirate-wallet-android';
const packageRoot = path.resolve(__dirname, '..');

function candidatePackageJsonPaths() {
  const candidates = [];
  try {
    candidates.push(
      require.resolve(`${androidPackageName}/package.json`, {
        paths: [process.cwd(), packageRoot],
      }),
    );
  } catch (_) {
    // The monorepo package is resolved below before it has been published.
  }
  candidates.push(
    path.resolve(packageRoot, '..', androidPackageName, 'package.json'),
  );
  return [...new Set(candidates)];
}

function resolveAndroidJniLibsPath() {
  const wrapperPackage = JSON.parse(
    fs.readFileSync(path.join(packageRoot, 'package.json'), 'utf8'),
  );
  const expectedVersion =
    wrapperPackage.optionalDependencies?.[androidPackageName];

  for (const packageJsonPath of candidatePackageJsonPaths()) {
    if (!fs.statSync(packageJsonPath, {throwIfNoEntry: false})?.isFile()) {
      continue;
    }

    const androidPackage = JSON.parse(fs.readFileSync(packageJsonPath, 'utf8'));
    if (androidPackage.name !== androidPackageName) {
      continue;
    }
    if (androidPackage.version !== expectedVersion) {
      throw new Error(
        `${androidPackageName}@${androidPackage.version} does not match ` +
          `react-native-pirate-wallet@${wrapperPackage.version}`,
      );
    }

    const jniLibsPath = path.join(
      path.dirname(packageJsonPath),
      'android',
      'src',
      'main',
      'jniLibs',
    );
    if (fs.statSync(jniLibsPath, {throwIfNoEntry: false})?.isDirectory()) {
      return jniLibsPath;
    }
  }

  throw new Error(
    `${androidPackageName}@${expectedVersion} is required to build ` +
      'react-native-pirate-wallet for Android',
  );
}

module.exports = {resolveAndroidJniLibsPath};
