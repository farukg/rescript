const fs = require("node:fs");
const path = require("node:path");
const { createRequire } = require("node:module");

function rewatchTestEnvironment(root) {
  const manifest = JSON.parse(
    fs.readFileSync(path.join(root, "package.json"), "utf8"),
  );
  const localRequire = createRequire(path.join(root, "package.json"));
  const runtimeManifest = localRequire.resolve(
    "@rescript/runtime/package.json",
  );
  const runtime = JSON.parse(fs.readFileSync(runtimeManifest, "utf8"));
  if (
    runtime.name !== "@rescript/runtime" ||
    runtime.version !== manifest.dependencies["@rescript/runtime"]
  ) {
    throw new Error(
      "Rewatch tests require the compiler package's declared @rescript/runtime version.",
    );
  }
  const runtimeRoot = path.dirname(runtimeManifest);
  for (const artifact of ["lib/ocaml/Stdlib.cmi", "lib/ocaml/Stdlib.cmj"]) {
    if (
      !fs.existsSync(path.join(runtimeRoot, artifact)) ||
      !fs.statSync(path.join(runtimeRoot, artifact)).isFile()
    ) {
      throw new Error(`Rewatch tests require a built runtime: ${artifact}`);
    }
  }
  const compiler = path.join(
    root,
    "packages",
    "@rescript",
    `${process.platform}-${process.arch}`,
    "bin",
    "bsc.exe",
  );
  if (!fs.existsSync(compiler) || !fs.statSync(compiler).isFile()) {
    throw new Error("Rewatch tests require the assembled platform compiler.");
  }
  return { RESCRIPT_BSC_EXE: compiler, RESCRIPT_RUNTIME: runtimeRoot };
}

module.exports = { rewatchTestEnvironment };

if (require.main === module) {
  try {
    const environment = rewatchTestEnvironment(path.join(__dirname, ".."));
    if (!process.env.GITHUB_ENV)
      throw new Error(
        "GITHUB_ENV is required to bind the rewatch test prerequisites.",
      );
    fs.appendFileSync(
      process.env.GITHUB_ENV,
      Object.entries(environment)
        .map(([name, value]) => `${name}=${value}\n`)
        .join(""),
    );
  } catch (error) {
    console.error(
      `[rewatch-tests] Compiler/runtime prerequisites unavailable: ${error.message}`,
    );
    process.exitCode = 1;
  }
}
