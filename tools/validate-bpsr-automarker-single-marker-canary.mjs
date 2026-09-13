import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import { fileURLToPath } from "node:url";

const source = await readFile(fileURLToPath(new URL(
  "../plugins/games/blue-protocol-star-resonance/src/automarker_single_carrier_canary.rs",
  import.meta.url,
)), "utf8");
const docs = await readFile(fileURLToPath(new URL(
  "../docs/AUTOMARKER_SINGLE_MARKER_XYZ_CANARY.md",
  import.meta.url,
)), "utf8");

for (const required of [
  "RLOGS_AUTOMARKER_SINGLE_FRESH_CARRIER_XYZ_V1",
  "AwaitingFreshCarrier",
  "intercept_complete_carrier_segment",
  "CarrierFrameNotExact",
  "AbortWithoutReinject",
  "prepare_outbound_segment",
  "PreparedRewriteNeedsPacketChecksumRepair",
  "cancel_prepared_rewrite_before_send",
  "commit_prepared_rewrite",
  "SingleMarkerXyzExternalSendOutcome",
  "IndeterminateModifiedSend",
  "committed_rewrite_stamp",
  "observation_monotonic_millis",
  "expected_packet_send_len",
  "observe_connection_terminated",
  "WinDivertHelperCalcChecksums",
  "observe_cumulative_ack",
  "observe_rpc_return",
  "observe_authoritative_self_add",
  "transport_ack_observed",
  "successful_rpc_return_observed",
  "authoritative_self_add_observed",
  "observed_age_millis",
  "new_instance_assertion",
  "AUTOMARKER_REQUEST_PACK_DIGEST",
]) assert.ok(source.includes(required), `missing activation guard: ${required}`);

assert.match(source, /const EXACT_FRAME_BYTES: usize = 197/);
assert.match(source, /SINGLE_MARKER_XYZ_MAX_CARRIER_AGE_MILLIS: u64 = 500/);
assert.equal(source.includes("SINGLE_MARKER_XYZ_MAX_DISTANCE"), false);
assert.equal(source.includes("TargetOutsideNearbyCanaryRadius"), false);
assert.match(docs, /not\s+constrained by the player's current position/);
assert.ok(docs.includes("loaded from very far away"));
assert.ok(docs.includes("There is no separate leader pre-gate"));
assert.ok(docs.includes("never synthesizes a request"));
assert.ok(source.includes("proof.allowed_mutable_bytes == 16"));
assert.ok(source.includes("proof.all_other_bytes_identical"));
assert.ok(source.includes("proof.game_owned_values_identical"));
assert.match(docs, /explicitly two phase/);
assert.match(docs, /Only a successful send whose[\s\S]*complete held-packet length[\s\S]*commits/);
assert.match(docs, /Retransmission commits never move that[\s\S]*pinned stamp/);
assert.match(docs, /zero changed bytes[\s\S]*never requests checksum repair/);
assert.match(docs, /false send return[\s\S]*short send[\s\S]*indeterminate/);
assert.match(docs, /retained ledger[\s\S]*matching overlapping retransmission/);

for (const forbidden of [
  "WinDivertOpen",
  "WinDivertSend",
  "WriteProcessMemory",
  "ReadProcessMemory",
  "SendInput",
  "mouse_event",
  "keybd_event",
]) assert.equal(source.includes(forbidden), false, `core owns forbidden capability: ${forbidden}`);

for (const blocker of [
  "WinDivertHelperCalcChecksums",
  "#[repr(C)]",
  "80 bytes",
  "reverse exact-tuple",
  "ExitLag authoritative-leg",
  "no package script exposes an armed live mode",
]) assert.ok(docs.includes(blocker), `missing live blocker: ${blocker}`);

console.log("validated fail-closed single-marker XYZ canary contract");
