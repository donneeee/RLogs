#!/usr/bin/env node

import crypto from "node:crypto";
import fs from "node:fs";
import path from "node:path";
import { StringDecoder } from "node:string_decoder";

const [command, ...args] = process.argv.slice(2);
if (command === "build") {
  await build(parseArguments(args));
} else if (command === "verify") {
  const options = parseArguments(args);
  verify(readJson(required(options, "input")));
} else {
  usage();
  process.exitCode = 2;
}

async function build(options) {
  const gameBuild = required(options, "build");
  const tierProofPath = path.resolve(required(options, "tier-proof"));
  const snapshotPath = path.resolve(required(options, "recipient-snapshots"));
  const timelinePath = path.resolve(required(options, "support-timeline"));
  const outputPath = path.resolve(required(options, "output"));
  const tierProof = readJson(tierProofPath);
  const snapshotAudit = readJson(snapshotPath);

  requireExact(String(tierProof.game_build) === gameBuild, "tier-proof build");
  requireExact(tierProof.schema_version === 1, "tier-proof schema");
  requireExact(Number(tierProof.effect_id) === 2110140, "tier-proof effect");
  requireExact(
    tierProof.policy?.tier_resolution_is_occurrence_scoped === true,
    "tier-proof occurrence scope",
  );
  requireExact(
    tierProof.policy?.provider_tier_is_not_propagated_across_time_or_recipients === true,
    "tier-proof non-propagation policy",
  );
  requireExact(String(snapshotAudit.aggregate?.manifest_game_build) === gameBuild, "snapshot build");
  requireExact(snapshotAudit.aggregate?.schema_version === 10, "snapshot schema");

  const occurrences = (tierProof.resolved_lifecycle_occurrences ?? []).map((row) => ({
    ...row,
    target_entity_uuid: String(row.target_entity_uuid),
    provider_entity_uuid: String(row.provider_entity_uuid),
    status_instance_id: Number(row.status_instance_id),
    lifecycle_rows: [],
    apply: null,
    remove: null,
    damage_actions: [],
  }));
  requireExact(occurrences.length === 8, "tier-resolved lifecycle count");
  const occurrenceByKey = uniqueIndex(occurrences, occurrenceKey, "tier-resolved lifecycle");
  const relevantSessions = new Set(occurrences.map((row) => row.session_id));
  const relevantTargets = new Set(occurrences.map((row) => row.target_entity_uuid));
  const snapshotIndex = buildSnapshotIndex(snapshotAudit);
  const activeEffectInstances = new Map();
  const activeExactOccurrences = new Map();

  const timeline = await streamTimeline(timelinePath, relevantTargets, (row) => {
    if (!relevantSessions.has(row.session_id)) return;
    if (row.event_kind === "status" && Number(row.effect_id) === 2110140) {
      const target = String(row.affected_entity_uuid ?? row.recipient_or_enemy_target_entity_uuid);
      if (!relevantTargets.has(target) || row.status_instance_id == null) return;
      const activeKey = [row.session_id, target, Number(row.status_instance_id)].join("|");
      const compact = compactStatus(row);
      const exact = occurrenceByKey.get(activeKey);
      if (exact) {
        requireExact(
          String(row.provider_entity_uuid) === exact.provider_entity_uuid,
          `${activeKey} provider identity`,
        );
        exact.lifecycle_rows.push(compact);
        if (row.status_state === "applied") {
          requireExact(exact.apply == null, `${activeKey} unique apply`);
          exact.apply = compact;
          activeExactOccurrences.set(activeKey, exact);
        } else if (row.status_state === "removed") {
          requireExact(exact.remove == null, `${activeKey} unique remove`);
          exact.remove = compact;
        }
      }
      if (["applied", "refreshed", "stacked"].includes(row.status_state)) {
        activeEffectInstances.set(activeKey, compact);
      }
      if (["consumed", "removed"].includes(row.status_state)) {
        activeEffectInstances.delete(activeKey);
        activeExactOccurrences.delete(activeKey);
      }
      return;
    }
    if (row.event_kind !== "damage") return;
    const actor = String(row.damage_actor_entity_uuid ?? "");
    if (!relevantTargets.has(actor)) return;
    for (const [key, occurrence] of activeExactOccurrences) {
      if (occurrence.session_id !== row.session_id || occurrence.target_entity_uuid !== actor) continue;
      const concurrent = [...activeEffectInstances.entries()]
        .filter(([activeKey]) => activeKey.startsWith(`${row.session_id}|${actor}|`))
        .map(([activeKey, status]) => ({
          status_instance_id: Number(activeKey.slice(activeKey.lastIndexOf("|") + 1)),
          provider_entity_uuid: status.provider_entity_uuid,
        }))
        .sort((left, right) => left.status_instance_id - right.status_instance_id);
      occurrence.damage_actions.push(compactDamage(row, concurrent, key));
    }
  });

  const windows = occurrences.map((occurrence) => finalizeWindow(occurrence, snapshotIndex));
  windows.sort(compareWindows);
  const damageActions = windows.flatMap((window) => window.damage_actions);
  const completeWindows = windows.filter((window) => window.window_input_state === "complete");
  const report = {
    schema_version: 1,
    generated_by: "tools/rdps-imagine-tier-window-counterfactual-inputs.mjs",
    game_build: gameBuild,
    effect_id: 2110140,
    imagine_skill_id: 3971,
    topology: {
      effect_edge: "provider -> effect/status lifecycle -> recipient or enemy target",
      damage_edge: "recipient damage action -> recipient or enemy target",
      source_side_join: "effect affected entity equals damage actor",
      allegiance_assumptions: false,
    },
    policy: {
      exact_numeric_ids_and_build_are_authoritative: true,
      localized_names_are_evidence_only: true,
      remote_player_cast_packets_required: false,
      remote_player_cast_packets_synthesized: false,
      status_tier_resolution_is_occurrence_scoped: true,
      tier_propagation_across_lifecycles_or_recipients: false,
      damage_actions_are_retained_counterfactual_inputs_only: true,
      damage_endpoint_is_assumed_enemy: false,
      damage_endpoint_is_assumed_friendly: false,
      integer_damage_stage_order_and_rounding_proven: false,
      ordinary_damage_totals_changed: false,
      observed_damage_reassigned_to_provider: 0,
      formula_authority: false,
      runtime_authority: false,
      ui_display_authority: false,
      provider_rdps_credit_allowed: false,
    },
    inputs: {
      tier_proof: receipt(tierProofPath),
      recipient_snapshots: receipt(snapshotPath),
      support_timeline: timeline.receipt,
    },
    join_contract: {
      lifecycle:
        "same session, effect 2110140, status instance, provider entity, affected entity, and canonical apply-before-remove sequence",
      recipient_snapshot:
        "same session, exact application trigger sequence, target actor, event-time class, and complete class-selected primary/attack values observed before the trigger",
      damage_action:
        "same session and effect affected entity equals canonical damage actor while the exact status instance is active; damage endpoint remains allegiance-neutral",
      concurrency:
        "all concurrently active effect 2110140 instances on that damage actor are retained per action; no provider is selected when concurrency is ambiguous",
    },
    summary: {
      tier_resolved_lifecycles: windows.length,
      exact_apply_remove_windows: windows.filter((row) => row.lifecycle_state === "exact-apply-remove").length,
      complete_window_inputs: completeWindows.length,
      unresolved_window_inputs: windows.length - completeWindows.length,
      retained_recipient_damage_actions: damageActions.length,
      retained_hp_loss: sumIntegerField(damageActions, "hp_loss"),
      retained_reported_damage: sumIntegerField(damageActions, "reported_amount"),
      single_effect_provider_damage_actions: damageActions.filter(
        (row) => row.concurrent_effect_2110140_instances.length === 1,
      ).length,
      concurrent_effect_provider_damage_actions: damageActions.filter(
        (row) => row.concurrent_effect_2110140_instances.length !== 1,
      ).length,
      observed_damage_reassigned_to_provider: 0,
    },
    lifecycle_windows: windows,
    remaining_proof_obligations: [
      "prove the exact current-build primary raw-percent evaluation base and integer operation order at each retained application",
      "prove the exact current-build attack-family update order after the primary-stat delta",
      "prove each retained damage action's downstream damage-stage formula and integer rounding",
      "resolve tiers and recipient snapshots independently for the other 128 effect 2110140 applications",
      "prove recipient debit equals provider credit while preserving ordinary damage totals",
      "satisfy canonical-replay-conservation and protocol-event-coverage with the exact-build protocol-pack identity",
    ],
  };
  verify(report);
  fs.writeFileSync(outputPath, `${JSON.stringify(report, null, 2)}\n`);
  console.log(
    `wrote ${outputPath}: ${completeWindows.length}/${windows.length} complete window inputs, ${damageActions.length} retained damage actions, zero provider credit`,
  );
}

async function streamTimeline(filePath, relevantTargets, onRelevantRow) {
  const hash = crypto.createHash("sha256");
  const decoder = new StringDecoder("utf8");
  const stream = fs.createReadStream(filePath, { highWaterMark: 8 * 1024 * 1024 });
  const damageActorNeedles = [...relevantTargets].map(
    (target) => `"damage_actor_entity_uuid":"${target}"`,
  );
  let carry = "";
  let currentSession = null;
  let manifest = null;
  let bytes = 0;
  let lines = 0;
  for await (const chunk of stream) {
    bytes += chunk.length;
    hash.update(chunk);
    const text = carry + decoder.write(chunk);
    let start = 0;
    for (;;) {
      const newline = text.indexOf("\n", start);
      if (newline < 0) break;
      const line = text.slice(start, newline).replace(/\r$/, "");
      start = newline + 1;
      lines += 1;
      ({ currentSession, manifest } = processTimelineLine(
        line,
        currentSession,
        manifest,
        damageActorNeedles,
        onRelevantRow,
      ));
    }
    carry = text.slice(start);
  }
  carry += decoder.end();
  if (carry.length > 0) {
    lines += 1;
    ({ currentSession, manifest } = processTimelineLine(
      carry.replace(/\r$/, ""),
      currentSession,
      manifest,
      damageActorNeedles,
      onRelevantRow,
    ));
  }
  requireExact([9, 10].includes(Number(manifest?.schema_version)), "support timeline schema");
  requireExact(
    manifest?.projection === "canonical-who-did-what-id-to-which-target-timeline",
    "support timeline projection",
  );
  requireExact(
    manifest?.policy?.damage_actor_and_damage_target_are_preserved_without_allegiance_assumptions ===
      true,
    "support timeline allegiance-neutral damage policy",
  );
  if (Number(manifest.schema_version) >= 10) {
    requireExact(
      manifest.policy?.packet_owner_stage_is_zero_based_stage_index_not_stage_type === true &&
        manifest.policy?.missing_packet_stage_or_level_is_preserved_as_null_not_zero === true &&
        manifest.policy?.packet_damage_stage_fields_grant_formula_authority === false,
      "support timeline packet-stage fail-closed policy",
    );
  }
  return {
    receipt: {
      path: filePath.replaceAll("\\", "/"),
      bytes,
      lines,
      sha256: hash.digest("hex"),
      schema_version: manifest.schema_version,
    },
  };
}

function processTimelineLine(line, currentSession, manifest, damageActorNeedles, onRelevantRow) {
  if (line.length === 0) return { currentSession, manifest };
  if (line.includes('"row_type":"manifest"')) {
    const row = JSON.parse(line);
    requireExact(manifest == null, "unique support timeline manifest");
    return { currentSession, manifest: row };
  }
  if (line.includes('"row_type":"run_header"')) {
    const row = JSON.parse(line);
    return { currentSession: String(row.session_id), manifest };
  }
  if (currentSession == null) return { currentSession, manifest };
  const statusCandidate = line.includes('"event_kind":"status"') &&
    line.includes('"effect_id":2110140');
  const damageCandidate = line.includes('"event_kind":"damage"') &&
    damageActorNeedles.some((needle) => line.includes(needle));
  if (!statusCandidate && !damageCandidate) return { currentSession, manifest };
  const row = JSON.parse(line);
  requireExact(String(row.session_id) === currentSession, "timeline run-header session grouping");
  onRelevantRow(row);
  return { currentSession, manifest };
}

function finalizeWindow(occurrence, snapshotIndex) {
  requireExact(occurrence.apply != null, `${occurrenceKey(occurrence)} observed apply`);
  requireExact(occurrence.remove != null, `${occurrenceKey(occurrence)} observed remove`);
  requireExact(
    occurrence.remove.capture_sequence === Number(occurrence.wire_capture_sequence),
    `${occurrenceKey(occurrence)} tier-proof removal capture sequence`,
  );
  requireExact(
    occurrence.remove.observed_micros === Number(occurrence.wire_observed_micros),
    `${occurrenceKey(occurrence)} tier-proof removal observation time`,
  );
  requireExact(
    occurrence.apply.sequence < occurrence.remove.sequence &&
      occurrence.apply.observed_micros <= occurrence.remove.observed_micros,
    `${occurrenceKey(occurrence)} ordered lifecycle`,
  );
  const lifecycleStates = occurrence.lifecycle_rows.map((row) => row.status_state);
  const lifecycleExact = JSON.stringify(lifecycleStates) === JSON.stringify(["applied", "removed"]);
  const snapshotKey = `${occurrence.session_id}|${occurrence.apply.sequence}`;
  const snapshots = snapshotIndex.get(snapshotKey) ?? [];
  const primary = snapshots.find(
    (row) => row.input_key === "recipient-class-selected-primary-final",
  );
  const attack = snapshots.find(
    (row) => row.input_key === "recipient-class-selected-attack-final",
  );
  const snapshotComplete = [primary, attack].every(
    (row) => row?.state === "complete" && row.values?.length === 1 &&
      String(row.actor_id) === String(occurrence.apply.affected_entity_actor_id),
  );
  const classAgreement = snapshotComplete && Number(primary.class_id) === Number(attack.class_id);
  const transform = classAgreement && Number(primary.class_id) === 11 &&
    Number(primary.values[0].attribute_id) === 11030 &&
    Number(attack.values[0].attribute_id) === 11330;
  const complete = lifecycleExact && snapshotComplete && classAgreement && transform;
  return {
    session_id: occurrence.session_id,
    run_ordinal: occurrence.run_ordinal,
    effect_id: 2110140,
    status_instance_id: occurrence.status_instance_id,
    provider_entity_uuid: occurrence.provider_entity_uuid,
    provider_character_id: occurrence.provider_character_id,
    loadout_tier: occurrence.loadout_tier,
    exact_attribute_pair: occurrence.exact_attribute_pair,
    affected_entity_actor_id: occurrence.apply.affected_entity_actor_id,
    affected_entity_uuid: occurrence.target_entity_uuid,
    lifecycle_state: lifecycleExact ? "exact-apply-remove" : "unresolved-lifecycle-mutation",
    lifecycle_rows: occurrence.lifecycle_rows,
    application_sequence: occurrence.apply.sequence,
    removal_sequence: occurrence.remove.sequence,
    application_observed_micros: occurrence.apply.observed_micros,
    removal_observed_micros: occurrence.remove.observed_micros,
    observed_window_micros: occurrence.remove.observed_micros - occurrence.apply.observed_micros,
    recipient_formula_input_snapshot: {
      state: snapshotComplete && classAgreement && transform ? "complete" : "unresolved",
      class_id: classAgreement ? Number(primary.class_id) : null,
      primary: primary ?? null,
      attack: attack ?? null,
    },
    damage_actions: occurrence.damage_actions,
    damage_action_count: occurrence.damage_actions.length,
    retained_hp_loss: sumIntegerField(occurrence.damage_actions, "hp_loss"),
    retained_reported_damage: sumIntegerField(occurrence.damage_actions, "reported_amount"),
    window_input_state: complete ? "complete" : "unresolved",
    counterfactual_damage_delta: null,
    provider_rdps_credit: "0",
    provider_rdps_credit_allowed: false,
    blocker:
      "exact primary raw-percent evaluation base, attack update order, downstream damage stages, and integer rounding are not yet proven",
  };
}

function buildSnapshotIndex(snapshotAudit) {
  const obligation = snapshotAudit.aggregate?.obligations?.find(
    (entry) =>
      entry.obligation_id ===
      "imagine:3971:effect:2110140:recipient-class-selected-pre-effect-inputs",
  );
  requireExact(Boolean(obligation), "recipient snapshot obligation");
  const index = new Map();
  for (const row of obligation.formula_input_snapshots ?? []) {
    const key = `${row.session_id}|${row.trigger_sequence}`;
    if (!index.has(key)) index.set(key, []);
    index.get(key).push(row);
  }
  return index;
}

function compactStatus(row) {
  return {
    sequence: Number(row.sequence),
    canonical_source_rlog_sequence: Number(row.canonical_source_rlog_sequence),
    capture_sequence: Number(row.capture_sequence),
    observed_micros: Number(row.observed_micros),
    game_time_millis: row.game_time_millis == null ? null : Number(row.game_time_millis),
    status_state: String(row.status_state),
    effect_id: Number(row.effect_id),
    status_instance_id: Number(row.status_instance_id),
    provider_actor_id: row.provider_actor_id == null ? null : String(row.provider_actor_id),
    provider_entity_uuid: row.provider_entity_uuid == null
      ? null
      : String(row.provider_entity_uuid),
    affected_entity_actor_id: row.affected_entity_actor_id == null
      ? null
      : String(row.affected_entity_actor_id),
    affected_entity_uuid: row.affected_entity_uuid == null
      ? null
      : String(row.affected_entity_uuid),
    status_stacks: row.status_stacks,
    status_level: row.status_level,
  };
}

function compactDamage(row, concurrent, occurrenceKeyValue) {
  return {
    lifecycle_occurrence_key: occurrenceKeyValue,
    sequence: Number(row.sequence),
    canonical_source_rlog_sequence: Number(row.canonical_source_rlog_sequence),
    capture_sequence: Number(row.capture_sequence),
    observed_micros: Number(row.observed_micros),
    game_time_millis: row.game_time_millis == null ? null : Number(row.game_time_millis),
    damage_actor_id: row.damage_actor_id == null ? null : String(row.damage_actor_id),
    damage_actor_entity_uuid: String(row.damage_actor_entity_uuid),
    action_id: row.action_id == null ? null : Number(row.action_id),
    action_instance_id: row.action_instance_id == null ? null : Number(row.action_instance_id),
    action_identity_resolution: row.action_identity_resolution,
    recipient_or_enemy_target_actor_id: row.recipient_or_enemy_target_actor_id == null
      ? null
      : String(row.recipient_or_enemy_target_actor_id),
    recipient_or_enemy_target_entity_uuid:
      row.recipient_or_enemy_target_entity_uuid == null
        ? null
        : String(row.recipient_or_enemy_target_entity_uuid),
    reported_amount: integerOrNull(row.reported_amount),
    hp_loss: integerOrNull(row.hp_loss),
    shield_loss: integerOrNull(row.shield_loss),
    actual_amount: integerOrNull(row.actual_amount),
    hit_event_id: row.hit_event_id == null ? null : Number(row.hit_event_id),
    owner_id: row.owner_id == null ? null : Number(row.owner_id),
    owner_level: row.owner_level == null ? null : Number(row.owner_level),
    owner_stage: row.owner_stage == null ? null : Number(row.owner_stage),
    damage_source: row.damage_source == null ? null : Number(row.damage_source),
    damage_type: row.damage_type == null ? null : Number(row.damage_type),
    type_flags: row.type_flags == null ? null : Number(row.type_flags),
    normal_value: integerOrNull(row.normal_value),
    lucky_value: integerOrNull(row.lucky_value),
    normal_hit: row.normal_hit == null ? null : Boolean(row.normal_hit),
    property: row.property == null ? null : Number(row.property),
    passive_uuid: row.passive_uuid == null ? null : Number(row.passive_uuid),
    rainbow: row.rainbow == null ? null : Boolean(row.rainbow),
    damage_mode: row.damage_mode == null ? null : Number(row.damage_mode),
    skill_effect_total_damage: integerOrNull(row.skill_effect_total_damage),
    skill_effect_group_index:
      row.skill_effect_group_index == null ? null : Number(row.skill_effect_group_index),
    skill_effect_component_index:
      row.skill_effect_component_index == null ? null : Number(row.skill_effect_component_index),
    skill_effect_component_count:
      row.skill_effect_component_count == null ? null : Number(row.skill_effect_component_count),
    direct_source_entity_uuid: row.direct_source_entity_uuid == null
      ? null
      : String(row.direct_source_entity_uuid),
    concurrent_effect_2110140_instances: concurrent,
    damage_endpoint_allegiance: "unresolved",
    provider_rdps_credit: "0",
  };
}

function verify(report) {
  requireExact(report.schema_version === 1, "report schema");
  requireExact(
    report.generated_by === "tools/rdps-imagine-tier-window-counterfactual-inputs.mjs",
    "report generator",
  );
  requireExact(Number(report.effect_id) === 2110140, "report effect");
  requireExact(report.topology?.allegiance_assumptions === false, "neutral topology");
  requireExact(report.policy?.remote_player_cast_packets_required === false, "remote cast policy");
  requireExact(
    report.policy?.damage_actions_are_retained_counterfactual_inputs_only === true,
    "counterfactual-only action policy",
  );
  requireExact(
    report.policy?.integer_damage_stage_order_and_rounding_proven === false,
    "damage-stage fail-closed policy",
  );
  requireExact(report.policy?.ordinary_damage_totals_changed === false, "ordinary damage conservation");
  requireExact(report.policy?.provider_rdps_credit_allowed === false, "credit policy");
  requireExact(Number(report.summary?.tier_resolved_lifecycles) === 8, "lifecycle count");
  requireExact(Number(report.summary?.exact_apply_remove_windows) === 8, "exact window count");
  requireExact(Number(report.summary?.complete_window_inputs) === 8, "complete input count");
  requireExact(Number(report.summary?.unresolved_window_inputs) === 0, "unresolved input count");
  requireExact(Number(report.summary?.observed_damage_reassigned_to_provider) === 0, "credit conservation");
  const windows = report.lifecycle_windows ?? [];
  requireExact(windows.length === 8, "retained windows");
  requireExact(
    windows.every(
      (window) => window.window_input_state === "complete" &&
        window.counterfactual_damage_delta == null &&
        window.provider_rdps_credit === "0" &&
        window.provider_rdps_credit_allowed === false &&
        window.retained_hp_loss === sumIntegerField(window.damage_actions, "hp_loss") &&
        window.retained_reported_damage ===
          sumIntegerField(window.damage_actions, "reported_amount") &&
        window.damage_actions.every(
          (action) =>
            action.damage_actor_entity_uuid === window.affected_entity_uuid &&
            action.sequence > window.application_sequence &&
            action.sequence < window.removal_sequence &&
            action.observed_micros >= window.application_observed_micros &&
            action.observed_micros <= window.removal_observed_micros &&
            action.concurrent_effect_2110140_instances.some(
              (status) =>
                Number(status.status_instance_id) === Number(window.status_instance_id) &&
                String(status.provider_entity_uuid) === String(window.provider_entity_uuid),
            ) &&
            action.damage_endpoint_allegiance === "unresolved" &&
            action.provider_rdps_credit === "0",
        ),
    ),
    "window authority and neutral action contract",
  );
  const actionCount = windows.reduce((sum, window) => sum + window.damage_actions.length, 0);
  requireExact(
    actionCount === Number(report.summary.retained_recipient_damage_actions),
    "retained action count",
  );
  const damageActions = windows.flatMap((window) => window.damage_actions);
  requireExact(
    report.summary.retained_hp_loss === sumIntegerField(damageActions, "hp_loss"),
    "retained hp-loss sum",
  );
  requireExact(
    report.summary.retained_reported_damage === sumIntegerField(damageActions, "reported_amount"),
    "retained reported-damage sum",
  );
  console.log(
    `verified effect 2110140 counterfactual inputs for build ${report.game_build}: 8 exact windows, ${actionCount} retained damage actions, zero provider credit`,
  );
  return report;
}

function occurrenceKey(row) {
  return [row.session_id, row.target_entity_uuid, Number(row.status_instance_id)].join("|");
}

function uniqueIndex(rows, keyOf, label) {
  const index = new Map();
  for (const row of rows) {
    const key = keyOf(row);
    requireExact(!index.has(key), `${label} ${key} unique`);
    index.set(key, row);
  }
  return index;
}

function compareWindows(left, right) {
  return left.session_id.localeCompare(right.session_id) ||
    left.application_sequence - right.application_sequence ||
    left.status_instance_id - right.status_instance_id;
}

function sumIntegerField(rows, field) {
  return rows.reduce((sum, row) => sum + BigInt(row[field] ?? "0"), 0n).toString();
}

function integerOrNull(value) {
  if (value == null) return null;
  requireExact(Number.isSafeInteger(Number(value)), "timeline integer amount");
  return String(value);
}

function receipt(filePath) {
  const bytes = fs.readFileSync(filePath);
  return {
    path: filePath.replaceAll("\\", "/"),
    bytes: bytes.length,
    sha256: crypto.createHash("sha256").update(bytes).digest("hex"),
  };
}

function readJson(filePath) {
  return JSON.parse(fs.readFileSync(filePath, "utf8"));
}

function parseArguments(values) {
  const result = {};
  for (let index = 0; index < values.length; index += 2) {
    const key = values[index];
    const value = values[index + 1];
    if (!key?.startsWith("--") || value == null) {
      usage();
      process.exit(2);
    }
    result[key.slice(2)] = value;
  }
  return result;
}

function required(options, key) {
  if (!options[key]) throw new Error(`missing --${key}`);
  return options[key];
}

function requireExact(condition, label) {
  if (!condition) throw new Error(`${label} does not match the exact proof contract`);
}

function usage() {
  console.log(`Usage:
  node tools/rdps-imagine-tier-window-counterfactual-inputs.mjs build --build <id> --tier-proof <json> --recipient-snapshots <json> --support-timeline <jsonl> --output <json>
  node tools/rdps-imagine-tier-window-counterfactual-inputs.mjs verify --input <json>`);
}
