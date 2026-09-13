import assert from "node:assert/strict";
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const proofPath = path.join(
  root,
  "plugins/games/blue-protocol-star-resonance/research/game-file-inventory/global/steam-25247556/automarker-multi-marker-carrier-plan.v1.json",
);
const modulePath = path.join(
  root,
  "plugins/games/blue-protocol-star-resonance/src/automarker_preset_sequence.rs",
);
const proof = JSON.parse(fs.readFileSync(proofPath, "utf8"));
const source = fs.readFileSync(modulePath, "utf8");

assert.equal(proof.schema_version, 1);
assert.equal(proof.build_id, "25247556");
assert.equal(proof.scope.packet_transmission_performed, false);
assert.equal(proof.scope.runtime_placement_enabled, false);
assert.equal(proof.six_request_observation.ordinary_game_ui_requests, 6);
assert.equal(proof.six_request_observation.distinct_skill_uuids, 6);
assert.equal(proof.six_request_observation.distinct_authenticated_envelopes, 6);
assert.equal(proof.single_request_duplication_adjudication.legal_or_protocol_safe_from_current_evidence, false);
assert.equal(proof.single_request_duplication_adjudication.same_length_substitution_can_create_additional_application_frames, false);
assert.equal(proof.carrier_count.six_marker_preset_requires_fresh_carriers, 6);
assert.equal(proof.carrier_count.same_marker_identity_required, true);
assert.equal(proof.no_menu_or_movement_architecture.player_movement_required_by_wire_format, false);
assert.equal(proof.no_menu_or_movement_architecture.current_position_must_be_game_generated_and_preserved, true);
assert.equal(proof.no_menu_or_movement_architecture.far_coordinate_server_acceptance_proven, false);
assert.equal(proof.conclusion.one_captured_request_can_be_duplicated_or_synthesized, false);
assert.equal(proof.conclusion.six_ordinary_fresh_carrier_requests_unavoidable_for_six_markers_under_current_evidence, true);
assert.equal(proof.conclusion.production_runtime_activation_ready, false);

for (const token of [
  "same-number carrier",
  "authenticated_envelope_fingerprint",
  "current_position_bits",
  "AwaitingCarrier",
  "AwaitingConfirmation",
  "observe_rpc_return",
  "observe_authoritative_add",
  "ReusedGameOwnedIdentity",
  "SceneOrLeadershipChanged",
]) {
  assert.ok(source.includes(token), `planner source lost required fail-closed token: ${token}`);
}

console.log("validated exact-build multi-marker carrier sequence plan");
