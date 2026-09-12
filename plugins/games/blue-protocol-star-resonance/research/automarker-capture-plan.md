# Native automarker protocol capture plan

Status: **the passive inbound numbered ground-marker add schema and the
outbound placement route/request topology are proven on the live Global
service, but an executable sender remains blocked**. Both focused captures came
from remote clients whose exact builds were not independently proven. Their
live wire evidence is triangulated with the exact local build-25247556 registry
and deterministic derived-pack digest. RLogs may locally project only that
exact runtime tuple, save observed marker locations, and preview a reviewed
assignment. It must not attempt native placement. Current request field `1.5`
has the exact current-build name and type `uint32 sessionSequence`, but its live
generator state is owned by the game session. Authenticated attribute plaintext
field `6` remains semantically unresolved, and no active fail-closed placement
transport exists.
Exact build 24687926 retains its reviewed provisional observer support; no
neighboring or future compatibility build inherits the new observer gate.

Steam build `25247556` was installed on 2026-09-11. Its exact distribution
snapshot and executable hash may authorize a raw evidence capture. The
`24687926` protocol pack may be attached only as an explicitly unverified
carry-forward decoder hypothesis; it is not exact-build or runtime authority
for `25247556`. Use `-RawCaptureWithUnverifiedProtocolCarryForward`,
`-DistributionSnapshotPath`, and `-ProtocolPackSourceBuild 24687926` for that
mode. Raw packet evidence remains valid even if the older decoder rejects the
new stream.
- A private 8.307-second live-game-flow probe from build `25247556` produced
  841 packet records, including 442 routes recognized by the `24687926` pack,
  with zero decoder gaps. The observed `WorldNtf` methods `6`, `45`, and `46`
  and paired `UseSlot` calls/returns retain their earlier framing. This is
  evidence that the older pack remains useful for capture correlation, not
  proof of complete current-build routing or marker semantics. The bounded
  aggregate and private-artifact hashes are recorded in
  `steam-25247556/protocol-carry-forward-probe.v1.json`.
- A bounded read-only static audit of the installed build is recorded in
  `steam-25247556/automarker-static-route-audit.v1.json`. The exact installed
  `resources.assets` (`sha256:83c9d53f385e5b0eae711af4855c02216ff3224bcf303bcc96bc3f01a21f256d`)
  retains `World.SetMapMark` and `World.RemoveMapMark`, but exposes no named
  party/ground-waymark route. The result keeps native placement closed: these
  methods remain personal map-pin candidates, not party-visible automarkers.
- The current package's serialized `stru_use_slot_request.proto` descriptor
  proves that `UseSlotRequest` field `5` is exactly
  `uint32 sessionSequence`. A sanitized comparison across 132 current-build
  marker and ordinary skill requests shows that it is present everywhere and
  monotonic per gameplay session, while its observed rate varies with the
  client update cadence. It is therefore live session state, not a universal
  fixed-80-ms value. The schema and transport proof is recorded in
  `steam-25247556/use-slot-current-schema-transport-proof.v1.json`.
- A focused 300.333-second remote Tina M20 observer capture now proves six
  passive inbound ground-marker additions on the live Global service. The
  remote capture client's exact build was not independently proven. Independent
  direct-PCAP reassembly and the private carry-forward protocol journal agree
  on exactly six `WorldNtf.SyncNearDeltaInfo` notifications (service
  `1664308034`, method `45`) whose passive skill field path `1.8.2.6` carries
  `1101..1106` and whose position path `1.8.2.9` carries six distinct Vector3
  ground coordinates. The non-leader observer confirmed that the party leader
  placed markers 1 through 6 during the matching wall-clock interval. The
  sanitized runtime-targeted proof, private evidence hashes, exact timestamps
  and positions are recorded in
  `steam-25247556/inbound-ground-marker-observation-proof.v1.json`.
  Triangulation with the exact local build registry authorizes only local
  passive observation on the deterministic build-25247556/digest pair. No
  update or end was observed, and no outbound request,
  acknowledgement, permission model, replay, or packet transmission gained
  authority.
- A focused 90.402-second placing-leader Tina M20 capture now proves six
  one-to-one outbound request/return/self-delta correlations for normal-UI
  placements 1 through 6. Each outbound `FrameUp` (fragment `5`) contains a
  logical `Call` (fragment `1`) to service `103198054`, stub `1`, method
  `249858` (`World.UseSlot` under the carry-forward naming hypothesis). Request
  skill IDs `1101..1106` and target-position XYZ values match the leader's six
  method-`46` passive additions exactly; the separate observer capture proves
  the same IDs and position shape on method `45`. Every call receives both a
  matching four-byte FrameUp acknowledgement and an empty RPC return. The
  sanitized topology, exact timestamps and private-artifact hashes are in
  `steam-25247556/outbound-ground-marker-request-correlation-proof.v1.json`.
  The 80-byte `attr_data` envelopes all authenticate and decrypt with the
  reviewed gameplay-only keys, but current plaintext field `6` is new and its
  semantics/generation are unresolved. Enclosing request field `1.5` is the
  exact current-build `uint32 sessionSequence`; its live value must be allocated
  by the game rather than copied or extrapolated. This proves the route/request
  topology and the local save-location fields; it does not authorize an
  encoder, sender, replay, or placement adapter.
- A read-only current-build package audit identifies the normal dungeon-marker
  UI at `ui/view/main_copy_punctuate_view.lua`. It loads
  `weapon_skill:GetSceneMaskSkillList()`, whose current-build implementation
  selects `SkillSlotPositionTableMgr` rows with
  `SlotLogicType == SkillSlotLogicType.SceneMaskSkill`. The view invokes
  `Z.PlayerInputController:FlagSkill(info.id, true|false)` and uses
  `Z.PlayerInputController:StopSkill(info.skillId)` for deletion. Its leader-id
  comparison is game UI eligibility behavior for the currently active dungeon
  character, not an RLogs account or identity binding. The high-level call
  accepts no player UUID, action UUID, session sequence, authenticated payload,
  or transport counter, so those values remain fresh game-owned state and must
  never be copied from a capture. The sanitized static proof is recorded in
  `steam-25247556/ground-marker-high-level-action-proof.v1.json`.

The six-marker capture/save boundary also has a privacy-minimal regression
fixture at `tests/fixtures/automarker/tina-m20-six-marker-points.v1.json`. Tests
reconstruct the marker notification at runtime, pass it through the real
`LocalMapMarkerProjection`, publish the reducer-shaped result through
`ObservedMarkerFeed`, save schema-v4 portable content, reopen it, and compare all
six numbered XYZ positions exactly. The fixture contains no raw capture bytes,
network endpoints, UUIDs, session values, or private paths. Offline combat-PCAP
replay does not publish markers into this feed because marker state is
intentionally local and non-canonical; that separation is preserved rather than
inventing canonical automarker events.

## Placement transport architecture

The preset is portable content, not a captured request. Its durable data is the
dungeon-family identity, a local name/identifier, and numbered XYZ positions.
It must never persist an account, character, player/entity UUID, action UUID,
session sequence, synchronized timestamp, RPC/frame counter, authentication
field, or captured request bytes. At activation, RLogs revalidates the current
scene family and asks the currently logged-in game client to perform the normal
marker action; the game supplies every live identity and transport value for
that current player. The server remains the authority on whether that player is
the party leader.

Cross-user exchange uses a still smaller JSON contract: format kind/version,
name, dungeon-family identity, and numbered XYZ points. It excludes the local
preset identifier and save time as well as every build, scene, map, player, and
session field. Import validates the exact document shape and active family, then
loads an unsaved editor draft; the user must choose **Save As** before it enters
the local preset store. Import never invokes placement.

For exact saved XYZ, the preferred future architecture is an in-process call
through the exact-current-build high-level normal marker action. The current
package now proves the normal UI boundary as
`Z.PlayerInputController:FlagSkill(info.id, true|false)`. That keeps
`sessionSequence`, action identity, synchronized time, authenticated attributes,
RPC call IDs, `FrameUp` IDs, ordering, and retransmission inside the game. The
view obtains `info.id` from the exact-current-build scene-mask skill table and
does not accept a world `Position`. It instead drives `Skill_Horizontal` and
`Skill_Vertical` through `TouchManager.TouchController:TrySetAxis`; native
targeting or raycast logic resolves the ground point. The package also proves a
generated `zproto.World.UseSlot` Lua proxy that encodes `vRequest` and
dispatches through `LuaProxyCall`, but it does not expose a reviewed external
ABI. Calling that lower-level proxy directly remains blocked because the caller
would still have to construct the complete live request.

Normal game-UI input is the least invasive fallback, but the reviewed code and
artifacts contain no deterministic saved-world-XYZ to camera/screen/raycast
adapter. It can reproduce an approximate visible click, not an exact preset
coordinate. Raw TCP insertion or rewriting is not an acceptable alternative:
the RLogs network path is passive and owns neither the live connection's
sequence state nor retransmission. Do not add a stream sender.

The least-invasive next evidence is one read-only runtime call trace during a
normal UI placement, starting at `PlayerInputController.FlagSkill` and following
the native target/raycast handler immediately before `World.UseSlot`. Record the
method names, argument types, and the game-owned field or property holding the
resolved ground `Position`. If an existing high-level position setter is
proven, it is the preferred exact-XYZ boundary before normal `FlagSkill`
release. If no setter exists, a world-to-screen/camera projection must be
validated against occlusion, range, navigation, and ground-raycast rules before
the existing two skill axes can be considered deterministic. Until then,
`Place` remains disabled.

A bounded exact-build offline search has exhausted the retained static path.
The `m0.pkg` marker chunk contains the proven `FlagSkill`, `StopSkill`, and
skill-axis calls, but no target-position or game-raycast setter. A census of all
4,833 decoded Lua chunks found no controller/axis and plausible target-setter
co-occurrence. The exact `GameAssembly.dll` exposes only unrelated Unity
target/raycast strings, while the installed `global-metadata.dat` is the
reviewed zero-byte placeholder. Consequently there is no matching-build method
token, RVA, or callsite bridge that can safely substitute for the runtime
trace. The sanitized details are retained in
`steam-25247556/ground-marker-high-level-action-proof.v1.json`.

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
- A private Mech Facility M1 capture is now present for Global Steam build
  `24687926` with the exact reviewed protocol-pack digest. It spans 299.586
  seconds (3,852 complete Ethernet frames; no truncated, backward-timestamp,
  or missing-timestamp frames). The user's six placement acknowledgements fall
  at approximately +169, +195, +217, +236, +260, and +285 seconds, so all six
  actions are inside the retained PCAPNG. These are later chat acknowledgements,
  not instrumented game-click timestamps, and are correlation bounds only.
- The exact-build nested protocol journal proves that the captured World
  connection stopped producing routed client calls at approximately +79
  seconds, before the first placement acknowledgement. The other retained
  connection continues through the end of capture but carries only gateway/team
  notifications and periodic echo traffic during the placement interval. All
  30 decoded client frames after +150 seconds repeat on an approximately
  five-second cadence and are not placement-specific. Consequently,
  this solo-M1 capture contains no decoded outbound placement request to replay.
  It remains useful negative evidence and must not be described as an empty or
  missing capture.
- The marker journal auditor inspected 6 `SyncNearEntities`, 170
  `SyncNearDeltaInfo`, and 39 `SyncToMeDeltaInfo` packets and found zero typed
  `SeqPassiveSkillInfo` marker lifecycles. One pre-placement entity snapshot
  contains untyped integer collisions covering marker numbers 1..6 below an
  attribute field path; because they precede all six actions and do not occupy
  the reviewed passive-skill field, they are rejected as marker proof.
- The canonical `MapEvent` model has add/update/remove shapes, but the current
  BPSR decoder has no producer for it. It is not evidence that the game exposes
  party-visible actor markers.
- The reviewed `resonance-logs-cn` source supplied the original
  inbound-observation lead: it interprets `SeqPassiveSkillInfo` entries with
  skill IDs `1101..=1106` as numbered in-game markers, retains their passive
  instance IDs and target positions, removes them when the matching passive
  instance ends, and clears its projection on scene change. The focused Global
  remote live-service evidence now independently verifies the six add
  semantics and ground positions, while the local registry and digest bound the
  build-25247556 runtime gate. Update, end, reconnect, and scene-transition
  behavior remain carried decoder hypotheses until separately observed.
- Those passive containers are carried by the reviewed pack's existing
  inbound `WorldNtf` routes: `SyncNearEntities` (service `1664308034`, method
  `6`), `SyncNearDeltaInfo` (method `45`), and `SyncToMeDeltaInfo` (method
  `46`). Packet-only add detection is now live-observed without process-memory
  reading, and the triangulated evidence supports only the bounded
  build-25247556 observer identity. The focused evidence says nothing about the
  outbound placement request and does not grant another compatibility build
  observation authority.
- The requested RLogs feature is specifically a save/load system for numbered
  ground markers at exact map locations. It is not actor-targeted and is not a
  DPS, run-clock, or overlay-canvas feature. A controlled capture must resolve
  the native ground-marker placement, movement, and removal lifecycle before
  RLogs chooses a canonical event shape or enables native loading.
- Exact build `25247556` static game files expose `World.SetMapMark` and
  `World.RemoveMapMark` at service `103198054`, methods `65538` and `65539`.
  Their names occur in the installed `resources.assets` at file offsets
  `0xBBCA5` and `0xBBDCF`; the containing `world` service begins at `0xB5475`.
  `SetMapMark` accepts `sceneId:int32` plus `vMark:markinfo`, while
  `RemoveMapMark` accepts `sceneId:int32` plus `vMarkId:int64`; both return
  `EErrorCode`. The carried repository schema describes title/content/icon,
  map-layer, and two-dimensional map coordinates, and carries `MapData` inside
  character serialization. That combination is personal map-pin evidence, not
  actor targeting, party broadcast, or a numbered three-dimensional ground
  waymark. Fail closed: these methods must not be used or labeled as native
  automarkers without controlled cross-client lifecycle proof.
- The current installed `GameAssembly.dll` is
  `sha256:4a079aec0a3e51a8023aa86bbd152e12068907b65b9aafb355d20bb6d6c41fe3`,
  and its `global-metadata.dat` is an unusable zero-byte file. Therefore the
  `24687926` `MarkInfo` merge RVA `0x5A12C10` and its field offsets are useful
  prior-build worklist evidence only; they are not current-build native
  offsets. The build-scoped static audit records the complete distinction.
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

For a mirrored or placing-client capture, filter on the explicit client host,
not one previously observed remote server. This retains dungeon world-server
and transport migration. `tools/windows/capture-client-host.ps1` defaults to
the capture filter `host <client-ip>` (all IPv4 transports), creates the legacy
exact TCP-flow connection sidecar, and also writes a bounded metadata-only
transport inventory. Invoke marker captures with `-CapturePurpose marker-audit`;
that purpose fails closed if TCP-only mode is requested. Use the explicit
`-TransportMode tcp` fallback only for a non-marker, known TCP-only
investigation. The launcher prints the effective capture filter before and
after capture. The PCAPNG and sidecars remain private research and must not be
committed or uploaded.

The first capture should stay short and target the user's reported placement
window. Because the markers were reportedly placed once and then persisted
through later pulls, start recording **before the first placement**, not at
boss engagement.

For build `25247556`, the initial placing-leader experiment is complete: six
normal-UI placements isolate the nested `World.UseSlot` request and the
placing-client method-`46` add. The next experiments should stay capture-only
and separately isolate move/replace, individual-clear, clear-all,
observer-broadcast, failure/permission, wipe, reconnect, and scene-change
controls. They should also vary reconnect/scene conditions to bound the
`sessionSequence` lifecycle and vary conditions needed to resolve authenticated
attribute plaintext field `6`. Do not substitute
`SetMapMark`, inject a request, replay captured values, or treat the older pack
as current-build runtime authority.

Use `tools/windows/capture-marker-audit.ps1` for this controlled sequence.
It wraps the explicit-client capture launcher in `marker-audit`/`all-ip` mode,
starts capture before accepting any action, enforces the idle and spacing
windows, and interactively timestamps the visible result of one through six
sequential markers beginning with marker `1`.
It also fails if the capture ends before its post-engagement state window.
It requires a schema-1 action plan with the exact scene, initiating character,
expected action, marker/icon identity, and either a known ground coordinate or
an explicit placement description when the coordinate must be learned from the
packet. Character and instance entity identifiers may likewise be marked
unknown before capture with acquisition notes when they must be resolved from
the authenticated or dungeon-entry state retained by the capture.
It also requires the installed game executable and either (a) the
repository-reviewed complete installed-file manifest plus protocol pack for
that exact build, or (b) explicit
`-RawCaptureWithUnverifiedProtocolCarryForward` mode with the reviewed Steam
distribution snapshot for the captured build and
`-ProtocolPackSourceBuild` naming the older selected pack. The latter mode is
only a raw-capture/decoder hypothesis: its manifest records the actual captured
build separately and states that the older pack is neither exact for that build
nor runtime authority. It never silently relabels the older pack as current.
On completion it
writes a private action ledger and session manifest with
SHA-256 hashes, executable/build/pack identity, the all-IPv4 transport
inventory, and an outbound-during-placement followed by inbound-on-the-same-flow
preservation check for every action window. The plan, pack, executable, and build-manifest hashes are frozen
before capture and checked again before completion. Failed sessions keep
self-describing partial evidence and a failed manifest with the reason and all
available artifact hashes.

For the first one-marker protocol proof, an exact XYZ is not required before
capture. A ground target may instead set
`"coordinates_known_before_capture": false`, omit `coordinates`, and provide a
nonblank `placement_description` telling the operator where to click. This
keeps guessed coordinates out of the evidence and lets the packet observation
establish the encoded position. The original finite `x`/`y`/`z` form remains
valid when the coordinates are independently known.
The client-host filter is intentionally a superset of process-owned traffic so
transport and remote-flow changes cannot escape it. Review the dry run before
capturing; the harness never injects, replays, or sends game-protocol data.

Example action plan (keep real actor identifiers and coordinates private):

```json
{
  "schema_version": 1,
  "scene_id": 6525,
  "scene_name": "Mech Facility M1",
  "initiating_character": { "character_id": "character-id", "entity_uuid": "current-entity-uuid" },
  "actions": [
    { "marker_number": 1, "marker_identity": { "slot_number": 1, "icon_id": "marker-1" }, "expected_action": "place marker 1", "target": { "kind": "ground", "coordinates": { "x": 1.0, "y": 2.0, "z": 3.0 } } },
    { "marker_number": 2, "marker_identity": { "slot_number": 2, "icon_id": "marker-2" }, "expected_action": "place marker 2", "target": { "kind": "ground", "coordinates": { "x": 2.0, "y": 3.0, "z": 4.0 } } },
    { "marker_number": 3, "marker_identity": { "slot_number": 3, "icon_id": "marker-3" }, "expected_action": "place marker 3", "target": { "kind": "ground", "coordinates": { "x": 4.0, "y": 5.0, "z": 6.0 } } },
    { "marker_number": 4, "marker_identity": { "slot_number": 4, "icon_id": "marker-4" }, "expected_action": "place marker 4", "target": { "kind": "ground", "coordinates": { "x": 5.0, "y": 6.0, "z": 7.0 } } },
    { "marker_number": 5, "marker_identity": { "slot_number": 5, "icon_id": "marker-5" }, "expected_action": "place marker 5", "target": { "kind": "ground", "coordinates": { "x": 7.0, "y": 8.0, "z": 9.0 } } },
    { "marker_number": 6, "marker_identity": { "slot_number": 6, "icon_id": "marker-6" }, "expected_action": "place marker 6", "target": { "kind": "ground", "coordinates": { "x": 8.0, "y": 9.0, "z": 10.0 } } }
  ]
}
```

1. Capture the client that can successfully place markers through the normal
   game UI. No separate leader-identity capture is required. Start already
   authenticated but **before entering the chosen dungeon**, so the dungeon
   World connection and any transport migration begin inside the capture.
   Record the dungeon's exact name, difficulty, and scene ID after entry.
2. Record 10-15 seconds of idle traffic before the first placement. Note the
   exact installed build, scene ID, character and current actor/entity
   identities, and a synchronized wall-clock time.
3. Through the normal game UI, place marker 1. For a multi-marker capture, wait
   five seconds, place marker 2, and continue sequentially with the same
   spacing, up to marker 6. Record whether each action targets a map/ground
   coordinate or a party member and the local UI result.
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

Do not enable a native placement adapter until the remaining proof gates are
closed. The client-to-server route, request topology, placing-client add, and
success-path empty return are now captured; outstanding requirements include:

- exact remote-client build identity and a complete current-build request
  schema, including field `1.5` and authenticated plaintext field `6`;
- for any direct packet-injection experiment only, actor addressing (character
  ID, actor ID, entity UUID, or another lifetime identity) without guessing;
  the preferred high-level game action must resolve the current actor itself;
- correlated error/rejection returns and permission behavior;
- marker slot/icon values, uniqueness and collision behavior;
- replace, individual-clear, clear-all, despawn, scene-change, wipe, and
  reconnect lifecycles;
- an idempotency/rate-limit strategy that cannot spam or leave stale marks;
- an active fail-closed injection transport with exact dynamic frame, call,
  authentication-envelope, and acknowledgement generation;
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
