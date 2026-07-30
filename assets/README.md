# rLogs assets

This directory holds install-time assets that should not be bundled into
plug-in code.

```text
assets/
├── <game-id>/
│   ├── shared/                       # canonical resources for that game
│   └── plugins/<plugin-folder>/      # game-specific plug-in resources
└── rlogs/
    ├── shared/<provider-folder>/     # game-neutral shared resources
    └── plugins/<plugin-folder>/      # game-neutral plug-in resources
```

Game names use filesystem-safe IDs such as `blue-protocol-star-resonance`; the
display name may still be `Blue Protocol: Star Resonance`. A manifest selects
an allowed storage class and a relative path, but it cannot escape the
host-derived namespace or claim another provider's resources.

Shared assets are still accessed through declared resource exports and imports.
Consumers should not hard-code another provider's filesystem path.
