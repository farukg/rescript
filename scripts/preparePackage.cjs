#!/usr/bin/env node
const path = require("node:path");
const { writePackageSourceSha } = require("./sourceIdentity.cjs");

try {
  writePackageSourceSha(path.join(__dirname, ".."));
} catch (error) {
  console.error(`[rescript-patched] ${error.message}`);
  process.exitCode = 1;
}
