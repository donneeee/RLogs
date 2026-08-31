#!/usr/bin/env node

import { createHash } from "node:crypto";
import { existsSync, mkdirSync, readFileSync, statSync, writeFileSync } from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const scriptDir = path.dirname(fileURLToPath(import.meta.url));
const repoRoot = path.resolve(scriptDir, "..");
const requiredTables = [
  "FightAttrTranTable.json",
  "DamageAttrTable.json",
  "BulletTable.json",
  "BuffTable.json",
];

const [command = "help", ...rest] = process.argv.slice(2);
const options = parseArgs(rest);

if (command === "generate") generate(options);
else if (command === "self-test") selfTest();
else usage(command === "help" ? 0 : 1);

function generate(options) {
  const previousPath = resolvePath(required(options, "previous"));
  const diffPath = resolvePath(required(options, "diff"));
  const combatSurfacePath = resolvePath(required(options, "combat-surface"));
  const protobufPath = resolvePath(required(options, "protobuf-wire"));
  const useSkillPath = resolvePath(required(options, "use-skill-envelope"));
  const distributionPath = resolvePath(required(options, "distribution-snapshot"));
  const decodedRoot = resolvePath(required(options, "decoded-root"));
  const outputPath = resolvePath(required(options, "output"));
  const buildId = String(required(options, "build"));

  const previous = readJson(previousPath);
  const directDiff = readJson(diffPath);
  const combatSurface = readJson(combatSurfacePath);
  const protobuf = readJson(protobufPath);
  const useSkill = readJson(useSkillPath);
  const distribution = readJson(distributionPath);

  validateInputs({
    previous,
    directDiff,
    combatSurface,
    protobuf,
    useSkill,
    distribution,
    decodedRoot,
    buildId,
  });

  const tableProofs = requiredTables.map((tableName) => {
    const tablePath = path.join(decodedRoot, tableName);
    return {
      table_name: tableName,
      relative_source: relativeRepo(tablePath),
      bytes: statSync(tablePath).size,
      sha256: hashFile(tablePath),
      semantic_state: "byte-identical-decoded-table-across-builds",
    };
  });

  const copiedProofs = previous.proofs.map((proof) => ({
    ...proof,
    carry_forward_state: "historical-packet-proof-with-current-static-native-and-wire-identity",
    current_build_runtime_enabled: false,
  }));

  const output = {
    schema_version: 2,
    generated_by: "tools/bpsr-formula-proof-carry-forward.mjs",
    game: "blue-protocol-star-resonance",
    deployment_id: "global",
    channel: "steam",
    build_id: buildId,
    supersedes: relativeRepo(previousPath),
    purpose: "Carry exact historical packet formula proofs onto a new client build only when the relevant decoded tables and current native/protobuf contracts are exact, while keeping runtime attribution fail-closed until matching-build packet replay exists.",
    policy: {
      historical_packet_proof_is_retained: true,
      byte_identical_static_tables_are_current_build_evidence: true,
      matching_build_native_identity_is_current_build_evidence: true,
      matching_build_protobuf_wire_identity_is_current_build_evidence: true,
      static_or_native_identity_does_not_prove_packet_occurrence_or_formula_application: true,
      runtime_promotion_allowed: false,
      unresolved_evidence_is_never_hidden: true,
      current_build_packet_replay_required: true,
    },
    current_client_identity: {
      distribution: {
        artifact: relativeRepo(distributionPath),
        app_id: distribution.app.appId,
        build_id: distribution.app.buildId,
        installed_depots: distribution.installedDepots,
        routing_fingerprint_sha256: distribution.routingFingerprintSha256,
      },
      game_assembly: normalizeIdentity(combatSurface.source_identity.game_assembly),
      metadata: normalizeIdentity(combatSurface.source_identity.metadata),
      il2cpp_dump: normalizeIdentity(combatSurface.source_identity.il2cpp_dump),
    },
    current_native_proofs: {
      combat_surface: {
        artifact: relativeRepo(combatSurfacePath),
        requested_types: combatSurface.summary.requested_types,
        resolved_types: combatSurface.summary.resolved_types,
        methods: combatSurface.summary.methods,
        fight_attribute_values: combatSurface.summary.fight_attribute_values,
        fight_attribute_families: combatSurface.summary.fight_attribute_families,
      },
      protobuf_wire: {
        artifact: relativeRepo(protobufPath),
        messages_requested: protobuf.summary.messages_requested,
        messages_exact: protobuf.summary.messages_exact,
        fields_requested: protobuf.summary.fields_requested,
        exact_field_tags: protobuf.summary.exact_field_tags,
        unresolved_field_tags: protobuf.summary.unresolved_field_tags,
      },
      use_skill_attribute_route: {
        artifact: relativeRepo(useSkillPath),
        ...useSkill.promotion_state,
      },
    },
    current_static_surface: {
      direct_table_diff: relativeRepo(diffPath),
      baseline_build_id: String(directDiff.baseline_build_id),
      build_id: String(directDiff.build_id),
      unchanged_tables: directDiff.summary.unchanged_tables,
      unchanged_rows: directDiff.summary.unchanged_rows,
      changed_tables: directDiff.summary.changed_tables,
      changed_rows: directDiff.summary.changed_rows,
      added_rows: directDiff.summary.added_rows,
      removed_rows: directDiff.summary.removed_rows,
      tables: tableProofs,
    },
    proofs: copiedProofs,
    promotion_blockers: [
      "No matching-build packet replay proves status occurrence, provider and recipient lifecycle, reversible attribute magnitudes, or damage-stage conservation.",
      "Static, native, and protobuf identity cannot replace matching-build packet formula evidence.",
    ],
  };

  writeJson(outputPath, output);
  console.log(`Carried ${copiedProofs.length} exact historical packet proof(s) to static build ${buildId}.`);
  console.log(`Validated ${tableProofs.length} unchanged formula tables and ${directDiff.summary.unchanged_rows} unchanged rows.`);
  console.log("Runtime attribution remains disabled pending matching-build packet replay.");
  console.log(`Wrote ${relativeRepo(outputPath)}`);
}

function validateInputs(input) {
  const { previous, directDiff, combatSurface, protobuf, useSkill, distribution, decodedRoot, buildId } = input;
  assert(previous.schema_version === 2, "Previous carry-forward schema must be 2");
  assert(Array.isArray(previous.proofs) && previous.proofs.length > 0, "Previous carry-forward has no packet proofs");
  assert(String(directDiff.build_id) === buildId, "Direct-table diff build mismatch");
  assert(String(combatSurface.build_id) === buildId, "Combat-surface build mismatch");
  assert(String(protobuf.game_build) === buildId, "Protobuf proof build mismatch");
  assert(String(useSkill.game_build) === buildId, "Use-skill proof build mismatch");
  assert(String(distribution.app?.buildId) === buildId, "Steam distribution build mismatch");
  assert(directDiff.summary.baseline_tables === requiredTables.length, "Direct-table diff baseline is incomplete");
  assert(directDiff.summary.candidate_tables === requiredTables.length, "Direct-table diff candidate is incomplete");
  assert(directDiff.summary.unchanged_tables === requiredTables.length, "Not every formula table is unchanged");
  for (const field of ["changed_tables", "added_tables", "removed_tables", "changed_rows", "added_rows", "removed_rows"]) {
    assert(directDiff.summary[field] === 0, `Direct-table diff ${field} must be zero`);
  }
  assert(combatSurface.summary.requested_types === combatSurface.summary.resolved_types, "Current native combat surface is incomplete");
  assert(protobuf.summary.messages_requested === protobuf.summary.messages_exact, "Current protobuf message proof is incomplete");
  assert(protobuf.summary.fields_requested === protobuf.summary.exact_field_tags, "Current protobuf field proof is incomplete");
  assert(protobuf.summary.unresolved_field_tags === 0, "Current protobuf field proof contains unresolved tags");
  assert(useSkill.promotion_state.static_action_contract_exact === true, "Use-skill action contract is not exact");
  assert(useSkill.promotion_state.current_build_service_id_exact === true, "Use-skill service identity is not exact");
  assert(useSkill.promotion_state.complete_static_route_exact === true, "Use-skill static route is not exact");
  assert(useSkill.promotion_state.matching_build_packet_replay_exact === false, "This carry-forward tool must not replace a matching-build runtime proof");
  assert(useSkill.promotion_state.runtime_route_enabled === false, "Runtime route must remain fail-closed");

  const identities = [combatSurface.source_identity, protobuf.source_identity, useSkill.source_identity];
  const gameAssemblyHashes = new Set(identities.map((identity) => identity.game_assembly.sha256));
  const metadataHashes = new Set(identities.map((identity) => identity.metadata.sha256));
  const dumpHashes = new Set(identities.map((identity) => identity.il2cpp_dump.sha256));
  assert(gameAssemblyHashes.size === 1, "Current GameAssembly identities disagree");
  assert(metadataHashes.size === 1, "Current metadata identities disagree");
  assert(dumpHashes.size === 1, "Current IL2CPP dump identities disagree");

  for (const tableName of requiredTables) {
    const tablePath = path.join(decodedRoot, tableName);
    assert(existsSync(tablePath) && statSync(tablePath).size > 0, `Missing decoded formula table ${tableName}`);
  }
}

function selfTest() {
  const fixture = {
    app: { buildId: "2" },
    installedDepots: [],
    routingFingerprintSha256: "abc",
  };
  assert(normalizeIdentity({ byte_length: 4, sha256: "a" }).bytes === 4, "Identity normalization failed");
  assert(fixture.app.buildId === "2", "Fixture failed");
  console.log("bpsr-formula-proof-carry-forward self-test passed");
}

function normalizeIdentity(value) {
  return {
    bytes: value.byte_length ?? value.bytes,
    sha256: value.sha256,
    ...(value.metadata_version === undefined ? {} : { metadata_version: value.metadata_version }),
  };
}

function parseArgs(args) {
  const output = {};
  for (let index = 0; index < args.length; index += 1) {
    const token = args[index];
    if (!token.startsWith("--")) throw new Error(`Unexpected argument ${token}`);
    const key = token.slice(2);
    const value = args[index + 1];
    if (!value || value.startsWith("--")) throw new Error(`Missing value for --${key}`);
    output[key] = value;
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

function readJson(filePath) {
  return JSON.parse(readFileSync(filePath, "utf8"));
}

function hashFile(filePath) {
  return createHash("sha256").update(readFileSync(filePath)).digest("hex");
}

function relativeRepo(filePath) {
  return path.relative(repoRoot, filePath).replaceAll("\\", "/");
}

function writeJson(filePath, value) {
  mkdirSync(path.dirname(filePath), { recursive: true });
  writeFileSync(filePath, `${JSON.stringify(value, null, 2)}\n`, "utf8");
}

function assert(condition, message) {
  if (!condition) throw new Error(message);
}

function usage(code) {
  console.log("Usage:");
  console.log("  node tools/bpsr-formula-proof-carry-forward.mjs generate --previous <json> --diff <json> --combat-surface <json> --protobuf-wire <json> --use-skill-envelope <json> --distribution-snapshot <json> --decoded-root <dir> --build <id> --output <json>");
  console.log("  node tools/bpsr-formula-proof-carry-forward.mjs self-test");
  process.exit(code);
}
