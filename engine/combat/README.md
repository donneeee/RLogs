# Combat and run reducer

`rlogs-combat` deterministically projects ordered canonical events into runs,
segments, encounter attempts, and leaderboard eligibility evidence. The same
reducer can run locally for previews and on the service for authoritative
verification.

The reducer is game-neutral. A game plug-in supplies versioned activity,
difficulty, route, encounter, segment, and season rules. Actor spawns, monster
IDs, and damage events never change a segment. This prevents boss adds from
being misclassified as a return to a dungeon's mobbing section.

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
