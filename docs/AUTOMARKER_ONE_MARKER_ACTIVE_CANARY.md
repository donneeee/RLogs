# Automarker one-marker active canary

This executable is the integration boundary for exactly one saved marker. It is not a multi-marker loader, UI automation system, memory writer, or release artifact.

The intended live lifecycle is deliberately narrow:

1. require literal consent, the exact supported build and pack digest, a local `BPSR_STEAM.exe` PID, a SYN-derived process-owned IPv4 tuple/epoch, one exact scene family, a fresh runtime revision/clock, local actor identity, and an unambiguous same-number marker baseline;
2. use the committed REFLECT-arbitrated WinDivert boundary on the game PC;
3. hold one fresh, complete 197-byte outbound `World.UseSlot` method-249858 same-number carrier;
4. ask `AutomarkerBridgeCoordinator` for only the saved target XYZ substitution;
5. pass the coordinator-owned changed packet through `prepare_automarker_ipv4_tcp_packet` and the pinned WinDivert checksum helper;
6. send the resulting packet exactly once and commit only an exact full-length success;
7. preserve the committed TCP mapping for deterministic retransmissions; and
8. observe the reverse cumulative ACK, successful empty RPC return for the original call ID, and a new authoritative local-owner method-46 marker add with bit-exact XYZ.

There is intentionally no player-position or distance gate. A saved point may be far from the character as long as it belongs to the exact active scene family.

## Current activation boundary

Dry-run mode writes a sanitized receipt without loading WinDivert or opening a handle. Armed mode validates every supplied static input and then fails closed with:

`blocked_unresolved_authoritative_inbound_decoder_and_context_wiring`

This is not a placeholder success. The pure coordinator, two-phase TCP mapping, checksum boundary, REFLECT arbitration, and retrospective confirmation contract exist, but this executable does not yet have a trustworthy live source for decoded authoritative RPC returns, authoritative marker-add instance/owner data, or fresh scene/local-actor continuity. It therefore stops before DLL loading, handle creation, packet interception, checksum repair, or transmission.

No separate leader field is required. The game rejects marker placement by a non-leader, so the bridge treats only a fresh, authenticated, exact outbound `World.UseSlot` method-249858 carrier from the locally process/SYN/tuple-bound game connection as game-authority evidence. It never synthesizes a request without that carrier. Method 46 is exclusively the authoritative inbound `SyncToMeDeltaInfo` marker-add confirmation route.

The executable already contains the one permitted coordinator-to-checksum handoff and the exact external-send commit mapping. A modified packet can only come from `AutomarkerBridgeCoordinator`, can only become sendable through `prepare_automarker_ipv4_tcp_packet`, and is committed as complete only when the external send reports the full held-packet length. A false or short modified send remains indeterminate and never releases the original overlapping bytes.

A mirrored capture on another computer can help confirm wire layouts and inbound decoding. It cannot satisfy the local process-owned tuple gate or modify the game PC's outbound stream, because it is not on the packet path the server receives. The armed executable must ultimately run on the game PC.

The literal outer arm token is `RLOGS_AUTOMARKER_ONE_MARKER_ACTIVE_CANARY_V1`. Supplying it does not bypass the unresolved live gate.
