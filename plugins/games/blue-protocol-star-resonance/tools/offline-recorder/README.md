# BPSR offline recorder

This tool converts one private, exact-flow pcap/pcapng capture into a sealed
canonical `.rlog` plus a privacy-safe coverage report:

```text
cargo run -p rlogs-bpsr-offline-recorder -- \
  --private-research \
  --pack plugins/games/blue-protocol-star-resonance/protocol-packs/global/steam-24252055/pack.json \
  --connections private/run.connections.json \
  --session-id run-001 \
  private/run.pcap \
  private/run.rlog
```

The automatically created `private/run.coverage.json` contains counts, route
IDs, pack dispositions, decoder results, feature coverage, event topics, and
data gaps. It contains no raw packet bytes, IP addresses, credentials, private
chat, or account data.

The current region defaults to the exact pack's region, or its deployment ID
when the pack is shared across the deployment. This unresolved fallback does
not mean that Global is one server: `global` is the deployment, while Asteria
and Bahamar are separate realm IDs. The BPSR decoder reads the scene endpoint
announced on world entry without declaring or retaining the adjacent account ID
and login-token fields. The recorder checks exact reviewed endpoint rules before
writing events, so a known connection receives its realm identity from the
start. Unknown endpoints remain unresolved; RLogs will not guess and misfile a
log.

The pcap and connection file remain private research artifacts. The `.rlog`
contains only privacy-reviewed canonical events and is the future
submission/replay boundary. `--region-id` remains available only for a
deployment or protocol pack with an independently verified narrower region.
