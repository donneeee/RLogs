#!/usr/bin/env node

import assert from "node:assert/strict";
import { createHash } from "node:crypto";
import { readFileSync, statSync, writeFileSync } from "node:fs";
import path from "node:path";

const GENERATED_BY = "tools/bpsr-rdps-origin-graph-diff-proof.mjs";
const SUITE_ID = "origin-graph-diff";
const ORIGIN_DOMAINS = new Set([
  "skills",
  "talents",
  "imagines",
  "psychoscope-factors",
  "equipment-set-bonuses",
  "relationships-recount",
]);

const [command = "help", ...rest] = process.argv.slice(2);
const options = parseOptions(rest);
if (command === "generate") generate(options);
else if (command === "verify") verify(options);
else if (command === "self-test") selfTest();
else usage(command === "help" ? 0 : 1);

function buildReport(values) {
  const gameBuild = required(values, "build");
  const schemaProof = source(required(values, "schema-proof"));
  const aoyi = source(required(values, "aoyi-ledger"));
  const recipient = source(required(values, "recipient-ledger"));
  const formulaReferences = source(required(values, "formula-reference-scan"));
  const sourceChains = source(required(values, "source-chain-scan"));
  const damageRoutes = source(required(values, "damage-source-route"));
  const relationships = source(required(values, "relationships-manifest"));
  const seasonalDiff = source(required(values, "seasonal-diff"));
  const conservation = source(required(values, "conservation"));

  validateSchemaProof(schemaProof.value, gameBuild);
  validateAoyi(aoyi.value, gameBuild);
  validateRecipient(recipient.value, gameBuild);
  validateReferenceScan(formulaReferences.value, gameBuild, 588, 3_484);
  validateReferenceScan(sourceChains.value, gameBuild, 7, 43);
  validateDamageRoutes(damageRoutes.value, gameBuild);
  validateRelationships(
    relationships,
    schemaProof.value,
    gameBuild,
  );
  const domainDiff = validateOriginDomainDiff(seasonalDiff.value, gameBuild);
  const segment = validateConservation(conservation.value, gameBuild);

  const recipientQueueTotal = Object.values(recipient.value.summary.scope_queues)
    .reduce((sum, count) => sum + count, 0);
  const report = {
    schema_version: 1,
    generated_by: GENERATED_BY,
    suite_id: SUITE_ID,
    game_build: gameBuild,
    baseline_build: seasonalDiff.value.baselineBuild,
    policy: {
      exact_numeric_ids_are_authoritative: true,
      localized_names_are_runtime_keys: false,
      static_references_are_provider_ownership: false,
      recount_membership_is_packet_damage_source: false,
      unresolved_routes_are_retained: true,
      unresolved_routes_are_hidden: false,
      graph_diff_grants_formula_authority: false,
      graph_diff_grants_provider_credit: false,
      candidate_data_is_auto_promoted: false,
    },
    sources: {
      complete_game_file_schema_diff: receipt(schemaProof),
      current_aoyi_origin_ledger: receipt(aoyi),
      recipient_scope_ledger: receipt(recipient),
      complete_formula_gap_reference_scan: receipt(formulaReferences),
      focused_source_chain_reference_scan: receipt(sourceChains),
      damage_source_route_partition: receipt(damageRoutes),
      relationships_recount_manifest: receipt(relationships),
      seasonal_domain_diff: receipt(seasonalDiff),
      exact_pack_conservation_boundary: receipt(conservation),
    },
    origin_graph_coverage: {
      exact_build_sources_linked:
        schemaProof.value.schema_coverage.candidate_manifest_sources_linked_to_exact_build,
      origin_domains_compared: ORIGIN_DOMAINS.size,
      changed_origin_domains: domainDiff.changed,
      unchanged_origin_domains: domainDiff.unchanged,
      relationship_manifest_sources: relationships.value.summary.sourceCount,
      relationship_rows: relationships.value.summary.rowCount,
      aoyi_skills: aoyi.value.summary.current_aoyi_skills,
      aoyi_exact_relationship_candidates:
        aoyi.value.summary.exact_relationship_candidates,
      aoyi_exact_damage_chain_candidates:
        aoyi.value.summary.exact_damage_chain_candidates,
      aoyi_exact_damage_chain_ids: aoyi.value.summary.exact_damage_chain_ids,
      aoyi_missing_damage_chain_ids:
        aoyi.value.summary.missing_exact_damage_chain_ids,
      aoyi_exact_damage_attr_rows: aoyi.value.summary.exact_damage_attr_rows,
      aoyi_missing_damage_attr_rows:
        aoyi.value.summary.missing_exact_damage_attr_rows,
      formula_gap_targets: formulaReferences.value.summary.distinct_target_values,
      formula_gap_direct_references:
        formulaReferences.value.summary.direct_scalar_references,
      formula_gap_targets_without_references:
        formulaReferences.value.summary.targets_without_references,
      focused_source_chain_targets: sourceChains.value.summary.distinct_target_values,
      focused_source_chain_references:
        sourceChains.value.summary.direct_scalar_references,
      focused_targets_without_references:
        sourceChains.value.summary.targets_without_references,
      damage_candidates: damageRoutes.value.summary.candidate_rows,
      damage_candidates_with_static_route:
        damageRoutes.value.summary.candidates_with_static_route,
      unresolved_damage_route_candidates:
        damageRoutes.value.summary.keys_with_unresolved_candidates,
      recipient_scope_candidates: recipient.value.summary.candidates,
      recipient_scope_queue_total: recipientQueueTotal,
      unresolved_provider_recipient_candidates:
        recipient.value.summary.scope_queues["unresolved-provider-recipient"],
      current_build_promotion_eligible_candidates:
        recipient.value.summary.candidates_eligible_for_current_build_promotion,
    },
    unresolved_frontier: {
      damage_route_candidates: damageRoutes.value.summary.keys_with_unresolved_candidates,
      provider_recipient_candidates:
        recipient.value.summary.scope_queues["unresolved-provider-recipient"],
      owner_local_scope_holds:
        recipient.value.summary.scope_queues[
          "owner-local-formula-context-requires-recipient-proof"
        ],
      mixed_source_scope_holds:
        recipient.value.summary.scope_queues[
          "mixed-source-output-and-open-owner-context"
        ],
      all_candidates_resolved: false,
      provider_recipient_replay_required: true,
      packet_damage_source_selection_required: true,
      formula_stage_replay_required: true,
    },
    conservation: {
      observed_damage_events: segment.damage_events,
      ordinary_raw_damage: segment.ordinary_raw_damage,
      ordinary_rdps_damage: segment.ordinary_rdps_damage,
      exact_party_conservation: true,
      scope:
        "complete static origin graph diff plus gap-free exact-pack zero-transfer replay; unresolved ownership and formula routes remain disabled",
    },
    conclusion: {
      suite_status: "passed",
      observed_event_count: segment.damage_events,
      exact_party_conservation: true,
      complete_origin_candidate_partition_proven: true,
      all_changed_origin_rows_retained: true,
      origin_graph_diff_proven: true,
      all_origin_routes_resolved: false,
      provider_recipient_replay_proven: false,
      formula_stage_replay_proven: false,
      provider_rdps_credit_allowed: false,
      runtime_promotion_allowed: false,
    },
  };
  return { ...report, content_sha256: contentHash(report) };
}

function validateSchemaProof(value, gameBuild) {
  assert.equal(value.schema_version, 1);
  assert.equal(value.generated_by, "tools/bpsr-rdps-game-file-schema-diff-proof.mjs");
  assert.equal(value.game_build, gameBuild);
  assert.equal(value.conclusion?.suite_status, "passed");
  assert.equal(value.conclusion?.game_file_schema_diff_proven, true);
  assert.equal(value.conclusion?.exact_build_source_linkage_proven, true);
  assert.equal(value.conclusion?.runtime_promotion_allowed, false);
  assert.equal(value.content_sha256, contentHash(withoutContentHash(value)));
  for (const entry of Object.values(value.sources)) {
    if (Array.isArray(entry)) entry.forEach(verifyReceipt);
    else verifyReceipt(entry);
  }
}

function validateAoyi(value, gameBuild) {
  assert.equal(value.schema_version, 18);
  assert.equal(value.game_build, gameBuild);
  assert.equal(value.summary?.current_aoyi_skills, value.skills?.length);
  assert.equal(value.summary?.missing_exact_damage_chain_ids, 0);
  assert.equal(value.summary?.missing_exact_damage_attr_rows, 0);
  assert.equal(value.summary?.missing_exact_source_target_damage_attr_rows, 0);
  assert.ok(value.summary?.exact_relationship_candidates > 0);
  assert.ok(value.summary?.exact_damage_chain_candidates > 0);
  assert.equal(value.summary?.enabled_for_rdps, 0);
}

function validateRecipient(value, gameBuild) {
  assert.equal(value.schema_version, 14);
  assert.equal(value.static_game_build, gameBuild);
  assert.equal(value.policy?.unresolved_evidence_hidden, false);
  assert.equal(value.policy?.static_description_proves_packet_recipient, false);
  assert.equal(value.policy?.historical_scope_promotes_current_build, false);
  assert.equal(value.policy?.current_component_scope_enables_runtime_attribution, false);
  assert.equal(value.summary?.candidates, value.candidates?.length);
  assert.equal(
    Object.values(value.summary?.scope_queues ?? {}).reduce((sum, count) => sum + count, 0),
    value.summary?.candidates,
  );
  assert.equal(value.summary?.candidates_eligible_for_current_build_promotion, 0);
}

function validateReferenceScan(value, gameBuild, targets, references) {
  assert.equal(value.schema_version, 4);
  assert.equal(value.build_id, gameBuild);
  assert.equal(value.policy?.exact_build_required, true);
  assert.equal(value.policy?.direct_references_are_route_authority, false);
  assert.equal(value.policy?.unresolved_targets_hidden, false);
  assert.equal(value.summary?.decoded_tables_scanned, value.table_sources?.length);
  assert.equal(value.summary?.distinct_target_values, targets);
  assert.equal(value.summary?.targets_with_references, targets);
  assert.equal(value.summary?.targets_without_references, 0);
  assert.equal(value.summary?.direct_scalar_references, references);
  assert.equal(value.references?.length, references);
}

function validateDamageRoutes(value, gameBuild) {
  assert.equal(value.schema_version, 9);
  assert.equal(value.game_build, gameBuild);
  assert.equal(value.policy?.exact_build_tables_required, true);
  assert.equal(value.policy?.packet_damage_source_required, true);
  assert.equal(value.policy?.unknown_source_values_retained, true);
  assert.equal(value.policy?.unresolved_candidates_hidden, false);
  assert.equal(value.summary?.lookup_keys, value.keys?.length);
  assert.equal(
    value.summary?.candidates_with_static_route +
      value.summary?.keys_with_unresolved_candidates,
    value.summary?.candidate_rows,
  );
  assert.ok(value.summary?.keys_with_unresolved_candidates > 0);
}

function validateRelationships(entry, schemaProof, gameBuild) {
  const value = entry.value;
  assert.equal(value.schemaVersion, 1);
  assert.equal(value.gameBuild, gameBuild);
  assert.equal(value.domain, "relationships-recount");
  assert.equal(value.policy?.allRowsRetained, true);
  assert.equal(value.policy?.unresolvedRowsHidden, false);
  assert.deepEqual(value.missingRequiredInputs, []);
  assert.deepEqual(value.missingOptionalInputs, []);
  assert.ok(value.summary?.sourceCount > 0);
  assert.ok(value.summary?.rowCount > 0);
  const schemaReceipt = schemaProof.sources.candidate_domain_manifests.find(
    (candidate) => candidate.path === entry.path,
  );
  assert.ok(schemaReceipt, "relationship manifest absent from schema proof");
  assert.deepEqual(schemaReceipt, receipt(entry));
}

function validateOriginDomainDiff(value, gameBuild) {
  assert.equal(value.schemaVersion, 1);
  assert.equal(value.candidateBuild, gameBuild);
  assert.equal(value.policy?.allRowsRetained, true);
  assert.equal(value.policy?.unresolvedRowsHidden, false);
  assert.deepEqual(value.missingManifests, []);
  const changed = new Set(value.changedDomains.map((entry) => entry.domain));
  const unchanged = new Set(value.unchangedDomains);
  for (const domain of ORIGIN_DOMAINS) {
    assert.equal(
      Number(changed.has(domain)) + Number(unchanged.has(domain)),
      1,
      `origin domain not uniquely compared ${domain}`,
    );
  }
  return {
    changed: [...ORIGIN_DOMAINS].filter((domain) => changed.has(domain)).length,
    unchanged: [...ORIGIN_DOMAINS].filter((domain) => unchanged.has(domain)).length,
  };
}

function validateConservation(value, gameBuild) {
  assert.equal(value.schema_version, 1);
  assert.equal(value.game_build, gameBuild);
  const segment = value.exact_pack_gap_free_segment;
  assert.ok(segment?.damage_events > 0);
  assert.equal(segment?.ordinary_raw_damage, segment?.ordinary_rdps_damage);
  assert.equal(segment?.ordinary_damage_conserved, true);
  return segment;
}

function generate(values) {
  const output = path.resolve(required(values, "output"));
  writeFileSync(output, `${JSON.stringify(buildReport(values), null, 2)}\n`, "utf8");
  console.log(output);
}

function verify(values) {
  const input = path.resolve(required(values, "input"));
  assert.deepEqual(JSON.parse(readFileSync(input, "utf8")), buildReport(values));
  console.log(input);
}

function selfTest() {
  const queues = { a: 3, b: 4, c: 5 };
  assert.equal(Object.values(queues).reduce((sum, count) => sum + count, 0), 12);
  assert.equal(ORIGIN_DOMAINS.size, 6);
  console.log("self-test passed");
}

function source(file) {
  const absolute = path.resolve(file);
  const bytes = readFileSync(absolute);
  return {
    path: path.relative(process.cwd(), absolute).replaceAll("\\", "/"),
    bytes: bytes.length,
    sha256: sha256(bytes),
    value: JSON.parse(bytes.toString("utf8")),
  };
}

function receipt(entry) {
  return { path: entry.path, bytes: entry.bytes, sha256: entry.sha256 };
}

function verifyReceipt(entry) {
  const absolute = path.resolve(entry.path);
  assert.equal(statSync(absolute).size, entry.bytes);
  assert.equal(sha256(readFileSync(absolute)), entry.sha256);
}

function withoutContentHash(value) {
  const clone = structuredClone(value);
  delete clone.content_sha256;
  return clone;
}

function contentHash(value) {
  return sha256(Buffer.from(JSON.stringify(stable(value)), "utf8"));
}

function stable(value) {
  if (Array.isArray(value)) return value.map(stable);
  if (value && typeof value === "object") {
    return Object.fromEntries(
      Object.keys(value).sort().map((key) => [key, stable(value[key])]),
    );
  }
  return value;
}

function sha256(value) {
  return createHash("sha256").update(value).digest("hex");
}

function parseOptions(args) {
  const values = {};
  for (let index = 0; index < args.length; index += 2) {
    const key = args[index];
    const value = args[index + 1];
    if (!key?.startsWith("--") || value === undefined) usage(1);
    values[key.slice(2)] = value;
  }
  return values;
}

function required(values, key) {
  const value = values[key];
  if (!value) throw new Error(`missing --${key}`);
  return value;
}

function usage(exitCode) {
  console.log(
    "Usage:\n" +
      "  node tools/bpsr-rdps-origin-graph-diff-proof.mjs generate --build <id> --schema-proof <json> --aoyi-ledger <json> --recipient-ledger <json> --formula-reference-scan <json> --source-chain-scan <json> --damage-source-route <json> --relationships-manifest <json> --seasonal-diff <json> --conservation <json> --output <json>\n" +
      "  node tools/bpsr-rdps-origin-graph-diff-proof.mjs verify --build <id> --schema-proof <json> --aoyi-ledger <json> --recipient-ledger <json> --formula-reference-scan <json> --source-chain-scan <json> --damage-source-route <json> --relationships-manifest <json> --seasonal-diff <json> --conservation <json> --input <json>\n" +
      "  node tools/bpsr-rdps-origin-graph-diff-proof.mjs self-test",
  );
  process.exit(exitCode);
}
