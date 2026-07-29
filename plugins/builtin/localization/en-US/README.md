# English (United States)

This will be the bundled default localization add-on and the fallback for
missing user-facing text.

It will contain:

- `ui/`: all first-party RLogs interface and accessibility strings.
- `games/<game-plugin-id>/game/`: reviewed official English names and
  descriptions for each installed game, preserving build availability and
  provenance.

No strings move here until their stable IDs, domains, and ownership relations
have passed catalog validation. The package will use the same public,
data-only add-on contract as every other locale.
