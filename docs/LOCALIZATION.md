# Localization add-on contract

Localization is presentation data, never parser logic.

The native engine emits stable, language-neutral identifiers and numeric
values. Canonical event variants, route keys, actor IDs, ability IDs, status
IDs, scene IDs, regions, client builds, and evidence confidence do not change
when the display language changes.

## Locale add-ons

User-facing applications and plugins resolve translation keys from locale
add-ons. Add-ons use BCP 47 language tags such as `en-US`, `zh-CN`, `ja-JP`,
`ko-KR`, `de-DE`, `fr-FR`, `es-ES`, and `pt-BR`. The first-party English
package lives at `plugins/builtin/localization/en-US/` and uses the same public
contract that future first-party and community locale packages will use.

A localization add-on is data-only. It has no executable runtime, event
subscriptions, network access, capture access, or native-code permission. Its
independent namespaces are:

- `ui/` for RLogs interface, accessibility, validation, and workflow text.
- `games/<game-plugin-id>/game/` for each installed game's official names and
  descriptions addressed by stable game-data localization keys.

One add-on contains one locale. Selecting a locale loads that add-on and the
minimum fallback package only; it never loads every shipped language into
memory.

Resolution order is:

1. exact requested locale;
2. the locale's base language when available;
3. `en-US`;
4. the stable translation key itself.

Game-data names remain separate from interface text inside the add-on because
official names may vary by client build. A protocol pack maps wire IDs to
language-neutral IDs. The shared game-data catalog stores stable localization
keys, while each locale add-on supplies text and preserves exact client-build
availability and provenance on game entries. UI entries are versioned against
the RLogs application/API release instead.

Official localization exported from the game is a preferred reviewed end
product. During the mapping phase it remains under
`plugins/games/<game>/game-data/catalog/localization/<locale>/` so the catalog
compiler can prove
that every promoted record resolves in every reviewed language. Once IDs and
domain ownership stabilize, those JSON shards will move without rewriting
their stable keys into:

```text
plugins/builtin/localization/
  en-US/
    plugin.toml
    ui/<surface>/<shard>.json
    games/
      app.rlogs.game.blue-protocol-star-resonance/
        game/<domain>/<shard>.json
```

The current catalog location is a migration staging layout, not the runtime
ownership model. Extraction tooling remains outside RLogs. Player region never
duplicates or selects locale data.

## Stable key rules

- Keys describe meaning, not English wording: `combat.damage.total`, not
  `total-damage-label`.
- Placeholders are named and typed: `{player}`, `{amount}`, `{duration_ms}`.
- Code never parses a translated string to recover an ID or state.
- Missing translations are visible during development and never silently
  converted into a different event.
- Plugin manifests declare their supported locales and may ship namespaced
  keys without replacing RLogs or official-game keys.
- Locale add-ons may replace text for the same locale and key only through an
  explicit user-selected override; installation order never silently wins.

This separation lets one captured run be replayed, analyzed, submitted, and
displayed in any supported language without changing its ranking data.
