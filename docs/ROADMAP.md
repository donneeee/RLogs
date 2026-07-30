# Roadmap

## Foundation

- define capture-source and canonical-event contracts;
- wire the implemented folder-package loader into desktop startup and settings;
- expose installed plug-in diagnostics, enable/disable state, and permissions;
- preserve unknown packet evidence without classifying it as safe;
- record region, build, and protocol-pack evidence;
- establish sanitized replay fixtures. (first reference combat fixture complete)

## Parser

- expand the implemented offline pcap/pcapng-to-rlog recording path;
- decode Ethernet, Linux cooked capture, loopback, IPv4/IPv6, and TCP;
- reconstruct bounded directional TCP streams with loss evidence;
- add bounded IP fragment reconstruction;
- implement BPSR framing once;
- expand native RLogs recording/replay beyond the sealed canonical-event slice;
- build immutable protocol packs per region and client build;
- map packets to typed canonical events;
- expand the implemented route, byte, decoder, feature, event, and gap coverage
  report into continuous live diagnostics;
- compile human-readable game-data/localization/icon end products into indexed
  build artifacts;
- expand selective decoders across every reviewed scene, map, dungeon, entity,
  monster, skill, status, equipment, profile, party, and permitted chat route.

## Native plugins

- expand the first combat meter and explicit encounter boundaries;
- complete dungeon timeline;
- character profile projection;
- rDPS and support contribution ledger;
- overlays and local integrations;
- resumable log submission.

## Community runtime

- add a sandboxed component adapter for directory-installed plug-ins;
- expose enable/disable state and user-reviewed permissions;
- add deterministic fixture suites and compatibility checks to the SDK;
- keep native game decoders outside the ordinary community trust boundary.

## Service

- Discord identity-only website authentication;
- revocable per-device desktop authorization;
- region-aware character claims;
- server-side log replay and verification;
- profiles, reports, rankings, and public APIs.
- exact-build module display and Web Worker/WASM optimization;
- game-like, localized talent boards backed by selected-node profile state.
