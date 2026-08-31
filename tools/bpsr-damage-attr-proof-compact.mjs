#!/usr/bin/env node

import { createHash } from "node:crypto";
import {
  createReadStream,
  existsSync,
  mkdirSync,
  mkdtempSync,
  readFileSync,
  renameSync,
  rmSync,
  statSync,
  writeFileSync,
} from "node:fs";
import { tmpdir } from "node:os";
import path from "node:path";
import { StringDecoder } from "node:string_decoder";

const [command = "help", ...rest] = process.argv.slice(2);
const options = parseArgs(rest);

if (command === "generate") await generate(options);
else if (command === "verify") verify(path.resolve(required(options, "input")));
else if (command === "self-test") await selfTest();
else usage(command === "help" ? 0 : 1);

async function generate(options) {
  const input = path.resolve(required(options, "input"));
  const output = path.resolve(required(options, "output"));
  requireFile(input, "damage attribute coefficient proof");

  const extracted = await readRetainedPrefix(input);
  const document = buildIndex(extracted.document, {
    path: input,
    bytes: extracted.bytes,
    sha256: extracted.sha256,
  });
  writeJsonAtomic(output, document);
  verifyDocument(document, output);

  console.log(`Compact damage evidence index created for ${document.game_build}.`);
  console.log(`Observed abilities: ${document.summary.observed_abilities}.`);
  console.log(`Observed damage rows: ${document.summary.observed_damage_rows}.`);
  console.log(`Indexed damage IDs: ${document.summary.indexed_damage_ids}.`);
  console.log(`Source proof bytes retained by digest: ${document.source.bytes}.`);
  console.log(`Wrote ${output}`);
}

async function readRetainedPrefix(input) {
  const decoder = new StringDecoder("utf8");
  const hash = createHash("sha256");
  let bytes = 0;
  let prefix = "";
  let markerFound = false;

  for await (const chunk of createReadStream(input, { highWaterMark: 1024 * 1024 })) {
    bytes += chunk.length;
    hash.update(chunk);
    if (markerFound) continue;
    prefix += decoder.write(chunk);
    const markerIndex = prefix.indexOf('"sibling_pairs":');
    if (markerIndex < 0) continue;
    const propertyLineStart = prefix.lastIndexOf("\n", markerIndex) + 1;
    prefix = prefix.slice(0, propertyLineStart).replace(/,\s*$/, "") + "\n}";
    markerFound = true;
  }

  if (!markerFound) {
    prefix += decoder.end();
    throw new Error("Damage proof does not contain the expected sibling_pairs boundary");
  }

  let document;
  try {
    document = JSON.parse(prefix);
  } catch (error) {
    throw new Error(`Cannot parse retained damage-proof prefix: ${error.message}`);
  }
  return { document, bytes, sha256: hash.digest("hex") };
}

function buildIndex(source, sourceFile) {
  const observedRows = Array.isArray(source.observed_damage_rows)
    ? source.observed_damage_rows
    : [];
  const observedAbilities = Array.isArray(source.observed_ability_result_kinds)
    ? source.observed_ability_result_kinds
    : [];
  const sessions = Array.isArray(source.sessions) ? source.sessions : [];
  const rowsById = {};
  for (const row of observedRows) {
    const damageId = String(row?.damage_id ?? "");
    if (!/^\d+$/.test(damageId)) throw new Error(`Invalid observed damage ID: ${damageId || "<missing>"}`);
    if (rowsById[damageId]) throw new Error(`Duplicate observed damage ID: ${damageId}`);
    rowsById[damageId] = row;
  }

  return {
    schema_version: 1,
    generated_by: "rlogs-bpsr-damage-attr-proof-compact",
    game_build: String(source.game_build ?? ""),
    packet_build: String(source.packet_build ?? ""),
    policy: {
      exact_packet_observation_index_only: true,
      static_identity_does_not_prove_transfer: true,
      unresolved_evidence_hidden: false,
      attribution_without_provider_formula_proof: "deferred",
      archived_run_recalculation_supported: true,
    },
    source: {
      path: sourceFile.path,
      bytes: sourceFile.bytes,
      sha256: sourceFile.sha256,
      schema_version: source.schema_version ?? null,
      generated_by: source.generated_by ?? null,
    },
    source_policy: source.policy ?? null,
    decoded_table_source: source.decoded_table_source ?? null,
    route_proof_source: source.route_proof_source ?? null,
    exact_family_result_kind_authority: source.exact_family_result_kind_authority ?? {},
    observed_ability_result_kinds: observedAbilities,
    observed_damage_rows: observedRows,
    observed_damage_rows_by_id: rowsById,
    sessions,
    coverage: source.coverage ?? {},
    summary: {
      observed_abilities: observedAbilities.length,
      observed_damage_rows: observedRows.length,
      indexed_damage_ids: Object.keys(rowsById).length,
      sessions: sessions.length,
      unresolved_evidence_hidden: false,
    },
  };
}

function verify(input) {
  requireFile(input, "compact damage evidence index");
  const document = JSON.parse(readFileSync(input, "utf8"));
  verifyDocument(document, input);
  console.log(
    `Compact damage evidence index verified: ${document.summary.indexed_damage_ids} damage IDs, zero hidden evidence.`,
  );
}

function verifyDocument(document, label) {
  if (document.schema_version !== 1) throw new Error(`${label}: unsupported schema version`);
  if (!/^\d+$/.test(document.game_build)) throw new Error(`${label}: invalid game build`);
  if (document.game_build !== document.packet_build) throw new Error(`${label}: packet build mismatch`);
  if (document.policy?.exact_packet_observation_index_only !== true) {
    throw new Error(`${label}: exact observation policy missing`);
  }
  if (document.policy?.static_identity_does_not_prove_transfer !== true) {
    throw new Error(`${label}: transfer-evidence policy missing`);
  }
  if (document.policy?.unresolved_evidence_hidden !== false) {
    throw new Error(`${label}: unresolved evidence must remain visible`);
  }
  if (document.policy?.attribution_without_provider_formula_proof !== "deferred") {
    throw new Error(`${label}: unsupported unresolved-attribution policy`);
  }
  if (!/^[a-f0-9]{64}$/.test(document.source?.sha256 ?? "")) {
    throw new Error(`${label}: invalid source SHA-256`);
  }
  const rows = document.observed_damage_rows ?? [];
  const rowsById = document.observed_damage_rows_by_id ?? {};
  if (rows.length !== Object.keys(rowsById).length) throw new Error(`${label}: damage-row index mismatch`);
  for (const row of rows) {
    if (rowsById[String(row.damage_id)]?.damage_id !== row.damage_id) {
      throw new Error(`${label}: missing exact damage-row index entry for ${row.damage_id}`);
    }
  }
  if (document.summary?.unresolved_evidence_hidden !== false) {
    throw new Error(`${label}: summary hides unresolved evidence`);
  }
}

async function selfTest() {
  const root = mkdtempSync(path.join(tmpdir(), "rlogs-damage-proof-compact-"));
  try {
    const input = path.join(root, "proof.json");
    const output = path.join(root, "index.json");
    writeFileSync(input, JSON.stringify({
      schema_version: 10,
      generated_by: "test",
      game_build: "24687926",
      packet_build: "24687926",
      policy: { unresolved_packet_evidence_is_hidden: false },
      exact_family_result_kind_authority: { Attack: { damage: 1, healing: 0 } },
      observed_ability_result_kinds: [{ ability_id: 1, packet_damage_results: 1 }],
      observed_damage_rows: [{ damage_id: "101", damage_script: "Attack" }],
      sessions: [{ session_id: "test" }],
      coverage: { observed: 1 },
      sibling_pairs: [],
      repeated_state_variation: { deliberately_omitted_from_compact_index: true },
    }, null, 2));
    const extracted = await readRetainedPrefix(input);
    const index = buildIndex(extracted.document, {
      path: input,
      bytes: extracted.bytes,
      sha256: extracted.sha256,
    });
    writeJsonAtomic(output, index);
    verify(output);
    if (index.observed_damage_rows_by_id[101]?.damage_script !== "Attack") {
      throw new Error("Self-test did not preserve the exact damage row");
    }
    if (Object.hasOwn(index, "repeated_state_variation")) {
      throw new Error("Self-test retained the intentionally excluded heavyweight evidence section");
    }
    console.log("Damage attribute compact-index self-test passed.");
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
}

function writeJsonAtomic(output, document) {
  mkdirSync(path.dirname(output), { recursive: true });
  const temporary = `${output}.tmp-${process.pid}`;
  writeFileSync(temporary, `${JSON.stringify(document, null, 2)}\n`);
  renameSync(temporary, output);
}

function requireFile(file, label) {
  if (!existsSync(file) || !statSync(file).isFile()) throw new Error(`${label} missing: ${file}`);
}

function parseArgs(args) {
  const result = {};
  for (let index = 0; index < args.length; index += 1) {
    const token = args[index];
    if (!token.startsWith("--")) throw new Error(`Unexpected argument: ${token}`);
    const key = token.slice(2);
    const value = args[index + 1];
    if (value === undefined || value.startsWith("--")) throw new Error(`Missing value for --${key}`);
    result[key] = value;
    index += 1;
  }
  return result;
}

function required(options, key) {
  const value = options[key];
  if (!value) throw new Error(`Missing required --${key}`);
  return value;
}

function usage(exitCode) {
  console.log("Usage:");
  console.log("  node tools/bpsr-damage-attr-proof-compact.mjs generate --input <proof.json> --output <index.json>");
  console.log("  node tools/bpsr-damage-attr-proof-compact.mjs verify --input <index.json>");
  console.log("  node tools/bpsr-damage-attr-proof-compact.mjs self-test");
  process.exit(exitCode);
}
