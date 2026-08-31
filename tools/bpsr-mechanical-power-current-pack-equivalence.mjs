#!/usr/bin/env node

import crypto from "node:crypto";
import fs from "node:fs";
import path from "node:path";

const SCHEMA_VERSION = 1;
const EFFECT_ID = 2_110_140;
const BUILD = "24687926";
const CANONICAL_LEDGER_FIELDS = [
  "observed_micros",
  "effect_id",
  "provider_actor_id",
  "provider_entity_uuid",
  "recipient_actor_id",
  "recipient_entity_uuid",
  "affected_damage_id",
  "damage_source_actor_id",
  "damage_source_entity_uuid",
  "target_actor_id",
  "target_entity_uuid",
  "numerator",
  "denominator",
  "observed_damage",
  "damage_context_complete",
  "formula_trace",
];

function assert(condition, message) {
  if (!condition) throw new Error(message);
}

function sha256(bytes) {
  return crypto.createHash("sha256").update(bytes).digest("hex").toUpperCase();
}

function canonicalize(value) {
  if (Array.isArray(value)) return value.map(canonicalize);
  if (value && typeof value === "object") {
    return Object.fromEntries(
      Object.keys(value)
        .sort()
        .map((key) => [key, canonicalize(value[key])]),
    );
  }
  return value;
}

function contentDigest(receipt) {
  const copy = structuredClone(receipt);
  delete copy.content_sha256;
  return sha256(JSON.stringify(canonicalize(copy)));
}

function loadJson(filePath) {
  const bytes = fs.readFileSync(filePath);
  return {
    value: JSON.parse(bytes.toString("utf8")),
    receipt: {
      path: path.resolve(filePath),
      bytes: bytes.length,
      sha256: sha256(bytes),
    },
  };
}

function report(audit, label) {
  assert(audit?.schema_version === 27, `${label} replay audit must use schema 27`);
  assert(audit?.reports?.length === 1, `${label} replay audit must contain one report`);
  assert(
    audit.attribution_mode === "offline_candidate_gate_audit_not_production_attribution",
    `${label} replay audit must remain audit-only`,
  );
  const selected = audit.reports[0];
  assert(selected.deployment_id === "global", `${label} deployment must be global`);
  assert(selected.client_build === BUILD, `${label} build must be ${BUILD}`);
  assert(selected.conserved === true, `${label} ordinary damage must be conserved`);
  assert(
    selected.emitted_contribution_events_by_effect?.[String(EFFECT_ID)] === 4_261,
    `${label} must contain exactly 4,261 effect-${EFFECT_ID} candidates`,
  );
  assert(
    selected.summary?.damage_event_count === 23_934 &&
      selected.summary?.attributed_damage_event_count === 4_261 &&
      selected.summary?.attributed_bonus_damage === 22_100_227,
    `${label} replay totals changed`,
  );
  assert(
    selected.emitted_contribution_ledger?.length === 4_261,
    `${label} contribution ledger length changed`,
  );
  return selected;
}

function normalizedLedger(selected) {
  return selected.emitted_contribution_ledger.map((row) =>
    CANONICAL_LEDGER_FIELDS.map((field) => row[field] ?? null),
  );
}

function ledgerDigest(rows) {
  return sha256(rows.map((row) => JSON.stringify(row)).join("\n"));
}

function generate(args) {
  const oldLoaded = loadJson(args.oldAudit);
  const currentLoaded = loadJson(args.currentAudit);
  const segmentLoaded = loadJson(args.segmentReport);
  const gapLoaded = loadJson(args.gapWindowAudit);
  const oldReport = report(oldLoaded.value, "old-pack");
  const currentReport = report(currentLoaded.value, "current-pack");

  assert(
    oldReport.protocol_pack_digest ===
      "sha256:c5902c7f1de05308abb9b3b2c34969ece9a38d8fb989ab5b5dd464b37e4e306b",
    "old replay protocol-pack identity changed",
  );
  assert(
    currentReport.protocol_pack_digest ===
      "sha256:f3a07130e33ea9f9ba3360920879ffc0a3def59ae0d31a9997f17cb99a218395",
    "current replay protocol-pack identity changed",
  );
  assert(oldReport.event_count === 151_164, "old run event count changed");
  assert(currentReport.event_count === 151_277, "current run event count changed");

  const oldLedger = normalizedLedger(oldReport);
  const currentLedger = normalizedLedger(currentReport);
  const oldLedgerSha = ledgerDigest(oldLedger);
  const currentLedgerSha = ledgerDigest(currentLedger);
  assert(oldLedgerSha === currentLedgerSha, "normalized contribution ledgers differ");
  assert(
    JSON.stringify(oldLedger) === JSON.stringify(currentLedger),
    "normalized contribution ledgers are not exactly ordered-equivalent",
  );

  const segment = segmentLoaded.value;
  assert(segment?.schema_version === 1, "run segmentation report schema changed");
  assert(
    segment.source?.header?.region?.client_build === BUILD &&
      segment.source?.header?.region?.protocol_pack_digest === currentReport.protocol_pack_digest,
    "run segmentation source identity does not match the current replay",
  );
  const run = segment.segments?.find((row) => row.session_id.endsWith(".run-0004"));
  assert(run, "current-pack run-0004 segment is missing");
  assert(
    run.event_count === currentReport.event_count &&
      run.started?.observed_micros === 403_903_844 &&
      run.ended?.observed_micros === 492_085_380 &&
      run.completed === true,
    "current-pack run-0004 boundary or seal changed",
  );

  const gap = gapLoaded.value;
  assert(gap?.schema_version === 3, "gap-window audit schema changed");
  assert(gap.game_build === BUILD, "gap-window build changed");
  assert(gap.effect_id === EFFECT_ID, "gap-window effect changed");
  assert(gap.damage_relationship === "source", "gap-window relationship must be damage_actor/source");
  assert(
    gap.summary?.selected_effect_status_event_count === 10 &&
      gap.summary?.selected_effect_complete_gap_bounded_lifecycle_count === 5 &&
      gap.summary?.selected_effect_complete_windows_with_damage_count === 5 &&
      gap.summary?.data_gap_count === 0 &&
      gap.summary?.selected_effect_lifecycles_cut_by_data_quality_boundary === 0,
    "current-pack lifecycle closure totals changed",
  );
  const recipientWindow = gap.sessions?.[0]?.complete_gap_bounded_windows?.find(
    (window) => window.target_actor_id === 7 && window.instance_id === 260,
  );
  assert(
    recipientWindow?.damage_events_while_active === 4_423,
    "Mechanical Power recipient window no longer contains 4,423 damage actions",
  );
  assert(
    gap.summary.formula_authority === false &&
      gap.summary.runtime_authority === false &&
      gap.summary.provider_rdps_credit_allowed === false,
    "gap-window evidence gained unsupported authority",
  );

  const receipt = {
    schema_version: SCHEMA_VERSION,
    generated_by: "tools/bpsr-mechanical-power-current-pack-equivalence.mjs",
    status: "exact-current-pack-replay-equivalence-proven-formula-authority-withheld",
    identity: {
      deployment_id: "global",
      game_build: BUILD,
      effect_id: EFFECT_ID,
      provider_actor_id: 5,
      provider_entity_uuid: "5424024453760",
      recipient_actor_id: 7,
      recipient_entity_uuid: "216009015936",
      recipient_class_id: 11,
      lifecycle_instance_id: 260,
    },
    topology: {
      effect_edge: "provider -> effect/status lifecycle -> recipient or enemy target",
      damage_edge: "recipient damage action -> recipient or enemy target",
      lifecycle_damage_join: "effect target equals damage actor",
      remote_player_cast_packet_required: false,
    },
    inputs: {
      old_pack_replay_audit: oldLoaded.receipt,
      current_pack_replay_audit: currentLoaded.receipt,
      current_pack_run_segmentation: segmentLoaded.receipt,
      current_pack_gap_window_audit: gapLoaded.receipt,
    },
    exact_scope: {
      old_protocol_pack_digest: oldReport.protocol_pack_digest,
      current_protocol_pack_digest: currentReport.protocol_pack_digest,
      old_run_event_count: oldReport.event_count,
      current_run_event_count: currentReport.event_count,
      run_started_observed_micros: run.started.observed_micros,
      run_ended_observed_micros: run.ended.observed_micros,
      damage_event_count: currentReport.summary.damage_event_count,
      effect_lifecycle_event_count: gap.summary.selected_effect_status_event_count,
      complete_gap_bounded_lifecycle_count:
        gap.summary.selected_effect_complete_gap_bounded_lifecycle_count,
      recipient_window_damage_event_count: recipientWindow.damage_events_while_active,
      candidate_action_count: currentReport.summary.attributed_damage_event_count,
      candidate_bonus_damage: currentReport.summary.attributed_bonus_damage,
      ordinary_damage_conserved: currentReport.conserved,
    },
    ledger_equivalence: {
      compared_fields: CANONICAL_LEDGER_FIELDS,
      excluded_transport_fields: [
        "sequence",
        "capture_sequence",
        "session_id",
        "protocol_pack_digest",
      ],
      reason_for_exclusion:
        "the protocol transition replay and per-run resequencing change transport-local ordinals while preserving observed time, exact actors, targets, actions, damage, and formula fractions",
      row_count: currentLedger.length,
      old_normalized_sha256: oldLedgerSha,
      current_normalized_sha256: currentLedgerSha,
      exact_ordered_equivalence: true,
    },
    authority: {
      exact_current_pack_lifecycle_replay_proven: true,
      exact_current_pack_candidate_ledger_equivalence_proven: true,
      ordinary_damage_conservation_proven_for_selected_run: true,
      formula_authority: false,
      runtime_authority: false,
      provider_rdps_credit_allowed: false,
      runtime_ui_promotion_allowed: false,
    },
    blockers: [
      "server AutoAttack damage-consumer operation order remains unproven",
      "server integer rounding for the exact AutoAttack operator remains unproven",
      "Mechanical Power tier and recipient generalization beyond this observed lifecycle remains unproven",
      "overlapping support-effect stacking and allocation remain unproven",
      "the aggregate rDPS runtime-promotion gate remains closed",
    ],
    content_sha256: "",
  };
  receipt.content_sha256 = contentDigest(receipt);
  verify(receipt);
  return receipt;
}

function verify(receipt) {
  assert(receipt?.schema_version === SCHEMA_VERSION, "unsupported receipt schema");
  assert(
    receipt.status === "exact-current-pack-replay-equivalence-proven-formula-authority-withheld",
    "unsafe receipt status",
  );
  assert(receipt.identity?.game_build === BUILD, "receipt build changed");
  assert(receipt.identity?.effect_id === EFFECT_ID, "receipt effect changed");
  assert(receipt.ledger_equivalence?.row_count === 4_261, "receipt ledger count changed");
  assert(receipt.ledger_equivalence?.exact_ordered_equivalence === true, "ledger equivalence missing");
  assert(
    receipt.ledger_equivalence.old_normalized_sha256 ===
      receipt.ledger_equivalence.current_normalized_sha256,
    "receipt ledger hashes differ",
  );
  assert(receipt.exact_scope?.candidate_bonus_damage === 22_100_227, "candidate total changed");
  assert(receipt.exact_scope?.ordinary_damage_conserved === true, "conservation proof missing");
  assert(receipt.authority?.formula_authority === false, "formula authority must remain false");
  assert(receipt.authority?.runtime_authority === false, "runtime authority must remain false");
  assert(
    receipt.authority?.provider_rdps_credit_allowed === false &&
      receipt.authority?.runtime_ui_promotion_allowed === false,
    "provider credit or UI promotion was enabled",
  );
  assert(receipt.content_sha256 === contentDigest(receipt), "receipt content digest mismatch");
}

function options(values) {
  const parsed = {};
  for (let index = 0; index < values.length; index += 2) {
    const flag = values[index];
    const value = values[index + 1];
    assert(flag?.startsWith("--") && value, `invalid option near ${flag ?? "end"}`);
    parsed[flag.slice(2)] = value;
  }
  return parsed;
}

function required(value, name) {
  assert(value, `--${name} is required`);
  return value;
}

function main() {
  const [command, ...rest] = process.argv.slice(2);
  if (command === "generate") {
    const parsed = options(rest);
    const output = required(parsed.output, "output");
    assert(!fs.existsSync(output), `refusing to overwrite ${output}`);
    const receipt = generate({
      oldAudit: required(parsed["old-audit"], "old-audit"),
      currentAudit: required(parsed["current-audit"], "current-audit"),
      segmentReport: required(parsed["segment-report"], "segment-report"),
      gapWindowAudit: required(parsed["gap-window-audit"], "gap-window-audit"),
    });
    fs.mkdirSync(path.dirname(path.resolve(output)), { recursive: true });
    fs.writeFileSync(output, `${JSON.stringify(receipt, null, 2)}\n`, { flag: "wx" });
    console.log(`wrote ${path.resolve(output)}`);
    return;
  }
  if (command === "verify") {
    const parsed = options(rest);
    const input = required(parsed.input, "input");
    verify(loadJson(input).value);
    console.log(`verified ${path.resolve(input)}`);
    return;
  }
  if (command === "self-test") {
    const fixture = {
      schema_version: SCHEMA_VERSION,
      status: "exact-current-pack-replay-equivalence-proven-formula-authority-withheld",
      identity: { game_build: BUILD, effect_id: EFFECT_ID },
      ledger_equivalence: {
        row_count: 4_261,
        exact_ordered_equivalence: true,
        old_normalized_sha256: "A",
        current_normalized_sha256: "A",
      },
      exact_scope: { candidate_bonus_damage: 22_100_227, ordinary_damage_conserved: true },
      authority: {
        formula_authority: false,
        runtime_authority: false,
        provider_rdps_credit_allowed: false,
        runtime_ui_promotion_allowed: false,
      },
      content_sha256: "",
    };
    fixture.content_sha256 = contentDigest(fixture);
    verify(fixture);
    const unsafe = structuredClone(fixture);
    unsafe.authority.formula_authority = true;
    unsafe.content_sha256 = contentDigest(unsafe);
    let rejected = false;
    try {
      verify(unsafe);
    } catch {
      rejected = true;
    }
    assert(rejected, "self-test did not reject unsafe formula authority");
    console.log("self-test passed");
    return;
  }
  throw new Error(
    "usage: bpsr-mechanical-power-current-pack-equivalence.mjs <generate|verify|self-test> ...",
  );
}

main();
