#!/usr/bin/env node

import { execFileSync } from "node:child_process";
import { createHash } from "node:crypto";
import {
  copyFileSync,
  existsSync,
  lstatSync,
  mkdtempSync,
  mkdirSync,
  readdirSync,
  readFileSync,
  rmSync,
  statSync,
  writeFileSync,
} from "node:fs";
import { tmpdir } from "node:os";
import path from "node:path";
import { fileURLToPath } from "node:url";

const scriptDir = path.dirname(fileURLToPath(import.meta.url));
const repoRoot = path.resolve(scriptDir, "..");
const [command = "help", ...rest] = process.argv.slice(2);
const options = parseArgs(rest);

if (command === "verify") verify(resolveContext(options));
else if (command === "refresh") refresh(resolveContext(options), false);
else if (command === "rebuild") refresh(resolveContext(options), true);
else if (command === "self-test") selfTest();
else usage(command === "help" ? 0 : 1);

function resolveContext(options) {
  const configPath = resolvePath(required(options, "config"));
  const config = readJson(configPath, "semantic refresh configuration");
  if (config.schema_version !== 1) throw new Error("Semantic refresh config schema_version must be 1");
  const build = required(options, "build");
  if (!/^\d+$/.test(build)) throw new Error("Build must contain only ASCII digits");
  const variables = {
    repo_root: repoRoot,
    build,
    build_root: resolvePath(required(options, "build-root")),
    extractor_root: resolvePath(required(options, "extractor-root")),
    decoded_root: resolvePath(required(options, "decoded-root")),
  };
  return {
    build,
    config,
    configPath,
    variables,
    paths: expandObject(config.paths, variables),
  };
}

function verify(context) {
  const { build, paths } = context;
  const inputs = collectInputs(paths);
  for (const [label, file] of inputs) requireFile(file, label);
  requireBuild(paths.severed_chapter_proof, build, "Severed Chapter proof", ["current_game_build"]);
  requireBuild(paths.battle_cry_proof, build, "Battle Cry proof", ["current_game_build"]);
  requireBuild(paths.denvel_proof, build, "Denvel proof", ["current_game_build"]);
  requireBuild(paths.focused_shot_proof, build, "Focused Shot proof", ["current_game_build"]);
  requireBuild(paths.stellar_spark_proof, build, "Stellar Spark proof", ["current_game_build"]);
  requireGeneratedBuild(paths.decoded_reference_graph, build, "DecodedTableReferenceGraph.gen");
  verifyReferenceCandidateArtifact(
    paths.decoded_reference_graph,
    paths.decoded_reference_candidates,
  );
  requireGeneratedBuild(
    paths.semantic_field_schema,
    build,
    "tools/bpsr-semantic-field-schema-ledger.mjs",
  );
  execFileSync(process.execPath, [
    path.join(scriptDir, "bpsr-semantic-field-schema-ledger.mjs"),
    "verify",
    "--input", paths.semantic_field_schema,
  ], { cwd: repoRoot, stdio: "inherit" });
  requireGeneratedBuild(
    paths.decoded_field_schema,
    build,
    "tools/bpsr-decoded-field-schema-manifest.mjs",
  );
  execFileSync(process.execPath, [
    path.join(scriptDir, "bpsr-decoded-field-schema-manifest.mjs"),
    "verify",
    "--input", paths.decoded_field_schema,
  ], { cwd: repoRoot, stdio: "inherit" });
  verifyReferenceOccurrenceArtifact(
    paths.decoded_reference_graph,
    paths.decoded_reference_occurrences,
  );
  console.log(`Semantic refresh inputs verified for build ${build}: ${inputs.length} explicit inputs, zero hidden omissions.`);
}

function refresh(context, force) {
  verify(context);
  const { build, paths } = context;
  const outputFiles = [
    paths.current_origin_ledger,
    paths.static_worklist,
    paths.magnitude_watchlist,
    paths.semantic_audit,
    paths.ctb_table_identity_map,
    paths.semantic_dependency_closure,
    paths.decoded_expression_reference_edges,
    paths.semantic_evidence_index,
    paths.produced_damage_proof_routes,
    paths.semantic_field_adjudications,
    paths.proof_frontier_workbench,
    paths.formula_gap_ledger,
    paths.formula_gap_watchlist,
    paths.static_formula_evidence,
    paths.primary_attack_runtime_route_proof,
    paths.mastery_runtime_route_proof,
    paths.source_hp_runtime_route_proof,
    paths.selected_factor_runtime_route_proof,
    paths.psychoscope_factor_offline_closure,
    paths.selected_factor_mechanic_route_proof,
    paths.selected_factor_capture_correlation_proof,
    paths.factor_capture_correlation_proof,
    paths.dreamscope_runtime_fingerprint_proof,
    paths.historical_factor_route_stability_proof,
    paths.all_element_fixed_point_family_proof,
    paths.shared_formula_proof_registry,
    paths.formula_model_workbench,
    paths.critical_event_state_route_proof,
    paths.recipient_scope_ledger,
    paths.proof_frontier_router,
    paths.semantic_resolution_batches,
    paths.proof_attempt_ledger,
    paths.proof_correlation_manifest,
    paths.runtime_effect_component_routing_proof,
    paths.protocol_status,
    paths.rdps_proof_closure,
    paths.deferred_attribution_ledger,
    paths.preflight,
    paths.refresh_report,
  ];
  const cacheFile = path.join(paths.build_root, "semantic-refresh-cache.v1.json");
  const backupRoot = path.join(paths.build_root, ".semantic-refresh-backup");
  rmSync(backupRoot, { recursive: true, force: true });
  mkdirSync(backupRoot, { recursive: true });
  const backups = backupOutputs([...outputFiles, cacheFile], backupRoot);
  const cache = force ? emptyCache(build) : loadCache(cacheFile, build);
  const stageResults = [];

  try {
    const originDecodedInputs = [
      "SkillAoyiTable.json",
      "SkillTable.json",
      "BuffTable.json",
      "MonsterTable.json",
      "SkillEffectTable.json",
      "DamageAttrTable.json",
      "AttrDescription.json",
      "FightAttrTable.json",
      "SkillAoyiStarTable.json",
    ].map((name) => path.join(context.variables.decoded_root, name));
    runCachedStage(cache, stageResults, {
      id: "current-aoyi-origin-ledger",
      inputs: [
        ...originDecodedInputs,
        paths.skill_aoyi_icons,
        paths.source_index,
        paths.modifier_relationship_table,
        paths.skill_damage_chain_bridge,
        paths.effect_sources,
        paths.historical_observed_status_origins,
        paths.buff_names,
        paths.aoyi_remodel_proof,
        paths.component_packet_proof,
      ],
      tools: rustToolFiles("current-aoyi-origin-ledger.rs"),
      outputs: [paths.current_origin_ledger],
      command: ["cargo", "rlogs-bpsr-current-aoyi-origin-ledger", build],
      run: () => runCargo("rlogs-bpsr-current-aoyi-origin-ledger", [
        context.variables.decoded_root,
        paths.skill_aoyi_icons,
        paths.source_index,
        paths.modifier_relationship_table,
        paths.skill_damage_chain_bridge,
        paths.effect_sources,
        paths.historical_observed_status_origins,
        paths.buff_names,
        paths.aoyi_remodel_proof,
        paths.component_packet_proof,
        paths.current_origin_ledger,
        build,
      ]),
    }, force);
    requireBuild(paths.current_origin_ledger, build, "current origin ledger", ["game_build"]);

    runCachedStage(cache, stageResults, {
      id: "static-rdps-worklist",
      inputs: [paths.classification, paths.contribution, paths.recount, paths.value_proof, paths.buff_table],
      tools: rustToolFiles("static-rdps-worklist.rs"),
      outputs: [paths.static_worklist, paths.magnitude_watchlist],
      command: ["cargo", "rlogs-bpsr-static-rdps-worklist", build],
      run: () => runCargo("rlogs-bpsr-static-rdps-worklist", [
        "--classification", paths.classification,
        "--contribution", paths.contribution,
        "--recount", paths.recount,
        "--value-proof", paths.value_proof,
        "--build", build,
        "--output", paths.static_worklist,
        "--watchlist-output", paths.magnitude_watchlist,
        "--buff-table", paths.buff_table,
      ]),
    }, force);
    requireGeneratedBuild(paths.static_worklist, build, "rlogs-bpsr-static-rdps-worklist");
    requireGeneratedBuild(paths.magnitude_watchlist, build, "rlogs-bpsr-static-rdps-worklist");

    runCachedStage(cache, stageResults, {
      id: "static-rdps-semantic-audit",
      inputs: [paths.static_worklist, paths.effect_sources],
      tools: rustToolFiles("static-rdps-semantic-audit.rs"),
      outputs: [paths.semantic_audit],
      command: ["cargo", "rlogs-bpsr-static-rdps-semantic-audit", build],
      run: () => runCargo("rlogs-bpsr-static-rdps-semantic-audit", [
        "--worklist", paths.static_worklist,
        "--effect-sources", paths.effect_sources,
        "--build", build,
        "--output", paths.semantic_audit,
      ]),
    }, force);
    requireGeneratedBuild(paths.semantic_audit, build, "rlogs-bpsr-static-rdps-semantic-audit");

    const ctbTool = path.join(scriptDir, "bpsr-ctb-table-identity-map.mjs");
    runCachedStage(cache, stageResults, {
      id: "ctb-table-identity-map",
      inputs: [
        paths.season_rogue_probe,
        paths.talent_effect_probe,
        path.join(context.variables.decoded_root, "RogueEntryTable.json"),
        path.join(context.variables.decoded_root, "TalentTable.json"),
      ],
      tools: [ctbTool],
      outputs: [paths.ctb_table_identity_map],
      command: ["node", "bpsr-ctb-table-identity-map.mjs", build],
      run: () => execFileSync(process.execPath, [
        ctbTool,
        "generate",
        "--build", build,
        "--decoded-root", context.variables.decoded_root,
        "--season-rogue-probe", paths.season_rogue_probe,
        "--talent-probe", paths.talent_effect_probe,
        "--output", paths.ctb_table_identity_map,
      ], { cwd: repoRoot, stdio: "inherit" }),
    }, force);
    requireGeneratedBuild(
      paths.ctb_table_identity_map,
      build,
      "tools/bpsr-ctb-table-identity-map.mjs",
    );
    execFileSync(process.execPath, [
      path.join(scriptDir, "bpsr-ctb-table-identity-map.mjs"),
      "verify",
      "--input", paths.ctb_table_identity_map,
    ], { cwd: repoRoot, stdio: "inherit" });

    const closureTool = path.join(scriptDir, "bpsr-semantic-mechanic-dependency-closure.mjs");
    runCachedStage(cache, stageResults, {
      id: "semantic-mechanic-dependency-closure",
      inputs: [
        paths.semantic_audit,
        paths.effect_sources,
        paths.decoded_reference_graph,
        paths.decoded_reference_occurrences,
        paths.decoded_reference_candidates,
        paths.decoded_field_schema,
        paths.ctb_table_identity_map,
      ],
      tools: [closureTool],
      outputs: [paths.semantic_dependency_closure],
      command: ["node", "bpsr-semantic-mechanic-dependency-closure.mjs", build],
      run: () => execFileSync(process.execPath, [
        closureTool,
        "generate",
        "--build", build,
        "--semantic-audit", paths.semantic_audit,
        "--effect-sources", paths.effect_sources,
        "--decoded-root", context.variables.decoded_root,
        "--reference-graph", paths.decoded_reference_graph,
        "--reference-occurrences", paths.decoded_reference_occurrences,
        "--reference-candidates", paths.decoded_reference_candidates,
        "--decoded-field-schema", paths.decoded_field_schema,
        "--ctb-table-identities", paths.ctb_table_identity_map,
        "--output", paths.semantic_dependency_closure,
      ], { cwd: repoRoot, stdio: "inherit" }),
    }, force);
    requireGeneratedBuild(
      paths.semantic_dependency_closure,
      build,
      "tools/bpsr-semantic-mechanic-dependency-closure.mjs",
    );
    execFileSync(process.execPath, [
      path.join(scriptDir, "bpsr-semantic-mechanic-dependency-closure.mjs"),
      "verify",
      "--input", paths.semantic_dependency_closure,
    ], { cwd: repoRoot, stdio: "inherit" });

    const expressionEdgeTool = path.join(scriptDir, "bpsr-decoded-expression-reference-edges.mjs");
    runCachedStage(cache, stageResults, {
      id: "decoded-expression-reference-edges",
      inputs: [
        paths.decoded_reference_graph,
        paths.decoded_expression_reference_rules,
        path.join(context.variables.decoded_root, "SkillEffectTable.json"),
        path.join(context.variables.decoded_root, "DamageAttrTable.json"),
      ],
      tools: [expressionEdgeTool],
      outputs: [paths.decoded_expression_reference_edges],
      command: ["node", "bpsr-decoded-expression-reference-edges.mjs", "build", build],
      run: () => execFileSync(process.execPath, [
        expressionEdgeTool,
        "build",
        "--build", build,
        "--decoded-root", context.variables.decoded_root,
        "--reference-graph", paths.decoded_reference_graph,
        "--rules", paths.decoded_expression_reference_rules,
        "--output", paths.decoded_expression_reference_edges,
      ], { cwd: repoRoot, stdio: "inherit" }),
    }, force);
    requireGeneratedBuild(
      paths.decoded_expression_reference_edges,
      build,
      "tools/bpsr-decoded-expression-reference-edges.mjs",
    );
    execFileSync(process.execPath, [
      expressionEdgeTool,
      "verify",
      "--input", paths.decoded_expression_reference_edges,
    ], { cwd: repoRoot, stdio: "inherit" });

    const evidenceIndexTool = path.join(scriptDir, "bpsr-semantic-evidence-index.mjs");
    const referenceGraph = readJson(paths.decoded_reference_graph, "decoded reference graph");
    const decodedTableFiles = (referenceGraph.tables ?? []).map((table) =>
      path.join(context.variables.decoded_root, table.file));
    runCachedStage(cache, stageResults, {
      id: "semantic-evidence-index",
      inputs: [
        paths.decoded_reference_graph,
        paths.decoded_reference_occurrences,
        paths.decoded_reference_candidates,
        paths.decoded_expression_reference_edges,
        paths.semantic_dependency_closure,
        paths.semantic_audit,
        ...decodedTableFiles,
      ],
      tools: [evidenceIndexTool],
      outputs: [paths.semantic_evidence_index],
      command: ["node", "bpsr-semantic-evidence-index.mjs", "build", build],
      run: () => execFileSync(process.execPath, [
        evidenceIndexTool,
        "build",
        "--build", build,
        "--decoded-root", context.variables.decoded_root,
        "--reference-graph", paths.decoded_reference_graph,
        "--reference-occurrences", paths.decoded_reference_occurrences,
        "--reference-candidates", paths.decoded_reference_candidates,
        "--expression-edges", paths.decoded_expression_reference_edges,
        "--semantic-closure", paths.semantic_dependency_closure,
        "--semantic-audit", paths.semantic_audit,
        "--output", paths.semantic_evidence_index,
      ], {
        cwd: repoRoot,
        stdio: "inherit",
        env: { ...process.env, NODE_NO_WARNINGS: "1" },
      }),
    }, force);
    execFileSync(process.execPath, [
      evidenceIndexTool,
      "verify",
      "--input", paths.semantic_evidence_index,
      "--verify-sources", "false",
    ], {
      cwd: repoRoot,
      stdio: "inherit",
      env: { ...process.env, NODE_NO_WARNINGS: "1" },
    });

    const producedDamageRouteTool = path.join(scriptDir, "bpsr-produced-damage-proof-routes.mjs");
    runCachedStage(cache, stageResults, {
      id: "produced-damage-proof-routes",
      inputs: [
        paths.semantic_evidence_index,
        paths.semantic_dependency_closure,
        paths.effect_sources,
        paths.battle_imagine_descriptions,
        paths.runtime_effect_origin_catalog,
        paths.exact_recount_table,
        paths.damage_attr_activation_index,
        paths.number_format_semantics,
      ],
      tools: [producedDamageRouteTool],
      outputs: [paths.produced_damage_proof_routes],
      command: ["node", "bpsr-produced-damage-proof-routes.mjs", "build", build],
      run: () => execFileSync(process.execPath, [
        producedDamageRouteTool,
        "build",
        "--build", build,
        "--index", paths.semantic_evidence_index,
        "--semantic-closure", paths.semantic_dependency_closure,
        "--effect-sources", paths.effect_sources,
        "--battle-imagines", paths.battle_imagine_descriptions,
        "--origin-catalog", paths.runtime_effect_origin_catalog,
        "--recount", paths.exact_recount_table,
        "--activation-index", paths.damage_attr_activation_index,
        "--number-semantics", paths.number_format_semantics,
        "--output", paths.produced_damage_proof_routes,
      ], {
        cwd: repoRoot,
        stdio: "inherit",
        env: { ...process.env, NODE_NO_WARNINGS: "1" },
      }),
    }, force);
    execFileSync(process.execPath, [
      producedDamageRouteTool,
      "verify",
      "--input", paths.produced_damage_proof_routes,
      "--index", paths.semantic_evidence_index,
      "--activation-index", paths.damage_attr_activation_index,
    ], {
      cwd: repoRoot,
      stdio: "inherit",
      env: { ...process.env, NODE_NO_WARNINGS: "1" },
    });

    const fieldAdjudicationTool = path.join(scriptDir, "bpsr-semantic-field-adjudications.mjs");
    runCachedStage(cache, stageResults, {
      id: "semantic-field-adjudications",
      inputs: [
        paths.semantic_field_schema,
        paths.semantic_field_adjudication_rules,
        path.join(context.variables.decoded_root, "RogueEntryTable.json"),
        path.join(context.variables.decoded_root, "RogueSceneTable.json"),
      ],
      tools: [fieldAdjudicationTool],
      outputs: [paths.semantic_field_adjudications],
      command: ["node", "bpsr-semantic-field-adjudications.mjs", "build", build],
      run: () => execFileSync(process.execPath, [
        fieldAdjudicationTool,
        "build",
        "--build", build,
        "--semantic-schema", paths.semantic_field_schema,
        "--decoded-root", context.variables.decoded_root,
        "--rules", paths.semantic_field_adjudication_rules,
        "--output", paths.semantic_field_adjudications,
      ], { cwd: repoRoot, stdio: "inherit" }),
    }, force);
    execFileSync(process.execPath, [
      fieldAdjudicationTool,
      "verify",
      "--input", paths.semantic_field_adjudications,
    ], { cwd: repoRoot, stdio: "inherit" });

    const proofFrontierTool = path.join(scriptDir, "bpsr-proof-frontier-workbench.mjs");
    runCachedStage(cache, stageResults, {
      id: "proof-frontier-workbench",
      inputs: [paths.semantic_evidence_index, paths.produced_damage_proof_routes, paths.semantic_field_adjudications],
      tools: [proofFrontierTool],
      outputs: [paths.proof_frontier_workbench],
      command: ["node", "bpsr-proof-frontier-workbench.mjs", "build", build, "5"],
      run: () => execFileSync(process.execPath, [
        proofFrontierTool,
        "build",
        "--build", build,
        "--index", paths.semantic_evidence_index,
        "--routes", paths.produced_damage_proof_routes,
        "--field-adjudications", paths.semantic_field_adjudications,
        "--output", paths.proof_frontier_workbench,
        "--depth", "5",
      ], {
        cwd: repoRoot,
        stdio: "inherit",
        env: { ...process.env, NODE_NO_WARNINGS: "1" },
      }),
    }, force);
    execFileSync(process.execPath, [
      proofFrontierTool,
      "verify",
      "--input", paths.proof_frontier_workbench,
      "--index", paths.semantic_evidence_index,
    ], {
      cwd: repoRoot,
      stdio: "inherit",
      env: { ...process.env, NODE_NO_WARNINGS: "1" },
    });

    runCachedStage(cache, stageResults, {
      id: "rdps-formula-gap-ledger",
      inputs: [paths.semantic_audit, paths.magnitude_watchlist, paths.current_origin_ledger, paths.source_index, paths.packet_proof, paths.retained_proofs],
      tools: rustToolFiles("rdps-formula-gap-ledger.rs"),
      outputs: [paths.formula_gap_ledger, paths.formula_gap_watchlist],
      command: ["cargo", "rlogs-bpsr-rdps-formula-gap-ledger", build, String(context.config.historical_packet_build)],
      run: () => runCargo("rlogs-bpsr-rdps-formula-gap-ledger", [
        "--semantic-audit", paths.semantic_audit,
        "--watchlist", paths.magnitude_watchlist,
        "--origin-ledger", paths.current_origin_ledger,
        "--source-index", paths.source_index,
        "--packet-proof", paths.packet_proof,
        "--retained-proofs", paths.retained_proofs,
        "--discovery-build", String(context.config.historical_packet_build),
        "--output", paths.formula_gap_ledger,
        "--gap-watchlist-output", paths.formula_gap_watchlist,
      ]),
    }, force);
    requireGeneratedBuild(paths.formula_gap_ledger, build, "rlogs-bpsr-rdps-formula-gap-ledger");

    const staticFormulaTool = path.join(scriptDir, "bpsr-static-formula-evidence.mjs");
    runCachedStage(cache, stageResults, {
      id: "static-formula-evidence",
      inputs: [paths.formula_gap_ledger, paths.semantic_evidence_index],
      tools: [staticFormulaTool],
      outputs: [paths.static_formula_evidence],
      command: ["node", "bpsr-static-formula-evidence.mjs", "build", build],
      run: () => execFileSync(process.execPath, [
        staticFormulaTool,
        "build",
        "--build", build,
        "--formula-ledger", paths.formula_gap_ledger,
        "--index", paths.semantic_evidence_index,
        "--output", paths.static_formula_evidence,
      ], {
        cwd: repoRoot,
        stdio: "inherit",
        env: { ...process.env, NODE_NO_WARNINGS: "1" },
      }),
    }, force);
    requireGeneratedBuild(paths.static_formula_evidence, build, "tools/bpsr-static-formula-evidence.mjs");
    execFileSync(process.execPath, [
      staticFormulaTool,
      "verify",
      "--input", paths.static_formula_evidence,
      "--index", paths.semantic_evidence_index,
    ], { cwd: repoRoot, stdio: "inherit", env: { ...process.env, NODE_NO_WARNINGS: "1" } });

    const primaryAttackRuntimeRouteProofTool = path.join(scriptDir, "bpsr-primary-attack-runtime-route-proof.mjs");
    runCachedStage(cache, stageResults, {
      id: "primary-attack-runtime-route-proof",
      inputs: [
        paths.static_formula_evidence,
        paths.static_worklist,
        paths.damage_stage,
        paths.primary_stat_attack_transform_proof,
        paths.events_source,
        paths.decoder_source,
        paths.state_rdps_source,
        paths.damage_stage_source,
      ],
      tools: [primaryAttackRuntimeRouteProofTool],
      outputs: [paths.primary_attack_runtime_route_proof],
      command: ["node", "bpsr-primary-attack-runtime-route-proof.mjs", "build", build],
      run: () => execFileSync(process.execPath, [
        primaryAttackRuntimeRouteProofTool,
        "build",
        "--build", build,
        "--static-formula-evidence", paths.static_formula_evidence,
        "--worklist", paths.static_worklist,
        "--damage-stage", paths.damage_stage,
        "--primary-stat-attack-proof", paths.primary_stat_attack_transform_proof,
        "--events-source", paths.events_source,
        "--decoder-source", paths.decoder_source,
        "--reducer-source", paths.state_rdps_source,
        "--damage-stage-source", paths.damage_stage_source,
        "--output", paths.primary_attack_runtime_route_proof,
      ], { cwd: repoRoot, stdio: "inherit", env: { ...process.env, NODE_NO_WARNINGS: "1" } }),
    }, force);
    execFileSync(process.execPath, [
      primaryAttackRuntimeRouteProofTool,
      "verify",
      "--input", paths.primary_attack_runtime_route_proof,
    ], { cwd: repoRoot, stdio: "inherit" });

    const masteryRuntimeRouteProofTool = path.join(scriptDir, "bpsr-mastery-runtime-route-proof.mjs");
    runCachedStage(cache, stageResults, {
      id: "mastery-runtime-route-proof",
      inputs: [
        paths.fight_attr_table,
        paths.complete_build_source_manifest,
        paths.fight_attribute_transform_proof,
        paths.static_formula_evidence,
        paths.events_source,
        paths.decoder_source,
        paths.state_rdps_source,
        paths.rdps_runtime_source,
        paths.rdps_runtime_config,
      ],
      tools: [masteryRuntimeRouteProofTool],
      outputs: [paths.mastery_runtime_route_proof],
      command: ["node", "bpsr-mastery-runtime-route-proof.mjs", "build", build],
      run: () => execFileSync(process.execPath, [
        masteryRuntimeRouteProofTool,
        "build",
        "--build", build,
        "--fight-attr-table", paths.fight_attr_table,
        "--source-manifest", paths.complete_build_source_manifest,
        "--fight-attribute-proof", paths.fight_attribute_transform_proof,
        "--static-formula-evidence", paths.static_formula_evidence,
        "--events-source", paths.events_source,
        "--decoder-source", paths.decoder_source,
        "--reducer-source", paths.state_rdps_source,
        "--runtime-source", paths.rdps_runtime_source,
        "--runtime-config", paths.rdps_runtime_config,
        "--output", paths.mastery_runtime_route_proof,
      ], { cwd: repoRoot, stdio: "inherit", env: { ...process.env, NODE_NO_WARNINGS: "1" } }),
    }, force);
    execFileSync(process.execPath, [
      masteryRuntimeRouteProofTool,
      "verify",
      "--input", paths.mastery_runtime_route_proof,
    ], { cwd: repoRoot, stdio: "inherit" });

    const sourceHpRuntimeRouteProofTool = path.join(scriptDir, "bpsr-source-hp-runtime-route-proof.mjs");
    runCachedStage(cache, stageResults, {
      id: "source-hp-runtime-route-proof",
      inputs: [
        paths.fight_attr_table,
        paths.complete_build_source_manifest,
        paths.static_formula_evidence,
        paths.events_source,
        paths.decoder_source,
        paths.state_rdps_source,
        paths.state_formula_source,
      ],
      tools: [sourceHpRuntimeRouteProofTool],
      outputs: [paths.source_hp_runtime_route_proof],
      command: ["node", "bpsr-source-hp-runtime-route-proof.mjs", "build", build],
      run: () => execFileSync(process.execPath, [
        sourceHpRuntimeRouteProofTool,
        "build",
        "--build", build,
        "--fight-attr-table", paths.fight_attr_table,
        "--source-manifest", paths.complete_build_source_manifest,
        "--static-formula-evidence", paths.static_formula_evidence,
        "--events-source", paths.events_source,
        "--decoder-source", paths.decoder_source,
        "--reducer-source", paths.state_rdps_source,
        "--state-formula-source", paths.state_formula_source,
        "--output", paths.source_hp_runtime_route_proof,
      ], { cwd: repoRoot, stdio: "inherit", env: { ...process.env, NODE_NO_WARNINGS: "1" } }),
    }, force);
    execFileSync(process.execPath, [
      sourceHpRuntimeRouteProofTool,
      "verify",
      "--input", paths.source_hp_runtime_route_proof,
    ], { cwd: repoRoot, stdio: "inherit" });

    const selectedFactorRuntimeRouteProofTool = path.join(scriptDir, "bpsr-selected-factor-runtime-route-proof.mjs");
    runCachedStage(cache, stageResults, {
      id: "selected-factor-runtime-route-proof",
      inputs: [
        paths.factor_catalog,
        paths.complete_build_source_manifest,
        paths.static_formula_evidence,
        paths.decoder_source,
        paths.dreamscope_inference_source,
        paths.factor_correlation_source,
        paths.rdps_validation_source,
        paths.runtime_selector_catalog,
      ],
      tools: [selectedFactorRuntimeRouteProofTool],
      outputs: [paths.selected_factor_runtime_route_proof],
      command: ["node", "bpsr-selected-factor-runtime-route-proof.mjs", "build", build],
      run: () => execFileSync(process.execPath, [
        selectedFactorRuntimeRouteProofTool,
        "build",
        "--build", build,
        "--factor-catalog", paths.factor_catalog,
        "--source-manifest", paths.complete_build_source_manifest,
        "--static-formula-evidence", paths.static_formula_evidence,
        "--decoder-source", paths.decoder_source,
        "--dreamscope-inference-source", paths.dreamscope_inference_source,
        "--factor-correlation-source", paths.factor_correlation_source,
        "--rdps-validation-source", paths.rdps_validation_source,
        "--runtime-selector-catalog", paths.runtime_selector_catalog,
        "--output", paths.selected_factor_runtime_route_proof,
      ], { cwd: repoRoot, stdio: "inherit", env: { ...process.env, NODE_NO_WARNINGS: "1" } }),
    }, force);
    execFileSync(process.execPath, [
      selectedFactorRuntimeRouteProofTool,
      "verify",
      "--input", paths.selected_factor_runtime_route_proof,
    ], { cwd: repoRoot, stdio: "inherit" });

    const psychoscopeFactorClosureTool = path.join(scriptDir, "psychoscope-factor-closure.mjs");
    runCachedStage(cache, stageResults, {
      id: "psychoscope-factor-offline-closure",
      inputs: [paths.factor_catalog, paths.exact_recount_table, paths.skill_names],
      tools: [psychoscopeFactorClosureTool],
      outputs: [paths.psychoscope_factor_offline_closure],
      command: ["node", "psychoscope-factor-closure.mjs", build],
      run: () => execFileSync(process.execPath, [
        psychoscopeFactorClosureTool,
        "--factors", paths.factor_catalog,
        "--recount", paths.exact_recount_table,
        "--skills", paths.skill_names,
        "--build", build,
        "--output", paths.psychoscope_factor_offline_closure,
      ], { cwd: repoRoot, stdio: "inherit", env: { ...process.env, NODE_NO_WARNINGS: "1" } }),
    }, force);

    const selectedFactorMechanicRouteProofTool = path.join(scriptDir, "bpsr-selected-factor-mechanic-route-proof.mjs");
    runCachedStage(cache, stageResults, {
      id: "selected-factor-mechanic-route-proof",
      inputs: [
        paths.selected_factor_runtime_route_proof,
        paths.factor_catalog,
        paths.psychoscope_factor_offline_closure,
        paths.modifier_relationship_table,
        paths.skill_damage_chain_bridge,
        paths.effect_sources,
        paths.exact_recount_table,
        paths.complete_build_source_manifest,
      ],
      tools: [selectedFactorMechanicRouteProofTool],
      outputs: [paths.selected_factor_mechanic_route_proof],
      command: ["node", "bpsr-selected-factor-mechanic-route-proof.mjs", "build", build],
      run: () => execFileSync(process.execPath, [
        selectedFactorMechanicRouteProofTool,
        "build",
        "--build", build,
        "--selection-route-proof", paths.selected_factor_runtime_route_proof,
        "--factor-catalog", paths.factor_catalog,
        "--closure", paths.psychoscope_factor_offline_closure,
        "--relationship-table", paths.modifier_relationship_table,
        "--damage-chain-bridge", paths.skill_damage_chain_bridge,
        "--effect-sources", paths.effect_sources,
        "--recount-table", paths.exact_recount_table,
        "--source-manifest", paths.complete_build_source_manifest,
        "--output", paths.selected_factor_mechanic_route_proof,
      ], { cwd: repoRoot, stdio: "inherit", env: { ...process.env, NODE_NO_WARNINGS: "1" } }),
    }, force);
    execFileSync(process.execPath, [
      selectedFactorMechanicRouteProofTool,
      "verify",
      "--input", paths.selected_factor_mechanic_route_proof,
    ], { cwd: repoRoot, stdio: "inherit" });

    const selectedFactorCaptureCorrelationProofTool = path.join(scriptDir, "bpsr-selected-factor-capture-correlation-proof.mjs");
    runCachedStage(cache, stageResults, {
      id: "selected-factor-capture-correlation-proof",
      inputs: [paths.selected_factor_mechanic_route_proof, paths.selected_factor_correlation_bundle],
      tools: [selectedFactorCaptureCorrelationProofTool],
      outputs: [paths.selected_factor_capture_correlation_proof],
      command: ["node", "bpsr-selected-factor-capture-correlation-proof.mjs", "build", build],
      run: () => execFileSync(process.execPath, [
        selectedFactorCaptureCorrelationProofTool,
        "build",
        "--build", build,
        "--mechanic-proof", paths.selected_factor_mechanic_route_proof,
        "--correlation-bundle", paths.selected_factor_correlation_bundle,
        "--output", paths.selected_factor_capture_correlation_proof,
      ], { cwd: repoRoot, stdio: "inherit", env: { ...process.env, NODE_NO_WARNINGS: "1" } }),
    }, force);
    execFileSync(process.execPath, [
      selectedFactorCaptureCorrelationProofTool,
      "verify",
      "--input", paths.selected_factor_capture_correlation_proof,
    ], { cwd: repoRoot, stdio: "inherit" });

    runCachedStage(cache, stageResults, {
      id: "factor-capture-correlation-proof",
      inputs: [paths.psychoscope_factor_offline_closure, paths.selected_factor_correlation_bundle],
      tools: [selectedFactorCaptureCorrelationProofTool],
      outputs: [paths.factor_capture_correlation_proof],
      command: ["node", "bpsr-selected-factor-capture-correlation-proof.mjs", "build-full-catalog", build],
      run: () => execFileSync(process.execPath, [
        selectedFactorCaptureCorrelationProofTool,
        "build",
        "--build", build,
        "--factor-closure", paths.psychoscope_factor_offline_closure,
        "--correlation-bundle", paths.selected_factor_correlation_bundle,
        "--output", paths.factor_capture_correlation_proof,
      ], { cwd: repoRoot, stdio: "inherit", env: { ...process.env, NODE_NO_WARNINGS: "1" } }),
    }, force);
    execFileSync(process.execPath, [
      selectedFactorCaptureCorrelationProofTool,
      "verify",
      "--input", paths.factor_capture_correlation_proof,
    ], { cwd: repoRoot, stdio: "inherit" });

    const dreamscopeRuntimeFingerprintProofTool = path.join(scriptDir, "bpsr-dreamscope-runtime-fingerprint-proof.mjs");
    runCachedStage(cache, stageResults, {
      id: "dreamscope-runtime-fingerprint-proof",
      inputs: [paths.psychoscope_factor_offline_closure, paths.runtime_selector_catalog, paths.factor_capture_correlation_proof, paths.deep_slumber_psychoscope_nodes],
      tools: [dreamscopeRuntimeFingerprintProofTool],
      outputs: [paths.dreamscope_runtime_fingerprint_proof],
      command: ["node", "bpsr-dreamscope-runtime-fingerprint-proof.mjs", "build", build],
      run: () => execFileSync(process.execPath, [
        dreamscopeRuntimeFingerprintProofTool,
        "build",
        "--build", build,
        "--factor-closure", paths.psychoscope_factor_offline_closure,
        "--runtime-catalog", paths.runtime_selector_catalog,
        "--capture-proof", paths.factor_capture_correlation_proof,
        "--tree-nodes", paths.deep_slumber_psychoscope_nodes,
        "--output", paths.dreamscope_runtime_fingerprint_proof,
      ], { cwd: repoRoot, stdio: "inherit", env: { ...process.env, NODE_NO_WARNINGS: "1" } }),
    }, force);
    execFileSync(process.execPath, [
      dreamscopeRuntimeFingerprintProofTool,
      "verify",
      "--input", paths.dreamscope_runtime_fingerprint_proof,
    ], { cwd: repoRoot, stdio: "inherit" });

    const historicalFactorRouteStabilityProofTool = path.join(scriptDir, "bpsr-historical-factor-route-stability-proof.mjs");
    runCachedStage(cache, stageResults, {
      id: "historical-factor-route-stability-proof",
      inputs: [paths.psychoscope_factor_offline_closure, paths.historical_factor_correlation_bundle],
      tools: [historicalFactorRouteStabilityProofTool],
      outputs: [paths.historical_factor_route_stability_proof],
      command: ["node", "bpsr-historical-factor-route-stability-proof.mjs", "build", build],
      run: () => execFileSync(process.execPath, [
        historicalFactorRouteStabilityProofTool,
        "build",
        "--build", build,
        "--factor-closure", paths.psychoscope_factor_offline_closure,
        "--correlation-bundle", paths.historical_factor_correlation_bundle,
        "--output", paths.historical_factor_route_stability_proof,
      ], { cwd: repoRoot, stdio: "inherit", env: { ...process.env, NODE_NO_WARNINGS: "1" } }),
    }, force);
    execFileSync(process.execPath, [
      historicalFactorRouteStabilityProofTool,
      "verify",
      "--input", paths.historical_factor_route_stability_proof,
    ], { cwd: repoRoot, stdio: "inherit" });

    const allElementFixedPointFamilyProofTool = path.join(scriptDir, "bpsr-all-element-fixed-point-family-proof.mjs");
    runCachedStage(cache, stageResults, {
      id: "all-element-fixed-point-family-proof",
      inputs: [paths.fight_attr_table, paths.imagine_formula_proof, paths.rdps_runtime_config, paths.state_formula_source],
      tools: [allElementFixedPointFamilyProofTool],
      outputs: [paths.all_element_fixed_point_family_proof],
      command: ["node", "bpsr-all-element-fixed-point-family-proof.mjs", "build", build],
      run: () => execFileSync(process.execPath, [
        allElementFixedPointFamilyProofTool,
        "build",
        "--build", build,
        "--fight-attr-table", paths.fight_attr_table,
        "--imagine-proof", paths.imagine_formula_proof,
        "--runtime-config", paths.rdps_runtime_config,
        "--packet-formula-source", paths.state_formula_source,
        "--output", paths.all_element_fixed_point_family_proof,
      ], { cwd: repoRoot, stdio: "inherit", env: { ...process.env, NODE_NO_WARNINGS: "1" } }),
    }, force);
    execFileSync(process.execPath, [
      allElementFixedPointFamilyProofTool,
      "verify",
      "--input", paths.all_element_fixed_point_family_proof,
    ], { cwd: repoRoot, stdio: "inherit" });

    const sharedFormulaProofRegistryTool = path.join(scriptDir, "bpsr-shared-formula-proof-registry.mjs");
    runCachedStage(cache, stageResults, {
      id: "shared-formula-proof-registry",
      inputs: [paths.fight_attribute_transform_proof, paths.primary_stat_attack_transform_proof, paths.primary_attack_runtime_route_proof, paths.mastery_runtime_route_proof, paths.source_hp_runtime_route_proof, paths.selected_factor_runtime_route_proof, paths.selected_factor_mechanic_route_proof, paths.selected_factor_capture_correlation_proof, paths.all_element_fixed_point_family_proof],
      tools: [sharedFormulaProofRegistryTool],
      outputs: [paths.shared_formula_proof_registry],
      command: ["node", "bpsr-shared-formula-proof-registry.mjs", "build", build],
      run: () => execFileSync(process.execPath, [
        sharedFormulaProofRegistryTool,
        "build",
        "--build", build,
        "--fight-attribute-proof", paths.fight_attribute_transform_proof,
        "--primary-stat-proof", paths.primary_stat_attack_transform_proof,
        "--primary-attack-route-proof", paths.primary_attack_runtime_route_proof,
        "--mastery-route-proof", paths.mastery_runtime_route_proof,
        "--source-hp-route-proof", paths.source_hp_runtime_route_proof,
        "--selected-factor-route-proof", paths.selected_factor_runtime_route_proof,
        "--selected-factor-mechanic-proof", paths.selected_factor_mechanic_route_proof,
        "--selected-factor-capture-correlation-proof", paths.selected_factor_capture_correlation_proof,
        "--all-element-family-proof", paths.all_element_fixed_point_family_proof,
        "--output", paths.shared_formula_proof_registry,
      ], { cwd: repoRoot, stdio: "inherit", env: { ...process.env, NODE_NO_WARNINGS: "1" } }),
    }, force);
    execFileSync(process.execPath, [
      sharedFormulaProofRegistryTool,
      "verify",
      "--input", paths.shared_formula_proof_registry,
    ], { cwd: repoRoot, stdio: "inherit" });

    const formulaModelTool = path.join(scriptDir, "bpsr-formula-model-workbench.mjs");
    runCachedStage(cache, stageResults, {
      id: "formula-model-workbench",
      inputs: [paths.static_formula_evidence, paths.value_proof, paths.shared_formula_proof_registry],
      tools: [formulaModelTool],
      outputs: [paths.formula_model_workbench],
      command: ["node", "bpsr-formula-model-workbench.mjs", "build", build],
      run: () => execFileSync(process.execPath, [
        formulaModelTool,
        "build",
        "--build", build,
        "--static-formula-evidence", paths.static_formula_evidence,
        "--value-proof", paths.value_proof,
        "--proof-registry", paths.shared_formula_proof_registry,
        "--output", paths.formula_model_workbench,
      ], { cwd: repoRoot, stdio: "inherit", env: { ...process.env, NODE_NO_WARNINGS: "1" } }),
    }, force);
    execFileSync(process.execPath, [
      formulaModelTool,
      "verify",
      "--input", paths.formula_model_workbench,
    ], { cwd: repoRoot, stdio: "inherit" });

    const criticalEventStateRouteProofTool = path.join(scriptDir, "bpsr-critical-event-state-route-proof.mjs");
    runCachedStage(cache, stageResults, {
      id: "critical-event-state-route-proof",
      inputs: [
        paths.formula_model_workbench,
        paths.events_source,
        paths.decoder_source,
        paths.state_rdps_source,
        paths.state_formula_source,
      ],
      tools: [criticalEventStateRouteProofTool],
      outputs: [paths.critical_event_state_route_proof],
      command: ["node", "bpsr-critical-event-state-route-proof.mjs", "build", build],
      run: () => execFileSync(process.execPath, [
        criticalEventStateRouteProofTool,
        "build",
        "--build", build,
        "--workbench", paths.formula_model_workbench,
        "--events-source", paths.events_source,
        "--decoder-source", paths.decoder_source,
        "--reducer-source", paths.state_rdps_source,
        "--formula-source", paths.state_formula_source,
        "--output", paths.critical_event_state_route_proof,
      ], { cwd: repoRoot, stdio: "inherit", env: { ...process.env, NODE_NO_WARNINGS: "1" } }),
    }, force);
    execFileSync(process.execPath, [
      criticalEventStateRouteProofTool,
      "verify",
      "--input", paths.critical_event_state_route_proof,
    ], { cwd: repoRoot, stdio: "inherit" });

    const recipientInputs = [
      paths.static_worklist, paths.magnitude_watchlist, paths.semantic_audit, paths.modifier_display,
      paths.packet_proof, paths.packet_inventory, paths.provider_audit, paths.exhaustive_provider_audit,
      paths.component_packet_proof, paths.severed_chapter_proof, paths.severed_chapter_audit,
      paths.battle_cry_proof, paths.battle_cry_audit, paths.denvel_proof, paths.denvel_audit,
      paths.focused_shot_proof, paths.focused_shot_audit, paths.stellar_spark_proof,
      paths.stellar_spark_audit, paths.current_origin_ledger,
    ];
    runCachedStage(cache, stageResults, {
      id: "rdps-recipient-scope-ledger",
      inputs: recipientInputs,
      tools: rustToolFiles("rdps-recipient-scope-ledger.rs"),
      outputs: [paths.recipient_scope_ledger],
      command: ["cargo", "rlogs-bpsr-rdps-recipient-scope-ledger", build, String(context.config.historical_packet_build)],
      run: () => runCargo("rlogs-bpsr-rdps-recipient-scope-ledger", [
        "--worklist", paths.static_worklist,
        "--watchlist", paths.magnitude_watchlist,
        "--semantic-audit", paths.semantic_audit,
        "--display", paths.modifier_display,
        "--packet-proof", paths.packet_proof,
        "--packet-inventory", paths.packet_inventory,
        "--provider-audit", paths.provider_audit,
        "--exhaustive-provider-audit", paths.exhaustive_provider_audit,
        "--component-packet-proof", paths.component_packet_proof,
        "--severed-chapter-proof", paths.severed_chapter_proof,
        "--severed-chapter-audit", paths.severed_chapter_audit,
        "--battle-cry-proof", paths.battle_cry_proof,
        "--battle-cry-audit", paths.battle_cry_audit,
        "--denvel-proof", paths.denvel_proof,
        "--denvel-audit", paths.denvel_audit,
        "--focused-shot-proof", paths.focused_shot_proof,
        "--focused-shot-audit", paths.focused_shot_audit,
        "--stellar-spark-proof", paths.stellar_spark_proof,
        "--stellar-spark-audit", paths.stellar_spark_audit,
        "--origin-ledger", paths.current_origin_ledger,
        "--packet-build", String(context.config.historical_packet_build),
        "--output", paths.recipient_scope_ledger,
      ]),
    }, force);
    requireGeneratedBuild(paths.recipient_scope_ledger, build, "rlogs-bpsr-rdps-recipient-scope-ledger");

    const proofFrontierRouterTool = path.join(scriptDir, "bpsr-proof-frontier-router.mjs");
    runCachedStage(cache, stageResults, {
      id: "proof-frontier-router",
      inputs: [paths.proof_frontier_workbench, paths.formula_gap_ledger, paths.static_formula_evidence, paths.recipient_scope_ledger],
      tools: [proofFrontierRouterTool],
      outputs: [paths.proof_frontier_router],
      command: ["node", "bpsr-proof-frontier-router.mjs", "build", build],
      run: () => execFileSync(process.execPath, [
        proofFrontierRouterTool,
        "build",
        "--build", build,
        "--workbench", paths.proof_frontier_workbench,
        "--formula-ledger", paths.formula_gap_ledger,
        "--static-formula-evidence", paths.static_formula_evidence,
        "--recipient-ledger", paths.recipient_scope_ledger,
        "--output", paths.proof_frontier_router,
      ], { cwd: repoRoot, stdio: "inherit" }),
    }, force);
    execFileSync(process.execPath, [
      proofFrontierRouterTool,
      "verify",
      "--input", paths.proof_frontier_router,
    ], { cwd: repoRoot, stdio: "inherit" });

    const resolutionBatchTool = path.join(scriptDir, "bpsr-semantic-resolution-batches.mjs");
    runCachedStage(cache, stageResults, {
      id: "semantic-resolution-batches",
      inputs: [
        paths.semantic_evidence_index,
        paths.semantic_dependency_closure,
        paths.produced_damage_proof_routes,
        paths.formula_gap_ledger,
        paths.static_formula_evidence,
        paths.static_worklist,
        paths.recipient_scope_ledger,
      ],
      tools: [resolutionBatchTool],
      outputs: [paths.semantic_resolution_batches],
      command: ["node", "bpsr-semantic-resolution-batches.mjs", "build", build, "25"],
      run: () => execFileSync(process.execPath, [
        resolutionBatchTool,
        "build",
        "--build", build,
        "--index", paths.semantic_evidence_index,
        "--semantic-closure", paths.semantic_dependency_closure,
        "--route-ledger", paths.produced_damage_proof_routes,
        "--formula-ledger", paths.formula_gap_ledger,
        "--static-formula-evidence", paths.static_formula_evidence,
        "--static-worklist", paths.static_worklist,
        "--recipient-ledger", paths.recipient_scope_ledger,
        "--output", paths.semantic_resolution_batches,
        "--batch-size", "25",
      ], {
        cwd: repoRoot,
        stdio: "inherit",
        env: { ...process.env, NODE_NO_WARNINGS: "1" },
      }),
    }, force);
    requireGeneratedBuild(
      paths.semantic_resolution_batches,
      build,
      "tools/bpsr-semantic-resolution-batches.mjs",
    );
    execFileSync(process.execPath, [
      resolutionBatchTool,
      "verify",
      "--input", paths.semantic_resolution_batches,
      "--index", paths.semantic_evidence_index,
    ], {
      cwd: repoRoot,
      stdio: "inherit",
      env: { ...process.env, NODE_NO_WARNINGS: "1" },
    });

    const proofAttemptTool = path.join(scriptDir, "bpsr-proof-attempt-ledger.mjs");
    const proofReceiptRegistry = path.join(paths.build_root, "proof-attempt-receipts.v1.json");
    const proofReceiptEvidence = proofReceiptEvidenceFiles(proofReceiptRegistry);
    runCachedStage(cache, stageResults, {
      id: "proof-attempt-ledger",
      inputs: [paths.proof_frontier_router, paths.semantic_resolution_batches, ...proofReceiptEvidence],
      optional_inputs: [proofReceiptRegistry],
      tools: [proofAttemptTool],
      outputs: [paths.proof_attempt_ledger],
      command: ["node", "bpsr-proof-attempt-ledger.mjs", "build", build],
      run: () => execFileSync(process.execPath, [
        proofAttemptTool,
        "build",
        "--build", build,
        "--router", paths.proof_frontier_router,
        "--batches", paths.semantic_resolution_batches,
        "--receipts", proofReceiptRegistry,
        "--output", paths.proof_attempt_ledger,
      ], { cwd: repoRoot, stdio: "inherit" }),
    }, force);
    requireGeneratedBuild(
      paths.proof_attempt_ledger,
      build,
      "tools/bpsr-proof-attempt-ledger.mjs",
    );
    execFileSync(process.execPath, [
      proofAttemptTool,
      "verify",
      "--input", paths.proof_attempt_ledger,
    ], { cwd: repoRoot, stdio: "inherit" });

    const proofCorrelationManifestTool = path.join(scriptDir, "bpsr-proof-correlation-manifest.mjs");
    runCachedStage(cache, stageResults, {
      id: "proof-correlation-manifest",
      inputs: [
        paths.proof_frontier_router,
        paths.semantic_resolution_batches,
        paths.primary_attack_runtime_route_proof,
        paths.mastery_runtime_route_proof,
        paths.source_hp_runtime_route_proof,
      ],
      tools: [proofCorrelationManifestTool],
      outputs: [paths.proof_correlation_manifest],
      command: ["node", "bpsr-proof-correlation-manifest.mjs", "build", build],
      run: () => execFileSync(process.execPath, [
        proofCorrelationManifestTool,
        "build",
        "--build", build,
        "--batches", paths.semantic_resolution_batches,
        "--router", paths.proof_frontier_router,
        "--primary-attack-route-proof", paths.primary_attack_runtime_route_proof,
        "--mastery-route-proof", paths.mastery_runtime_route_proof,
        "--source-hp-route-proof", paths.source_hp_runtime_route_proof,
        "--output", paths.proof_correlation_manifest,
        "--report-schema", "10",
      ], { cwd: repoRoot, stdio: "inherit" }),
    }, force);
    requireGeneratedBuild(
      paths.proof_correlation_manifest,
      build,
      "tools/bpsr-proof-correlation-manifest.mjs",
    );
    execFileSync(process.execPath, [
      proofCorrelationManifestTool,
      "verify",
      "--input", paths.proof_correlation_manifest,
    ], { cwd: repoRoot, stdio: "inherit" });

    const runtimeEffectComponentRoutingProofTool = path.join(
      scriptDir,
      "bpsr-runtime-effect-component-routing-proof.mjs",
    );
    runCachedStage(cache, stageResults, {
      id: "runtime-effect-component-routing-proof",
      inputs: [paths.contribution],
      tools: [runtimeEffectComponentRoutingProofTool],
      outputs: [paths.runtime_effect_component_routing_proof],
      command: ["node", "bpsr-runtime-effect-component-routing-proof.mjs", "build", build],
      run: () => execFileSync(process.execPath, [
        runtimeEffectComponentRoutingProofTool,
        "build",
        "--build", build,
        "--contribution", paths.contribution,
        "--output", paths.runtime_effect_component_routing_proof,
      ], { cwd: repoRoot, stdio: "inherit" }),
    }, force);
    requireGeneratedBuild(
      paths.runtime_effect_component_routing_proof,
      build,
      "tools/bpsr-runtime-effect-component-routing-proof.mjs",
    );
    execFileSync(process.execPath, [
      runtimeEffectComponentRoutingProofTool,
      "verify",
      "--input", paths.runtime_effect_component_routing_proof,
    ], { cwd: repoRoot, stdio: "inherit" });

    // Build and verify the exact protocol-pack gate before proof closure so
    // replay-complete subsets cannot be reported as production-runtime credit
    // while the matching-build packet identity is absent or blocked.
    const protocolStatusTool = path.join(scriptDir, "bpsr-protocol-pack-status.mjs");
    const protocolReportsRoot = path.join(paths.build_root, "protocol-decode-recordings-v2");
    const protocolCandidate = path.join(paths.build_root, "protocol-pack-static-candidate.v2.json");
    const protocolAudit = path.join(protocolReportsRoot, "protocol-pack-promotion-audit.v2.json");
    const promotedProtocolPack = path.join(
      repoRoot,
      "plugins", "games", "blue-protocol-star-resonance", "protocol-packs", "global",
      `steam-${build}`, "pack.json",
    );
    const protocolReports = walkFiles(protocolReportsRoot, ".offline-recording-report.json");
    runCachedStage(cache, stageResults, {
      id: "protocol-pack-status",
      inputs: [protocolCandidate, protocolAudit, ...protocolReports],
      optional_inputs: [promotedProtocolPack],
      tools: [protocolStatusTool],
      outputs: [paths.protocol_status],
      command: ["node", "bpsr-protocol-pack-status.mjs", build],
      run: () => execFileSync(process.execPath, [
        protocolStatusTool,
        "generate",
        "--build", build,
        "--candidate", protocolCandidate,
        "--audit", protocolAudit,
        "--reports-root", protocolReportsRoot,
        "--promoted-pack", promotedProtocolPack,
        "--output", paths.protocol_status,
      ], { cwd: repoRoot, stdio: "inherit" }),
    }, force);
    requireGeneratedBuild(paths.protocol_status, build, "tools/bpsr-protocol-pack-status.mjs");
    execFileSync(process.execPath, [
      protocolStatusTool,
      "verify",
      "--input", paths.protocol_status,
    ], { cwd: repoRoot, stdio: "inherit" });

    const proofClosureTool = path.join(scriptDir, "bpsr-rdps-proof-closure.mjs");
    runCachedStage(cache, stageResults, {
      id: "rdps-proof-closure",
      inputs: [
        paths.proof_correlation_manifest,
        paths.static_formula_evidence,
        paths.formula_model_workbench,
        paths.retained_proofs,
        paths.runtime_effect_component_routing_proof,
        paths.party_skill_static_closure,
        paths.party_effect_window_audit,
        paths.protocol_status,
      ],
      optional_inputs: [
        paths.proof_correlation_aggregate,
        paths.runtime_attribution_evidence,
        paths.life_wave_trigger_proof,
        paths.life_wave_remote_inference_proof,
      ],
      tools: [proofClosureTool],
      outputs: [paths.rdps_proof_closure],
      command: ["node", "bpsr-rdps-proof-closure.mjs", "build", build],
      run: () => execFileSync(process.execPath, [
        proofClosureTool,
        "build",
        "--build", build,
        "--manifest", paths.proof_correlation_manifest,
        "--aggregate", paths.proof_correlation_aggregate,
        "--static-formula-evidence", paths.static_formula_evidence,
        "--workbench", paths.formula_model_workbench,
        "--carry-forward", paths.retained_proofs,
        "--runtime-effect-component-routing-proof", paths.runtime_effect_component_routing_proof,
        "--party-skill-static-closure", paths.party_skill_static_closure,
        "--party-effect-window-audit", paths.party_effect_window_audit,
        "--protocol-status", paths.protocol_status,
        ...(existsSync(paths.runtime_attribution_evidence)
          ? ["--runtime-attribution-evidence", paths.runtime_attribution_evidence]
          : []),
        ...(existsSync(paths.life_wave_trigger_proof)
          ? ["--life-wave-trigger-proof", paths.life_wave_trigger_proof]
          : []),
        ...(existsSync(paths.life_wave_remote_inference_proof)
          ? ["--life-wave-remote-inference-proof", paths.life_wave_remote_inference_proof]
          : []),
        "--output", paths.rdps_proof_closure,
      ], { cwd: repoRoot, stdio: "inherit" }),
    }, force);
    requireGeneratedBuild(paths.rdps_proof_closure, build, "tools/bpsr-rdps-proof-closure.mjs");
    execFileSync(process.execPath, [
      proofClosureTool,
      "verify",
      "--input", paths.rdps_proof_closure,
    ], { cwd: repoRoot, stdio: "inherit" });

    const deferredAttributionLedgerTool = path.join(scriptDir, "bpsr-deferred-attribution-ledger.mjs");
    runCachedStage(cache, stageResults, {
      id: "deferred-attribution-ledger",
      inputs: [paths.rdps_proof_closure],
      optional_inputs: [paths.proof_correlation_aggregate],
      tools: [deferredAttributionLedgerTool],
      outputs: [paths.deferred_attribution_ledger],
      command: ["node", "bpsr-deferred-attribution-ledger.mjs", "build", build],
      run: () => execFileSync(process.execPath, [
        deferredAttributionLedgerTool,
        "build",
        "--build", build,
        "--closure", paths.rdps_proof_closure,
        "--aggregate", paths.proof_correlation_aggregate,
        "--output", paths.deferred_attribution_ledger,
      ], { cwd: repoRoot, stdio: "inherit" }),
    }, force);
    requireGeneratedBuild(
      paths.deferred_attribution_ledger,
      build,
      "tools/bpsr-deferred-attribution-ledger.mjs",
    );
    execFileSync(process.execPath, [
      deferredAttributionLedgerTool,
      "verify",
      "--input", paths.deferred_attribution_ledger,
    ], { cwd: repoRoot, stdio: "inherit" });

    // The preflight traverses a plan whose referenced files can change without
    // the plan itself changing. It is intentionally cheap and always rebuilt.
    const preflightStarted = performance.now();
    runCargo("rlogs-bpsr-rdps-build-audit", [
      "preflight",
      "--plan", paths.build_audit_plan,
      "--root", repoRoot,
      "--build", build,
      "--output", paths.preflight,
    ]);
    requireGeneratedBuild(paths.preflight, build, "rlogs-bpsr-rdps-build-audit");
    stageResults.push({
      id: "rdps-build-preflight",
      status: "executed",
      cache_policy: "always_run_recursive_plan_validation",
      duration_ms: Math.round(performance.now() - preflightStarted),
      outputs: fingerprintFiles([paths.preflight]),
    });

    cache.generated_at = new Date().toISOString();
    writeJson(cacheFile, cache);
    writeReport(context, outputFiles.filter((file) => file !== paths.refresh_report), stageResults, force);
    const executed = stageResults.filter((stage) => stage.status === "executed").length;
    const reused = stageResults.filter((stage) => stage.status === "reused").length;
    console.log(`Current-build semantic and rDPS research layers ready for build ${build}: ${executed} stages executed, ${reused} reused by exact content hash.`);
  } catch (error) {
    restoreOutputs([...outputFiles, cacheFile], backups);
    throw error;
  } finally {
    rmSync(backupRoot, { recursive: true, force: true });
  }
}

function collectInputs(paths) {
  const outputKeys = new Set([
    "current_origin_ledger", "static_worklist", "magnitude_watchlist", "semantic_audit", "ctb_table_identity_map", "semantic_dependency_closure", "decoded_expression_reference_edges", "semantic_evidence_index", "produced_damage_proof_routes", "semantic_field_adjudications", "proof_frontier_workbench", "formula_gap_ledger",
    "formula_gap_watchlist", "static_formula_evidence", "primary_attack_runtime_route_proof", "mastery_runtime_route_proof", "source_hp_runtime_route_proof", "selected_factor_runtime_route_proof", "psychoscope_factor_offline_closure", "selected_factor_mechanic_route_proof", "selected_factor_capture_correlation_proof", "factor_capture_correlation_proof", "dreamscope_runtime_fingerprint_proof", "historical_factor_route_stability_proof", "all_element_fixed_point_family_proof", "shared_formula_proof_registry", "formula_model_workbench", "critical_event_state_route_proof", "recipient_scope_ledger", "proof_frontier_router", "semantic_resolution_batches", "proof_attempt_ledger", "proof_correlation_manifest", "runtime_effect_component_routing_proof", "protocol_status", "rdps_proof_closure", "deferred_attribution_ledger", "preflight", "refresh_report",
  ]);
  const optionalInputKeys = new Set([
    "proof_correlation_aggregate",
    "runtime_attribution_evidence",
    "life_wave_trigger_proof",
    "life_wave_remote_inference_proof",
  ]);
  return Object.entries(paths)
    .filter(([key]) => !outputKeys.has(key) && !optionalInputKeys.has(key) && !key.endsWith("_root"))
    .map(([key, file]) => [key.replaceAll("_", " "), file]);
}

function writeReport(context, outputs, stageResults, force) {
  const artifacts = outputs.map((file) => {
    requireFile(file, "semantic refresh output");
    const binary = path.extname(file).toLowerCase() === ".sqlite";
    const value = binary ? null : readJson(file, "semantic refresh output");
    return {
      path: relativePath(file),
      bytes: statSync(file).size,
      sha256: sha256(file),
      format: binary ? "sqlite" : "json",
      generated_by: binary ? "tools/bpsr-semantic-evidence-index.mjs" : value.generated_by ?? null,
      schema_version: binary ? 1 : value.schema_version ?? null,
      summary: binary ? null : value.summary ?? null,
    };
  });
  const report = {
    schema_version: 1,
    generated_by: "tools/bpsr-current-build-semantic-refresh.mjs",
    game_build: context.build,
    config: relativePath(context.configPath),
    policy: {
      steam_manifest_is_physical_change_detector_only: true,
      semantic_hashes_select_regeneration_and_reproof: true,
      unresolved_evidence_hidden: false,
      historical_evidence_enables_current_runtime: false,
      failed_refresh_rolls_back_all_outputs: true,
      content_addressed_stage_reuse: true,
      output_hashes_verified_before_reuse: true,
    },
    summary: {
      artifacts: artifacts.length,
      executed_stages: stageResults.filter((stage) => stage.status === "executed").length,
      reused_stages: stageResults.filter((stage) => stage.status === "reused").length,
      forced_rebuild: force,
      explicit_inputs: collectInputs(context.paths).length,
      hidden_omissions: 0,
    },
    stages: stageResults,
    artifacts,
  };
  mkdirSync(path.dirname(context.paths.refresh_report), { recursive: true });
  writeFileSync(context.paths.refresh_report, `${JSON.stringify(report, null, 2)}\n`, "utf8");
}

function proofReceiptEvidenceFiles(receiptRegistry) {
  if (!existsSync(receiptRegistry)) return [];
  const registry = readJson(receiptRegistry, "proof attempt receipt registry");
  if (registry.schema_version !== 1 || !Array.isArray(registry.receipts)) {
    throw new Error(`Invalid proof attempt receipt registry: ${receiptRegistry}`);
  }
  return [...new Set(registry.receipts.flatMap((receipt) =>
    (receipt.evidence ?? []).map((entry) => resolvePath(entry.path)),
  ))].sort();
}

function emptyCache(build) {
  return {
    schema_version: 1,
    generated_by: "tools/bpsr-current-build-semantic-refresh.mjs",
    game_build: String(build),
    generated_at: null,
    stages: {},
  };
}

function loadCache(file, build) {
  if (!existsSync(file)) return emptyCache(build);
  const cache = readJson(file, "semantic refresh cache");
  if (cache.schema_version !== 1 || String(cache.game_build) !== String(build) || !cache.stages) {
    return emptyCache(build);
  }
  return cache;
}

function runCachedStage(cache, results, stage, force) {
  const started = performance.now();
  const inputs = fingerprintFiles(stage.inputs);
  const optionalInputs = fingerprintOptionalFiles(stage.optional_inputs ?? []);
  const tools = fingerprintFiles(stage.tools);
  const fingerprint = hashJson({ command: stage.command, inputs, optional_inputs: optionalInputs, tools });
  const previous = cache.stages[stage.id];
  const outputsValid = !force
    && previous?.fingerprint === fingerprint
    && verifyFingerprints(previous.outputs ?? []);
  if (outputsValid) {
    const durationMs = Math.round(performance.now() - started);
    results.push({ id: stage.id, status: "reused", duration_ms: durationMs, fingerprint, outputs: previous.outputs });
    console.log(`reuse ${stage.id}: exact inputs, tools, and outputs unchanged`);
    return;
  }
  stage.run();
  const outputs = fingerprintFiles(stage.outputs);
  cache.stages[stage.id] = { fingerprint, command: stage.command, inputs, optional_inputs: optionalInputs, tools, outputs };
  const durationMs = Math.round(performance.now() - started);
  results.push({ id: stage.id, status: "executed", duration_ms: durationMs, fingerprint, outputs });
}

function fingerprintOptionalFiles(files) {
  return [...new Set(files.map((file) => path.normalize(file)))].sort().map((file) => {
    if (!existsSync(file)) return { path: relativePath(file), present: false };
    return { path: relativePath(file), present: true, bytes: statSync(file).size, sha256: sha256(file) };
  });
}

function fingerprintFiles(files) {
  return [...new Set(files.map((file) => path.normalize(file)))].sort().map((file) => {
    requireFile(file, "content-addressed stage input/output");
    return { path: relativePath(file), bytes: statSync(file).size, sha256: sha256(file) };
  });
}

function verifyFingerprints(entries) {
  return entries.length > 0 && entries.every((entry) => {
    const file = resolvePath(entry.path);
    return existsSync(file)
      && statSync(file).size === Number(entry.bytes)
      && sha256(file) === entry.sha256;
  });
}

function hashJson(value) {
  return createHash("sha256").update(JSON.stringify(value)).digest("hex");
}

function rustToolFiles(binaryFile) {
  return [
    path.join(repoRoot, "Cargo.lock"),
    path.join(repoRoot, "plugins", "games", "blue-protocol-star-resonance", "Cargo.toml"),
    path.join(repoRoot, "plugins", "games", "blue-protocol-star-resonance", "tools", binaryFile),
    ...walkFiles(path.join(repoRoot, "plugins", "games", "blue-protocol-star-resonance", "src"), ".rs"),
  ];
}

function walkFiles(root, extension) {
  if (!existsSync(root)) return [];
  const output = [];
  for (const name of readdirSync(root)) {
    const entry = path.join(root, name);
    const stat = lstatSync(entry);
    if (stat.isDirectory()) output.push(...walkFiles(entry, extension));
    else if (stat.isFile() && entry.endsWith(extension)) output.push(entry);
  }
  return output.sort();
}

function writeJson(file, value) {
  mkdirSync(path.dirname(file), { recursive: true });
  writeFileSync(file, `${JSON.stringify(value, null, 2)}\n`, "utf8");
}

function backupOutputs(files, backupRoot) {
  const backups = new Map();
  files.forEach((file, index) => {
    if (!existsSync(file)) return;
    const backup = path.join(backupRoot, `${index}.json`);
    copyFileSync(file, backup);
    backups.set(file, backup);
  });
  return backups;
}

function restoreOutputs(files, backups) {
  for (const file of files) {
    const backup = backups.get(file);
    if (backup) copyFileSync(backup, file);
    else rmSync(file, { force: true });
  }
}

function runCargo(binary, args) {
  execFileSync("cargo", [
    "run", "--quiet", "-p", "rlogs-game-bpsr", "--bin", binary, "--", ...args,
  ], { cwd: repoRoot, stdio: "inherit" });
}

function requireGeneratedBuild(file, build, generator) {
  requireFile(file, generator);
  const value = readJson(file, generator);
  if (value.generated_by !== generator) throw new Error(`${file} was not generated by ${generator}`);
  const actual = String(
    value.game_build
      ?? value.static_game_build
      ?? value.current_game_build
      ?? value.build_id
      ?? "",
  );
  if (actual !== String(build)) throw new Error(`${file} is build ${actual || "<missing>"}, expected ${build}`);
}

function requireBuild(file, build, label, keys) {
  const value = readJson(file, label);
  const actual = keys.map((key) => value[key]).find((candidate) => candidate !== undefined);
  if (String(actual ?? "") !== String(build)) {
    throw new Error(`${label} is build ${actual ?? "<missing>"}, expected ${build}: ${file}`);
  }
}

function verifyReferenceOccurrenceArtifact(graphFile, occurrenceFile) {
  const graph = readJson(graphFile, "decoded table reference graph");
  const artifact = graph.ambiguous_reference_occurrence_artifact;
  if (!artifact || artifact.format !== "jsonl") {
    throw new Error(`${graphFile} does not declare its untyped-reference JSONL artifact`);
  }
  const declaredPath = path.resolve(path.dirname(graphFile), artifact.path);
  if (path.normalize(declaredPath) !== path.normalize(occurrenceFile)) {
    throw new Error(
      `Decoded reference occurrence path mismatch: graph declares ${declaredPath}, configured ${occurrenceFile}`,
    );
  }
  requireFile(occurrenceFile, "decoded table untyped-reference worklist");
  const actualBytes = statSync(occurrenceFile).size;
  if (actualBytes !== Number(artifact.bytes)) {
    throw new Error(
      `Decoded reference occurrence byte mismatch: ${actualBytes}, expected ${artifact.bytes}`,
    );
  }
  const actualSha256 = sha256(occurrenceFile);
  if (actualSha256 !== artifact.sha256) {
    throw new Error(
      `Decoded reference occurrence SHA256 mismatch: ${actualSha256}, expected ${artifact.sha256}`,
    );
  }
  if (Number(artifact.rows) !== Number(graph.summary?.ambiguous_reference_occurrences)) {
    throw new Error(
      `Decoded reference occurrence row mismatch: artifact ${artifact.rows}, graph ${graph.summary?.ambiguous_reference_occurrences}`,
    );
  }
}

function verifyReferenceCandidateArtifact(graphFile, candidateFile) {
  const graph = readJson(graphFile, "decoded table reference graph");
  const artifact = graph.reference_candidate_artifact;
  if (!artifact || artifact.format !== "jsonl") {
    throw new Error(`${graphFile} does not declare its reference-candidate JSONL artifact`);
  }
  const declaredPath = path.resolve(path.dirname(graphFile), artifact.path);
  if (path.normalize(declaredPath) !== path.normalize(candidateFile)) {
    throw new Error(
      `Decoded reference candidate path mismatch: graph declares ${declaredPath}, configured ${candidateFile}`,
    );
  }
  requireFile(candidateFile, "decoded table reference-candidate ledger");
  const actualBytes = statSync(candidateFile).size;
  if (actualBytes !== Number(artifact.bytes)) {
    throw new Error(
      `Decoded reference candidate byte mismatch: ${actualBytes}, expected ${artifact.bytes}`,
    );
  }
  const actualSha256 = sha256(candidateFile);
  if (actualSha256 !== artifact.sha256) {
    throw new Error(
      `Decoded reference candidate SHA256 mismatch: ${actualSha256}, expected ${artifact.sha256}`,
    );
  }
  if (Number(artifact.rows) !== Number(graph.summary?.reference_candidate_ledger_rows)) {
    throw new Error(
      `Decoded reference candidate row mismatch: artifact ${artifact.rows}, graph ${graph.summary?.reference_candidate_ledger_rows}`,
    );
  }
}

function expandObject(value, variables) {
  if (Array.isArray(value)) return value.map((entry) => expandObject(entry, variables));
  if (value && typeof value === "object") {
    return Object.fromEntries(Object.entries(value).map(([key, entry]) => [key, expandObject(entry, variables)]));
  }
  if (typeof value !== "string") return value;
  const expanded = value.replace(/\{([a-z_]+)\}/g, (_, key) => {
    if (!(key in variables)) throw new Error(`Unknown semantic refresh path variable {${key}}`);
    return variables[key];
  });
  return path.isAbsolute(expanded) ? path.normalize(expanded) : path.resolve(repoRoot, expanded);
}

function selfTest() {
  const variables = { repo_root: "C:\\repo", build: "123", build_root: "C:\\build", extractor_root: "C:\\extract", decoded_root: "C:\\decoded" };
  const expanded = expandObject({ one: "{build_root}/one.json", two: "fixed/two.json" }, variables);
  if (expanded.one !== path.normalize("C:\\build/one.json")) throw new Error("Variable expansion self-test failed");
  if (!expanded.two.endsWith(path.join("fixed", "two.json"))) throw new Error("Relative expansion self-test failed");
  const root = mkdtempSync(path.join(tmpdir(), "rlogs-semantic-cache-"));
  try {
    const input = path.join(root, "input.txt");
    const tool = path.join(root, "tool.txt");
    const output = path.join(root, "output.txt");
    writeFileSync(input, "input-v1", "utf8");
    writeFileSync(tool, "tool-v1", "utf8");
    const cache = emptyCache("123");
    let executions = 0;
    const stage = {
      id: "cache-self-test",
      inputs: [input],
      tools: [tool],
      outputs: [output],
      command: ["self-test"],
      run: () => {
        executions += 1;
        writeFileSync(output, `${readFileSync(input, "utf8")}:${readFileSync(tool, "utf8")}`, "utf8");
      },
    };
    const first = [];
    runCachedStage(cache, first, stage, false);
    if (executions !== 1 || first[0]?.status !== "executed") throw new Error("Cold cache self-test failed");
    const second = [];
    runCachedStage(cache, second, stage, false);
    if (executions !== 1 || second[0]?.status !== "reused") throw new Error("Exact reuse self-test failed");
    writeFileSync(input, "input-v2", "utf8");
    const third = [];
    runCachedStage(cache, third, stage, false);
    if (executions !== 2 || third[0]?.status !== "executed") throw new Error("Input invalidation self-test failed");
    writeFileSync(output, "corrupt", "utf8");
    const fourth = [];
    runCachedStage(cache, fourth, stage, false);
    if (executions !== 3 || fourth[0]?.status !== "executed") throw new Error("Output corruption self-test failed");
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
  console.log("bpsr-current-build-semantic-refresh self-test passed");
}

function readJson(file, label) { requireFile(file, label); return JSON.parse(readFileSync(file, "utf8")); }
function requireFile(file, label) { if (!existsSync(file)) throw new Error(`Missing ${label}: ${file}`); }
function required(value, key) { if (!value[key]) throw new Error(`Missing --${key}`); return String(value[key]); }
function resolvePath(value) { return path.isAbsolute(value) ? path.normalize(value) : path.resolve(repoRoot, value); }
function relativePath(value) { return path.relative(repoRoot, value).replaceAll("\\", "/"); }
function sha256(file) { return createHash("sha256").update(readFileSync(file)).digest("hex"); }
function parseArgs(args) {
  const output = {};
  for (let index = 0; index < args.length; index += 2) {
    const token = args[index];
    if (!token?.startsWith("--")) throw new Error(`Unexpected argument ${token}`);
    const next = args[index + 1];
    if (!next || next.startsWith("--")) throw new Error(`Missing value for ${token}`);
    output[token.slice(2)] = next;
  }
  return output;
}

function usage(exitCode) {
  console.log(`Usage:
  node tools/bpsr-current-build-semantic-refresh.mjs verify --config <json> --build <id> --build-root <path> --extractor-root <path> --decoded-root <path>
  node tools/bpsr-current-build-semantic-refresh.mjs refresh --config <json> --build <id> --build-root <path> --extractor-root <path> --decoded-root <path>
  node tools/bpsr-current-build-semantic-refresh.mjs rebuild --config <json> --build <id> --build-root <path> --extractor-root <path> --decoded-root <path>
  node tools/bpsr-current-build-semantic-refresh.mjs self-test

Regenerates the build-locked current origin ledger, static rDPS worklist, magnitude watchlist, semantic audit,
raw CTB-to-decoded-table identity proof, semantic mechanic dependency closure, formula-gap ledger, recipient-scope ledger,
matching-build protocol-pack status, and
preflight inventory as one rollback-safe transaction. Refresh reuses exact content-addressed
stages; rebuild forces every stage for periodic cache verification. Steam depot manifests only select
changed physical files; exact semantic hashes and these regenerated ledgers decide what must
be re-proven.`);
  process.exit(exitCode);
}
