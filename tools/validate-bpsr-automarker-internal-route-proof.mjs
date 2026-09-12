import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import { fileURLToPath } from "node:url";

const proofUrl = new URL(
  "../plugins/games/blue-protocol-star-resonance/research/game-file-inventory/global/steam-25247556/ground-marker-internal-game-owned-route-design-proof.v1.json",
  import.meta.url,
);
const proof = JSON.parse(await readFile(fileURLToPath(proofUrl), "utf8"));

assert.equal(proof.schema_version, 1);
assert.equal(proof.build_id, "25247556");
assert.equal(proof.build_identity.game_assembly.sha256, "4a079aec0a3e51a8023aa86bbd152e12068907b65b9aafb355d20bb6d6c41fe3");
assert.equal(proof.build_identity.global_metadata.sha256, "2f8213cf2a253d7a99a37b81f796e3a0162d0cf2d1ca6a2d0e0719f8f9a57779");
assert.equal(proof.abi_proof.set_indicator_position.rva, "0x53E86A0");
assert.equal(proof.abi_proof.fire_indicator_skill.rva, "0x52E09E0");
assert.equal(proof.abi_proof.vector3_layout.size_bytes, 12);
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

console.log("validated exact-build automarker internal-route proof");
