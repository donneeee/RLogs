# Canonical timeline, Event Viewer, and leaderboard source

## One source of truth

The sealed canonical timeline is the authoritative evidence for a combat-log
submission. The Event Viewer, Combat Meter, dungeon timeline, local result
preview, rDPS attribution, and leaderboard service are consumers of that same
append-only event stream. None of them owns a second combat history.

```text
captured packet
    |
    v
framing and route/opcode decode
    |
    v
privacy-reviewed typed canonical event
    |
    +--> Event Viewer (raw IDs)
    +--> localization and presentation
    +--> local analysis plug-ins
    +--> sealed .rlog chunks
              |
              v
        server verification and replay
              |
              v
        versioned DPS/rDPS/rankings
```

This is deliberately similar to an event-based log service: the upload
contains the facts needed to reproduce a result, not a client-authored ranking
score.

Event schema v2 adds `recorder_pause`, a completed user-requested capture
pause interval containing exact monotonic start and resume timestamps. It is
written when capture resumes, remains part of run wall time, and is distinct
from involuntary capture/decode gaps. The reader remains compatible with
schema-v1 logs.

Event schema v7 adds `party_roster_observed` without changing the older
`party_changed` projection. It distinguishes a complete join snapshot from an
incremental member observation, a single member-leave notification, and a
party-dissolve notification. Exact numeric party IDs and raw leave-type values
are retained when present. Each listed member also retains the exact raw entry
time, online-state, scene, and subgroup integers when the packet supplies them;
their meanings are not inferred. A partial observation is never treated as a
full roster, and logs predating schema v7 remain explicitly without this
evidence.

## Canonical boundary

The Event Viewer begins after the trusted game plug-in has framed, decoded, and
privacy-filtered a message, but before a localization or alias plug-in replaces
an ID with display text. A row therefore retains stable evidence such as:

```text
00:42.381  damage  entity:8831 -> entity:9912  ability:120430  amount:18452
00:43.004  cast    entity:9912 -> entity:8831  ability:70112   completed
00:47.810  life    entity:8831                              died
```

Each event keeps:

- monotonic timeline sequence and observed time;
- optional authoritative game/server time;
- typed event kind;
- canonical source, direct source, target, ability, status, monster, scene, and
  encounter identifiers when applicable;
- decoded numeric facts and flags;
- evidence provenance and confidence;
- explicit capture, TCP, unknown-route, and decoder gaps.

Support attribution is projected as two allegiance-neutral relationships over
that ordered evidence:

```text
provider -> effect/status lifecycle -> recipient or enemy target
                                      |
                                      v
                           recipient damage action -> recipient or enemy target
```

The lifecycle endpoint and the damage-action target are independent
identities. Either may later be proven to be a party member, a hostile entity,
some other entity, or may remain unresolved. In particular, the endpoint of a
damage action is
not assumed to be an enemy: reflected damage, friendly-fire-like mechanics,
and other player-targeted actions must remain representable. Actor kind and
party membership come only from event-time canonical evidence. The projection
does not infer allegiance from the direction of an arrow, a localized name, or
a current character snapshot.
The downward relationship has two exact, separately reviewed forms. For a
source-side support effect, the lifecycle endpoint must equal the later damage
actor. For a target-side vulnerability or mitigation-changing effect, the
lifecycle endpoint must equal the later damage target. Ordered timestamps and
an identity match identify only a candidate relationship; effect kind,
magnitude, scope, stacking, operation order, and integer rounding still need
exact-build proof before any provider credit is transferred.
When the packet does not prove the provider, the provider identity remains
nullable evidence; the relationship is not discarded and no provider is
invented.

For BPSR build `24687926`, the support-timeline projection also exposes raw
entity attribute `194` (`AttrTeamId`) and attribute `195`
(`AttrTeamMemberNums`) as allegiance-neutral `party_affiliation` observations.
The numeric meaning is gated to that exact build; another build retains the
canonical raw attribute event but leaves its party meaning unresolved. A
positive team ID is rendered as a decimal string, zero is an observed clear,
and malformed or non-canonical varints remain unresolved. Matching last-seen
team IDs are evidence for later scope review, not party-membership, formula,
rDPS-credit, runtime, or UI authority while current-build protocol-event
coverage remains open.

BPSR healing is carried by the same `DamageInfo` result as damage. Its
canonical healing event retains the packet's actual value, HP/shield loss,
semantic hit, damage source/type, normal/lucky values, owner level/stage, hit
parts, passive, mode, and enclosing skill-effect component identity. Generic
meters may ignore those research fields, but offline formula and rDPS proof
must not reconstruct or lose them.

Packet bytes, encrypted payloads, passwords, login tokens, account
authentication, private chat, and unreviewed protocol fields never cross this
boundary. “Raw” in the Event Viewer means raw canonical IDs and values, not raw
network data.

## Viewer behavior

The native Event Viewer is a read-only projection. It will support:

- completed `.rlog` replay through resumable, bounded server-side pages;
- topic, event-kind, and canonical-ID search without loading a full run into
  the browser;
- a stable detail view with provenance and exact canonical JSON;
- decimal-string transport for 64-bit IDs and amounts so JavaScript cannot
  round gameplay evidence;
- copy of selected sanitized events for plug-in debugging.

Live follow, richer field-specific filters, optional localized labels displayed
beside canonical IDs, and multi-event export remain subsequent viewer slices.

The writer appends an event once. Consumers receive references or bounded
stream batches. Viewer indexes are disposable local accelerators and are not
uploaded as evidence.

## Submission artifact

A run submission references sealed `.rlog` chunks and includes enough identity
to select the correct decoder and ranking partition:

- game, canonical schema, log-format, and client versions;
- protocol-pack and observed game-build evidence;
- region, realm/server, character identity, and run/encounter identity;
- ordered canonical events and data-gap evidence;
- chunk digests and final artifact integrity seal.

Live upload may transmit acknowledged chunks while a run is active, but a
report becomes complete or rank-eligible only after the final seal and
server-side checks. Post-run upload sends the same sealed artifact. Profile
snapshots use a separate typed and consent-controlled website payload; they do
not need to be embedded in a public combat report.

## Leaderboard calculation

The website must not trust DPS, rDPS, deaths, clear time, or a rank supplied by
the client. The server:

1. validates the seal, sequence, identity, build, region, and gap evidence;
2. replays encounter boundaries and canonical events;
3. resolves actor ownership and eligibility;
4. calculates damage, healing, deaths, activity, mechanics, and support
   attribution;
5. stores the result with explicit parser, encounter-rule, and attribution-rule
   versions.

Versioned calculation policies let rLogs correct an rDPS formula, exclude a
broken skill, or change encounter boundaries and then recalculate historical
reports without asking users to upload again. Local calculations are useful
previews, but the server-owned replay is the leaderboard authority.

Runs with packet loss, unknown relevant routes, decode failures, unsupported
builds, manual events, or impossible ordering remain inspectable. A versioned
eligibility policy decides whether they are ranked, unranked, or invalid; the
client must not silently discard those gaps.

## Run and segment projections

The canonical log remains one sealed run. Dungeon `mobbing` and `boss` logs,
raid routes, encounter attempts, and winning pulls are sequence/time-range
projections over it rather than duplicated event streams. Wipes and repulls
stay inside the active run; an exit, failure, end, changed instance, or new
authoritative run start creates a separate run.

Run wall time, segment wall time, total time spent across attempts, elapsed
first-pull-to-clear time, and winning-pull time are distinct measurements.
Boss adds do not define boundaries. See
[`RUN_SUBMISSIONS.md`](RUN_SUBMISSIONS.md) for the reducer and leaderboard
contract.

## Storage encoding

Log-format v2 stores canonical events in independently compressed, bounded zstd
blocks. The reader streams one block at a time, enforces encoded/decoded/event
limits before exposing its records, and verifies the same canonical SHA-256
event digest at the final seal. This reduces archive size without pruning,
summarizing, localizing, or reordering the timeline.

Legacy v1 newline-delimited JSON logs remain replayable through format
auto-detection. The encoding version is separate from the canonical event
schema and from the server-owned calculation policy.
