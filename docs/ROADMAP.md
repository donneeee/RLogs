# Roadmap

## Foundation

- define capture-source and canonical-event contracts;
- [done] wire the folder-package loader into desktop startup and settings;
- [done] expose installed plug-in diagnostics, persisted enable/disable state,
  dependencies, permissions, subscriptions, and safe workspace metadata;
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
- [done] expose sealed post-decode, pre-localization canonical logs through a
  bounded Event Viewer with exact-ID filtering, resumable pagination,
  integrity verification, and replay details;
- [done] follow the writer's acknowledged event stream in the bounded Event
  Inspector, with true frozen review, lazy protocol detail, pinning, comparison,
  and disabled evidence-backed trigger drafts;
- expand the implemented route, byte, decoder, feature, event, and gap coverage
  report into continuous live diagnostics;
- compile human-readable game-data/localization/icon end products into indexed
  build artifacts;
- expand selective decoders across every reviewed scene, map, dungeon, entity,
  monster, skill, status, equipment, profile, party, and permitted chat route.

## Native plugins

- [done] persist recorder pause and capture-gap evidence and replay sealed logs
  through the built-in deterministic Encounter Recorder;
- [in progress] expand the deterministic run reducer from explicit run,
  mobbing, boss, attempt, retry/repull, pause, and quality boundaries;
- [in progress] retain BPSR's raw full-snapshot dungeon flow evidence and wire
  reviewed authoritative boss, retry, raid-route, and completion meanings
  into game-owned versioned rules;
- [done] expose the segmented dungeon timeline and winning-attempt comparison
  through the sealed-log Run Report;
- [done] merge personal BPSR profile observations from sealed canonical logs
  into privacy-reviewed character packages while excluding public social
  lookups for other characters;
- rDPS and support contribution ledger;
- overlays and local integrations;
- [done] build verified sealed-log upload artifacts with deterministic
  resumable chunks and atomically persist real captures as local unlisted
  drafts;
- [done] recover existing sealed `.rlog` files into the local draft queue and
  re-verify every immutable artifact identity on demand without network access;
- [done] add independent disabled-by-default Log Uploader and BPSR Profile Sync
  workspaces with atomically persisted consent and automatic-action policies;
- [done] exercise artifact re-verification, chunk acknowledgement, forced
  restart recovery, and final receipts against a bounded zero-network mock
  receiver;
- [done] add device authorization and real resumable authenticated transport,
  retain or remove successful artifacts only after a verified server receipt,
  and expose bounded automatic-upload progress and retry errors in the desktop;
- [done] atomically persist current BPSR character-profile packages behind the
  separate Profile Sync permission and expose bounded summaries plus lazy exact
  JSON review;
- [done] derive local previews from the same canonical events submitted for
  server-owned leaderboard replay.

## Community runtime

- add a sandboxed component adapter for directory-installed plug-ins;
- [done] expose persisted enable/disable state and require the manifest's
  unsafe-native permission before supervising a native developer package;
- [done] mount validated installed browser/native settings surfaces without
  exposing arbitrary package files;
- [done] add deterministic sealed-fixture suites and strict manifest/code
  compatibility checks to the Rust SDK;
- keep native game decoders outside the ordinary community trust boundary.

## Service

- Discord identity-only website authentication;
- revocable per-device desktop authorization;
- region-aware character claims;
- server-side sealed-log replay, data-quality verification, and versioned
  leaderboard calculations;
- completed-run pruning, attempt-aware dungeon reports, season partition
  lifecycle, and archived leaderboards;
- profiles, reports, rankings, and public APIs.
- exact-build module display and Web Worker/WASM optimization;
- [done] game-like, localized all-specialization talent boards backed by
  selected-node profile state while keeping all static tree data website-owned.
