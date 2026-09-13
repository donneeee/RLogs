# Automarker single-marker rewrite confirmation contract

Status: pure research-only retrospective contract; not wired to a live bridge.

This contract answers one narrow question: after a separate component reports
that it rewrote one game-generated same-number marker carrier, do later passive
observations conclusively show that the server acknowledged and applied that
exact rewrite? It owns no WinDivert handle, driver, socket, packet capture,
packet send, process-memory access, app command, or product activation path.
A `Confirmed` result is evidence about a past attempt. It is never send authorization
and cannot cause another attempt.

## Baseline and immutable identity

The contract begins from a pre-rewrite authoritative snapshot and the exact
rewrite identity. The baseline fixes the game build, scene family, local actor,
connection epoch, client-to-server IPv4 TCP four-tuple, runtime revision,
observed clock, and every passive instance identity already present for the
same marker number. Zero or one pre-existing instance is accepted; multiple
same-number instances are ambiguous and rejected.

The rewrite is rejected unless it is explicitly method 46, and it fixes the
original RPC call ID and the mapped TCP sequence range.
Every subsequent observation must have a strictly newer bridge observation
ordinal and observed-microsecond clock. Runtime revisions may remain equal for
transport ACK and RPC-return observations, but may never regress. The marker
add itself must have both a strictly newer runtime revision and strictly newer
observed-microsecond clock than the rewrite.

## Required independent observations

Confirmation requires all three signals, in any order, before the two-second
deadline:

1. An ACK-bearing TCP packet on the exact reverse four-tuple and same connection
   epoch, with a cumulative ACK covering the exclusive end of the entire mapped
   TCP range. Sequence comparison is wrap-aware.
2. A caller-asserted authoritative inbound RPC return with the exact original call ID,
   decoded success, and an empty body.
3. A caller-asserted authoritative inbound method-46 marker add for the same
   marker number, owned by the unchanged local actor, with bit-exact target XYZ
   and a nonzero passive instance identity absent from the baseline.

The two fields named `asserted_*` honestly describe this pure module boundary.
They must eventually come from reviewed trusted decoder provenance. Neither
assertion is independent evidence inside this module, and neither can arm or
authorize a sender.

## Fail-closed outcomes

FIN or RST, timeout, build/scene/local-actor/leader assertion/socket/epoch
change, runtime regression, stale or reordered evidence, a wrong or duplicate
RPC candidate, a non-authoritative or nonempty return, an incorrect method,
marker number, owner, or XYZ, a reused passive instance identity, and ambiguous
baseline or marker evidence all abort the proof. An aborted or confirmed
contract is terminal and cannot later change state.

The deadline is checked independently against both the caller-supplied elapsed
milliseconds and the monotonic observed-microsecond delta from the committed
rewrite. A caller cannot extend the deadline by reporting a smaller elapsed
value: an elapsed value below the integral monotonic delta is inconsistent and
fails closed, while either clock reaching two seconds is a timeout. Clock
underflow is rejected as a non-post-rewrite observation. The method-46 marker
event's observed clock must also exactly equal its observation stamp.

Transport recovery remains outside this contract. In particular, this module
does not imply that a live bridge should immediately close a handle or drop a
packet after a logical confirmation failure. Once changed bytes have been sent,
the transport ledger must continue rewriting matching overlapping
retransmissions until cumulative ACK, FIN, or RST, pass non-overlapping and
zero-change traffic through unchanged, and use its separately reviewed recovery
policy. Confirmation failure and TCP byte-mapping lifetime are different state machines.
