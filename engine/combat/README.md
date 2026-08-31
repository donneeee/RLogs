# Combat and run reducer

`rlogs-combat` deterministically projects ordered canonical events into runs,
segments, encounter attempts, and leaderboard eligibility evidence. The same
reducer can run locally for previews and on the service for authoritative
verification.

The reducer is game-neutral. A game plug-in supplies versioned activity,
difficulty, route, encounter, segment, and season rules. An exact-build rule
may use a reviewed objective or boss monster ID together with hostile damage
to open a segment or attempt. Untyped actor spawns and damage never invent
those meanings, and boss adds cannot switch an active boss segment back to
mobbing.

Difficulty identity is deliberately split into a normalized family and an
optional tier. For example, BPSR Master dungeons use the `master` family with
tiers `M1` through `M20`; those tiers are not twenty unrelated difficulty
families. The raw packet difficulty ID is retained separately for audit and is
only normalized when the active game rule defines its meaning.

A dungeon is one run with `mobbing` and `boss` segment projections. A wipe and
repull inside the same run creates another encounter attempt. Exiting or ending
the instance closes that run; entering again creates a new run rather than a
retry.

Each segment reports:

- full segment wall time;
- active combat time;
- attempt and retry counts;
- summed time inside attempts;
- first-pull-to-final-pull elapsed trying time;
- time between attempts;
- all successful attempts; and
- the final cleared, or winning, attempt time.

Run wall time always remains the authoritative start-to-completion duration.
It includes traversal, cutscenes, recovery, repositioning, and any manual
recorder pause. See [`docs/RUN_SUBMISSIONS.md`](../../docs/RUN_SUBMISSIONS.md)
for the complete contract.

The desktop Run Report is a presentation of this serialized reducer output.
It never derives attempt boundaries, completion, or eligibility in
TypeScript. The future service will replay the same canonical artifact through
the same reducer and versioned game rules instead of trusting client totals.

The contribution reducer also accepts exact marginal transfers from an
optional game plug-in projector. This boundary keeps game-specific attributes,
formula stages, and packet timing outside `rlogs-combat`; the reducer only
checks the transfer, updates provider and recipient rDPS ledgers, and enforces
party-damage conservation. The raw damage event remains unchanged.
