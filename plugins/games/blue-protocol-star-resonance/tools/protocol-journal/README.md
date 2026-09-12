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

By default the selected protocol pack is exact for the capture and supplies the
journal's `game_build`. To investigate a newer captured build with an older
pack strictly as a decoder hypothesis, both identities must be explicit:

```text
cargo run -p rlogs-protocol-journal -- \
  --private-research \
  --captured-build 25247556 \
  --unverified-carry-forward-pack-source-build 24687926 \
  --pack plugins/games/blue-protocol-star-resonance/protocol-packs/global/steam-24687926/pack.json \
  --connections private/connections.json \
  --capture-id marker-proof-001 \
  private/marker-proof-001.pcapng \
  private/marker-proof-001.jsonl
```

This mode writes `game_build.build_id` as the actual captured build and adds a
`protocol_pack_authority` object identifying the pack's source build with
`exact_for_captured_build: false` and `runtime_authority: false`. Supplying only
one build flag, naming a source build that differs from the pack target, or
naming the captured build as its own carry-forward source fails closed. Exact
mode retains its previous serialized shape because the authority field is
omitted when no carry-forward is used.
