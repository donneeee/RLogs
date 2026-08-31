#!/usr/bin/env node

import crypto from "node:crypto";
import fs from "node:fs";
import path from "node:path";

const SCHEMA_VERSION = 1;
const GENERATED_BY = "tools/bpsr-fatal-spiral-partial-prefix-frontier.mjs";
const GAME_BUILD = "24687926";
const EFFECT_ID = 2110125;
const ATTRIBUTE_IDS = [13100, 13101, 13102, 13103, 13104, 13105];

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

async function descriptor(file) {
  const hash = crypto.createHash("sha256");
  let bytes = 0;
  for await (const chunk of fs.createReadStream(file)) {
    bytes += chunk.length;
    hash.update(chunk);
  }
  return {
    path: path.resolve(file).replaceAll("\\", "/"),
    bytes,
    sha256: hash.digest("hex").toUpperCase(),
  };
}

function normalized(file) {
  return path.resolve(file).replaceAll("\\", "/");
}

function sortedNumbers(values) {
  return [...new Set((values ?? []).map(Number))].sort((left, right) => left - right);
}

function readCohortHeader(file) {
  const marker = Buffer.from('"attribute_states":');
  const handle = fs.openSync(file, "r");
  try {
    let buffered = Buffer.alloc(0);
    const chunk = Buffer.alloc(64 * 1024);
    while (buffered.length <= 4 * 1024 * 1024) {
      const read = fs.readSync(handle, chunk, 0, chunk.length, null);
      if (read === 0) break;
      buffered = Buffer.concat([buffered, chunk.subarray(0, read)]);
      const index = buffered.indexOf(marker);
      if (index >= 0) {
        const prefix = buffered.subarray(0, index).toString("utf8");
        return JSON.parse(`${prefix}"attribute_states":[],"status_states":[],"samples":[]}`);
      }
    }
  } finally {
    fs.closeSync(handle);
  }
  fail(`Formula cohort header exceeds the bounded prefix or lacks attribute_states: ${file}`);
}

function validatePrefixAudit(audit, relationship) {
  const summary = audit.summary ?? {};
  if (
    audit.schema_version !== 2 ||
    audit.generated_by !== "rlogs-bpsr-rlog-partial-prefix-audit" ||
    audit.expected_game_build !== GAME_BUILD ||
    audit.damage_relationship !== relationship ||
    canonical(sortedNumbers(audit.selected_effect_ids)) !== canonical([EFFECT_ID]) ||
    audit.policy?.original_partial_rlogs_are_read_only !== true ||
    audit.policy?.missing_or_truncated_tail_is_an_exclusion_boundary !== true ||
    audit.policy?.partial_prefix_has_integrity_seal_authority !== false ||
    audit.policy?.packet_absence_is_zero !== false ||
    audit.policy?.formula_authority !== false ||
    Number(summary.input_count) !== 10 ||
    Number(summary.valid_prefix_event_count) !== 1_039_616 ||
    Number(summary.damage_event_count) !== 113_303 ||
    Number(summary.selected_effect_status_event_count) !== 86 ||
    Number(summary.selected_effect_complete_prefix_lifecycle_count) !== 23 ||
    Number(summary.record_boundary_missing_seal_count) !== 9 ||
    Number(summary.truncated_record_tail_count) !== 1 ||
    summary.controlled_counterfactual_pair_proven !== false ||
    summary.formula_authority !== false
  ) fail(`${relationship}-side partial-prefix audit is unsafe or inconsistent`);
  const expectedMemberships = relationship === "source" ? 29_815 : 73;
  if (Number(summary.selected_effect_damage_events_while_endpoint_active) !== expectedMemberships) {
    fail(`${relationship}-side prefix membership count changed`);
  }
}

async function validateRecoveryManifest(manifest, manifestPath) {
  if (
    manifest.schema_version !== 1 ||
    manifest.game_build !== GAME_BUILD ||
    manifest.policy?.source_prefix_integrity_seal_authority !== false ||
    manifest.policy?.derived_rlog_seal_authenticates_transformation_only !== true ||
    manifest.policy?.packet_absence_is_zero !== false ||
    manifest.policy?.formula_authority !== false ||
    !Array.isArray(manifest.sessions) ||
    manifest.sessions.length !== 9
  ) fail("recovered-prefix manifest is unsafe or inconsistent");

  const receipts = manifest.sessions.map((session) => {
    if (typeof session.rlog !== "string" || typeof session.recovery_receipt !== "string") {
      fail("recovered-prefix manifest session lacks its RLOG or receipt path");
    }
    const receipt = readJson(session.recovery_receipt, "partial-prefix recovery receipt");
    if (
      receipt.schema_version !== 1 ||
      receipt.generated_by !== "rlogs-bpsr-rlog-partial-prefix-recovery" ||
      receipt.game_build !== GAME_BUILD ||
      receipt.policy?.source_prefix_has_integrity_seal_authority !== false ||
      receipt.policy?.no_missing_source_event_is_synthesized !== true ||
      receipt.policy?.recovered_rlog_seal_authenticates_the_transformation_not_the_original_capture !== true ||
      receipt.policy?.formula_authority !== false ||
      receipt.output?.integrity_seal_validated !== true ||
      normalized(receipt.output?.file?.path ?? "") !== normalized(session.rlog) ||
      Number(receipt.output?.event_count) !== Number(receipt.validated_prefix?.valid_prefix_event_count) + 1 ||
      receipt.derived_terminal_gap?.kind !== "capture_drop"
    ) fail(`unsafe or inconsistent recovery receipt ${session.recovery_receipt}`);
    return { session, receipt };
  });
  const prefixEvents = receipts.reduce(
    (sum, row) => sum + Number(row.receipt.validated_prefix.valid_prefix_event_count), 0,
  );
  const outputEvents = receipts.reduce(
    (sum, row) => sum + Number(row.receipt.output.event_count), 0,
  );
  if (prefixEvents !== 1_039_616 || outputEvents !== 1_039_625) {
    fail("recovered-prefix receipt event totals changed");
  }
  const receiptDescriptors = await Promise.all(
    receipts.map((row) => descriptor(row.session.recovery_receipt)),
  );
  const rlogDescriptors = await Promise.all(receipts.map((row) => descriptor(row.session.rlog)));
  for (let index = 0; index < receipts.length; index += 1) {
    const declared = receipts[index].receipt.output.file;
    const actual = rlogDescriptors[index];
    if (Number(declared.bytes) !== actual.bytes || String(declared.sha256).toUpperCase() !== actual.sha256) {
      fail(`recovered RLOG receipt mismatch for ${actual.path}`);
    }
  }
  return {
    manifest: await descriptor(manifestPath),
    recovery_receipts: receiptDescriptors,
    recovered_rlogs: rlogDescriptors,
    prefix_events: prefixEvents,
    recovered_events: outputEvents,
  };
}

function validateGapAudit(audit, manifestDescriptor) {
  const summary = audit.summary ?? {};
  if (
    audit.schema_version !== 3 ||
    audit.generated_by !== "rlogs-bpsr-rlog-gap-window-audit" ||
    audit.game_build !== GAME_BUILD ||
    Number(audit.effect_id) !== EFFECT_ID ||
    audit.damage_relationship !== "source" ||
    audit.policy?.formula_authority !== false ||
    audit.policy?.provider_rdps_credit_allowed !== false ||
    normalized(audit.inputs?.source_manifest?.path ?? "") !== manifestDescriptor.path ||
    Number(audit.inputs?.source_manifest?.bytes) !== manifestDescriptor.bytes ||
    String(audit.inputs?.source_manifest?.sha256 ?? "").toUpperCase() !== manifestDescriptor.sha256 ||
    Number(summary.sealed_rlog_count) !== 9 ||
    Number(summary.canonical_event_count) !== 1_039_625 ||
    Number(summary.data_gap_count) !== 966 ||
    Number(summary.selected_effect_complete_gap_bounded_lifecycle_count) !== 23 ||
    Number(summary.selected_effect_damage_events_while_active) !== 16_376 ||
    Number(summary.gap_kind_counts?.capture_drop) !== 9 ||
    summary.formula_authority !== false
  ) fail("recovered-prefix gap-window audit is unsafe or inconsistent");
}

function validatePresentCohort(cohort, gapDescriptor) {
  if (
    cohort.schema_version !== 44 ||
    cohort.generated_by !== "rlogs-bpsr-state-scaling-damage-proof" ||
    cohort.game_build !== GAME_BUILD ||
    cohort.policy?.formula_authority !== false ||
    cohort.selection?.effect_locus !== "source" ||
    canonical(sortedNumbers(cohort.selection?.source_effect_ids)) !== canonical([EFFECT_ID]) ||
    normalized(cohort.gap_window_filter?.source ?? "") !== gapDescriptor.path ||
    String(cohort.gap_window_filter?.source_sha256 ?? "").toUpperCase() !== gapDescriptor.sha256 ||
    Number(cohort.gap_window_filter?.matched_window_damage_memberships) !== 16_376 ||
    !Array.isArray(cohort.inputs) ||
    cohort.inputs.length !== 9
  ) fail("recovered-prefix present-state cohort is unsafe or inconsistent");
  return sortedNumbers(cohort.selection?.ability_ids);
}

function validateComparisonCohort(cohort) {
  const abilities = sortedNumbers(cohort.selection?.ability_ids);
  if (
    cohort.schema_version !== 44 ||
    cohort.generated_by !== "rlogs-bpsr-state-scaling-damage-proof" ||
    cohort.game_build !== GAME_BUILD ||
    cohort.policy?.formula_authority !== false ||
    abilities.length !== 89 ||
    (cohort.selection?.selected_effect_ids ?? []).length !== 0 ||
    !Array.isArray(cohort.inputs) ||
    cohort.inputs.length !== 9
  ) fail("recovered-prefix comparison cohort is unsafe or inconsistent");
  return abilities;
}

function transitionAggregate(proof, field) {
  const effects = proof[field] ?? [];
  const variants = effects.flatMap((effect) => effect.variants ?? []);
  return {
    variants: variants.length,
    present_groups: variants.reduce(
      (sum, row) => sum + Number(row.candidate_present_groups ?? 0), 0,
    ),
    absent_pairs: variants.reduce(
      (sum, row) => sum + Number(row.candidate_absent_formula_state_pairs ?? 0), 0,
    ),
    rejected_without_source_attribute_transition: variants.reduce(
      (sum, row) => sum + Number(row.rejected_without_source_attribute_transition ?? 0), 0,
    ),
    controlled_pairs: effects.reduce((sum, row) => sum + Number(row.controlled_pairs ?? 0), 0),
    divergent_pairs: effects.reduce((sum, row) => sum + Number(row.divergent_output_pairs ?? 0), 0),
  };
}

function validateCounterfactual(proof, comparisonDescriptor) {
  const summary = proof.summary ?? {};
  const broad = transitionAggregate(
    proof,
    "cross_entity_source_transition_target_current_hp_excluded_source_and_target_status_transition_diagnostic",
  );
  const projections = (proof.cross_entity_source_transition_diagnostic ?? [])
    .flatMap((effect) => effect.variants ?? [])
    .map((variant) => variant.all_element_damage_candidate_projection)
    .filter(Boolean);
  if (
    proof.schema_version !== 17 ||
    proof.generated_by !== "rlogs-bpsr-status-effect-counterfactual-proof" ||
    proof.game_build !== GAME_BUILD ||
    proof.policy?.formula_authority !== false ||
    proof.policy?.runtime_authority !== false ||
    proof.policy?.all_element_damage_candidate_projection_authority !== false ||
    proof.policy?.structurally_absent_remote_skill_cast_packets_required !== false ||
    Number(proof.processing?.memory_limit_mib) !== 512 ||
    proof.processing?.measured_peak_within_configured_limit !== true ||
    canonical(sortedNumbers(proof.processing?.selected_effect_ids)) !== canonical([EFFECT_ID]) ||
    canonical(sortedNumbers(proof.processing?.selected_source_transition_attribute_ids)) !== canonical(ATTRIBUTE_IDS) ||
    Number(summary.samples) !== 92_161 ||
    Number(summary.exact_controlled_groups) !== 0 ||
    Number(summary.relaxed_controlled_groups) !== 0 ||
    Number(summary.near_controlled_target_pairs) !== 0 ||
    Number(summary.near_controlled_source_pairs) !== 0 ||
    Number(summary.cross_entity_formula_state_controlled_groups) !== 0 ||
    Number(summary.cross_entity_source_transition_controlled_pairs) !== 0 ||
    Number(summary.cross_entity_source_transition_target_current_hp_excluded_source_and_target_status_transition_controlled_pairs) !== 0 ||
    Number(summary.cross_entity_source_transition_target_current_hp_excluded_source_status_review_band_pairs) !== 57 ||
    Number(summary.cross_entity_source_transition_target_current_hp_excluded_source_status_review_band_pairs_without_source_attribute_transition) !== 57 ||
    Number(summary.cross_entity_source_transition_target_current_hp_excluded_source_status_review_band_pairs_with_selected_source_attribute_transition) !== 0 ||
    broad.controlled_pairs !== 0 ||
    broad.divergent_pairs !== 0 ||
    projections.length !== 74 ||
    projections.some((projection) =>
      Number(projection.deterministic_pairs) !== 0 ||
      projection.candidate_selected !== false ||
      projection.formula_authority !== false) ||
    Number(proof.input?.bytes) !== comparisonDescriptor.bytes ||
    String(proof.input?.sha256 ?? "").replace(/^sha256:/, "").toUpperCase() !== comparisonDescriptor.sha256
  ) fail("recovered-prefix counterfactual proof is unsafe or inconsistent");
  return {
    samples: Number(summary.samples),
    review_band_pairs: 57,
    review_band_rejected_without_source_attribute_transition: 57,
    broad,
  };
}

function validateReport(report) {
  if (
    report.schema_version !== SCHEMA_VERSION ||
    report.generated_by !== GENERATED_BY ||
    report.game_build !== GAME_BUILD ||
    report.effect_id !== EFFECT_ID ||
    report.coverage?.validated_prefix_events !== 1_039_616 ||
    report.coverage?.recovered_canonical_events !== 1_039_625 ||
    report.coverage?.complete_gap_bounded_lifecycles !== 23 ||
    report.coverage?.safe_source_damage_memberships !== 16_376 ||
    report.coverage?.comparison_samples !== 92_161 ||
    report.coverage?.controlled_pairs !== 0 ||
    report.coverage?.review_band_pairs !== 57 ||
    report.coverage?.review_band_rejected_without_source_attribute_transition !== 57 ||
    report.proof_state?.retained_partial_prefix_search_exhausted !== true ||
    report.proof_state?.source_capture_integrity_seal_authority !== false ||
    report.proof_state?.formula_proven !== false ||
    report.proof_state?.runtime_authority !== false ||
    report.proof_state?.provider_rdps_credit_allowed !== false ||
    report.content_sha256 !== digest(report)
  ) fail("partial-prefix frontier report is unsafe or inconsistent");
}

async function build(args) {
  const sourceAudit = readJson(args.sourcePrefixAudit, "source partial-prefix audit");
  const targetAudit = readJson(args.targetPrefixAudit, "target partial-prefix audit");
  validatePrefixAudit(sourceAudit, "source");
  validatePrefixAudit(targetAudit, "target");

  const manifest = readJson(args.recoveryManifest, "recovered-prefix manifest");
  const recovery = await validateRecoveryManifest(manifest, args.recoveryManifest);
  const gapAudit = readJson(args.gapWindowAudit, "recovered-prefix gap-window audit");
  validateGapAudit(gapAudit, recovery.manifest);
  const gapDescriptor = await descriptor(args.gapWindowAudit);

  const presentCohort = readCohortHeader(args.presentCohort);
  validatePresentCohort(presentCohort, gapDescriptor);
  const comparisonCohort = readCohortHeader(args.comparisonCohort);
  const abilities = validateComparisonCohort(comparisonCohort);
  const comparisonDescriptor = await descriptor(args.comparisonCohort);
  const counterfactual = readJson(args.counterfactual, "recovered-prefix counterfactual proof");
  const comparison = validateCounterfactual(counterfactual, comparisonDescriptor);

  const report = {
    schema_version: SCHEMA_VERSION,
    generated_by: GENERATED_BY,
    game_build: GAME_BUILD,
    effect_id: EFFECT_ID,
    topology: {
      lifecycle_endpoint_and_damage_endpoint_are_independent: true,
      source_join: "status recipient -> damage actor",
      target_join: "status recipient -> damage target",
      allegiance_is_inferred: false,
    },
    policy: {
      original_partial_rlogs_are_read_only: true,
      recovered_seals_authenticate_transformation_only: true,
      packet_absence_is_zero: false,
      structurally_absent_remote_skill_cast_packets_required: false,
      formula_authority: false,
      runtime_authority: false,
      provider_rdps_credit_allowed: false,
    },
    inputs: {
      source_prefix_audit: await descriptor(args.sourcePrefixAudit),
      target_prefix_audit: await descriptor(args.targetPrefixAudit),
      recovery_manifest: recovery.manifest,
      recovery_receipts: recovery.recovery_receipts,
      recovered_rlogs: recovery.recovered_rlogs,
      gap_window_audit: gapDescriptor,
      present_formula_cohort: await descriptor(args.presentCohort),
      comparison_formula_cohort: comparisonDescriptor,
      counterfactual_proof: await descriptor(args.counterfactual),
    },
    coverage: {
      partial_input_count: 10,
      recovered_nonempty_input_count: 9,
      validated_prefix_events: recovery.prefix_events,
      derived_terminal_gap_events: 9,
      recovered_canonical_events: recovery.recovered_events,
      damage_events: 113_303,
      selected_effect_status_events: 86,
      complete_gap_bounded_lifecycles: 23,
      apparent_source_memberships_before_complete_window_filter: 29_815,
      apparent_target_memberships_before_complete_window_filter: 73,
      safe_source_damage_memberships: 16_376,
      selected_damage_action_ids: abilities,
      selected_damage_action_id_count: abilities.length,
      comparison_samples: comparison.samples,
      controlled_pairs: 0,
      divergent_controlled_pairs: 0,
      review_band_pairs: comparison.review_band_pairs,
      review_band_rejected_without_source_attribute_transition:
        comparison.review_band_rejected_without_source_attribute_transition,
    },
    proof_state: {
      retained_partial_prefix_search_exhausted: true,
      source_capture_integrity_seal_authority: false,
      controlled_counterfactual_pair_found: false,
      exact_damage_projection_proven: false,
      exact_operation_order_proven: false,
      exact_integer_rounding_proven: false,
      packet_conservation_proven: false,
      formula_proven: false,
      runtime_authority: false,
      ui_display_authority: false,
      provider_rdps_credit_allowed: false,
    },
    blockers: [
      "the original partial prefixes do not carry integrity seals",
      "the 92,161-sample recovered-prefix comparison contains zero controlled source-transition pairs",
      "all 57 broad review-band pairs lack the required 13100..13105 source-attribute transition",
      "damage-stage binding, operation order, integer rounding, stacking, and conservation remain unproven",
    ],
    next_acquisition: [
      "obtain a new sealed same-build controlled effect-present/effect-absent damage pair",
      "or prove the authoritative server damage operator including order and integer rounding",
    ],
    content_sha256: "",
  };
  report.content_sha256 = digest(report);
  validateReport(report);
  if (fs.existsSync(args.output)) fail(`refusing to overwrite ${args.output}`);
  fs.mkdirSync(path.dirname(path.resolve(args.output)), { recursive: true });
  fs.writeFileSync(args.output, `${JSON.stringify(report, null, 2)}\n`, { flag: "wx" });
  console.log(
    `Wrote partial-prefix frontier: ${recovery.prefix_events} validated prefix events, ${comparison.samples} comparison samples, 0 controlled pairs; formula authority=false.`,
  );
}

function required(options, name) {
  const value = options.get(name);
  if (!value) fail(`${name} is required`);
  return value;
}

function parseOptions(values) {
  const options = new Map();
  for (let index = 0; index < values.length; index += 2) {
    const name = values[index];
    const value = values[index + 1];
    if (!name?.startsWith("--") || value === undefined) fail("arguments must be --name value pairs");
    options.set(name, value);
  }
  return options;
}

async function main() {
  const [command, ...values] = process.argv.slice(2);
  if (command === "verify") {
    const options = parseOptions(values);
    const input = required(options, "--input");
    validateReport(readJson(input, "partial-prefix frontier"));
    console.log(`Verified partial-prefix frontier ${input}`);
    return;
  }
  if (command === "self-test") {
    const fixture = {
      schema_version: SCHEMA_VERSION,
      generated_by: GENERATED_BY,
      game_build: GAME_BUILD,
      effect_id: EFFECT_ID,
      coverage: {
        validated_prefix_events: 1_039_616,
        recovered_canonical_events: 1_039_625,
        complete_gap_bounded_lifecycles: 23,
        safe_source_damage_memberships: 16_376,
        comparison_samples: 92_161,
        controlled_pairs: 0,
        review_band_pairs: 57,
        review_band_rejected_without_source_attribute_transition: 57,
      },
      proof_state: {
        retained_partial_prefix_search_exhausted: true,
        source_capture_integrity_seal_authority: false,
        formula_proven: false,
        runtime_authority: false,
        provider_rdps_credit_allowed: false,
      },
      content_sha256: "",
    };
    fixture.content_sha256 = digest(fixture);
    validateReport(fixture);
    console.log("partial-prefix frontier self-test passed");
    return;
  }
  if (command !== "build") {
    fail("usage: build --source-prefix-audit <json> --target-prefix-audit <json> --recovery-manifest <json> --gap-window-audit <json> --present-cohort <json> --comparison-cohort <json> --counterfactual <json> --output <json> | verify --input <json> | self-test");
  }
  const options = parseOptions(values);
  await build({
    sourcePrefixAudit: required(options, "--source-prefix-audit"),
    targetPrefixAudit: required(options, "--target-prefix-audit"),
    recoveryManifest: required(options, "--recovery-manifest"),
    gapWindowAudit: required(options, "--gap-window-audit"),
    presentCohort: required(options, "--present-cohort"),
    comparisonCohort: required(options, "--comparison-cohort"),
    counterfactual: required(options, "--counterfactual"),
    output: required(options, "--output"),
  });
}

main().catch((error) => {
  console.error(`fatal-spiral partial-prefix frontier failed: ${error.message}`);
  process.exitCode = 1;
});
