import { execFileSync } from "node:child_process";
import { mkdtempSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { fileURLToPath } from "node:url";

const endpoint = String(process.env.RLOGS_API_BASE_URL ?? "https://rlogs-submissions.pages.dev").replace(/\/$/u, "");
const catalog = await getJson(`${endpoint}/v1/profiles`);
const directory = mkdtempSync(join(tmpdir(), "rlogs-profile-leaderboards-"));
const serviceDirectory = fileURLToPath(new URL("..", import.meta.url));
const wrangler = join(serviceDirectory, "node_modules", "wrangler", "bin", "wrangler.js");

try {
  for (const entry of catalog.profiles ?? []) {
    const profile = await getJson(`${endpoint}/v1/profiles/${encodeURIComponent(entry.profile_id)}`);
    const sql = profileSql(profile);
    const path = join(directory, `${profile.profile_id}.sql`);
    writeFileSync(path, sql, "utf8");
    execFileSync(process.execPath, [
      wrangler, "d1", "execute", "rlogs-production", "--remote", "--file", path,
    ], { cwd: serviceDirectory, stdio: "inherit" });
    const body = profile.envelope.body;
    const recordCount = observedRecords(body).length;
    console.log(`Backfilled ${profile.display_name ?? profile.character_id}: ${recordCount} timed records`);
  }
} finally {
  rmSync(directory, { recursive: true, force: true });
}

async function getJson(url) {
  const response = await fetch(url);
  if (!response.ok) throw new Error(`${url} returned HTTP ${response.status}`);
  return response.json();
}

function profileSql(profile) {
  const profileId = String(profile.profile_id);
  if (!/^prf_[a-z0-9_]+$/u.test(profileId)) throw new Error("unsafe profile id");
  const body = profile.envelope?.body ?? {};
  const observed = positiveInteger(profile.updated_unix_millis);
  if (observed == null) throw new Error(`profile ${profileId} has no observation time`);
  const characterId = sqlText(profile.character_id);
  const displayName = sqlText(profile.display_name);
  const deployment = sqlText(profile.deployment);
  const region = sqlText(profile.region);
  const realm = sqlText(profile.realm);
  const statements = [
    `DELETE FROM profile_season_rankings WHERE profile_id='${profileId}'`,
    `DELETE FROM profile_dungeon_records WHERE profile_id='${profileId}'`,
  ];
  const season = positiveInteger(body.season?.season_id);
  const score = nonnegativeInteger(body.master_score);
  if (season != null && score != null) {
    statements.push(`INSERT INTO profile_season_rankings (
      profile_id, character_id, display_name, deployment_id, region_id, realm_id,
      season_id, master_score, observed_unix_millis
    ) VALUES ('${profileId}', ${characterId}, ${displayName}, ${deployment}, ${region}, ${realm},
      ${season}, ${score}, ${observed})`);
  }
  const rows = observedRecords(body).map((record) => `(
    '${profileId}', ${characterId}, ${displayName}, ${deployment}, ${region}, ${realm},
    ${record.season_id}, ${record.activity_id}, ${record.tier},
    ${sqlInteger(record.score)}, ${record.pass_time_seconds}, ${sqlInteger(record.completion_count)}, ${observed}
  )`);
  if (rows.length > 0) {
    statements.push(`INSERT INTO profile_dungeon_records (
      profile_id, character_id, display_name, deployment_id, region_id, realm_id,
      season_id, activity_id, tier, score, pass_time_seconds, completion_count,
      observed_unix_millis
    ) VALUES ${rows.join(",")}`);
  }
  return `${statements.join(";\n")};\n`;
}

function observedRecords(body) {
  const rows = [];
  const seen = new Set();
  for (const entry of body.activity_progress?.master_mode_dungeons ?? []) {
    const seasonId = positiveInteger(entry?.season_id);
    const activityId = positiveInteger(entry?.difficulty_id);
    const tier = positiveInteger(entry?.dungeon?.dungeon_id);
    const passTime = positiveInteger(entry?.dungeon?.pass_time);
    if (seasonId == null || activityId == null || tier == null || tier > 20 || passTime == null) continue;
    const key = `${seasonId}:${activityId}:${tier}`;
    if (seen.has(key)) continue;
    seen.add(key);
    rows.push({
      season_id: seasonId,
      activity_id: activityId,
      tier,
      score: nonnegativeInteger(entry?.dungeon?.score),
      pass_time_seconds: passTime,
      completion_count: nonnegativeInteger(entry?.dungeon?.completion_count),
    });
  }
  return rows;
}

function positiveInteger(value) {
  const number = Number(value);
  return Number.isSafeInteger(number) && number > 0 ? number : null;
}

function nonnegativeInteger(value) {
  const number = Number(value);
  return Number.isSafeInteger(number) && number >= 0 ? number : null;
}

function sqlInteger(value) {
  return value == null ? "NULL" : String(value);
}

function sqlText(value) {
  if (value == null) return "NULL";
  return `'${String(value).replaceAll("'", "''")}'`;
}
