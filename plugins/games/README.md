# Trusted game integrations

Each child folder is one self-contained game integration. Use the human game
slug as the folder name and a reverse-domain ID in `plugin.toml`.

```text
games/
  <game>/
    plugin.toml
    Cargo.toml
    src/                  framing, decoding, region, profile, website projection
    protocol-packs/       exact deployment/build knowledge
    protocol-references/  pinned public evidence
    game-data/            reviewed human-readable end products
    research/             sanitized mapping inventories
    tools/                game-specific research and validation CLIs
```

A game folder may depend on the game-neutral engine crates. Engine crates must
not depend on a concrete game plug-in. Ordinary community add-ons must use
`engine/plugin-api`; raw reconstructed streams are reserved for manifests
validated through `engine/game-plugin-api`. Core owns network capture and hands
the selected trusted integration only process-filtered reconstructed streams;
the integration owns framing, optional protocol decryption, decompression, and
decoding.

The first implementation is
[`blue-protocol-star-resonance/`](blue-protocol-star-resonance/).
