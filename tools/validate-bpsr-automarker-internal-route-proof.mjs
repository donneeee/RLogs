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
const leaderProofUrl = new URL(
  "../plugins/games/blue-protocol-star-resonance/research/game-file-inventory/global/steam-25247556/automarker-read-only-party-leader-proof.v1.json",
  import.meta.url,
);
const leaderProof = JSON.parse(await readFile(fileURLToPath(leaderProofUrl), "utf8"));
const markerSkillProofUrl = new URL(
  "../plugins/games/blue-protocol-star-resonance/research/game-file-inventory/global/steam-25247556/automarker-read-only-marker-skill-proof.v1.json",
  import.meta.url,
);
const markerSkillProof = JSON.parse(await readFile(fileURLToPath(markerSkillProofUrl), "utf8"));
const schedulerProofUrl = new URL(
  "../plugins/games/blue-protocol-star-resonance/research/game-file-inventory/global/steam-25247556/automarker-static-main-thread-scheduler-proof.v1.json",
  import.meta.url,
);
const schedulerProof = JSON.parse(await readFile(fileURLToPath(schedulerProofUrl), "utf8"));

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

assert.equal(leaderProof.schema_version, 1);
assert.equal(leaderProof.build_id, "25247556");
assert.equal(leaderProof.proof_kind, "exact-build-read-only-current-party-leader-preflight");
assert.equal(leaderProof.inputs.game_assembly.sha256, proof.build_identity.game_assembly.sha256);
assert.equal(leaderProof.inputs.recovered_metadata.sha256, proof.build_identity.global_metadata.sha256);
assert.equal(leaderProof.authoritative_game_predicate.rva_hex, "0x54215C0");
assert.equal(leaderProof.authoritative_game_predicate.semantic_block_rva_hex, "0x5421671");
assert.equal(leaderProof.authoritative_game_predicate.semantic_block_byte_length, 122);
assert.match(leaderProof.authoritative_game_predicate.semantic_block_sha256, /^[0-9a-f]{64}$/);
assert.equal(leaderProof.cache_lookup_semantics.native_try_get.rva_hex, "0x3F54D80");
assert.equal(leaderProof.cache_lookup_semantics.local_attribute_key_hex, "0x80000097");
assert.equal(leaderProof.runtime_acceptance_contract.length, 9);
assert.equal(leaderProof.sanitized_runtime_observations.length, 1);
const leaderObservation = leaderProof.sanitized_runtime_observations[0];
assert.equal(leaderObservation.receipt_schema_version, 8);
assert.equal(leaderObservation.receipt_sha256, "addbaf36139bc0a0c67fb9ee28d688578fcb1287fb108fbf0a60abf9b9613a90");
assert.equal(leaderObservation.scene_id, 6525);
assert.equal(leaderObservation.map_id, 6525);
assert.equal(leaderObservation.activity_family_id, "mech-facility");
assert.deepEqual(leaderObservation.leader_gate, {
  proven: true,
  reason: "proven-read-only-current-player-is-party-leader",
});
assert.equal(leaderObservation.activation_attempted, false);
for (const denied of ["process_memory_write", "game_method_invocation", "runtime_activation_enabled"]) {
  assert.equal(leaderProof.scope[denied], false, `leader proof ${denied} must stay disabled`);
}
assert.equal(leaderProof.privacy.contains_private_paths, false);
assert.equal(leaderProof.privacy.contains_process_addresses, false);
assert.equal(leaderProof.privacy.contains_personal_identity, false);
assert.doesNotMatch(
  JSON.stringify(leaderProof),
  /[A-Z]:\\|\\\\[A-Za-z0-9._-]+\\/i,
  "party-leader proof must not contain private absolute paths",
);

assert.equal(markerSkillProof.schema_version, 1);
assert.equal(markerSkillProof.build_id, "25247556");
assert.equal(markerSkillProof.proof_kind, "exact-build-read-only-marker-skill-resolution-preflight");
assert.equal(markerSkillProof.inputs.game_assembly.sha256, proof.build_identity.game_assembly.sha256);
assert.equal(markerSkillProof.inputs.recovered_metadata.sha256, proof.build_identity.global_metadata.sha256);
assert.deepEqual(
  markerSkillProof.field_layouts.map(({ instance_offset_hex }) => instance_offset_hex),
  ["0x20", "0x18", "0x28", "0x10"],
);
assert.equal(markerSkillProof.concrete_zdictionary_layout.entry_int_int.stride_bytes, 16);
assert.equal(markerSkillProof.concrete_zdictionary_layout.entry_int_object.stride_bytes, 24);
assert.equal(markerSkillProof.runtime_acceptance_contract.length, 7);
assert.deepEqual(markerSkillProof.sanitized_receipt.success, {
  proven: true,
  reason: "proven-read-only-marker-1-slot-and-skill-resolution",
});
assert.equal(markerSkillProof.sanitized_runtime_observations.length, 1);
const markerSkillObservation = markerSkillProof.sanitized_runtime_observations[0];
assert.equal(markerSkillObservation.receipt_schema_version, 8);
assert.equal(markerSkillObservation.receipt_sha256, leaderObservation.receipt_sha256);
assert.equal(markerSkillObservation.scene_id, 6525);
assert.equal(markerSkillObservation.map_id, 6525);
assert.equal(markerSkillObservation.activity_family_id, "mech-facility");
assert.deepEqual(markerSkillObservation.marker_skill_resolution_gate, {
  proven: false,
  reason: "unavailable-or-invalid-read-only-marker-skill-chain",
});
assert.equal(markerSkillObservation.activation_attempted, false);
assert.equal(markerSkillProof.decision.slot_201_to_skill_1101_static_layout_proven, true);
assert.equal(markerSkillProof.decision.non_invoking_live_resolution_probe_implemented, true);
assert.equal(markerSkillProof.decision.game_method_invocation_enabled, false);
assert.equal(markerSkillProof.decision.native_placement_enabled, false);
for (const denied of ["process_memory_write", "game_method_invocation", "packet_synthesis_or_transmission", "runtime_activation_enabled"]) {
  assert.equal(markerSkillProof.scope[denied], false, `marker-skill proof ${denied} must stay disabled`);
}
assert.equal(markerSkillProof.privacy.contains_private_paths, false);
assert.equal(markerSkillProof.privacy.contains_process_addresses, false);
assert.equal(markerSkillProof.privacy.contains_personal_identity, false);
assert.doesNotMatch(
  JSON.stringify(markerSkillProof),
  /[A-Z]:\\|\\\\[A-Za-z0-9._-]+\\/i,
  "marker-skill proof must not contain private absolute paths",
);

assert.equal(schedulerProof.schema_version, 1);
assert.equal(schedulerProof.build_id, "25247556");
assert.equal(schedulerProof.proof_kind, "exact-build-static-main-thread-scheduler-audit");
assert.equal(schedulerProof.inputs.game_assembly.sha256, proof.build_identity.game_assembly.sha256);
assert.equal(schedulerProof.inputs.recovered_metadata.sha256, proof.build_identity.global_metadata.sha256);
assert.deepEqual(schedulerProof.selected_scheduler, {
  identity: "Cysharp.Threading.Tasks.UniTask.Post(System.Action, PlayerLoopTiming)",
  rva_hex: "0x670EB30",
  byte_length: 16,
  sha256: "01311db2ce75535a228e3edd96331773c56b27e08392ed317af9ae05366fcb49",
  generated_c_signature: "void Cysharp_Threading_Tasks_UniTask__Post(System_Action_o* action, int32_t timing, const MethodInfo* method)",
  selected_timing: { name: "Update", value: 8 },
  entry_contract: {
    rcx: "class-valid managed System.Action object",
    edx: "PlayerLoopTiming value 8",
    r8: "hidden MethodInfo slot; this exact method clears it before forwarding",
    native_flow: "Moves timing to ECX and Action to RDX, clears R8, then tail-jumps to PlayerLoopHelper.AddContinuation.",
  },
  classification: "best reviewed one-shot game-main-player-loop queue surface",
  classification_limit: "This selection proves queue mechanics only. It does not supply an external process bridge or construct the required managed callback.",
});
assert.deepEqual(
  schedulerProof.queue_chain.map(({ rva_hex, byte_length, sha256 }) => ({ rva_hex, byte_length, sha256 })),
  [
    { rva_hex: "0x670AFE0", byte_length: 128, sha256: "120e184efe897e07e7ac4f2a10307a1cf115544208ec6f95fd0353822234e0a2" },
    { rva_hex: "0x676A0B0", byte_length: 1392, sha256: "25888277bd929d97b136cdb9bcc99b57e523e933e644588b70af27b51d9c52fa" },
    { rva_hex: "0x676A620", byte_length: 16, sha256: "7142a15a89818528d3373816863921c29555c736474721e04257bb196275c50c" },
    { rva_hex: "0x676A630", byte_length: 960, sha256: "5dca8245396e0fa60f0f7f74ad34ca76d511e269c3cbfae71fda909440e9a9cd" },
  ],
);
assert.equal(schedulerProof.player_loop_layout.type_info_pointer_slot_rva_hex, "0x9591498");
assert.equal(schedulerProof.player_loop_layout.static_fields.yielders_offset_hex, "0x18");
assert.equal(schedulerProof.player_loop_layout.continuation_queue_instance_fields.timing_offset_hex, "0x10");
assert.equal(schedulerProof.managed_callback_boundary.system_action_constructor.shared_rva_hex, "0xB38650");
assert.match(
  schedulerProof.managed_callback_boundary.system_action_constructor.native_observations.join(" "),
  /arbitrary native code pointer is not a valid constructor argument/,
);
assert.equal(schedulerProof.managed_callback_boundary.supported_external_ipc_or_plugin_entry_found, false);
assert.equal(schedulerProof.managed_callback_boundary.precompiled_automarker_managed_callback_found, false);
assert.equal(schedulerProof.candidate_comparison.length, 7);
assert.deepEqual(
  schedulerProof.candidate_comparison.map(({ identity, status, one_shot }) => ({ identity, status, one_shot })),
  [
    {
      identity: "Cysharp.Threading.Tasks.UniTask.Post(System.Action, PlayerLoopTiming)",
      status: "selected-static-candidate",
      one_shot: true,
    },
    {
      identity: "Panda.Utility.ZTaskUtils.NextFrameForLua(PlayerLoopTiming, uint, Action, Action<Exception>)",
      status: "rejected-more-stateful-wrapper",
      one_shot: true,
    },
    {
      identity: "UnityEngine.UnitySynchronizationContext.Post(SendOrPostCallback, object)",
      status: "rejected-no-bridge-advantage",
      one_shot: true,
    },
    {
      identity: "ZenFulcrum.EmbeddedBrowser.Browser.RunOnMainThread(Action)",
      status: "rejected-component-owned",
      one_shot: true,
    },
    {
      identity: "DreamMaker.Event.EPFlowCommonEventMgr dispatch queue",
      status: "rejected-domain-event-queue",
      one_shot: true,
    },
    {
      identity: "UpdateManager.AddUpdate(MonoBehaviour, int, UpdateManager.OnUpdate)",
      status: "rejected-recurring-registration",
      one_shot: false,
    },
    {
      identity: "VContainer.Unity.PlayerLoopHelper.Dispatch(PlayerLoopTiming, IPlayerLoopItem)",
      status: "rejected-interface-item-registration",
      one_shot: false,
    },
  ],
);
assert.equal(schedulerProof.safe_adapter_contract.status, "design-contract-only-not-implemented");
assert.equal(schedulerProof.read_only_scheduler_preflight.possible, true);
assert.equal(schedulerProof.read_only_scheduler_preflight.activation_permitted_by_preflight, false);
assert.ok(schedulerProof.unresolved_blockers.length >= 9, "scheduler proof must retain every exact unresolved blocker");
assert.equal(schedulerProof.scope.offline_static_analysis_only, true);
for (const denied of [
  "process_access",
  "game_method_invocation",
  "managed_delegate_allocation",
  "process_memory_write",
  "code_injection",
  "packet_synthesis_or_transmission",
  "runtime_activation_enabled",
]) {
  assert.equal(schedulerProof.scope[denied], false, `scheduler proof ${denied} must stay disabled`);
}
assert.equal(schedulerProof.decision.one_shot_queue_semantics_statically_proven, true);
assert.equal(schedulerProof.decision.stronger_reviewed_candidate_that_removes_ingress_or_delegate_requirement_found, false);
assert.equal(schedulerProof.decision.safe_managed_callback_construction_proven, false);
assert.equal(schedulerProof.decision.supported_in_process_entry_proven, false);
assert.equal(schedulerProof.decision.safe_main_thread_invocation_proven, false);
assert.equal(schedulerProof.decision.runtime_activation_enabled, false);
assert.equal(schedulerProof.decision.native_placement_must_remain_disabled, true);
assert.equal(schedulerProof.privacy.contains_private_paths, false);
assert.equal(schedulerProof.privacy.contains_absolute_native_addresses, false);
assert.equal(schedulerProof.privacy.contains_process_addresses, false);
assert.equal(schedulerProof.privacy.contains_personal_identity, false);
assert.doesNotMatch(
  JSON.stringify(schedulerProof),
  /[A-Z]:\\|\\\\[A-Za-z0-9._-]+\\/i,
  "scheduler proof must not contain private absolute paths",
);

console.log("validated exact-build automarker internal-route proof");
