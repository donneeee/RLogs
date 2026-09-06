import { createHash } from "node:crypto";
import { mkdir, readFile, readdir, rm, writeFile } from "node:fs/promises";
import { basename, join, relative, resolve, sep } from "node:path";

const source = resolve(process.argv[2] ?? "");
const output = resolve(process.argv[3] ?? "");
if (!process.argv[2] || !process.argv[3]) {
  throw new Error("usage: node build-kv-import.mjs <submission-service-root> <output-directory>");
}

const maximumValueBytes = 25 * 1024 * 1024;
const maximumBatchBytes = 18 * 1024 * 1024;
const includeRoots = ["accounts", "characters", "memberships", "profiles", "projections", "reconciliations"];
const includeRootFiles = ["catalog.v1.json", "community-milestones.v1.json"];
const excludedSegments = new Set(["login-codes", "oauth-states"]);

await rm(output, { recursive: true, force: true });
await mkdir(output, { recursive: true });

const entries = [];
for (const name of includeRootFiles) await addFile(join(source, name));
for (const name of includeRoots) await walk(join(source, name));
await addHttpSnapshots();

entries.sort((left, right) => left.key.localeCompare(right.key));
const batches = [];
let batch = [];
let batchBytes = 2;
for (const entry of entries) {
  const encodedBytes = Buffer.byteLength(JSON.stringify(entry), "utf8") + 1;
  if (batch.length && batchBytes + encodedBytes > maximumBatchBytes) {
    batches.push(batch);
    batch = [];
    batchBytes = 2;
  }
  batch.push(entry);
  batchBytes += encodedBytes;
}
if (batch.length) batches.push(batch);

const batchFiles = [];
for (const [index, values] of batches.entries()) {
  const name = `batch-${String(index + 1).padStart(3, "0")}.json`;
  await writeFile(
    join(output, name),
    JSON.stringify(values.map(({ key, value, base64 }) => ({ key, value, base64 }))),
    "utf8",
  );
  batchFiles.push(name);
}
const manifest = {
  schema_version: 1,
  source,
  generated_unix_millis: Date.now(),
  entry_count: entries.length,
  value_bytes: entries.reduce((sum, entry) => sum + entry.source_bytes, 0),
  sha256: createHash("sha256")
    .update(entries.map((entry) => `${entry.key}\0${entry.source_sha256}\n`).join(""))
    .digest("hex"),
  batch_files: batchFiles,
};
await writeFile(join(output, "manifest.json"), `${JSON.stringify(manifest, null, 2)}\n`, "utf8");
process.stdout.write(`${JSON.stringify(manifest)}\n`);

async function walk(directory) {
  for (const item of await readdir(directory, { withFileTypes: true })) {
    if (excludedSegments.has(item.name)) continue;
    const path = join(directory, item.name);
    if (item.isDirectory()) await walk(path);
    else if (item.isFile()) await addFile(path);
  }
}

async function addFile(path) {
  const bytes = await readFile(path);
  if (bytes.byteLength > maximumValueBytes) {
    throw new Error(`${path} exceeds the Cloudflare KV value limit`);
  }
  const key = `fs:${relative(source, path).split(sep).join("/")}`;
  entries.push({
    key,
    value: bytes.toString("base64"),
    base64: true,
    source_bytes: bytes.byteLength,
    source_sha256: createHash("sha256").update(bytes).digest("hex"),
  });
}

async function addHttpSnapshots() {
  const base = process.env.RLOGS_LOCAL_API_BASE ?? "http://127.0.0.1:8788";
  for (const sort of ["newest", "popular"]) {
    const response = await fetch(`${base}/v1/photos?sort=${sort}&limit=4`);
    if (!response.ok) throw new Error(`local ${sort} photo catalog returned HTTP ${response.status}`);
    addBytes(`fs:snapshots/photos-${sort}-4-anonymous.json`, Buffer.from(await response.arrayBuffer()));
  }
  const accountIndex = join(source, "accounts", "account-id-index");
  for (const item of await readdir(accountIndex, { withFileTypes: true })) {
    if (!item.isFile() || !item.name.endsWith(".json")) continue;
    const accountId = basename(item.name, ".json");
    const response = await fetch(`${base}/v1/users/${accountId}`);
    if (!response.ok) throw new Error(`local public user ${accountId} returned HTTP ${response.status}`);
    addBytes(`fs:snapshots/users/${accountId}.json`, Buffer.from(await response.arrayBuffer()));
  }
}

function addBytes(key, bytes) {
  entries.push({
    key,
    value: bytes.toString("base64"),
    base64: true,
    source_bytes: bytes.byteLength,
    source_sha256: createHash("sha256").update(bytes).digest("hex"),
  });
}
