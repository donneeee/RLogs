# Automarker single-marker XYZ substitution canary

Status: research-only activation contract implemented; live Windows bridge not
yet enabled or packaged.

This is the next bounded step after the byte-identical WinDivert pass-through.
It may alter only the target XYZ inside one fresh, game-generated marker request
for the same marker number. It does not synthesize, duplicate, replay, or insert
a request and it has no process-memory or input-automation path.

## Fail-closed contract

The default state is `DryRun`. Arming requires the exact literal token, reviewed
Steam build and pack digest, an opaque process-owned four-tuple binding whose
epoch began with an observed SYN, a fresh trusted runtime scene revision, and
exact scene-family equality. There is no separate leader pre-gate: the fresh,
authenticated, same-number `World.UseSlot` request is evidence that the game
allowed this placement. The canary never synthesizes a request. Target XYZ must be
finite and inside the supported numeric domain, but it is deliberately not
constrained by the player's current position. Saved markers may therefore be
loaded from very far away anywhere in the exact same map/scene family.

The carrier must be a complete uncompressed 197-byte FrameUp wholly contained
in the packet currently blocked by the interception boundary. SYN-with-payload,
split frames, stale carriers, wrong-number carriers, uncertain framing, and
ambiguous context abort before mutation. The frame is armed at
`tcp.sequence + frame_payload_offset` and that same held packet is rewritten.
The established TCP ledger retains the mapping for matching segmentation,
overlap, retransmission, wrap, and cumulative ACK behavior.

The outbound API is explicitly two phase. `prepare_outbound_segment` returns an
owned original and proposed replacement plus an opaque preparation ID and the
full held-packet length. Preparation does **not** set the rewrite revision,
advance into confirmation, or claim that any changed byte was sent. The bridge
must apply the replacement to a copy, complete and verify external checksum
repair, and then call `commit_prepared_rewrite`. Only a successful send whose
reported byte count equals the complete held-packet length commits the first
rewrite revision and monotonic time. Retransmission commits never move that
pinned stamp.

If checksum preparation fails before any send attempt,
`cancel_prepared_rewrite_before_send` discards the uncommitted mapping and
returns the exact original payload for reinjection. A false send return, a
short send, or a reported successful length other than the complete held packet
is indeterminate. Those outcomes never authorize the original overlapping
bytes. Likewise, an overlap that contains zero changed bytes is returned as the
exact original and never requests checksum repair.

After any modified range is sent, a conflicting overlap, poisoned ledger,
epoch change, send ambiguity, or context change must not fail open. The bridge
must stop reinjection, close the active handle, and require the game connection
to reconnect. Otherwise a retransmission could expose original and replacement
bytes at the same TCP sequence numbers.

A logical abort after a committed or indeterminate modified send is terminal:
ACKs or later traffic cannot revive it. Its retained ledger nevertheless keeps
rewriting every matching overlapping retransmission deterministically and lets
non-overlapping traffic pass byte-identically. That transport obligation ends
only when the exact operation is cumulatively ACKed or the bridge reports a
FIN/RST/otherwise-proven connection termination.

Success requires all three observations after the rewrite:

- a cumulative transport ACK retiring the exact operation;
- the matching successful empty RPC return for the original call ID;
- a new authoritative self-add for the same marker number and bit-exact XYZ.

A pre-existing marker snapshot, an older runtime revision, or just seeing the
outbound packet is not success.

## Mandatory live bridge work still open

The state machine is intentionally not a runnable live tool yet. These gates
must be implemented and tested in the separate research executable first:

1. Load the pinned `WinDivertHelperCalcChecksums` export. NETWORK captures may
   carry checksum-offload state, so raw IPv4/TCP checksums cannot be assumed
   valid when the corresponding `WINDIVERT_ADDRESS` flags are unset. Preserve
   the original packet and address for every non-candidate. For the one approved
   mutation, call the official helper on the changed packet and mutable address,
   require success, verify its postconditions, and then send.
2. Define the WinDivert address with `#[repr(C)]`, prove its x64 size is exactly
   80 bytes, and validate layer/event/outbound/checksum flags before mutation.
3. Add a reverse exact-tuple observation handle (or a proven fresh trusted
   rLogs event feed) for ACK, return, FIN/RST, and authoritative method-46 add.
   The existing outbound-only active filter cannot observe these.
4. Require strictly post-rewrite observation revisions and derive the
   new-instance assertion by comparing a newly observed passive instance
   identity against the pre-rewrite authoritative snapshot. The state-machine
   boolean is explicitly an assertion, not independent evidence.
5. Retain the pass-through gates for official hashes/signature/version,
   Administrator/BFE, same-priority REFLECT arbitration, actual-topology
   pass-through evidence, and the separate ExitLag authoritative-leg proof when
   ExitLag is enabled.
6. On shutdown or error, drain only packets proven safe to reinject. A held
   candidate with uncertain checksum, mapping, or send result requires reconnect.

Until those items are closed, no package script exposes an armed live mode and
the normal rLogs application remains unable to activate this canary.
