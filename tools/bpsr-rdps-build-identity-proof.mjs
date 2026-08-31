import assert from "node:assert/strict";
import { createHash } from "node:crypto";
import { readFileSync, writeFileSync } from "node:fs";
import path from "node:path";

const GENERATED_BY = "tools/bpsr-rdps-build-identity-proof.mjs";

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

function source(file) {
  const absolute = path.resolve(file);
  const bytes = readFileSync(absolute);
  return {
    path: path.relative(process.cwd(), absolute).replaceAll("\\", "/"),
    bytes: bytes.length,
    sha256: sha256(bytes),
    value: JSON.parse(bytes.toString("utf8")),
  };
}

function receipt(entry) {
  return { path: entry.path, bytes: entry.bytes, sha256: entry.sha256 };
}

function buildReport(options) {
  const gameBuild = required(options, "build");
  const pack = source(required(options, "pack"));
  const protocolStatus = source(required(options, "protocol-status"));
  const preflight = source(required(options, "preflight"));
  const snapshot = source(required(options, "snapshot"));
  const conservation = source(required(options, "conservation"));

  assert.equal(pack.value.schema_version, 1);
  assert.equal(pack.value.target?.build_id, gameBuild);
  assert.equal(pack.value.target?.deployment_id, "global");
  assert.equal(pack.value.target?.channel, "steam");
  assert.ok(Array.isArray(pack.value.routes) && pack.value.routes.length > 0);

  assert.equal(protocolStatus.value.schema_version, 4);
  assert.equal(protocolStatus.value.game_build, gameBuild);
  assert.equal(protocolStatus.value.status, "promoted");
  assert.equal(protocolStatus.value.promoted_pack?.present, true);
  assert.equal(protocolStatus.value.promoted_pack?.build_matches, true);
  assert.equal(protocolStatus.value.promoted_pack?.byte_identical_to_candidate, true);
  assert.equal(protocolStatus.value.promoted_pack?.pack_id, pack.value.pack_id);
  assert.equal(protocolStatus.value.promoted_pack?.sha256, pack.sha256);
  assert.equal(protocolStatus.value.audit?.promotion_ready, true);
  assert.equal(protocolStatus.value.audit?.capture_gap_count, 0);
  assert.equal(protocolStatus.value.audit?.validated_observable_migrated_decoder_route_count,
    protocolStatus.value.audit?.observable_migrated_decoder_route_count);
  assert.equal(protocolStatus.value.audit?.unvalidated_routes?.length, 0);

  assert.equal(preflight.value.schema_version, 1);
  assert.equal(preflight.value.game_build, gameBuild);
  assert.equal(preflight.value.ready_for_snapshot, true);
  assert.equal(preflight.value.runtime_promotion_allowed, false);
  assert.equal(preflight.value.summary?.missing_required_inputs, 0);
  assert.equal(preflight.value.summary?.present_required_inputs, 46);

  assert.equal(snapshot.value.schema_version, 1);
  assert.equal(snapshot.value.game_build, gameBuild);
  assert.equal(snapshot.value.promotion_state, "candidate");
  assert.equal(snapshot.value.policy?.canonical_events_retained, true);
  assert.equal(snapshot.value.policy?.unresolved_events_hidden, false);
  assert.equal(snapshot.value.inputs?.length, 47);
  assert.equal(snapshot.value.inputs.filter((entry) => entry.required).length, 46);

  const segment = conservation.value.exact_pack_gap_free_segment;
  assert.equal(conservation.value.schema_version, 1);
  assert.equal(conservation.value.game_build, gameBuild);
  assert.equal(conservation.value.installed_protocol_pack?.pack_id, pack.value.pack_id);
  assert.equal(conservation.value.installed_protocol_pack?.byte_identical_to_audited_candidate, true);
  assert.equal(conservation.value.conclusion?.protocol_pack_identity_installed, true);
  assert.equal(conservation.value.conclusion?.protocol_event_coverage_proven, true);
  assert.equal(segment?.capture_gaps, 0);
  assert.ok(segment?.damage_events > 0);
  assert.equal(segment?.ordinary_raw_damage, segment?.ordinary_rdps_damage);
  assert.equal(segment?.ordinary_damage_conserved, true);
  assert.equal(segment?.attributed_damage_events, 0);
  assert.equal(segment?.attributed_bonus_damage, 0);

  const report = {
    schema_version: 1,
    generated_by: GENERATED_BY,
    suite_id: "build-identity",
    game_build: gameBuild,
    policy: {
      exact_numeric_build_and_pack_identity_required: true,
      candidate_snapshot_is_not_runtime_promotion: true,
      unresolved_events_are_retained: true,
      ordinary_damage_may_not_change: true,
    },
    sources: {
      installed_pack: receipt(pack),
      promoted_protocol_status: receipt(protocolStatus),
      build_preflight: receipt(preflight),
      candidate_snapshot: receipt(snapshot),
      exact_pack_conservation_boundary: receipt(conservation),
    },
    identity: {
      pack_id: pack.value.pack_id,
      pack_file_sha256: pack.sha256,
      protocol_pack_digest: protocolStatus.value.candidate.audited_digest,
      route_count: pack.value.routes.length,
      observable_routes_required: protocolStatus.value.audit.observable_migrated_decoder_route_count,
      observable_routes_validated:
        protocolStatus.value.audit.validated_observable_migrated_decoder_route_count,
      structural_non_obligations: protocolStatus.value.audit.structural_non_obligation_route_count,
      required_snapshot_inputs: 46,
      total_snapshot_inputs: snapshot.value.inputs.length,
    },
    conservation: {
      observed_event_count: segment.damage_events,
      ordinary_raw_damage: segment.ordinary_raw_damage,
      ordinary_rdps_damage: segment.ordinary_rdps_damage,
      attributed_damage_events: segment.attributed_damage_events,
      attributed_bonus_damage: segment.attributed_bonus_damage,
      exact_party_conservation: true,
      scope: "gap-free exact-pack identity replay; no formula or closed-lifecycle authority",
    },
    conclusion: {
      suite_status: "passed",
      observed_event_count: segment.damage_events,
      exact_party_conservation: true,
      exact_build_identity_proven: true,
      exact_protocol_pack_identity_proven: true,
      candidate_snapshot_complete_for_required_inputs: true,
      formula_authority_proven: false,
      runtime_promotion_allowed: false,
    },
  };
  return { ...report, content_sha256: contentSha256(report) };
}

function generate(options) {
  const output = path.resolve(required(options, "output"));
  writeFileSync(output, `${JSON.stringify(buildReport(options), null, 2)}\n`, "utf8");
  console.log(output);
}

function verify(options) {
  const input = path.resolve(required(options, "input"));
  assert.deepEqual(JSON.parse(readFileSync(input, "utf8")), buildReport(options));
  console.log(input);
}

const [command, ...rest] = process.argv.slice(2);
if (command === "generate") generate(parseOptions(rest));
else if (command === "verify") verify(parseOptions(rest));
else {
  console.log("Usage:\n  node tools/bpsr-rdps-build-identity-proof.mjs generate --build <id> --pack <json> --protocol-status <json> --preflight <json> --snapshot <json> --conservation <json> --output <json>\n  node tools/bpsr-rdps-build-identity-proof.mjs verify --build <id> --pack <json> --protocol-status <json> --preflight <json> --snapshot <json> --conservation <json> --input <json>");
  process.exit(1);
}
