#!/usr/bin/env node

import { createHash } from "node:crypto";
import { existsSync, mkdirSync, readFileSync, statSync, writeFileSync } from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const scriptDir = path.dirname(fileURLToPath(import.meta.url));
const repoRoot = path.resolve(scriptDir, "..");
const [command = "help", ...rest] = process.argv.slice(2);
const options = parseArgs(rest);

if (command === "generate") generate(options);
else if (command === "self-test") selfTest();
else usage(command === "help" ? 0 : 1);

function generate(options) {
  const buildRoot = resolvePath(required(options, "build-root"));
  const referenceGraphPath = resolvePath(required(options, "reference-graph"));
  const semanticFieldSchemaPath = resolvePath(
    options["semantic-field-schema"]
      ?? path.join(path.dirname(referenceGraphPath), "DecodedTableReferenceGraph.semantic-field-schema.v1.json"),
  );
  const decodedFieldSchemaPath = resolvePath(
    options["decoded-field-schema"]
      ?? path.join(path.dirname(referenceGraphPath), "DecodedTableReferenceGraph.decoded-field-schema.v1.json"),
  );
  const outputPath = resolvePath(options.output ?? path.join(buildRoot, "current-build-unmapped-catalog.v1.json"));
  const shardRoot = resolvePath(options["shard-root"] ?? path.join(buildRoot, "current-build-unmapped-catalog"));
  const inputs = loadInputs(buildRoot, referenceGraphPath, semanticFieldSchemaPath, decodedFieldSchemaPath);
  const buildId = String(inputs.coverage.value.client_build);
  validateBuildIdentity(inputs, buildId);

  const shards = buildShards(inputs, buildId);
  mkdirSync(shardRoot, { recursive: true });
  const shardIndex = [];
  for (const shard of shards) {
    const filePath = path.join(shardRoot, shard.file);
    const payload = {
      schema_version: 1,
      generated_by: "tools/bpsr-current-build-unmapped-catalog.mjs",
      game_build: buildId,
      category: shard.category,
      blocking_class: shard.blockingClass,
      semantic_domain: shard.semanticDomain,
      policy: shard.policy,
      count: shard.entries.length,
      entries: stableSort(shard.entries),
    };
    writeJson(filePath, payload);
    shardIndex.push({
      id: shard.id,
      category: shard.category,
      blocking_class: shard.blockingClass,
      semantic_domain: shard.semanticDomain,
      count: shard.entries.length,
      file: relativeRepo(filePath),
      sha256: sha256File(filePath),
    });
  }

  const report = buildIndex(inputs, buildId, outputPath, shardRoot, shardIndex);
  writeJson(outputPath, report);
  console.log(`Current-build unmapped catalog created for ${buildId}.`);
  console.log(`Open catalog entries: ${report.summary.open_catalog_entries}.`);
  console.log(`Mechanics findings: ${report.summary.by_blocking_class["mechanics-blocker"] ?? 0}.`);
  console.log(`Runtime observation rows: ${report.summary.by_blocking_class["runtime-observation"] ?? 0}.`);
  console.log(`Runtime proof gates: ${report.summary.by_blocking_class["runtime-proof"] ?? 0}.`);
  console.log(`Protocol blockers: ${report.summary.by_blocking_class.protocol ?? 0}.`);
  console.log(`Presentation rows: ${report.summary.by_blocking_class.presentation ?? 0}.`);
  console.log(`Dormant definitions: ${report.summary.by_blocking_class["dormant-definition"] ?? 0}.`);
  console.log(`Review-only rows: ${report.summary.by_blocking_class["review-only"] ?? 0}.`);
  console.log(`Wrote ${relativeRepo(outputPath)}`);
}

function loadInputs(buildRoot, referenceGraphPath, semanticFieldSchemaPath, decodedFieldSchemaPath) {
  const paths = {
    coverage: "combat-domain-coverage-audit.v1.json",
    staticAudit: "static-rdps-semantic-audit.json",
    semanticRefresh: "current-build-semantic-refresh.v1.json",
    formulaLedger: "formula-magnitude-gap-ledger.v11.json",
    scopeLedger: "rdps-recipient-scope-ledger.v2.json",
    preflight: "rdps-build-preflight.v3.json",
    protocol: "protocol-decode-recordings-v2/protocol-pack-promotion-audit.v2.json",
    equipmentReachability: "equipment-set-child-buff-reachability.v1.json",
    effectActivation: "effect-activation-ledger.v1.json",
    damageActivation: "unrouted-damage-activation-ledger.v1.json",
    scriptFamilies: "damage-script-family-worklist.v6.json",
    skills: "combat-domain-coverage/all-skills-and-actions.json",
    damageActions: "combat-domain-coverage/all-damage-actions.json",
    buffs: "combat-domain-coverage/all-buffs.json",
    effectSources: "combat-domain-coverage/all-effect-sources.json",
    equipment: "combat-domain-coverage/all-equipment-set-effects.json",
  };
  const inputs = Object.fromEntries(Object.entries(paths).map(([key, relativePath]) => {
    const filePath = path.join(buildRoot, relativePath);
    if (!existsSync(filePath)) throw new Error(`Missing catalog input ${relativePath}`);
    return [key, { path: filePath, value: readJson(filePath) }];
  }));
  if (!existsSync(referenceGraphPath)) throw new Error(`Missing decoded reference graph ${referenceGraphPath}`);
  inputs.referenceGraph = { path: referenceGraphPath, value: readJson(referenceGraphPath) };
  if (!existsSync(semanticFieldSchemaPath)) throw new Error(`Missing semantic field-schema ledger ${semanticFieldSchemaPath}`);
  inputs.semanticFieldSchema = { path: semanticFieldSchemaPath, value: readJson(semanticFieldSchemaPath) };
  if (!existsSync(decodedFieldSchemaPath)) throw new Error(`Missing decoded field-schema manifest ${decodedFieldSchemaPath}`);
  inputs.decodedFieldSchema = { path: decodedFieldSchemaPath, value: readJson(decodedFieldSchemaPath) };
  return inputs;
}

function buildShards(inputs, buildId) {
  const value = values(inputs);
  const equipmentRows = value.equipment.set_named_buffs ?? [];
  const coverage = value.coverage;
  const semanticFindings = value.staticAudit.findings.map((finding) => ({
    key: finding.source_rule_id,
    source_rule_id: finding.source_rule_id,
    source_id: finding.source_id,
    source_name: finding.source_name,
    categories: uniqueSorted(finding.issues.map((issue) => issue.category)),
    issues: finding.issues,
    status: "current-build-static-semantics-incomplete",
    required_resolution: "Resolve every listed issue with exact current-build client or packet evidence before runtime rDPS promotion.",
  }));

  const formulaObservationBacklog = value.formulaLedger.candidates
    .filter((candidate) => !hasPacketObservation(candidate.historical_packet_observations))
    .map((candidate) => ({
      key: candidate.source_rule_id,
      source_rule_id: candidate.source_rule_id,
      source_id: candidate.source_id,
      source_name: candidate.source_name,
      effect_ids: candidate.effect_ids,
      formula_term_ids: candidate.formula_term_ids,
      selected_attribute_ids: candidate.selected_attribute_ids,
      outcome: candidate.outcome,
      static_blockers: candidate.static_blockers,
      required_runtime_evidence: candidate.required_runtime_evidence,
      remaining_requirement: candidate.remaining_requirement,
      status: "not-observed-in-indexed-packet-corpus",
    }));

  const unresolvedScopes = value.scopeLedger.candidates
    .filter((candidate) => (candidate.effective_transfer_eligibilities ?? []).includes("recipient-scope-unresolved"))
    .map((candidate) => ({
      key: `recipient-scope:${candidate.source_rule_id}`,
      gate: "recipient-scope-unresolved",
      source_rule_id: candidate.source_rule_id,
      source_id: candidate.source_id,
      source_name: candidate.source_name,
      effective_transfer_eligibilities: candidate.effective_transfer_eligibilities,
      scope_resolution: candidate.scope_resolution,
      component_scope_routes: candidate.component_scope_routes,
      remaining_requirement: candidate.remaining_requirement,
      status: "current-build-runtime-proof-required",
    }));
  const preflightProofGates = value.preflight.inputs
    .filter((entry) => entry.status !== "present" && entry.id !== "protocol-pack-identity")
    .map((entry) => ({
      key: `preflight:${entry.id}`,
      gate: entry.id,
      required: entry.required,
      role: entry.role,
      status: entry.status,
      required_proof_suites: value.preflight.required_proof_suites_from_missing_inputs?.[entry.id] ?? [],
    }));

  const protocolBlockers = value.protocol.blockers.map((blocker, index) => ({
    key: `protocol:${index + 1}`,
    blocker,
    protocol_pack_id: value.protocol.protocol_pack_id,
    exact_world_service_id: value.protocol.exact_world_service_id,
    status: "protocol-pack-not-promotable",
  }));

  const missingSkillEnglish = value.skills
    .filter((entry) => !textPresent(entry.english_name))
    .map((entry) => ({
      key: `action:${entry.action_id}`,
      action_id: entry.action_id,
      kind: entry.kind,
      design_name: entry.design_name,
      icon: entry.icon,
      damage_row_ids: entry.damage_row_ids,
      status: textPresent(entry.design_name) ? "design-name-only" : "uid-only",
      sources: entry.sources,
    }));
  const missingBuffEnglish = value.buffs
    .filter((entry) => !textPresent(entry.english_name))
    .map((entry) => ({
      key: `buff:${entry.buff_id}:${entry.level}`,
      buff_id: entry.buff_id,
      level: entry.level,
      design_name: entry.design_name,
      icon: entry.icon,
      is_equipment_set_effect: entry.is_equipment_set_effect,
      status: textPresent(entry.design_name) ? "design-name-only" : "uid-only",
    }));
  const missingEffectSourceEnglish = value.effectSources
    .filter((entry) => !textPresent(entry.english_name))
    .map((entry) => ({
      key: entry.source_id,
      source_id: entry.source_id,
      source_entity_id: entry.source_entity_id,
      source_kind: entry.source_kind,
      source_type: entry.source_type,
      design_name: entry.design_name,
      localization_status: entry.localization_status,
      attribution_status: entry.attribution_status,
      buff_ids: entry.buff_ids,
      damage_ids: entry.damage_ids,
      recount_ids: entry.recount_ids,
    }));
  const missingEquipmentEnglish = equipmentRows
    .filter((entry) => !textPresent(entry.english_name))
    .map((entry) => ({
      key: `equipment-buff:${entry.buff_id}:${entry.level}`,
      buff_id: entry.buff_id,
      level: entry.level,
      design_name: entry.design_name,
      icon: entry.icon,
      effect_source_ids: entry.effect_source_ids,
      source_resolution: entry.source_resolution,
      status: textPresent(entry.design_name) ? "design-name-only" : "uid-only",
    }));

  const damageIdentityGaps = value.damageActions
    .filter((entry) => entry.action_identity_present !== true)
    .map((entry) => ({
      key: `damage:${entry.damage_id}`,
      damage_id: entry.damage_id,
      action_id: entry.action_id,
      action_parent_relation: entry.action_parent_relation,
      action_identity_status: entry.action_identity_status,
      design_name: entry.design_name,
      linked_source: entry.linked_source,
      category: entry.category,
      status: "exact-action-id-known-identity-presentation-missing",
    }));
  const unknownDamageCategories = value.damageActions
    .filter((entry) => entry.category === "unknown")
    .map((entry) => ({
      key: `damage-category:${entry.damage_id}`,
      damage_id: entry.damage_id,
      action_id: entry.action_id,
      action_identity_status: entry.action_identity_status,
      english_name: entry.english_name,
      design_name: entry.design_name,
      linked_source: entry.linked_source,
      action_parent_relation: entry.action_parent_relation,
      status: "exact-parent-retained-category-review-required",
    }));

  const recountReviews = [
    ...(coverage.worklists.client_recount_partial_reviews ?? []).map((entry) => ({
      key: `partial:${idOf(entry)}`,
      review_kind: "partial-client-recount",
      ...asRecord(entry, "action_id"),
      status: "presentation-group-review-exact-damage-parent-known",
    })),
    ...(coverage.worklists.client_recount_ambiguous_reviews ?? []).map((entry) => ({
      key: `ambiguous:${idOf(entry)}`,
      review_kind: "ambiguous-client-recount",
      ...asRecord(entry, "action_id"),
      status: "presentation-group-review-exact-damage-parent-known",
    })),
  ];

  const dormantEquipment = value.effectActivation.effects.map((entry) => ({
    key: `equipment-effect:${entry.effect_id}`,
    effect_id: entry.effect_id,
    activation_status: entry.activation_status,
    reachability_status: entry.reachability_status,
    current_build_relationship_proven: entry.current_build_relationship_proven,
    blocks_exact_current_build_relationship: entry.blocks_exact_current_build_relationship,
    status: entry.activation_status,
  }));
  const dormantDamage = value.damageActivation.entries.map((entry) => ({
    key: `damage-route:${entry.lookup_key}`,
    lookup_key: entry.lookup_key,
    damage_id: entry.damage_id,
    action_id: entry.action_id,
    activation_status: entry.activation_status,
    blocks_exact_current_build_relationship: entry.blocks_exact_current_build_relationship,
    status: entry.activation_status,
  }));
  const scriptRouteReviews = collectScriptRouteGaps(value.scriptFamilies).map((entry) => ({
    key: `script-route:${entry.lookup_key}`,
    family_id: entry.family_id,
    formula_signature: entry.formula_signature,
    lookup_key: entry.lookup_key,
    ability_id: entry.ability_id,
    hit_event_id: entry.hit_event_id,
    damage_id: entry.damage_attr?.id ?? entry.damage_attr?.damage_id ?? null,
    damage_name: entry.damage_attr?.name ?? entry.damage_attr?.design_name ?? null,
    gap_reason: entry.gap_reason,
    route_resolution_state: entry.route_resolution_state,
    status: "static-script-route-review-preserved",
  }));

  const tableDomains = new Map((value.referenceGraph.tables ?? []).map((table) => [table.table, table.domain ?? "other"]));
  const exactReferenceGapsByDomain = groupBy(
    (value.referenceGraph.missing_targets ?? []).map((entry) => ({
      key: `missing:${entry.source_table}:${entry.source_id}:${entry.source_pointer}:${entry.target_table ?? (entry.target_tables ?? []).join("+")}:${entry.target_id}`,
      ...entry,
      source_domain: tableDomains.get(entry.source_table) ?? "other",
      target_domains: [...new Set(
        (entry.target_tables ?? [entry.target_table])
          .filter(Boolean)
          .map((targetTable) => tableDomains.get(targetTable) ?? "other"),
      )].sort(),
      impact: entry.blocks_mechanics === false ? "presentation" : "mechanics",
      blocking_class: entry.blocks_mechanics === false
        ? "presentation-localization-gap"
        : "static-reference-gap",
      status: entry.missing_target_classification
        ?? "declared-current-build-reference-target-missing",
      required_resolution: entry.blocks_mechanics === false
        ? "Preserve the canonical source ID and provide localized presentation when the client exposes it; mechanics remain intact."
        : "Locate a current-build definition route or prove the declaration intentionally has no mechanics target before closing this item.",
    })),
    (entry) => entry.source_domain,
  );
  const openSemanticFields = value.semanticFieldSchema.fields.filter((entry) => entry.resolution_state === "open");
  const dormantSemanticFields = openSemanticFields.filter((entry) => entry.evidence_state === "dormant-zero-only-identifier");
  const activeSemanticReviews = openSemanticFields.filter((entry) => entry.evidence_state !== "dormant-zero-only-identifier");
  const referenceReviewsByDomain = groupBy(
    activeSemanticReviews.map((entry) => ({
      key: `unproven:${entry.key}`,
      field_key: entry.key,
      source_table: entry.source_table,
      field: entry.field,
      path_pattern: entry.path_pattern,
      source_domain: tableDomains.get(entry.source_table) ?? "other",
      evidence_state: entry.evidence_state,
      resolution_state: entry.resolution_state,
      accepted_target_tables: entry.accepted_target_tables,
      open_reason: entry.open_reason,
      decoded_value_profile: entry.decoded_value_profile,
      candidate_targets: entry.candidate_targets,
      il2cpp_schema: entry.il2cpp_schema,
      status: "current-build-semantic-field-schema-open",
      required_resolution: "Classify this field from current-build schema, client code, or packet evidence; every occurrence remains preserved in the linked JSONL artifact.",
    })),
    (entry) => entry.source_domain,
  );
  const dormantSemanticFieldsByDomain = groupBy(
    dormantSemanticFields.map((entry) => ({
      key: `dormant-field:${entry.key}`,
      field_key: entry.key,
      source_table: entry.source_table,
      field: entry.field,
      path_pattern: entry.path_pattern,
      source_domain: tableDomains.get(entry.source_table) ?? "other",
      evidence_state: entry.evidence_state,
      open_reason: entry.open_reason,
      decoded_value_profile: entry.decoded_value_profile,
      il2cpp_schema: entry.il2cpp_schema,
      status: "current-build-zero-only-field-schema-unproven",
      required_resolution: "Retain and diff this field until a future build or packet observation provides nonzero values that can prove its semantic domain.",
    })),
    (entry) => entry.source_domain,
  );
  const decodedFieldSemanticReviews = value.decodedFieldSchema.fields
    .filter((entry) => entry.mechanics_review_routing?.requires_semantic_review === true)
    .map((entry) => ({
      key: `mechanics-field:${entry.key}`,
      field_key: entry.key,
      source_table: entry.source_table,
      top_level_field: entry.top_level_field,
      path_pattern: entry.path_pattern,
      source_domain: tableDomains.get(entry.source_table) ?? "other",
      mechanics_categories: entry.mechanics_review_routing.categories,
      routing_evidence_state: entry.mechanics_review_routing.evidence_state,
      value_profile: entry.value_profile,
      il2cpp_top_level_schema: entry.il2cpp_top_level_schema,
      semantic_relationship: entry.semantic_relationship,
      status: "current-build-mechanics-field-semantics-unproven",
      required_resolution: "Prove unit, scaling, ownership, scope, stacking, lifecycle, and formula role from current-build code, tables, or packets before runtime use. Field-name routing is review prioritization only.",
    }));
  const decodedFieldReviewsByDomain = groupBy(
    decodedFieldSemanticReviews,
    (entry) => entry.source_domain,
  );

  const definitions = [
    shard("semantic-mechanics", "semantic-mechanics-findings.json", "semantic-mechanics-finding", "mechanics-blocker", "formulas/scaling", semanticFindings,
      "These are exact current-build semantic findings. None may be guessed, hidden, or enabled for runtime rDPS until every listed issue is proven."),
    shard("runtime-observation", "runtime-observation-backlog.json", "formula-runtime-observation-backlog", "runtime-observation", "formulas/scaling", formulaObservationBacklog,
      "Static ownership is known. These rows require packet observation; absence from the indexed corpus is not an unresolved client definition."),
    shard("runtime-proof", "runtime-proof-gates.json", "runtime-proof-gate", "runtime-proof", "formulas/scaling", [...unresolvedScopes, ...preflightProofGates],
      "These are concrete runtime proof gates after static mapping, not localization or definition gaps."),
    shard("protocol", "protocol-blockers.json", "protocol-blocker", "protocol", "protocol", protocolBlockers,
      "Protocol blockers remain fail-closed for protocol-pack promotion while older compatible decoding may continue with an out-of-date warning."),
    shard("skill-presentation", "skill-presentation-gaps.json", "skill-presentation-gap", "presentation", "skills", missingSkillEnglish,
      "Canonical action IDs and all damage are retained. Missing English is a localization plug-in concern and never removes mechanics evidence."),
    shard("buff-presentation", "buff-presentation-gaps.json", "buff-presentation-gap", "presentation", "buffs/effects", missingBuffEnglish,
      "Every current-build buff row remains addressable by ID even when it is hidden or lacks user-facing English."),
    shard("effect-source-presentation", "effect-source-presentation-gaps.json", "effect-source-presentation-gap", "presentation", "buffs/effects", missingEffectSourceEnglish,
      "Effect-source mechanics and attribution remain active by canonical ID; presentation falls back to IDs or design names."),
    shard("equipment-presentation", "equipment-presentation-gaps.json", "equipment-presentation-gap", "presentation", "equipment/set-bonuses", missingEquipmentEnglish,
      "Equipment set mechanics remain keyed by exact buff ID and source relationship regardless of localization coverage."),
    shard("damage-identity", "damage-action-identity-gaps.json", "damage-action-identity-gap", "presentation", "skills", damageIdentityGaps,
      "DamageAttr.LinkedId remains the exact action parent. These rows lack a user-facing action definition, not a damage ownership relationship."),
    shard("damage-category", "damage-category-reviews.json", "damage-category-review", "review-only", "relationships/recount", unknownDamageCategories,
      "Unknown display categories stay visible with exact damage and action IDs; they are never dropped from totals."),
    shard("client-recount", "client-recount-reviews.json", "client-recount-review", "review-only", "relationships/recount", recountReviews,
      "Client RecountTable presentation grouping is reviewed independently from the exact DamageAttr.LinkedId action parent."),
    shard("dormant-equipment", "dormant-equipment-definitions.json", "dormant-equipment-definition", "dormant-definition", "equipment/set-bonuses", dormantEquipment,
      "Definition-only equipment effects remain indexed and diffable but do not block active relationships without an incoming client reference or packet observation."),
    shard("dormant-damage", "dormant-damage-definitions.json", "dormant-damage-definition", "dormant-definition", "relationships/recount", dormantDamage,
      "Unobserved damage definitions remain indexed and diffable; they do not feed runtime rDPS until activated by exact current-build evidence."),
    shard("script-route", "script-route-reviews.json", "script-route-review", "review-only", "relationships/recount", scriptRouteReviews,
      "Script-family gaps remain explicit. Exact damage rows are retained even when the static script lookup route is absent."),
    ...[...exactReferenceGapsByDomain.entries()].map(([domain, entries]) => shard(
      `static-reference-gap-${slug(domain)}`,
      `static-reference-gaps-${slug(domain)}.json`,
      "static-reference-gap",
      entries.every((entry) => entry.blocks_mechanics === false)
        ? "presentation-localization-gap"
        : "static-reference-gap",
      domain,
      entries,
      "Every absent declared target remains visible. Presentation-only gaps preserve mechanics by canonical ID; semantic-definition gaps remain blocking until resolved with exact evidence.",
    )),
    ...[...referenceReviewsByDomain.entries()].map(([domain, entries]) => shard(
      `reference-review-${slug(domain)}`,
      `reference-reviews-${slug(domain)}.json`,
      "reference-field-review",
      "reference-review",
      domain,
      entries,
      "These group every ID-like field whose target semantics are not yet proven. The exhaustive occurrence JSONL is retained and hash-verified.",
    )),
    ...[...decodedFieldReviewsByDomain.entries()].map(([domain, entries]) => shard(
      `decoded-field-semantic-review-${slug(domain)}`,
      `decoded-field-semantic-reviews-${slug(domain)}.json`,
      "decoded-field-semantic-review",
      "field-semantic-review",
      domain,
      entries,
      "These are current-build mechanics-sensitive scalar, array, object, and nested field paths. Name routing is not semantic or formula proof; every path remains blocking until its exact mechanics are proven.",
    )),
    ...[...dormantSemanticFieldsByDomain.entries()].map(([domain, entries]) => shard(
      `dormant-semantic-fields-${slug(domain)}`,
      `dormant-semantic-fields-${slug(domain)}.json`,
      "dormant-semantic-fields",
      "dormant-definition",
      domain,
      entries,
      "Zero-only current-build identifier fields remain open, indexed, and patch-diffable. They are never guessed or silently marked resolved.",
    )),
  ];

  assert(semanticFindings.length === value.staticAudit.summary.candidates_with_findings, "Semantic finding count mismatch");
  assert(formulaObservationBacklog.length === value.formulaLedger.summary.candidates_without_packet_observations, "Formula observation backlog count mismatch");
  assert(unresolvedScopes.length === (value.scopeLedger.summary.effective_transfer_eligibilities["recipient-scope-unresolved"] ?? 0), "Recipient-scope gate count mismatch");
  assert(protocolBlockers.length === value.protocol.blockers.length, "Protocol blocker count mismatch");
  assert(missingSkillEnglish.length === coverage.skills_and_actions.design_only_or_missing_english, "Skill presentation count mismatch");
  assert(missingBuffEnglish.length === coverage.buffs_and_effects.design_only_or_missing_english_buffs, "Buff presentation count mismatch");
  assert(missingEffectSourceEnglish.length === coverage.buffs_and_effects.effect_sources_missing_user_facing_english, "Effect-source presentation count mismatch");
  assert(missingEquipmentEnglish.length === coverage.equipment_set_effects.missing_user_facing_english, "Equipment presentation count mismatch");
  assert(damageIdentityGaps.length === coverage.damage_actions.action_ids_missing_skill_identity, "Damage identity count mismatch");
  assert(unknownDamageCategories.length === coverage.damage_actions.unknown_category, "Damage category count mismatch");
  assert(recountReviews.length === 18, "Client recount review count mismatch");
  assert(dormantEquipment.length === value.effectActivation.summary.effects, "Dormant equipment count mismatch");
  assert(dormantDamage.length === value.damageActivation.summary.unresolved_static_route_definitions, "Dormant damage count mismatch");
  assert(scriptRouteReviews.length === value.scriptFamilies.summary.candidates_without_static_route, "Script route review count mismatch");
  assert(sumGrouped(exactReferenceGapsByDomain) === value.referenceGraph.summary.exact_edges_with_missing_target, "Exact reference-gap count mismatch");
  assert(openSemanticFields.length === value.semanticFieldSchema.summary.open_field_groups, "Semantic field-schema open count mismatch");
  assert(sumGrouped(referenceReviewsByDomain) === activeSemanticReviews.length, "Reference-review field-group count mismatch");
  assert(sumGrouped(dormantSemanticFieldsByDomain) === dormantSemanticFields.length, "Dormant semantic field-group count mismatch");
  assert(decodedFieldSemanticReviews.length === value.decodedFieldSchema.summary.mechanics_sensitive_field_paths, "Decoded mechanics-sensitive field count mismatch");
  assert(sumGrouped(decodedFieldReviewsByDomain) === decodedFieldSemanticReviews.length, "Decoded field-semantic review count mismatch");
  assert(value.decodedFieldSchema.fields.length === value.decodedFieldSchema.summary.decoded_field_paths, "Decoded field-schema row count mismatch");
  assert(activeSemanticReviews.length + dormantSemanticFields.length === openSemanticFields.length, "Semantic field-schema classification is not conserved");
  assert(buildId.length > 0, "Build ID is empty");
  return definitions;
}

function buildIndex(inputs, buildId, outputPath, shardRoot, shards) {
  const byBlockingClass = countBy(shards, (entry) => entry.blocking_class, (entry) => entry.count);
  const byDomain = countBy(shards, (entry) => entry.semantic_domain, (entry) => entry.count);
  const byCategory = countBy(shards, (entry) => entry.category, (entry) => entry.count);
  const openCatalogEntries = shards.reduce((total, entry) => total + entry.count, 0);
  return {
    schema_version: 3,
    generated_by: "tools/bpsr-current-build-unmapped-catalog.mjs",
    game: "blue-protocol-star-resonance",
    deployment: "global",
    channel: "steam",
    game_build: buildId,
    policy: {
      complete_current_build_inventory_precedes_patch_diffing: true,
      steam_depot_manifest_narrows_changed_physical_files_only: true,
      semantic_domain_hashes_select_regeneration_and_reproof_work: true,
      complete_decoded_scalar_array_object_and_nested_field_profiles_are_diffed: true,
      field_name_routing_never_counts_as_semantic_or_formula_proof: true,
      no_unresolved_evidence_is_hidden_or_dropped: true,
      localization_and_presentation_are_separate_from_mechanics: true,
      dormant_definitions_remain_diffable_without_blocking_active_relationships: true,
      exact_static_identity_does_not_substitute_for_runtime_formula_proof: true,
      runtime_rdps_promotion_remains_fail_closed: true,
    },
    summary: {
      open_catalog_entries: openCatalogEntries,
      shard_count: shards.length,
      by_blocking_class: byBlockingClass,
      by_semantic_domain: byDomain,
      by_category: byCategory,
      current_build_definition_inventory_complete: true,
      current_build_reference_inventory_complete: true,
      current_build_semantic_reference_mapping_complete:
        (byBlockingClass["static-reference-gap"] ?? 0) === 0
        && (byBlockingClass["reference-review"] ?? 0) === 0,
      current_build_mechanics_field_semantics_complete:
        (byBlockingClass["field-semantic-review"] ?? 0) === 0,
      current_build_runtime_rdps_complete: false,
      decoded_reference_candidate_field_groups: inputs.referenceGraph.value.summary.reference_candidate_field_groups,
      decoded_reference_candidate_full_coverage_field_groups: inputs.referenceGraph.value.summary.reference_candidate_full_coverage_field_groups,
      decoded_reference_candidate_zero_only_field_groups: inputs.referenceGraph.value.summary.reference_candidate_zero_only_field_groups,
      decoded_exact_field_schemas: inputs.referenceGraph.value.summary.exact_field_schemas,
      decoded_current_build_callsite_proven_field_schemas:
        inputs.referenceGraph.value.summary.current_build_callsite_proven_field_schemas,
      decoded_semantic_field_groups: inputs.semanticFieldSchema.value.summary.semantic_field_groups,
      decoded_semantic_field_groups_closed: inputs.semanticFieldSchema.value.summary.closed_field_groups,
      decoded_semantic_field_groups_open: inputs.semanticFieldSchema.value.summary.open_field_groups,
      decoded_semantic_field_evidence_states: inputs.semanticFieldSchema.value.summary.evidence_states,
      decoded_field_paths: inputs.decodedFieldSchema.value.summary.decoded_field_paths,
      decoded_scalar_field_paths: inputs.decodedFieldSchema.value.summary.scalar_field_paths,
      decoded_array_field_paths: inputs.decodedFieldSchema.value.summary.array_field_paths,
      decoded_object_field_paths: inputs.decodedFieldSchema.value.summary.object_field_paths,
      decoded_mechanics_sensitive_field_paths: inputs.decodedFieldSchema.value.summary.mechanics_sensitive_field_paths,
      decoded_field_structural_inventory_complete: inputs.decodedFieldSchema.value.summary.structural_inventory_complete,
    },
    decoded_reference_inventory: {
      graph: relativeRepo(inputs.referenceGraph.path),
      summary: inputs.referenceGraph.value.summary,
      ambiguous_occurrence_artifact: referenceOccurrenceMetadata(inputs.referenceGraph),
      reference_candidate_artifact: referenceCandidateMetadata(inputs.referenceGraph),
      callsite_proof_artifact: callsiteProofMetadata(inputs.referenceGraph),
      semantic_field_schema_artifact: semanticFieldSchemaMetadata(inputs.semanticFieldSchema),
      decoded_field_schema_artifact: decodedFieldSchemaMetadata(inputs.decodedFieldSchema),
      policy: "Every decoded field path and every ID-like occurrence is retained. The verified manifests separate structural inventory, reference semantics, mechanics review routing, active-open, and dormant-open domains without treating names as proof.",
    },
    shard_root: relativeRepo(shardRoot),
    shards,
    inputs: Object.fromEntries(Object.entries(inputs).map(([key, input]) => [key, relativeRepo(input.path)])),
    output: relativeRepo(outputPath),
  };
}

function shard(id, file, category, blockingClass, semanticDomain, entries, policy) {
  return { id, file, category, blockingClass, semanticDomain, entries, policy };
}

function collectScriptRouteGaps(worklist) {
  const rows = [];
  for (const family of worklist.families ?? []) {
    for (const signature of family.formula_signatures ?? []) {
      for (const item of signature.work_items ?? []) {
        if ((item.static_routes ?? []).length > 0) continue;
        rows.push({
          family_id: family.family_id ?? family.script_family ?? family.id ?? null,
          formula_signature: signature.formula_signature ?? signature.signature ?? signature.id ?? null,
          ...item,
        });
      }
    }
  }
  return rows;
}

function hasPacketObservation(observations = []) {
  return observations.some((entry) => [
    "status_events",
    "mechanic_state_changes",
    "selected_attributes_examined",
    "complete_attribute_pairs",
    "same_wire_attribute_delta_observations",
    "binary_presence_equation_occurrences",
    "reversible_static_coefficient_proofs",
    "matched_lifecycle_coefficient_proofs",
    "historical_runtime_eligible_proofs",
  ].some((key) => Number(entry[key] ?? 0) > 0));
}

function validateBuildIdentity(inputs, buildId) {
  const value = values(inputs);
  for (const observed of [
    value.staticAudit.game_build,
    value.semanticRefresh.game_build,
    value.formulaLedger.static_game_build,
    value.scopeLedger.static_game_build,
    value.preflight.game_build,
    value.protocol.build_id,
    value.effectActivation.game_build,
    value.damageActivation.game_build,
    value.scriptFamilies.game_build,
    value.referenceGraph.game_build,
    value.semanticFieldSchema.game_build,
    value.decodedFieldSchema.game_build,
  ]) assert(String(observed) === buildId, `Build identity mismatch: expected ${buildId}, got ${observed}`);
  assert(value.semanticRefresh.generated_by === "tools/bpsr-current-build-semantic-refresh.mjs", "Semantic refresh report has an unexpected generator");
  assert(value.semanticRefresh.summary.hidden_omissions === 0, "Semantic refresh report contains hidden omissions");
  for (const artifact of value.semanticRefresh.artifacts) {
    const filePath = path.resolve(repoRoot, artifact.path);
    assert(existsSync(filePath), `Semantic refresh artifact is missing: ${artifact.path}`);
    assert(sha256File(filePath) === artifact.sha256, `Semantic refresh artifact is stale: ${artifact.path}`);
  }
  assert(value.coverage.worklists.damage_rows_without_explicit_action_parent.length === 0, "Exact damage action-parent blockers remain");
  assert(value.coverage.worklists.unresolved_effect_source_ids.length === 0, "Unresolved effect sources remain outside the catalog boundary");
  assert(value.referenceGraph.generated_by === "DecodedTableReferenceGraph.gen", "Decoded reference graph has an unexpected generator");
  assert(value.semanticFieldSchema.generated_by === "tools/bpsr-semantic-field-schema-ledger.mjs", "Semantic field-schema ledger has an unexpected generator");
  assert(value.semanticFieldSchema.fields.length === value.semanticFieldSchema.summary.semantic_field_groups, "Semantic field-schema ledger row count mismatch");
  assert(value.semanticFieldSchema.fields.filter((entry) => entry.resolution_state === "open").length === value.semanticFieldSchema.summary.open_field_groups, "Semantic field-schema ledger open count mismatch");
  assert(value.decodedFieldSchema.generated_by === "tools/bpsr-decoded-field-schema-manifest.mjs", "Decoded field-schema manifest has an unexpected generator");
  assert(value.decodedFieldSchema.fields.length === value.decodedFieldSchema.summary.decoded_field_paths, "Decoded field-schema manifest row count mismatch");
  assert(value.decodedFieldSchema.summary.structural_inventory_complete === true, "Decoded field-schema structural inventory is incomplete");
  validateReferenceOccurrenceArtifact(inputs.referenceGraph);
  validateReferenceCandidateArtifact(inputs.referenceGraph);
  validateCallsiteProofArtifact(inputs.referenceGraph);
}

function validateReferenceOccurrenceArtifact(input) {
  const graph = input.value;
  const artifact = graph.ambiguous_reference_occurrence_artifact;
  assert(artifact && artifact.path, "Decoded reference graph has no ambiguous occurrence artifact");
  const filePath = path.resolve(path.dirname(input.path), artifact.path);
  assert(existsSync(filePath), `Decoded reference occurrence artifact is missing: ${filePath}`);
  assert(statSync(filePath).size === Number(artifact.bytes), "Decoded reference occurrence byte count mismatch");
  assert(sha256File(filePath) === artifact.sha256, "Decoded reference occurrence artifact hash mismatch");
  assert(Number(artifact.rows) === Number(graph.summary.ambiguous_reference_occurrences), "Decoded reference occurrence row count mismatch");
}

function referenceOccurrenceMetadata(input) {
  const artifact = input.value.ambiguous_reference_occurrence_artifact;
  return {
    path: relativeRepo(path.resolve(path.dirname(input.path), artifact.path)),
    rows: artifact.rows,
    bytes: artifact.bytes,
    sha256: artifact.sha256,
    format: artifact.format,
  };
}

function validateReferenceCandidateArtifact(input) {
  const graph = input.value;
  const artifact = graph.reference_candidate_artifact;
  assert(Number(graph.schema_version) >= 3, "Decoded reference graph does not provide the schema-v3 candidate ledger");
  assert(artifact && artifact.path, "Decoded reference graph has no reference candidate artifact");
  const filePath = path.resolve(path.dirname(input.path), artifact.path);
  assert(existsSync(filePath), `Decoded reference candidate artifact is missing: ${filePath}`);
  assert(statSync(filePath).size === Number(artifact.bytes), "Decoded reference candidate byte count mismatch");
  assert(sha256File(filePath) === artifact.sha256, "Decoded reference candidate artifact hash mismatch");
  assert(
    Number(artifact.rows) === Number(
      graph.summary.reference_candidate_ledger_rows ?? graph.summary.ambiguous_reference_field_groups,
    ),
    "Decoded reference candidate row count mismatch",
  );
}

function referenceCandidateMetadata(input) {
  const artifact = input.value.reference_candidate_artifact;
  return {
    path: relativeRepo(path.resolve(path.dirname(input.path), artifact.path)),
    rows: artifact.rows,
    bytes: artifact.bytes,
    sha256: artifact.sha256,
    format: artifact.format,
  };
}

function semanticFieldSchemaMetadata(input) {
  return {
    path: relativeRepo(input.path),
    rows: input.value.summary.semantic_field_groups,
    closed_rows: input.value.summary.closed_field_groups,
    open_rows: input.value.summary.open_field_groups,
    evidence_states: input.value.summary.evidence_states,
    sha256: sha256File(input.path),
    semantic_sha256: input.value.semantic_sha256,
  };
}

function decodedFieldSchemaMetadata(input) {
  return {
    path: relativeRepo(input.path),
    rows: input.value.summary.decoded_field_paths,
    scalar_rows: input.value.summary.scalar_field_paths,
    array_rows: input.value.summary.array_field_paths,
    object_rows: input.value.summary.object_field_paths,
    mechanics_sensitive_rows: input.value.summary.mechanics_sensitive_field_paths,
    structural_inventory_complete: input.value.summary.structural_inventory_complete,
    sha256: sha256File(input.path),
    semantic_sha256: input.value.semantic_sha256,
  };
}

function validateCallsiteProofArtifact(input) {
  const graph = input.value;
  assert(Number(graph.schema_version) >= 4, "Decoded reference graph does not provide schema-v4 current-build proof lineage");
  const artifact = graph.callsite_proof_artifact;
  assert(artifact && artifact.path, "Decoded reference graph has no current-build callsite proof artifact");
  const filePath = path.resolve(path.dirname(input.path), artifact.path);
  assert(existsSync(filePath), `Decoded reference callsite proof artifact is missing: ${filePath}`);
  assert(statSync(filePath).size === Number(artifact.bytes), "Decoded reference callsite proof byte count mismatch");
  assert(sha256File(filePath) === artifact.sha256, "Decoded reference callsite proof artifact hash mismatch");
  const proof = readJson(filePath);
  assert(Number(proof.schema_version) === 3, "Decoded reference callsite proof schema is unsupported");
  assert(String(proof.game_build) === String(graph.game_build), "Decoded reference callsite proof build mismatch");
  assert(
    Number(artifact.promoted_field_schemas) === Number(graph.summary.current_build_callsite_proven_field_schemas),
    "Decoded reference promoted callsite proof count mismatch",
  );
}

function callsiteProofMetadata(input) {
  const artifact = input.value.callsite_proof_artifact;
  return {
    path: relativeRepo(path.resolve(path.dirname(input.path), artifact.path)),
    rows: artifact.promoted_field_schemas,
    bytes: artifact.bytes,
    sha256: artifact.sha256,
    schema_version: artifact.schema_version,
    game_build: artifact.game_build,
    inputs: artifact.inputs,
  };
}

function groupBy(entries, keyFn) {
  const groups = new Map();
  for (const entry of entries) {
    const key = keyFn(entry);
    if (!groups.has(key)) groups.set(key, []);
    groups.get(key).push(entry);
  }
  return new Map([...groups.entries()].sort(([left], [right]) => String(left).localeCompare(String(right))));
}

function sumGrouped(groups) {
  return [...groups.values()].reduce((total, entries) => total + entries.length, 0);
}

function slug(value) {
  return String(value).toLowerCase().replace(/[^a-z0-9]+/g, "-").replace(/^-|-$/g, "") || "other";
}

function values(inputs) {
  return Object.fromEntries(Object.entries(inputs).map(([key, input]) => [key, input.value]));
}

function stableSort(entries) {
  return [...entries].sort((left, right) => String(left.key).localeCompare(String(right.key), "en", { numeric: true }));
}

function countBy(entries, keyFn, valueFn = () => 1) {
  const output = {};
  for (const entry of entries) output[keyFn(entry)] = (output[keyFn(entry)] ?? 0) + valueFn(entry);
  return Object.fromEntries(Object.entries(output).sort(([left], [right]) => left.localeCompare(right)));
}

function uniqueSorted(entries) {
  return [...new Set(entries)].sort();
}

function textPresent(value) {
  return typeof value === "string" && value.trim().length > 0;
}

function idOf(entry) {
  return typeof entry === "number" || typeof entry === "string" ? entry : entry.action_id ?? entry.actionId ?? entry.id;
}

function asRecord(entry, key) {
  return typeof entry === "object" && entry !== null ? entry : { [key]: Number(entry) };
}

function sha256File(filePath) {
  return createHash("sha256").update(readFileSync(filePath)).digest("hex");
}

function selfTest() {
  assert(hasPacketObservation([{ status_events: 1 }]), "Positive packet observation was not detected");
  assert(!hasPacketObservation([{ status_events: 0, mechanic_state_changes: 0 }]), "Zero packet observation was incorrectly detected");
  assert(JSON.stringify(uniqueSorted(["b", "a", "a"])) === JSON.stringify(["a", "b"]), "Unique sorting failed");
  assert(countBy([{ x: "a", n: 2 }, { x: "a", n: 3 }], (entry) => entry.x, (entry) => entry.n).a === 5, "Weighted counting failed");
  console.log("bpsr-current-build-unmapped-catalog self-test passed");
}

function parseArgs(args) {
  const output = {};
  for (let index = 0; index < args.length; index += 1) {
    const token = args[index];
    if (!token.startsWith("--")) throw new Error(`Unexpected argument ${token}`);
    const key = token.slice(2);
    const next = args[index + 1];
    if (!next || next.startsWith("--")) throw new Error(`Missing value for --${key}`);
    output[key] = next;
    index += 1;
  }
  return output;
}

function required(value, key) {
  if (!value[key]) throw new Error(`Missing --${key}`);
  return value[key];
}

function resolvePath(value) {
  return path.isAbsolute(value) ? value : path.resolve(repoRoot, value);
}

function relativeRepo(value) {
  return path.relative(repoRoot, value).replaceAll("\\", "/");
}

function readJson(filePath) {
  return JSON.parse(readFileSync(filePath, "utf8"));
}

function writeJson(filePath, value) {
  mkdirSync(path.dirname(filePath), { recursive: true });
  writeFileSync(filePath, `${JSON.stringify(value, null, 2)}\n`, "utf8");
}

function assert(condition, message) {
  if (!condition) throw new Error(message);
}

function usage(exitCode) {
  console.log("Usage:");
  console.log("  node tools/bpsr-current-build-unmapped-catalog.mjs generate --build-root <directory> --reference-graph <json> [--semantic-field-schema <json>] [--decoded-field-schema <json>] [--output <json>] [--shard-root <directory>]");
  console.log("  node tools/bpsr-current-build-unmapped-catalog.mjs self-test");
  process.exit(exitCode);
}
