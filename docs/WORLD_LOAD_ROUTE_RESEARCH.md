# World-load route research

This is the human-readable backlog for the 38 routed messages in sanitized
observation `world-load-process-001`.

The checked-in reconciler compares exact CN `0.2.0`, Global, and ZDPS
revisions. CN and Global share one lineage and therefore contribute one vote
together. The current result is:

- 15 routes corroborated by two independent lineages;
- 11 routes named by one lineage;
- 1 preserved reference conflict;
- 11 routes whose service is known but method remains unnormalized.

The reconciler intentionally preserves historical claims rather than rewriting
them. Current-build structural evidence resolves both that reference conflict
and one manifest-level unknown below.

Service names are independently checked with the game's BKDR-131 service hash.
No route in this document is permission to decode a payload.

The exact-build research pack now has entries for all 38 observed routes. A
replay classifies all 1,944 routed packets: 1,886 packets reach reviewed
decoders, 41 remain opaque, and 17 are blocked as prohibited. The other 4,518
protocol packets have no route header and remain separately visible rather
than being guessed. The reviewed set currently covers world entry and scene
state, nearby actors, public social-profile snapshots, the owner-character
snapshot and its bounded incremental dirty stream, the complete dungeon
snapshot route's public timeline subset, authoritative server time, nearby
deltas, and local-player deltas.
The same replay produces zero decoder failures.

## Corroborated routes

| Direction | Service/method | Packets | Candidate | Profile value |
| --- | --- | ---: | --- | --- |
| server to client | `ChitChatNtf/1` | 24 | `NotifyNewestChitChatMsgs` | none; chat stays local and separately permissioned |
| server to client | `SocialNtf/1` | 7 | `NotifySocialData` | selective decoder verified: public display, avatar IDs, profession, equipped item IDs, power, season, guild, medals, and world context |
| server to client | `WorldNtf/3` | 1 | `EnterScene` | region/world/map context |
| server to client | `WorldNtf/4` | 1 | `NotifyLoadSceneEnd` | selective decoder verified; exact scene ID and instance UUID |
| server to client | `WorldNtf/6` | 5 | `SyncNearEntities` | public nearby identity and actor context |
| server to client | `WorldNtf/20` | 1 | `EnterGame` | world-load boundary |
| server to client | `WorldNtf/21` | 1 | `SyncContainerData` | primary owner-character snapshot; verified for appearance, progression, equipment, combat/life professions, skill/talent loadouts, and cosmetic collections |
| server to client | `WorldNtf/22` | 5 | `SyncContainerDirtyData` | selective bounded dirty-stream decoder verified; the five observed updates were empty, quest/story-only, or private time/internal-serial changes and correctly emitted no website-bound events |
| server to client | `WorldNtf/23` | 1 | `SyncDungeonData` | selective decoder verified; idle snapshot has no flow transition and correctly emits no false boundary |
| server to client | `WorldNtf/43` | 31 | `SyncServerTime` | selective decoder verified; authoritative game-time anchor |
| server to client | `WorldNtf/45` | 1,826 | `SyncNearDeltaInfo` | selective decoder verified for combat, position, identity, and state timeline |
| server to client | `WorldNtf/46` | 6 | `SyncToMeDeltaInfo` | selective decoder verified for local character deltas and cooldowns |
| server to client | `WorldNtf/62` | 1 | `NotifyUserCloseFunction` | capability state; not a profile field |
| server to client | `WorldNtf/63` | 1 | `NotifyServerCloseFunction` | capability state; not a profile field |
| server to client | `WorldNtf/72` | 1 | `NotifyTimerList` | timer/world state |

## Single-lineage route names

| Direction | Service/method | Packets | Candidate | Decision |
| --- | --- | ---: | --- | --- |
| client to server | `World/4098` | 1 | `ConnectWorld` | context only; never a profile decoder |
| client to server | `World/24579` | 1 | `LoadMapSuccess` | useful world-load boundary |
| client to server | `GrpcCharactor/1` | 1 | `Login` | prohibited authentication/account-control route |
| client to server | `GrpcCharactor/3` | 1 | `SelectChar` | prohibited account-control route |
| server to client | `WorldLoginNtf/3` | 1 | `NotifyEnterWorld` | narrow endpoint-only decoder verified; adjacent account/token tags remain undeclared |
| server to client | `WorldActNtf/450561` | 1 | `SyncWorldActData` | world-event context, not a core profile source |
| server to client | `WorldNtf/27` | 1 | `SyncSeason` | selective decoder verified; current season ID attaches to the owner-character profile |
| server to client | `WorldNtf/74` | 1 | `NotifyUserAllSourcePriviledgeEffectData` | entitlement-like data; keep local/opaque |
| server to client | `WorldNtf/385025` | 2 | `SignRewardNotify` | reward state, not profile identity |
| server to client | `MailNtf/2` | 1 | `SyncMailListNum` | no profile value; never expand into mail content |
| server to client | `UnionNtf/6` | 1 | `NotifyUnionActivity` | historical method number retained as a candidate; the current request was empty and exposed no approved profile field |

## Reference conflict resolved for the current build

| Direction | Service/method | Packets | Historical claims | Current-build result |
| --- | --- | ---: | --- | --- |
| server to client | `WorldNtf/79` | 1 | Global: `NotifySceneLineInfo`; ZDPS: `NotifyUserAllValidBattlePassData` | verified as `NotifyUserAllValidBattlePassData`: the packet exactly matches the valid-pass map, BattlePass scalar tags, and 17 award-map entries; the scene-line candidate is rejected because its required line/GUID structure is absent |

The exact-build route remains opaque. It includes progression mixed with
unlock, pass-purchase, validity, and reward-claim state, so no field becomes
website-bound until the opt-in profile plug-in has an explicit field policy.

## Current-build structural resolution

| Direction | Service/method | Packets | Result | Decision |
| --- | --- | ---: | --- | --- |
| server to client | `UnionNtf/15` | 4 across two observations | `NotifyMemberOnline`; every request exactly matches packed repeated `int64` member and offline-timer fields, including zero-online and Unix-millisecond offline behavior | verified name, opaque payload; member identifiers and presence are neither profile fields nor website upload data |

`UnionNtf/6` remains candidate-confidence because its sole current request was
empty. That empty request is compatible with `NotifyUnionActivity`, but cannot
independently distinguish the schema. Neither union route revealed approved
public guild identity/display fields.

## Method-level backlog

The service name is proven by its numeric service hash. Method names below are
either absent from the exact manifests or only historical descriptor clues.

| Direction | Service/method | Packets | Historical clue | Research action |
| --- | --- | ---: | --- | --- |
| client to server | `Ace/2` | 1 | old schema contains `ReqLoginAntiData` | prohibited anti-cheat/login domain; name only, no payload research |
| client to server | `GrpcCharactor/21` | 1 | old method catalog leaves `21` unassigned between face-data and account-control calls | prohibited service area; do not inspect payload |
| client to server | `GrpcCharactor/30` | 1 | generated descriptor contains `GetFacePushList` after method `29` | verify current method registry; this is face-configuration distribution, not the character portrait |
| client to server | `ChitChat/2` | 5 | `GetChipChatRecords` candidate | private-chat domain; name-only catalog work |
| client to server | `ChitChat/3` | 1 | `GetPrivateChatTargets` candidate | private-chat domain; name-only catalog work |
| client to server | `ChitChat/9` | 1 | `PrivateChatBlockList` candidate | private-chat domain; name-only catalog work |
| client to server | `ChitChat/11` | 1 | `GetWorldChatChannelId` candidate | channel metadata only; no message decode |
| client to server | `ChitChat/12` | 1 | `QueryChatMute` candidate | moderation/account state; no profile use |
| client to server | `ChitChat/19` | 1 | `GetNewbieChatChannelId` candidate | channel metadata only |
| server to client | `GrpcCommunityNtf/17` | 3 | historical descriptor order points to a homestead sell-shop update | keep separate from guild/union; low profile priority |

## Offline resolution order

1. Recover a nonempty `UnionNtf/6` shape or current method registry when either
   becomes available; do not force a new capture for it.
2. Use the existing complete snapshot to map Imagine ownership/equipped state,
   panel-stat breakdowns, season cultivation, and remaining profile records.
3. Promote only selective fields with synthetic secret-exclusion tests.

No additional user capture is required for this route-normalization pass.
