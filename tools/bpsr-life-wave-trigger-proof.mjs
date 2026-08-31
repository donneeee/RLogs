#!/usr/bin/env node

import assert from "node:assert/strict";
import crypto from "node:crypto";
import fs from "node:fs";
import path from "node:path";
import readline from "node:readline";

const GENERATED_BY = "tools/bpsr-life-wave-trigger-proof.mjs";
const SCHEMA_VERSION = 3;
const MODULE_EFFECT_ID = 2_404;
const PARENT_BUFF_ID = 2_302_420;
const WINDOW_BUFF_ID = 2_302_421;
const MAX_HP_ADD_ATTRIBUTE_ID = 11_322;
const DAMAGE_ID = 2_230_242_103;
const WINDOW_MILLIS = 5_000;
const SECONDARY_RAW_ATTRIBUTE_IDS = [11_110, 11_120, 11_130, 11_140, 11_150];
const SECONDARY_DERIVED_ATTRIBUTE_IDS = [11_710, 11_780, 11_840, 11_930, 11_940, 11_950];
const CANDIDATE_WINDOWS_MILLIS = [0, 10, 25, 50, 100, 250, 500, 1_000];

const [command = "help", ...rest] = process.argv.slice(2);
const options = parseArgs(rest);

if (command === "generate") await generate(options);
else if (command === "verify") verify(path.resolve(required(options, "input")));
else if (command === "self-test") await selfTest();
else usage(command === "help" ? 0 : 1);

async function generate(values) {
  const build = required(values, "build");
  if (!/^\d+$/.test(build)) throw new Error("--build must contain only ASCII digits");
  const inputs = {
    timeline: path.resolve(required(values, "timeline")),
    buff_table: path.resolve(required(values, "buff-table")),
    attr_description: path.resolve(required(values, "attr-description")),
    fight_attr_table: path.resolve(required(values, "fight-attr-table")),
    mod_effect_table: path.resolve(required(values, "mod-effect-table")),
    damage_actions: path.resolve(required(values, "damage-actions")),
    status_attribute_proof: path.resolve(required(values, "status-attribute-proof")),
    secondary_attribute_proof: path.resolve(required(values, "secondary-attribute-proof")),
    calculator_calc: path.resolve(required(values, "calculator-calc")),
    calculator_modules: path.resolve(required(values, "calculator-modules")),
  };
  const calculatorRevision = required(values, "calculator-revision");
  if (!/^[0-9a-f]{40}$/.test(calculatorRevision)) {
    throw new Error("--calculator-revision must be a lowercase 40-character Git commit");
  }
  const outputPath = path.resolve(required(values, "output"));
  for (const [label, file] of Object.entries(inputs)) requireFile(file, label);
  refuseExisting(outputPath);

  const staticProof = buildStaticProof(inputs);
  const statusAttributeProof = buildStatusAttributeProof(inputs.status_attribute_proof, build);
  const secondaryAttributeProof = buildSecondaryAttributeProof(
    inputs.secondary_attribute_proof,
    build,
  );
  const timelineProof = await analyzeTimeline(inputs.timeline);
  const calculatorCrosscheck = buildCalculatorCrosscheck(inputs, calculatorRevision);
  const report = {
    schema_version: SCHEMA_VERSION,
    generated_by: GENERATED_BY,
    deployment_id: "global",
    channel: "steam",
    game_build: build,
    mechanic: {
      name: "Life Wave",
      module_effect_id: MODULE_EFFECT_ID,
      parent_buff_id: PARENT_BUFF_ID,
      refreshable_window_buff_id: WINDOW_BUFF_ID,
      max_hp_add_attribute_id: MAX_HP_ADD_ATTRIBUTE_ID,
      linked_damage_id: DAMAGE_ID,
    },
    sources: Object.fromEntries(
      Object.entries(inputs).map(([label, file]) => [label, fileReceipt(file)]),
    ),
    policy: {
      trigger_semantics: "each observed HP-change trigger may apply or refresh the five-second Life Wave window",
      refresh_semantics: "a later activation on the same status instance resets the attribution window",
      self_trigger: "Life Wave owner retains ordinary damage; no third-party transfer",
      external_trigger: "credit only a uniquely resolved external trigger provider for marginal owner damage caused by Life Wave's active secondary-stat increase",
      ambiguous_trigger: "retain ordinary damage and emit no provider transfer",
      missing_heal_candidate: "do not infer a healer; the trigger may be a max-HP change or an unobserved HP-change route",
      remote_character_snapshot_required: false,
      remote_recipient_counterfactual:
        "infer the bounded marginal from same-build, same-recipient, same-mechanic, same-target active/inactive packet outputs; never require or substitute a private remote loadout snapshot",
      conservation: "provider credit must be a transfer from the Life Wave owner's unchanged ordinary damage",
      production_promotion_allowed: false,
    },
    static_proof: staticProof,
    status_attribute_proof: statusAttributeProof,
    secondary_attribute_proof: secondaryAttributeProof,
    external_formula_crosscheck: calculatorCrosscheck,
    observed_timeline_proof: timelineProof,
    conclusion: {
      refreshable_five_second_window_proven:
        staticProof.child_window.duration_millis === WINDOW_MILLIS &&
        timelineProof.summary.duration_5000_activation_count === timelineProof.summary.activation_count &&
        timelineProof.summary.same_instance_reproc_before_expiry_count > 0,
      self_and_external_heal_trigger_candidates_observed:
        timelineProof.same_capture_packet.unique_self_provider_activation_count > 0 &&
        timelineProof.same_capture_packet.unique_external_provider_activation_count > 0,
      same_capture_packet_trigger_cohort_observed:
        timelineProof.same_capture_packet.unique_provider_activation_count > 0,
      current_hp_change_same_wire_with_life_wave_observed:
        statusAttributeProof.current_hp.complete_before_and_after > 0 &&
        statusAttributeProof.current_hp.same_wire_transition_count ===
          statusAttributeProof.current_hp.complete_before_and_after,
      max_hp_change_same_wire_with_life_wave_observed:
        statusAttributeProof.max_hp.complete_before_and_after > 0 &&
        statusAttributeProof.max_hp.same_wire_transition_count ===
          statusAttributeProof.max_hp.complete_before_and_after,
      configured_secondary_stat_magnitude_observed_in_candidate_lanes:
        secondaryAttributeProof.configured_magnitude_observation_count > 0,
      max_hp_trigger_provider_observable_in_this_timeline: false,
      life_wave_secondary_lane_counterfactual_complete: false,
      runtime_promotion_allowed: false,
      remaining_gates: [
        "retain max-HP attribute transitions in the support timeline and bind them to Life Wave refreshes",
        "resolve max-HP-only refreshes that have no healing action in the same capture packet",
        "learn a same-context active/inactive final-damage marginal for each remote recipient mechanic without requiring a character snapshot",
        "prove the trigger packet cohort to refreshed-window to paired-output counterfactual chain across the current-build capture cohort",
        "verify exact integer transfer conservation in live and historical replay",
      ],
    },
  };
  report.content_sha256 = contentHash(report);
  verifyReport(report);
  fs.mkdirSync(path.dirname(outputPath), { recursive: true });
  fs.writeFileSync(outputPath, `${JSON.stringify(report, null, 2)}\n`, { flag: "wx" });
  process.stdout.write(`${JSON.stringify({
    output: outputPath,
      activation_count: timelineProof.summary.activation_count,
    same_instance_reproc_before_expiry_count:
      timelineProof.summary.same_instance_reproc_before_expiry_count,
    same_capture_packet: timelineProof.same_capture_packet,
    selected_window: timelineProof.selected_window,
    conclusion: report.conclusion,
  }, null, 2)}\n`);
}

function buildStaticProof(inputs) {
  const buffs = readJson(inputs.buff_table);
  const descriptions = readJson(inputs.attr_description);
  const fightAttrs = readJson(inputs.fight_attr_table);
  const moduleEffects = readJson(inputs.mod_effect_table);
  const damageActions = readJson(inputs.damage_actions);
  const parent = buffs[String(PARENT_BUFF_ID)];
  const child = buffs[String(WINDOW_BUFF_ID)];
  const description = descriptions[String(PARENT_BUFF_ID)];
  const maxHp = Object.values(fightAttrs).find(
    (row) => Number(row?.AttrAdd) === MAX_HP_ADD_ATTRIBUTE_ID,
  );
  const secondaryAttributes = SECONDARY_RAW_ATTRIBUTE_IDS.map((attributeId) =>
    Object.values(fightAttrs).find((row) => Number(row?.Id) === attributeId),
  );
  const levels = Object.values(moduleEffects)
    .filter((row) => Number(row?.EffectID) === MODULE_EFFECT_ID && Number(row?.Level) > 0)
    .sort((left, right) => Number(left.Level) - Number(right.Level));
  const damage = (Array.isArray(damageActions) ? damageActions : Object.values(damageActions))
    .find((row) => Number(row?.damage_id) === DAMAGE_ID);
  if (!parent || !child || !description || !maxHp || secondaryAttributes.some((row) => !row) || levels.length !== 6 || !damage) {
    throw new Error("Life Wave static proof inputs are incomplete");
  }
  assert.match(description.Description, /When HP changes/i);
  assert.match(description.Description, /lasting 5s/i);
  assert.deepEqual(child.DestroyParam, [[0, 5]]);
  assert.deepEqual(child.RepeatAddRule, [2, 1]);
  assert.equal(Number(maxHp.AttrFinal), 11_320);
  assert.equal(maxHp.OfficialName, "Max HP");
  assert.deepEqual(levels.map((row) => Number(row.EffectConfig?.[0]?.[1])), Array(6).fill(MAX_HP_ADD_ATTRIBUTE_ID));
  assert.deepEqual(levels.map((row) => Number(row.EffectConfig?.[0]?.[2])), [600, 1200, 1800, 2400, 3000, 3600]);
  assert.deepEqual(levels.slice(4).map((row) => Number(row.EffectConfig?.[2]?.[1])), [PARENT_BUFF_ID, PARENT_BUFF_ID]);
  assert.deepEqual(levels.slice(4).map((row) => Number(row.EffectValue?.[0]?.[0])), [600, 1_000]);
  return {
    localized_trigger_description: description.Description,
    parent_buff: {
      id: PARENT_BUFF_ID,
      note: parent.Note,
      repeat_add_rule: parent.RepeatAddRule,
      time_refresh_type: parent.TimeRefreshType,
    },
    child_window: {
      id: WINDOW_BUFF_ID,
      repeat_add_rule: child.RepeatAddRule,
      destroy_param: child.DestroyParam,
      time_refresh_type: child.TimeRefreshType,
      duration_millis: WINDOW_MILLIS,
    },
    max_hp_attribute: {
      family_id: Number(maxHp.Id),
      enum_name: maxHp.EnumName,
      official_name: maxHp.OfficialName,
      additive_attribute_id: Number(maxHp.AttrAdd),
    },
    eligible_secondary_stat_families: secondaryAttributes.map((row) => ({
      family_id: Number(row.Id),
      enum_name: row.EnumName,
      official_name: row.OfficialName,
      final_attribute_id: Number(row.AttrFinal),
      total_attribute_id: Number(row.AttrTotal),
      additive_attribute_id: Number(row.AttrAdd),
      extra_additive_attribute_id: Number(row.AttrExAdd),
      percent_attribute_id: Number(row.AttrPer),
      extra_percent_attribute_id: Number(row.AttrExPer),
    })),
    module_levels: levels.map((row) => ({
      level: Number(row.Level),
      max_hp_add: Number(row.EffectConfig[0][2]),
      chance_config: row.EffectConfig.find((entry) => Number(entry?.[1]) === 99_005) ?? null,
      parent_buff_config: row.EffectConfig.find((entry) => Number(entry?.[1]) === PARENT_BUFF_ID) ?? null,
      effect_value: row.EffectValue,
    })),
    configured_bonus_basis_points_by_level: { 5: 600, 6: 1_000 },
    configured_bonus_percentage_points_by_level: { 5: 6, 6: 10 },
    catalog_damage_identity: {
      damage_id: Number(damage.damage_id),
      linked_action_id: Number(damage.action_id),
      action_parent_relation: damage.action_parent_relation,
      category: damage.category,
      client_catch_all: damage.client_catch_all,
      observed_direct_damage_claimed: false,
    },
  };
}

function buildCalculatorCrosscheck(inputs, revision) {
  const calc = fs.readFileSync(inputs.calculator_calc, "utf8");
  const modules = fs.readFileSync(inputs.calculator_modules, "utf8");
  assert.match(modules, /moduleLevel === 5 \? 0\.06 : 0\.10/);
  assert.match(calc, /moduleLifeWaveStat = \[/);
  for (const lane of ["crit", "luck", "mastery", "vers", "haste"]) {
    assert.match(calc, new RegExp(`moduleLifeWaveStat === '${lane}'`));
  }
  assert.match(calc, /postWlVersDmgPct = postWlVersPct \* 0\.35/);
  return {
    repository: "https://github.com/domaticcode/BPSR-dmg-calc",
    revision,
    role: "user-supplied-formula-crosscheck-not-standalone-authority",
    life_wave_level_5_percentage_points: 6,
    life_wave_level_6_percentage_points: 10,
    highest_lane_candidates: ["crit", "luck", "mastery", "versatility", "haste"],
    versatility_to_damage_ratio: { numerator: 35, denominator: 100 },
    remote_character_snapshot_required_by_rdps_accounting: false,
  };
}

function buildStatusAttributeProof(proofPath, build) {
  const proof = readJson(proofPath);
  assert.equal(Number(proof.schema_version), 29);
  assert.equal(proof.expected_deployment_id, "global");
  assert.equal(proof.expected_game_build, build);
  assert.ok(proof.selected_effect_ids.includes(PARENT_BUFF_ID));
  assert.ok(proof.selected_effect_ids.includes(WINDOW_BUFF_ID));
  for (const attributeId of [11_310, 11_320, MAX_HP_ADD_ATTRIBUTE_ID]) {
    assert.ok(proof.selected_attribute_ids.includes(attributeId));
  }
  const effect = proof.effects.find((row) => Number(row.effect_id) === WINDOW_BUFF_ID);
  if (!effect) throw new Error("status attribute proof has no Life Wave child effect report");
  const summarize = (attributeId) => {
    const report = effect.attributes.find((row) => Number(row.attribute_id) === attributeId);
    if (!report) throw new Error(`status attribute proof has no attribute ${attributeId}`);
    const aggregateTotal = report.aggregates.reduce((sum, row) => sum + Number(row.count), 0);
    const sameWire = report.aggregates
      .filter((row) => row.same_wire_attribute_update === true)
      .reduce((sum, row) => sum + Number(row.count), 0);
    assert.equal(aggregateTotal, Number(report.complete_before_and_after));
    return {
      attribute_id: attributeId,
      transitions_examined: Number(report.transitions_examined),
      complete_before_and_after: Number(report.complete_before_and_after),
      missing_before: Number(report.missing_before),
      missing_after_within_window: Number(report.missing_after_within_window),
      same_wire_transition_count: sameWire,
      isolated_transition_count: Number(report.isolated_transitions),
      transitions_with_competing_target_statuses: Number(report.transitions_with_competing_target_statuses),
      observed_states: [...new Set(report.aggregates.map((row) => row.state))].sort(),
    };
  };
  return {
    schema_version: Number(proof.schema_version),
    selected_life_wave_status_events: Number(effect.selected_status_events),
    selected_life_wave_mechanic_state_changes: Number(effect.selected_mechanic_state_changes),
    current_hp: summarize(11_310),
    max_hp: summarize(11_320),
    max_hp_add: summarize(MAX_HP_ADD_ATTRIBUTE_ID),
    source_rlogs: proof.sessions.map((session) => ({
      path: session.rlog,
      bytes: Number(session.bytes),
      sha256: session.sha256,
      session_id: session.session_id,
      protocol_pack_digest: session.protocol_pack_digest,
    })),
    interpretation: {
      same_wire_is_temporal_cooccurrence_not_provider_identity: true,
      stateful_hp_pools_require_dedicated_state_ledger: true,
      competing_statuses_prevent_isolated_formula_claim: true,
    },
  };
}

function buildSecondaryAttributeProof(proofPath, build) {
  const proof = readJson(proofPath);
  assert.equal(Number(proof.schema_version), 29);
  assert.equal(proof.expected_deployment_id, "global");
  assert.equal(proof.expected_game_build, build);
  assert.deepEqual(proof.selected_effect_ids, [WINDOW_BUFF_ID]);
  for (const attributeId of [...SECONDARY_RAW_ATTRIBUTE_IDS, ...SECONDARY_DERIVED_ATTRIBUTE_IDS]) {
    assert.ok(proof.selected_attribute_ids.includes(attributeId));
  }
  const effect = proof.effects.find((row) => Number(row.effect_id) === WINDOW_BUFF_ID);
  if (!effect) throw new Error("secondary attribute proof has no Life Wave child effect report");
  const attributes = effect.attributes.map((report) => ({
    attribute_id: Number(report.attribute_id),
    transitions_examined: Number(report.transitions_examined),
    complete_before_and_after: Number(report.complete_before_and_after),
    isolated_transition_count: Number(report.isolated_transitions),
    transitions_with_competing_target_statuses: Number(report.transitions_with_competing_target_statuses),
    aggregates: report.aggregates.map((row) => ({
      state: row.state,
      raw_delta_units: Number(row.raw_delta_units),
      count: Number(row.count),
      same_wire_attribute_update: row.same_wire_attribute_update === true,
      provider_is_target: row.provider_is_target === true,
    })),
  }));
  const configuredMagnitudeObservations = attributes.flatMap((attribute) =>
    attribute.aggregates
      .filter((row) => [600, 1_000].includes(Math.abs(row.raw_delta_units)))
      .map((row) => ({ attribute_id: attribute.attribute_id, ...row })),
  );
  return {
    schema_version: Number(proof.schema_version),
    selected_life_wave_status_events: Number(effect.selected_status_events),
    selected_life_wave_mechanic_state_changes: Number(effect.selected_mechanic_state_changes),
    configured_raw_magnitudes: [600, 1_000],
    configured_magnitude_observation_count: configuredMagnitudeObservations.reduce(
      (sum, row) => sum + row.count,
      0,
    ),
    configured_magnitude_observations: configuredMagnitudeObservations,
    attributes,
    interpretation: {
      configured_magnitude_match_is_candidate_lane_evidence: true,
      competing_statuses_prevent_isolated_lane_claim: true,
      refreshes_may_reset_duration_without_reapplying_the_attribute_delta: true,
    },
  };
}

async function analyzeTimeline(timelinePath) {
  const hash = crypto.createHash("sha256");
  const input = fs.createReadStream(timelinePath);
  input.on("data", (chunk) => hash.update(chunk));
  const lines = readline.createInterface({ input, crlfDelay: Infinity });
  const healsByTarget = new Map();
  const activationsByKey = new Map();
  let lineCount = 0;
  let healingRowCount = 0;
  let statusRowCount = 0;
  let activationStatusRowCount = 0;
  const terminalStatusStateCounts = Object.create(null);
  for await (const line of lines) {
    lineCount += 1;
    if (!line.trim()) continue;
    const row = JSON.parse(line);
    if (row?.row_type !== "relationship") continue;
    if (row.event_kind === "healing" && row.target_entity_uuid && row.source_entity_uuid) {
      healingRowCount += 1;
      append(healsByTarget, String(row.target_entity_uuid), {
        session_id: row.session_id,
        sequence: Number(row.sequence),
        capture_sequence: Number(row.capture_sequence),
        game_time_millis: Number(row.game_time_millis),
        observed_micros: Number(row.observed_micros),
        source_actor_id: String(row.source_actor_id),
        source_entity_uuid: String(row.source_entity_uuid),
        target_actor_id: String(row.target_actor_id),
        target_entity_uuid: String(row.target_entity_uuid),
        action_id: Number(row.action_id),
        reported_amount: Number(row.reported_amount),
        hp_loss: Number(row.hp_loss),
      });
    } else if (row.event_kind === "status" && Number(row.effect_id) === WINDOW_BUFF_ID) {
      statusRowCount += 1;
      if (!["applied", "refreshed", "stacked"].includes(row.status_state)) {
        terminalStatusStateCounts[row.status_state ?? "<null>"] =
          (terminalStatusStateCounts[row.status_state ?? "<null>"] ?? 0) + 1;
        continue;
      }
      activationStatusRowCount += 1;
      const key = [
        row.session_id,
        row.affected_entity_uuid,
        row.status_instance_id,
        row.status_created_at_millis,
      ].join("|");
      let activation = activationsByKey.get(key);
      if (!activation) {
        activation = {
          session_id: row.session_id,
          target_actor_id: String(row.affected_entity_actor_id),
          target_entity_uuid: String(row.affected_entity_uuid),
          source_actor_id: String(row.source_actor_id),
          source_entity_uuid: String(row.source_entity_uuid),
          source_config_id: Number(row.source_config_id),
          status_instance_id: Number(row.status_instance_id),
          status_created_at_millis: Number(row.status_created_at_millis),
          status_duration_millis: Number(row.status_duration_millis),
          observed_micros: Number(row.observed_micros),
          capture_sequence: Number(row.capture_sequence),
          sequences: [],
          states: [],
        };
        activationsByKey.set(key, activation);
      }
      activation.sequences.push(Number(row.sequence));
      if (!activation.states.includes(row.status_state)) activation.states.push(row.status_state);
    }
  }
  for (const heals of healsByTarget.values()) {
    heals.sort((left, right) => left.game_time_millis - right.game_time_millis || left.sequence - right.sequence);
  }
  const activations = [...activationsByKey.values()].sort(
    (left, right) => left.status_created_at_millis - right.status_created_at_millis || left.observed_micros - right.observed_micros,
  );
  const allPriorWindowSummaries = CANDIDATE_WINDOWS_MILLIS.map((windowMillis) =>
    summarizeWindow(activations, healsByTarget, windowMillis, false),
  );
  const nearestPrecedingWindowSummaries = CANDIDATE_WINDOWS_MILLIS.map((windowMillis) =>
    summarizeWindow(activations, healsByTarget, windowMillis, true),
  );
  const selectedWindow = nearestPrecedingWindowSummaries.find((row) => row.window_millis === 250);
  const sameCapturePacket = summarizeSameCapturePacket(activations, healsByTarget);
  const reproc = summarizeReproc(activations);
  const duration5000Count = activations.filter(
    (activation) => activation.status_duration_millis === WINDOW_MILLIS,
  ).length;
  return {
    source_line_count: lineCount,
    source_sha256: hash.digest("hex"),
    raw_life_wave_status_row_count: statusRowCount,
    activation_status_row_count: activationStatusRowCount,
    terminal_status_state_counts: terminalStatusStateCounts,
    healing_row_count: healingRowCount,
    deduplication_key: "session + target entity + status instance + status created-at",
    candidate_rule: "same target; heal game time is at or before status created-at and within the stated window",
    candidate_windows_all_prior: allPriorWindowSummaries,
    candidate_windows_nearest_preceding_time: nearestPrecedingWindowSummaries,
    same_capture_packet_rule:
      "same session, target entity, and capture sequence; every distinct provider in the co-serialized HP-change cohort remains a candidate",
    same_capture_packet: sameCapturePacket,
    selected_window_rule: "at the nearest preceding heal game-time within 250ms; all same-time providers remain candidates",
    selected_window: selectedWindow,
    summary: {
      activation_count: activations.length,
      duration_5000_activation_count: duration5000Count,
      distinct_target_count: new Set(activations.map((row) => row.target_entity_uuid)).size,
      distinct_status_instance_count: new Set(activations.map((row) => `${row.target_entity_uuid}|${row.status_instance_id}`)).size,
      duplicate_activation_status_row_count: activationStatusRowCount - activations.length,
      status_state_counts: countValues(activations.flatMap((row) => row.states)),
      ...reproc,
    },
    examples: {
      same_instance_reprocs_before_expiry: reproc.examples,
      unique_external_trigger_candidates: sameCapturePacket.examples.unique_external,
      unique_self_trigger_candidates: sameCapturePacket.examples.unique_self,
      no_heal_candidate: selectedWindow.examples.none,
    },
  };
}

function summarizeSameCapturePacket(activations, healsByTarget) {
  const counts = emptyCandidateCounts();
  const examples = emptyCandidateExamples();
  for (const activation of activations) {
    const candidates = (healsByTarget.get(activation.target_entity_uuid) ?? []).filter(
      (heal) => heal.session_id === activation.session_id &&
        heal.capture_sequence === activation.capture_sequence,
    );
    classifyCandidates(activation, candidates, counts, examples);
  }
  return {
    candidate_mode: "same_capture_packet",
    ...counts,
    examples,
  };
}

function summarizeWindow(activations, healsByTarget, windowMillis, nearestPrecedingTime = false) {
  const counts = emptyCandidateCounts();
  const examples = emptyCandidateExamples();
  for (const activation of activations) {
    let candidates = (healsByTarget.get(activation.target_entity_uuid) ?? []).filter((heal) => {
      const delta = activation.status_created_at_millis - heal.game_time_millis;
      return delta >= 0 && delta <= windowMillis;
    });
    if (nearestPrecedingTime && candidates.length > 0) {
      const nearestTime = Math.max(...candidates.map((row) => row.game_time_millis));
      candidates = candidates.filter((row) => row.game_time_millis === nearestTime);
    }
    classifyCandidates(activation, candidates, counts, examples);
  }
  return {
    window_millis: windowMillis,
    candidate_mode: nearestPrecedingTime ? "nearest_preceding_game_time" : "all_prior_in_window",
    ...counts,
    examples,
  };
}

function emptyCandidateCounts() {
  return {
    unique_event_activation_count: 0,
    unique_provider_activation_count: 0,
    unique_external_provider_activation_count: 0,
    unique_self_provider_activation_count: 0,
    ambiguous_provider_activation_count: 0,
    no_heal_candidate_activation_count: 0,
  };
}

function emptyCandidateExamples() {
  return { unique_external: [], unique_self: [], ambiguous: [], none: [] };
}

function classifyCandidates(activation, candidates, counts, examples) {
  const providers = [...new Set(candidates.map((row) => row.source_entity_uuid))];
  if (candidates.length === 1) counts.unique_event_activation_count += 1;
  if (providers.length === 0) {
    counts.no_heal_candidate_activation_count += 1;
    takeExample(examples.none, activationExample(activation, candidates));
  } else if (providers.length === 1) {
    counts.unique_provider_activation_count += 1;
    const self = providers[0] === activation.target_entity_uuid;
    if (self) {
      counts.unique_self_provider_activation_count += 1;
      takeExample(examples.unique_self, activationExample(activation, candidates));
    } else {
      counts.unique_external_provider_activation_count += 1;
      takeExample(examples.unique_external, activationExample(activation, candidates));
    }
  } else {
    counts.ambiguous_provider_activation_count += 1;
    takeExample(examples.ambiguous, activationExample(activation, candidates));
  }
}

function summarizeReproc(activations) {
  const byInstance = new Map();
  for (const activation of activations) {
    append(byInstance, `${activation.session_id}|${activation.target_entity_uuid}|${activation.status_instance_id}`, activation);
  }
  let sameInstanceReprocCount = 0;
  let beforeExpiryCount = 0;
  const examples = [];
  for (const rows of byInstance.values()) {
    rows.sort((left, right) => left.status_created_at_millis - right.status_created_at_millis);
    for (let index = 1; index < rows.length; index += 1) {
      const previous = rows[index - 1];
      const current = rows[index];
      if (current.status_created_at_millis === previous.status_created_at_millis) continue;
      sameInstanceReprocCount += 1;
      const interval = current.status_created_at_millis - previous.status_created_at_millis;
      if (interval < WINDOW_MILLIS) {
        beforeExpiryCount += 1;
        takeExample(examples, {
          target_actor_id: current.target_actor_id,
          target_entity_uuid: current.target_entity_uuid,
          status_instance_id: current.status_instance_id,
          previous_created_at_millis: previous.status_created_at_millis,
          refreshed_created_at_millis: current.status_created_at_millis,
          interval_millis: interval,
          reset_expiry_from_millis: previous.status_created_at_millis + WINDOW_MILLIS,
          reset_expiry_to_millis: current.status_created_at_millis + WINDOW_MILLIS,
        });
      }
    }
  }
  return {
    same_instance_reproc_count: sameInstanceReprocCount,
    same_instance_reproc_before_expiry_count: beforeExpiryCount,
    examples,
  };
}

function activationExample(activation, candidates) {
  return {
    target_actor_id: activation.target_actor_id,
    target_entity_uuid: activation.target_entity_uuid,
    status_instance_id: activation.status_instance_id,
    status_created_at_millis: activation.status_created_at_millis,
    status_duration_millis: activation.status_duration_millis,
    capture_sequence: activation.capture_sequence,
    observed_micros: activation.observed_micros,
    status_sequences: activation.sequences,
    states: activation.states,
    heal_candidates: candidates.slice(-5).map((row) => ({
      source_actor_id: row.source_actor_id,
      source_entity_uuid: row.source_entity_uuid,
      action_id: row.action_id,
      reported_amount: row.reported_amount,
      sequence: row.sequence,
      capture_sequence: row.capture_sequence,
      game_time_millis: row.game_time_millis,
      delta_to_status_millis: activation.status_created_at_millis - row.game_time_millis,
    })),
  };
}

function verify(inputPath) {
  const report = readJson(inputPath);
  verifyReport(report);
  process.stdout.write(`${JSON.stringify({ verified: true, input: inputPath, content_sha256: report.content_sha256 }, null, 2)}\n`);
}

function verifyReport(report) {
  assert.equal(report.schema_version, SCHEMA_VERSION);
  assert.equal(report.generated_by, GENERATED_BY);
  assert.equal(report.mechanic.module_effect_id, MODULE_EFFECT_ID);
  assert.equal(report.mechanic.refreshable_window_buff_id, WINDOW_BUFF_ID);
  assert.equal(report.static_proof.child_window.duration_millis, WINDOW_MILLIS);
  assert.equal(report.static_proof.max_hp_attribute.additive_attribute_id, MAX_HP_ADD_ATTRIBUTE_ID);
  assert.equal(report.static_proof.catalog_damage_identity.damage_id, DAMAGE_ID);
  assert.deepEqual(report.static_proof.configured_bonus_basis_points_by_level, { 5: 600, 6: 1_000 });
  assert.equal(report.external_formula_crosscheck.revision.length, 40);
  assert.equal(report.external_formula_crosscheck.role, "user-supplied-formula-crosscheck-not-standalone-authority");
  assert.equal(report.status_attribute_proof.schema_version, 29);
  assert.equal(report.secondary_attribute_proof.schema_version, 29);
  assert.equal(report.policy.remote_character_snapshot_required, false);
  assert.ok(report.observed_timeline_proof.same_capture_packet.unique_provider_activation_count > 0);
  assert.equal(report.policy.production_promotion_allowed, false);
  assert.equal(report.conclusion.runtime_promotion_allowed, false);
  assert.equal(report.content_sha256, contentHash(report));
}

async function selfTest() {
  const activations = [
    { session_id: "s", target_actor_id: "1", target_entity_uuid: "a", status_instance_id: 7, status_created_at_millis: 1_000, status_duration_millis: 5_000, observed_micros: 1_100_000, capture_sequence: 10, sequences: [2], states: ["applied"] },
    { session_id: "s", target_actor_id: "1", target_entity_uuid: "a", status_instance_id: 7, status_created_at_millis: 4_000, status_duration_millis: 5_000, observed_micros: 4_100_000, capture_sequence: 11, sequences: [4], states: ["refreshed"] },
  ];
  const heals = new Map([["a", [
    { session_id: "s", source_entity_uuid: "a", game_time_millis: 990, capture_sequence: 10 },
    { session_id: "s", source_entity_uuid: "b", game_time_millis: 3_900, capture_sequence: 11 },
  ]] ]);
  const summary = summarizeWindow(activations, heals, 250, true);
  assert.equal(summary.unique_self_provider_activation_count, 1);
  assert.equal(summary.unique_external_provider_activation_count, 1);
  const packetSummary = summarizeSameCapturePacket(activations, heals);
  assert.equal(packetSummary.unique_self_provider_activation_count, 1);
  assert.equal(packetSummary.unique_external_provider_activation_count, 1);
  const reproc = summarizeReproc(activations);
  assert.equal(reproc.same_instance_reproc_before_expiry_count, 1);
  const object = { schema_version: SCHEMA_VERSION, value: 1 };
  object.content_sha256 = contentHash(object);
  assert.equal(object.content_sha256, contentHash(object));
  process.stdout.write("bpsr-life-wave-trigger-proof self-test passed\n");
}

function parseArgs(args) {
  const values = Object.create(null);
  for (let index = 0; index < args.length; index += 1) {
    const token = args[index];
    if (!token.startsWith("--")) throw new Error(`Unexpected argument: ${token}`);
    const key = token.slice(2);
    const value = args[index + 1];
    if (value === undefined || value.startsWith("--")) throw new Error(`Missing value for ${token}`);
    values[key] = value;
    index += 1;
  }
  return values;
}

function required(values, key) {
  const value = values[key];
  if (!value) throw new Error(`Missing --${key}`);
  return value;
}

function readJson(file) {
  return JSON.parse(fs.readFileSync(file, "utf8"));
}

function fileReceipt(file) {
  const bytes = fs.readFileSync(file);
  return { path: file, bytes: bytes.length, sha256: crypto.createHash("sha256").update(bytes).digest("hex") };
}

function contentHash(value) {
  const clone = structuredClone(value);
  delete clone.content_sha256;
  return crypto.createHash("sha256").update(JSON.stringify(clone)).digest("hex");
}

function requireFile(file, label) {
  if (!fs.statSync(file, { throwIfNoEntry: false })?.isFile()) throw new Error(`${label} is not a file: ${file}`);
}

function refuseExisting(file) {
  if (fs.existsSync(file)) throw new Error(`Refusing to overwrite existing output: ${file}`);
}

function append(map, key, value) {
  const values = map.get(key);
  if (values) values.push(value);
  else map.set(key, [value]);
}

function takeExample(values, value, limit = 10) {
  if (values.length < limit) values.push(value);
}

function countValues(values) {
  const counts = Object.create(null);
  for (const value of values) counts[value] = (counts[value] ?? 0) + 1;
  return counts;
}

function usage(exitCode) {
  process.stdout.write(`Usage:\n  node ${GENERATED_BY} generate --build <id> --timeline <jsonl> --buff-table <json> --attr-description <json> --fight-attr-table <json> --mod-effect-table <json> --damage-actions <json> --status-attribute-proof <json> --secondary-attribute-proof <json> --calculator-calc <js> --calculator-modules <js> --calculator-revision <git-sha> --output <json>\n  node ${GENERATED_BY} verify --input <json>\n  node ${GENERATED_BY} self-test\n`);
  process.exit(exitCode);
}
