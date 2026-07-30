# Run segmentation and submission contract

## One artifact, multiple views

A completed dungeon produces one sealed `.rlog`. Mobbing, boss, individual
pull, and whole-run reports are deterministic time and event ranges projected
from that artifact. They are not copied into independent log files.

This keeps uploads small, preserves a single evidence chain, and lets a newer
version of the server rules recalculate old reports without another upload.

```text
sealed dungeon run
    |
    +-- whole run: start notification -> completion notification
    |
    +-- mobbing segment
    |     +-- pull 1
    |     +-- retry or later wave
    |
    +-- boss segment
          +-- failed pull
          +-- recovery/repositioning
          +-- winning pull
```

## Time fields

The service must keep these measurements separate:

| Measurement | Meaning |
| --- | --- |
| Run wall time | Authoritative run start through authoritative completion. Includes traversal, cutscenes, loading observed in the instance, retries, recovery, and recorder pauses. |
| Segment wall time | Segment start through segment end, including time outside combat. |
| Active combat time | Sum of bounded combat windows. |
| Total attempt time | Sum of the wall time of every bounded pull. Excludes gaps between pulls. |
| Elapsed trying time | First pull start through the final observed pull end. Includes recovery and repositioning between pulls. |
| Between-attempt time | Elapsed trying time minus the summed pull durations. |
| Winning attempt time | Start through clear of the final successful pull. For a boss segment, this is the winning boss pull shown beside total time spent trying. |

The leaderboard can therefore display, for example, `9:00 spent trying`,
`7:15 in pulls`, and `3:42 winning pull` without changing the dungeon's full
completion time.

## Retries, repulls, and restarts

An encounter ending in `wiped`, followed by another start for the same
versioned encounter identity in the same active run, is a retry or repull.
Attempts are one-based and every failed attempt remains visible.

These are not retries:

- exiting the dungeon and entering it again;
- an authoritative run end or failure followed by another start;
- a new dungeon instance ID; or
- a new route or difficulty.

Those boundaries close the old run as incomplete, failed, ended, or exited and
create a separate run. Only an authoritatively completed run can proceed to
leaderboard eligibility checks.

## Mobbing, bosses, and adds

Dungeon segment changes come from explicit decoded dungeon/boss notifications
or a versioned encounter ruleset. A monster spawn, target change, or damage
event can never select a segment. Adds spawned by a boss therefore remain
inside the boss segment.

Mobbing may contain multiple successful waves, so every successful attempt is
retained. The final cleared attempt is also exposed for interfaces that need a
single winning-pull field.

## Pauses and evidence quality

Manual recorder pauses never reduce run wall time. Their exact intervals and
durations are included as evidence, and the initial policy marks the completed
run as needing review instead of silently ranking it. Capture gaps, manual
boundaries, non-authoritative start/completion signals, and forced boundary
closures are handled the same way.

Event schema v2 persists a completed user-requested pause interval as a
`recorder_pause` event when capture resumes. The Encounter Recorder consumes
that same event during local and server replay. Connecting an actual pause and
resume button to the live capture adapter remains a later capture-control
step; stopping a capture is not treated as pausing it.

## Raids

Raid segmentation is supplied by data rather than hard-coded to the dungeon
shape. A route can represent a selected single boss or a three-boss gauntlet.
Each boss keeps its own attempts while the route retains its full wall time.
Portal selection, route, difficulty, and authoritative completion packets will
come from the BPSR game plug-in's versioned rules.

## Seasons and archives

A leaderboard partition key contains:

- season;
- activity;
- difficulty;
- optional route;
- encounter-ruleset ID; and
- encounter-ruleset version.

The key recorded with a run is immutable. A partition can move from `active`
to `frozen` to `archived`; archiving removes it from current-season rankings
but never deletes reports or rewrites their original season. Recalculation
creates a versioned result under the applicable rules rather than modifying
the source log.

## Current decoder boundary

The BPSR plug-in now retains the full dungeon-flow snapshot as raw canonical
evidence, including the known flow phase, five timing fields, dungeon-times
value, result value, instance UUID, difficulty ID, and objective identities.
It does not yet interpret timer units or convert result values into completed
or failed run boundaries.

That semantic promotion requires reviewed current-build dungeon captures.
Until then, an observed `end` or `settlement` phase is timeline evidence, not
proof that a run is leaderboard eligible. See the game plug-in's
`research/dungeon-capture-review.md` for the evidence checklist.
