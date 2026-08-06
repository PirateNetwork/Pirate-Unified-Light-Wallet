'use strict';

const fs = require('fs');
const path = require('path');

const androidPackageNames = [
  'react-native-pirate-wallet-android',
  'react-native-pirate-wallet-android-x86_64',
];
const packageRoot = path.resolve(__dirname, '..');

function candidatePackageJsonPaths(packageName) {
  const candidates = [];
  try {
    candidates.push(
      require.resolve(`${packageName}/package.json`, {
        paths: [process.cwd(), packageRoot],
      }),
    );
  } catch (_) {
    // The monorepo package is resolved below before it has been published.
  }
  candidates.push(
    path.resolve(packageRoot, '..', packageName, 'package.json'),
  );
  return [...new Set(candidates)];
}

function resolveAndroidJniLibsPaths() {
  const wrapperPackage = JSON.parse(
    fs.readFileSync(path.join(packageRoot, 'package.json'), 'utf8'),
  );

  return androidPackageNames.map(packageName => {
    const expectedVersion =
      wrapperPackage.optionalDependencies?.[packageName];

    for (const packageJsonPath of candidatePackageJsonPaths(packageName)) {
      if (!fs.statSync(packageJsonPath, {throwIfNoEntry: false})?.isFile()) {
        continue;
      }

      const androidPackage = JSON.parse(
        fs.readFileSync(packageJsonPath, 'utf8'),
      );
      if (androidPackage.name !== packageName) {
        continue;
      }
      if (androidPackage.version !== expectedVersion) {
        throw new Error(
          `${packageName}@${androidPackage.version} does not match ` +
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
      `${packageName}@${expectedVersion} is required to build ` +
        'react-native-pirate-wallet for Android',
    );
  });
}

module.exports = {resolveAndroidJniLibsPaths};
