#!/usr/bin/env node

import { createHash } from "node:crypto";
import { createReadStream, existsSync, renameSync, rmSync, statSync } from "node:fs";
import { createInterface } from "node:readline";
import { DatabaseSync } from "node:sqlite";
import path from "node:path";

const [command = "help", ...args] = process.argv.slice(2);
const options = parseArgs(args);

if (command === "build") await build(options);
else if (command === "verify") verify(options);
else usage(command === "help" ? 0 : 1);

async function build(value) {
  const input = path.resolve(required(value, "input"));
  const output = path.resolve(required(value, "output"));
  if (!existsSync(input)) throw new Error(`missing input ${input}`);
  if (existsSync(output) || existsSync(`${output}.partial`)) {
    throw new Error(`refusing to overwrite existing index ${output}`);
  }
  const partial = `${output}.partial`;
  const db = new DatabaseSync(partial);
  try {
    db.exec(`
      PRAGMA journal_mode=OFF;
      PRAGMA synchronous=OFF;
      PRAGMA locking_mode=EXCLUSIVE;
      PRAGMA temp_store=MEMORY;
      PRAGMA cache_size=-1048576;
      CREATE TABLE metadata (key TEXT PRIMARY KEY, value TEXT NOT NULL);
      CREATE TABLE events (
        session_id TEXT NOT NULL,
        sequence INTEGER NOT NULL,
        capture_sequence INTEGER,
        observed_micros INTEGER,
        event_kind TEXT NOT NULL,
        source_entity_uuid INTEGER,
        direct_source_entity_uuid INTEGER,
        target_entity_uuid INTEGER,
        provider_entity_uuid INTEGER,
        affected_entity_uuid INTEGER,
        action_id INTEGER,
        skill_effect_group_uuid TEXT,
        hit_event_id INTEGER,
        owner_id INTEGER,
        owner_level INTEGER,
        owner_stage INTEGER,
        damage_source INTEGER,
        damage_type INTEGER,
        type_flags INTEGER,
        normal_value INTEGER,
        lucky_value INTEGER,
        normal_hit INTEGER,
        property INTEGER,
        reported_amount INTEGER,
        actual_amount INTEGER,
        effect_id INTEGER,
        status_instance_id INTEGER,
        status_state TEXT,
        source_type_id INTEGER,
        source_config_id INTEGER,
        status_stacks INTEGER,
        status_duration_millis INTEGER,
        status_level INTEGER,
        filtered_effect_join TEXT,
        PRIMARY KEY (session_id, sequence)
      ) WITHOUT ROWID;
    `);
    const insert = db.prepare(`INSERT INTO events VALUES (
      ?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?
    )`);
    const stream = createReadStream(input);
    const hash = createHash("sha256");
    stream.on("data", (chunk) => hash.update(chunk));
    const lines = createInterface({ input: stream, crlfDelay: Infinity });
    let rows = 0;
    let relationshipRows = 0;
    db.exec("BEGIN");
    for await (const line of lines) {
      rows += 1;
      if (!line.trim()) continue;
      const row = JSON.parse(line);
      if (row.row_type !== "relationship") continue;
      insert.run(
        text(row.session_id), integer(row.sequence), integer(row.capture_sequence),
        integer(row.observed_micros), text(row.event_kind), integer(row.source_entity_uuid),
        integer(row.direct_source_entity_uuid), integer(row.target_entity_uuid),
        integer(row.provider_entity_uuid), integer(row.affected_entity_uuid), integer(row.action_id),
        text(row.skill_effect_group_uuid), integer(row.hit_event_id), integer(row.owner_id),
        integer(row.owner_level), integer(row.owner_stage), integer(row.damage_source),
        integer(row.damage_type), integer(row.type_flags), integer(row.normal_value),
        integer(row.lucky_value), booleanInteger(row.normal_hit), integer(row.property),
        integer(row.reported_amount), integer(row.actual_amount), integer(row.effect_id),
        integer(row.status_instance_id), text(row.status_state), integer(row.source_type_id),
        integer(row.source_config_id), integer(row.status_stacks),
        integer(row.status_duration_millis), integer(row.status_level), text(row.filtered_effect_join),
      );
      relationshipRows += 1;
      if (relationshipRows % 100_000 === 0) {
        db.exec("COMMIT; BEGIN");
        process.stdout.write(`indexed ${relationshipRows.toLocaleString()} relationship rows\r`);
      }
    }
    db.exec("COMMIT");
    const sourceSha256 = hash.digest("hex");
    const metadata = db.prepare("INSERT INTO metadata(key,value) VALUES (?,?)");
    for (const [key, item] of Object.entries({
      schema_version: "1",
      generated_by: "tools/bpsr-relationship-index-sqlite.mjs",
      source_bytes: String(statSync(input).size),
      source_sha256: sourceSha256,
      jsonl_rows: String(rows),
      relationship_rows: String(relationshipRows),
      exact_numeric_ids_authoritative: "true",
      attribution_authority: "false",
    })) metadata.run(key, item);
    db.exec(`
      CREATE INDEX status_effect_lifecycle ON events(effect_id, session_id, sequence)
        WHERE effect_id IS NOT NULL;
      CREATE INDEX provider_recipient_lifecycle ON events(
        effect_id, provider_entity_uuid, affected_entity_uuid, session_id, sequence
      ) WHERE effect_id IS NOT NULL;
      CREATE INDEX source_damage_timeline ON events(
        source_entity_uuid, session_id, sequence, action_id
      ) WHERE event_kind='damage';
      CREATE INDEX target_damage_timeline ON events(
        target_entity_uuid, session_id, sequence, action_id
      ) WHERE event_kind='damage';
      CREATE INDEX event_time_timeline ON events(session_id, observed_micros, sequence);
      ANALYZE;
    `);
    const integrity = db.prepare("PRAGMA integrity_check").get();
    if (integrity.integrity_check !== "ok") throw new Error("SQLite integrity check failed");
    db.close();
    renameSync(partial, output);
    console.log(`\nindexed ${relationshipRows.toLocaleString()} relationships from ${rows.toLocaleString()} JSONL rows`);
    console.log(JSON.stringify({ output, source_sha256: sourceSha256, relationship_rows: relationshipRows }));
  } catch (error) {
    try { db.close(); } catch {}
    if (existsSync(partial)) rmSync(partial, { force: true });
    throw error;
  }
}

function verify(value) {
  const input = path.resolve(required(value, "input"));
  const db = new DatabaseSync(input, { readOnly: true });
  try {
    const integrity = db.prepare("PRAGMA integrity_check").get();
    if (integrity.integrity_check !== "ok") throw new Error("SQLite integrity check failed");
    const metadata = Object.fromEntries(
      db.prepare("SELECT key,value FROM metadata ORDER BY key").all().map((row) => [row.key, row.value]),
    );
    const count = db.prepare("SELECT count(*) AS count FROM events").get().count;
    if (String(count) !== metadata.relationship_rows) throw new Error("relationship row count mismatch");
    console.log(JSON.stringify({ input, relationship_rows: Number(count), metadata }, null, 2));
  } finally {
    db.close();
  }
}

function parseArgs(values) {
  const result = {};
  for (let index = 0; index < values.length; index += 2) {
    const token = values[index];
    const next = values[index + 1];
    if (!token?.startsWith("--") || !next || next.startsWith("--")) {
      throw new Error(`invalid argument near ${token ?? "<end>"}`);
    }
    result[token.slice(2)] = next;
  }
  return result;
}

function required(value, key) {
  if (!value[key]) throw new Error(`missing --${key}`);
  return value[key];
}

function integer(value) {
  if (Number.isSafeInteger(value)) return value;
  if (typeof value === "string" && /^-?\d+$/.test(value)) {
    const parsed = BigInt(value);
    if (parsed >= -(1n << 63n) && parsed <= (1n << 63n) - 1n) return parsed;
  }
  return null;
}

function booleanInteger(value) {
  return typeof value === "boolean" ? Number(value) : null;
}

function text(value) {
  return typeof value === "string" ? value : null;
}

function usage(exitCode) {
  console.log("Usage: node tools/bpsr-relationship-index-sqlite.mjs build --input <timeline.jsonl> --output <index.sqlite>\n       node tools/bpsr-relationship-index-sqlite.mjs verify --input <index.sqlite>");
  process.exit(exitCode);
}
