#!/usr/bin/env node

import crypto from "node:crypto";
import fs from "node:fs";
import path from "node:path";

const DEFAULT_ROOT = path.join(
  "plugins",
  "games",
  "blue-protocol-star-resonance",
  "research",
  "game-file-inventory",
  "global",
  "steam-24687926",
);

function parseArgs(argv) {
  const args = {
    closure: path.join(DEFAULT_ROOT, "party-skill-static-closure.v1.json"),
    formulaHypothesis: path.join(
      DEFAULT_ROOT,
      "external-damage-formula-hypothesis.v1.json",
    ),
    output: path.join(DEFAULT_ROOT, "rdps-formula-stage-worklist.v1.json"),
  };
  for (let index = 0; index < argv.length; index += 1) {
    const flag = argv[index];
    const value = argv[index + 1];
    if (flag === "--closure" && value) {
      args.closure = value;
      index += 1;
    } else if (flag === "--formula-hypothesis" && value) {
      args.formulaHypothesis = value;
      index += 1;
    } else if (flag === "--output" && value) {
      args.output = value;
      index += 1;
    } else {
      throw new Error(`unknown or incomplete argument: ${flag}`);
    }
  }
  return args;
}

function readJsonWithDigest(filePath) {
  const absolutePath = path.resolve(filePath);
  const bytes = fs.readFileSync(absolutePath);
  return {
    absolutePath,
    sha256: crypto.createHash("sha256").update(bytes).digest("hex"),
    value: JSON.parse(bytes.toString("utf8")),
  };
}

function unique(values) {
  return [...new Set(values)];
}

function baseStages(categories) {
  const stages = [];
  if (categories.includes("party-action-opportunity")) {
    stages.push("action_opportunity_outside_per_hit_formula");
  }
  if (categories.includes("external-target-vulnerability")) {
    stages.push("target_mitigation_or_vulnerability_multiplier");
  }
  if (categories.includes("party-offensive-stat")) {
    stages.push("upstream_offensive_attribute_or_multiplier_unresolved");
  }
  return stages;
}

const SKILL_REFINEMENTS = new Map([
  [1410, []],
  [1422, []],
  [1426, []],
  [2209, []],
  [3303, []],
  [3401, []],
  [3703, []],
  [3957, ["elemental_damage_bucket_candidate"]],
  [3971, ["main_stat_percent", "class_main_stat_to_attack", "action_opportunity_outside_per_hit_formula"]],
  [3974, ["attack_percent", "action_opportunity_outside_per_hit_formula", "provider_owned_damage_exclusion"]],
  [3982, ["target_mitigation_or_vulnerability_multiplier"]],
  [112402, []],
  [112403, []],
  [2002840, []],
]);

const BUFF_REFINEMENTS = new Map([
  [997511, ["attack_percent_or_flat_attack_unresolved"]],
  [997514, ["main_stat_or_secondary_stat_transfer_unresolved"]],
  [997515, ["critical_haste_luck_mastery_or_versatility_unresolved"]],
  [997517, ["offensive_multiplier_bucket_unresolved"]],
  [997518, ["offensive_multiplier_bucket_unresolved"]],
  [997533, ["lifecycle_root_no_direct_formula_stage_proven"]],
  [997534, ["critical_and_lucky"]],
  [997536, ["lifecycle_root_no_direct_formula_stage_proven"]],
  [997537, ["critical_chance"]],
  [997538, ["critical_effect_or_multiplier_unresolved"]],
  [2110121, ["attack_percent_or_flat_attack_unresolved"]],
  [2204471, ["critical_chance"]],
  [2207250, ["offensive_stat_or_range_only_false_positive_unresolved"]],
  [2302120, ["upstream_offensive_attribute_or_multiplier_unresolved"]],
]);

const ROGUE_REFINEMENTS = new Map([
  [103, ["upstream_offensive_attribute_or_multiplier_unresolved"]],
  [195, ["attack_percent_or_flat_attack_unresolved"]],
  [196, ["elemental_damage_bucket_candidate"]],
  [197, ["main_stat_or_secondary_stat_transfer_unresolved"]],
  [199, ["offensive_multiplier_bucket_unresolved"]],
  [208, ["critical_and_lucky"]],
  [209, ["critical_and_lucky"]],
]);

function assertExactIds(rows, refinementMap, kind) {
  const ids = rows.map((row) => row[`${kind}_id`] ?? row.entry_id);
  const missing = ids.filter((id) => !refinementMap.has(id));
  const extra = [...refinementMap.keys()].filter((id) => !ids.includes(id));
  if (missing.length > 0 || extra.length > 0) {
    throw new Error(
      `${kind} adjudication mismatch: missing=${missing.join(",")} extra=${extra.join(",")}`,
    );
  }
}

const args = parseArgs(process.argv.slice(2));
const closureInput = readJsonWithDigest(args.closure);
const formulaInput = readJsonWithDigest(args.formulaHypothesis);
const closure = closureInput.value;
const formula = formulaInput.value;

if (closure.game_build !== formula.game_build) {
  throw new Error(
    `build mismatch: closure=${closure.game_build} formula=${formula.game_build}`,
  );
}

const skills = closure.skill_candidates.filter((row) => row.rdps_relevant_candidate);
const buffs = closure.buff_candidates.filter((row) => row.rdps_relevant_candidate);
const rogueEntries = closure.rogue_party_entry_candidates.filter(
  (row) => row.rdps_relevant_candidate,
);

assertExactIds(skills, SKILL_REFINEMENTS, "skill");
assertExactIds(buffs, BUFF_REFINEMENTS, "buff");
assertExactIds(rogueEntries, ROGUE_REFINEMENTS, "entry");

const skillWorklist = skills.map((row) => ({
  skill_id: row.skill_id,
  localized_name_evidence: row.localized_name_evidence,
  design_name_evidence: row.design_name_evidence,
  support_categories: row.support_categories,
  exact_reviewed_buff_or_status_ids: row.exact_reviewed_buff_or_status_ids,
  graph_state: row.skill_to_buff_graph_state,
  candidate_formula_stages: unique([
    ...baseStages(row.support_categories),
    ...SKILL_REFINEMENTS.get(row.skill_id),
  ]),
  placement_authority: "candidate-only",
  next_proof: row.exact_reviewed_buff_or_status_ids.length > 0
    ? "bind each numeric effect component to one packet attribute or target-state transition and then to affected damage actions"
    : "close the exact numeric skill-to-effect edge before formula placement",
  provider_rdps_credit_allowed: false,
}));

const buffWorklist = buffs.map((row) => ({
  buff_id: row.buff_id,
  level: row.level,
  localized_name_evidence: row.localized_name_evidence,
  design_name_evidence: row.design_name_evidence,
  support_categories: row.support_categories,
  candidate_formula_stages: BUFF_REFINEMENTS.get(row.buff_id),
  placement_evidence: "static category and design-name evidence only unless a packet transition is separately cited",
  next_proof: "observe an exact lifecycle transition, identify the changed numeric attribute family, and replay its equation stage",
  provider_rdps_credit_allowed: false,
}));

const rogueWorklist = rogueEntries.map((row) => ({
  entry_id: row.entry_id,
  entry_type: row.entry_type,
  exact_root_buff_id: row.exact_root_buff_id,
  localized_name_evidence: row.localized_name_evidence,
  candidate_child_buff_family: row.candidate_child_buff_family,
  candidate_formula_stages: ROGUE_REFINEMENTS.get(row.entry_id),
  placement_evidence: "root identity is exact; child edge and formula stage remain candidate-only",
  proof_obligations: row.proof_obligations,
  provider_rdps_credit_allowed: false,
}));

const report = {
  schema_version: 1,
  generated_by: "tools/bpsr-rdps-formula-stage-worklist.mjs",
  game_build: closure.game_build,
  proof_state: "complete-rdps-relevant-static-inventory-partitioned-by-candidate-formula-stage",
  policy: formula.policy,
  inputs: {
    party_skill_static_closure: {
      path: closureInput.absolutePath.replaceAll("\\", "/"),
      sha256: closureInput.sha256,
    },
    external_damage_formula_hypothesis: {
      path: formulaInput.absolutePath.replaceAll("\\", "/"),
      sha256: formulaInput.sha256,
      external_commit: formula.external_source.commit,
    },
  },
  summary: {
    rdps_relevant_skill_ids: skillWorklist.length,
    rdps_relevant_buff_ids: buffWorklist.length,
    rdps_relevant_rogue_entry_ids: rogueWorklist.length,
    skills_with_exact_reviewed_effect_components: skillWorklist.filter(
      (row) => row.exact_reviewed_buff_or_status_ids.length > 0,
    ).length,
    skills_requiring_exact_skill_to_effect_edge: skillWorklist.filter(
      (row) => row.exact_reviewed_buff_or_status_ids.length === 0,
    ).length,
    runtime_formula_authoritative_rows: 0,
    provider_rdps_credit_allowed_rows: 0,
    production_promotion_count: 0,
  },
  formula_stage_taxonomy: formula.hypothesis_stages.map((stage) => ({
    stage: stage.stage,
    current_build_authority: stage.current_build_authority,
  })),
  skill_worklist: skillWorklist,
  buff_worklist: buffWorklist,
  rogue_entry_worklist: rogueWorklist,
  runtime_decision: {
    promoted_effect_ids: [],
    runtime_formula_authority: false,
    ui_rdps_authority: false,
    provider_rdps_credit_allowed: false,
    reason: "stage placement is a bounded worklist, not proof of the exact effect edge, event-time magnitude, stacking, operation order, rounding, or conservation",
  },
};

report.content_sha256 = crypto
  .createHash("sha256")
  .update(JSON.stringify(report))
  .digest("hex");

const outputPath = path.resolve(args.output);
fs.mkdirSync(path.dirname(outputPath), { recursive: true });
fs.writeFileSync(outputPath, `${JSON.stringify(report, null, 2)}\n`);
console.log(
  JSON.stringify({
    output: outputPath,
    summary: report.summary,
    content_sha256: report.content_sha256,
  }),
);
