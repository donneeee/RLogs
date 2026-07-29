# Protocol journal

Replay an exact, narrowly filtered pcap/pcapng connection set through the
shared network and BPSR pipeline:

```text
cargo run -p rlogs-protocol-journal -- \
  --private-research \
  --pack plugins/games/blue-protocol-star-resonance/protocol-packs/global/steam-24252055/pack.json \
  --connections private/connections.json \
  --capture-id controlled-001 \
  private/controlled-001.pcapng \
  private/controlled-001.jsonl
```

`--private-research` is deliberately mandatory. Both the packet capture and
JSONL journal can retain opaque game payloads and must never be uploaded,
committed, or shared. The future `.rlog` submission format contains only
privacy-reviewed canonical evidence.

The connection file contains exact client/server TCP endpoints:

```json
{
  "schema_version": 1,
  "connections": [
    {
      "client": { "address": "192.0.2.10", "port": 50000 },
      "server": { "address": "198.51.100.20", "port": 12345 }
    }
  ]
}
```

Frames outside those exact bidirectional flows are ignored before TCP
reassembly. Output creation is non-overwriting and uses a visible partial file
until processing completes.
