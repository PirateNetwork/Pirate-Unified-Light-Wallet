'use strict';

const fs = require('fs');
const path = require('path');

const packageRoot = path.resolve(__dirname, '..');
const frameworkName = 'PirateWalletNative.xcframework';
const iosPackages = [
  {
    name: 'react-native-pirate-wallet-ios-device',
    slice: 'ios-arm64',
  },
  {
    name: 'react-native-pirate-wallet-ios-simulator',
    slice: 'ios-arm64_x86_64-simulator',
  },
];

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

function resolvePackage(packageName, expectedVersion) {
  for (const packageJsonPath of candidatePackageJsonPaths(packageName)) {
    if (!fs.statSync(packageJsonPath, {throwIfNoEntry: false})?.isFile()) {
      continue;
    }
    const packageJson = JSON.parse(fs.readFileSync(packageJsonPath, 'utf8'));
    if (packageJson.name !== packageName) {
      continue;
    }
    if (packageJson.version !== expectedVersion) {
      throw new Error(
        `${packageName}@${packageJson.version} does not match ` +
          `react-native-pirate-wallet@${expectedVersion}`,
      );
    }
    return path.dirname(packageJsonPath);
  }
  throw new Error(
    `${packageName}@${expectedVersion} is required to build ` +
      'react-native-pirate-wallet for iOS',
  );
}

function linkTree(source, destination) {
  fs.mkdirSync(destination, {recursive: true});
  for (const entry of fs.readdirSync(source, {withFileTypes: true})) {
    const sourcePath = path.join(source, entry.name);
    const destinationPath = path.join(destination, entry.name);
    if (entry.isDirectory()) {
      linkTree(sourcePath, destinationPath);
      continue;
    }
    if (!entry.isFile()) {
      throw new Error(`Unsupported iOS package entry: ${sourcePath}`);
    }
    try {
      fs.linkSync(sourcePath, destinationPath);
    } catch (error) {
      if (!['EXDEV', 'EPERM', 'EACCES', 'EMLINK'].includes(error.code)) {
        throw error;
      }
      fs.copyFileSync(sourcePath, destinationPath);
    }
  }
}

function assemble() {
  if (process.platform !== 'darwin' && !process.argv.includes('--force')) {
    return;
  }

  const wrapperPackage = JSON.parse(
    fs.readFileSync(path.join(packageRoot, 'package.json'), 'utf8'),
  );
  const resolved = iosPackages.map(({name, slice}) => {
    const expectedVersion = wrapperPackage.optionalDependencies?.[name];
    if (expectedVersion !== wrapperPackage.version) {
      throw new Error(`${name} must use the exact wrapper version`);
    }
    const root = resolvePackage(name, expectedVersion);
    return {
      name,
      slice,
      framework: path.join(root, 'ios', 'Frameworks', frameworkName),
    };
  });

  const infoPlists = resolved.map(({framework}) =>
    fs.readFileSync(path.join(framework, 'Info.plist')),
  );
  if (!infoPlists.slice(1).every(data => data.equals(infoPlists[0]))) {
    throw new Error('iOS binary packages contain different XCFramework metadata');
  }

  const output = path.join(packageRoot, 'ios', 'Frameworks', frameworkName);
  fs.rmSync(output, {recursive: true, force: true});
  fs.mkdirSync(output, {recursive: true});
  fs.writeFileSync(path.join(output, 'Info.plist'), infoPlists[0]);

  for (const {name, slice, framework} of resolved) {
    const source = path.join(framework, slice);
    if (!fs.statSync(source, {throwIfNoEntry: false})?.isDirectory()) {
      throw new Error(`${name} is missing XCFramework slice ${slice}`);
    }
    linkTree(source, path.join(output, slice));
  }
}

assemble();
