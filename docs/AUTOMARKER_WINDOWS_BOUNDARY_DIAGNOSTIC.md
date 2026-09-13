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
Only the file ending in `.safe.v2.json` is created.

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

Schema 2 also emits one fail-closed `topology` assessment. It assigns a local,
receipt-only ordinal to each signature-confirmed connection so the diagnostic
can report whether the marker appeared on exactly one connection epoch without
retaining an endpoint. Its independent gates are:

- **Plaintext BPSR leg:** at least one exact marker request, exactly one
  connection epoch, and directly locatable uncompressed offsets for every
  observed request. Seeing the same action on loopback and a routed adapter is
  reported as ambiguous, not silently resolved.
- **Game socket:** every request must match the exact bidirectional four-tuple
  in the selected game's IP Helper table and the diagnostic must have observed
  the SYN that began that epoch. A process name or protocol signature does not
  satisfy this gate.
- **Local proxy:** loopback requests are counted separately according to whether
  the game-owned tuple was observed. The diagnostic does not query peer-process
  ownership, so neither an ExitLag peer nor the authoritative proxy leg can be
  claimed from this receipt.
- **WFP ordering:** passive Npcap cannot observe relative WFP callout order and
  no interception layer is exercised. This gate therefore always remains
  false in this diagnostic.

`-ExitLag` records that the operator selected the ExitLag test mode; it is not
evidence that an ExitLag process, driver, or callout was present. Live
interception activation remains false even when every passive wire and socket
gate passes.

## ZDPS comparison

The retained ZDPS source does not supply a stronger ExitLag transport route. It
documents manual Loopback selection for VPNs and recommends ExitLag Legacy-NDIS
mode. Its capture code opens one selected SharpPcap device and checks packets
against game TCP-table rows. rLogs retains selected/routed readers, adds one
bounded loopback candidate only in the opt-in compatibility mode, and applies
one shared BPSR signature privacy boundary across the fan-in. The ZDPS guidance
is useful setup advice, but it does not prove a plaintext leg or WFP ordering
for Automarkers.

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
