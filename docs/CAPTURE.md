# Packet capture decision

## Default

RLogs uses the portable pcap API as its initial live-capture boundary:

- **Windows:** Npcap;
- **Linux:** libpcap;
- **offline on either platform:** pcap and pcapng import.

Npcap is the Windows implementation of the libpcap API, so the RLogs capture
adapter can share one implementation and test contract across both operating
systems. OS installation, permissions, and driver discovery remain
platform-specific setup concerns.

Pcap and pcapng are capture-file formats, not a second parser. Imported frames
enter the exact same validation, reconstruction, protocol, event, and plugin
pipeline as live frames.

## Current implementation

The pure-Rust offline reader is implemented in `engine/capture`. It streams
legacy pcap and pcapng, supports pcapng sections and multiple interfaces,
preserves capture truncation and original timestamps, and produces
deterministic monotonic replay time.

`tools/capture-inspect` provides a payload-free validation summary. The shared
`engine/core` pipeline now carries exact-allowlisted offline frames through
link/IP/TCP decoding and bounded reconstruction. The selected trusted game
plug-in receives only the resulting directional streams; the bundled BPSR
plug-in performs BPSR framing and private JSONL journaling. The Windows desktop
loads the installed Npcap `wpcap.dll` directly from the trusted system
directory and opens the selected `\\Device\\NPF_{GUID}` adapter without
requiring Wireshark or `dumpcap.exe`. A configured dumpcap path remains a
compatibility fallback. A portable Linux libpcap adapter remains a later slice.

Npcap installs `wpcap.dll` and its lower-level `Packet.dll` as a matched pair.
If an interrupted or partial update leaves incompatible versions, Windows can
otherwise display a blocking "Entry Point Not Found" dialog while an app probes
capture support. rLogs suppresses that system dialog only around its trusted
system-DLL probe, tries both trusted installation locations, and reports an
actionable repair/update diagnostic in Settings. It never loads a capture DLL
from the application or working directory.

## Stored formats

Pcapng is preferred for opt-in protocol research because it can preserve
interface metadata and modern timestamp information. RLogs also imports the
older pcap format for compatibility.

Normal user logs are `.rlog` files containing privacy-reviewed canonical
evidence. Raw pcap, pcapng, and JSONL journals remain local research artifacts
and are never leaderboard uploads.

## Filtering and privacy

Live capture applies the narrowest reliable filter after the game process is
identified. Capture statistics and detected drops are recorded. RLogs must not
silently switch to broad whole-machine persistence.

The initial Windows research helper snapshots exact established endpoint pairs
owned by a process selected by the trusted game plug-in and gives only those
pairs to `dumpcap`. The bundled BPSR manifest selects `BPSR_STEAM.exe`. This
snapshot mode is safe for stable connections but cannot follow a world
transition that rotates the remote server. See
[Controlled Global capture](CONTROLLED_CAPTURE.md).

The Windows live adapter tracks the process-owned socket table while capturing.
Native Npcap ingress, or the optional dumpcap compatibility pipe, never receives
a filesystem output path. RLogs holds unattributed frames in bounded memory long
enough to confirm ownership, then discards unrelated traffic before TCP
reconstruction, protocol decoding, or persistence. A newly observed flow can
be retained only after its exact tuple is attributed to the selected game
process; unknown flows expire from the bounded pending buffer. This same
ownership-filter contract will sit above the Linux socket-owner implementation.

Even filtered packet data is treated as private. Raw capture access requires
developer mode and is not available to ordinary plugins.

## Optional adapters

The `CaptureSource` interface allows future sources such as `dumpcap`, a
privilege-separated helper, or platform-specific drivers. They are alternate
frame sources, not alternate decoders. The canonical pipeline remains singular.

Npcap redistribution and installer terms must be reviewed before RLogs bundles
the driver itself. The rLogs installer does not redistribute Npcap; it uses an
existing Npcap installation directly and reports a clear capture error when the
driver is absent or its DLL pair is incompatible. Users can repair the pair with
the current installer from [Npcap's official download page](https://npcap.com/);
`dumpcap.exe` is not required.
