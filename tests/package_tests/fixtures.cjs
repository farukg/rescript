const fs = require("node:fs");
const os = require("node:os");
const path = require("node:path");
const { execFileSync } = require("node:child_process");

const sourceRoot = path.resolve(__dirname, "../..");

function fixture(t) {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), "rescript-package-test-"));
  t.after(() => fs.rmSync(root, { recursive: true, force: true }));
  return root;
}

function copyPackage(root) {
  for (const name of [
    "scripts/postinstall.cjs",
    "scripts/sourceIdentity.cjs",
    "scripts/preparePackage.cjs",
    "packages/@rescript/source-sha.txt",
    ".gitattributes",
    ".gitignore",
  ]) {
    const dest = path.join(root, name);
    fs.mkdirSync(path.dirname(dest), { recursive: true });
    fs.copyFileSync(path.join(sourceRoot, name), dest);
  }
  const manifest = JSON.parse(
    fs.readFileSync(path.join(sourceRoot, "package.json"), "utf8"),
  );
  // Isolate package lifecycles from registry dependencies and compiler binaries.
  manifest.dependencies = {};
  manifest.optionalDependencies = {};
  manifest.devDependencies = {};
  manifest.workspaces = [];
  manifest.packageManager = undefined;
  fs.writeFileSync(path.join(root, "package.json"), JSON.stringify(manifest));
}

function git(root, ...args) {
  return execFileSync("git", ["-C", root, ...args], {
    encoding: "utf8",
  }).trim();
}

function commitPackage(root) {
  git(root, "init", "--quiet");
  git(root, "add", ".");
  git(
    root,
    "-c",
    "user.name=Package Test",
    "-c",
    "user.email=package-test@example.invalid",
    "-c",
    "commit.gpgsign=false",
    "commit",
    "--quiet",
    "-m",
    "Package fixture",
  );
  return git(root, "rev-parse", "HEAD");
}

function npm(root, ...args) {
  const options = { cwd: root, encoding: "utf8", stdio: "pipe" };
  if (process.env.npm_execpath) {
    return execFileSync(
      process.execPath,
      [process.env.npm_execpath, ...args],
      options,
    );
  }
  return execFileSync("npm", args, options);
}

module.exports = { fixture, copyPackage, git, commitPackage, npm, sourceRoot };
