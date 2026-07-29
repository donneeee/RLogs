# Put installed plug-ins here

Create or extract each plug-in as its own folder:

```text
plugins/
  installed/
    my-plugin/
      plugin.toml
      bin/ or web/            optional executable content
      resources/              small IDs, aliases, schemas, and other data

assets/
  my-plugin/                   private external assets
  shared/
    my-plugin/                 provider-owned assets exported for reuse
```

Do not place loose DLL, WASM, ZIP, JSON, or asset files directly in
`installed/`. rLogs discovers child folders containing `plugin.toml`. Every
entrypoint stays inside the package. A resource either stays in that package
or selects the host-derived `assets/my-plugin/` or
`assets/shared/my-plugin/` namespace; the manifest cannot choose a different
plug-in's folder.

Bundled game integrations under `plugins/games/` publish named read-only
resources. Import those resources in `plugin.toml` instead of copying game
catalogs, UUID/UID tables, localization, or icons into another plug-in.

See [`../examples/bpsr-uid-aliases/`](../examples/bpsr-uid-aliases/) for a
data-only plug-in that runs after canonical localization.
