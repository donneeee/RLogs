# Read-only automarker wire-layout capture package

This package closes one specific evidence gap: whether one normal outbound
ground-marker request is compressed, batched, split across TCP segments, or
retransmitted on the exact global Steam build `25247556`.

It does not send, replay, rewrite, drop, or inject packets. It does not read or
write game memory, automate input, or use WinDivert. Capture is limited to TCP
frames attributed to the running `BPSR_STEAM` process. Native Npcap is preferred;
an installed `dumpcap.exe` is an optional fallback.

## Before running

1. Start the exact Global Steam client and enter a dungeon as party leader.
2. Be ready to place exactly one marker through the normal game UI.
3. Open PowerShell in this package folder.
4. Run:

```powershell
pwsh -NoProfile -File .\run-bpsr-automarker-wire-layout-capture.ps1
```

The launcher verifies its packaged payload hashes, Steam app/build identity,
and exact `BPSR_STEAM.exe` and `GameAssembly.dll` hashes before capture. It
discovers the process-owned adapter, prefers local Npcap, and uses dumpcap only
when available as a fallback. Unsupported or ambiguous setups fail with a
specific error.

When prompted, return focus to the game. Once capture starts, wait about eight
seconds, place exactly one marker normally, and then do nothing until the
20–30-second capture ends. The default is 25 seconds.

## Outputs

Every output is create-only under:

```text
PRIVATE-RAW-DO-NOT-SHARE\automarker-wire-layout-<UTC timestamp>\
```

The `.pcap`, `.connections.json`, `.protocol.jsonl`, and private hash manifest
are sensitive research evidence and must not be shared. Only the file ending in
`.safe-wire-layout-receipt.json` is sanitized for review. It contains frame
compression and length, batching, stream-chunk span, and aggregate TCP
reassembly classifications; it contains no payload, endpoint, port, TCP
sequence, RPC call ID, session/action identifier, timestamp, local path, or
personal identity.

If the safe-receipt step says no marker request was present, keep the failed
private run intact and simply run a new capture folder. Do not edit or reuse a
partial run.
