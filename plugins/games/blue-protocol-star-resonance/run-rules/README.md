# BPSR run rules

This tree contains versioned, build-specific meanings that the game-neutral
run reducer cannot know on its own.

```text
run-rules/<deployment>/<channel>-<client-build>/activities/<activity>.json
```

Each file may map reviewed scenes, objectives, boss monster IDs, routes,
segments, and difficulty semantics. A rule is runtime-enabled only after an
exact-build observation supports its boundaries. Candidate rules remain
readable in the same activity file but cannot affect a run.

Difficulty families and tiers are separate. In BPSR, `normal` and `hard` are
families without tiers. `master` is one family with the bounded tier range
`1..20`, displayed as `M1` through `M20`. The raw wire difficulty ID is always
preserved; it becomes a tier only when the active exact-build rule validates
that interpretation.

These JSON files contain semantic end products only. Game-file acquisition,
table probing, and packet-research tooling stay outside the parser.
