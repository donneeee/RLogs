import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import { fileURLToPath } from "node:url";

const source = await readFile(fileURLToPath(new URL(
  "../plugins/games/blue-protocol-star-resonance/src/automarker_bridge_coordinator.rs",
  import.meta.url,
)), "utf8");
const docs = await readFile(fileURLToPath(new URL(
  "../docs/AUTOMARKER_BRIDGE_COORDINATOR.md",
  import.meta.url,
)), "utf8");

for (const required of [
  "SingleMarkerXyzCanary",
  "SingleMarkerRewriteConfirmation",
  "AutomarkerBridgeChecksumInput",
  "prepare_outbound_segment",
  "cancel_prepared_rewrite_before_send",
  "commit_prepared_rewrite",
  "AutomarkerBridgeObservation",
  "observe_cumulative_ack",
  "observe_rpc_return",
  "observe_authoritative_marker_add",
  "tcp_rewrite_obligation_active",
  "CommittedButConfirmationAborted",
  "IndeterminateModifiedSend",
  "mapped_tcp_length: EXACT_CARRIER_BYTES",
  "rewrite_context: &AutomarkerConfirmationContext",
  "rewrite_context.local_actor_id != baseline.local_actor_id",
  "rewrite_context.runtime_revision != context.runtime_revision",
  "self.coordinator_terminal_error = Some(reason)",
]) assert.ok(source.includes(required), `missing bridge invariant: ${required}`);

for (const forbidden of [
  "WinDivertOpen",
  "WinDivertSend",
  "WriteProcessMemory",
  "ReadProcessMemory",
  "SendInput",
  "TcpStream",
  "UdpSocket",
]) assert.equal(source.includes(forbidden), false, `pure bridge owns forbidden capability: ${forbidden}`);

for (const required of [
  "not send authorization",
  "stored exact original packet",
  "failed or short",
  "confirmation is not created",
  "retrospective confirmation",
  "cumulative ACK",
  "bit-exact XYZ",
  "can never authorize a send",
  "confirmation failure",
  "exact connection terminates",
  "retransmissions",
  "no unrelated scalar timestamp is accepted",
  "wrong preparation ID",
  "cannot later",
  "REFLECT arbitration",
]) assert.ok(docs.includes(required), `missing documented bridge boundary: ${required}`);

console.log("validated pure Automarker bridge coordinator");
