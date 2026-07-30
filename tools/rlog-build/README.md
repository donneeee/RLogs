# rLog fixture builder

Compiles a readable sanitized fixture source into the exact sealed `.rlog`
stream used by replay:

```text
cargo run -p rlogs-rlog-build -- \
  tests/fixtures/replay/reference-combat.source.json \
  tests/fixtures/replay/reference-combat.rlog
```

The source keeps region/build identity once and lists canonical event drafts in
observed order. The builder assigns public/timeline sequences and exact
synthetic fixture provenance. It contains no packet capture or game-file
extraction logic.
