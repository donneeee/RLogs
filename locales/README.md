# Locales

This folder will contain first-party interface locale packs organized by BCP
47 language tag:

```text
locales/
  en-US/
  zh-CN/
  ja-JP/
  ko-KR/
  de-DE/
  fr-FR/
  es-ES/
  pt-BR/
```

Only translated presentation strings belong here. Game-data names that vary
by region or client build belong with their versioned game-data/protocol pack.
Canonical events and log files contain stable IDs, never localized labels.

See [`docs/LOCALIZATION.md`](../docs/LOCALIZATION.md) for fallback and key
rules. Locale files will be added with the first user-facing RLogs application
so every key can be validated against a real consumer.
