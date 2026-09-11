import assert from "node:assert/strict";
import { createHash } from "node:crypto";
import { mkdtemp, readFile, rm } from "node:fs/promises";
import { createServer } from "node:net";
import { tmpdir } from "node:os";
import { dirname, join, resolve, sep } from "node:path";
import { fileURLToPath } from "node:url";
import { spawn, spawnSync } from "node:child_process";

const verifierRoot = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const wranglerBin = join(verifierRoot, "node_modules", "wrangler", "bin", "wrangler.js");
const configPath = join(verifierRoot, "wrangler.backfill-rehearsal.jsonc");

async function freePort() {
  const server = createServer();
  await new Promise((accept, reject) => {
    server.once("error", reject);
    server.listen(0, "127.0.0.1", accept);
  });
  const address = server.address();
  const port = typeof address === "object" && address ? address.port : null;
  await new Promise((accept, reject) => server.close((error) => error ? reject(error) : accept()));
  assert.ok(Number.isInteger(port) && port > 0, "could not reserve a local rehearsal port");
  return port;
}

async function waitForWorker(url, child, output) {
  let lastError;
  for (let attempt = 0; attempt < 80; attempt += 1) {
    if (child.exitCode !== null) {
      throw new Error(`local Wrangler exited before startup with code ${child.exitCode}:\n${output()}`);
    }
    try {
      const response = await fetch(url);
      if (response.status === 404) return;
    } catch (cause) {
      lastError = cause;
    }
    await new Promise((accept) => setTimeout(accept, 100));
  }
  throw new Error(`local Wrangler did not become ready: ${lastError ?? "timeout"}`);
}

async function stop(child) {
  if (child.exitCode !== null) return;
  child.kill("SIGTERM");
  await Promise.race([
    new Promise((accept) => child.once("exit", accept)),
    new Promise((accept) => setTimeout(accept, 5_000)),
  ]);
  if (child.exitCode === null) {
    child.kill("SIGKILL");
    await Promise.race([
      new Promise((accept) => child.once("exit", accept)),
      new Promise((accept) => setTimeout(accept, 2_000)),
    ]);
  }
}

export async function runBackfillRolloutRehearsal() {
  const config = JSON.parse(await readFile(configPath, "utf8"));
  assert.equal(config.name, "rlogs-backfill-rehearsal-isolated");
  assert.equal(config.d1_databases?.[0]?.binding, "RLOGS_DB");
  assert.equal(config.d1_databases?.[0]?.database_name, "rlogs-backfill-rehearsal-isolated");
  assert.equal(config.d1_databases?.[0]?.database_id, "00000000-0000-0000-0000-000000000000");
  assert.equal(config.r2_buckets?.[0]?.binding, "RLOGS_ARTIFACTS");
  assert.equal(config.r2_buckets?.[0]?.bucket_name, "rlogs-backfill-rehearsal-isolated");
  assert.doesNotMatch(JSON.stringify(config), /rlogs-production|e957a66e-5ca9-4fc9-a1e2-9c004d23cdb6/u,
    "isolated rehearsal config must never reference production resources");
  const storagePath = await mkdtemp(join(tmpdir(), "rlogs-backfill-rehearsal-"));
  const resolvedStorage = resolve(storagePath);
  const resolvedTemp = resolve(tmpdir());
  assert.ok(resolvedStorage.startsWith(`${resolvedTemp}${sep}`),
    "refusing to use a rehearsal directory outside the system temporary directory");
  const port = await freePort();
  let child;
  try {
    const migration = spawnSync(process.execPath, [
      wranglerBin, "d1", "migrations", "apply", "rlogs-backfill-rehearsal-isolated",
      "--local", "--config", configPath, "--persist-to", resolvedStorage,
    ], { cwd: verifierRoot, encoding: "utf8" });
    if (migration.status !== 0) {
      throw new Error(`isolated D1 migration failed:\n${migration.stdout}\n${migration.stderr}`);
    }

    child = spawn(process.execPath, [
      wranglerBin, "dev", "--local", "--config", configPath,
      "--persist-to", resolvedStorage, "--ip", "127.0.0.1", "--port", String(port),
      "--log-level", "error",
    ], { cwd: verifierRoot, stdio: ["ignore", "pipe", "pipe"] });
    let workerOutput = "";
    child.stdout.on("data", (chunk) => { workerOutput += chunk; });
    child.stderr.on("data", (chunk) => { workerOutput += chunk; });
    const baseUrl = `http://127.0.0.1:${port}`;
    await waitForWorker(baseUrl, child, () => workerOutput);
    const response = await fetch(`${baseUrl}/exercise`, { method: "POST" });
    const text = await response.text();
    if (!response.ok) throw new Error(`isolated Worker exercise failed (${response.status}): ${text}\n${workerOutput}`);
    const receipt = JSON.parse(text);
    const { receipt_sha256: claimedDigest, ...payload } = receipt;
    const actualDigest = createHash("sha256").update(JSON.stringify(payload)).digest("hex");
    assert.equal(claimedDigest, actualDigest, "rehearsal receipt digest does not match its payload");
    assert.equal(receipt.environment, "isolated-wrangler-miniflare");
    assert.equal(receipt.production_bindings_present, false);
    assert.equal(receipt.production_publication_pause.active, true);
    return receipt;
  } finally {
    if (child) await stop(child);
    await rm(resolvedStorage, { recursive: true, force: true, maxRetries: 20, retryDelay: 100 });
  }
}

if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  const receipt = await runBackfillRolloutRehearsal();
  process.stdout.write(`${JSON.stringify(receipt, null, 2)}\n`);
}
