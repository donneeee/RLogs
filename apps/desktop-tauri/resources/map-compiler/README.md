# Local BPSR map compiler

Release builds place `rlogs-bpsr-map-compiler.exe` here before Tauri packages
the installer. The helper reads map textures only from the user's own installed
BPSR client and writes them only to rLogs' local runtime-data directory. Extracted
game assets are never bundled, committed, or uploaded.

The helper is built from `tools/bpsr-local-map-asset.py` with the exact package
versions in `tools/bpsr-map-compiler-requirements.txt`. UnityPy and its runtime
dependencies retain their respective upstream licenses; PyInstaller's bootloader
is distributed under its GPL exception for bundled applications.

`reviewed-map-assets.v1.json` is the fail-closed allowlist used by batch mode.
Every entry binds an exact packet-observed build to scene IDs, game addresses,
bundle hashes, texture dimensions, and the game-authored region transform.

The compiler's `--inventory-output` mode audits every
`ui/textures/scenemaps` address in the installed client's own `m0.pkg`. When
exact-build `SceneTable.json` and `SceneResourceTable.json` files are supplied,
it also joins scene IDs to asset families and records both input hashes. The
inventory is candidate evidence only: it never enables or extracts a map until
the complete entry is promoted into the reviewed manifest.

For `global/steam-24687926`, the reviewed allowlist covers all 56 scene-map
families that the exact current `SceneTable` and `SceneResourceTable` join to
live scenes. That includes cities, open-world regions, ordinary and heroic
dungeons, raids, towers, world-boss arenas, guild/activity maps, and housing;
it is not limited to the six maps that currently have reviewed dungeon
mechanic packs. Five families contain additional floor/foreground textures.
Their main game-authored map is enabled now, while the additional layers stay
in the audited inventory until layer selection is represented explicitly.

`--inventory-input` plus `--candidate-manifest-output` materializes only the
strict single-texture rows into a separate review directory. The resulting
candidate-only JSON is deliberately not a production allowlist. Batch
extraction caches the large address catalog and package index once, including
when validating the production manifest.
