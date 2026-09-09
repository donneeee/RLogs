# English (United States)

This is the bundled default localization add-on and the fallback for missing
user-facing text. The first runtime-backed shards cover foundational Mechanics
Map controls plus the combat-history browser and graph inspector; other desktop
surfaces are still being migrated.

It contains or will contain:

- `ui/`: all first-party RLogs interface and accessibility strings.
- `games/<game-plugin-id>/game/`: reviewed official English names and
  descriptions for each installed game, preserving build availability and
  provenance.

Strings move here only after their stable IDs, domains, and ownership relations
have passed validation. The package uses the same public, data-only add-on
contract as every other locale.
