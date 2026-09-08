import { execFileSync } from "node:child_process";
import { mkdirSync, writeFileSync } from "node:fs";
import { resolve } from "node:path";

const NAMESPACE_ID = "1a2a4d10731e4c7ebe31d568c87fe1c2";
const apply = process.argv.includes("--apply");
const node = process.execPath;
const wranglerBin = resolve(process.cwd(), "node_modules/wrangler/bin/wrangler.js");
const backupDir = resolve(
  process.cwd(),
  `../../../runtime-data/cloudflare-reconciliation-backups/${new Date().toISOString().slice(0, 10)}-profile-realms`,
);

function wrangler(...args) {
  return execFileSync(node, [wranglerBin, ...args], {
    cwd: process.cwd(), encoding: "utf8", maxBuffer: 128 * 1024 * 1024,
    stdio: ["ignore", "pipe", "inherit"],
  });
}

function readKey(key) {
  return wrangler("kv", "key", "get", key, "--namespace-id", NAMESPACE_ID, "--remote", "--text");
}

function realmFor(profile) {
  if (profile.deployment !== "global") return null;
  if (profile.region === "north-america") return "asteria";
  if (profile.region === "europe") return "bahamar";
  return null;
}

function normalizeProfile(profile, realm) {
  profile.realm = realm;
  profile.world ??= null;
  profile.envelope.routing.realm = realm;
  delete profile.envelope.routing.world;
  profile.envelope.body.character.region.realm_id = realm;
  profile.envelope.body.character.region.world_id ??= null;
}

mkdirSync(backupDir, { recursive: true });
const catalogKey = "fs:profiles/catalog.v1.json";
const catalogText = readKey(catalogKey);
const catalog = JSON.parse(catalogText);
const records = [];

for (const entry of catalog.profiles) {
  const realm = realmFor(entry);
  if (!realm) continue;
  const key = `fs:profiles/${entry.profile_id}/public.json`;
  const before = readKey(key);
  const profile = JSON.parse(before);
  if (String(profile.character_id) !== String(entry.character_id)) {
    throw new Error(`${entry.profile_id} character identity mismatch`);
  }
  normalizeProfile(profile, realm);
  entry.realm = realm;
  entry.world ??= null;
  records.push({ key, before, after: JSON.stringify(profile) });
}

records.push({ key: catalogKey, before: catalogText, after: JSON.stringify(catalog) });
for (const record of records) {
  const name = record.key.replaceAll(":", "_").replaceAll("/", "_");
  writeFileSync(resolve(backupDir, `${name}.before.json`), record.before, "utf8");
  writeFileSync(resolve(backupDir, `${name}.after.json`), record.after, "utf8");
  if (apply && record.before !== record.after) {
    wrangler("kv", "key", "put", record.key, "--path", resolve(backupDir, `${name}.after.json`),
      "--namespace-id", NAMESPACE_ID, "--remote");
  }
}

const changed = records.filter((record) => record.before !== record.after);
console.log(`${apply ? "Applied" : "Dry run validated"}: ${changed.length} changed keys; ${records.length} backed up keys.`);
