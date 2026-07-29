# World-load route research

This is the human-readable backlog for the 38 routed messages in sanitized
observation `world-load-process-001`.

The checked-in reconciler compares exact CN `0.2.0`, Global, and ZDPS
revisions. CN and Global share one lineage and therefore contribute one vote
together. The current result is:

- 15 routes corroborated by two independent lineages;
- 10 routes named by one lineage;
- 1 preserved naming conflict;
- 12 routes whose service is known but method remains unnormalized.

Service names are independently checked with the game's BKDR-131 service hash.
No route in this document is permission to decode a payload.

The exact-build research pack now has entries for all 38 observed routes. A
replay classifies all 1,944 routed packets: 1 packet reaches the reviewed
character-profile decoder, 1,925 remain opaque, and 18 are blocked as
prohibited. The other 4,518 protocol packets have no route header and remain
separately visible rather than being guessed.

## Corroborated routes

| Direction | Service/method | Packets | Candidate | Profile value |
| --- | --- | ---: | --- | --- |
| server to client | `ChitChatNtf/1` | 24 | `NotifyNewestChitChatMsgs` | none; chat stays local and separately permissioned |
| server to client | `SocialNtf/1` | 7 | `NotifySocialData` | high: public character display, avatar, profession, gear, and guild candidates |
| server to client | `WorldNtf/3` | 1 | `EnterScene` | region/world/map context |
| server to client | `WorldNtf/4` | 1 | `NotifyLoadSceneEnd` | world-load boundary |
| server to client | `WorldNtf/6` | 5 | `SyncNearEntities` | public nearby identity and actor context |
| server to client | `WorldNtf/20` | 1 | `EnterGame` | world-load boundary |
| server to client | `WorldNtf/21` | 1 | `SyncContainerData` | primary owner-character snapshot; selective decoder verified |
| server to client | `WorldNtf/22` | 5 | `SyncContainerDirtyData` | high: incremental character/profile updates |
| server to client | `WorldNtf/23` | 1 | `SyncDungeonData` | dungeon profile/history context |
| server to client | `WorldNtf/43` | 31 | `SyncServerTime` | ordering and clock evidence |
| server to client | `WorldNtf/45` | 1,826 | `SyncNearDeltaInfo` | combat, position, identity, and state timeline |
| server to client | `WorldNtf/46` | 6 | `SyncToMeDeltaInfo` | local character combat/stat changes |
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
| server to client | `WorldLoginNtf/3` | 1 | `NotifyEnterWorld` | prohibited payload; timing may mark a boundary |
| server to client | `WorldActNtf/450561` | 1 | `SyncWorldActData` | world-event context, not a core profile source |
| server to client | `WorldNtf/27` | 1 | `SyncSeason` | high profile priority after current schema proof |
| server to client | `WorldNtf/74` | 1 | `NotifyUserAllSourcePriviledgeEffectData` | entitlement-like data; keep local/opaque |
| server to client | `WorldNtf/385025` | 2 | `SignRewardNotify` | reward state, not profile identity |
| server to client | `MailNtf/2` | 1 | `SyncMailListNum` | no profile value; never expand into mail content |

## Preserved conflict

| Direction | Service/method | Packets | Claims | Decision |
| --- | --- | ---: | --- | --- |
| server to client | `WorldNtf/79` | 1 | Global: `NotifySceneLineInfo`; ZDPS: `NotifyUserAllValidBattlePassData` | neutral name, opaque payload, current descriptor/shape required |

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
| server to client | `UnionNtf/6` | 1 | historical descriptor order points to `NotifyUnionActivity` | high: resolve from current service registry, then inspect only guild-facing fields |
| server to client | `UnionNtf/15` | 3 | beyond the historical 13-method union-notify catalog | highest unresolved guild priority; current descriptor required |
| server to client | `GrpcCommunityNtf/17` | 3 | historical descriptor order points to a homestead sell-shop update | keep separate from guild/union; low profile priority |

## Offline resolution order

1. Extract current service and method registries from the installed build.
2. Diff them against the immutable historical descriptor catalog.
3. Resolve `UnionNtf/6`, `UnionNtf/15`, and the method-79 conflict first.
4. Use the existing complete snapshot to map avatar, equipment, profession,
   Imagine, cosmetic, season, and progression subtrees structurally.
5. Promote only selective fields with synthetic secret-exclusion tests.

No additional user capture is required for this route-normalization pass.
