# Native automarker protocol capture plan

Status: **unresolved for global Steam build 24687926**. RLogs may preview a
reviewed assignment, but it must not claim or attempt native placement until
the game protocol below is proven from a controlled capture.

## Evidence already available

- The four Tina/M17, scene `1633`, submission artifacts from session
  `monitor-1788651367734` contain zero canonical `Map` events:

  | Run | Submission artifact SHA-256 | Map events |
  | --- | --- | ---: |
  | `run-0001` | `8acc19a630407d1f00cb9176a6d3af5e7ef373ca5808cb9cb690b8f41a61e688` | 0 |
  | `run-0002` | `199a57cfe0598793f43a3d2444a9f87fe7b1186a636bcff3f6712b8ba399024e` | 0 |
  | `run-0003` | `88a5a86b2fa28be5ebe325b960f116bbfb71117670b75cc1b5f32b2fbca6dfff` | 0 |
  | `run-0005` | `282c1f09ddd76c996c305c4497132b87bc9098d6731515cc59001c9d19c90889` | 0 |

  These are boss/run-scoped canonical submission artifacts. The result proves
  only that no decoded canonical `Map` event survived inside those retained
  slices. It does **not** prove that no native-marker action occurred before
  the slice, that no unknown/opaque route was present, or that the marker was
  not implemented locally by the game UI.
- The player reports one native party-visible marker-placement sequence per
  recorded log/run, at the start immediately before the boss was first
  engaged. The markers were not reapplied or reset after wipes; they reportedly
  persisted into later pulls. Treat the four initial placements as high-value
  search timestamps and any later marker traffic as a possible persistence or
  resynchronization notification, not a second user action. This remains user
  observation rather than protocol proof. The retained run scope should
  include each pre-engagement interval, so the absence of canonical `Map`
  events is consistent with an undecoded route or non-`Map` game state; it is
  not evidence that the reported action did not occur.
- Submission `.rlog` files contain canonical events, not packet/link headers or
  unknown route bodies. They cannot recover an undecoded marker request.
- No private PCAP/PCAPNG or selective research journal for these runs is
  present under the local RLogs data root.
- The canonical `MapEvent` model has add/update/remove shapes, but the current
  BPSR decoder has no producer for it. It is not evidence that the game exposes
  party-visible actor markers.
- Static game files expose `World.SetMapMark` and `World.RemoveMapMark` at
  service `103198054`, methods `65538` and `65539`. Their payloads are
  scene/map coordinates and custom text/icon data, not an actor target. Treat
  them as personal map-pin candidates, not automarkers.
- Static team files expose leader and group structure. Relevant candidates are
  `GrpcTeamNtf.NoticeUpdateTeamInfo` (service `966773353`, method `1`),
  `GrpcTeamNtf.NotifyTeamGroupUpdate` (method `29`), and
  `World.UpdateTeamGroup` (service `103198054`, method `311333`, fields
  `group_id`, `char_id`, `group_index`). This may explain leader-visible party
  ordering, but it is not proof of an overhead marker protocol.
- The current canonical party roster retains character identity and `group_id`
  but not `leader_id`, group index, or group membership ordering. Those fields
  must be added only after current-build packet verification.
- FFXIV automarker systems demonstrate why configured or alphabetic party
  order is unsafe: a party-list ordering mismatch can mark the wrong person.
  The current desktop projector converts roster membership to a set and sorts
  joined actors by display name. That is suitable for a HUD list, but it must
  never be used as native automarker slot authority.

## Controlled experiment

Capture two clients in the same party at the same time: the leader and one
non-leader. Use private, local PCAPNG capture; do not upload it or include chat,
login, account, or authentication traffic in a shareable artifact.

1. Record 10 seconds idle and note both character IDs, current actor IDs, party
   group/slot order, scene ID, and wall-clock time.
2. Through the normal game UI, have the leader place each available native
   marker type on one party member, waiting five seconds between actions.
3. Repeat on an enemy, if the UI permits it. Record whether both clients see
   the marker and whether it appears overhead, in party frames, or only on the
   map.
4. Replace a marker on the same target, move it to a second target, attempt a
   duplicate marker, clear one marker, and clear all markers.
5. Have the non-leader attempt the same operations and retain every success or
   denial result.
6. Transfer leadership, repeat one place/update/clear cycle, then disconnect
   and reconnect the marked player to observe persistence and resync.
7. As a separate control, reorder one member within a group and move one member
   between groups. This distinguishes party-slot synchronization from actual
   marker placement.

For every action, record a short action ledger with wall-clock timestamp,
initiating character, target character/actor/entity UUID, marker icon/slot,
expected action, UI result, and what the second client observed.

## Proof gate for native placement

Do not enable a native placement adapter until the capture proves all of:

- exact current-build client-to-server route and complete request schema;
- actor addressing (character ID, actor ID, entity UUID, or another lifetime
  identity) without guessing;
- leader/permission requirement and the server's denial behavior;
- correlated return/acknowledgement including success and error codes;
- server-to-client broadcast or resync route observed by the second client;
- marker slot/icon values, uniqueness and collision behavior;
- replace, individual-clear, clear-all, despawn, scene-change, wipe, reconnect,
  and leader-transfer lifecycles;
- an idempotency/rate-limit strategy that cannot spam or leave stale marks;
- exact-build routing and a fail-closed response to packet gaps or protocol
  drift.

Until then, RLogs should expose only the deterministic, reviewed assignment
plan and clearly label every rendered badge as a local preview. Packet
injection and process-memory writes remain out of scope for this evidence pass.
