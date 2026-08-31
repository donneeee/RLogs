#!/usr/bin/env node

import { createHash } from "node:crypto";
import { readFileSync, writeFileSync } from "node:fs";
import path from "node:path";

const [command = "help", ...rest] = process.argv.slice(2);
const options = parseArgs(rest);

if (command === "build") build(options);
else if (command === "verify") verifyFile(required(options, "input"));
else if (command === "self-test") selfTest();
else usage(command === "help" ? 0 : 1);

function build(options) {
  const gameBuild = required(options, "build");
  const factorClosurePath = required(options, "factor-closure");
  const runtimeCatalogPath = required(options, "runtime-catalog");
  const captureProofPath = required(options, "capture-proof");
  const treeNodesPath = required(options, "tree-nodes");
  const outputPath = required(options, "output");

  const factorClosure = readJson(factorClosurePath);
  const runtimeCatalog = readJson(runtimeCatalogPath);
  const captureProof = readJson(captureProofPath);
  const treeNodes = readJson(treeNodesPath);
  requireBuild(factorClosure, gameBuild, "factor closure");
  requireBuild(runtimeCatalog, gameBuild, "Dreamscope runtime catalog");
  requireBuild(captureProof, gameBuild, "factor capture proof");
  if (!Array.isArray(treeNodes)) throw new Error("Tree-node input must be an array");

  const artifact = createArtifact({
    gameBuild,
    factorClosure,
    runtimeCatalog,
    captureProof,
    treeNodes,
    inputs: {
      factor_closure: describeInput(factorClosurePath),
      runtime_catalog: describeInput(runtimeCatalogPath),
      capture_proof: describeInput(captureProofPath),
      tree_nodes: describeInput(treeNodesPath),
    },
  });
  artifact.content_sha256 = contentHash(artifact);
  writeFileSync(outputPath, `${JSON.stringify(artifact, null, 2)}\n`);
  verifyArtifact(artifact);
  console.log(`Dreamscope runtime fingerprint proof written: ${outputPath}`);
  printSummary(artifact.summary);
}

function createArtifact({ gameBuild, factorClosure, runtimeCatalog, captureProof, treeNodes, inputs }) {
  const currentFamilies = factorClosure.families
    .filter((family) => family.current_runtime_eligible)
    .sort((a, b) => a.family_id - b.family_id);
  const captureByFamily = new Map(
    captureProof.blocker_obligations.map((entry) => [entry.factor_identity.family_id, entry]),
  );
  const effectOwners = new Map();
  for (const family of currentFamilies) {
    for (const effectId of family.source_buff_ids) {
      const owners = effectOwners.get(effectId) ?? [];
      owners.push(family.family_id);
      effectOwners.set(effectId, owners);
    }
  }

  const factorFingerprints = currentFamilies.map((family) => {
    const capture = captureByFamily.get(family.family_id);
    const parameterSignatures = unique(family.grade_routes.map((route) => stableString(route.parameter_values)));
    const energySignatures = unique(family.grade_routes.map((route) => stableString({
      behavior: route.energy_behavior,
      amount: route.energy_amount,
    })));
    const effectFamilyCandidates = unique(
      family.source_buff_ids.flatMap((effectId) => effectOwners.get(effectId) ?? []),
    );
    const exactFamilyFromEffect = family.source_buff_ids.length > 0
      && effectFamilyCandidates.length === 1
      && effectFamilyCandidates[0] === family.family_id;
    const gradeCoefficientEquivalent = parameterSignatures.length <= 1;
    const exactGradeWithoutSnapshot = family.grade_routes.length === 1;
    const gradeState = exactGradeWithoutSnapshot
      ? "exact-single-grade"
      : gradeCoefficientEquivalent
        ? "coefficient-equivalent-across-candidate-grades"
        : "requires-packet-magnitude-energy-selector-or-exact-selection";
    return {
      family_id: family.family_id,
      family_name: family.family_name,
      slot_category: family.slot_category,
      runtime_role: family.runtime_role,
      class_gate_ids: family.class_gate_ids,
      source_effect_ids: family.source_buff_ids,
      family_resolution: exactFamilyFromEffect
        ? "exact-family-from-effect"
        : family.source_buff_ids.length === 0
          ? "exact-family-requires-attribute-state-route"
          : "bounded-family-candidates-from-effect",
      effect_family_candidates: effectFamilyCandidates,
      candidate_grades: family.grade_routes.map((route) => ({
        grade: route.grade,
        item_id: route.item_id,
        source_effect_id: route.source_buff_id,
        parameter_values: route.parameter_values,
        energy_behavior: route.energy_behavior,
        energy_amount: route.energy_amount,
        resolved_description: route.resolved_description,
      })),
      grade_resolution: {
        state: gradeState,
        parameter_signatures: parameterSignatures.length,
        energy_signatures: energySignatures.length,
        coefficient_equivalent_across_candidates: gradeCoefficientEquivalent,
        effect_only_is_formula_safe: exactGradeWithoutSnapshot || gradeCoefficientEquivalent,
        accepted_exact_discriminators: [
          "packet-encoded-parameter-magnitude",
          "packet-visible-energy-transition-unique-to-one-grade",
          "runtime-selector-unique-to-one-grade",
          "triggered-output-unique-to-one-grade",
          "exact-owner-selection-snapshot-at-capture-time",
        ],
      },
      mechanic_classes: family.mechanic_classes,
      exact_skill_ids: family.exact_skill_ids,
      exact_recount_ids: family.exact_recount_ids,
      direct_damage_ids: family.direct_damage_ids,
      generated_damage_families: family.generated_damage_families,
      generated_output_families: family.generated_output_families,
      state_routes: family.state_routes,
      runtime_selectors: family.runtime_selectors,
      capture_coverage: capture ? {
        state: capture.coverage_state,
        selection_observations: capture.selection_observations.length,
        lifecycle_windows: capture.lifecycle_windows.length,
        exact_owner_bindings: capture.exact_owner_bindings.length,
        emitted_action_matches: capture.emitted_action_matches.length,
        distinct_provider_recipient_windows:
          capture.provider_recipient_evidence.distinct_provider_recipient_windows,
      } : {
        state: "missing-capture-obligation",
        selection_observations: 0,
        lifecycle_windows: 0,
        exact_owner_bindings: 0,
        emitted_action_matches: 0,
        distinct_provider_recipient_windows: 0,
      },
      rdps_state: "not-promoted-runtime-proof-open",
      hidden_omissions: 0,
    };
  });

  const treeFingerprints = treeNodes
    .slice()
    .sort((a, b) => a.node_id - b.node_id)
    .map((node) => createTreeFingerprint(node, runtimeCatalog));
  const duplicateEffectIds = [...effectOwners.entries()]
    .filter(([, owners]) => unique(owners).length > 1)
    .map(([effectId, owners]) => ({ effect_id: effectId, family_ids: unique(owners) }));
  const familyStates = countBy(factorFingerprints, (entry) => entry.family_resolution);
  const gradeStates = countBy(factorFingerprints, (entry) => entry.grade_resolution.state);
  const treeIdentityStates = countBy(treeFingerprints, (entry) => entry.identity_state);
  const externallyTransferable = treeFingerprints.filter((entry) => entry.external_recipient_candidate);

  return {
    schema_version: 1,
    generated_by: "tools/bpsr-dreamscope-runtime-fingerprint-proof.mjs",
    game: "Blue Protocol: Star Resonance",
    game_build: gameBuild,
    proof_state: "family-fingerprints-exact-grade-and-recipient-proof-open",
    policy: {
      packet_observation_is_authoritative: true,
      full_remote_build_snapshot_required: false,
      exact_active_mechanic_inference_allowed: true,
      effect_id_may_select_family_but_not_grade: true,
      grade_is_never_guessed: true,
      descriptions_only_create_candidates: true,
      provider_recipient_lifecycle_required_before_rdps_promotion: true,
      unresolved_evidence_is_preserved: true,
      hidden_omissions_allowed: false,
    },
    inputs,
    runtime_contract: {
      factor_family_key: "source status-effect ID when unique; otherwise attribute/state transition",
      grade_key: "packet magnitude, energy delta, runtime selector, triggered output, or capture-time owner snapshot",
      tree_node_key: "unique packet terminal effect ID",
      attribution_key: "provider + recipient + effect instance + lifecycle window + dependent damage event",
      snapshot_role: "optional exact enrichment, not a prerequisite for active-family inference",
    },
    summary: {
      current_factor_families: factorFingerprints.length,
      current_factor_effect_ids: effectOwners.size,
      duplicate_factor_effect_ids: duplicateEffectIds.length,
      factor_family_resolution_states: familyStates,
      factor_grade_resolution_states: gradeStates,
      current_tree_nodes: treeFingerprints.length,
      tree_identity_states: treeIdentityStates,
      external_recipient_tree_candidates: externallyTransferable.length,
      offensive_external_tree_candidates: externallyTransferable.filter((entry) => entry.rdps_relevance_candidate).length,
      runtime_rdps_promotions: 0,
      hidden_omissions: 0,
    },
    duplicate_factor_effect_ids: duplicateEffectIds,
    factor_fingerprints: factorFingerprints,
    tree_fingerprints: treeFingerprints,
    still_required_runtime_gates: [
      "packet-visible-grade-discriminator-for-grade-dependent-coefficients",
      "provider-recipient-effect-lifecycle-window",
      "dependent-damage-event-in-provider-recipient-window",
      "integer-counterfactual-projection",
      "party-damage-conservation",
    ],
  };
}

function createTreeFingerprint(node, runtimeCatalog) {
  const buffIds = unique(node.buff_ids ?? []);
  const candidateNodes = unique(buffIds.flatMap((effectId) =>
    (runtimeCatalog.candidates_by_terminal_effect_id[String(effectId)] ?? [])
      .filter((candidate) => candidate.source_kind === "tree_node")
      .map((candidate) => candidate.source_id)));
  const identityState = buffIds.length > 0 && candidateNodes.length === 1
    && candidateNodes[0] === node.node_id
    ? "exact-tree-node-from-effect"
    : buffIds.length === 0
      ? "tree-node-requires-state-route"
      : "bounded-tree-node-candidates-from-effect";
  const description = node.english_description ?? "";
  const mediatedExternalNodes = new Set([1702, 1704]);
  const externalRecipientCandidate = mediatedExternalNodes.has(node.node_id)
    || /\b(all(?:y|ies)|teammates?|party|enemy targets?|enemies within|nearby enemies)\b/i.test(description);
  const rdpsRelevanceCandidate = externalRecipientCandidate
    && /\b(damage|dmg|attack|main stat|all-element|vulnerab|cooldown|expertise|crit|lucky)\b/i.test(description);
  return {
    node_id: node.node_id,
    name: node.english_name,
    template_id: runtimeCatalog.tree_nodes_by_id[String(node.node_id)]?.template_id ?? null,
    terminal_effect_ids: buffIds,
    identity_state: identityState,
    candidate_node_ids: candidateNodes,
    english_description: description,
    effect_component_keys: node.effect_component_keys ?? [],
    damage_ids: node.damage_ids ?? [],
    recount_ids: node.recount_ids ?? [],
    prior_attribution_status: node.attribution_status,
    external_recipient_candidate: externalRecipientCandidate,
    external_recipient_evidence: mediatedExternalNodes.has(node.node_id)
      ? "shared-Endless-Mind-stack-modifier-with-current-teammate-transfer-node-1707"
      : externalRecipientCandidate
        ? "exact-current-English-description-names-non-self-recipient"
        : "none-yet",
    rdps_relevance_candidate: rdpsRelevanceCandidate,
    runtime_promotion_ready: false,
    still_required: externalRecipientCandidate
      ? ["provider-recipient-effect-lifecycle-window", "dependent-damage-event-correlation"]
      : ["runtime-recipient-scope-observation"],
  };
}

function verifyFile(inputPath) {
  const artifact = readJson(inputPath);
  verifyArtifact(artifact);
  console.log(`Dreamscope runtime fingerprint proof verified: ${inputPath}`);
  printSummary(artifact.summary);
}

function verifyArtifact(artifact) {
  if (artifact.schema_version !== 1) throw new Error("Expected schema_version 1");
  if (artifact.generated_by !== "tools/bpsr-dreamscope-runtime-fingerprint-proof.mjs") {
    throw new Error("Unexpected generator");
  }
  if (artifact.content_sha256 && artifact.content_sha256 !== contentHash(artifact)) {
    throw new Error("Content SHA-256 mismatch");
  }
  const summary = artifact.summary;
  if (summary.current_factor_families !== artifact.factor_fingerprints.length) {
    throw new Error("Factor summary count mismatch");
  }
  if (summary.current_tree_nodes !== artifact.tree_fingerprints.length) {
    throw new Error("Tree summary count mismatch");
  }
  if (summary.duplicate_factor_effect_ids !== artifact.duplicate_factor_effect_ids.length) {
    throw new Error("Duplicate effect summary mismatch");
  }
  if (summary.runtime_rdps_promotions !== 0 || summary.hidden_omissions !== 0) {
    throw new Error("Fingerprint proof cannot promote or hide rDPS evidence");
  }
  for (const factor of artifact.factor_fingerprints) {
    if (factor.hidden_omissions !== 0 || factor.rdps_state !== "not-promoted-runtime-proof-open") {
      throw new Error(`Factor ${factor.family_id} violates proof-only policy`);
    }
    if (factor.family_resolution === "exact-family-from-effect"
      && factor.source_effect_ids.length === 0) {
      throw new Error(`Factor ${factor.family_id} has no effect for exact-effect resolution`);
    }
    if (!factor.grade_resolution.coefficient_equivalent_across_candidates
      && factor.grade_resolution.effect_only_is_formula_safe) {
      throw new Error(`Factor ${factor.family_id} incorrectly treats effect-only grade as formula safe`);
    }
  }
  const requiredTreeNodes = new Set([1506, 1507, 1701, 1702, 1704, 1707]);
  for (const nodeId of requiredTreeNodes) {
    const node = artifact.tree_fingerprints.find((entry) => entry.node_id === nodeId);
    if (!node || !node.external_recipient_candidate) {
      throw new Error(`Expected current external-recipient candidate tree node ${nodeId}`);
    }
  }
}

function selfTest() {
  const artifact = createArtifact({
    gameBuild: "1",
    factorClosure: { families: [
      syntheticFamily(1, [100], [[10], [20]]),
      syntheticFamily(2, [], [[], []]),
    ] },
    runtimeCatalog: {
      tree_nodes_by_id: { "1506": { template_id: 6 } },
      candidates_by_terminal_effect_id: {
        "300": [{ source_kind: "tree_node", source_id: 1506 }],
      },
    },
    captureProof: { blocker_obligations: [syntheticCapture(1), syntheticCapture(2)] },
    treeNodes: [{
      node_id: 1506,
      english_name: "Harmony Grace",
      buff_ids: [300],
      english_description: "Increases allies' main stats.",
      effect_component_keys: [], damage_ids: [], recount_ids: [], attribution_status: "open",
    }, { node_id: 1507, english_name: "n", buff_ids: [], english_description: "allies damage", effect_component_keys: [], damage_ids: [], recount_ids: [] },
      { node_id: 1701, english_name: "n", buff_ids: [], english_description: "teammates damage", effect_component_keys: [], damage_ids: [], recount_ids: [] },
      { node_id: 1702, english_name: "n", buff_ids: [], english_description: "party main stat", effect_component_keys: [], damage_ids: [], recount_ids: [] },
      { node_id: 1704, english_name: "n", buff_ids: [], english_description: "allies cooldown", effect_component_keys: [], damage_ids: [], recount_ids: [] },
      { node_id: 1707, english_name: "n", buff_ids: [], english_description: "teammates damage", effect_component_keys: [], damage_ids: [], recount_ids: [] }],
    inputs: {},
  });
  verifyArtifact(artifact);
  if (artifact.factor_fingerprints[0].grade_resolution.effect_only_is_formula_safe) {
    throw new Error("Self-test failed: differing grade coefficients were treated as safe");
  }
  if (artifact.factor_fingerprints[1].family_resolution !== "exact-family-requires-attribute-state-route") {
    throw new Error("Self-test failed: effectless family route was not retained");
  }
  console.log("Dreamscope runtime fingerprint proof self-test passed.");
}

function syntheticFamily(id, effects, parameters) {
  return {
    family_id: id, family_name: `Family ${id}`, slot_category: "test", runtime_role: "test",
    class_gate_ids: [], current_runtime_eligible: true, source_buff_ids: effects,
    grade_routes: parameters.map((parameterValues, index) => ({
      grade: index + 1, item_id: id * 100 + index, source_buff_id: effects[0] ?? 0,
      parameter_values: parameterValues, energy_behavior: "none-observed", energy_amount: null,
      resolved_description: "",
    })),
    mechanic_classes: [], exact_skill_ids: [], exact_recount_ids: [], direct_damage_ids: [],
    generated_damage_families: [], generated_output_families: [], state_routes: [], runtime_selectors: [],
  };
}

function syntheticCapture(familyId) {
  return {
    factor_identity: { family_id: familyId }, coverage_state: "none", selection_observations: [],
    lifecycle_windows: [], exact_owner_bindings: [], emitted_action_matches: [],
    provider_recipient_evidence: { distinct_provider_recipient_windows: 0 },
  };
}

function printSummary(summary) {
  console.log(JSON.stringify(summary, null, 2));
}

function parseArgs(args) {
  const result = {};
  for (let index = 0; index < args.length; index += 1) {
    const token = args[index];
    if (!token.startsWith("--")) throw new Error(`Unexpected argument: ${token}`);
    const key = token.slice(2);
    const value = args[index + 1];
    if (value === undefined || value.startsWith("--")) throw new Error(`Missing value for --${key}`);
    result[key] = value;
    index += 1;
  }
  return result;
}

function required(options, key) {
  if (!options[key]) throw new Error(`Missing required --${key}`);
  return options[key];
}

function readJson(filePath) {
  return JSON.parse(readFileSync(filePath, "utf8"));
}

function requireBuild(input, expected, label) {
  if (String(input.game_build) !== String(expected)) {
    throw new Error(`${label} build ${input.game_build} does not match ${expected}`);
  }
}

function describeInput(filePath) {
  const bytes = readFileSync(filePath);
  return { path: path.resolve(filePath), sha256: createHash("sha256").update(bytes).digest("hex") };
}

function contentHash(value) {
  const clone = structuredClone(value);
  delete clone.content_sha256;
  return createHash("sha256").update(`${JSON.stringify(clone)}\n`).digest("hex");
}

function stableString(value) {
  return JSON.stringify(value);
}

function unique(values) {
  return [...new Set(values)];
}

function countBy(values, selector) {
  const result = {};
  for (const value of values) {
    const key = selector(value);
    result[key] = (result[key] ?? 0) + 1;
  }
  return result;
}

function usage(exitCode) {
  console.log("Usage:");
  console.log("  node tools/bpsr-dreamscope-runtime-fingerprint-proof.mjs build --build <id> --factor-closure <json> --runtime-catalog <json> --capture-proof <json> --tree-nodes <json> --output <json>");
  console.log("  node tools/bpsr-dreamscope-runtime-fingerprint-proof.mjs verify --input <json>");
  console.log("  node tools/bpsr-dreamscope-runtime-fingerprint-proof.mjs self-test");
  process.exit(exitCode);
}
