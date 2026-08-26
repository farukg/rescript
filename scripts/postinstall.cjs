#!/usr/bin/env node
const fs = require("fs");
const path = require("path");
const { execSync } = require("child_process");

const REPO = "farukg/rescript";
const RELEASE_TAG = "patched";
const BINARIES = [
  "bsb_helper",
  "bsc",
  "ninja",
  "rescript-editor-analysis",
  "rescript-tools",
  "rescript-legacy",
  "rescript",
];

function isSourceCheckout() {
  return fs.existsSync(path.join(__dirname, "..", ".git"));
}

function getPlatformKey() {
  const platform = process.platform;
  const arch = process.arch;
  const mapping = {
    "linux-x64": "linux-x64",
    "linux-arm64": "linux-arm64",
    "darwin-arm64": "darwin-arm64",
    "darwin-x64": "darwin-x64",
    "win32-x64": "win32-x64",
  };
  const key = mapping[`${platform}-${arch}`];
  if (!key) {
    console.warn(
      `[rescript-patched] Unsupported platform ${platform}-${arch}. ` +
        `Supported: linux-x64, linux-arm64, darwin-arm64, darwin-x64, win32-x64.`
    );
    return null;
  }
  return key;
}

function getBinDir() {
  const platformKey = getPlatformKey();
  if (!platformKey) return null;
  return path.join(
    __dirname,
    "..",
    "packages",
    "@rescript",
    platformKey,
    "bin"
  );
}

function binaryExists() {
  const binDir = getBinDir();
  if (!binDir) return false;
  return BINARIES.every((binary) =>
    fs.existsSync(path.join(binDir, `${binary}.exe`))
  );
}

async function downloadFile(url, dest) {
  const response = await fetch(url, { redirect: "follow" });
  if (!response.ok) {
    throw new Error(`Download failed with status ${response.status}: ${url}`);
  }
  fs.writeFileSync(dest, Buffer.from(await response.arrayBuffer()));
}

async function downloadToolchain() {
  const platformKey = getPlatformKey();
  if (!platformKey) {
    console.log(
      `[rescript-patched] No prebuilt toolchain for this platform. Trying source build...`
    );
    return false;
  }

  const binDir = getBinDir();
  const stagingDir = path.join(binDir, `.install-${process.pid}`);

  console.log(
    `[rescript-patched] Downloading toolchain for ${platformKey}...`
  );

  try {
    fs.mkdirSync(binDir, { recursive: true });
    fs.mkdirSync(stagingDir);
    for (const binary of BINARIES) {
      const filename = `${binary}.exe`;
      const url = `https://github.com/${REPO}/releases/download/${RELEASE_TAG}/${binary}-${platformKey}.exe`;
      const staged = path.join(stagingDir, filename);
      await downloadFile(url, staged);
      fs.chmodSync(staged, 0o755);
    }
    for (const binary of BINARIES) {
      const filename = `${binary}.exe`;
      const destination = path.join(binDir, filename);
      if (fs.existsSync(destination)) {
        fs.unlinkSync(destination);
      }
      fs.renameSync(path.join(stagingDir, filename), destination);
    }
    fs.rmdirSync(stagingDir);
    console.log(`[rescript-patched] Toolchain installed to ${binDir}`);
    return true;
  } catch (err) {
    fs.rmSync(stagingDir, { recursive: true, force: true });
    console.warn(
      `[rescript-patched] Download failed: ${err.message}. Trying source build...`
    );
    return false;
  }
}

function buildFromSource() {
  console.log(
    `[rescript-patched] Building toolchain from source...`
  );
  try {
    const rootDir = path.join(__dirname, "..");
    execSync("make", { cwd: rootDir, stdio: "inherit" });
    console.log(`[rescript-patched] Source build completed.`);
    return true;
  } catch (err) {
    console.error(
      `[rescript-patched] Source build failed: ${err.message}`
    );
    return false;
  }
}

async function main() {
  if (isSourceCheckout()) {
    console.log(`[rescript-patched] Source checkout detected, skipping install.`);
    return;
  }

  if (binaryExists()) {
    console.log(`[rescript-patched] Toolchain already exists, skipping.`);
    return;
  }

  const downloaded = await downloadToolchain();
  if (downloaded) return;

  const built = buildFromSource();
  if (built) return;

  console.error(
    `[rescript-patched] FAILED: Could not install the toolchain.\n` +
      `  - Prebuilt toolchain not available for your platform\n` +
      `  - Source build failed\n` +
      `  - Manual fix: build with "make" in the repo root`
  );
  process.exit(1);
}

main().catch((err) => {
  console.error(`[rescript-patched] Unexpected error: ${err.message}`);
  process.exit(1);
});
