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

The selective decoder for `WorldNtf/24 SyncDungeonDirtyData` is now verified
for Global Steam build `24252055`. The reviewed no-wipe sample contained 60
packets on that route with no decode failures, so the exact-build protocol
pack allows it through `sync_dungeon_dirty_data_v1`.

For that build, an end-state snapshot with result ID `1` is promoted to
`completed`. The promotion is not based on the end phase alone: it was
corroborated by all three objective completions, the reviewed boss death, the
settlement sequence, and return to the origin scene.

Scene `1631` is verified as Normal `Chaotic - Tina's Mindrealm`. Its versioned
rule maps objective `100178` to mobbing completion, `100176` to the boss-phase
gate, `100164` to the final objective, and monster ID `33701` to the boss.
Replay of the same canonical log now derives deterministic mobbing and boss
segments. Hard scene `1632` and Master scene `1633` remain disabled candidates
until captured. Master is modeled as one family with twenty tiers, `M1`
through `M20`, rather than twenty separate difficulty families.

## Static current-client findings

The current build's reviewed Master table contains 380 tier rows grouped as 19
dungeon activities with exactly 20 tiers each. The row identity is
`dungeon_id * 100 + tier` for every current row. Its season grouping is:

- season 1: 6 activities;
- season 2: 6 activities;
- season 3: 6 activities; and
- season 4: 1 activity.

The external seasonal scan records fingerprints for the related dungeon,
target, settlement, stage, title, raid, and scene tables. It rejects a changed
packed row size and emits a semantic activity diff against the previous
reviewed build. Repeated tier data is nested beneath one activity instead of
being published as 380 duplicate dungeon definitions. The reviewed runtime
catalog now contains four `dungeon-seasons/season-<id>.json` records. Each
stores the season's canonical dungeon references, the complete M1-M20 identity
range, and first/last tier-row bounds; full per-tier rows remain in the
external evidence package until differing score, power, reward, or affix
fields are proven. Catalog compilation rejects incomplete tiers, duplicate
activities, and missing or mismatched dungeon references.

Official tier text is not automatically authoritative. The current scan found
70 label-to-tier mismatches attached to two season 3 Master activities across
five Western locales. Numeric tier identity passed its invariants, but these
raw labels remain blocked from promotion. The catalog instead publishes one
reviewed `difficulty.master.label_format` per locale and substitutes the
numeric tier. English, French, German, Spanish, and Brazilian Portuguese are
marked reviewed corrections. Thai has correct numbers but two official styles;
the 361-of-380 majority form, `ระดับ Master {tier}`, is canonicalized. The raw
strings and all alternatives remain in the external audit.

## Meanings still requiring evidence

- units and exact semantics of every raw flow time field;
- numeric result values for failure, abandonment, and any other non-success
  outcome;
- whether `dungeon_times` counts entries, attempts, retries, or something
  else;
- authoritative activity/dungeon identity beyond the reviewed Normal scene;
- boss engaged, wiped, defeated, and repull signals;
- route or portal selection for raids;
- season attachment at the run boundary;
- cutscene and transition markers; and
- exact packet difficulty-tier semantics for Master `M1`-`M20`.

Actor spawns, ordinary damage, and objective presence will not be promoted to
boss, wipe, or completion boundaries merely because they correlate in one
capture.

## Useful future samples

These do not need frame-perfect repeated actions. For each capture, record the
activity name, difficulty, outcome, approximate wipe count, and selected raid
route or portal when applicable.

1. A successful dungeon with one natural wipe and repull.
2. A successful Hard dungeon.
3. A successful Master dungeon with its selected tier recorded.
4. A naturally failed or abandoned dungeon, if one is readily available.
5. A single-boss raid route.
6. A raid gauntlet.

Login and account authentication must remain outside the capture window. No
password, token, or account-authentication route is required for this work.

Collect these progressively rather than as one large test batch. Review the
first successful no-wipe dungeon before requesting the wipe/repull sample; if
it already resolves a route or timer field, the next capture can target only
the remaining uncertainty. The abandoned dungeon and raid samples are later
gates and are not required to begin ordinary dungeon mapping.

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
