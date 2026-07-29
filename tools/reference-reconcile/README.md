# Reference reconciliation

Compare the routes in one sanitized observation with curated, exact-revision
reference manifests:

```text
cargo run -p rlogs-reference-reconcile -- \
  --observation plugins/games/blue-protocol-star-resonance/protocol-packs/global/steam-24252055/observations/world-load-process-001.json \
  --json \
  plugins/games/blue-protocol-star-resonance/protocol-references/manifests/resonance-logs-cn-0.2.0.json \
  plugins/games/blue-protocol-star-resonance/protocol-references/manifests/resonance-logs-global-77380ca.json \
  plugins/games/blue-protocol-star-resonance/protocol-references/manifests/zdps-0.1.7.3.json
```

Projects in the same `lineage_id` contribute one vote. This prevents a parent
project and its fork from creating false independent agreement. A route is:

- `corroborated` when two or more independent lineages agree;
- `single_lineage` when only one lineage names it;
- `conflict` when claims disagree;
- `unmapped` when no manifest names it.

Reference agreement can justify a candidate route name. It cannot verify a
current-build payload shape, grant decoder permission, or override RLogs'
privacy policy.

Manifest validation also recomputes each service ID with the game's
BKDR-131 service-name hash (`hash & 0x7fffffff`). A typo or incorrect service
name therefore cannot silently become a route claim.
