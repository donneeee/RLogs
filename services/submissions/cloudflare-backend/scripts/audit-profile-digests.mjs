import { readFile } from "node:fs/promises";
import { createHash } from "node:crypto";

import { canonicalJson } from "../src/profile.js";

let failures = 0;
for (const packagePath of process.argv.slice(2)) {
  const packageValue = JSON.parse(await readFile(packagePath, "utf8"));
  const actual = createHash("sha256").update(canonicalJson(packageValue.request)).digest("hex");
  const matches = actual === packageValue.package_id;
  process.stdout.write(`${matches ? "ok" : "mismatch"} ${packageValue.package_id} ${packagePath}\n`);
  if (!matches) failures += 1;
}
if (failures) process.exitCode = 1;
