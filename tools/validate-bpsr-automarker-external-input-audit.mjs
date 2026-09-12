import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import { fileURLToPath } from "node:url";

const proofUrl = new URL(
  "../plugins/games/blue-protocol-star-resonance/research/game-file-inventory/global/steam-25247556/ground-marker-external-input-surface-audit.v1.json",
  import.meta.url,
);
const proof = JSON.parse(await readFile(fileURLToPath(proofUrl), "utf8"));

assert.equal(proof.schema_version, 1);
assert.equal(proof.build_id, "25247556");
assert.equal(proof.build_identity.game_assembly_sha256, "4a079aec0a3e51a8023aa86bbd152e12068907b65b9aafb355d20bb6d6c41fe3");
assert.equal(proof.input_action_registry.constant_count, 171);
assert.equal(proof.input_action_registry.marker_named_constant_count, 0);
assert.deepEqual(proof.chat_and_console.registered_commands, ["scene.loadasync", "common.framerate"]);
assert.equal(proof.normal_game_ui_route.slot_mapping, "scene-mask slots 201-206 map to marker skills 1101-1106");
assert.equal(proof.normal_game_ui_route.position_argument_on_selection, false);

for (const field of [
  "direct_numbered_marker_input_action_found",
  "direct_marker_chat_or_console_command_found",
  "direct_marker_controller_or_accessibility_action_found",
  "direct_marker_launcher_or_deep_link_hook_found",
  "direct_saved_xyz_external_action_found",
  "foreground_input_without_camera_or_menu_traversal_proven",
  "external_input_implementation_authorized",
]) {
  assert.equal(proof.decision[field], false, `${field} must remain false`);
}
for (const field of ["input_generated", "process_access", "memory_write", "game_method_invocation", "packet_synthesis_or_transmission", "runtime_modified"]) {
  assert.equal(proof.scope[field], false, `${field} must remain false`);
}
assert.equal(proof.privacy.contains_private_paths, false);
assert.equal(proof.privacy.contains_absolute_native_addresses, false);
assert.doesNotMatch(JSON.stringify(proof), /[A-Z]:\\|\\\\[A-Za-z0-9._-]+\\/i);
assert.ok(proof.limits.length >= 4, "negative audit must retain its limitations");

console.log("validated exact-build automarker external-input negative audit");
