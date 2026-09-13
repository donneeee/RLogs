# Passive Windows automarker boundary diagnostic

This diagnostic answers the remaining Windows transport-placement questions
without sending, suppressing, changing, or reinjecting a packet. It also does
not access process memory or automate input. The executable has no API for any
of those operations.

The capture path is the same bounded, route-aware Npcap fan-in used by rLogs.
Raw frames remain behind the BPSR protocol-signature privacy gate and are never
written to disk. Windows IP Helper supplies exact TCP four-tuples owned by the
selected `BPSR_STEAM` process, including loopback sockets. The analyzer compares
those tuples in memory and emits only categorical results.

## Running the packaged diagnostic

Start the game and enter a scene where you can place a marker normally. Close
any second game client so there is exactly one `BPSR_STEAM` process, then run:

```powershell
pwsh -NoProfile -File .\run-bpsr-automarker-windows-boundary.ps1
```

When ExitLag is enabled for that test, use:

```powershell
pwsh -NoProfile -File .\run-bpsr-automarker-windows-boundary.ps1 -ExitLag
```

On a separate PC that receives mirrored traffic, no local game process is
required:

```powershell
pwsh -NoProfile -File .\run-bpsr-automarker-windows-boundary.ps1 -Mirror
```

The mirror path uses the active routed adapter by default. If mirrored traffic
arrives on a different local adapter, select its exact Windows friendly name:

```powershell
pwsh -NoProfile -File .\run-bpsr-automarker-windows-boundary.ps1 `
  -Mirror -InterfaceName 'Ethernet 2'
```

The name must uniquely match a locally enumerated adapter. It is used only to
open the capture surface and is never included in the receipt. Mirror mode does
not query a local game socket table and categorically reports game-process
ownership, the remote game's ExitLag state, and WFP/callout ordering as
unproven. `-Mirror` and `-ExitLag` cannot be combined.

The signature boundary recognizes either the normal early BPSR server proof or
an exact authenticated marker request. The second path lets a mirror capture
attach after the game connection was established without broadening ordinary
traffic access. Place the marker promptly after the diagnostic starts so the
bounded 64 KiB private prefix window is not exhausted first.

Place exactly one marker through the normal game UI during the 30-second
window. A separate standard and ExitLag run makes the comparison unambiguous.
Only the file ending in `.safe.v1.json` is created.

## What the receipt establishes

For each decoded exact-build marker request, the receipt records:

- whether plaintext BPSR was visible on loopback or a physical/routed surface;
- whether its exact bidirectional four-tuple was observed in the game's Windows
  process-owned socket table;
- a local connection-epoch ordinal and whether that epoch began with an
  observed SYN;
- nested and outer compression state, TCP stream-chunk span, segmentation,
  reorder, duplicate, overlap, retransmission, and forced-gap aggregates;
- whether the exact decoder and copied-buffer verifier reconstructed all 16
  approved application value bytes (slot varint, skill varint, and target XYZ);
- whether those 16 bytes are directly locatable in the observed uncompressed
  wire layout.

Npcap observation cannot determine checksum-offload state. A checksum that
looks invalid before egress cannot distinguish a bad wire checksum from normal
transmit offload, so this tool deliberately does not claim checksum validity.
The receipt states that any future inline mechanism must recalculate the TCP
checksum and correctly cover segmentation, overlap, and retransmission.

The receipt contains no raw payload, endpoint, port, TCP sequence or
acknowledgement number, RPC call ID, timestamp, local path, or account, action,
or session identifier. It does not prove Windows Filtering Platform callout
ordering, server acceptance, anti-cheat safety, or permission to enable a
sender.
