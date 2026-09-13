import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import { fileURLToPath } from "node:url";

const decisionUrl = new URL(
  "../plugins/games/blue-protocol-star-resonance/research/game-file-inventory/global/steam-25247556/automarker-windivert-backend-decision.v1.json",
  import.meta.url,
);
const sourceUrl = new URL(
  "../plugins/games/blue-protocol-star-resonance/src/automarker_inline_transport.rs",
  import.meta.url,
);

const decision = JSON.parse(await readFile(fileURLToPath(decisionUrl), "utf8"));
const source = await readFile(fileURLToPath(sourceUrl), "utf8");

assert.equal(decision.schema_version, 1);
assert.equal(decision.game_build, 25247556);
assert.equal(decision.decision.backend, "windivert");
assert.equal(decision.decision.upstream_version, "2.2.2");
assert.equal(
  decision.decision.upstream_tag_commit,
  "1789526ecfb9ff5397c94f9f54c1a3dc2fb60440",
);
assert.equal(decision.decision.live_activation_implemented, false);
assert.equal(decision.decision.live_activation_allowed, false);

assert.equal(
  decision.distribution.x64_dll.sha256_observed,
  "c1e060ee19444a259b2162f8af0f3fe8c4428a1c6f694dce20de194ac8d7d9a2",
);
assert.equal(
  decision.distribution.x64_driver.sha256_observed,
  "8da085332782708d8767bcace5327a6ec7283c17cfb85e40b03cd2323a90ddc2",
);
assert.equal(decision.distribution.x64_driver.authenticode, "valid");
assert.equal(decision.platform.administrator_required_for_first_open_and_driver_install, true);
assert.equal(decision.platform.no_install_preflight, true);

const active = decision.handles.find((entry) => entry.purpose === "single-epoch-substitution");
assert.ok(active);
assert.equal(active.layer, "NETWORK");
assert.equal(active.priority, 0);
assert.deepEqual(active.flags, ["NO_INSTALL"]);
for (const token of [
  "outbound",
  "ip.SrcAddr",
  "tcp.SrcPort",
  "ip.DstAddr",
  "tcp.DstPort",
  "tcp.PayloadLength > 0",
]) {
  assert.match(active.filter_template, new RegExp(token.replaceAll(".", "\\.")));
}

assert.equal(decision.packet_contract.batching_initially_allowed, false);
assert.equal(decision.packet_contract.overlapped_io_initially_allowed, false);
assert.equal(decision.packet_contract.max_userspace_inflight_packets, 1);
assert.equal(decision.packet_contract.preserve_received_address, true);
assert.equal(decision.packet_contract.send_length_policy.includes("equal"), true);
assert.deepEqual(decision.queue_policy, {
  initial_length: 4096,
  initial_time_ms: 2000,
  initial_size_bytes: 4194304,
  source: "WinDivert 2.2 defaults",
  rule: "read back every configured value; do not increase before measured load testing; any observed queue loss disables placement for the epoch",
});

assert.equal(decision.exitlag_decision.supported_now, false);
assert.ok(decision.activation_gates.includes("wfp-coexistence-proven"));
assert.ok(decision.activation_gates.includes("exitlag-authoritative-leg-proven-when-enabled"));
assert.ok(decision.known_residual_risks.some((risk) => risk.includes("terminates before reinjection")));
assert.ok(decision.known_residual_risks.some((risk) => risk.includes("mutual loops")));

for (const value of [
  decision.decision.upstream_version,
  decision.decision.upstream_tag_commit,
  decision.distribution.x64_dll.sha256_observed,
  decision.distribution.x64_driver.sha256_observed,
]) {
  assert.ok(source.toLowerCase().includes(value.toLowerCase()), `Rust policy is missing ${value}`);
}
for (const api of ["WinDivertOpen(", "WinDivertRecv(", "WinDivertSend("]) {
  assert.equal(source.includes(api), false, `${api} must not be callable in the offline policy`);
}

console.log("validated dependency-neutral Automarker WinDivert backend decision");
