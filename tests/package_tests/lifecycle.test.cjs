const assert = require("node:assert/strict");
const fs = require("node:fs");
const path = require("node:path");
const { execFileSync } = require("node:child_process");
const { test } = require("node:test");
const { fixture, copyPackage, commitPackage, npm } = require("./fixtures.cjs");
const { writePackageSourceSha } = require("../../scripts/sourceIdentity.cjs");

function consumer(t, sha, spec) {
  const root = fixture(t);
  fs.writeFileSync(
    path.join(root, "package.json"),
    JSON.stringify({
      name: "package-consumer",
      private: true,
      dependencies: { rescript: spec },
      allowScripts: { rescript: true },
    }),
  );
  fs.writeFileSync(
    path.join(root, "pnpm-workspace.yaml"),
    "onlyBuiltDependencies:\n  - rescript\n",
  );
  const preload = path.join(root, "downloads.cjs");
  fs.writeFileSync(
    preload,
    `
    require("node:child_process").execSync = () => { throw new Error("Compiler builds forbidden in lifecycle tests"); };
    globalThis.fetch = async url => ({
      ok: true,
      arrayBuffer: async () => Buffer.from(url.endsWith("/source-sha.txt") ? "${sha}\\n" : "fixture binary")
    });
  `,
  );
  return {
    root,
    env: {
      ...process.env,
      NODE_OPTIONS: `--require=${JSON.stringify(preload)}`,
    },
  };
}

function assertToolchain(root, sha) {
  const bin = path.join(
    root,
    "node_modules/rescript/packages/@rescript",
    `${process.platform}-${process.arch}`,
    "bin",
  );
  assert.equal(
    fs.readFileSync(path.join(bin, "source-sha.txt"), "utf8"),
    `${sha}\n`,
  );
  assert.equal(fs.readdirSync(bin).length, 8);
}

test("npm packed dependency executes the real installer with generated provenance", t => {
  if (process.platform !== "linux" || process.arch !== "x64")
    return t.skip("Fixture download names target Linux x64");
  const source = fixture(t);
  copyPackage(source);
  const sha = commitPackage(source);
  const output = fixture(t);
  npm(source, "pack", "--pack-destination", output);
  const tarball = path.join(
    output,
    fs.readdirSync(output).find(name => name.endsWith(".tgz")),
  );
  const { root, env } = consumer(t, sha, `file:${tarball}`);
  const npmCli = process.env.npm_execpath;
  assert.ok(npmCli, "Run through npm run test-installation");
  const npmVersion = execFileSync(process.execPath, [npmCli, "--version"], {
    cwd: root,
    encoding: "utf8",
  }).trim();
  const installArgs = [
    npmCli,
    "install",
    "--offline",
    "--no-audit",
    "--no-fund",
    "--foreground-scripts",
  ];
  execFileSync(process.execPath, installArgs, {
    cwd: root,
    env,
    stdio: "pipe",
  });
  if (Number(npmVersion.split(".")[0]) >= 12) {
    execFileSync(
      process.execPath,
      [npmCli, "install-scripts", "approve", "rescript"],
      { cwd: root, env, stdio: "pipe" },
    );
    execFileSync(process.execPath, [npmCli, "rebuild", "rescript"], {
      cwd: root,
      env,
      stdio: "pipe",
    });
  }
  assertToolchain(root, sha);
});

test("pnpm copied file uses prepared provenance and checkout link remains untouched", t => {
  if (process.platform !== "linux" || process.arch !== "x64")
    return t.skip("Fixture download names target Linux x64");
  const source = fixture(t);
  copyPackage(source);
  const sha = commitPackage(source);
  writePackageSourceSha(source);
  const pnpm = "pnpm";
  const { root, env } = consumer(t, sha, `file:${source}`);
  const version = execFileSync(pnpm, ["--version"], {
    cwd: root,
    env,
    encoding: "utf8",
  }).trim();
  const args = [
    "install",
    "--offline",
    "--ignore-pnpmfile",
    "--store-dir",
    path.join(root, "store"),
  ];
  // pnpm 11 moved dependency-script approval to its new policy surface.
  if (Number(version.split(".")[0]) >= 11)
    args.push("--dangerously-allow-all-builds");
  execFileSync(pnpm, args, { cwd: root, env, stdio: "pipe" });
  assertToolchain(root, sha);
  assert.equal(
    fs.existsSync(path.join(source, "packages/@rescript/linux-x64/bin")),
    false,
  );
  const linked = consumer(t, sha, `link:${source}`);
  execFileSync(pnpm, args, {
    cwd: linked.root,
    env: linked.env,
    stdio: "pipe",
  });
  assert.equal(
    fs.realpathSync(path.join(linked.root, "node_modules/rescript")),
    fs.realpathSync(source),
  );
  assert.equal(
    fs.existsSync(path.join(source, "packages/@rescript/linux-x64/bin")),
    false,
  );
});
