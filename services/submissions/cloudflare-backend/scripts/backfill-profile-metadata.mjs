import { execFileSync } from "node:child_process";
import { createHash } from "node:crypto";
import { mkdirSync, writeFileSync } from "node:fs";
import { resolve } from "node:path";

const NAMESPACE_ID = "1a2a4d10731e4c7ebe31d568c87fe1c2";
const DATABASE_NAME = "rlogs-production";
const GAME_ID = "app.rlogs.game.blue-protocol-star-resonance";
const apply = process.argv.includes("--apply");
const node = process.execPath;
const wranglerBin = resolve(process.cwd(), "node_modules/wrangler/bin/wrangler.js");
const backupDir = resolve(
  process.cwd(),
  "../../../runtime-data/cloudflare-reconciliation-backups/2026-09-07-profile-metadata",
);

function wrangler(...args) {
  return execFileSync(node, [wranglerBin, ...args], {
    cwd: process.cwd(),
    encoding: "utf8",
    maxBuffer: 256 * 1024 * 1024,
    stdio: ["ignore", "pipe", "inherit"],
  });
}

function readKey(key) {
  return JSON.parse(wrangler(
    "kv", "key", "get", key,
    "--namespace-id", NAMESPACE_ID,
    "--remote", "--text",
  ));
}

function d1Rows(command) {
  const pages = JSON.parse(wrangler(
    "d1", "execute", DATABASE_NAME, "--remote", "--command", command, "--json",
  ));
  return pages.flatMap((page) => page.results ?? []);
}

function sql(value) {
  if (value == null) return "NULL";
  if (typeof value === "number") {
    if (!Number.isSafeInteger(value)) throw new Error(`unsafe SQL integer ${value}`);
    return String(value);
  }
  return `'${String(value).replaceAll("'", "''")}'`;
}

function sha256(value) {
  return createHash("sha256").update(value).digest("hex");
}

function requireValue(condition, message) {
  if (!condition) throw new Error(message);
}

mkdirSync(backupDir, { recursive: true });
const catalog = readKey("fs:profiles/catalog.v1.json");
requireValue(Array.isArray(catalog.profiles), "profile catalog is invalid");
const accountIds = new Set(d1Rows("SELECT submitter_id FROM accounts;").map((row) => row.submitter_id));
const statements = [];
const manifestProfiles = [];

for (const entry of catalog.profiles) {
  const profile = readKey(`fs:profiles/${entry.profile_id}/public.json`);
  const claim = readKey(`fs:profiles/${entry.profile_id}/claim.json`);
  requireValue(profile.profile_id === entry.profile_id, `profile ID mismatch for ${entry.profile_id}`);
  requireValue(String(profile.character_id) === String(entry.character_id), `character mismatch for ${entry.profile_id}`);
  requireValue(claim.profile_id === entry.profile_id, `claim profile mismatch for ${entry.profile_id}`);
  requireValue(String(claim.character_id) === String(entry.character_id), `claim character mismatch for ${entry.profile_id}`);
  requireValue(/^usr_[a-f0-9]{32}$/.test(claim.submitter_id), `claim submitter is invalid for ${entry.profile_id}`);
  if (!accountIds.has(claim.submitter_id)) {
    manifestProfiles.push({
      profile_id: profile.profile_id,
      character_id: String(profile.character_id),
      submitter_id: claim.submitter_id,
      display_name: profile.display_name ?? null,
      loadout_count: (profile.loadouts ?? []).filter((summary) => summary.snapshot_available).length,
      state: "waiting_for_account_sync",
    });
    continue;
  }

  statements.push(`INSERT INTO uid_claims (
    game_id, character_id, profile_id, submitter_id, deployment_id, region_id, realm_id, claimed_unix_millis
  ) VALUES (${[
    GAME_ID, String(profile.character_id), profile.profile_id, claim.submitter_id,
    profile.deployment, profile.region, profile.realm ?? null, claim.claimed_unix_millis,
  ].map(sql).join(", ")})
  ON CONFLICT(game_id, character_id) DO UPDATE SET
    profile_id=excluded.profile_id,
    deployment_id=excluded.deployment_id,
    region_id=excluded.region_id,
    realm_id=excluded.realm_id
  WHERE uid_claims.submitter_id=excluded.submitter_id;`);
  statements.push(`INSERT INTO profiles (
    profile_id, game_id, character_id, submitter_id, current_package_id, source_client_build,
    deployment_id, region_id, realm_id, public_projection_json, created_unix_millis, updated_unix_millis
  ) VALUES (${[
    profile.profile_id, GAME_ID, String(profile.character_id), claim.submitter_id,
    profile.package_id, profile.source_client_build, profile.deployment, profile.region,
    profile.realm ?? null, JSON.stringify({
      schema_version: 1,
      profile_id: profile.profile_id,
      character_id: String(profile.character_id),
      display_name: profile.display_name ?? profile.envelope?.body?.display_name ?? null,
    }), profile.created_unix_millis,
    profile.updated_unix_millis,
  ].map(sql).join(", ")})
  ON CONFLICT(profile_id) DO UPDATE SET
    current_package_id=excluded.current_package_id,
    source_client_build=excluded.source_client_build,
    deployment_id=excluded.deployment_id,
    region_id=excluded.region_id,
    realm_id=excluded.realm_id,
    public_projection_json=excluded.public_projection_json,
    created_unix_millis=min(profiles.created_unix_millis, excluded.created_unix_millis),
    updated_unix_millis=excluded.updated_unix_millis
  WHERE profiles.submitter_id=excluded.submitter_id
    AND profiles.game_id=excluded.game_id
    AND profiles.character_id=excluded.character_id
    AND excluded.updated_unix_millis >= profiles.updated_unix_millis;`);

  let loadoutCount = 0;
  for (const summary of profile.loadouts ?? []) {
    if (!summary.snapshot_available) continue;
    const loadout = readKey(`fs:profiles/${profile.profile_id}/loadouts/${summary.project_id}.json`);
    requireValue(loadout.profile_id === profile.profile_id, `loadout profile mismatch for ${profile.profile_id}`);
    requireValue(loadout.project_id === summary.project_id, `loadout project mismatch for ${profile.profile_id}`);
    statements.push(`INSERT INTO profile_loadouts (
      profile_id, project_id, package_id, display_name, projection_json, updated_unix_millis
    ) VALUES (${[
      profile.profile_id, summary.project_id, profile.package_id, summary.project_name ?? null,
      JSON.stringify({
        schema_version: 1,
        profile_id: loadout.profile_id,
        project_id: loadout.project_id,
        project_name: summary.project_name ?? null,
        source_client_build: loadout.source_client_build,
        class_id: loadout.class_id ?? null,
        specialization_id: loadout.specialization_id ?? null,
        module_inventory_count: loadout.module_inventory_count ?? 0,
        equipped_module_count: loadout.equipped_module_count ?? 0,
        updated_unix_millis: loadout.updated_unix_millis,
      }), loadout.updated_unix_millis,
    ].map(sql).join(", ")})
    ON CONFLICT(profile_id, project_id) DO UPDATE SET
      package_id=excluded.package_id,
      display_name=excluded.display_name,
      projection_json=excluded.projection_json,
      updated_unix_millis=excluded.updated_unix_millis
    WHERE excluded.updated_unix_millis >= profile_loadouts.updated_unix_millis;`);
    loadoutCount += 1;
  }
  manifestProfiles.push({
    profile_id: profile.profile_id,
    character_id: String(profile.character_id),
    submitter_id: claim.submitter_id,
    display_name: profile.display_name ?? null,
    loadout_count: loadoutCount,
    state: "ready",
  });
}
const migration = `${statements.join("\n\n")}\n`;
const migrationPath = resolve(backupDir, "profile-metadata.sql");
writeFileSync(migrationPath, migration, "utf8");
writeFileSync(resolve(backupDir, apply ? "manifest.applied.json" : "manifest.dry-run.json"), `${JSON.stringify({
  schema_version: 1,
  applied: apply,
  profile_count: manifestProfiles.length,
  sql_sha256: sha256(migration),
  profiles: manifestProfiles,
}, null, 2)}\n`, "utf8");

if (apply) {
  writeFileSync(resolve(backupDir, "d1.before.json"), wrangler(
    "d1", "execute", DATABASE_NAME, "--remote", "--command",
    "SELECT profile_id, character_id, submitter_id, updated_unix_millis FROM profiles ORDER BY profile_id;",
    "--json",
  ), "utf8");
  // Wrangler's remote import path may reset the D1 coordinator while the live
  // verifier is reading it. These writes are independently idempotent, so use
  // the ordinary query endpoint one statement at a time and safely resume on
  // any transient failure.
  for (const statement of statements) {
    wrangler("d1", "execute", DATABASE_NAME, "--remote", "--command", statement);
  }
  writeFileSync(resolve(backupDir, "d1.after.json"), wrangler(
    "d1", "execute", DATABASE_NAME, "--remote", "--command",
    "SELECT profile_id, character_id, submitter_id, updated_unix_millis FROM profiles ORDER BY profile_id;",
    "--json",
  ), "utf8");
}

const readyCount = manifestProfiles.filter((profile) => profile.state === "ready").length;
console.log(`${apply ? "Applied" : "Dry run validated"}: ${readyCount} ready profile metadata rows; ${manifestProfiles.length - readyCount} await account sync.`);
