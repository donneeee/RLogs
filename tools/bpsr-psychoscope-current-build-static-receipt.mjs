import assert from "node:assert/strict";
import { createHash } from "node:crypto";
import { readFileSync, writeFileSync } from "node:fs";
import path from "node:path";

const GENERATED_BY = "tools/bpsr-psychoscope-current-build-static-receipt.mjs";

function fail(message) {
  throw new Error(message);
}

function parseOptions(argv) {
  const options = {};
  for (let index = 0; index < argv.length; index += 2) {
    const key = argv[index];
    const value = argv[index + 1];
    if (!key?.startsWith("--") || value === undefined) fail(`invalid option near ${key ?? "<end>"}`);
    options[key.slice(2)] = value;
  }
  return options;
}

function required(options, key) {
  const value = options[key];
  if (!value) fail(`missing --${key}`);
  return value;
}

function sha256(bytes) {
  return createHash("sha256").update(bytes).digest("hex");
}

function stable(value) {
  if (Array.isArray(value)) return value.map(stable);
  if (value && typeof value === "object") {
    return Object.fromEntries(Object.keys(value).sort().map((key) => [key, stable(value[key])]));
  }
  return value;
}

function contentSha256(value) {
  return sha256(Buffer.from(JSON.stringify(stable(value)), "utf8"));
}

function buildReceipt(options) {
  const gameBuild = required(options, "build");
  const inventoryPath = path.resolve(required(options, "inventory"));
  const bytes = readFileSync(inventoryPath);
  const inventory = JSON.parse(bytes.toString("utf8"));

  assert.equal(inventory.schemaVersion, 1);
  assert.equal(inventory.gameBuild, gameBuild);
  assert.equal(inventory.deployment, "global");
  assert.equal(inventory.channel, "steam");
  assert.equal(inventory.domain, "psychoscope-factors");
  assert.equal(inventory.policy?.candidateDataNeverAutoPromoted, true);
  assert.equal(inventory.policy?.packetReplayRequiredForRuntimeRules, true);
  assert.equal(inventory.policy?.allRowsRetained, true);
  assert.equal(inventory.policy?.unresolvedRowsHidden, false);
  assert.equal(inventory.summary?.missingRequiredCount, 0);
  assert.ok(inventory.summary?.sourceCount > 0);
  assert.ok(inventory.summary?.rowCount > 0);
  assert.ok(Array.isArray(inventory.proofSuites) && inventory.proofSuites.length > 0);

  const receipt = {
    schema_version: 1,
    generated_by: GENERATED_BY,
    game_build: gameBuild,
    deployment_id: "global",
    channel: "steam",
    promotion_state: "current-build-static-inventory-only",
    policy: {
      exact_numeric_ids_and_build_are_authoritative: true,
      localization_is_supporting_evidence_only: true,
      static_rows_are_not_runtime_activation_proof: true,
      packet_replay_required_for_runtime_rules: true,
      unresolved_rows_are_retained: true,
      active_factor_rules: 0,
      provider_rdps_credit_allowed: false,
      runtime_promotion_allowed: false,
      ui_rdps_display_allowed: false,
    },
    source: {
      path: path.relative(process.cwd(), inventoryPath).replaceAll("\\", "/"),
      bytes: bytes.length,
      sha256: sha256(bytes),
      generated_by: inventory.generatedBy,
      source_count: inventory.summary.sourceCount,
      row_count: inventory.summary.rowCount,
      aggregate_sha256: inventory.aggregateSha256,
    },
    proof_suites: inventory.proofSuites,
    open_proof_obligations: [
      "factor-event-correlation",
      "origin-graph-diff",
      "provider-recipient-replay",
      "canonical-replay-conservation",
    ],
    conclusion: {
      exact_current_build_static_inventory_present: true,
      semantic_factor_rules_proven_for_current_build: false,
      matching_build_runtime_activation_proven: false,
      provider_rdps_credit_allowed: false,
      runtime_promotion_allowed: false,
      ui_rdps_display_allowed: false,
    },
  };
  return { ...receipt, content_sha256: contentSha256(receipt) };
}

function generate(options) {
  const output = path.resolve(required(options, "output"));
  writeFileSync(output, `${JSON.stringify(buildReceipt(options), null, 2)}\n`, "utf8");
  console.log(output);
}

function verify(options) {
  const input = path.resolve(required(options, "input"));
  assert.deepEqual(JSON.parse(readFileSync(input, "utf8")), buildReceipt(options));
  console.log(input);
}

const [command, ...rest] = process.argv.slice(2);
if (command === "generate") generate(parseOptions(rest));
else if (command === "verify") verify(parseOptions(rest));
else {
  console.log("Usage:\n  node tools/bpsr-psychoscope-current-build-static-receipt.mjs generate --build <id> --inventory <json> --output <json>\n  node tools/bpsr-psychoscope-current-build-static-receipt.mjs verify --build <id> --inventory <json> --input <json>");
  process.exit(1);
}
