import { execFileSync } from "node:child_process";
import { mkdirSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { dirname, resolve } from "node:path";

const keysPath = resolve(process.argv[2] ?? "");
const outputPath = resolve(process.argv[3] ?? "");
const namespaceId = process.argv[4];
if (!process.argv[2] || !process.argv[3] || !namespaceId) {
  throw new Error(
    "usage: node backup-kv-keys.mjs <keys.json> <output.json> <namespace-id>",
  );
}

const keys = JSON.parse(readFileSync(keysPath, "utf8"));
if (!Array.isArray(keys) || keys.some((key) => typeof key !== "string" || !key)) {
  throw new Error("keys file must contain a JSON array of non-empty strings");
}

const wranglerBin = resolve(process.cwd(), "node_modules/wrangler/bin/wrangler.js");
mkdirSync(dirname(outputPath), { recursive: true });
const values = {};
for (let index = 0; index < keys.length; index += 4) {
  const batchKeys = keys.slice(index, index + 4);
  const batchPath = `${outputPath}.keys-${String(index / 4 + 1).padStart(3, "0")}.json`;
  writeFileSync(batchPath, JSON.stringify(batchKeys), "utf8");
  try {
    const output = execFileSync(
      process.execPath,
      [wranglerBin, "kv", "bulk", "get", batchPath, "--namespace-id", namespaceId, "--remote"],
      {
        cwd: process.cwd(),
        encoding: "utf8",
        maxBuffer: 128 * 1024 * 1024,
        stdio: ["ignore", "pipe", "inherit"],
      },
    );
    const batchValues = JSON.parse(output);
    if (typeof batchValues !== "object" || batchValues === null || Array.isArray(batchValues)) {
      throw new Error("Wrangler returned an unexpected KV backup shape");
    }
    Object.assign(values, batchValues);
  } finally {
    rmSync(batchPath, { force: true });
  }
}
const returnedKeys = Object.keys(values);
if (returnedKeys.length !== keys.length || keys.some((key) => !(key in values))) {
  throw new Error(`Wrangler returned ${returnedKeys.length} of ${keys.length} requested keys`);
}

writeFileSync(outputPath, JSON.stringify(values), "utf8");
console.log(JSON.stringify({ keys: keys.length, output: outputPath }));
