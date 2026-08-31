#!/usr/bin/env node

import crypto from "node:crypto";
import fs from "node:fs";
import path from "node:path";

const SCHEMA_VERSION = 1;
const GENERATED_BY = "tools/bpsr-fake-bullet-lifecycle-frontier.mjs";
const GAME_BUILD = "24687926";
const DAMAGE_SOURCE_ID = 4;
const EVENT_SCHEMA_VERSION = 9;

const EXPECTED_FAKE_BULLET_FIELDS = [
  ["Uuid", "int", 1, 8],
  ["BulletId", "int", 2, 16],
  ["TargetId", "long", 3, 24],
  ["PartId", "int", 4, 32],
  ["Offset", "Vector3", 5, 42],
  ["Rotate", "Vector3", 6, 50],
  ["SkinId", "int", 7, 56],
];

function fail(message) {
  throw new Error(message);
}

function readJson(file, label) {
  try {
    return JSON.parse(fs.readFileSync(file, "utf8"));
  } catch (error) {
    fail(`Cannot read ${label} ${file}: ${error.message}`);
  }
}

function readText(file, label) {
  try {
    return fs.readFileSync(file, "utf8");
  } catch (error) {
    fail(`Cannot read ${label} ${file}: ${error.message}`);
  }
}

function canonical(value) {
  if (Array.isArray(value)) return `[${value.map(canonical).join(",")}]`;
  if (value && typeof value === "object") {
    return `{${Object.keys(value).sort().map((key) => `${JSON.stringify(key)}:${canonical(value[key])}`).join(",")}}`;
  }
  return JSON.stringify(value);
}

function digest(report) {
  const copy = structuredClone(report);
  delete copy.content_sha256;
  return crypto.createHash("sha256").update(canonical(copy)).digest("hex").toUpperCase();
}

function descriptor(file) {
  const bytes = fs.readFileSync(file);
  return {
    path: path.resolve(file).replaceAll("\\", "/"),
    bytes: bytes.length,
    sha256: crypto.createHash("sha256").update(bytes).digest("hex"),
  };
}

function requireFragments(text, label, fragments) {
  for (const fragment of fragments) {
    if (!text.includes(fragment)) fail(`${label} is missing required fail-closed fragment: ${fragment}`);
  }
}

function validateNativeWireProof(proof) {
  if (
    proof.schema_version !== 1 ||
    proof.generated_by !== "rlogs-bpsr-protobuf-native-wire-proof" ||
    proof.deployment !== "global" ||
    proof.channel !== "steam" ||
    String(proof.game_build) !== GAME_BUILD ||
    proof.policy?.packet_replay_required_for_semantics_and_occurrence !== true ||
    proof.source_identity?.game_assembly?.sha256 !==
      "4ba9e3f194bfd1769e57e3f12d192208e4d34db04374636738dfc9d5525495a4"
  ) fail("Native wire proof is unsafe or does not identify the exact current build");

  const delta = proof.messages?.find((message) => message.full_name === "Zproto.AoiSyncDelta");
  const repeated = delta?.fields?.find((field) => field.name === "FakeBullets");
  if (
    delta?.state !== "exact" ||
    repeated?.field_type !== "RepeatedField<FakeBulletInfo>" ||
    Number(repeated.protobuf_tag) !== 11 ||
    canonical(repeated.accepted_wire_keys_decimal) !== canonical([90]) ||
    repeated.proof_state !== "exact_native_merge_branch"
  ) fail("AoiSyncDelta.FakeBullets exact wire branch is missing");

  const message = proof.messages?.find((candidate) => candidate.full_name === "Zproto.FakeBulletInfo");
  if (
    message?.state !== "exact" ||
    Number(message.field_count) !== EXPECTED_FAKE_BULLET_FIELDS.length ||
    Number(message.exact_field_tags) !== EXPECTED_FAKE_BULLET_FIELDS.length ||
    (message.ambiguous_branches ?? []).length !== 0
  ) fail("FakeBulletInfo exact wire surface is incomplete");
  for (const [name, fieldType, tag, wireKey] of EXPECTED_FAKE_BULLET_FIELDS) {
    const field = message.fields.find((candidate) => candidate.name === name);
    if (
      field?.field_type !== fieldType ||
      Number(field.protobuf_tag) !== tag ||
      canonical(field.accepted_wire_keys_decimal) !== canonical([wireKey]) ||
      field.proof_state !== "exact_native_merge_branch"
    ) fail(`FakeBulletInfo.${name} exact wire contract changed`);
  }
}

function validateSourceContracts(sources) {
  requireFragments(sources.eventSchema, "event schema", [
    `pub const EVENT_SCHEMA_VERSION: u16 = ${EVENT_SCHEMA_VERSION};`,
  ]);
  requireFragments(sources.event, "canonical event model", [
    "UnresolvedAction(UnresolvedActionEvent)",
    "pub struct UnresolvedActionEvent",
    "pub container: Option<EntityRef>",
    "pub target: Option<EntityRef>",
    "pub action_instance_id: Option<i64>",
    "pub action_id: Option<i64>",
    "pub target_part_id: Option<i32>",
    "pub wire_action_type: Option<i32>",
    "pub raw_payload: Vec<u8>",
    "ProviderOwnershipUnproven",
  ]);
  requireFragments(sources.gameSchema, "BPSR protobuf schema", [
    "pub raw_fake_bullets: Vec<Vec<u8>>",
    "pub(crate) struct FakeBulletInfo",
    "pub uuid: Option<i32>",
    "pub bullet_id: Option<i32>",
    "pub target_id: Option<i64>",
    "pub part_id: Option<i32>",
    "pub offset: Option<FakeBulletVector3>",
    "pub rotate: Option<FakeBulletVector3>",
    "pub skin_id: Option<i32>",
  ]);
  requireFragments(sources.decoder, "BPSR canonical decoder", [
    "fn decode_unresolved_fake_bullets(",
    "schema::FakeBulletInfo::decode(raw_payload.as_slice())",
    "TimelineEventKind::UnresolvedAction(UnresolvedActionEvent",
    "UnresolvedActionReason::ProviderOwnershipUnproven",
    "raw_payload: raw_payload.clone()",
  ]);
  requireFragments(sources.damageProtocol, "BPSR damage-source enum", [
    "FakeBullet = 4",
    'Self::FakeBullet => "fake_bullet"',
  ]);
}

function build(options) {
  const nativeWireProof = readJson(options.nativeWireProof, "native wire proof");
  const sourceFiles = {
    eventSchema: options.eventSchema,
    event: options.event,
    gameSchema: options.gameSchema,
    decoder: options.decoder,
    damageProtocol: options.damageProtocol,
  };
  const sources = Object.fromEntries(
    Object.entries(sourceFiles).map(([name, file]) => [name, readText(file, name)]),
  );
  validateNativeWireProof(nativeWireProof);
  validateSourceContracts(sources);

  const report = {
    schema_version: SCHEMA_VERSION,
    generated_by: GENERATED_BY,
    game_build: GAME_BUILD,
    deployment: "global",
    channel: "steam",
    inputs: {
      native_wire_proof: descriptor(options.nativeWireProof),
      source_files: Object.fromEntries(
        Object.entries(sourceFiles).map(([name, file]) => [name, descriptor(file)]),
      ),
    },
    exact_wire_contract: {
      container_message: "Zproto.AoiSyncDelta",
      repeated_field: "FakeBullets",
      repeated_field_tag: 11,
      message: "Zproto.FakeBulletInfo",
      fields: EXPECTED_FAKE_BULLET_FIELDS.map(([name, field_type, protobuf_tag, wire_key]) => ({
        name,
        field_type,
        protobuf_tag,
        wire_key,
      })),
      damage_source_id: DAMAGE_SOURCE_ID,
      damage_source_name: "EDamageSourceFakeBullet",
      exact_build_wire_authority: true,
    },
    canonical_timeline_contract: {
      event_schema_version: EVENT_SCHEMA_VERSION,
      event_kind: "unresolved_action",
      container_entity_retained: true,
      container_entity_is_named_provider: false,
      action_instance_id_retained: true,
      numeric_action_id_retained: true,
      target_entity_retained: true,
      target_part_id_retained: true,
      raw_payload_retained: true,
      malformed_payload_retained: true,
      ordinary_damage_event_synthesized: false,
    },
    observed_evidence_frontier: {
      current_build_observed_fake_bullet_lifecycle_records: 0,
      current_build_observed_action_220101_fake_bullet_lifecycle_records: 0,
      historical_canonical_logs_backfilled: false,
      historical_logs_can_be_assumed_to_contain_discarded_fake_bullet_records: false,
      future_captures_can_preserve_exact_join_keys: true,
      enclosing_aoi_entity_is_provider_proven: false,
      recipient_scope_and_allegiance_proven: false,
      source4_to_damage_component_join_proven: false,
      operation_order_proven: false,
      integer_rounding_proven: false,
    },
    policy: {
      exact_numeric_ids_are_authoritative: true,
      localized_names_are_runtime_keys: false,
      container_entity_may_be_promoted_to_provider_without_proof: false,
      absent_historical_lifecycle_records_may_be_synthesized: false,
      unresolved_actions_hidden: false,
      formula_authority: false,
      runtime_authority: false,
      ui_display_authority: false,
      provider_rdps_credit_allowed: false,
    },
    summary: {
      exact_wire_fields: EXPECTED_FAKE_BULLET_FIELDS.length,
      canonical_join_keys_retained: 5,
      current_build_observed_lifecycle_records: 0,
      source4_damage_route_resolved: false,
      provider_ownership_proven: false,
      provider_rdps_credit_allowed: false,
    },
    next_action:
      "capture exact-build AoiSyncDelta.FakeBullets alongside source-4 damage, join by numeric instance/action/target keys, and prove whether the enclosing entity is the provider before any provider credit",
    content_sha256: "",
  };
  report.content_sha256 = digest(report);
  verify(report);
  return report;
}

function verify(report) {
  if (
    report.schema_version !== SCHEMA_VERSION ||
    report.generated_by !== GENERATED_BY ||
    String(report.game_build) !== GAME_BUILD ||
    report.exact_wire_contract?.exact_build_wire_authority !== true ||
    Number(report.exact_wire_contract?.damage_source_id) !== DAMAGE_SOURCE_ID ||
    Number(report.exact_wire_contract?.fields?.length) !== EXPECTED_FAKE_BULLET_FIELDS.length ||
    Number(report.canonical_timeline_contract?.event_schema_version) !== EVENT_SCHEMA_VERSION ||
    report.canonical_timeline_contract?.event_kind !== "unresolved_action" ||
    report.canonical_timeline_contract?.container_entity_retained !== true ||
    report.canonical_timeline_contract?.container_entity_is_named_provider !== false ||
    report.canonical_timeline_contract?.raw_payload_retained !== true ||
    report.canonical_timeline_contract?.malformed_payload_retained !== true ||
    report.canonical_timeline_contract?.ordinary_damage_event_synthesized !== false ||
    Number(report.observed_evidence_frontier?.current_build_observed_fake_bullet_lifecycle_records) !== 0 ||
    report.observed_evidence_frontier?.historical_canonical_logs_backfilled !== false ||
    report.observed_evidence_frontier?.future_captures_can_preserve_exact_join_keys !== true ||
    report.observed_evidence_frontier?.enclosing_aoi_entity_is_provider_proven !== false ||
    report.observed_evidence_frontier?.source4_to_damage_component_join_proven !== false ||
    report.policy?.container_entity_may_be_promoted_to_provider_without_proof !== false ||
    report.policy?.absent_historical_lifecycle_records_may_be_synthesized !== false ||
    report.policy?.unresolved_actions_hidden !== false ||
    report.policy?.formula_authority !== false ||
    report.policy?.ui_display_authority !== false ||
    report.policy?.provider_rdps_credit_allowed !== false ||
    Number(report.summary?.current_build_observed_lifecycle_records) !== 0 ||
    report.summary?.source4_damage_route_resolved !== false ||
    report.summary?.provider_ownership_proven !== false ||
    report.summary?.provider_rdps_credit_allowed !== false ||
    report.content_sha256 !== digest(report)
  ) fail("Fake-bullet lifecycle frontier is unsafe or has an invalid digest");
}

function parse(argv) {
  const args = {};
  for (let index = 0; index < argv.length; index += 2) {
    const flag = argv[index];
    const value = argv[index + 1];
    if (!flag?.startsWith("--") || value == null) fail(`Invalid argument ${flag ?? "<missing>"}`);
    args[flag.slice(2)] = value;
  }
  return args;
}

function required(args, name) {
  if (!args[name]) fail(`Missing --${name}`);
  return args[name];
}

function selfTest() {
  const sample = {
    schema_version: SCHEMA_VERSION,
    generated_by: GENERATED_BY,
    game_build: GAME_BUILD,
    exact_wire_contract: {
      exact_build_wire_authority: true,
      damage_source_id: DAMAGE_SOURCE_ID,
      fields: Array.from({ length: EXPECTED_FAKE_BULLET_FIELDS.length }, () => ({})),
    },
    canonical_timeline_contract: {
      event_schema_version: EVENT_SCHEMA_VERSION,
      event_kind: "unresolved_action",
      container_entity_retained: true,
      container_entity_is_named_provider: false,
      raw_payload_retained: true,
      malformed_payload_retained: true,
      ordinary_damage_event_synthesized: false,
    },
    observed_evidence_frontier: {
      current_build_observed_fake_bullet_lifecycle_records: 0,
      historical_canonical_logs_backfilled: false,
      future_captures_can_preserve_exact_join_keys: true,
      enclosing_aoi_entity_is_provider_proven: false,
      source4_to_damage_component_join_proven: false,
    },
    policy: {
      container_entity_may_be_promoted_to_provider_without_proof: false,
      absent_historical_lifecycle_records_may_be_synthesized: false,
      unresolved_actions_hidden: false,
      formula_authority: false,
      ui_display_authority: false,
      provider_rdps_credit_allowed: false,
    },
    summary: {
      current_build_observed_lifecycle_records: 0,
      source4_damage_route_resolved: false,
      provider_ownership_proven: false,
      provider_rdps_credit_allowed: false,
    },
    content_sha256: "",
  };
  sample.content_sha256 = digest(sample);
  verify(sample);
  sample.summary.provider_rdps_credit_allowed = true;
  try {
    verify(sample);
    fail("self-test accepted provider credit");
  } catch (error) {
    if (error.message === "self-test accepted provider credit") throw error;
  }
  console.log("bpsr-fake-bullet-lifecycle-frontier self-test passed");
}

const [command = "help", ...argv] = process.argv.slice(2);
try {
  if (command === "self-test") selfTest();
  else if (command === "verify") {
    const args = parse(argv);
    verify(readJson(path.resolve(required(args, "input")), "fake-bullet lifecycle frontier"));
    console.log("Fake-bullet lifecycle frontier verified");
  } else if (command === "build") {
    const args = parse(argv);
    const output = path.resolve(required(args, "output"));
    if (fs.existsSync(output)) fail(`Refusing to overwrite ${output}`);
    const report = build({
      nativeWireProof: required(args, "native-wire-proof"),
      eventSchema: required(args, "event-schema"),
      event: required(args, "event"),
      gameSchema: required(args, "game-schema"),
      decoder: required(args, "decoder"),
      damageProtocol: required(args, "damage-protocol"),
    });
    fs.writeFileSync(output, `${JSON.stringify(report, null, 2)}\n`, { flag: "wx" });
    console.log(JSON.stringify({ output, summary: report.summary, content_sha256: report.content_sha256 }, null, 2));
  } else {
    console.log(
      "Usage:\n  node tools/bpsr-fake-bullet-lifecycle-frontier.mjs build --native-wire-proof <json> --event-schema <rs> --event <rs> --game-schema <rs> --decoder <rs> --damage-protocol <rs> --output <json>\n  node tools/bpsr-fake-bullet-lifecycle-frontier.mjs verify --input <json>\n  node tools/bpsr-fake-bullet-lifecycle-frontier.mjs self-test",
    );
    process.exitCode = command === "help" ? 0 : 1;
  }
} catch (error) {
  console.error(error.stack ?? String(error));
  process.exitCode = 1;
}
