# Global Steam build 24252055

This is the first exact-build Global research pack. Its build evidence contains
only distribution and executable metadata; no installation path, platform
owner identifier, login data, or account data is retained.

Inherited route names and feature labels begin as candidates. A route mapping
can become verified when an exact-build capture and payload-shape check agree,
but the route still remains opaque until a sanitized fixture proves a selective
decoder. Copying a decoder from a reference parser is not verification.

Sanitized observations are stored under `observations/`. They contain route
tuples, counts, structural lengths, and privacy decisions only. Raw packet
bytes, endpoints, and character or account values remain in the ignored local
research workspace.

The first profile-screen observation verified the
`WorldNtf/SyncContainerDirtyData` route, but it did not produce the complete
`SyncContainerData` character snapshot. Its dirty `char_base` tree remains
opaque because historical layouts co-locate useful character data with an
account identifier.

The process-aware world-load observation followed three process-owned
connections across the character-to-world transition without broadening the
persistent capture. It observed and selectively decoded one complete
`WorldNtf/SyncContainerData` snapshot. Character UID, display identity, class,
level progression, combat power, appearance, detailed equipment, combat and
life professions, skill/talent loadouts, and cosmetic collections were
present. Server ID and scene data were absent from this particular snapshot
and must come from other reviewed evidence.

The same structural-only audit confirmed the current outer `AvatarInfo` shape:
avatar ID, profile picture, half-body picture, business-card style, and avatar
frame are present. Picture URLs were not rendered. The nested verification
tags differ from the historical schema and remain unmapped.

The current snapshot still contains protobuf tags historically associated with
account ID and open ID. The RLogs schema deliberately does not declare those
tags, so Prost skips them without materializing their values. A structural-only
audit and a synthetic-secret regression test enforce this boundary. The
observed `WorldLoginNtf` method remains explicitly prohibited from schema
decoding.

Method 79 conflicted between the historical Global reference and ZDPS
0.1.7.3. Exact payload-shape analysis resolves it as
`NotifyUserAllValidBattlePassData`. It remains opaque because pass progression
is co-located with purchase and reward-claim state.

The pack is deployment-wide because no evidence currently proves that Global
regions use different wire schemas. Region is still resolved and recorded per
capture. A region-specific pack will take precedence if a real schema
difference is found.
