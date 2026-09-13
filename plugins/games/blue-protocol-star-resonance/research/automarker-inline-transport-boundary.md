# Automarker inline transport boundary

Status: offline adapter implemented; live interception and activation unavailable.

The retained current-build marker request can now be transformed as a copied
197-byte BPSR frame, tracked across TCP segmentation/retransmission, and applied
to copied WinDivert `NETWORK`-layer IPv4/TCP packet shapes. The adapter validates
the original IPv4 and TCP checksums, preserves packet length and the complete IP
header, preserves every TCP header byte except its checksum, and recalculates a
valid TCP checksum after payload substitution. IPv4 fragments, checksum-offload
ambiguity, wrong direction/four-tuples, wrong epochs, conflicting retransmits,
and ledger gaps all return the original copied packet byte-for-byte.

An epoch binding requires all of the following evidence:

- a nonzero exact game process ID;
- exactly one matching process-owned local/remote IPv4 socket four-tuple; and
- observation of the connection SYN that starts the epoch.

A protocol signature or process name is not ownership evidence. A mirror capture
cannot create this binding because the game socket exists on the other computer.

The implemented boundary deliberately has no WinDivert dependency, driver
handle, receive/block/reinject/send operation, product service, HTTP endpoint,
setting, hotkey, or UI action. It only transforms caller-owned byte vectors in
memory. Therefore it cannot alter live traffic.

## Remaining activation gates

Before a live Windows backend can exist, every item below still needs explicit
proof and review:

1. Select and vendor/review a documented WinDivert or WFP backend, including
   signed-driver installation, least-privilege lifecycle, deterministic shutdown,
   queue bounds, and an unconditional fail-open path that reinjects the original.
2. Bind interception to the exact game-owned socket epoch using the IP Helper
   socket tables and SYN lifecycle. The backend must never infer ownership from
   an executable name, protocol signature, or remote endpoint alone.
3. Establish which plaintext leg is authoritative with ExitLag enabled. A likely
   game-to-local-proxy loopback leg is not proof. WFP callout ordering relative to
   ExitLag must be measured on the game computer, and the backend must refuse to
   activate if more than one candidate leg or callout order is observed.
4. Translate reassembled frame offsets back to every intersecting physical TCP
   segment and keep the rewrite ledger active through cumulative ACK, including
   retransmission, overlap, reordering, sequence wrap, FIN/RST, and epoch reset.
5. Validate checksum-offload behavior at the chosen interception layer. The
   offline adapter intentionally rejects invalid captured checksums rather than
   treating them as writable packets.
6. Prove server acceptance and visible party-wide marker placement for saved
   far-away coordinates in a controlled current-build canary. The existing
   substitution proof demonstrates wire-layout conservation, not permission,
   anti-cheat safety, server acceptance, or coordinate authorization.

No product UI activation should be added until all gates are satisfied. A future
backend must remain separately feature-gated and disabled by default through its
first reviewed canary phase.

The schema-2 passive Windows boundary receipt now keeps the observable pieces of
gate 3 separate: exact marker visibility per capture surface, a unique sanitized
connection epoch, exact game-owned four-tuple evidence, SYN lifecycle, and direct
uncompressed byte offsets. It explicitly records that peer-process identity and
WFP ordering were not observed. Selecting `-ExitLag` is only an operator-requested
capture mode; it is not treated as evidence that ExitLag is running. This closes
the prior diagnostic ambiguity without claiming that passive Npcap can satisfy
the remaining activation gate.
