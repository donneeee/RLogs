# Automarker one-marker bridge coordinator

Status: pure research-only composition; no live driver or product activation.

This coordinator connects the reviewed single-carrier two-phase canary, the
WinDivert checksum boundary's data inputs, and the retrospective rewrite
confirmation contract. It owns no driver handle, socket, process lookup,
packet capture, packet send, process-memory access, or input automation.

## Outbound lifecycle

The caller first supplies one fresh, complete, game-generated same-number
carrier to `observe_fresh_carrier`. `prepare` then accepts an owned view of the
already-held full packet and delegates its TCP payload to the canary. An exact
pass-through decision returns the full original packet and address unchanged.
A rewrite preparation also requires the complete confirmation context at that
same boundary. Build, scene family, local actor, connection epoch, exact tuple,
leader assertion, and runtime revision must remain coherent with both the
immutable baseline and the canary context. Its observed-microsecond clock is
the rewrite anchor; no unrelated scalar timestamp is accepted.

A rewrite decision returns `AutomarkerBridgeChecksumInput`: the exact original
packet, the proposed same-length changed packet, the original 80-byte address,
and the opaque preparation ID.

That value is only a checksum-boundary input. It is **not send authorization**.
The external bridge must pass it to `prepare_automarker_ipv4_tcp_packet`; only
that separately reviewed boundary may return
`AutomarkerPacketSendPreparation::ModifiedAuthorized`. This coordinator never
calls a checksum helper and cannot transmit its result.

Before any send attempt, `cancel` abandons the preparation and returns the
stored exact original packet. After a send attempt, `commit` accepts only the
canary's explicit complete, failed, or short outcome. A failed or short
modified send is indeterminate, activates the TCP rewrite obligation, and
confirmation is not created. It never authorizes overlapping original bytes.

An exact complete send starts retrospective confirmation using the original
RPC call ID, the complete 197-byte mapped carrier range, the immutable baseline
scene/socket/local-actor identity, and the first committed rewrite stamp.
Retransmission commits do not restart or move that confirmation baseline.

## Observation and independent lifetimes

`observe` accepts already-decoded reverse TCP, RPC return, authoritative marker
add, timeout, and connection-termination evidence. Confirmation still requires
all three independent post-send signals from the confirmation contract: exact
reverse-tuple cumulative ACK, matching successful empty RPC return, and a new
authoritative local-owner method-46 add with bit-exact XYZ.

Confirmation is retrospective only. Confirmed, awaiting, aborted, or missing
confirmation can never authorize a send and can never clear a TCP mapping.
After any committed or indeterminate modified send, the coordinator retains
`tcp_rewrite_obligation_active` across confirmation failure. It clears that
transport obligation only when the canary ledger reports the exact mapped
operation cumulatively ACKed, or when the exact connection terminates. While
the obligation exists, later matching overlaps must still pass through the
canary so retransmissions receive the same replacement bytes.

A commit with the wrong preparation ID is itself an indeterminate modified-send
receipt. It terminally aborts the coordinator and canary, discards the pending
coordinator preparation, retains the TCP rewrite obligation, and cannot later
begin confirmation even if a second commit presents the formerly correct ID.

The module deliberately exposes data and deterministic state transitions only.
It does not make the currently blocked live WinDivert tool safe to enable; live
REFLECT arbitration, topology proof, driver lifecycle, and recovery remain
separate gates.
