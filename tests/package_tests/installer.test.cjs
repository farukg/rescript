const assert = require("node:assert/strict");
const fs = require("node:fs");
const path = require("node:path");
const vm = require("node:vm");
const { createRequire } = require("node:module");
const { test } = require("node:test");
const { fixture, copyPackage } = require("./fixtures.cjs");

const current = "1234567890abcdef1234567890abcdef12345678";
const previous = "abcdef1234567890abcdef1234567890abcdef12";
const binaries = [
  "bsb_helper",
  "bsc",
  "ninja",
  "rescript-editor-analysis",
  "rescript-tools",
  "rescript-legacy",
  "rescript",
];

async function install(
  t,
  {
    expected = current,
    release = current,
    resolved = "",
    directory = "package",
    installed,
    missing,
    failDownload,
    failActivation = false,
    checkout = false,
  } = {},
) {
  const root = path.join(fixture(t), directory);
  copyPackage(root);
  if (checkout) fs.mkdirSync(path.join(root, ".git"));
  fs.writeFileSync(
    path.join(root, "packages/@rescript/packaged-source-sha.txt"),
    `${expected}\n`,
  );
  const binDir = path.join(root, "packages/@rescript/linux-x64/bin");
  if (installed) {
    fs.mkdirSync(binDir, { recursive: true });
    fs.writeFileSync(path.join(binDir, "source-sha.txt"), `${installed}\n`);
    for (const binary of binaries) {
      if (binary !== missing)
        fs.writeFileSync(path.join(binDir, `${binary}.exe`), "previous binary");
    }
  }
  let builds = 0;
  const requests = [];
  const messages = [];
  const processStub = {
    platform: "linux",
    arch: "x64",
    pid: process.pid,
    env: { npm_package_resolved: resolved },
    exit(code) {
      this.exitCode = code;
    },
    exitCode: 0,
  };
  const localRequire = createRequire(
    path.join(root, "scripts/postinstall.cjs"),
  );
  await vm.runInNewContext(
    fs.readFileSync(path.join(root, "scripts/postinstall.cjs"), "utf8"),
    {
      __dirname: path.join(root, "scripts"),
      Buffer,
      process: processStub,
      console: Object.fromEntries(
        ["log", "warn", "error"].map(level => [
          level,
          message => messages.push(message),
        ]),
      ),
      require(name) {
        if (name === "child_process")
          return {
            execSync() {
              builds++;
              throw new Error("Compiler execution forbidden in installer test");
            },
          };
        if (name === "fs" && failActivation)
          return {
            ...fs,
            renameSync(from, to) {
              if (from.includes(".bin-install-") && to === binDir)
                throw new Error("Activation interrupted");
              fs.renameSync(from, to);
            },
          };
        return localRequire(name);
      },
      async fetch(url) {
        requests.push(url);
        if (url.includes(failDownload || "not-a-release-asset"))
          throw new Error("Download interrupted");
        const body = url.endsWith("/source-sha.txt")
          ? `${release}\n`
          : "downloaded binary";
        return {
          ok: true,
          async arrayBuffer() {
            return Buffer.from(body);
          },
        };
      },
    },
  );
  return {
    root,
    binDir,
    builds,
    requests,
    messages,
    exitCode: processStub.exitCode,
  };
}

function assertInstalled(result, sha) {
  assert.equal(result.exitCode, 0);
  assert.equal(result.builds, 0);
  assert.equal(
    fs.readFileSync(path.join(result.binDir, "source-sha.txt"), "utf8"),
    `${sha}\n`,
  );
  for (const binary of binaries)
    assert.ok(fs.existsSync(path.join(result.binDir, `${binary}.exe`)));
}

test("file package installs its producer identity and rejects historical binaries", async t => {
  assertInstalled(await install(t), current);
  const rejected = await install(t, { release: previous });
  assert.equal(rejected.exitCode, 1);
  assert.equal(rejected.requests.length, 1);
  assert.equal(rejected.builds, 1);
  assert.equal(fs.existsSync(rejected.binDir), false);
});

test("complete matching toolchain is reused but missing binaries require reinstall", async t => {
  const reused = await install(t, { installed: current });
  assertInstalled(reused, current);
  assert.equal(reused.requests.length, 0);
  const repaired = await install(t, { installed: current, missing: "bsc" });
  assertInstalled(repaired, current);
  assert.equal(repaired.requests.length, 8);
});

test("Git fragment and pnpm codeload identities override packaged fallback", async t => {
  assertInstalled(
    await install(t, {
      expected: previous,
      resolved: `git+https://github.com/farukg/rescript.git#${current}`,
    }),
    current,
  );
  assertInstalled(
    await install(t, {
      expected: previous,
      directory: `farukg+rescript+tar.gz+${current.slice(0, 24)}_fixture/node_modules/rescript`,
    }),
    current,
  );
});

test("unknown file identity is rejected instead of accepting arbitrary release", async t => {
  const result = await install(t, { expected: "$Format:%H$" });
  assert.equal(result.exitCode, 1);
  assert.equal(result.requests.length, 0);
  assert.equal(result.builds, 0);
  assert.ok(
    result.messages.some(message => message.includes("npm run prepack")),
  );
  assert.equal(fs.existsSync(result.binDir), false);
});

for (const failure of ["download", "activation"]) {
  test(`interrupted ${failure} preserves the previous toolchain`, async t => {
    const result = await install(t, {
      installed: previous,
      failDownload: failure === "download" ? "bsc-linux" : undefined,
      failActivation: failure === "activation",
    });
    assert.equal(result.exitCode, 1);
    assert.equal(
      fs.readFileSync(path.join(result.binDir, "source-sha.txt"), "utf8"),
      `${previous}\n`,
    );
    assert.equal(
      fs.readFileSync(path.join(result.binDir, "bsc.exe"), "utf8"),
      "previous binary",
    );
    assert.deepEqual(fs.readdirSync(path.dirname(result.binDir)), ["bin"]);
  });
}

test("checkout links skip downloads and source builds", async t => {
  const result = await install(t, { checkout: true });
  assert.equal(result.exitCode, 0);
  assert.equal(result.requests.length, 0);
  assert.equal(result.builds, 0);
});
