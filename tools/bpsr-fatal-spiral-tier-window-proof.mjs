#!/usr/bin/env node

import crypto from "node:crypto";
import fs from "node:fs";
import readline from "node:readline";

const GAME_BUILD = "24687926";
const IMAGINE_SKILL_ID = 3957;
const EFFECT_ID = 2110125;
const PROVIDER_MARKER_EFFECT_ID = 2110124;
const COMPONENT_ID = "fatal-spiral-shared-all-element-bonus";
const EXCLUDED_OWNER_DAMAGE_ID = 111007400108;

function fail(message) {
  throw new Error(message);
}

function key(...values) {
  return values.map((value) => String(value ?? "null")).join("|");
}

function increment(map, value, count = 1n, amount = 0n) {
  const entry = map.get(value) ?? { count: 0n, amount: 0n };
  entry.count += count;
  entry.amount += amount;
  map.set(value, entry);
}

function mapSummary(map, label) {
  return [...map.entries()]
    .sort(([left], [right]) => String(left).localeCompare(String(right), "en", { numeric: true }))
    .map(([value, entry]) => ({
      [label]: value,
      count: Number(entry.count),
      observed_damage: String(entry.amount),
    }));
}

function providerAuthority(runtimeProof) {
  if (runtimeProof.schema_version !== 1 || runtimeProof.game_build !== GAME_BUILD) {
    fail("Runtime provider proof does not match schema 1 and build 24687926");
  }
  if (
    runtimeProof.policy?.equipped_ability_and_tier_are_runtime_identity_authority !== true ||
    runtimeProof.policy?.latest_profile_snapshot_must_not_retroactively_override_a_run !== true
  ) {
    fail("Runtime provider proof is missing run-local tier authority policy");
  }
  const skill = runtimeProof.skills?.find((entry) => Number(entry.imagine_skill_id) === IMAGINE_SKILL_ID);
  if (!skill) fail("Fatal Spiral runtime skill proof is missing");
  const component = skill.components?.find((entry) => entry.component_id === COMPONENT_ID);
  if (
    !component ||
    JSON.stringify(component.effect_ids) !== JSON.stringify([EFFECT_ID]) ||
    !String(component.proof_state).includes("provider-recipient-identity")
  ) {
    fail("Fatal Spiral runtime component identity is incomplete");
  }

  const providers = new Map();
  for (const observation of skill.provider_observations ?? []) {
    if (
      Number(observation.equipped_item_id) !== 3000106 ||
      ![1, 2, 3, 4, 5].includes(Number(observation.equipped_tier))
    ) {
      fail("Fatal Spiral provider observation has invalid item or tier identity");
    }
    const observationKey = key(observation.session_id, observation.provider_entity_uuid);
    const prior = providers.get(observationKey);
    if (prior && Number(prior.equipped_tier) !== Number(observation.equipped_tier)) {
      fail(`Conflicting run-local tiers for ${observationKey}`);
    }
    providers.set(observationKey, observation);
  }
  if (providers.size !== 7) {
    // One run contains two providers, but every provider/session pair remains distinct.
    fail(`Expected 7 exact provider/session observations, found ${providers.size}`);
  }
  return { skill, component, providers };
}

function scalarAuthority(formulaProof) {
  if (formulaProof.schema_version !== 1 || formulaProof.game_build !== GAME_BUILD) {
    fail("Imagine formula proof does not match schema 1 and build 24687926");
  }
  const component = formulaProof.components?.find((entry) => entry.component_id === COMPONENT_ID);
  if (
    !component ||
    component.exact_component_scalar_available !== true ||
    component.fixed_point_denominator !== 10000 ||
    component.equation !== "all_element_bonus_basis_points = 500 + tier_attr_per"
  ) {
    fail("Fatal Spiral exact scalar authority is incomplete");
  }
  const tiers = new Map();
  for (const entry of component.tier_values ?? []) {
    tiers.set(Number(entry.tier), Number(entry.total_basis_points));
  }
  if (JSON.stringify([...tiers.entries()]) !== JSON.stringify([[1, 600], [2, 700], [3, 800], [4, 900], [5, 1000]])) {
    fail("Fatal Spiral tier scalar table is not the exact 600/700/800/900/1000 basis-point family");
  }
  if (
    Number(component.packet_attribute_oracle?.effect_id) !== EFFECT_ID ||
    Number(component.packet_attribute_oracle?.tier) !== 5 ||
    Number(component.packet_attribute_oracle?.applied_delta) !== 1000 ||
    Number(component.packet_attribute_oracle?.removed_delta) !== -1000
  ) {
    fail("Fatal Spiral tier-5 packet attribute oracle is incomplete");
  }
  return { component, tiers };
}

function newAccumulator(providers, tiers) {
  return {
    providers,
    tiers,
    manifest: null,
    runHeaders: new Map(),
    runSummaries: new Map(),
    active: new Map(),
    closedWindows: [],
    statusRows: 0,
    appliedRows: 0,
    removedRows: 0,
    unresolvedProviderRows: 0,
    sourceSideEvents: 0,
    sourceSideDamage: 0n,
    targetSideEvents: 0,
    targetSideDamage: 0n,
    candidateEdges: 0,
    candidateDamage: 0n,
    externalEdges: 0,
    externalDamage: 0n,
    selfEdges: 0,
    selfDamage: 0n,
    strictSingleExternalEvents: 0,
    strictSingleExternalDamage: 0n,
    strictSingleExternalElementalEvents: 0,
    strictSingleExternalElementalDamage: 0n,
    elementalCandidateEdges: 0,
    elementalCandidateDamage: 0n,
    missingPropertyCandidateEdges: 0,
    missingPropertyCandidateDamage: 0n,
    excludedOwnerDamageEdges: 0,
    excludedOwnerDamage: 0n,
    ambiguousMultiWindowEvents: 0,
    ambiguousMultiWindowDamage: 0n,
    selectedDamageHash: crypto.createHash("sha256"),
    byTier: new Map(),
    externalByTier: new Map(),
    byProperty: new Map(),
    byAction: new Map(),
    examples: {
      strict_single_external: [],
      ambiguous_multi_window: [],
      target_side: [],
    },
  };
}

function validateManifest(row) {
  if (
    row.schema_version !== 10 ||
    row.row_type !== "manifest" ||
    row.projection_filter?.effect_id !== EFFECT_ID ||
    row.projection_filter?.include_related_damage !== true ||
    row.topology?.effect_edge !== "provider -> effect/status lifecycle -> recipient or enemy target" ||
    row.topology?.damage_edge !== "recipient damage action -> recipient or enemy target" ||
    row.topology?.source_side_join !== "effect endpoint equals damage actor" ||
    row.topology?.target_side_join !== "effect endpoint equals damage target" ||
    row.policy?.remote_player_cast_packets_required !== false ||
    row.policy?.current_character_snapshots_substituted_into_older_runs !== false
  ) {
    fail("Filtered support timeline manifest does not carry the exact allegiance-neutral topology and policy");
  }
}

function statusWindow(row, provider, scalarBasisPoints) {
  return {
    session_id: row.session_id,
    provider_actor_id: row.provider_actor_id,
    provider_entity_uuid: row.provider_entity_uuid,
    provider_character_id: provider.provider_character_id,
    affected_entity_actor_id: row.affected_entity_actor_id,
    affected_entity_uuid: row.affected_entity_uuid,
    status_instance_id: row.status_instance_id,
    equipped_tier: Number(provider.equipped_tier),
    scalar_basis_points: scalarBasisPoints,
    provider_self: row.provider_entity_uuid === row.affected_entity_uuid,
    applied_sequence: row.sequence,
    applied_observed_micros: row.observed_micros,
    removed_sequence: null,
    removed_observed_micros: null,
  };
}

function consumeRow(acc, row) {
  if (row.row_type === "manifest") {
    validateManifest(row);
    acc.manifest = row;
    return;
  }
  if (row.row_type === "run_header") {
    if (row.client_build !== GAME_BUILD || !String(row.protocol_pack_digest).startsWith("sha256:")) {
      fail(`Run header ${row.session_id} lacks exact build or protocol-pack identity`);
    }
    acc.runHeaders.set(row.session_id, row);
    return;
  }
  if (row.row_type === "run_summary") {
    if (row.filtered_effect_id !== EFFECT_ID || row.sealed_canonical_rlog !== true) {
      fail(`Run summary ${row.session_id} is not a sealed exact effect projection`);
    }
    acc.runSummaries.set(row.session_id, row);
    return;
  }
  if (row.row_type !== "relationship") return;

  if (row.event_kind === "status") {
    acc.statusRows += 1;
    if (
      row.effect_id !== EFFECT_ID ||
      row.source_config_id !== PROVIDER_MARKER_EFFECT_ID ||
      row.source_type_id !== 1 ||
      row.status_instance_id == null ||
      row.status_level !== 1 ||
      row.status_stacks !== 1 ||
      row.status_duration_millis !== 10000
    ) {
      fail(`Status row ${row.session_id}:${row.sequence} lacks exact Fatal Spiral lifecycle identity`);
    }
    const authority = acc.providers.get(key(row.session_id, row.provider_entity_uuid));
    if (!authority) {
      acc.unresolvedProviderRows += 1;
      return;
    }
    const scalarBasisPoints = acc.tiers.get(Number(authority.equipped_tier));
    if (!scalarBasisPoints) fail("Run-local tier has no exact scalar authority");
    const windowKey = key(row.session_id, row.provider_entity_uuid, row.affected_entity_uuid, row.status_instance_id);
    if (row.status_state === "applied") {
      acc.appliedRows += 1;
      if (acc.active.has(windowKey)) fail(`Duplicate active status instance ${windowKey}`);
      acc.active.set(windowKey, statusWindow(row, authority, scalarBasisPoints));
    } else if (row.status_state === "removed") {
      acc.removedRows += 1;
      const window = acc.active.get(windowKey);
      if (!window) fail(`Removal lacks exact applied status instance ${windowKey}`);
      window.removed_sequence = row.sequence;
      window.removed_observed_micros = row.observed_micros;
      acc.closedWindows.push(window);
      acc.active.delete(windowKey);
    } else {
      fail(`Unexpected Fatal Spiral lifecycle state ${row.status_state}`);
    }
    return;
  }

  if (row.event_kind !== "damage") return;
  const amount = BigInt(row.reported_amount ?? 0);
  if (row.filtered_effect_join === "target-side-effect-endpoint-equals-damage-target") {
    acc.targetSideEvents += 1;
    acc.targetSideDamage += amount;
    if (acc.examples.target_side.length < 8) {
      acc.examples.target_side.push({
        session_id: row.session_id,
        sequence: row.sequence,
        damage_actor_entity_uuid: row.damage_actor_entity_uuid,
        damage_target_entity_uuid: row.damage_target_entity_uuid,
        action_id: row.action_id,
        reported_amount: String(amount),
      });
    }
    return;
  }
  if (row.filtered_effect_join !== "source-side-effect-endpoint-equals-damage-actor") {
    fail(`Unexpected filtered effect join ${row.filtered_effect_join}`);
  }

  acc.sourceSideEvents += 1;
  acc.sourceSideDamage += amount;
  const windows = [...acc.active.values()].filter(
    (window) => window.session_id === row.session_id && window.affected_entity_uuid === row.damage_actor_entity_uuid,
  );
  if (windows.length === 0) {
    fail(`Source-side damage row ${row.session_id}:${row.sequence} has no active effect window`);
  }
  const external = windows.filter((window) => !window.provider_self);
  acc.candidateEdges += windows.length;
  acc.candidateDamage += amount * BigInt(windows.length);
  acc.externalEdges += external.length;
  acc.externalDamage += amount * BigInt(external.length);
  acc.selfEdges += windows.length - external.length;
  acc.selfDamage += amount * BigInt(windows.length - external.length);
  increment(acc.byProperty, String(row.property ?? "null"), BigInt(windows.length), amount * BigInt(windows.length));
  increment(acc.byAction, String(row.action_id ?? "null"), BigInt(windows.length), amount * BigInt(windows.length));
  for (const window of windows) increment(acc.byTier, String(window.equipped_tier), 1n, amount);
  for (const window of external) increment(acc.externalByTier, String(window.equipped_tier), 1n, amount);

  const strictSingleExternal = windows.length === 1 && external.length === 1;
  const elementalProperty = Number.isInteger(row.property) && row.property >= 1 && row.property <= 8;
  const excludedOwnerDamage = Number(row.action_id) === EXCLUDED_OWNER_DAMAGE_ID;
  if (elementalProperty) {
    acc.elementalCandidateEdges += windows.length;
    acc.elementalCandidateDamage += amount * BigInt(windows.length);
  } else if (row.property == null) {
    acc.missingPropertyCandidateEdges += windows.length;
    acc.missingPropertyCandidateDamage += amount * BigInt(windows.length);
  }
  if (excludedOwnerDamage) {
    acc.excludedOwnerDamageEdges += windows.length;
    acc.excludedOwnerDamage += amount * BigInt(windows.length);
  }
  if (strictSingleExternal) {
    acc.strictSingleExternalEvents += 1;
    acc.strictSingleExternalDamage += amount;
    if (elementalProperty && !excludedOwnerDamage) {
      acc.strictSingleExternalElementalEvents += 1;
      acc.strictSingleExternalElementalDamage += amount;
    }
  } else if (windows.length > 1) {
    acc.ambiguousMultiWindowEvents += 1;
    acc.ambiguousMultiWindowDamage += amount;
  }
  const eventProof = {
    session_id: row.session_id,
    sequence: row.sequence,
    observed_micros: row.observed_micros,
    damage_actor_entity_uuid: row.damage_actor_entity_uuid,
    damage_target_entity_uuid: row.damage_target_entity_uuid,
    action_id: row.action_id,
    property: row.property,
    reported_amount: String(amount),
    active_windows: windows.map((window) => ({
      provider_entity_uuid: window.provider_entity_uuid,
      status_instance_id: window.status_instance_id,
      equipped_tier: window.equipped_tier,
      scalar_basis_points: window.scalar_basis_points,
      provider_self: window.provider_self,
    })),
  };
  acc.selectedDamageHash.update(`${JSON.stringify(eventProof)}\n`);
  const bucket = strictSingleExternal ? acc.examples.strict_single_external : acc.examples.ambiguous_multi_window;
  if (bucket.length < 8) bucket.push(eventProof);
}

function fileDescriptor(filePath) {
  const bytes = fs.readFileSync(filePath);
  return {
    path: filePath,
    bytes: bytes.length,
    sha256: crypto.createHash("sha256").update(bytes).digest("hex").toUpperCase(),
  };
}

function finalize(acc, timelineDescriptor, runtimeDescriptor, formulaDescriptor) {
  if (!acc.manifest || acc.runHeaders.size !== 6 || acc.runSummaries.size !== 6) {
    fail("Filtered timeline is missing its manifest or six exact run boundaries");
  }
  if (acc.statusRows !== 394 || acc.appliedRows !== 202 || acc.removedRows !== 192 || acc.active.size !== 10) {
    fail("Fatal Spiral lifecycle totals do not match the current six-run corpus frontier");
  }
  if (
    acc.sourceSideEvents !== 96684 ||
    acc.sourceSideDamage !== 13313077252n ||
    acc.candidateEdges !== 100774 ||
    acc.candidateDamage !== 13912378707n ||
    acc.unresolvedProviderRows !== 0
  ) {
    fail("Fatal Spiral event-time tier/window join does not reproduce the reviewed frontier");
  }
  const openWindows = [...acc.active.values()].sort((left, right) =>
    key(left.session_id, left.applied_sequence).localeCompare(key(right.session_id, right.applied_sequence), "en", { numeric: true }),
  );
  return {
    schema_version: 3,
    generated_by: "tools/bpsr-fatal-spiral-tier-window-proof.mjs",
    game: "blue-protocol-star-resonance",
    game_build: GAME_BUILD,
    identity: {
      imagine_skill_id: IMAGINE_SKILL_ID,
      component_id: COMPONENT_ID,
      provider_marker_effect_id: PROVIDER_MARKER_EFFECT_ID,
      effect_id: EFFECT_ID,
      excluded_owner_damage_id: EXCLUDED_OWNER_DAMAGE_ID,
    },
    topology: acc.manifest.topology,
    policy: {
      run_local_equipped_tier_is_identity_authority: true,
      later_profile_snapshots_may_rewrite_older_runs: false,
      source_side_and_target_side_joins_are_independent: true,
      endpoint_allegiance_is_assumed: false,
      remote_cast_packets_are_required: false,
      provider_credit_authorized_by_this_artifact: false,
      multi_window_events_are_deferred_until_stacking_and_split_are_proven: true,
      damage_stage_and_integer_rounding_remain_fail_closed: true,
    },
    inputs: {
      filtered_support_timeline: timelineDescriptor,
      runtime_provider_recipient_proof: runtimeDescriptor,
      imagine_formula_proof: formulaDescriptor,
    },
    scalar_authority: {
      fixed_point_denominator: 10000,
      tier_basis_points: [600, 700, 800, 900, 1000],
      tier_5_packet_oracle: { applied_delta: 1000, removed_delta: -1000 },
    },
    summary: {
      exact_run_count: acc.runHeaders.size,
      exact_provider_session_observations: acc.providers.size,
      status_rows: acc.statusRows,
      applied_rows: acc.appliedRows,
      removed_rows: acc.removedRows,
      closed_windows: acc.closedWindows.length,
      open_windows: openWindows.length,
      status_rows_with_exact_run_local_tier: acc.statusRows - acc.unresolvedProviderRows,
      status_rows_without_exact_run_local_tier: acc.unresolvedProviderRows,
      unique_source_side_damage_events: acc.sourceSideEvents,
      unique_source_side_observed_damage: String(acc.sourceSideDamage),
      candidate_window_edges: acc.candidateEdges,
      candidate_window_edge_observed_damage: String(acc.candidateDamage),
      external_candidate_edges: acc.externalEdges,
      external_candidate_edge_observed_damage: String(acc.externalDamage),
      provider_self_candidate_edges: acc.selfEdges,
      provider_self_candidate_edge_observed_damage: String(acc.selfDamage),
      strict_single_external_window_events: acc.strictSingleExternalEvents,
      strict_single_external_window_observed_damage: String(acc.strictSingleExternalDamage),
      strict_single_external_elemental_candidate_events: acc.strictSingleExternalElementalEvents,
      strict_single_external_elemental_candidate_observed_damage: String(acc.strictSingleExternalElementalDamage),
      elemental_property_candidate_edges: acc.elementalCandidateEdges,
      elemental_property_candidate_edge_observed_damage: String(acc.elementalCandidateDamage),
      missing_property_candidate_edges: acc.missingPropertyCandidateEdges,
      missing_property_candidate_edge_observed_damage: String(acc.missingPropertyCandidateDamage),
      excluded_owner_damage_candidate_edges: acc.excludedOwnerDamageEdges,
      excluded_owner_damage_candidate_edge_observed_damage: String(acc.excludedOwnerDamage),
      ambiguous_multi_window_events: acc.ambiguousMultiWindowEvents,
      ambiguous_multi_window_observed_damage: String(acc.ambiguousMultiWindowDamage),
      target_side_damage_events_preserved_separately: acc.targetSideEvents,
      target_side_observed_damage_preserved_separately: String(acc.targetSideDamage),
      selected_source_side_event_sha256: acc.selectedDamageHash.digest("hex").toUpperCase(),
    },
    distributions: {
      candidate_edges_by_equipped_tier: mapSummary(acc.byTier, "equipped_tier"),
      external_candidate_edges_by_equipped_tier: mapSummary(acc.externalByTier, "equipped_tier"),
      candidate_edges_by_damage_property: mapSummary(acc.byProperty, "damage_property"),
      candidate_edges_by_damage_action: mapSummary(acc.byAction, "damage_action_id"),
    },
    windows: {
      closed: acc.closedWindows,
      open: openWindows,
    },
    examples: acc.examples,
    proof_closure: {
      exact_event_time_provider_tier_join_complete: true,
      exact_effect_lifecycle_window_selection_complete: true,
      source_side_affected_damage_selection_complete: true,
      target_side_rows_preserved_without_allegiance_inference: true,
      exact_packet_damage_property_identity_preserved: true,
      elemental_property_candidate_filter_is_1_through_8: true,
      damage_property_coverage_proven: false,
      combat_damage_stage_consumer_proven: false,
      integer_damage_counterfactual_projection_complete: false,
      matching_window_conservation_replay_complete: false,
      runtime_rdps_credit_enabled: false,
    },
    remaining_proof_obligations: [
      "prove which damage property and packet modes consume the All-Element fixed-point family",
      "prove the combat damage-stage operation order and integer rounding for the exact build",
      "resolve overlapping provider/self windows using exact stacking and provider split rules",
      "project the provider-removal counterfactual for eligible damage events",
      "replay recipient debit and provider credit with exact integer conservation",
    ],
  };
}

async function buildProof(timelinePath, runtimePath, formulaPath) {
  const runtimeProof = JSON.parse(fs.readFileSync(runtimePath, "utf8"));
  const formulaProof = JSON.parse(fs.readFileSync(formulaPath, "utf8"));
  const { providers } = providerAuthority(runtimeProof);
  const { tiers } = scalarAuthority(formulaProof);
  const acc = newAccumulator(providers, tiers);
  const inputHash = crypto.createHash("sha256");
  let inputBytes = 0;
  const input = fs.createReadStream(timelinePath);
  const lines = readline.createInterface({ input, crlfDelay: Infinity });
  for await (const line of lines) {
    const encoded = Buffer.from(`${line}\n`);
    inputBytes += encoded.length;
    inputHash.update(encoded);
    consumeRow(acc, JSON.parse(line));
  }
  return finalize(
    acc,
    {
      path: timelinePath,
      bytes: inputBytes,
      sha256: inputHash.digest("hex").toUpperCase(),
    },
    fileDescriptor(runtimePath),
    fileDescriptor(formulaPath),
  );
}

function argumentsFrom(argv) {
  const values = [...argv];
  const take = (flag) => {
    const index = values.indexOf(flag);
    if (index < 0 || index + 1 >= values.length) fail(`Missing ${flag}`);
    values.splice(index, 1);
    return values.splice(index, 1)[0];
  };
  const timeline = take("--timeline");
  const runtimeProof = take("--runtime-proof");
  const formulaProof = take("--formula-proof");
  const output = take("--output");
  if (values.length) fail(`Unexpected arguments: ${values.join(" ")}`);
  return { timeline, runtimeProof, formulaProof, output };
}

async function main() {
  if (process.argv.includes("--self-test")) {
    const active = new Map();
    active.set("a", { session_id: "s", affected_entity_uuid: "2", provider_self: false });
    if ([...active.values()].filter((window) => window.affected_entity_uuid === "2").length !== 1) {
      fail("self-test source-side window join failed");
    }
    console.log("bpsr-fatal-spiral-tier-window-proof self-test passed");
    return;
  }
  const args = argumentsFrom(process.argv.slice(2));
  if (fs.existsSync(args.output)) fail(`Refusing to overwrite existing output: ${args.output}`);
  const proof = await buildProof(args.timeline, args.runtimeProof, args.formulaProof);
  fs.writeFileSync(args.output, `${JSON.stringify(proof, null, 2)}\n`, { flag: "wx" });
  console.log(JSON.stringify({ output: args.output, summary: proof.summary, proof_closure: proof.proof_closure }, null, 2));
}

main().catch((error) => {
  console.error(error.stack ?? String(error));
  process.exitCode = 1;
});
