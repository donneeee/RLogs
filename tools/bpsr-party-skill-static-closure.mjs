import { createHash } from "node:crypto";
import { existsSync, readFileSync, statSync, writeFileSync } from "node:fs";
import path from "node:path";

const SCHEMA_VERSION = 2;
const MAX_INPUT_BYTES = 32 * 1024 * 1024;
const PARTY_RECIPIENT = /(?:all allies|nearby allies|allies within|allies standing|allies hit|affected allies|up to\s+\d+\s+(?:nearby\s+)?allies|party members?|team members?|teammates?|other players|all companions|队友|全队|队伍中|队员|友方|友军|团队)/iu;
const TEAM_DESIGN = /(?:\[团队\]|【团队】|团队|队友|全队|友方|友军)/u;
const TARGET_SUPPORT = /(?:(?:enemies|targets).{0,120}(?:vulnerab|damage taken|defen[cs]e reduced|resistance reduced|resilience reduced)|(?:vulnerab|damage taken|defen[cs]e reduced|resistance reduced|reducing.{0,80}(?:defen[cs]e|\bDEF\b)|reduce[sd]?.{0,80}(?:defen[cs]e|\bDEF\b)).{0,120}(?:enemies|targets)|易伤|防御.{0,12}(?:降低|减少)|抗性.{0,12}(?:降低|减少))/iu;
const [command = "help", ...argv] = process.argv.slice(2);

try {
  if (command === "build") build(parseArgs(argv));
  else if (command === "verify") verify(path.resolve(required(parseArgs(argv), "input")));
  else if (command === "self-test") selfTest();
  else usage(command === "help" ? 0 : 1);
} catch (error) {
  console.error(error instanceof Error ? error.message : String(error));
  process.exit(1);
}

function build(args) {
  const buildId = required(args, "build");
  if (!/^\d+$/.test(buildId)) throw new Error("Build must contain only ASCII digits");
  const inputs = {
    manifest: path.resolve(required(args, "manifest")),
    skill_table: path.resolve(required(args, "skill-table")),
    skill_effect_table: path.resolve(required(args, "skill-effect-table")),
    buff_table: path.resolve(required(args, "buff-table")),
    rogue_entry_table: path.resolve(required(args, "rogue-entry-table")),
    rogue_entry_description_table: path.resolve(required(args, "rogue-entry-description-table")),
    talent_table: path.resolve(required(args, "talent-table")),
    current_aoyi_origin_ledger: path.resolve(required(args, "current-aoyi-origin-ledger")),
    recipient_scope_ledger: path.resolve(required(args, "recipient-scope-ledger")),
    reviewed_link_candidates: path.resolve(required(args, "reviewed-link-candidates")),
  };
  const output = path.resolve(required(args, "output"));
  const manifest = readBoundedJson(inputs.manifest, "build manifest");
  const skillTable = readBoundedJson(inputs.skill_table, "SkillTable");
  const skillEffectTable = readBoundedJson(inputs.skill_effect_table, "SkillEffectTable");
  const buffTable = readBoundedJson(inputs.buff_table, "BuffTable");
  const rogueEntryTable = readBoundedJson(inputs.rogue_entry_table, "RogueEntryTable");
  const rogueEntryDescriptionTable = readBoundedJson(
    inputs.rogue_entry_description_table,
    "RogueEntryDescriptionTable",
  );
  const talentTable = readBoundedJson(inputs.talent_table, "TalentTable");
  const aoyiOriginLedger = readBoundedJson(
    inputs.current_aoyi_origin_ledger,
    "current Aoyi origin ledger",
  );
  const scopeLedger = readBoundedJson(inputs.recipient_scope_ledger, "recipient-scope ledger");
  const reviewedLinkCandidates = readBoundedJson(
    inputs.reviewed_link_candidates,
    "reviewed link candidates",
  );
  validateInputs(
    manifest,
    scopeLedger,
    aoyiOriginLedger,
    reviewedLinkCandidates,
    buildId,
    inputs,
  );
  const analysis = analyze(
    skillTable,
    skillEffectTable,
    buffTable,
    rogueEntryTable,
    rogueEntryDescriptionTable,
    talentTable,
    aoyiOriginLedger,
    scopeLedger,
    reviewedLinkCandidates,
  );
  const report = {
    schema_version: SCHEMA_VERSION,
    generated_by: "tools/bpsr-party-skill-static-closure.mjs",
    game_build: buildId,
    proof_state: "exact-build-party-support-static-surface-enumerated-runtime-formulas-open",
    policy: {
      exact_numeric_skill_effect_buff_ids_and_build_are_authoritative: true,
      localized_names_and_descriptions_are_discovery_evidence_only: true,
      description_percentages_are_runtime_formula_authority: false,
      static_target_fields_are_runtime_recipient_selector_authority: false,
      remote_player_cast_packets_required: false,
      remote_player_cast_packets_treated_as_zero: false,
      remote_player_cast_packets_synthesized: false,
      unresolved_skill_to_buff_edges_preserved: true,
      reviewed_candidate_links_are_exact_runtime_edges: false,
      unknown_party_support_rows_hidden: false,
      provider_rdps_credit_allowed: false,
      runtime_promotion_allowed: false,
      ui_rdps_display_allowed: false,
    },
    bounded_processing: {
      maximum_input_bytes_each: MAX_INPUT_BYTES,
      whole_rlog_cohort_deserialized: false,
      raw_rlogs_read: false,
      recommended_node_heap_mib: 256,
    },
    inputs: Object.fromEntries(Object.entries(inputs).map(([key, value]) => [key, descriptor(value)])),
    ...analysis,
  };
  report.content_sha256 = contentHash(report);
  validateReport(report);
  writeFileSync(output, `${JSON.stringify(report, null, 2)}\n`, "utf8");
  console.log(
    `Party-skill static closure built for ${buildId}: ${report.summary.skill_candidates} skill rows, ` +
      `${report.summary.buff_candidates} buff rows, ${report.summary.rdps_relevant_skill_candidates} ` +
      "rDPS-relevant skill candidates; runtime authority remains open.",
  );
}

function verify(input) {
  const report = readBoundedJson(input, "party-skill static closure");
  validateReport(report);
  for (const descriptorValue of Object.values(report.inputs)) {
    const current = descriptor(path.resolve(descriptorValue.path));
    if (current.bytes !== descriptorValue.bytes || current.sha256 !== descriptorValue.sha256) {
      throw new Error(`Party-skill input changed: ${descriptorValue.path}`);
    }
  }
  const skillTable = readBoundedJson(path.resolve(report.inputs.skill_table.path), "SkillTable");
  const skillEffectTable = readBoundedJson(
    path.resolve(report.inputs.skill_effect_table.path),
    "SkillEffectTable",
  );
  const buffTable = readBoundedJson(path.resolve(report.inputs.buff_table.path), "BuffTable");
  const rogueEntryTable = readBoundedJson(
    path.resolve(report.inputs.rogue_entry_table.path),
    "RogueEntryTable",
  );
  const rogueEntryDescriptionTable = readBoundedJson(
    path.resolve(report.inputs.rogue_entry_description_table.path),
    "RogueEntryDescriptionTable",
  );
  const talentTable = readBoundedJson(
    path.resolve(report.inputs.talent_table.path),
    "TalentTable",
  );
  const aoyiOriginLedger = readBoundedJson(
    path.resolve(report.inputs.current_aoyi_origin_ledger.path),
    "current Aoyi origin ledger",
  );
  const scopeLedger = readBoundedJson(
    path.resolve(report.inputs.recipient_scope_ledger.path),
    "recipient-scope ledger",
  );
  const reviewedLinkCandidates = readBoundedJson(
    path.resolve(report.inputs.reviewed_link_candidates.path),
    "reviewed link candidates",
  );
  const expected = analyze(
    skillTable,
    skillEffectTable,
    buffTable,
    rogueEntryTable,
    rogueEntryDescriptionTable,
    talentTable,
    aoyiOriginLedger,
    scopeLedger,
    reviewedLinkCandidates,
  );
  if (stableStringify(expected) !== stableStringify(selectAnalysis(report))) {
    throw new Error("Party-skill static closure does not reproduce from its inputs");
  }
  console.log(
    `Party-skill static closure verified for ${report.game_build}: ` +
      `${report.summary.skill_candidates} skills, ${report.summary.buff_candidates} buffs, ` +
      "no runtime or UI authority.",
  );
}

function analyze(
  skillTable,
  skillEffectTable,
  buffTable,
  rogueEntryTable,
  rogueEntryDescriptionTable,
  talentTable,
  aoyiOriginLedger,
  scopeLedger,
  reviewedLinkCandidates,
) {
  const reverseBuffsBySkill = new Map();
  for (const row of Object.values(buffTable ?? {})) {
    const skillId = Number(row?.SkillId);
    const buffId = Number(row?.Id);
    if (!Number.isSafeInteger(skillId) || skillId <= 0 ||
      !Number.isSafeInteger(buffId) || buffId <= 0) continue;
    if (!reverseBuffsBySkill.has(skillId)) reverseBuffsBySkill.set(skillId, []);
    reverseBuffsBySkill.get(skillId).push(buffId);
  }
  const reviewedBySkill = new Map();
  const aoyiBySkill = new Map((aoyiOriginLedger?.skills ?? []).map((row) => [
    Number(row?.skill_id),
    row,
  ]));
  const reviewedBySource = new Map();
  const candidateLinksBySkill = new Map((reviewedLinkCandidates?.reviewed_candidates ?? []).map(
    (row) => [Number(row.skill_id), row],
  ));
  const candidateLinkedBuffIds = new Set((reviewedLinkCandidates?.reviewed_candidates ?? [])
    .flatMap((row) => row?.candidate_buff_ids ?? []).map((row) => Number(row.buff_id)));
  const partyTalentRows = numericEntries(talentTable).map(([key, row]) => ({
    talent_id: exactPositiveId(row?.Id ?? key, "talent ID"),
    localized_name_evidence: nullableString(row?.TalentName),
    design_name_evidence: nullableString(row?.Des),
    description_evidence: nullableString(row?.TalentDes),
    talent_effect: structuredClone(row?.TalentEffect ?? []),
    buff_parameters: structuredClone(row?.BuffPar ?? []),
    row_sha256: valueHash(row),
  })).filter((row) => hasPartyRecipientClaim(evidenceText({
    NameDesign: row.design_name_evidence,
    Name: row.localized_name_evidence,
    Desc: row.description_evidence,
  })));
  const partyTalentEvidenceBySkill = new Map();
  for (const talent of partyTalentRows) {
    for (const effect of talent.talent_effect) {
      if (!Array.isArray(effect)) continue;
      for (const value of effect.slice(1)) {
        const skillId = Number(value);
        if (!Number.isSafeInteger(skillId) || skillId <= 0 ||
          skillTable[String(skillId)] === undefined) continue;
        if (!partyTalentEvidenceBySkill.has(skillId)) partyTalentEvidenceBySkill.set(skillId, []);
        partyTalentEvidenceBySkill.get(skillId).push({
          ...structuredClone(talent),
          matching_talent_effect_tuple: structuredClone(effect),
          transformation_opcode_semantics_authoritative: false,
          runtime_formula_authority: false,
        });
      }
    }
  }
  for (const source of scopeLedger?.candidates ?? []) {
    reviewedBySource.set(String(source.source_id), source);
    for (const evidence of source?.current_component_evidence ?? []) {
      const skillId = Number(evidence?.skill_id);
      if (!Number.isSafeInteger(skillId) || skillId <= 0) continue;
      if (!reviewedBySkill.has(skillId)) reviewedBySkill.set(skillId, []);
      reviewedBySkill.get(skillId).push({
        source_rule_id: String(source.source_rule_id),
        source_id: String(source.source_id),
        component_id: String(evidence.component_id),
        exact_effect_ids: numericIds(evidence.effect_ids),
        recipient_scope: String(evidence.recipient_scope),
        rdps_disposition: String(evidence.rdps_disposition),
        proof_state: String(evidence.proof_state),
      });
    }
  }

  const skills = [];
  const staticallyLinkedBuffIds = new Set();
  for (const [key, row] of numericEntries(skillTable)) {
    const skillId = exactPositiveId(row?.Id ?? key, "skill ID");
    const talentRouteEvidence = partyTalentEvidenceBySkill.get(skillId) ?? [];
    const directSkillText = evidenceText(row);
    const text = [
      directSkillText,
      ...talentRouteEvidence.map((entry) => entry.description_evidence ?? ""),
    ].filter(Boolean).join("\n");
    const directPartyScope = hasPartyRecipientClaim(directSkillText);
    const partyScope = hasPartyRecipientClaim(text);
    const targetSupport = hasExternalTargetSupportClaim(text);
    if (!partyScope && !targetSupport && talentRouteEvidence.length === 0 &&
      !candidateLinksBySkill.has(skillId)) continue;
    const effectIds = numericIds(row?.EffectIDs);
    const effectRows = effectIds.map((effectId) => {
      const effect = skillEffectTable[String(effectId)] ?? null;
      return effect
        ? {
          skill_effect_id: effectId,
          bound_skill_id: Number(effect.SkillId),
          level: Number(effect.Level),
          skill_attr_expressions: normalizeSkillAttributes(effect.SkillAttrDes),
          skill_learn_buff_ids: numericIds(effect.SkillLearnBuffs),
          install_skill_add_buff_ids: numericIds(effect.InstallSkillAddBuffs),
          row_sha256: valueHash(effect),
        }
        : { skill_effect_id: effectId, missing: true };
    });
    const categories = supportCategories(text, partyScope, targetSupport);
    const reviewed = (reviewedBySkill.get(skillId) ?? []).sort(compareComponents);
    const aoyiEvidence = aoyiBySkill.get(skillId) ?? null;
    const candidateLinkEvidence = candidateLinksBySkill.get(skillId) ?? null;
    const aoyiComponentRoutes = structuredClone(aoyiEvidence?.component_routes ?? []);
    const aoyiActiveParameterEvidence = structuredClone(
      aoyiEvidence?.active_modifier_parameter_evidence ?? [],
    );
    const aoyiBuffIds = uniqueSortedNumbers(aoyiComponentRoutes.flatMap(
      (entry) => numericIds(entry.effect_ids),
    ).filter((effectId) => buffTable[String(effectId)] !== undefined));
    const reviewedBuffIds = uniqueSortedNumbers(reviewed.flatMap((entry) => entry.exact_effect_ids));
    const learnedBuffIds = uniqueSortedNumbers(effectRows.flatMap(
      (entry) => entry.skill_learn_buff_ids ?? [],
    ));
    const installedBuffIds = uniqueSortedNumbers(effectRows.flatMap(
      (entry) => entry.install_skill_add_buff_ids ?? [],
    ));
    const reverseBuffIds = uniqueSortedNumbers(reverseBuffsBySkill.get(skillId) ?? []);
    const exactBuffIds = uniqueSortedNumbers([
      ...reviewedBuffIds,
      ...aoyiBuffIds,
      ...learnedBuffIds,
      ...installedBuffIds,
      ...reverseBuffIds,
    ]);
    exactBuffIds.forEach((buffId) => staticallyLinkedBuffIds.add(buffId));
    const directTableBinding = learnedBuffIds.length + installedBuffIds.length + reverseBuffIds.length > 0;
    skills.push({
      skill_id: skillId,
      skill_effect_ids: effectIds,
      localized_name_evidence: nullableString(row?.Name),
      design_name_evidence: nullableString(row?.NameDesign),
      description_evidence: nullableString(row?.Desc),
      presentation_state: presentationState(row),
      static_target_fields: {
        target_type: Number(row?.TargetType),
        skill_target_range_type: Number(row?.SkillTargetRangeType),
        skill_range_type: Number(row?.SkillRangeType),
        is_aoe: row?.IsAoe === true,
      },
      discovery_reasons: [
        ...(directPartyScope ? ["explicit-party-or-friendly-recipient-language"] : []),
        ...(talentRouteEvidence.length > 0 ? ["exact-build-party-talent-effect-tuple"] : []),
        ...(targetSupport ? ["explicit-enemy-target-support-debuff-language"] : []),
      ],
      support_categories: categories,
      rdps_relevant_candidate: categories.some((value) =>
        ["party-offensive-stat", "party-action-opportunity", "external-target-vulnerability"]
          .includes(value)),
      description_numeric_claims: descriptionClaims(row?.Desc),
      skill_effect_rows: effectRows,
      reviewed_component_bindings: reviewed,
      current_aoyi_component_routes: aoyiComponentRoutes,
      current_aoyi_active_modifier_parameter_evidence: aoyiActiveParameterEvidence,
      exact_build_party_talent_route_evidence: talentRouteEvidence,
      exact_skill_to_buff_edges: [
        ...learnedBuffIds.map((buff_id) => ({
          buff_id, relationship: "SkillEffectTable.SkillLearnBuffs",
        })),
        ...installedBuffIds.map((buff_id) => ({
          buff_id, relationship: "SkillEffectTable.InstallSkillAddBuffs",
        })),
        ...reverseBuffIds.map((buff_id) => ({
          buff_id, relationship: "BuffTable.SkillId",
        })),
        ...reviewedBuffIds.map((buff_id) => ({
          buff_id, relationship: "reviewed-current-component-binding",
        })),
        ...aoyiBuffIds.map((buff_id) => ({
          buff_id, relationship: "current-aoyi-origin-ledger-component-route",
        })),
      ],
      reviewed_candidate_skill_to_buff_links: candidateLinkEvidence
        ? structuredClone(candidateLinkEvidence.candidate_buff_ids).map((entry) => ({
          ...entry,
          exact_skill_to_buff_edge_proven: false,
          runtime_attribution_enabled: false,
        }))
        : [],
      exact_reviewed_buff_or_status_ids: exactBuffIds,
      skill_to_buff_graph_state: directTableBinding
        ? "exact-selected-table-skill-to-buff-edge-present"
        : aoyiBuffIds.length > 0
        ? "reviewed-current-aoyi-component-routes-present"
        : reviewedBuffIds.length > 0
        ? "reviewed-current-component-binding-present"
        : "unresolved-no-exact-skill-to-buff-edge-in-selected-static-tables",
      proof_obligations: proofObligations(exactBuffIds.length > 0),
      runtime_formula_authority: false,
      provider_rdps_credit_allowed: false,
      row_sha256: valueHash(row),
    });
  }

  const buffs = [];
  for (const [key, row] of numericEntries(buffTable)) {
    const text = evidenceText(row);
    const buffId = exactPositiveId(row?.Id ?? key, "buff ID");
    const partyDiscovery = hasPartyRecipientClaim(text) || hasTeamDesignMarker(text);
    const linkedDiscovery = staticallyLinkedBuffIds.has(buffId);
    const candidateLinkedDiscovery = candidateLinkedBuffIds.has(buffId);
    if (!partyDiscovery && !linkedDiscovery && !candidateLinkedDiscovery) continue;
    const categories = supportCategories(text, true, false);
    buffs.push({
      buff_id: buffId,
      level: Number(row?.Level),
      localized_name_evidence: nullableString(row?.Name),
      design_name_evidence: nullableString(row?.NameDesign),
      description_evidence: nullableString(row?.Desc),
      note_evidence: nullableString(row?.Note),
      discovery_reasons: [
        ...(partyDiscovery ? ["explicit-party-or-team-language"] : []),
        ...(linkedDiscovery ? ["exact-or-reviewed-party-skill-edge"] : []),
        ...(candidateLinkedDiscovery ? ["reviewed-candidate-only-skill-edge"] : []),
      ],
      support_categories: categories,
      rdps_relevant_candidate: categories.some((value) =>
        ["party-offensive-stat", "party-action-opportunity"].includes(value)),
      static_lifecycle_fields: {
        repeat_add_rule: structuredClone(row?.RepeatAddRule ?? []),
        destroy_param: structuredClone(row?.DestroyParam ?? []),
        time_refresh_type: Number(row?.TimeRefreshType),
        delete_dead: row?.DeleteDead === true,
        delete_offline: row?.DeleteOffline === true,
        delete_change_scene: row?.DeleteChangeScene === true,
        delete_source_dead: row?.DeleteSourceDead === true,
      },
      description_numeric_claims: descriptionClaims(`${row?.Desc ?? ""} ${row?.Note ?? ""}`),
      runtime_magnitude_authority: false,
      stacking_operation_order_authority: false,
      integer_rounding_authority: false,
      provider_rdps_credit_allowed: false,
      row_sha256: valueHash(row),
    });
  }

  const rogueRootBuffIds = uniqueSortedNumbers(Object.values(rogueEntryTable ?? {})
    .map((row) => Number(row?.BuffId)).filter((value) => Number.isSafeInteger(value) && value > 0));
  const rogueEntries = [];
  for (const [key, row] of numericEntries(rogueEntryTable)) {
    const entryId = exactPositiveId(row?.EntryId ?? key, "rogue entry ID");
    const rootBuffId = exactPositiveId(row?.BuffId, "rogue entry root buff ID");
    const rootBuff = buffTable[String(rootBuffId)] ?? null;
    const descriptionRows = normalizeRogueDescriptions(row, rogueEntryDescriptionTable);
    const text = [
      row?.EntryName,
      ...descriptionRows.map((entry) => entry.content_evidence),
      rootBuff?.NameDesign,
      rootBuff?.Name,
      rootBuff?.Desc,
    ].filter((value) => typeof value === "string").map(stripMarkup).join("\n");
    const partyDiscovery = hasPartyRecipientClaim(text) || hasTeamDesignMarker(text);
    if (!partyDiscovery) continue;
    const targetSupport = hasExternalTargetSupportClaim(text);
    const categories = supportCategories(text, true, targetSupport);
    const nextRoot = rogueRootBuffIds.find((value) => value > rootBuffId) ?? Number.MAX_SAFE_INTEGER;
    const candidateChildBuffIds = numericEntries(buffTable).map(([buffKey, buffRow]) => ({
      buff_id: Number(buffRow?.Id ?? buffKey),
      design: String(buffRow?.NameDesign ?? ""),
    })).filter((candidate) => candidate.buff_id > rootBuffId && candidate.buff_id < nextRoot &&
      hasTeamDesignMarker(candidate.design)).map((candidate) => candidate.buff_id);
    const reviewed = reviewedBySource.get(`season-rogue-entry:${entryId}`) ?? null;
    rogueEntries.push({
      entry_id: entryId,
      entry_type: Number(row?.EntryType),
      exact_root_buff_id: rootBuffId,
      localized_name_evidence: nullableString(row?.EntryName),
      applicable_professions: numericIds(row?.ApplicableProfessions),
      description_rows: descriptionRows,
      description_numeric_claims: descriptionClaims(
        descriptionRows.map((entry) => entry.content_evidence ?? "").join(" "),
      ),
      discovery_reasons: [
        ...(hasPartyRecipientClaim(text) ? ["explicit-party-or-friendly-recipient-language"] : []),
        ...(hasTeamDesignMarker(text) ? ["exact-root-buff-team-design-marker"] : []),
      ],
      support_categories: categories,
      rdps_relevant_candidate: categories.some((value) =>
        ["party-offensive-stat", "party-action-opportunity", "external-target-vulnerability"]
          .includes(value)),
      exact_entry_to_root_buff_edge: {
        source_table: "RogueEntryTable",
        source_field: "BuffId",
        target_table: "BuffTable",
        target_buff_id: rootBuffId,
        target_present: rootBuff !== null,
      },
      candidate_child_buff_family: candidateChildBuffIds.map((buff_id) => ({
        buff_id,
        evidence: "contiguous-id-and-localized-team-design-family-only",
        exact_runtime_edge_proven: false,
      })),
      reviewed_scope_ledger_binding: reviewed ? {
        source_rule_id: String(reviewed.source_rule_id),
        scope_queue: String(reviewed.scope_queue),
        effect_ids: numericIds(reviewed.effect_ids),
        current_build_promotion_eligible: reviewed.current_build_promotion_eligible === true,
      } : null,
      root_buff_static_lifecycle_fields: rootBuff ? {
        repeat_add_rule: structuredClone(rootBuff.RepeatAddRule ?? []),
        destroy_param: structuredClone(rootBuff.DestroyParam ?? []),
        time_refresh_type: Number(rootBuff.TimeRefreshType),
      } : null,
      proof_obligations: [
        ...(candidateChildBuffIds.length > 0 ? ["exact-root-to-child-buff-runtime-edge"] : []),
        "provider-ownership",
        "recipient-selector-cap-radius-priority-and-party-membership",
        "runtime-magnitude-and-level-selection",
        "duration-lifecycle",
        "stacking-overwrite-and-operation-order",
        "integer-rounding",
        "matching-build-canonical-conservation-replay",
      ],
      runtime_formula_authority: false,
      provider_rdps_credit_allowed: false,
      row_sha256: valueHash(row),
    });
  }

  skills.sort((left, right) => left.skill_id - right.skill_id);
  buffs.sort((left, right) => left.buff_id - right.buff_id);
  rogueEntries.sort((left, right) => left.entry_id - right.entry_id);
  const missingSkillEffectRows = skills.reduce(
    (sum, skill) => sum + skill.skill_effect_rows.filter((row) => row.missing === true).length,
    0,
  );
  const categoryCounts = countCategories([
    ...skills.map((row) => row.support_categories),
    ...buffs.map((row) => row.support_categories),
    ...rogueEntries.map((row) => row.support_categories),
  ]);
  return {
    summary: {
      skill_table_rows: Object.keys(skillTable).length,
      skill_effect_table_rows: Object.keys(skillEffectTable).length,
      buff_table_rows: Object.keys(buffTable).length,
      rogue_entry_table_rows: Object.keys(rogueEntryTable).length,
      talent_table_rows: Object.keys(talentTable).length,
      explicit_party_talent_rows: partyTalentRows.length,
      skill_candidates: skills.length,
      skill_candidates_with_reviewed_component_binding:
        skills.filter((row) => row.reviewed_component_bindings.length > 0).length,
      skill_candidates_with_current_aoyi_component_routes:
        skills.filter((row) => row.current_aoyi_component_routes.length > 0).length,
      skill_candidates_with_exact_selected_table_buff_edge:
        skills.filter((row) => row.skill_to_buff_graph_state ===
          "exact-selected-table-skill-to-buff-edge-present").length,
      skill_candidates_with_reviewed_candidate_only_buff_links:
        skills.filter((row) => row.reviewed_candidate_skill_to_buff_links.length > 0).length,
      skill_candidates_with_exact_build_party_talent_route_evidence:
        skills.filter((row) => row.exact_build_party_talent_route_evidence.length > 0).length,
      reviewed_candidate_only_skill_to_buff_links:
        skills.reduce((sum, row) => sum + row.reviewed_candidate_skill_to_buff_links.length, 0),
      skill_candidates_with_unresolved_skill_to_buff_graph:
        skills.filter((row) => row.skill_to_buff_graph_state ===
          "unresolved-no-exact-skill-to-buff-edge-in-selected-static-tables").length,
      missing_referenced_skill_effect_rows: missingSkillEffectRows,
      rdps_relevant_skill_candidates: skills.filter((row) => row.rdps_relevant_candidate).length,
      buff_candidates: buffs.length,
      rdps_relevant_buff_candidates: buffs.filter((row) => row.rdps_relevant_candidate).length,
      rogue_party_entry_candidates: rogueEntries.length,
      rdps_relevant_rogue_party_entry_candidates:
        rogueEntries.filter((row) => row.rdps_relevant_candidate).length,
      rogue_party_entries_with_exact_root_buff_edge:
        rogueEntries.filter((row) => row.exact_entry_to_root_buff_edge.target_present).length,
      rogue_party_entries_with_candidate_child_buff_family:
        rogueEntries.filter((row) => row.candidate_child_buff_family.length > 0).length,
      category_counts: categoryCounts,
      runtime_formula_authoritative_skills: 0,
      runtime_formula_authoritative_buffs: 0,
      provider_rdps_credit_allowed_rows: 0,
      hidden_omissions: 0,
    },
    discovery_contract: {
      explicit_party_recipient_language_required_for_friendly_scope: true,
      explicit_enemy_debuff_language_required_for_external_target_support: true,
      generic_cooperation_flavor_text_is_not_a_party_scope_match: true,
      selected_tables: [
        "SkillTable",
        "SkillEffectTable",
        "BuffTable",
        "RogueEntryTable",
        "RogueEntryDescriptionTable",
        "TalentTable",
      ],
      excluded_domains: [
        "seasonal rogue-entry source graphs not represented as SkillTable rows",
        "native-only or script-only selectors without decoded table evidence",
        "implicit party behavior without explicit selected-table recipient evidence",
      ],
      excluded_domains_are_follow_up_obligations_not_claimed_complete: true,
      reviewed_candidate_link_ledger_is_discovery_evidence_not_runtime_authority: true,
    },
    skill_candidates: skills,
    buff_candidates: buffs,
    rogue_party_entry_candidates: rogueEntries,
    remaining_global_obligations: [
      "prove exact root-to-child edges for seasonal team-entry buff families",
      "enumerate native and script recipient selectors not represented in selected decoded tables",
      "prove exact skill-to-buff/status edges for every unresolved skill candidate",
      "prove exact recipient caps, radii, prioritization, and party membership checks",
      "prove per-level magnitudes, duration lifecycle, stacking and overwrite behavior",
      "prove server operation order and integer rounding for every damage-affecting component",
      "run matching-build provider-recipient lifecycle and canonical conservation replay",
    ],
    runtime_decision: {
      provider_rdps_credit_allowed: false,
      runtime_catalog_promotion_allowed: false,
      ui_rdps_display_allowed: false,
      ordinary_damage_totals_unchanged: true,
    },
  };
}

function validateInputs(
  manifest,
  ledger,
  aoyiOriginLedger,
  reviewedLinkCandidates,
  buildId,
  inputs,
) {
  if (String(manifest?.gameBuild) !== buildId || String(ledger?.static_game_build) !== buildId ||
    String(aoyiOriginLedger?.game_build) !== buildId ||
    Number(reviewedLinkCandidates?.schema_version) !== 1 ||
    String(reviewedLinkCandidates?.game_build) !== buildId ||
    Number(aoyiOriginLedger?.summary?.enabled_for_rdps) !== 0 ||
    ledger?.policy?.static_description_proves_packet_recipient !== false ||
    ledger?.policy?.historical_scope_promotes_current_build !== false ||
    ledger?.policy?.current_component_scope_enables_runtime_attribution !== false ||
    ledger?.policy?.unresolved_evidence_hidden !== false ||
    Number(ledger?.summary?.candidates) !== Number(ledger?.candidates?.length) ||
    reviewedLinkCandidates?.policy?.exact_numeric_ids_and_build_are_authoritative !== true ||
    reviewedLinkCandidates?.policy?.candidate_relationship_is_exact_skill_to_buff_edge !== false ||
    reviewedLinkCandidates?.policy?.candidate_relationship_enables_runtime_attribution !== false ||
    reviewedLinkCandidates?.policy?.remote_player_cast_packets_required !== false ||
    reviewedLinkCandidates?.policy?.remote_player_cast_packets_treated_as_zero !== false ||
    reviewedLinkCandidates?.policy?.remote_player_cast_packets_synthesized !== false ||
    reviewedLinkCandidates?.policy?.unresolved_candidates_hidden !== false ||
    reviewedLinkCandidates?.policy?.provider_rdps_credit_allowed !== false) {
    throw new Error("Party-skill input identity or fail-closed scope-ledger policy is invalid");
  }
  const manifestFiles = new Map((manifest.files ?? []).map((entry) => [entry.relativePath, entry]));
  for (const [relativePath, inputKey] of [
    ["SkillTable.json", "skill_table"],
    ["SkillEffectTable.json", "skill_effect_table"],
    ["BuffTable.json", "buff_table"],
    ["RogueEntryTable.json", "rogue_entry_table"],
    ["RogueEntryDescriptionTable.json", "rogue_entry_description_table"],
    ["TalentTable.json", "talent_table"],
  ]) {
    const entry = manifestFiles.get(relativePath);
    const current = descriptor(inputs[inputKey]);
    if (entry?.authority !== "exact-current-build-static-data" ||
      Number(entry.bytes) !== current.bytes || String(entry.sha256) !== current.sha256) {
      throw new Error(`${relativePath} is not bound to the exact-build manifest`);
    }
  }
  const skillTable = readBoundedJson(inputs.skill_table, "SkillTable candidate validation");
  const buffTable = readBoundedJson(inputs.buff_table, "BuffTable candidate validation");
  const seenSkills = new Set();
  for (const candidate of reviewedLinkCandidates?.reviewed_candidates ?? []) {
    const skillId = exactPositiveId(candidate?.skill_id, "candidate skill ID");
    const skillEffectId = exactPositiveId(candidate?.skill_effect_id, "candidate skill-effect ID");
    if (seenSkills.has(skillId)) throw new Error(`Duplicate reviewed candidate skill ${skillId}`);
    seenSkills.add(skillId);
    const skill = skillTable[String(skillId)];
    if (!skill || valueHash(skill) !== String(candidate?.expected_skill_row_sha256) ||
      !numericIds(skill?.EffectIDs).includes(skillEffectId)) {
      throw new Error(`Reviewed candidate skill ${skillId} drifted from exact-build evidence`);
    }
    if (!Array.isArray(candidate?.candidate_buff_ids) || candidate.candidate_buff_ids.length === 0) {
      throw new Error(`Reviewed candidate skill ${skillId} has no retained buff candidates`);
    }
    const seenBuffs = new Set();
    for (const link of candidate.candidate_buff_ids) {
      const buffId = exactPositiveId(link?.buff_id, "candidate buff ID");
      if (seenBuffs.has(buffId)) throw new Error(`Duplicate candidate buff ${buffId} for skill ${skillId}`);
      seenBuffs.add(buffId);
      const buff = buffTable[String(buffId)];
      if (!buff || valueHash(buff) !== String(link?.expected_buff_row_sha256) ||
        link?.exact_skill_to_buff_edge_proven === true ||
        link?.runtime_attribution_enabled === true) {
        throw new Error(`Reviewed candidate buff ${buffId} drifted or was promoted without proof`);
      }
    }
  }
}

function validateReport(report) {
  if (Number(report?.schema_version) !== SCHEMA_VERSION ||
    report?.generated_by !== "tools/bpsr-party-skill-static-closure.mjs" ||
    !/^\d+$/.test(String(report?.game_build)) || report?.content_sha256 !== contentHash(report) ||
    report?.policy?.exact_numeric_skill_effect_buff_ids_and_build_are_authoritative !== true ||
    report?.policy?.localized_names_and_descriptions_are_discovery_evidence_only !== true ||
    report?.policy?.remote_player_cast_packets_required !== false ||
    report?.policy?.remote_player_cast_packets_treated_as_zero !== false ||
    report?.policy?.remote_player_cast_packets_synthesized !== false ||
    report?.policy?.reviewed_candidate_links_are_exact_runtime_edges !== false ||
    report?.policy?.provider_rdps_credit_allowed !== false ||
    report?.runtime_decision?.provider_rdps_credit_allowed !== false ||
    report?.runtime_decision?.runtime_catalog_promotion_allowed !== false ||
    report?.runtime_decision?.ui_rdps_display_allowed !== false ||
    report?.runtime_decision?.ordinary_damage_totals_unchanged !== true ||
    Number(report?.summary?.skill_candidates) !== report?.skill_candidates?.length ||
    Number(report?.summary?.buff_candidates) !== report?.buff_candidates?.length ||
    Number(report?.summary?.rogue_party_entry_candidates) !==
      report?.rogue_party_entry_candidates?.length ||
    Number(report?.summary?.provider_rdps_credit_allowed_rows) !== 0 ||
    Number(report?.summary?.hidden_omissions) !== 0 ||
    Number(report?.summary?.reviewed_candidate_only_skill_to_buff_links) !==
      report?.skill_candidates?.reduce(
        (sum, row) => sum + Number(row?.reviewed_candidate_skill_to_buff_links?.length ?? 0),
        0,
      ) ||
    report?.skill_candidates?.some((row) => row?.reviewed_candidate_skill_to_buff_links?.some(
      (link) => link?.exact_skill_to_buff_edge_proven !== false ||
        link?.runtime_attribution_enabled !== false,
    )) ||
    report?.skill_candidates?.some((row) => row?.exact_build_party_talent_route_evidence?.some(
      (entry) => entry?.transformation_opcode_semantics_authoritative !== false ||
        entry?.runtime_formula_authority !== false,
    )) ||
    !Array.isArray(report?.remaining_global_obligations) ||
    report.remaining_global_obligations.length === 0) {
    throw new Error("Party-skill static closure identity, conservation, or fail-closed policy is invalid");
  }
}

function hasPartyRecipientClaim(text) {
  return PARTY_RECIPIENT.test(text);
}

function hasTeamDesignMarker(text) {
  return TEAM_DESIGN.test(text);
}

function hasExternalTargetSupportClaim(text) {
  return TARGET_SUPPORT.test(text);
}

function supportCategories(text, partyScope, targetSupport) {
  const result = new Set();
  if (targetSupport) result.add("external-target-vulnerability");
  const partySegments = text.split(/[.!?。；;\n]+/u).filter((segment) =>
    hasPartyRecipientClaim(segment) || hasTeamDesignMarker(segment)
  );
  const explicitTeamOffenseMarker = hasTeamDesignMarker(text) &&
    /(?:攻击加成|暴击|幸运|伤害加成|属性传导|主属性|增效)/u.test(text);
  if (partyScope && (explicitTeamOffenseMarker || partySegments.some((segment) =>
    /(?:(?:increase|increased|gain|grants?|boost|bonus|提高|增加|加成|附加).{0,100}(?:\bATK\b|attack spd|casting spd|main stats?|crit|lucky|DMG Boost|damage dealt|elemental damage|all-element bonus|PHY Boost|MAG Boost|penetration|攻击|暴击|幸运|主属性|伤害加成)|(?:\bATK\b|attack spd|casting spd|main stats?|crit|lucky|DMG Boost|elemental damage|all-element bonus|PHY Boost|MAG Boost|penetration|攻击|暴击|幸运|主属性|伤害加成).{0,100}(?:increase|increased|gain|grants?|boost|bonus|提高|增加|加成|附加))/iu.test(segment)
  ))) {
    result.add("party-offensive-stat");
  }
  if (partyScope && partySegments.some((segment) =>
    /(?:(?:gain|grants?|increase|increased).{0,60}(?:haste|action speed)|(?:haste|action speed)\s*\+|急速.{0,20}(?:提高|增加|加成)|冷却.{0,20}(?:降低|减少))/iu.test(segment)
  )) {
    result.add("party-action-opportunity");
  }
  if (partyScope && partySegments.some((segment) =>
    /(?:heal|HOT|lifesteal|治疗|回复|回血|治愈)/iu.test(segment)
  )) {
    result.add("party-healing");
  }
  if (partyScope && partySegments.some((segment) =>
    /(?:shield|damage reduction|DMG Reduction|defen[cs]e|resistance|immun(?:e|ity)|护盾|减伤|防御|抗性|免疫)/iu.test(segment)
  )) {
    result.add("party-defensive-support");
  }
  if (partyScope && partySegments.some((segment) =>
    /(?:energy|resource|courage|能量|资源|勇气)/iu.test(segment)
  )) {
    result.add("party-resource-support");
  }
  if (result.size === 0) result.add("party-scope-unclassified-component");
  return [...result].sort();
}

function proofObligations(hasReviewedBinding) {
  return [
    ...(hasReviewedBinding ? [] : ["exact-skill-to-buff-or-status-edge"]),
    "provider-ownership",
    "recipient-selector-cap-radius-priority-and-party-membership",
    "per-level-magnitude",
    "duration-lifecycle",
    "stacking-overwrite-and-operation-order",
    "integer-rounding",
    "matching-build-canonical-conservation-replay",
  ];
}

function descriptionClaims(value) {
  const text = stripMarkup(String(value ?? ""));
  return [...text.matchAll(/\b\d+(?:\.\d+)?\s*(?:%|s\b|seconds?\b|allies?\b|targets?\b)/giu)]
    .map((match) => match[0]);
}

function evidenceText(row) {
  return [row?.NameDesign, row?.Name, row?.Desc, row?.Note]
    .filter((value) => typeof value === "string")
    .map(stripMarkup)
    .join("\n");
}

function presentationState(row) {
  const name = `${row?.NameDesign ?? ""} ${row?.Name ?? ""}`;
  const description = stripMarkup(String(row?.Desc ?? ""));
  if (/(?:placeholder|test|测试|占位|场地标记)/iu.test(`${name} ${description}`)) {
    return "placeholder-or-test-evidence-retained";
  }
  if (description.length === 0) return "description-absent-evidence-retained";
  return "described-row-evidence-retained";
}

function stripMarkup(value) {
  return value.replace(/<br\s*\/?\s*>/giu, ". ").replace(/<[^>]*>/g, " ")
    .replace(/\s+/g, " ").trim();
}

function normalizeSkillAttributes(value) {
  if (!Array.isArray(value)) return [];
  return value.map((entry) => ({
    label_evidence: nullableString(entry?.[0]),
    expression_evidence: nullableString(entry?.[1]),
  }));
}

function normalizeRogueDescriptions(row, descriptionTable) {
  if (!Array.isArray(row?.EntryDescription)) return [];
  return row.EntryDescription.map((entry) => {
    const level = Number(entry?.[0]);
    const descriptionId = Number(entry?.[1]);
    const description = Number.isSafeInteger(descriptionId) && descriptionId > 0
      ? descriptionTable[String(descriptionId)] ?? null
      : null;
    return {
      level: Number.isFinite(level) ? level : null,
      description_id: Number.isSafeInteger(descriptionId) && descriptionId > 0
        ? descriptionId
        : null,
      content_evidence: nullableString(description?.Content),
      row_present: description !== null,
      row_sha256: description ? valueHash(description) : null,
    };
  });
}

function numericEntries(value) {
  return Object.entries(value ?? {}).filter(([key]) => /^\d+$/.test(key));
}

function numericIds(value) {
  return uniqueSortedNumbers((Array.isArray(value) ? value : []).map(Number).filter(
    (entry) => Number.isSafeInteger(entry) && entry > 0,
  ));
}

function uniqueSortedNumbers(values) {
  return [...new Set(values)].sort((left, right) => left - right);
}

function countCategories(rows) {
  const counts = {};
  for (const categories of rows) {
    for (const category of categories) counts[category] = (counts[category] ?? 0) + 1;
  }
  return Object.fromEntries(Object.entries(counts).sort(([left], [right]) => left.localeCompare(right)));
}

function compareComponents(left, right) {
  return left.component_id.localeCompare(right.component_id) || left.source_id.localeCompare(right.source_id);
}

function exactPositiveId(value, label) {
  const number = Number(value);
  if (!Number.isSafeInteger(number) || number <= 0) throw new Error(`${label} is invalid`);
  return number;
}

function nullableString(value) {
  return typeof value === "string" && value.length > 0 ? value : null;
}

function descriptor(file) {
  if (!existsSync(file)) throw new Error(`Required input does not exist: ${file}`);
  const bytes = statSync(file).size;
  if (!Number.isSafeInteger(bytes) || bytes <= 0 || bytes > MAX_INPUT_BYTES) {
    throw new Error(`Input exceeds bounded size or is empty: ${file}`);
  }
  return { path: path.relative(process.cwd(), file), bytes, sha256: fileHash(file) };
}

function readBoundedJson(file, label) {
  descriptor(file);
  try {
    return JSON.parse(readFileSync(file, "utf8"));
  } catch (error) {
    throw new Error(`${label} is not valid JSON: ${error instanceof Error ? error.message : error}`);
  }
}

function fileHash(file) {
  return createHash("sha256").update(readFileSync(file)).digest("hex");
}

function valueHash(value) {
  return createHash("sha256").update(stableStringify(value)).digest("hex");
}

function contentHash(report) {
  const copy = structuredClone(report);
  delete copy.content_sha256;
  return valueHash(copy);
}

function selectAnalysis(report) {
  return {
    summary: report.summary,
    discovery_contract: report.discovery_contract,
    skill_candidates: report.skill_candidates,
    buff_candidates: report.buff_candidates,
    rogue_party_entry_candidates: report.rogue_party_entry_candidates,
    remaining_global_obligations: report.remaining_global_obligations,
    runtime_decision: report.runtime_decision,
  };
}

function stableStringify(value) {
  if (Array.isArray(value)) return `[${value.map(stableStringify).join(",")}]`;
  if (value && typeof value === "object") {
    return `{${Object.keys(value).sort().map((key) =>
      `${JSON.stringify(key)}:${stableStringify(value[key])}`).join(",")}}`;
  }
  return JSON.stringify(value);
}

function parseArgs(values) {
  const result = {};
  for (let index = 0; index < values.length; index += 2) {
    const key = values[index];
    const value = values[index + 1];
    if (!key?.startsWith("--") || value === undefined) throw new Error("Arguments must be --key value pairs");
    result[key.slice(2)] = value;
  }
  return result;
}

function required(args, key) {
  const value = args[key];
  if (!value) throw new Error(`Missing --${key}`);
  return value;
}

function selfTest() {
  const skills = {
    10: { Id: 10, Name: "Test", Desc: "Grants all allies 10% Haste for 5s.", EffectIDs: [100] },
    11: { Id: 11, Name: "Flavor", Desc: "Best paired with allies' attacks.", EffectIDs: [101] },
  };
  const effects = {
    100: { Id: 100, SkillId: 10, Level: 1, SkillAttrDes: [["Haste", "10%"]] },
    101: { Id: 101, SkillId: 11, Level: 1, SkillAttrDes: [] },
  };
  const buffs = {
    200: { Id: 200, Level: 1, NameDesign: "[团队]：测试", RepeatAddRule: [0, 1], DestroyParam: [[0, 5]] },
  };
  const ledger = { candidates: [] };
  const rogueEntries = {
    20: {
      EntryId: 20,
      EntryName: "Team Test",
      BuffId: 200,
      EntryType: 1,
      EntryDescription: [[1, 300]],
      ApplicableProfessions: [0],
    },
  };
  const rogueDescriptions = {
    300: { Id: 300, Content: "Nearby allies gain 5% ATK for 3s." },
  };
  const aoyiOriginLedger = { skills: [], summary: { enabled_for_rdps: 0 } };
  const talents = {
    30: {
      Id: 30,
      TalentName: "Party Test Talent",
      TalentDes: "All allies gain 10% Haste for 5s.",
      TalentEffect: [[6, 99, 10]],
      BuffPar: [[0]],
    },
  };
  const reviewedLinkCandidates = {
    reviewed_candidates: [{
      skill_id: 10,
      candidate_buff_ids: [{ buff_id: 200, candidate_role: "test-candidate" }],
    }],
  };
  const result = analyze(
    skills,
    effects,
    buffs,
    rogueEntries,
    rogueDescriptions,
    talents,
    aoyiOriginLedger,
    ledger,
    reviewedLinkCandidates,
  );
  if (result.summary.skill_candidates !== 1 || result.summary.buff_candidates !== 1 ||
    result.summary.rogue_party_entry_candidates !== 1 ||
    result.rogue_party_entry_candidates[0].exact_root_buff_id !== 200 ||
    result.skill_candidates[0].skill_id !== 10 ||
    result.skill_candidates[0].exact_build_party_talent_route_evidence.length !== 1 ||
    result.skill_candidates[0].reviewed_candidate_skill_to_buff_links.length !== 1 ||
    result.skill_candidates[0].reviewed_candidate_skill_to_buff_links[0]
      .exact_skill_to_buff_edge_proven !== false ||
    !result.skill_candidates[0].support_categories.includes("party-action-opportunity")) {
    throw new Error("Party-skill static closure self-test failed");
  }
  console.log("Party-skill static closure self-test passed.");
}

function usage(code) {
  console.log(
    "Usage: node tools/bpsr-party-skill-static-closure.mjs build --build <id> " +
      "--manifest <json> --skill-table <json> --skill-effect-table <json> " +
      "--buff-table <json> --rogue-entry-table <json> " +
      "--rogue-entry-description-table <json> --talent-table <json> " +
      "--current-aoyi-origin-ledger <json> " +
      "--recipient-scope-ledger <json> " +
      "--reviewed-link-candidates <json> " +
      "--output <json> | " +
      "verify --input <json> | self-test",
  );
  process.exit(code);
}
