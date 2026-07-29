# Sanitized research indexes

Research inventories are game-owned. The current BPSR inventory lives at
[`plugins/games/blue-protocol-star-resonance/research/game-file-inventory/`](../plugins/games/blue-protocol-star-resonance/research/game-file-inventory/).
This top-level folder retains the rules shared by future game plug-ins.

This folder contains compact, build-scoped inventories and mapping worklists
that are safe to review with the RLogs source. It never contains raw game
payloads, packet captures, absolute install paths, credentials, private
communications, anti-cheat decoding, or acquisition/extraction code.

Research records are not runtime truth. Unknown and candidate evidence stays
inside its game plug-in until a typed schema and provenance review promotes it
into that plug-in's `game-data/` or build-specific `protocol-packs/`.
