import assert from "node:assert/strict";
import { createHash } from "node:crypto";
import { readFileSync, writeFileSync } from "node:fs";
import path from "node:path";

const GENERATED_BY = "tools/bpsr-rdps-protocol-suite-proof.mjs";
const SUITES = new Set(["protocol-event-coverage", "protobuf-coverage"]);

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
  const suiteId = required(options, "suite");
  assert.equal(SUITES.has(suiteId), true, `unsupported suite ${suiteId}`);
  const status = source(required(options, "protocol-status"));
  const audit = source(required(options, "protocol-audit"));
  const conservation = source(required(options, "conservation"));

  assert.equal(status.value.schema_version, 4);
  assert.equal(status.value.game_build, gameBuild);
  assert.equal(status.value.status, "promoted");
  assert.equal(status.value.audit?.promotion_ready, true);
  assert.equal(status.value.audit?.capture_gap_count, 0);
  assert.equal(status.value.audit?.observable_migrated_decoder_route_count, 11);
  assert.equal(status.value.audit?.validated_observable_migrated_decoder_route_count, 11);
  assert.equal(status.value.audit?.structural_non_obligation_route_count, 1);
  assert.equal(status.value.audit?.unvalidated_routes?.length, 0);

  assert.equal(audit.value.schema_version, 6);
  assert.equal(audit.value.build_id, gameBuild);
  assert.equal(audit.value.promotion_ready, true);
  assert.equal(audit.value.capture_gap_count, 0);
  assert.equal(audit.value.observable_migrated_decoder_route_count, 11);
  assert.equal(audit.value.validated_observable_migrated_decoder_route_count, 11);
  assert.equal(audit.value.structural_non_obligation_route_count, 1);
  assert.equal(audit.value.blockers?.length, 0);
  assert.equal(audit.value.route_audits?.length, 12);

  const observedRoutes = audit.value.route_audits.filter((route) => route.packet_count > 0);
  const structuralRoutes = audit.value.route_audits.filter((route) => route.packet_count === 0);
  assert.equal(observedRoutes.length, 11);
  assert.equal(structuralRoutes.length, 1);
  assert.equal(structuralRoutes[0].promotion_requirement_satisfied, true);
  assert.equal(observedRoutes.every((route) =>
    route.packet_count === route.decoded_records &&
    route.missing_application_payload_records === 0 &&
    route.decode_failed_records === 0 &&
    route.promotion_requirement_satisfied === true), true);
  const decodedRecords = observedRoutes.reduce((total, route) => total + route.decoded_records, 0);
  assert.ok(decodedRecords > 0);

  const segment = conservation.value.exact_pack_gap_free_segment;
  assert.equal(conservation.value.schema_version, 1);
  assert.equal(conservation.value.game_build, gameBuild);
  assert.equal(conservation.value.conclusion?.protocol_event_coverage_proven, true);
  assert.equal(segment?.capture_gaps, 0);
  assert.ok(segment?.packet_records > 0);
  assert.ok(segment?.canonical_events > 0);
  assert.ok(segment?.damage_events > 0);
  assert.equal(segment?.remote_cast_rows_synthesized, 0);
  assert.equal(segment?.ordinary_raw_damage, segment?.ordinary_rdps_damage);
  assert.equal(segment?.ordinary_damage_conserved, true);

  const observedEventCount = suiteId === "protocol-event-coverage"
    ? segment.packet_records : decodedRecords;
  const report = {
    schema_version: 1,
    generated_by: GENERATED_BY,
    suite_id: suiteId,
    game_build: gameBuild,
    policy: {
      observable_routes_require_matching_build_packets: true,
      structural_non_obligations_are_not_zero_events: true,
      remote_player_casts_are_not_synthesized: true,
      unknown_and_unresolved_events_are_retained: true,
      coverage_does_not_grant_formula_authority: true,
    },
    sources: {
      promoted_protocol_status: receipt(status),
      promotion_audit: receipt(audit),
      exact_pack_conservation_boundary: receipt(conservation),
    },
    coverage: {
      exact_pack_packet_records: segment.packet_records,
      canonical_events: segment.canonical_events,
      observable_decoder_routes_required: 11,
      observable_decoder_routes_validated: 11,
      structural_non_obligation_routes: 1,
      observed_route_packet_records: decodedRecords,
      decoded_route_records: decodedRecords,
      missing_application_payload_records: 0,
      decode_failed_records: 0,
      capture_gaps: 0,
      remote_cast_rows_synthesized: 0,
    },
    conservation: {
      observed_damage_events: segment.damage_events,
      ordinary_raw_damage: segment.ordinary_raw_damage,
      ordinary_rdps_damage: segment.ordinary_rdps_damage,
      exact_party_conservation: true,
      scope: "gap-free exact-pack observable protocol segment; closed lifecycle and formula replay remain separate",
    },
    conclusion: {
      suite_status: "passed",
      observed_event_count: observedEventCount,
      exact_party_conservation: true,
      observable_protocol_event_coverage_proven: true,
      protobuf_decoder_coverage_proven_for_observable_migrated_routes: true,
      closed_lifecycle_conservation_proven: false,
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
  console.log("Usage:\n  node tools/bpsr-rdps-protocol-suite-proof.mjs generate --build <id> --suite <protocol-event-coverage|protobuf-coverage> --protocol-status <json> --protocol-audit <json> --conservation <json> --output <json>\n  node tools/bpsr-rdps-protocol-suite-proof.mjs verify --build <id> --suite <id> --protocol-status <json> --protocol-audit <json> --conservation <json> --input <json>");
  process.exit(1);
}
