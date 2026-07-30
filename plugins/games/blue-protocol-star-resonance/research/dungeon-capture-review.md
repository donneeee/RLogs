# Dungeon capture review

This note tracks the current-build evidence required before BPSR dungeon
packets can drive completed-run submissions. It intentionally separates raw
decoding from leaderboard meaning.

## Decoded now

The verified full snapshot route, `WorldNtf/23 SyncDungeonData`, preserves:

- dungeon instance UUID;
- difficulty ID;
- raw flow state and its known phase name;
- raw active, ready, play, end, and settlement time fields;
- raw dungeon-times and result fields;
- objective map keys and target IDs; and
- deduplicated flow and objective changes.

Known flow phase names are `null`, `active`, `ready`, `playing`, `end`,
`settlement`, and `vote`. Unknown numeric states remain unknown instead of
being discarded.

The selective decoder for `WorldNtf/24 SyncDungeonDirtyData` is implemented
and tested against bounded synthetic fixtures. The current Global protocol
pack deliberately keeps that route `candidate` and `opaque` until a
current-build capture verifies its on-wire shape.

Neither route currently turns a raw end or result value into `completed` or
`failed`. An end phase alone is insufficient evidence for leaderboard
eligibility.

## Meanings still requiring evidence

- units and exact semantics of every raw flow time field;
- numeric result values for completion, failure, abandonment, and any other
  outcome;
- whether `dungeon_times` counts entries, attempts, retries, or something
  else;
- authoritative activity/dungeon identity;
- boss engaged, wiped, defeated, and repull signals;
- route or portal selection for raids;
- season attachment at the run boundary;
- cutscene and transition markers; and
- whether the dirty route has changed in the current Global build.

Actor spawns, ordinary damage, and objective presence will not be promoted to
boss, wipe, or completion boundaries merely because they correlate in one
capture.

## Useful future samples

These do not need frame-perfect repeated actions. For each capture, record the
activity name, difficulty, outcome, approximate wipe count, and selected raid
route or portal when applicable.

1. A successful ordinary dungeon with no wipe.
2. A successful dungeon with one natural wipe and repull.
3. A naturally failed or abandoned dungeon, if one is readily available.
4. A single-boss raid route.
5. A raid gauntlet.

Login and account authentication must remain outside the capture window. No
password, token, or account-authentication route is required for this work.

## Promotion gates

A build-specific protocol pack may enable the dirty route or publish semantic
run boundaries only when:

- the captured Global payload matches the bounded decoder shape;
- result meanings are corroborated by at least two suitable observations or
  another authoritative signal;
- timer units have a consistent monotonic relationship to observed run time;
- boss and retry boundaries come from explicit packet evidence or a reviewed,
  versioned ruleset; and
- sanitized fixtures retain structural evidence without raw account data.

When those gates pass, the BPSR plug-in can map the evidence to the
game-neutral run reducer without teaching the core BPSR-specific opcodes.
