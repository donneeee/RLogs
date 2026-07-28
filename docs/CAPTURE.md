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

`tools/capture-inspect` provides a payload-free validation summary. Live Npcap
and libpcap adapters, TCP reconstruction, and BPSR frame decoding are later
slices.

## Stored formats

Pcapng is preferred for opt-in protocol research because it can preserve
interface metadata and modern timestamp information. RLogs also imports the
older pcap format for compatibility.

Normal user logs are `.rlog` files containing privacy-reviewed canonical
evidence. Raw pcap, pcapng, and JSONL journals remain local research artifacts
and are never leaderboard uploads.

## Filtering and privacy

Live capture applies the narrowest reliable BPF filter after the game
connection is identified. Capture statistics and detected drops are recorded.
RLogs must not silently switch to broad whole-machine capture.

Even filtered packet data is treated as private. Raw capture access requires
developer mode and is not available to ordinary plugins.

## Optional adapters

The `CaptureSource` interface allows future sources such as `dumpcap`, a
privilege-separated helper, or platform-specific drivers. They are alternate
frame sources, not alternate decoders. The canonical pipeline remains singular.

Npcap redistribution and installer terms must be reviewed before RLogs bundles
an installer. Early development may require users to install Npcap separately.
