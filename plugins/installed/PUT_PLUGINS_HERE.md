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
  rlogs/
    plugins/my-plugin/         private game-neutral external assets
    shared/my-plugin/          provider-owned game-neutral shared assets
  <game-id>/
    plugins/my-plugin/         private game-specific external assets
    shared/                    canonical shared game assets
```

Do not place loose DLL, WASM, ZIP, JSON, or asset files directly in
`installed/`. rLogs discovers child folders containing `plugin.toml`. Every
entrypoint stays inside the package. A resource either stays in that package
or selects a host-derived game-neutral namespace under
`assets/rlogs/`; the manifest cannot choose a different plug-in's folder.
Game-specific resources remain under `assets/<game-id>/`.

Bundled game integrations under `plugins/games/` publish named read-only
resources. Import those resources in `plugin.toml` instead of copying game
catalogs, UUID/UID tables, localization, or icons into another plug-in.

See [`../examples/bpsr-uid-aliases/`](../examples/bpsr-uid-aliases/) for a
data-only plug-in that runs after canonical localization.
