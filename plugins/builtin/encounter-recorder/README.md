# Encounter recorder

This built-in native plug-in streams a verified sealed `.rlog` through the
game-neutral run reducer and publishes a versioned run-projection snapshot. It
subscribes only to encounter, dungeon, and data-quality events.

The recorder will present one sealed dungeon as whole-run, mobbing, boss, and
attempt views. It retains failed pulls, time spent recovering between repulls,
and the winning pull. Exiting and entering a dungeon creates a new run instead
of incrementing the attempt count. The deterministic projection contract lives
in [`docs/RUN_SUBMISSIONS.md`](../../../docs/RUN_SUBMISSIONS.md).

Packet/decode gaps and canonical user-requested pause intervals are retained
as evidence. They remain in wall time and downgrade a completed run to review
instead of silently disappearing from a leaderboard submission.
