// @ts-check

import * as fs from "node:fs";
import * as path from "node:path";

const minimumNodeVersion = "20.11.0";

/**
 * @typedef {import("@rescript/linux-x64")} BinaryModuleExports
 */

const target = `${process.platform}-${process.arch}`;

const supportedPlatforms = [
  "darwin-arm64",
  "darwin-x64",
  "linux-arm64",
  "linux-x64",
  "win32-x64",
];

/** @type {BinaryModuleExports} */
let upstream;

if (supportedPlatforms.includes(target)) {
  const binPackageName = `@rescript/${target}`;

  try {
    upstream = await import(binPackageName);
  } catch {
    // First check if we are on an unsupported node version, as that may be the cause for the error.
    checkNodeVersionSupported();

    throw new Error(
      `Package ${binPackageName} not found. Make sure the rescript package is installed correctly.`,
    );
  }
} else {
  throw new Error(`Platform ${target} is not supported!`);
}

const patchedBinDir = path.join(
  import.meta.dirname,
  "..",
  "..",
  "packages",
  "@rescript",
  target,
  "bin",
);

const patchedBinPaths = {
  bsb_helper_exe: path.join(patchedBinDir, "bsb_helper.exe"),
  bsc_exe: path.join(patchedBinDir, "bsc.exe"),
  ninja_exe: path.join(patchedBinDir, "ninja.exe"),
  rescript_editor_analysis_exe: path.join(
    patchedBinDir,
    "rescript-editor-analysis.exe",
  ),
  rescript_tools_exe: path.join(patchedBinDir, "rescript-tools.exe"),
  rescript_legacy_exe: path.join(patchedBinDir, "rescript-legacy.exe"),
  rescript_exe: path.join(patchedBinDir, "rescript.exe"),
};

const patchedToolchainComplete = Object.values(patchedBinPaths).every((file) =>
  fs.existsSync(file),
);

export const binDir = patchedToolchainComplete
  ? patchedBinDir
  : upstream.binDir;
export const binPaths = patchedToolchainComplete
  ? patchedBinPaths
  : upstream.binPaths;

export const {
  bsb_helper_exe,
  bsc_exe,
  ninja_exe,
  rescript_editor_analysis_exe,
  rescript_tools_exe,
  rescript_legacy_exe,
  rescript_exe,
} = binPaths;

function checkNodeVersionSupported() {
  if (
    typeof process !== "undefined" &&
    process.versions != null &&
    process.versions.node != null
  ) {
    const currentVersion = process.versions.node;
    const required = minimumNodeVersion.split(".").map(Number);
    const current = currentVersion.split(".").map(Number);
    if (
      current[0] < required[0] ||
      (current[0] === required[0] && current[1] < required[1]) ||
      (current[0] === required[0] &&
        current[1] === required[1] &&
        current[2] < required[2])
    ) {
      throw new Error(
        `ReScript requires Node.js >=${minimumNodeVersion}, but found ${currentVersion}.`,
      );
    }
  }
}
