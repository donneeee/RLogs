# Localization contract

Localization is presentation data, never parser logic.

The native engine emits stable, language-neutral identifiers and numeric
values. Canonical event variants, route keys, actor IDs, ability IDs, status
IDs, scene IDs, regions, client builds, and evidence confidence do not change
when the display language changes.

## Locale packs

User-facing applications and plugins resolve translation keys from locale
packs. Packs use BCP 47 language tags such as `en-US`, `zh-CN`, `ja-JP`,
`ko-KR`, `de-DE`, `fr-FR`, `es-ES`, and `pt-BR`.

Resolution order is:

1. exact requested locale;
2. the locale's base language when available;
3. `en-US`;
4. the stable translation key itself.

Game-data names are stored separately from interface text because names may
vary by deployment, client build, and official localization. A protocol pack
maps wire IDs to language-neutral IDs. A region/build game-data pack supplies
localized names for those IDs.

## Stable key rules

- Keys describe meaning, not English wording: `combat.damage.total`, not
  `total-damage-label`.
- Placeholders are named and typed: `{player}`, `{amount}`, `{duration_ms}`.
- Code never parses a translated string to recover an ID or state.
- Missing translations are visible during development and never silently
  converted into a different event.
- Plugin manifests declare their supported locales and may ship namespaced
  keys without replacing core keys.

This separation lets one captured run be replayed, analyzed, submitted, and
displayed in any supported language without changing its ranking data.
