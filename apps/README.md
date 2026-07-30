# Applications

User-facing applications live here. Applications compose engine services and
plugins; they do not own packet decoding or combat formulas.

- [`plugin-lab/`](plugin-lab/) is the current read-only UI for plug-in
  discovery, API compatibility, resources, dependency order, hook plans, and
  diagnostics.
- [`desktop/`](desktop/) contains the production desktop shell boundary. Its
  framework-free UI prototype consumes plug-in workspace descriptors; it does
  not duplicate Plugin Lab.
