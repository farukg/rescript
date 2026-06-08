#!/usr/bin/env node
/**
 * Postinstall script for patched rescript fork.
 * Downloads the pre-built rewatch binary from GitHub Releases if available,
 * otherwise falls back to building from source (requires Rust toolchain).
 */

const fs = require("fs");
const path = require("path");
const https = require("https");
const { execSync } = require("child_process");

const REPO = "farukg/rescript";
const RELEASE_TAG = "patched";

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
  const binPath = path.join(binDir, "rescript.exe");
  return fs.existsSync(binPath);
}

function downloadFile(url, dest) {
  return new Promise((resolve, reject) => {
    const file = fs.createWriteStream(dest);
    https
      .get(url, { followRedirect: true }, (response) => {
        if (response.statusCode === 302 || response.statusCode === 301) {
          downloadFile(response.headers.location, dest)
            .then(resolve)
            .catch(reject);
          return;
        }
        if (response.statusCode !== 200) {
          reject(
            new Error(
              `Download failed with status ${response.statusCode}: ${url}`
            )
          );
          return;
        }
        response.pipe(file);
        file.on("finish", () => {
          file.close(resolve);
        });
      })
      .on("error", (err) => {
        fs.unlink(dest, () => {});
        reject(err);
      });
  });
}

async function downloadBinary() {
  const platformKey = getPlatformKey();
  if (!platformKey) {
    console.log(
      `[rescript-patched] No prebuilt binary for this platform. Trying source build...`
    );
    return false;
  }

  const binDir = getBinDir();
  const binPath = path.join(binDir, "rescript.exe");
  const url = `https://github.com/${REPO}/releases/download/${RELEASE_TAG}/rescript-${platformKey}.exe`;

  console.log(
    `[rescript-patched] Downloading rewatch binary for ${platformKey}...`
  );

  try {
    fs.mkdirSync(binDir, { recursive: true });
    await downloadFile(url, binPath);
    fs.chmodSync(binPath, 0o755);
    console.log(`[rescript-patched] Binary installed to ${binPath}`);
    return true;
  } catch (err) {
    console.warn(
      `[rescript-patched] Download failed: ${err.message}. Trying source build...`
    );
    return false;
  }
}

function buildFromSource() {
  console.log(
    `[rescript-patched] Building rewatch from source (requires Rust toolchain)...`
  );
  try {
    const rootDir = path.join(__dirname, "..");
    execSync("make rewatch", { cwd: rootDir, stdio: "inherit" });
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
  if (binaryExists()) {
    console.log(`[rescript-patched] Binary already exists, skipping.`);
    return;
  }

  const downloaded = await downloadBinary();
  if (downloaded) return;

  const built = buildFromSource();
  if (built) return;

  console.error(
    `[rescript-patched] FAILED: Could not install rewatch binary.\n` +
      `  - Prebuilt binary not available for your platform\n` +
      `  - Source build failed (is Rust installed?)\n` +
      `  - Manual fix: build with "make rewatch" in the repo root`
  );
  process.exit(1);
}

main().catch((err) => {
  console.error(`[rescript-patched] Unexpected error: ${err.message}`);
  process.exit(1);
});
