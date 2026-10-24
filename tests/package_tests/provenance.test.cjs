const assert = require("node:assert/strict");
const fs = require("node:fs");
const path = require("node:path");
const { execFileSync } = require("node:child_process");
const { test } = require("node:test");
const {
  fixture,
  copyPackage,
  git,
  commitPackage,
  npm,
} = require("./fixtures.cjs");
const {
  writePackageSourceSha,
  packagedSourceSha,
} = require("../../scripts/sourceIdentity.cjs");

const marker = "packages/@rescript/packaged-source-sha.txt";

test("npm pack derives the exact clean root commit and ships its marker", t => {
  const root = fixture(t);
  copyPackage(root);
  const sha = commitPackage(root);
  const output = fixture(t);
  npm(root, "pack", "--json", "--pack-destination", output);
  assert.equal(fs.readFileSync(path.join(root, marker), "utf8"), `${sha}\n`);
  const tarball = fs.readdirSync(output).find(name => name.endsWith(".tgz"));
  const packed = execFileSync(
    "tar",
    ["-xOf", path.join(output, tarball), `package/${marker}`],
    { encoding: "utf8" },
  );
  assert.equal(packed, `${sha}\n`);
  assert.equal(git(root, "status", "--porcelain"), "");
});

test("dirty source cannot be packaged as its previous commit", t => {
  const root = fixture(t);
  copyPackage(root);
  commitPackage(root);
  fs.appendFileSync(
    path.join(root, "scripts/postinstall.cjs"),
    "\n// changed fixture\n",
  );
  assert.throws(
    () => writePackageSourceSha(root),
    /Commit package source changes/,
  );
  assert.equal(fs.existsSync(path.join(root, marker)), false);
});

test("an enclosing consumer repository cannot supply package identity", t => {
  const outer = fixture(t);
  copyPackage(outer);
  commitPackage(outer);
  const inner = path.join(outer, "node_modules/rescript");
  copyPackage(inner);
  assert.throws(
    () => writePackageSourceSha(inner),
    /Cannot identify ReScript source/,
  );
  assert.equal(packagedSourceSha(inner), null);
});

test("git archive substitutes its own source commit for file packaging", t => {
  const root = fixture(t);
  copyPackage(root);
  const sha = commitPackage(root);
  const output = fixture(t);
  const archive = path.join(output, "source.tar");
  git(root, "archive", "--format=tar", `--output=${archive}`, "HEAD");
  const extracted = path.join(output, "extracted");
  fs.mkdirSync(extracted);
  execFileSync("tar", ["-xf", archive, "-C", extracted]);
  assert.equal(packagedSourceSha(extracted), sha);
  writePackageSourceSha(extracted);
  assert.equal(
    fs.readFileSync(path.join(extracted, marker), "utf8"),
    `${sha}\n`,
  );
});

test("unexpanded and malformed archive markers never become identities", t => {
  const root = fixture(t);
  copyPackage(root);
  assert.equal(packagedSourceSha(root), null);
  for (const invalid of ["", "abc", "$Format:%H$", "not-a-source-identity"]) {
    fs.writeFileSync(
      path.join(root, "packages/@rescript/source-sha.txt"),
      invalid,
    );
    assert.equal(packagedSourceSha(root), null);
    assert.throws(
      () => writePackageSourceSha(root),
      /Cannot identify ReScript source/,
    );
  }
});
