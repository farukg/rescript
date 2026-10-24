const { spawnSync } = require("node:child_process");
const path = require("node:path");
const { rewatchTestEnvironment } = require("../rewatchTestEnvironment.cjs");

try {
  const root = path.resolve(__dirname, "../..");
  const environment = rewatchTestEnvironment(root);
  const result = spawnSync(
    "cargo",
    ["test", "--manifest-path", "rewatch/Cargo.toml", "--release"],
    {
      cwd: root,
      env: { ...process.env, ...environment },
      stdio: "inherit",
    },
  );
  if (result.error) throw result.error;
  process.exitCode = result.status ?? 1;
} catch (error) {
  console.error(`[rewatch-tests] ${error.message}`);
  process.exitCode = 1;
}
