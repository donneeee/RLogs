# Engine

The engine is the small trusted foundation shared by live capture, offline
replay, plugins, local logs, and server verification.

Each folder owns one boundary:

- `capture`: platform-neutral capture records and sources;
- `protocol`: lossless packet evidence and protocol-pack routing;
- `events`: region-aware canonical events and ordered run timelines;
- `plugin-api`: public manifests, permissions, and subscriptions;
- `core`: future orchestration and isolated plugin hosting;
- `combat`: future deterministic encounter reducers;
- `profiles`: future privacy-reviewed character projections;
- `attribution`: future rDPS and support contribution ledger;
- `log-format`: future `.rlog` container and replay validation;
- `submission`: resumable upload state and integrity contracts.

Feature code must not bypass canonical events to read mutable parser state.

