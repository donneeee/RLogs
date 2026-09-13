import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import { fileURLToPath } from "node:url";

const source = await readFile(fileURLToPath(new URL(
  "../plugins/games/blue-protocol-star-resonance/src/automarker_rewrite_confirmation.rs",
  import.meta.url,
)), "utf8");
const docs = await readFile(fileURLToPath(new URL(
  "../docs/AUTOMARKER_REWRITE_CONFIRMATION_CONTRACT.md",
  import.meta.url,
)), "utf8");

for (const required of [
  "AUTOMARKER_OUTBOUND_CARRIER_METHOD_ID: u32 = 249_858",
  "AUTOMARKER_AUTHORITATIVE_MARKER_ADD_METHOD_ID: u32 = 46",
  "AUTOMARKER_CONFIRMATION_TIMEOUT_MILLIS",
  "AUTOMARKER_CONFIRMATION_TIMEOUT_MICROS",
  "rewrite.method_id != AUTOMARKER_OUTBOUND_CARRIER_METHOD_ID",
  "mapped_tcp_sequence_start",
  "mapped_tcp_length",
  "NotReverseExactTupleAck",
  "original_rpc_call_id",
  "decoded_body_length",
  "asserted_decoded_from_authoritative_server_stream",
  "marker_owner_actor_id",
  "passive_instance_identity",
  "same_position_bits",
  "runtime_revision <= self.rewrite.runtime_revision",
  "observed_micros <= self.rewrite.observed_micros",
  "FinOrRstObserved",
  "StaleOrReorderedObservation",
  "InconsistentObservationAge",
  "current_observed_micros.checked_sub(self.rewrite.observed_micros)",
  "observed_delta_micros >= AUTOMARKER_CONFIRMATION_TIMEOUT_MICROS",
  "observation.observed_micros != observation.stamp.observed_micros",
  "AlreadyTerminal",
]) assert.ok(source.includes(required), `missing confirmation invariant: ${required}`);

for (const forbidden of [
  "WinDivertOpen",
  "WinDivertSend",
  "WriteProcessMemory",
  "ReadProcessMemory",
  "SendInput",
  "TcpStream",
  "UdpSocket",
]) assert.equal(source.includes(forbidden), false, `pure contract owns forbidden capability: ${forbidden}`);

for (const required of [
  "never send authorization",
  "exact reverse four-tuple",
  "cumulative ACK",
  "exact original call ID",
  "outbound `World.UseSlot`",
  "Method 46 is reserved",
  "empty body",
  "method-46",
  "bit-exact target XYZ",
  "strictly newer runtime revision",
  "FIN or RST",
  "retransmissions",
  "zero-change traffic",
  "different state machines",
  "monotonic observed-microsecond delta",
]) assert.ok(docs.includes(required), `missing documented boundary: ${required}`);

console.log("validated pure single-marker rewrite confirmation contract");
