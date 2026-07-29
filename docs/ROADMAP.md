# Roadmap

## Foundation

- define capture-source and canonical-event contracts;
- wire the implemented folder-package loader into desktop startup and settings;
- expose installed plug-in diagnostics, enable/disable state, and permissions;
- preserve unknown packet evidence without classifying it as safe;
- record region, build, and protocol-pack evidence;
- establish sanitized replay fixtures.

## Parser

- stream offline pcap and pcapng replay;
- decode Ethernet, Linux cooked capture, loopback, IPv4/IPv6, and TCP;
- reconstruct bounded directional TCP streams with loss evidence;
- add bounded IP fragment reconstruction;
- implement BPSR framing once;
- add native RLogs replay;
- build immutable protocol packs per region and client build;
- map packets to typed canonical events;
- measure route, byte, and event coverage continuously.
- compile human-readable game-data/localization/icon end products into indexed
  build artifacts;
- expand selective decoders across every reviewed scene, map, dungeon, entity,
  monster, skill, status, equipment, profile, party, and permitted chat route.

## Native plugins

- combat meter and encounter boundaries;
- complete dungeon timeline;
- character profile projection;
- rDPS and support contribution ledger;
- overlays and local integrations;
- resumable log submission.

## Service

- Discord identity-only website authentication;
- revocable per-device desktop authorization;
- region-aware character claims;
- server-side log replay and verification;
- profiles, reports, rankings, and public APIs.
