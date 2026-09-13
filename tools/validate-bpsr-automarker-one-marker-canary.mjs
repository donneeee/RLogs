import fs from "node:fs";

const cargo = fs.readFileSync("plugins/games/blue-protocol-star-resonance/Cargo.toml", "utf8");
const source = fs.readFileSync("plugins/games/blue-protocol-star-resonance/tools/automarker-one-marker-canary.rs", "utf8");
const docs = fs.readFileSync("docs/AUTOMARKER_ONE_MARKER_ACTIVE_CANARY.md", "utf8");

const required = [
  [cargo, 'name = "rlogs-bpsr-automarker-one-marker-canary"'],
  [source, "RLOGS_AUTOMARKER_ONE_MARKER_ACTIVE_CANARY_V1"],
  [source, "blocked_unresolved_authoritative_inbound_decoder_and_context_wiring"],
  [source, "reflect_arbitration_required: true"],
  [source, "complete_fresh_world_use_slot_249858_carrier_required: true"],
  [source, "checksum_repair_required: true"],
  [source, "deterministic_retransmission_mapping_required: true"],
  [source, "reverse_ack_rpc_authoritative_add_required: true"],
  [source, "player_distance_gate: false"],
  [source, "packet_or_memory_write_attempted: false"],
  [source, "prepare_automarker_ipv4_tcp_packet"],
  [source, "AutomarkerBridgeCoordinator"],
  [source, "SingleMarkerXyzExternalSendOutcome::Short"],
  [docs, "No separate leader field is required"],
  [docs, "World.UseSlot` method-249858"],
  [docs, "Method 46 is exclusively the authoritative inbound"],
  [docs, "one permitted coordinator-to-checksum handoff"],
  [docs, "must ultimately run on the game PC"],
  [docs, "There is intentionally no player-position or distance gate"],
];

for (const [haystack, needle] of required) {
  if (!haystack.includes(needle)) throw new Error(`missing required one-marker canary invariant: ${needle}`);
}

if (source.includes("complete_fresh_method_46_carrier_required") || docs.includes("outbound method-46 carrier")) {
  throw new Error("method 46 must not be described as the outbound carrier");
}

console.log("validated fail-closed one-marker active canary boundary");
