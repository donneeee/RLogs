# Canonical events

This crate owns RLogs' region-aware public event contract and the ordered,
loss-aware history of a dungeon run.

It intentionally does not:

- capture packets;
- decode game-specific messages;
- calculate DPS or rDPS;
- save the final `.rlog` file;
- know anything about the desktop interface.

Those jobs belong to neighboring engine folders. Keeping canonical events
independent makes packet-decoder updates much less likely to disturb plugins,
calculations, or user interfaces.

## Core rule

Events are append-only. Every event receives a stable sequence number and must
not move backward in observed capture time.

When data is missing or cannot be decoded, add a `DataGap` event instead of
silently discarding the uncertainty.
