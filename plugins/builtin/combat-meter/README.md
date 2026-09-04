# Combat timeline

The first executable bundled rLogs analysis plug-in. It consumes only
privacy-reviewed canonical events and maintains two independent projections:

- a compact live snapshot for the overlay; and
- a filterable capture-time history snapshot that is saved beside the local
  history index without replaying the sealed `.rlog`.

The projections contain:

- encounter and active-combat duration;
- attributed damage, HP loss, shield loss, healing, overheal, and shielding;
- casts, hits, critical hits, deaths, and revives;
- position sample counts and raw path distance;
- actor, ability, target-entity, and status-effect breakdowns;
- entire-run, mobbing, boss, and repeated-segment views;
- one-second damage, effective-healing, and damage-taken graph buckets;
- data-gap count and replay provenance.

History `eDPS` divides damage by the selected encounter or segment elapsed
time and freezes on its packet-proven clear. Live `eDPS` uses the cumulative
encounter-combat clock across retries while excluding recovery and transition
gaps. Live `aDPS` uses only the current attempt's active-combat clock and resets
on a packet-proven wipe or forced reset. These semantic rates do not change when
the overlay's visible timer is cycled; the selected timer instead reprojects the
generic DPS/HPS/TPS/rDPS columns. The full-run `DPS` clock freezes on a run
completion packet or scene departure. Gauntlet scenes keep one encounter clock
across intermediate bosses and summoned mobs. HPS is healing per selected
elapsed second; TPS is damage taken per selected elapsed second.

rDPS is produced only when the active game plug-in supplies reviewed,
build-matched contribution rules. The reducer follows exact status-effect
lifecycles, attributes only damage observed inside those windows, subtracts
the external contribution from the recipient, and grants the same integer
amount to the provider. Multiplicative overlaps use a symmetric Shapley split
and largest-remainder rounding, so party raw damage and party rDPS remain
exactly equal. History also retains each actor's adjusted damage and the exact
amount granted and received for audit. The player detail layer exposes the same
packet-proven relationships as a compact influence table: effect, provider,
recipient, affected damage ID, target entity, affected event count, observed
damage, and exact attributed delta. Missing matching-build proof is shown as
missing proof; it is never replaced with a static-table estimate.

Unclassified, conditional, haste, stat, and critical-rate effects remain in
the canonical timeline but do not receive invented contribution. APM is also
nullable until the game plug-in classifies player-pressed active skills, role
skills, and Imagines; hit packets and passive effects must never inflate it.
