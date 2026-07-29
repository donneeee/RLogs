# Engine

The engine is the small trusted foundation shared by live capture, offline
replay, plugins, local logs, and server verification.

Each folder owns one boundary:

- `capture`: platform-neutral capture records and sources;
- `network`: link/IP/TCP decoding and bounded directional stream reconstruction;
- `protocol`: lossless packet evidence and protocol-pack routing;
- `events`: region-aware canonical events and ordered run timelines;
- `game-data`: validated, indexed runtime end products for IDs, localization, and assets;
- `plugin-api`: public manifests, permissions, and subscriptions;
- `plugin-host`: folder discovery, shared resources, dependencies, and ordered
  before/after operation plans;
- `core`: future orchestration and isolated plugin hosting;
- `combat`: future deterministic encounter reducers;
- `profiles`: future privacy-reviewed character projections;
- `attribution`: future rDPS and support contribution ledger;
- `log-format`: future `.rlog` container and replay validation;
- `submission`: resumable upload state and integrity contracts.

Feature code must not bypass canonical events to read mutable parser state.
