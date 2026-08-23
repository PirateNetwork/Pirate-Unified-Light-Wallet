"use strict";

const childProcess = require("child_process");
const fs = require("fs");
const path = require("path");

const packageRoot = path.resolve(__dirname, "..");
const frameworkName = "PirateWalletNative.xcframework";
const devicePackage = {
  name: "react-native-pirate-wallet-ios-device",
  slice: "ios-arm64",
};
const simulatorPackages = [
  {
    name: "react-native-pirate-wallet-ios-simulator-arm64",
    architecture: "arm64",
  },
  {
    name: "react-native-pirate-wallet-ios-simulator-x86_64",
    architecture: "x86_64",
  },
];

function candidatePackageJsonPaths(packageName) {
  const candidates = [];
  try {
    candidates.push(
      require.resolve(`${packageName}/package.json`, {
        paths: [process.cwd(), packageRoot],
      })
    );
  } catch (_) {
    // The monorepo package is resolved below before it has been published.
  }
  candidates.push(path.resolve(packageRoot, "..", packageName, "package.json"));
  return [...new Set(candidates)];
}

function resolvePackage(packageName, expectedVersion) {
  for (const packageJsonPath of candidatePackageJsonPaths(packageName)) {
    if (!fs.statSync(packageJsonPath, { throwIfNoEntry: false })?.isFile()) {
      continue;
    }
    const packageJson = JSON.parse(fs.readFileSync(packageJsonPath, "utf8"));
    if (packageJson.name !== packageName) {
      continue;
    }
    if (packageJson.version !== expectedVersion) {
      throw new Error(
        `${packageName}@${packageJson.version} does not match ` +
          `react-native-pirate-wallet@${expectedVersion}`
      );
    }
    return { root: path.dirname(packageJsonPath), packageJson };
  }
  throw new Error(
    `${packageName}@${expectedVersion} is required to build ` +
      "react-native-pirate-wallet for iOS"
  );
}

function linkTree(source, destination) {
  fs.mkdirSync(destination, { recursive: true });
  for (const entry of fs.readdirSync(source, { withFileTypes: true })) {
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
      if (!["EXDEV", "EPERM", "EACCES", "EMLINK"].includes(error.code)) {
        throw error;
      }
      fs.copyFileSync(sourcePath, destinationPath);
    }
  }
}

function runXcrun(args) {
  const result = childProcess.spawnSync("xcrun", args, { encoding: "utf8" });
  if (result.error) {
    throw result.error;
  }
  if (result.status !== 0) {
    throw new Error(
      `xcrun ${args.join(" ")} failed:\n${result.stderr || result.stdout}`
    );
  }
  return result.stdout.trim();
}

function verifyArchitectures(archive, expected) {
  const actual = runXcrun(["lipo", "-archs", archive]).split(/\s+/).sort();
  const wanted = [...expected].sort();
  if (JSON.stringify(actual) !== JSON.stringify(wanted)) {
    throw new Error(
      `${archive} has architectures ${actual.join(
        ", "
      )}; expected ${wanted.join(", ")}`
    );
  }
}

function assertMatchingFile(left, right, label) {
  const leftData = fs.readFileSync(left);
  const rightData = fs.readFileSync(right);
  if (!leftData.equals(rightData)) {
    throw new Error(`iOS binary packages contain different ${label}`);
  }
}

function assemble() {
  if (process.platform !== "darwin") {
    if (process.argv.includes("--force")) {
      throw new Error("iOS XCFramework assembly requires macOS and xcrun lipo");
    }
    return;
  }

  const wrapperPackage = JSON.parse(
    fs.readFileSync(path.join(packageRoot, "package.json"), "utf8")
  );
  function resolveExact(descriptor) {
    const expectedVersion =
      wrapperPackage.optionalDependencies?.[descriptor.name];
    if (expectedVersion !== wrapperPackage.version) {
      throw new Error(`${descriptor.name} must use the exact wrapper version`);
    }
    return {
      ...descriptor,
      ...resolvePackage(descriptor.name, expectedVersion),
    };
  }

  const device = resolveExact(devicePackage);
  const simulators = simulatorPackages.map(resolveExact);
  const deviceFramework = path.join(
    device.root,
    "ios",
    "Frameworks",
    frameworkName
  );
  const deviceSlice = path.join(deviceFramework, device.slice);
  if (!fs.statSync(deviceSlice, { throwIfNoEntry: false })?.isDirectory()) {
    throw new Error(
      `${device.name} is missing XCFramework slice ${device.slice}`
    );
  }

  const simulatorInputs = simulators.map((simulator) => {
    const metadata = simulator.packageJson.pirateWalletNative;
    if (
      metadata?.platform !== "ios-simulator" ||
      JSON.stringify(metadata.architectures) !==
        JSON.stringify([simulator.architecture])
    ) {
      throw new Error(
        `${simulator.name} does not identify its expected simulator architecture`
      );
    }
    const archive = path.join(simulator.root, "ios", "libpirate_ffi_native.a");
    const headers = path.join(simulator.root, "ios", "Headers");
    verifyArchitectures(archive, [simulator.architecture]);
    return { ...simulator, archive, headers };
  });

  const canonicalHeaders = path.join(deviceSlice, "Headers");
  for (const simulator of simulatorInputs) {
    for (const header of ["module.modulemap", "pirate_wallet_service.h"]) {
      assertMatchingFile(
        path.join(canonicalHeaders, header),
        path.join(simulator.headers, header),
        header
      );
    }
  }

  const frameworksRoot = path.join(packageRoot, "ios", "Frameworks");
  fs.mkdirSync(frameworksRoot, { recursive: true });
  const temporaryRoot = fs.mkdtempSync(
    path.join(frameworksRoot, `${frameworkName}.tmp-`)
  );
  const output = path.join(frameworksRoot, frameworkName);
  try {
    fs.copyFileSync(
      path.join(deviceFramework, "Info.plist"),
      path.join(temporaryRoot, "Info.plist")
    );
    linkTree(deviceSlice, path.join(temporaryRoot, device.slice));

    const simulatorSlice = path.join(
      temporaryRoot,
      "ios-arm64_x86_64-simulator"
    );
    linkTree(canonicalHeaders, path.join(simulatorSlice, "Headers"));
    const universalArchive = path.join(
      simulatorSlice,
      "libpirate_ffi_native.a"
    );
    runXcrun([
      "lipo",
      "-create",
      ...simulatorInputs.map((simulator) => simulator.archive),
      "-output",
      universalArchive,
    ]);
    verifyArchitectures(universalArchive, ["arm64", "x86_64"]);

    fs.rmSync(output, { recursive: true, force: true });
    fs.renameSync(temporaryRoot, output);
  } finally {
    fs.rmSync(temporaryRoot, { recursive: true, force: true });
  }
}

assemble();
