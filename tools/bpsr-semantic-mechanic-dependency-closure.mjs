#!/usr/bin/env node

import { createHash } from "node:crypto";
import {
  existsSync,
  mkdirSync,
  readFileSync,
  readdirSync,
  statSync,
  writeFileSync,
} from "node:fs";
import path from "node:path";

const EFFECT_SOURCE_NAMESPACE_TABLES = Object.freeze({
  itemnames: "ItemTable",
  monsternames: "MonsterTable",
  skill_aoyi_icons: "SkillAoyiTable",
});

const [command = "help", ...rest] = process.argv.slice(2);
const options = parseArgs(rest);

if (command === "generate") generate(resolveOptions(options));
else if (command === "diff") diff(options);
else if (command === "verify") verifyArtifact(required(options, "input"));
else if (command === "self-test") selfTest();
else usage(command === "help" ? 0 : 1);

function resolveOptions(args) {
  return {
    build: required(args, "build"),
    semanticAudit: absolute(required(args, "semantic-audit")),
    effectSources: absolute(required(args, "effect-sources")),
    decodedRoot: absolute(required(args, "decoded-root")),
    referenceGraph: absolute(required(args, "reference-graph")),
    referenceOccurrences: absolute(required(args, "reference-occurrences")),
    referenceCandidates: absolute(required(args, "reference-candidates")),
    ctbTableIdentities: absolute(required(args, "ctb-table-identities")),
    decodedFieldSchema: args["decoded-field-schema"]
      ? absolute(args["decoded-field-schema"])
      : null,
    output: absolute(required(args, "output")),
  };
}

function generate(config) {
  if (!/^\d+$/.test(config.build)) throw new Error("Build must contain only ASCII digits");
  for (const [name, file] of Object.entries(config)) {
    if (["build", "decodedRoot", "output"].includes(name) || file === null) continue;
    requireFile(file, name);
  }
  if (!statSync(config.decodedRoot).isDirectory()) {
    throw new Error(`Decoded root is not a directory: ${config.decodedRoot}`);
  }

  const audit = readJson(config.semanticAudit, "semantic audit");
  const effectSourceArtifact = readJson(config.effectSources, "effect sources");
  const graph = readJson(config.referenceGraph, "reference graph");
  const ctbIdentityArtifact = readJson(config.ctbTableIdentities, "CTB table identity map");
  const decodedFieldSchema = config.decodedFieldSchema
    ? readJson(config.decodedFieldSchema, "decoded field schema")
    : null;
  requireBuild(audit, config.build, "semantic audit");
  requireBuild(graph, config.build, "reference graph");
  requireBuild(ctbIdentityArtifact, config.build, "CTB table identity map");
  if (decodedFieldSchema) requireBuild(decodedFieldSchema, config.build, "decoded field schema");

  const findings = Array.isArray(audit.findings) ? audit.findings : [];
  const effectSources = effectSourceArtifact.effectSourcesById ?? {};
  const decoded = loadDecodedTables(config.decodedRoot);
  const exact = indexExactEdges(graph.exact_edges ?? []);
  const candidates = loadJsonLines(config.referenceCandidates, "reference candidates");
  const candidateByField = new Map(candidates.map((entry) => [entry.semantic_field_key, entry]));
  const occurrences = loadJsonLines(config.referenceOccurrences, "reference occurrences");
  const occurrenceIndex = indexOccurrences(occurrences);
  const fieldPathIndex = indexFieldPaths(decodedFieldSchema);
  const ctbIdentityMap = ctbIdentityArtifact.mapping_by_raw_source ?? {};

  const mechanics = findings
    .map((finding) => buildMechanicClosure({
      finding,
      effectSource: effectSources[finding.source_id] ?? null,
      decoded,
      exact,
      candidateByField,
      occurrenceIndex,
      fieldPathIndex,
      ctbIdentityMap,
    }))
    .sort((a, b) => a.source_id.localeCompare(b.source_id));

  const affectedTables = sortedUnique(mechanics.flatMap((entry) => entry.affected_tables));
  const candidateEvidenceTables = sortedUnique(
    mechanics.flatMap((entry) => entry.candidate_evidence_tables),
  );
  const incomingEvidenceTables = sortedUnique(
    mechanics.flatMap((entry) => entry.incoming_evidence_tables),
  );
  const report = {
    schema_version: 1,
    generated_by: "tools/bpsr-semantic-mechanic-dependency-closure.mjs",
    game_build: config.build,
    policy: {
      steam_manifest_is_physical_change_locator_only: true,
      semantic_dependency_closure_selects_regeneration_and_reproof: true,
      ambiguous_namespace_matches_are_never_promoted_to_relationships: true,
      every_semantic_finding_is_retained: true,
      unresolved_evidence_hidden: false,
      identifier_values_rewritten: false,
      full_completeness_audit_required_after_every_patch: true,
      raw_ctb_sources_require_exact_build_locked_identity_proof: true,
      localization_keys_retained_as_external_plugin_references: true,
    },
    inputs: Object.fromEntries(
      Object.entries(config)
        .filter(([key, value]) => key !== "output" && key !== "decodedRoot" && value !== null)
        .map(([key, value]) => [key, key === "build" ? value : describeFile(value)]),
    ),
    decoded_root: normalize(config.decodedRoot),
    ctb_table_identities: ctbIdentityMap,
    summary: {
      semantic_findings: mechanics.length,
      findings_with_effect_source: mechanics.filter((entry) => entry.effect_source_present).length,
      seed_identifiers: mechanics.reduce((sum, entry) => sum + entry.seeds.length, 0),
      attached_decoded_rows: mechanics.reduce((sum, entry) => sum + entry.decoded_rows.length, 0),
      exact_reference_edges: mechanics.reduce((sum, entry) => sum + entry.exact_reference_edges.length, 0),
      candidate_reference_edges: mechanics.reduce((sum, entry) => sum + entry.candidate_reference_edges.length, 0),
      incoming_reference_evidence: mechanics.reduce((sum, entry) => sum + entry.incoming_reference_evidence.length, 0),
      mechanics_sensitive_fields: mechanics.reduce((sum, entry) => sum + entry.mechanics_sensitive_fields.length, 0),
      affected_tables: affectedTables.length,
      candidate_evidence_tables: candidateEvidenceTables.length,
      incoming_evidence_tables: incomingEvidenceTables.length,
      unresolved_dependency_groups: mechanics.reduce((sum, entry) => sum + entry.unresolved_dependencies.length, 0),
      external_reference_groups: mechanics.reduce((sum, entry) => sum + entry.external_references.length, 0),
      zero_hidden_omissions: true,
    },
    affected_tables: affectedTables,
    candidate_evidence_tables: candidateEvidenceTables,
    incoming_evidence_tables: incomingEvidenceTables,
    mechanics,
  };
  report.content_sha256 = canonicalHash({ ...report, content_sha256: undefined });
  mkdirSync(path.dirname(config.output), { recursive: true });
  writeFileSync(config.output, `${JSON.stringify(report, null, 2)}\n`, "utf8");
  verify(config.output);
  console.log(
    `Semantic mechanic dependency closure generated for build ${config.build}: ` +
      `${report.summary.semantic_findings} findings, ${report.summary.attached_decoded_rows} rows, ` +
      `${report.summary.affected_tables} affected tables, ${report.summary.unresolved_dependency_groups} unresolved dependency groups.`,
  );
}

function buildMechanicClosure(context) {
  const { finding, effectSource, decoded, exact, candidateByField, occurrenceIndex, fieldPathIndex, ctbIdentityMap } = context;
  const seeds = collectSeeds(finding, effectSource);
  const rowQueue = [];
  const rowKeys = new Set();
  const decodedRows = [];
  const exactEdges = [];
  const candidateEdges = [];
  const incomingEvidence = [];
  const mechanicsFields = [];
  const unresolved = [];
  const externalReferences = [];

  for (const seed of seeds) {
    if (seed.roles.includes("localization-description")) {
      externalReferences.push({
        kind: "localization-key",
        seed_id: seed.id,
        seed_roles: seed.roles,
        sources: seed.sources,
        status: "retained-outside-decoded-table-namespace",
        authority: "game-locale-assets-via-localization-plugin",
      });
    }
    const mechanicalSeed = {
      ...seed,
      roles: seed.roles.filter((role) => role !== "localization-description"),
    };
    if (mechanicalSeed.roles.length === 0) continue;
    const preferred = preferredTables(mechanicalSeed, finding, effectSource, decoded.tableNames, ctbIdentityMap);
    const matches = decoded.rowsById.get(mechanicalSeed.id) ?? [];
    const selected = preferred.length > 0
      ? matches.filter((match) => preferred.includes(match.table))
      : matches.length === 1 ? matches : [];
    if (selected.length === 0) {
      unresolved.push({
        kind: matches.length > 1 ? "seed-namespace-ambiguous" : "seed-has-no-decoded-primary-row",
        seed_id: mechanicalSeed.id,
        seed_roles: mechanicalSeed.roles,
        candidate_tables: matches.map((match) => match.table).sort(),
      });
    }
    for (const match of selected) enqueueRow(match.table, match.id, 0, `seed:${mechanicalSeed.roles.join("+")}`);
  }

  while (rowQueue.length > 0) {
    const queued = rowQueue.shift();
    const row = decoded.rowsByKey.get(`${queued.table}/${queued.id}`);
    if (!row) continue;
    const summary = summarizeRow(queued.table, queued.id, row.value, queued.depth, queued.via);
    decodedRows.push(summary);
    mechanicsFields.push(...extractMechanicsFields(queued.table, queued.id, row.value, fieldPathIndex));

    for (const edge of exact.outgoing.get(`${queued.table}/${queued.id}`) ?? []) {
      exactEdges.push(edge);
      if (queued.depth < 2) enqueueRow(edge.target_table, String(edge.target_id), queued.depth + 1, `exact:${edge.relationship}`);
    }

    for (const occurrence of occurrenceIndex.outgoing.get(`${queued.table}/${queued.id}`) ?? []) {
      const candidate = candidateByField.get(occurrence.semantic_field_key);
      const fullyResolved = (candidate?.target_candidates ?? []).filter(
        (target) => target.all_distinct_values_resolve && target.all_nonzero_occurrences_resolve,
      );
      const unique = fullyResolved.length === 1 ? fullyResolved[0] : null;
      const evidence = {
        source_table: occurrence.source_table,
        source_id: String(occurrence.source_id),
        source_pointer: occurrence.json_pointer,
        semantic_field_key: occurrence.semantic_field_key,
        candidate_id: String(occurrence.candidate_id),
        classification: occurrence.classification,
        full_coverage_target_tables: fullyResolved.map((target) => target.target_table).sort(),
        semantic_state: unique
          ? "unique-full-coverage-needs-semantic-proof"
          : fullyResolved.length > 1
            ? "multiple-full-coverage-namespaces"
            : "no-full-coverage-target",
      };
      candidateEdges.push(evidence);
    }
  }

  const closureIds = new Set([
    ...seeds.map((seed) => seed.id),
    ...decodedRows.map((row) => row.row_id),
  ]);
  for (const id of closureIds) {
    for (const edge of exact.incoming.get(id) ?? []) {
      incomingEvidence.push({ ...edge, evidence_kind: "exact-edge" });
    }
    for (const occurrence of occurrenceIndex.incoming.get(id) ?? []) {
      incomingEvidence.push({
        source_table: occurrence.source_table,
        source_id: String(occurrence.source_id),
        source_pointer: occurrence.json_pointer,
        semantic_field_key: occurrence.semantic_field_key,
        target_id: String(occurrence.candidate_id),
        evidence_kind: "reference-like-unproven",
      });
    }
  }

  for (const issue of finding.issues ?? []) {
    unresolved.push({
      kind: issue.category,
      severity: issue.severity,
      required_model: issue.required_model,
      evidence: issue.evidence,
    });
  }

  const affectedTables = sortedUnique([
    ...decodedRows.map((row) => row.table),
    ...exactEdges.flatMap((edge) => [edge.source_table, edge.target_table]),
  ]);
  const candidateEvidenceTables = sortedUnique(
    candidateEdges.flatMap((edge) => [edge.source_table, ...edge.full_coverage_target_tables]),
  );
  const incomingEvidenceTables = sortedUnique(
    incomingEvidence.map((edge) => edge.source_table),
  );

  return {
    source_id: finding.source_id,
    source_name: finding.source_name,
    source_rule_id: finding.source_rule_id,
    effect_source_present: effectSource !== null,
    source_kind: effectSource?.sourceKind ?? null,
    source_type: effectSource?.sourceType ?? null,
    source_table: effectSource?.sourceTable ?? null,
    source_table_decoded_identity: effectSource?.sourceTable
      ? ctbIdentityMap[effectSource.sourceTable] ?? null
      : null,
    promotion_blocked: finding.promotion_blocked === true,
    issue_categories: sortedUnique((finding.issues ?? []).map((issue) => issue.category)),
    seeds,
    affected_tables: affectedTables,
    candidate_evidence_tables: candidateEvidenceTables,
    incoming_evidence_tables: incomingEvidenceTables,
    decoded_rows: dedupe(decodedRows, (row) => `${row.table}/${row.row_id}`),
    exact_reference_edges: dedupe(exactEdges, edgeKey),
    candidate_reference_edges: dedupe(candidateEdges, candidateKey),
    incoming_reference_evidence: dedupe(incomingEvidence, incomingKey),
    mechanics_sensitive_fields: dedupe(mechanicsFields, (entry) => `${entry.table}/${entry.row_id}${entry.pointer}`),
    external_references: dedupe(externalReferences, (entry) => `${entry.kind}/${entry.seed_id}`),
    unresolved_dependencies: dedupe(unresolved, (entry) => canonicalHash(entry)),
    static_resolution_state: unresolved.some((entry) => entry.kind.startsWith("seed-"))
      ? "partial-static-closure"
      : "static-dependency-closure-complete",
    current_build_runtime_proof_state: finding.promotion_blocked
      ? "runtime-or-formula-proof-required"
      : "not-blocked-by-semantic-audit",
  };

  function enqueueRow(table, id, depth, via) {
    const key = `${table}/${id}`;
    if (rowKeys.has(key)) return;
    rowKeys.add(key);
    rowQueue.push({ table, id: String(id), depth, via });
  }
}

function collectSeeds(finding, effectSource) {
  const seeds = new Map();
  walk(finding, "finding");
  if (effectSource) walk(effectSource, "effect-source");
  return [...seeds.values()].sort((a, b) => compareNumericStrings(a.id, b.id) || a.roles.join().localeCompare(b.roles.join()));

  function walk(value, location, key = "") {
    if (Array.isArray(value)) {
      if (isIdentifierKey(key)) for (const item of value) add(item, key, location);
      value.forEach((item, index) => {
        if (item && typeof item === "object") walk(item, `${location}/${key}/${index}`);
      });
      return;
    }
    if (!value || typeof value !== "object") return;
    for (const [childKey, child] of Object.entries(value)) {
      if (isIdentifierKey(childKey)) {
        if (Array.isArray(child)) child.forEach((item) => add(item, childKey, location));
        else add(child, childKey, location);
      }
      if (child && typeof child === "object") walk(child, `${location}/${childKey}`, childKey);
    }
  }

  function add(value, key, location) {
    const id = numericId(value);
    if (!id) return;
    const role = seedRole(key);
    const existing = seeds.get(id) ?? { id, roles: [], sources: [] };
    existing.roles = sortedUnique([...existing.roles, role]);
    existing.sources = sortedUnique([...existing.sources, `${location}/${key}`]);
    seeds.set(id, existing);
  }
}

function isIdentifierKey(key) {
  return /^(?:sourceEntityId|buffId|buffIds|activationBuffIds|linkedBuffIds|targetDamageIds|target_damage_ids|targetRecountIds|target_recount_ids|damageIds|recountIds|skillId|skillIds|ownerSourceId|ownerSkillId|ownerSkillEffectId|sourceConfigId|sourceConfigIds|itemId|itemIds|monsterId|monsterIds|talentId|descriptionId)$/i.test(key);
}

function seedRole(key) {
  const lower = key.toLowerCase();
  if (lower.includes("activationbuff")) return "activation-buff";
  if (lower.includes("buff")) return "runtime-buff";
  if (lower.includes("damage")) return "damage-row";
  if (lower.includes("recount")) return "recount-row";
  if (lower.includes("skill")) return "skill-or-effect";
  if (lower.includes("talent")) return "talent";
  if (lower.includes("description")) return "localization-description";
  if (lower.includes("item")) return "item-owner";
  if (lower.includes("monster")) return "monster-owner";
  if (lower.includes("sourceconfig")) return "source-config";
  return "source-entity";
}

function preferredTables(seed, finding, effectSource, tableNames, ctbIdentityMap = {}) {
  const preferred = new Set();
  if (seed.roles.includes("runtime-buff") || seed.roles.includes("activation-buff")) preferred.add("BuffTable");
  if (seed.roles.includes("damage-row")) preferred.add("DamageAttrTable");
  if (seed.roles.includes("recount-row")) preferred.add("RecountTable");
  if (seed.roles.includes("talent")) preferred.add("TalentTable");
  if (seed.roles.includes("item-owner")) preferred.add("ItemTable");
  if (seed.roles.includes("monster-owner")) preferred.add("MonsterTable");
  if (seed.roles.includes("skill-or-effect")) {
    preferred.add("SkillTable");
    preferred.add("SkillEffectTable");
    preferred.add("SkillFightLevelTable");
  }
  if (seed.roles.includes("source-entity")) {
    const qualifiedTables = effectSourceNamespaceTables(seed, effectSource, tableNames);
    for (const table of qualifiedTables) preferred.add(table);
    if (qualifiedTables.length === 0) {
      if (finding.source_id.startsWith("buff-source:")) preferred.add("BuffTable");
      if (finding.source_id.startsWith("talent:")) preferred.add("TalentTable");
      if (effectSource?.sourceTable && !effectSource.sourceTable.startsWith("CTB:")) {
        const exact = effectSource.sourceTable.endsWith("Table")
          ? effectSource.sourceTable
          : `${effectSource.sourceTable.replace(/Name$/, "")}Table`;
        if (tableNames.includes(exact)) preferred.add(exact);
      }
      const decodedIdentity = effectSource?.sourceTable
        ? ctbIdentityMap[effectSource.sourceTable]
        : null;
      if (decodedIdentity && tableNames.includes(decodedIdentity)) preferred.add(decodedIdentity);
    }
  }
  return [...preferred].filter((table) => tableNames.includes(table)).sort();
}

function effectSourceNamespaceTables(seed, effectSource, tableNames) {
  const qualified = new Set();
  for (const evidence of effectSource?.evidence ?? []) {
    if (String(evidence.ownerSourceId ?? "") !== seed.id) continue;
    const table = EFFECT_SOURCE_NAMESPACE_TABLES[evidence.ownerNameSource];
    if (table && tableNames.includes(table)) qualified.add(table);
  }
  return [...qualified].sort();
}

function loadDecodedTables(root) {
  const rowsByKey = new Map();
  const rowsById = new Map();
  const tableNames = [];
  for (const name of readdirSync(root).filter((file) => file.endsWith(".json")).sort()) {
    const table = path.basename(name, ".json");
    const artifact = readJson(path.join(root, name), table);
    if (!artifact || Array.isArray(artifact) || typeof artifact !== "object") continue;
    tableNames.push(table);
    for (const [id, value] of Object.entries(artifact)) {
      const row = { table, id: String(id), value };
      rowsByKey.set(`${table}/${id}`, row);
      const matches = rowsById.get(String(id)) ?? [];
      matches.push(row);
      rowsById.set(String(id), matches);
    }
  }
  return { rowsByKey, rowsById, tableNames };
}

function summarizeRow(table, rowId, value, depth, via) {
  const selected = {};
  for (const [key, child] of Object.entries(value ?? {})) {
    if (isIdentifierKey(key) || isMechanicsKey(key) || /(?:Name|Desc|Type|Tag|Rule)$/i.test(key)) {
      selected[key] = child;
    }
  }
  return {
    table,
    row_id: String(rowId),
    depth,
    reached_via: via,
    row_sha256: canonicalHash(value),
    selected_fields: selected,
  };
}

function extractMechanicsFields(table, rowId, value, fieldPathIndex) {
  const output = [];
  walk(value, "");
  return output;
  function walk(child, pointer) {
    if (Array.isArray(child)) {
      child.forEach((item, index) => walk(item, `${pointer}/${index}`));
      return;
    }
    if (child && typeof child === "object") {
      for (const [key, item] of Object.entries(child)) walk(item, `${pointer}/${escapePointer(key)}`);
      return;
    }
    const key = pointer.split("/").at(-1) ?? "";
    if (!isMechanicsKey(key)) return;
    const normalizedPointer = pointer.replace(/\/\d+(?=\/|$)/g, "/*");
    output.push({
      table,
      row_id: String(rowId),
      pointer,
      normalized_pointer: normalizedPointer,
      value: child,
      field_schema_state: fieldPathIndex.get(`${table}${normalizedPointer}`) ?? null,
    });
  }
}

function isMechanicsKey(key) {
  return /(?:damage|attack|heal|shield|hp|attr|stat|ratio|rate|percent|coefficient|param|value|duration|interval|cooldown|stack|count|limit|skill|buff|effect|factor|formula|recount|probability|chance|lucky|mastery|versatility)/i.test(key);
}

function indexExactEdges(edges) {
  const outgoing = new Map();
  const incoming = new Map();
  for (const edge of edges) {
    append(outgoing, `${edge.source_table}/${edge.source_id}`, edge);
    append(incoming, String(edge.target_id), edge);
  }
  return { outgoing, incoming };
}

function indexOccurrences(occurrences) {
  const outgoing = new Map();
  const incoming = new Map();
  for (const occurrence of occurrences) {
    append(outgoing, `${occurrence.source_table}/${occurrence.source_id}`, occurrence);
    append(incoming, String(occurrence.candidate_id), occurrence);
  }
  return { outgoing, incoming };
}

function indexFieldPaths(artifact) {
  const index = new Map();
  if (!artifact) return index;
  for (const entry of artifact.field_paths ?? artifact.fields ?? []) {
    const table = entry.table ?? entry.source_table;
    const pointer = entry.path_pattern ?? entry.normalized_pointer ?? entry.pointer;
    if (!table || !pointer) continue;
    index.set(`${table}${pointer}`, entry.semantic_state ?? entry.evidence_state ?? entry.classification ?? "inventoried");
  }
  return index;
}

function loadJsonLines(file, label) {
  const text = readFileSync(file, "utf8");
  const output = [];
  for (const [index, line] of text.split(/\r?\n/).entries()) {
    if (!line.trim()) continue;
    try { output.push(JSON.parse(line)); }
    catch (error) { throw new Error(`${label} contains invalid JSON at line ${index + 1}: ${error.message}`); }
  }
  return output;
}

function verify(input) {
  const file = absolute(input);
  requireFile(file, "dependency closure");
  const report = readJson(file, "dependency closure");
  if (report.schema_version !== 1) throw new Error("Dependency closure schema_version must be 1");
  if (report.generated_by !== "tools/bpsr-semantic-mechanic-dependency-closure.mjs") {
    throw new Error("Dependency closure generated_by is invalid");
  }
  if (!/^\d+$/.test(String(report.game_build))) throw new Error("Dependency closure game_build is invalid");
  if (!Array.isArray(report.mechanics)) throw new Error("Dependency closure mechanics must be an array");
  if (report.summary.semantic_findings !== report.mechanics.length) {
    throw new Error("Dependency closure finding count does not match mechanics array");
  }
  if (report.policy.unresolved_evidence_hidden !== false || report.summary.zero_hidden_omissions !== true) {
    throw new Error("Dependency closure must retain all unresolved evidence");
  }
  if (report.policy.raw_ctb_sources_require_exact_build_locked_identity_proof !== true) {
    throw new Error("Dependency closure must require exact CTB table identity proof");
  }
  if (report.policy.localization_keys_retained_as_external_plugin_references !== true) {
    throw new Error("Dependency closure must retain localization keys as external plugin references");
  }
  const externalReferences = report.mechanics.flatMap((entry) => entry.external_references ?? []);
  if (report.summary.external_reference_groups !== externalReferences.length) {
    throw new Error("Dependency closure external reference count is incomplete");
  }
  for (const mechanic of report.mechanics) {
    for (const reference of mechanic.external_references ?? []) {
      if (reference.kind !== "localization-key"
        || reference.status !== "retained-outside-decoded-table-namespace"
        || reference.authority !== "game-locale-assets-via-localization-plugin") {
        throw new Error(`Dependency closure external reference is invalid for ${mechanic.source_id}`);
      }
      const seed = (mechanic.seeds ?? []).find((entry) => entry.id === reference.seed_id);
      if (!seed?.roles?.includes("localization-description")) {
        throw new Error(`Dependency closure external reference has no retained localization seed for ${mechanic.source_id}`);
      }
    }
  }
  for (const mechanic of report.mechanics) {
    if (mechanic.source_table?.startsWith("CTB:")) {
      const expectedIdentity = report.ctb_table_identities?.[mechanic.source_table] ?? null;
      if (mechanic.source_table_decoded_identity !== expectedIdentity) {
        throw new Error(`Dependency closure CTB identity mismatch for ${mechanic.source_id}`);
      }
    }
  }
  const expectedAffectedTables = sortedUnique(
    report.mechanics.flatMap((entry) => [
      ...(entry.decoded_rows ?? []).map((row) => row.table),
      ...(entry.exact_reference_edges ?? []).flatMap((edge) => [edge.source_table, edge.target_table]),
    ]),
  );
  if (canonicalJson(report.affected_tables ?? []) !== canonicalJson(expectedAffectedTables)) {
    throw new Error("Dependency closure affected_tables contains non-proven relationship targets");
  }
  const expectedCandidateEvidenceTables = sortedUnique(
    report.mechanics.flatMap((entry) =>
      (entry.candidate_reference_edges ?? []).flatMap((edge) => [
        edge.source_table,
        ...(edge.full_coverage_target_tables ?? []),
      ]),
    ),
  );
  if (canonicalJson(report.candidate_evidence_tables ?? []) !== canonicalJson(expectedCandidateEvidenceTables)) {
    throw new Error("Dependency closure candidate_evidence_tables does not match retained candidate evidence");
  }
  for (const mechanic of report.mechanics) {
    if ((mechanic.decoded_rows ?? []).some((row) => row.reached_via === "candidate-not-promoted")) {
      throw new Error(`Dependency closure promoted candidate evidence for ${mechanic.source_id}`);
    }
    const mechanicAffected = sortedUnique([
      ...(mechanic.decoded_rows ?? []).map((row) => row.table),
      ...(mechanic.exact_reference_edges ?? []).flatMap((edge) => [edge.source_table, edge.target_table]),
    ]);
    if (canonicalJson(mechanic.affected_tables ?? []) !== canonicalJson(mechanicAffected)) {
      throw new Error(`Dependency closure affected_tables is not exact for ${mechanic.source_id}`);
    }
  }
  const expectedHash = canonicalHash({ ...report, content_sha256: undefined });
  if (report.content_sha256 !== expectedHash) throw new Error("Dependency closure content hash mismatch");
  console.log(`Semantic mechanic dependency closure verified: ${report.mechanics.length} findings, zero hidden omissions.`);
}

function verifyArtifact(input) {
  const artifact = readJson(absolute(input), "semantic mechanic dependency artifact");
  if (artifact.artifact_type === "semantic-mechanic-dependency-diff") verifyDiff(input);
  else verify(input);
}

function diff(args) {
  const baselinePath = absolute(required(args, "baseline"));
  const candidatePath = absolute(required(args, "candidate"));
  const outputPath = absolute(required(args, "output"));
  verify(baselinePath);
  verify(candidatePath);
  const baseline = readJson(baselinePath, "baseline dependency closure");
  const candidate = readJson(candidatePath, "candidate dependency closure");
  if (String(baseline.game_build) === String(candidate.game_build)) {
    throw new Error("Dependency closure diff requires two different game builds");
  }

  const baselineById = new Map(baseline.mechanics.map((entry) => [entry.source_id, entry]));
  const candidateById = new Map(candidate.mechanics.map((entry) => [entry.source_id, entry]));
  const added = [];
  const removed = [];
  const changed = [];
  const unchanged = [];

  for (const [sourceId, mechanic] of candidateById) {
    const prior = baselineById.get(sourceId);
    const candidateSemantic = mechanicSemanticProjection(mechanic);
    const candidateHash = canonicalHash(candidateSemantic);
    if (!prior) {
      added.push({ source_id: sourceId, semantic_sha256: candidateHash, mechanic: candidateSemantic });
      continue;
    }
    const baselineSemantic = mechanicSemanticProjection(prior);
    const baselineHash = canonicalHash(baselineSemantic);
    if (baselineHash === candidateHash) {
      unchanged.push({ source_id: sourceId, semantic_sha256: candidateHash });
      continue;
    }
    changed.push({
      source_id: sourceId,
      baseline_semantic_sha256: baselineHash,
      candidate_semantic_sha256: candidateHash,
      changed_components: changedMechanicComponents(baselineSemantic, candidateSemantic),
      baseline: baselineSemantic,
      candidate: candidateSemantic,
    });
  }
  for (const [sourceId, mechanic] of baselineById) {
    if (candidateById.has(sourceId)) continue;
    const semantic = mechanicSemanticProjection(mechanic);
    removed.push({ source_id: sourceId, semantic_sha256: canonicalHash(semantic), mechanic: semantic });
  }

  for (const rows of [added, removed, changed, unchanged]) {
    rows.sort((a, b) => a.source_id.localeCompare(b.source_id));
  }
  const changedDependencyTables = sortedUnique([
    ...added.flatMap((entry) => entry.mechanic.affected_tables),
    ...removed.flatMap((entry) => entry.mechanic.affected_tables),
    ...changed.flatMap((entry) => [
      ...entry.baseline.affected_tables,
      ...entry.candidate.affected_tables,
    ]),
  ]);
  const report = {
    schema_version: 1,
    artifact_type: "semantic-mechanic-dependency-diff",
    generated_by: "tools/bpsr-semantic-mechanic-dependency-closure.mjs",
    baseline_build: String(baseline.game_build),
    candidate_build: String(candidate.game_build),
    inputs: {
      baseline: describeFile(baselinePath),
      candidate: describeFile(candidatePath),
    },
    policy: {
      steam_manifest_is_physical_change_locator_only: true,
      unchanged_mechanics_require_static_regeneration: false,
      changed_mechanics_require_focused_regeneration_and_reproof: true,
      unresolved_evidence_hidden: false,
      removed_mechanics_retained_as_history: true,
    },
    summary: {
      baseline_mechanics: baseline.mechanics.length,
      candidate_mechanics: candidate.mechanics.length,
      added: added.length,
      removed: removed.length,
      changed: changed.length,
      unchanged: unchanged.length,
      changed_dependency_tables: changedDependencyTables.length,
      zero_hidden_omissions: true,
    },
    changed_dependency_tables: changedDependencyTables,
    added,
    removed,
    changed,
    unchanged,
  };
  report.content_sha256 = canonicalHash({ ...report, content_sha256: undefined });
  mkdirSync(path.dirname(outputPath), { recursive: true });
  writeFileSync(outputPath, `${JSON.stringify(report, null, 2)}\n`, "utf8");
  verifyDiff(outputPath);
  console.log(
    `Semantic mechanic dependency diff ${report.baseline_build} -> ${report.candidate_build}: ` +
      `${added.length} added, ${removed.length} removed, ${changed.length} changed, ${unchanged.length} unchanged.`,
  );
}

function mechanicSemanticProjection(mechanic) {
  return {
    source_id: mechanic.source_id,
    source_name: mechanic.source_name ?? null,
    source_rule_id: mechanic.source_rule_id ?? null,
    effect_source_present: mechanic.effect_source_present === true,
    source_kind: mechanic.source_kind ?? null,
    source_type: mechanic.source_type ?? null,
    source_table: mechanic.source_table ?? null,
    source_table_decoded_identity: mechanic.source_table_decoded_identity ?? null,
    promotion_blocked: mechanic.promotion_blocked === true,
    issue_categories: mechanic.issue_categories ?? [],
    seeds: mechanic.seeds ?? [],
    affected_tables: mechanic.affected_tables ?? [],
    candidate_evidence_tables: mechanic.candidate_evidence_tables ?? [],
    incoming_evidence_tables: mechanic.incoming_evidence_tables ?? [],
    decoded_rows: mechanic.decoded_rows ?? [],
    exact_reference_edges: mechanic.exact_reference_edges ?? [],
    candidate_reference_edges: mechanic.candidate_reference_edges ?? [],
    incoming_reference_evidence: mechanic.incoming_reference_evidence ?? [],
    mechanics_sensitive_fields: mechanic.mechanics_sensitive_fields ?? [],
    external_references: mechanic.external_references ?? [],
    unresolved_dependencies: mechanic.unresolved_dependencies ?? [],
    static_resolution_state: mechanic.static_resolution_state ?? null,
    current_build_runtime_proof_state: mechanic.current_build_runtime_proof_state ?? null,
  };
}

function changedMechanicComponents(baseline, candidate) {
  return Object.keys(candidate)
    .filter((key) => canonicalJson(baseline[key]) !== canonicalJson(candidate[key]))
    .sort();
}

function verifyDiff(input) {
  const file = absolute(input);
  requireFile(file, "dependency closure diff");
  const report = readJson(file, "dependency closure diff");
  if (report.schema_version !== 1 || report.artifact_type !== "semantic-mechanic-dependency-diff") {
    throw new Error("Dependency closure diff schema or artifact type is invalid");
  }
  if (report.generated_by !== "tools/bpsr-semantic-mechanic-dependency-closure.mjs") {
    throw new Error("Dependency closure diff generated_by is invalid");
  }
  for (const key of ["baseline_build", "candidate_build"]) {
    if (!/^\d+$/.test(String(report[key]))) throw new Error(`Dependency closure diff ${key} is invalid`);
  }
  const all = [report.added, report.removed, report.changed, report.unchanged];
  if (all.some((rows) => !Array.isArray(rows))) throw new Error("Dependency closure diff rows are incomplete");
  const seen = new Set();
  for (const rows of all) {
    for (const entry of rows) {
      if (!entry.source_id || seen.has(entry.source_id)) {
        throw new Error(`Dependency closure diff has a missing or duplicate source ID: ${entry.source_id ?? "missing"}`);
      }
      seen.add(entry.source_id);
    }
  }
  if (report.summary.added !== report.added.length
    || report.summary.removed !== report.removed.length
    || report.summary.changed !== report.changed.length
    || report.summary.unchanged !== report.unchanged.length
    || report.summary.zero_hidden_omissions !== true
    || seen.size !== report.summary.baseline_mechanics + report.summary.added
    || seen.size !== report.summary.candidate_mechanics + report.summary.removed) {
    throw new Error("Dependency closure diff summary does not conserve every mechanic");
  }
  const expectedTables = sortedUnique([
    ...report.added.flatMap((entry) => entry.mechanic.affected_tables),
    ...report.removed.flatMap((entry) => entry.mechanic.affected_tables),
    ...report.changed.flatMap((entry) => [...entry.baseline.affected_tables, ...entry.candidate.affected_tables]),
  ]);
  if (canonicalJson(report.changed_dependency_tables) !== canonicalJson(expectedTables)) {
    throw new Error("Dependency closure diff changed dependency tables are incomplete");
  }
  const expectedHash = canonicalHash({ ...report, content_sha256: undefined });
  if (report.content_sha256 !== expectedHash) throw new Error("Dependency closure diff content hash mismatch");
  console.log(`Semantic mechanic dependency diff verified: ${seen.size} conserved source IDs.`);
}

function selfTest() {
  const finding = {
    source_id: "buff-source:42",
    target_damage_ids: [1001],
    issues: [{ category: "formula-magnitude-unresolved" }],
  };
  const effect = { sourceEntityId: 42, buffIds: [42], evidence: [{ ownerSkillId: 8 }] };
  const seeds = collectSeeds(finding, effect);
  const ids = seeds.map((entry) => entry.id);
  for (const expected of ["8", "42", "1001"]) {
    if (!ids.includes(expected)) throw new Error(`Self-test did not retain seed ${expected}`);
  }
  if (numericId("10%") !== null) throw new Error("Self-test accepted a percentage as an identifier");
  if (!isMechanicsKey("DamageRate") || isMechanicsKey("DisplayName")) {
    throw new Error("Self-test mechanics-field classifier failed");
  }
  const namespaceTables = effectSourceNamespaceTables(
    { id: "3948" },
    { evidence: [{ ownerSourceId: 3948, ownerNameSource: "skill_aoyi_icons" }] },
    ["BuffTable", "SkillAoyiTable"],
  );
  if (canonicalJson(namespaceTables) !== canonicalJson(["SkillAoyiTable"])) {
    throw new Error("Self-test exact effect-source namespace resolution failed");
  }
  const baselineMechanic = mechanicSemanticProjection({
    source_id: "buff-source:42",
    affected_tables: ["BuffTable"],
    decoded_rows: [{ table: "BuffTable", row_id: "42", row_sha256: "old" }],
  });
  const candidateMechanic = mechanicSemanticProjection({
    source_id: "buff-source:42",
    affected_tables: ["BuffTable"],
    decoded_rows: [{ table: "BuffTable", row_id: "42", row_sha256: "new" }],
  });
  const changed = changedMechanicComponents(baselineMechanic, candidateMechanic);
  if (canonicalJson(changed) !== canonicalJson(["decoded_rows"])) {
    throw new Error(`Self-test mechanic diff classifier failed: ${changed.join(", ")}`);
  }
  console.log("Semantic mechanic dependency closure self-test passed.");
}

function edgeKey(edge) {
  return [edge.source_table, edge.source_id, edge.source_pointer, edge.relationship, edge.target_table, edge.target_id].join("|");
}
function candidateKey(edge) {
  return [edge.source_table, edge.source_id, edge.source_pointer, edge.candidate_id].join("|");
}
function incomingKey(edge) {
  return [edge.evidence_kind, edge.source_table, edge.source_id, edge.source_pointer, edge.target_id].join("|");
}
function append(map, key, value) {
  const values = map.get(key) ?? [];
  values.push(value);
  map.set(key, values);
}
function dedupe(values, keyFn) {
  const output = [];
  const seen = new Set();
  for (const value of values) {
    const key = keyFn(value);
    if (seen.has(key)) continue;
    seen.add(key);
    output.push(value);
  }
  return output;
}
function numericId(value) {
  const text = String(value ?? "");
  return /^[1-9]\d*$/.test(text) ? text : null;
}
function compareNumericStrings(a, b) {
  if (a.length !== b.length) return a.length - b.length;
  return a.localeCompare(b);
}
function sortedUnique(values) {
  return [...new Set(values.filter((value) => value !== null && value !== undefined))].sort((a, b) => String(a).localeCompare(String(b)));
}
function canonicalHash(value) {
  return createHash("sha256").update(canonicalJson(value)).digest("hex");
}
function canonicalJson(value) {
  if (Array.isArray(value)) return `[${value.map(canonicalJson).join(",")}]`;
  if (value && typeof value === "object") {
    return `{${Object.keys(value).filter((key) => value[key] !== undefined).sort().map((key) => `${JSON.stringify(key)}:${canonicalJson(value[key])}`).join(",")}}`;
  }
  return JSON.stringify(value);
}
function describeFile(file) {
  return { path: normalize(file), bytes: statSync(file).size, sha256: sha256(file) };
}
function sha256(file) {
  return createHash("sha256").update(readFileSync(file)).digest("hex");
}
function readJson(file, label) {
  try { return JSON.parse(readFileSync(file, "utf8")); }
  catch (error) { throw new Error(`Unable to read ${label} ${file}: ${error.message}`); }
}
function requireBuild(artifact, build, label) {
  const actual = String(artifact.game_build ?? artifact.current_game_build ?? "");
  if (actual !== String(build)) throw new Error(`${label} build ${actual || "missing"} does not match ${build}`);
}
function requireFile(file, label) {
  if (!existsSync(file) || !statSync(file).isFile()) throw new Error(`Missing ${label}: ${file}`);
}
function parseArgs(args) {
  const output = {};
  for (let index = 0; index < args.length; index += 1) {
    const token = args[index];
    if (!token.startsWith("--")) throw new Error(`Unexpected argument: ${token}`);
    const key = token.slice(2);
    const value = args[index + 1];
    if (value === undefined || value.startsWith("--")) throw new Error(`Missing value for --${key}`);
    output[key] = value;
    index += 1;
  }
  return output;
}
function required(args, key) {
  const value = args[key];
  if (!value) throw new Error(`Missing required --${key}`);
  return value;
}
function absolute(value) { return path.resolve(value); }
function normalize(value) { return value.replaceAll("\\", "/"); }
function escapePointer(value) { return value.replaceAll("~", "~0").replaceAll("/", "~1"); }
function usage(exitCode) {
  console.log(`Usage:
  node tools/bpsr-semantic-mechanic-dependency-closure.mjs generate --build BUILD --semantic-audit FILE --effect-sources FILE --decoded-root DIR --reference-graph FILE --reference-occurrences FILE --reference-candidates FILE --ctb-table-identities FILE --decoded-field-schema FILE --output FILE
  node tools/bpsr-semantic-mechanic-dependency-closure.mjs diff --baseline FILE --candidate FILE --output FILE
  node tools/bpsr-semantic-mechanic-dependency-closure.mjs verify --input FILE
  node tools/bpsr-semantic-mechanic-dependency-closure.mjs self-test`);
  process.exit(exitCode);
}
