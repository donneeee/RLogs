# Built-in localization add-ons

Each child folder is one BCP 47 locale package. Locale packages own both
official game presentation text and RLogs interface text without owning any
numeric IDs, packet rules, encounter logic, or ranking math.

```text
<locale>/
  plugin.toml
  ui/
    <surface>/<shard>.json
  games/
    <game-plugin-id>/
      game/<domain>/<shard>.json
```

These add-ons are pure data: no executable module, event subscription, capture
access, network access, or native permission is allowed. The loader will open
only the selected locale and its explicit fallback. Game entries retain
client-build availability and source provenance; UI entries target an RLogs
application/API version.

The official BPSR localization JSON currently remains in that game's plug-in
under
`plugins/games/blue-protocol-star-resonance/game-data/catalog/localization/`
while IDs and ownership are being mapped. It will be promoted to
`en-US/games/app.rlogs.game.blue-protocol-star-resonance/game/` after the
mapping schema stabilizes.
