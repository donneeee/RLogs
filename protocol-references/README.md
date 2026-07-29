# Protocol reference manifests

Reference evidence is game-owned. Current BPSR manifests and profile bridges
live at
[`plugins/games/blue-protocol-star-resonance/protocol-references/`](../plugins/games/blue-protocol-star-resonance/protocol-references/).

These manifests record route-name claims from immutable public source
revisions. They contain no copied decoder implementation and no packet
payloads.

Each project has a `lineage_id`. Resonance Logs Global and Resonance Logs CN
share the `resonance-logs` lineage, so agreement between them counts as one
vote. ZDPS is an independent lineage.

Run the checked-in world-load reconciliation with:

```text
cargo run -p rlogs-reference-reconcile -- \
  --observation plugins/games/blue-protocol-star-resonance/protocol-packs/global/steam-24252055/observations/world-load-process-001.json \
  plugins/games/blue-protocol-star-resonance/protocol-references/manifests/resonance-logs-cn-0.2.0.json \
  plugins/games/blue-protocol-star-resonance/protocol-references/manifests/resonance-logs-global-77380ca.json \
  plugins/games/blue-protocol-star-resonance/protocol-references/manifests/zdps-0.1.7.3.json
```

The result names reference candidates only. It does not prove that the current
Global payload shape matches a reference, authorize a decoder, or make a route
safe for plugins or website submissions.

Service IDs are checked against the game's BKDR-131 service-name hash with the
high bit masked. This resolves a service name without reading a payload, but it
does not identify the method inside that service.

`profile-bridges/` records how exact-build, privacy-reviewed runtime fields may
join to static game definitions. A bridge never turns a CTB row into character
ownership and never promotes a historical field tag without current-build
evidence.
