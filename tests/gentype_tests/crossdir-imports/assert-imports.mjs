import { readFileSync } from "node:fs";
import { join, dirname } from "node:path";
import { fileURLToPath } from "node:url";

const root = dirname(fileURLToPath(import.meta.url));
const consumerGen = join(root, "src/b/CrossDirConsumer.gen.tsx");
const text = readFileSync(consumerGen, "utf8");

const forbidden = [
  // same-dir claim for a peer that lives in src/a
  /from\s+['"]\.\/CrossDirPeer\.gen['"]/,
  // project-root walk-up form
  /from\s+['"](\.\.\/)+src\/a\/CrossDirPeer\.gen['"]/,
];

const required = /from\s+['"]\.\.\/a\/CrossDirPeer\.gen['"]/;

const failures = [];
for (const re of forbidden) {
  if (re.test(text)) {
    failures.push(`forbidden import matched ${re}`);
  }
}
if (!required.test(text)) {
  failures.push("missing minimal relative import from '../a/CrossDirPeer.gen'");
}

if (failures.length > 0) {
  console.error("crossdir-imports genType path assertions failed:");
  for (const f of failures) console.error(" -", f);
  console.error("\n--- src/b/CrossDirConsumer.gen.tsx ---\n" + text);
  process.exit(1);
}

console.log(
  "crossdir-imports: ok (../a/CrossDirPeer.gen, no ./Peer and no ../../src walk-up)",
);
