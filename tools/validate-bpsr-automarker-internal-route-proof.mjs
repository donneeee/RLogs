import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import { fileURLToPath } from "node:url";

const proofUrl = new URL(
  "../plugins/games/blue-protocol-star-resonance/research/game-file-inventory/global/steam-25247556/ground-marker-internal-game-owned-route-design-proof.v1.json",
  import.meta.url,
);
const proof = JSON.parse(await readFile(fileURLToPath(proofUrl), "utf8"));
const dungeonProofUrl = new URL(
  "../plugins/games/blue-protocol-star-resonance/research/game-file-inventory/global/steam-25247556/automarker-read-only-dungeon-stage-proof.v1.json",
  import.meta.url,
);
const dungeonProof = JSON.parse(await readFile(fileURLToPath(dungeonProofUrl), "utf8"));

assert.equal(proof.schema_version, 1);
assert.equal(proof.build_id, "25247556");
assert.equal(proof.build_identity.game_assembly.sha256, "4a079aec0a3e51a8023aa86bbd152e12068907b65b9aafb355d20bb6d6c41fe3");
assert.equal(proof.build_identity.global_metadata.sha256, "2f8213cf2a253d7a99a37b81f796e3a0162d0cf2d1ca6a2d0e0719f8f9a57779");
assert.equal(proof.abi_proof.set_indicator_position.rva, "0x53E86A0");
assert.equal(proof.abi_proof.fire_indicator_skill.rva, "0x52E09E0");
assert.equal(proof.abi_proof.vector3_layout.size_bytes, 12);
assert.match(proof.abi_proof.set_indicator_position.register_contract.r8, /passes null/);
assert.match(proof.abi_proof.fire_indicator_skill.register_contract.r8, /hidden MethodInfo ABI slot/);
assert.equal(proof.overwrite_reconciliation.normal_flag_skill_release.set_indicator_pos_callsite_rva, "0x41B80B1");
assert.equal(proof.overwrite_reconciliation.direct_indicator_dispatch_candidate.calls_z_indicator_mgr_fire_skill, false);
assert.equal(proof.overwrite_reconciliation.direct_indicator_dispatch_candidate.calls_set_indicator_pos, false);
assert.equal(proof.overwrite_reconciliation.direct_indicator_dispatch_candidate.calls_reset_indicator_pos, false);
assert.equal(proof.overwrite_reconciliation.direct_indicator_dispatch_candidate.calls_set_select_point, false);
assert.equal(proof.overwrite_reconciliation.direct_indicator_dispatch_candidate.fire_play_skill_event_arguments.hidden_method_info, null);
assert.equal(proof.remote_coordinate_boundary.manual_reticle_limit_bypassed_by_candidate, true);
assert.equal(proof.remote_coordinate_boundary.server_distance_rejection_excluded, false);
assert.equal(proof.remote_coordinate_boundary.party_visible_remote_placement_proven, false);
assert.equal(proof.next_non_activating_canary.activation_after_preflight, false);
assert.deepEqual(proof.pacing_and_acknowledgement.skills, [1101, 1102, 1103, 1104, 1105, 1106]);
assert.deepEqual(proof.pacing_and_acknowledgement.slots, [201, 202, 203, 204, 205, 206]);

for (const denied of ["process_access", "game_method_invocation", "memory_write", "packet_synthesis_or_transmission", "runtime_activation_enabled"]) {
  assert.equal(proof.scope[denied], false, `${denied} must stay disabled`);
}
assert.equal(proof.decision.safe_main_thread_invocation_proven, false);
assert.equal(proof.decision.complete_runtime_gating_proven, false);
assert.equal(proof.decision.native_placement_must_remain_disabled, true);
assert.equal(proof.privacy.contains_private_paths, false);
assert.equal(proof.privacy.contains_absolute_native_addresses, false);

const serialized = JSON.stringify(proof);
assert.doesNotMatch(serialized, /[A-Z]:\\|\\\\[A-Za-z0-9._-]+\\/i, "proof must not contain private absolute paths");
assert.ok(proof.unresolved_risks.length >= 6, "proof must retain explicit unresolved risks");

assert.equal(dungeonProof.schema_version, 1);
assert.equal(dungeonProof.build_id, "25247556");
assert.equal(dungeonProof.proof_kind, "exact-build-read-only-dungeon-stage-preflight");
assert.equal(dungeonProof.inputs.game_assembly.sha256, proof.build_identity.game_assembly.sha256);
assert.equal(dungeonProof.inputs.recovered_metadata.sha256, proof.build_identity.global_metadata.sha256);
assert.equal(dungeonProof.stage_singleton.method_info_slot_rva_hex, "0x95D6B50");
assert.equal(dungeonProof.stage_singleton.type_info_slot_rva_hex, "0x95D6B68");
assert.deepEqual(
  dungeonProof.field_layouts.map(({ instance_offset_hex }) => instance_offset_hex),
  ["0x18", "0x20", "0x10"],
);
assert.equal(dungeonProof.enum_values["Panda.ESwitchState.ENone"], 0);
assert.equal(dungeonProof.enum_values["Panda.EStageType.Dungeon"], 5);
assert.equal(dungeonProof.generated_layout_evidence.stage_base_field_declaration, "uint8_t _Stage_k__BackingField");
assert.equal(dungeonProof.generated_layout_evidence.stage_enum_underlying_declaration, "uint8_t value__");
assert.equal(dungeonProof.generated_layout_evidence.switch_enum_underlying_declaration, "uint8_t value__");
assert.equal(dungeonProof.generated_layout_evidence.stage_getter.rva_hex, "0xBCDE50");
assert.equal(dungeonProof.generated_layout_evidence.stage_getter.generated_c_return_type, "uint8_t");
assert.equal(dungeonProof.stage_singleton.independent_native_slot_references.length, 3);
assert.equal(dungeonProof.runtime_acceptance_contract.length, 5);
assert.equal(dungeonProof.sanitized_runtime_observations.length, 1);
const mechFacilityObservation = dungeonProof.sanitized_runtime_observations[0];
assert.equal(mechFacilityObservation.receipt_schema_version, 8);
assert.match(mechFacilityObservation.receipt_sha256, /^[0-9a-f]{64}$/);
assert.equal(mechFacilityObservation.scene_id, 6525);
assert.equal(mechFacilityObservation.map_id, 6525);
assert.equal(mechFacilityObservation.activity_family_id, "mech-facility");
assert.equal(mechFacilityObservation.exact_image_identity, true);
assert.equal(mechFacilityObservation.root_chain_class_valid, true);
assert.equal(mechFacilityObservation.root_chain_stable, true);
assert.equal(mechFacilityObservation.lifecycle_idle, true);
assert.deepEqual(mechFacilityObservation.dungeon_stage_gate, {
  proven: true,
  reason: "proven-read-only-current-dungeon-stage",
});
assert.equal(mechFacilityObservation.activation_attempted, false);
assert.equal(dungeonProof.scope.process_memory_write, false);
assert.equal(dungeonProof.scope.game_method_invocation, false);
assert.equal(dungeonProof.scope.runtime_activation_enabled, false);
assert.equal(dungeonProof.privacy.contains_private_paths, false);
assert.equal(dungeonProof.privacy.contains_process_addresses, false);
assert.doesNotMatch(
  JSON.stringify(dungeonProof),
  /[A-Z]:\\|\\\\[A-Za-z0-9._-]+\\/i,
  "dungeon-stage proof must not contain private absolute paths",
);

console.log("validated exact-build automarker internal-route proof");
