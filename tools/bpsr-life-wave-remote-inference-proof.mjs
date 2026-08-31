#!/usr/bin/env node

import assert from "node:assert/strict";
import crypto from "node:crypto";
import fs from "node:fs";
import path from "node:path";
import readline from "node:readline";

const GENERATED_BY = "tools/bpsr-life-wave-remote-inference-proof.mjs";
const SCHEMA_VERSION = 1;
const LIFE_WAVE_EFFECT_ID = 2_302_421;
const WINDOW_MILLIS = 5_000;
const MAX_PAIR_GAP_MILLIS = 30_000;
const MAX_CONFIGURED_BONUS_BASIS_POINTS = 1_000;
const MAX_EXAMPLES = 25;

const [command = "help", ...rest] = process.argv.slice(2);
const options = parseArgs(rest);

if (command === "generate") await generate(options);
else if (command === "verify") verify(path.resolve(required(options, "input")));
else if (command === "self-test") selfTest();
else usage(command === "help" ? 0 : 1);

async function generate(values) {
  const build = required(values, "build");
  if (!/^\d+$/.test(build)) throw new Error("--build must contain only ASCII digits");
  const timelinePath = path.resolve(required(values, "timeline"));
  const triggerProofPath = path.resolve(required(values, "trigger-proof"));
  const outputPath = path.resolve(required(values, "output"));
  requireFile(timelinePath, "timeline");
  requireFile(triggerProofPath, "trigger-proof");
  refuseExisting(outputPath);

  const triggerProof = readJson(triggerProofPath);
  validateTriggerProof(triggerProof, build);
  const observed = await readTimeline(timelinePath);
  const analysis = analyze(observed);
  const report = {
    schema_version: SCHEMA_VERSION,
    generated_by: GENERATED_BY,
    deployment_id: "global",
    channel: "steam",
    game_build: build,
    effect_id: LIFE_WAVE_EFFECT_ID,
    sources: {
      timeline: observed.receipt,
      trigger_proof: fileReceipt(triggerProofPath),
    },
    accounting_contract: {
      remote_character_snapshot_required: false,
      remote_loadout_required: false,
      cross_vantage_exact_evidence_preferred: true,
      cross_vantage_join_rule:
        "join only reports with the same exact-instance run group, build, protocol pack, and stable character identity",
      cross_vantage_damage_rule:
        "choose one canonical damage-event spine and use other observer uploads only as evidence witnesses; never sum duplicate combat events",
      inference_fallback_rule:
        "apply formula-bounded inference only when no exact same-run observer upload supplies the recipient state",
      selected_highest_stat_lane_required_for_direct_paired_output: false,
      exact_lane_formula_required_when_no_output_pair_exists: true,
      trigger_owner_source:
        "same-session, same-recipient, same-capture packet HP-change cohort from canonical healing events",
      active_output_source:
        "same-build packet-final damage emitted strictly inside the latest five-second Life Wave refresh window",
      inactive_output_source:
        "same-build packet-final damage for the same recipient/mechanic/target identity outside every Life Wave window",
      exact_direct_pair_rule:
        "active minus inactive only when all packet identity dimensions match, the inactive row is within 30 seconds, the active amount is larger, and the delta fits the configured 10-percentage-point upper envelope",
      chance_lane_rule:
        "Crit/Luck require occurrence-rate inference over matching packet mechanic cohorts; a single hit amount never proves chance contribution",
      haste_lane_rule:
        "Haste requires action-opportunity inference over bounded active/inactive exposure time; it is never treated as a per-hit multiplier",
      ambiguous_trigger_rule:
        "do not label one provider exact; retain the candidate set for a separately displayed inferred allocation",
      conservation:
        "every credited marginal transfers from the recipient's unchanged ordinary damage; candidate and inferred values cannot enter exact rDPS totals",
    },
    observed: analysis,
    conclusion: {
      remote_snapshot_dependency_removed: true,
      remote_packet_final_damage_available: analysis.summary.damage_rows_for_life_wave_wearers > 0,
      unique_external_trigger_windows_available:
        analysis.summary.unique_external_provider_windows > 0,
      exact_direct_pairs_available: analysis.direct_output.summary.accepted_pair_count > 0,
      occurrence_cohorts_available:
        analysis.occurrence.summary.cohorts_with_active_and_inactive_samples > 0,
      runtime_exact_promotion_allowed: false,
      inferred_display_path_required:
        analysis.direct_output.summary.unpaired_unique_external_active_damage_rows > 0 ||
        analysis.occurrence.summary.cohorts_with_active_and_inactive_samples > 0,
      remaining_gates: [
        "retain complete source and target status-context digests in the support timeline so direct pairs cannot cross a hidden buff transition",
        "promote accepted direct pairs only after exact replay proves provider/recipient/effect conservation",
        "fit Crit and Luck occurrence-stage posteriors with uncertainty and show them as inferred rather than exact",
        "fit Haste action-opportunity capacity only over encounter-stationary exposure windows",
        "replay exact-instance server run groups jointly so another participant's local state can replace remote inference without duplicating damage",
        "bind max-HP-only refreshes to their packet-observed provider when no healing event exists",
      ],
    },
  };
  report.content_sha256 = contentHash(report);
  verifyReport(report);
  fs.mkdirSync(path.dirname(outputPath), { recursive: true });
  fs.writeFileSync(outputPath, `${JSON.stringify(report, null, 2)}\n`, { flag: "wx" });
  process.stdout.write(`${JSON.stringify({
    output: outputPath,
    summary: report.observed.summary,
    direct_output: report.observed.direct_output.summary,
    occurrence: report.observed.occurrence.summary,
    conclusion: report.conclusion,
  }, null, 2)}\n`);
}

async function readTimeline(timelinePath) {
  const hash = crypto.createHash("sha256");
  const stream = fs.createReadStream(timelinePath);
  stream.on("data", (chunk) => hash.update(chunk));
  const lines = readline.createInterface({ input: stream, crlfDelay: Infinity });
  const activations = [];
  const healsByPacketTarget = new Map();
  const damage = [];
  let lineCount = 0;
  let relationshipRows = 0;
  for await (const line of lines) {
    lineCount += 1;
    if (!line.trim()) continue;
    const row = JSON.parse(line);
    if (row?.row_type !== "relationship") continue;
    relationshipRows += 1;
    if (row.event_kind === "healing" && row.target_entity_uuid && row.source_entity_uuid) {
      append(healsByPacketTarget, packetTargetKey(row), {
        sequence: number(row.sequence),
        source_actor_id: string(row.source_actor_id),
        source_entity_uuid: string(row.source_entity_uuid),
        target_entity_uuid: string(row.target_entity_uuid),
        action_id: number(row.action_id),
        amount: number(row.reported_amount),
        hp_loss: nullableNumber(row.hp_loss),
      });
      continue;
    }
    if (
      row.event_kind === "status" &&
      number(row.effect_id) === LIFE_WAVE_EFFECT_ID &&
      ["applied", "refreshed", "stacked"].includes(row.status_state)
    ) {
      activations.push({
        session_id: string(row.session_id),
        sequence: number(row.sequence),
        capture_sequence: number(row.capture_sequence),
        target_actor_id: string(row.affected_entity_actor_id),
        target_entity_uuid: string(row.affected_entity_uuid),
        instance_id: number(row.status_instance_id),
        created_at_millis: number(row.status_created_at_millis),
        duration_millis: number(row.status_duration_millis),
      });
      continue;
    }
    if (row.event_kind === "damage" && row.source_entity_uuid && row.target_entity_uuid) {
      damage.push({
        session_id: string(row.session_id),
        sequence: number(row.sequence),
        capture_sequence: number(row.capture_sequence),
        game_time_millis: number(row.game_time_millis),
        source_actor_id: string(row.source_actor_id),
        source_entity_uuid: string(row.source_entity_uuid),
        target_actor_id: string(row.damage_target_actor_id ?? row.target_actor_id),
        target_entity_uuid: string(row.damage_target_entity_uuid ?? row.target_entity_uuid),
        action_id: nullableNumber(row.action_id),
        hit_event_id: nullableNumber(row.hit_event_id),
        amount: number(row.reported_amount),
        critical: row.type_flags == null ? null : (number(row.type_flags) & 1) !== 0,
        lucky: row.lucky_value == null ? (row.normal_value == null ? null : false) : true,
        damage_mode: nullableNumber(row.damage_mode),
        damage_source: nullableNumber(row.damage_source),
        damage_type: nullableNumber(row.damage_type),
        property: nullableNumber(row.property),
        owner_id: nullableNumber(row.owner_id),
        owner_level: nullableNumber(row.owner_level),
        owner_stage: nullableNumber(row.owner_stage),
        type_flags: nullableNumber(row.type_flags),
        skill_effect_group_index: nullableNumber(row.skill_effect_group_index),
        skill_effect_component_index: nullableNumber(row.skill_effect_component_index),
        skill_effect_component_count: nullableNumber(row.skill_effect_component_count),
      });
    }
  }
  return {
    activations,
    healsByPacketTarget,
    damage,
    receipt: {
      path: timelinePath,
      bytes: fs.statSync(timelinePath).size,
      sha256: hash.digest("hex"),
      line_count: lineCount,
      relationship_row_count: relationshipRows,
    },
  };
}

function analyze(observed) {
  const deduplicatedActivations = deduplicateActivations(observed.activations);
  const windowsByWearer = new Map();
  const triggerCounts = {
    unique_external: 0,
    unique_self: 0,
    ambiguous: 0,
    none: 0,
  };
  for (const activation of deduplicatedActivations) {
    assert.equal(activation.duration_millis, WINDOW_MILLIS);
    const candidates = observed.healsByPacketTarget.get(packetTargetKey(activation)) ?? [];
    const providers = distinctProviders(candidates);
    let trigger_classification = "none";
    let provider = null;
    if (providers.length === 1) {
      provider = providers[0];
      trigger_classification =
        provider.entity_uuid === activation.target_entity_uuid ? "unique_self" : "unique_external";
    } else if (providers.length > 1) {
      trigger_classification = "ambiguous";
    }
    triggerCounts[trigger_classification] += 1;
    append(windowsByWearer, wearerKey(activation), {
      ...activation,
      expires_at_millis: activation.created_at_millis + WINDOW_MILLIS,
      trigger_classification,
      provider,
      candidate_providers: providers,
    });
  }
  for (const windows of windowsByWearer.values()) {
    windows.sort((a, b) => a.created_at_millis - b.created_at_millis || a.sequence - b.sequence);
  }

  const wearerEntities = new Set(
    deduplicatedActivations.map((activation) => activation.target_entity_uuid),
  );
  const classifiedDamage = [];
  for (const row of observed.damage) {
    if (!wearerEntities.has(row.source_entity_uuid) || row.amount <= 0) continue;
    const window = latestWindowAt(
      windowsByWearer.get(wearerKey(row)) ?? [],
      row.game_time_millis,
    );
    classifiedDamage.push({ ...row, window });
  }

  const directOutput = analyzeDirectOutput(classifiedDamage);
  const occurrence = analyzeOccurrence(classifiedDamage);
  return {
    summary: {
      activation_count: deduplicatedActivations.length,
      wearer_count: wearerEntities.size,
      damage_rows_for_life_wave_wearers: classifiedDamage.length,
      active_damage_rows: classifiedDamage.filter((row) => row.window).length,
      inactive_damage_rows: classifiedDamage.filter((row) => !row.window).length,
      unique_external_provider_windows: triggerCounts.unique_external,
      unique_self_windows: triggerCounts.unique_self,
      ambiguous_provider_windows: triggerCounts.ambiguous,
      no_packet_heal_candidate_windows: triggerCounts.none,
      remote_character_snapshot_rows_consumed: 0,
    },
    trigger_windows: triggerCounts,
    direct_output: directOutput,
    occurrence,
  };
}

function analyzeDirectOutput(rows) {
  const inactiveByKey = new Map();
  for (const row of rows.filter((candidate) => !candidate.window)) {
    append(inactiveByKey, mechanicKey(row, true), row);
  }
  for (const candidates of inactiveByKey.values()) {
    candidates.sort((a, b) => a.game_time_millis - b.game_time_millis || a.sequence - b.sequence);
  }
  const accepted = [];
  const rejected = Object.create(null);
  let uniqueExternalActiveDamageRows = 0;
  let unpairedUniqueExternalActiveDamageRows = 0;
  let acceptedPairCount = 0;
  let acceptedMarginalDamage = 0;
  for (const active of rows) {
    if (active.window?.trigger_classification !== "unique_external") continue;
    uniqueExternalActiveDamageRows += 1;
    const candidates = inactiveByKey.get(mechanicKey(active, true)) ?? [];
    const inactive = nearestByTime(candidates, active.game_time_millis);
    let gate = "accepted";
    if (!inactive) gate = "inactive_match_missing";
    else if (Math.abs(inactive.game_time_millis - active.game_time_millis) > MAX_PAIR_GAP_MILLIS) {
      gate = "pair_gap_exceeded";
    } else if (active.amount <= inactive.amount) gate = "non_positive_delta";
    else if (!fitsMagnitudeEnvelope(active.amount, active.amount - inactive.amount)) {
      gate = "configured_magnitude_envelope_exceeded";
    }
    if (gate !== "accepted") {
      rejected[gate] = (rejected[gate] ?? 0) + 1;
      unpairedUniqueExternalActiveDamageRows += 1;
      continue;
    }
    acceptedPairCount += 1;
    acceptedMarginalDamage += active.amount - inactive.amount;
    take(accepted, {
      active_sequence: active.sequence,
      inactive_sequence: inactive.sequence,
      recipient_actor_id: active.source_actor_id,
      recipient_entity_uuid: active.source_entity_uuid,
      provider_actor_id: active.window.provider.actor_id,
      provider_entity_uuid: active.window.provider.entity_uuid,
      target_actor_id: active.target_actor_id,
      target_entity_uuid: active.target_entity_uuid,
      action_id: active.action_id,
      hit_event_id: active.hit_event_id,
      active_damage: active.amount,
      inactive_damage: inactive.amount,
      marginal_damage: active.amount - inactive.amount,
      gap_millis: Math.abs(inactive.game_time_millis - active.game_time_millis),
      confidence: "candidate_exact_pending_status_context_digest",
    });
  }
  return {
    summary: {
      unique_external_active_damage_rows: uniqueExternalActiveDamageRows,
      accepted_pair_count: acceptedPairCount,
      accepted_marginal_damage: acceptedMarginalDamage,
      accepted_example_count: accepted.length,
      unpaired_unique_external_active_damage_rows: unpairedUniqueExternalActiveDamageRows,
      rejected_by_gate: rejected,
      examples_are_capped: acceptedPairCount > accepted.length,
      production_exact_authority: false,
    },
    examples: accepted,
  };
}

function analyzeOccurrence(rows) {
  const cohorts = new Map();
  for (const row of rows) {
    if (row.critical == null && row.lucky == null) continue;
    const key = occurrenceMechanicKey(row);
    const cohort = cohorts.get(key) ?? {
      recipient_actor_id: row.source_actor_id,
      recipient_entity_uuid: row.source_entity_uuid,
      action_id: row.action_id,
      hit_event_id: null,
      hit_event_identity_grouped: true,
      active: emptyOccurrence(),
      inactive: emptyOccurrence(),
    };
    observeOccurrence(row.window ? cohort.active : cohort.inactive, row);
    cohorts.set(key, cohort);
  }
  const comparable = [...cohorts.values()].filter(
    (cohort) => cohort.active.total > 0 && cohort.inactive.total > 0,
  );
  const reports = comparable
    .map((cohort) => ({
      ...cohort,
      critical_rate_delta: rateDelta(cohort.active.critical, cohort.active.total, cohort.inactive.critical, cohort.inactive.total),
      lucky_rate_delta: rateDelta(cohort.active.lucky, cohort.active.total, cohort.inactive.lucky, cohort.inactive.total),
      confidence: "descriptive_only_pending_bounded_posterior",
    }))
    .sort((a, b) => b.active.total + b.inactive.total - (a.active.total + a.inactive.total));
  return {
    summary: {
      cohort_count: cohorts.size,
      cohorts_with_active_and_inactive_samples: comparable.length,
      active_samples: comparable.reduce((sum, row) => sum + row.active.total, 0),
      inactive_samples: comparable.reduce((sum, row) => sum + row.inactive.total, 0),
      production_exact_authority: false,
    },
    largest_cohorts: reports.slice(0, MAX_EXAMPLES),
  };
}

function deduplicateActivations(activations) {
  const byKey = new Map();
  for (const activation of activations) {
    const key = [
      activation.session_id,
      activation.target_entity_uuid,
      activation.instance_id,
      activation.created_at_millis,
    ].join("|");
    if (!byKey.has(key)) byKey.set(key, activation);
  }
  return [...byKey.values()].sort(
    (a, b) => a.created_at_millis - b.created_at_millis || a.sequence - b.sequence,
  );
}

function distinctProviders(candidates) {
  const providers = new Map();
  for (const candidate of candidates) {
    providers.set(candidate.source_entity_uuid, {
      actor_id: candidate.source_actor_id,
      entity_uuid: candidate.source_entity_uuid,
    });
  }
  return [...providers.values()].sort((a, b) => a.entity_uuid.localeCompare(b.entity_uuid));
}

function latestWindowAt(windows, gameTimeMillis) {
  let selected = null;
  for (const window of windows) {
    if (window.created_at_millis >= gameTimeMillis) break;
    if (gameTimeMillis < window.expires_at_millis) selected = window;
    else selected = null;
  }
  return selected;
}

function mechanicKey(row, includeOutcome) {
  const values = [
    row.session_id,
    row.source_entity_uuid,
    row.target_entity_uuid,
    row.action_id,
    row.hit_event_id,
    row.damage_mode,
    row.damage_source,
    row.damage_type,
    row.property,
    row.owner_id,
    row.owner_level,
    row.owner_stage,
    row.skill_effect_group_index,
    row.skill_effect_component_index,
    row.skill_effect_component_count,
  ];
  if (includeOutcome) values.push(row.critical, row.lucky, row.type_flags);
  return values.join("|");
}

function occurrenceMechanicKey(row) {
  const values = [
    row.session_id,
    row.source_entity_uuid,
    row.action_id,
    row.damage_mode,
    row.damage_source,
    row.damage_type,
    row.property,
  ];
  return values.join("|");
}

function fitsMagnitudeEnvelope(activeDamage, delta) {
  return (
    delta > 0 &&
    BigInt(delta) * 10_000n <= BigInt(activeDamage) * BigInt(MAX_CONFIGURED_BONUS_BASIS_POINTS)
  );
}

function nearestByTime(rows, millis) {
  let best = null;
  for (const row of rows) {
    if (!best || Math.abs(row.game_time_millis - millis) < Math.abs(best.game_time_millis - millis)) {
      best = row;
    }
  }
  return best;
}

function emptyOccurrence() {
  return { total: 0, critical: 0, lucky: 0, critical_lucky: 0 };
}

function observeOccurrence(target, row) {
  target.total += 1;
  if (row.critical === true) target.critical += 1;
  if (row.lucky === true) target.lucky += 1;
  if (row.critical === true && row.lucky === true) target.critical_lucky += 1;
}

function rateDelta(activeSuccesses, activeTotal, inactiveSuccesses, inactiveTotal) {
  return {
    active_successes: activeSuccesses,
    active_total: activeTotal,
    inactive_successes: inactiveSuccesses,
    inactive_total: inactiveTotal,
    active_rate: activeSuccesses / activeTotal,
    inactive_rate: inactiveSuccesses / inactiveTotal,
    delta: activeSuccesses / activeTotal - inactiveSuccesses / inactiveTotal,
  };
}

function validateTriggerProof(proof, build) {
  assert.equal(Number(proof.schema_version), 3);
  assert.equal(proof.generated_by, "tools/bpsr-life-wave-trigger-proof.mjs");
  assert.equal(proof.game_build, build);
  assert.equal(Number(proof.mechanic.refreshable_window_buff_id), LIFE_WAVE_EFFECT_ID);
  assert.equal(proof.policy.remote_character_snapshot_required, false);
  assert.equal(proof.conclusion.runtime_promotion_allowed, false);
}

function verify(inputPath) {
  const report = readJson(inputPath);
  verifyReport(report);
  process.stdout.write(`${JSON.stringify({
    verified: true,
    input: inputPath,
    content_sha256: report.content_sha256,
  }, null, 2)}\n`);
}

function verifyReport(report) {
  assert.equal(report.schema_version, SCHEMA_VERSION);
  assert.equal(report.generated_by, GENERATED_BY);
  assert.equal(report.effect_id, LIFE_WAVE_EFFECT_ID);
  assert.equal(report.accounting_contract.remote_character_snapshot_required, false);
  assert.equal(report.accounting_contract.remote_loadout_required, false);
  assert.equal(report.accounting_contract.cross_vantage_exact_evidence_preferred, true);
  assert.equal(report.conclusion.remote_snapshot_dependency_removed, true);
  assert.equal(report.conclusion.runtime_exact_promotion_allowed, false);
  assert.equal(report.observed.summary.remote_character_snapshot_rows_consumed, 0);
  assert.equal(report.content_sha256, contentHash(report));
}

function selfTest() {
  const activations = [
    {
      session_id: "s",
      sequence: 1,
      capture_sequence: 10,
      target_actor_id: "1",
      target_entity_uuid: "wearer",
      instance_id: 7,
      created_at_millis: 1_000,
      duration_millis: WINDOW_MILLIS,
    },
  ];
  const healsByPacketTarget = new Map([
    ["s|10|wearer", [{ source_actor_id: "2", source_entity_uuid: "healer" }]],
  ]);
  const base = {
    session_id: "s",
    capture_sequence: 20,
    source_actor_id: "1",
    source_entity_uuid: "wearer",
    target_actor_id: "9",
    target_entity_uuid: "target",
    action_id: 100,
    hit_event_id: 1,
    damage_mode: 1,
    damage_source: null,
    damage_type: null,
    property: 3,
    owner_id: 100,
    owner_level: 1,
    owner_stage: null,
    type_flags: 0,
    skill_effect_group_index: 0,
    skill_effect_component_index: 0,
    skill_effect_component_count: 1,
    critical: false,
    lucky: false,
  };
  const damage = [
    { ...base, sequence: 2, game_time_millis: 500, amount: 1_000 },
    { ...base, sequence: 3, game_time_millis: 2_000, amount: 1_050 },
  ];
  const result = analyze({ activations, healsByPacketTarget, damage });
  assert.equal(result.summary.unique_external_provider_windows, 1);
  assert.equal(result.direct_output.summary.accepted_pair_count, 1);
  assert.equal(result.direct_output.examples[0].marginal_damage, 50);
  assert.equal(result.occurrence.summary.cohorts_with_active_and_inactive_samples, 1);
  const object = { schema_version: SCHEMA_VERSION, value: 1 };
  object.content_sha256 = contentHash(object);
  assert.equal(object.content_sha256, contentHash(object));
  process.stdout.write("bpsr-life-wave-remote-inference-proof self-test passed\n");
}

function packetTargetKey(row) {
  return [
    string(row.session_id),
    number(row.capture_sequence),
    string(row.target_entity_uuid ?? row.affected_entity_uuid),
  ].join("|");
}

function wearerKey(row) {
  return [string(row.session_id), string(row.source_entity_uuid ?? row.target_entity_uuid)].join("|");
}

function append(map, key, value) {
  const values = map.get(key) ?? [];
  values.push(value);
  map.set(key, values);
}

function take(values, value) {
  if (values.length < MAX_EXAMPLES) values.push(value);
}

function number(value) {
  const parsed = Number(value);
  if (!Number.isFinite(parsed)) throw new Error(`expected finite number, got ${value}`);
  return parsed;
}

function nullableNumber(value) {
  return value == null ? null : number(value);
}

function string(value) {
  if (value == null || value === "") throw new Error(`expected identity, got ${value}`);
  return String(value);
}

function parseArgs(args) {
  const result = Object.create(null);
  for (let index = 0; index < args.length; index += 1) {
    const token = args[index];
    if (!token.startsWith("--")) throw new Error(`unexpected positional argument: ${token}`);
    const key = token.slice(2);
    const value = args[index + 1];
    if (value == null || value.startsWith("--")) throw new Error(`${token} requires a value`);
    if (result[key] != null) throw new Error(`${token} was supplied more than once`);
    result[key] = value;
    index += 1;
  }
  return result;
}

function required(values, key) {
  const value = values[key];
  if (!value) throw new Error(`--${key} is required`);
  return value;
}

function requireFile(file, label) {
  if (!fs.statSync(file, { throwIfNoEntry: false })?.isFile()) {
    throw new Error(`${label} is not a file: ${file}`);
  }
}

function refuseExisting(file) {
  if (fs.existsSync(file)) throw new Error(`refusing to overwrite existing output: ${file}`);
}

function readJson(file) {
  return JSON.parse(fs.readFileSync(file, "utf8"));
}

function fileReceipt(file) {
  const bytes = fs.readFileSync(file);
  return {
    path: file,
    bytes: bytes.length,
    sha256: crypto.createHash("sha256").update(bytes).digest("hex"),
  };
}

function contentHash(value) {
  const clone = structuredClone(value);
  delete clone.content_sha256;
  return crypto.createHash("sha256").update(`${stableStringify(clone)}\n`).digest("hex");
}

function stableStringify(value) {
  if (Array.isArray(value)) return `[${value.map(stableStringify).join(",")}]`;
  if (value && typeof value === "object") {
    return `{${Object.keys(value)
      .sort()
      .map((key) => `${JSON.stringify(key)}:${stableStringify(value[key])}`)
      .join(",")}}`;
  }
  return JSON.stringify(value);
}

function usage(exitCode) {
  process.stderr.write(
    "Usage:\n" +
      "  node tools/bpsr-life-wave-remote-inference-proof.mjs generate --build <id> --timeline <support.jsonl> --trigger-proof <json> --output <json>\n" +
      "  node tools/bpsr-life-wave-remote-inference-proof.mjs verify --input <json>\n" +
      "  node tools/bpsr-life-wave-remote-inference-proof.mjs self-test\n",
  );
  process.exit(exitCode);
}
