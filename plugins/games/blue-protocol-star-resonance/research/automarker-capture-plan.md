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
- The reviewed `resonance-logs-cn` source provides a strong inbound-observation
  lead: it interprets `SeqPassiveSkillInfo` entries with skill IDs `1101..=1106`
  as numbered in-game player markers, retains their passive instance IDs and
  target positions, removes them when the matching passive instance ends, and
  clears its projection on scene change. RLogs already decodes the same
  `SeqPassiveSkillInfo` wire fields (`actor_uuid`, passive `uuid`,
  `target_uuid`, `skill_id`, and `target_position`), but currently consumes
  those entries only as specialization evidence and does not emit a marker
  event.
- Those passive containers are carried by the current build pack's existing
  inbound `WorldNtf` routes: `SyncNearEntities` (service `1664308034`, method
  `6`), `SyncNearDeltaInfo` (method `45`), and `SyncToMeDeltaInfo` (method
  `46`). This makes packet-only detection plausible without process-memory
  reading. It does **not** prove that Global Steam build `24687926` uses the
  same skill IDs or lifecycle, and it says nothing about the outbound placement
  request.
- The `resonance-logs-cn` projection treats these entries as numbered spatial
  markers with coordinates. The player's description may instead refer to
  actor-targeted party icons like FFXIV automarkers. A controlled capture must
  resolve whether the Tina/M17 action targets a ground/map position, a party
  member, or both before RLogs chooses a canonical event shape.
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

## Minimum controlled placement capture

For a mirrored or leader-host capture, filter on the explicit client host, not
one previously observed remote server. This retains a dungeon world-server
migration. `tools/windows/capture-client-host.ps1` uses `tcp and host
<client-ip>` and then creates the exact-flow connection sidecar from transport
metadata after capture. The PCAPNG and sidecar remain private research and must
not be committed or uploaded.

The first capture should stay short and target the user's reported placement
window. Because the markers were reportedly placed once and then persisted
through later pulls, start recording **before the first placement**, not at
boss engagement.

1. Capture the client that can successfully place markers through the normal
   game UI. No separate leader-identity capture is required. Start already
   authenticated and inside the chosen dungeon, and record its exact name,
   difficulty, and scene ID.
2. Record 10-15 seconds of idle traffic before the first placement. Note the
   exact installed build, scene ID, character and current actor/entity
   identities, and a synchronized wall-clock time.
3. Through the normal game UI, place marker 1, wait five seconds, place marker
   2, and continue one marker at a time with the same spacing. Record whether
   each action targets a map/ground coordinate or a party member and the local
   UI result.
4. Wait ten seconds after the final marker, engage the boss, and retain roughly
   the first ten seconds of combat. This brackets the placement and tests
   whether the marker state persists across engagement.
5. For every action, record a ledger entry with wall-clock timestamp,
   initiating character, marker number/icon, target character/actor/entity UUID
   or coordinates, expected action, and local UI result.
6. Keep the process-owned PCAP or PCAPNG, connection evidence, and private
   JSONL protocol journal local. Do not upload them or include chat, login,
   account, or authentication traffic in any shareable artifact.

One successful placement capture is sufficient to prove the client request
schema when it also correlates the same client's inbound acknowledgement or
marker-state update.
A simultaneous observer capture can separately corroborate cross-client
broadcast or resynchronization, but it is not a prerequisite for the native
placement implementation. If the `1101..=1106` passive entries correlate,
preserve the containing inbound route, passive instance ID, actor and target
UUIDs, target position, and end notification in the private evidence. Do not
promote the interpretation from IDs alone.

## Follow-up lifecycle controls

Run these as separate short captures after the initial placement trace so
each action has a clean packet delta:

1. Replace one marker in place, move it to a different position or target,
   attempt a duplicate, clear one marker, and clear all markers.
2. Wipe without resetting markers, then disconnect and reconnect a marked
   player to observe persistence and resynchronization.
3. Change scene and return to determine whether scene change truly clears the
   server state or only the local projection.
4. Optionally repeat after a leader transfer or from a non-leader client to
   document server rejection behavior. This is diagnostic coverage, not an
   implementation prerequisite; the game server remains the authority.

## Proof gate for native placement

Do not enable a native placement adapter until the capture proves all of:

- exact current-build client-to-server route and complete request schema;
- actor addressing (character ID, actor ID, entity UUID, or another lifetime
  identity) without guessing;
- correlated return/acknowledgement including success and error codes;
- server-to-client marker-state update observed by the placing client;
- marker slot/icon values, uniqueness and collision behavior;
- replace, individual-clear, clear-all, despawn, scene-change, wipe, and
  reconnect lifecycles;
- an idempotency/rate-limit strategy that cannot spam or leave stale marks;
- exact-build routing and a fail-closed response to packet gaps or protocol
  drift.

Until then, RLogs should expose only the deterministic, reviewed assignment
plan and clearly label every rendered badge as a local preview. Packet
injection and process-memory writes remain out of scope for this evidence pass.
Inbound detection, deterministic assignment planning, and any future native
executor must remain separate boundaries. A future executor must be host-owned,
explicitly enabled, exact-build locked, acknowledgement driven, rate-limited,
and idempotent. The server owns leader authorization; RLogs must surface a
rejection and must never present a rejected request as successful. Detection
and local display must continue to work while that executor is absent or
disabled.
