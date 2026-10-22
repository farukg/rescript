const assert = require("node:assert/strict");
const fs = require("node:fs");
const path = require("node:path");
const { test } = require("node:test");
const { fixture } = require("./fixtures.cjs");
const {
  rewatchTestEnvironment,
} = require("../../scripts/rewatchTestEnvironment.cjs");

function prerequisites(t) {
  const root = fixture(t);
  fs.writeFileSync(
    path.join(root, "package.json"),
    JSON.stringify({
      name: "rescript",
      dependencies: { "@rescript/runtime": "12.3.0" },
    }),
  );
  const runtime = path.join(root, "node_modules/@rescript/runtime");
  fs.mkdirSync(path.join(runtime, "lib/ocaml"), { recursive: true });
  fs.writeFileSync(
    path.join(runtime, "package.json"),
    JSON.stringify({
      name: "@rescript/runtime",
      version: "12.3.0",
      exports: { "./package.json": "./package.json" },
    }),
  );
  const compiler = path.join(
    root,
    "packages/@rescript",
    `${process.platform}-${process.arch}`,
    "bin/bsc.exe",
  );
  fs.mkdirSync(path.dirname(compiler), { recursive: true });
  fs.writeFileSync(compiler, "compiler path fixture");
  for (const artifact of ["Stdlib.cmi", "Stdlib.cmj"])
    fs.writeFileSync(
      path.join(runtime, "lib/ocaml", artifact),
      "artifact path fixture",
    );
  return { root, runtime, compiler };
}

test("rewatch test setup binds the declared installed runtime and platform compiler", t => {
  const { root, runtime, compiler } = prerequisites(t);
  assert.deepEqual(rewatchTestEnvironment(root), {
    RESCRIPT_BSC_EXE: compiler,
    RESCRIPT_RUNTIME: runtime,
  });
});

test("rewatch test setup rejects mismatched runtime versions", t => {
  const { root, runtime } = prerequisites(t);
  fs.writeFileSync(
    path.join(runtime, "package.json"),
    JSON.stringify({ name: "@rescript/runtime", version: "0.0.0" }),
  );
  assert.throws(
    () => rewatchTestEnvironment(root),
    /declared @rescript\/runtime version/,
  );
});

test("rewatch test setup rejects unavailable compiler or runtime artifacts", t => {
  for (const artifact of ["compiler", "runtime"]) {
    const { root, runtime, compiler } = prerequisites(t);
    fs.rmSync(
      artifact === "compiler"
        ? compiler
        : path.join(runtime, "lib/ocaml/Stdlib.cmj"),
    );
    assert.throws(
      () => rewatchTestEnvironment(root),
      artifact === "compiler" ? /assembled platform compiler/ : /built runtime/,
    );
  }
});
