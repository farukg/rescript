const fs = require("node:fs");
const path = require("node:path");
const { execFileSync } = require("node:child_process");

const packagedMarker = "packages/@rescript/packaged-source-sha.txt";
const archiveMarker = "packages/@rescript/source-sha.txt";
const fullSha = /^[0-9a-f]{40}$/i;

function readMarker(root, marker) {
  try {
    const sha = fs.readFileSync(path.join(root, marker), "utf8").trim();
    return fullSha.test(sha) ? sha.toLowerCase() : null;
  } catch (error) {
    if (error.code === "ENOENT") return null;
    throw error;
  }
}

function packagedSourceSha(root) {
  return readMarker(root, packagedMarker) || readMarker(root, archiveMarker);
}

function writePackageSourceSha(root) {
  let sha;
  if (fs.existsSync(path.join(root, ".git"))) {
    const git = args =>
      execFileSync("git", ["-C", root, ...args], { encoding: "utf8" }).trim();
    const top = git(["rev-parse", "--show-toplevel"]);
    if (fs.realpathSync(top) !== fs.realpathSync(root)) {
      throw new Error(
        "Package source identity requires its own repository root.",
      );
    }
    if (git(["status", "--porcelain", "--untracked-files=normal"])) {
      throw new Error("Commit package source changes before packing ReScript.");
    }
    sha = git(["rev-parse", "HEAD"]);
  } else {
    // Git archives substitute the committed identity without repository metadata.
    sha = readMarker(root, archiveMarker);
  }
  if (!sha || !fullSha.test(sha)) {
    throw new Error(
      "Cannot identify ReScript source. Pack a clean checkout or a Git source archive.",
    );
  }
  fs.writeFileSync(path.join(root, packagedMarker), `${sha.toLowerCase()}\n`);
}

module.exports = { packagedSourceSha, writePackageSourceSha };
