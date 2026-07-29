# rLogs assets

This directory holds install-time assets that should not be bundled into
plug-in code.

```text
assets/
├── <plugin-folder>/                 # private to one plug-in
└── shared/
    └── <provider-plugin-folder>/    # exported for other plug-ins to reuse
```

The host derives both folder names from the provider plug-in's package folder.
A manifest can select `plugin_assets` or `shared_assets` storage and a relative
path, but it cannot choose a different plug-in's namespace.

Shared assets are still accessed through declared resource exports and imports.
Consumers should not hard-code another provider's filesystem path.
