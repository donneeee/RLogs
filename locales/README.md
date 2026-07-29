# Locale package migration marker

Standalone interface locale folders are no longer the target architecture.
Official game text and RLogs UI text will live together, in separate
namespaces, inside data-only add-ons under
[`plugins/builtin/localization/`](../plugins/builtin/localization/).

See [`docs/LOCALIZATION.md`](../docs/LOCALIZATION.md) for fallback and key
rules. This redirect remains until the locale add-on loader and package
validator land, so new strings are not accidentally added to the old layout.
