# Profile automation

RLogs builds character profiles from privacy-reviewed canonical events. It
does not upload a raw character container, packet journal, login message, or
platform account record.

The implementation is deliberately split:

- Core carries a `GameProfileEvent` with explicit game plug-in ID, payload
  schema ID/version, public character identity, and an opaque JSON body.
- The BPSR plug-in owns `CharacterProfilePatch`, validates that typed body, and
  projects it into a game-neutral `WebsitePayloadRequest`.
- The request contains only a safe relative route. The host owns the website
  base URL, authentication, retry policy, and final transport.
- Core recursively rejects credential and account field names before a
  website payload can leave the game plug-in.

The stable profile key is:

```text
deployment + region + world/server + character UID
```

Region and world evidence are attached before a profile update can leave the
client. A character UID is never replaced with an account ID, open ID,
publisher ID, Discord ID, or session identifier.

## What is proven on the current Global build

Global Steam build `24252055` sent one complete
`WorldNtf/SyncContainerData` snapshot during the process-aware world load. The
selective decoder verified character UID, display UID, name, class, level, and
combat power.

A structural-only audit also verified that character-base tag `25` still has
the historical five-part `AvatarInfo` outer shape:

| Avatar surface | Current structural evidence |
| --- | --- |
| avatar ID | present |
| profile picture record | present, including a URL tag |
| half-body picture record | present, including a URL tag |
| business-card style ID | present |
| avatar-frame ID | present |

No URL, character value, account identifier, or endpoint was printed or added
to the sanitized observation. The current picture-verification records use
tags `3` and `4`; the historical parser described tags `1`, `2`, and `3`.
RLogs therefore treats the outer image shape as verified and the inner
verification metadata as changed and unmapped.

The build-scoped game-file inventory now evaluates all 565 named CTBs and
retains 864 exact-build semantic fields. The first profile/equipment reviews
confirm IDs and display relations for profile images and objectives, profile
attributes, medals, name cards, professions, item types, equipment parts and
models, fashion collection milestones, weapon skins, vehicles, and guild
icons. They also confirm the primary level keys in `PlayerLevelTable` and
`SeasonLevelTable`.

These are static definitions. They can describe an ID after a reviewed runtime
route observes it, but they cannot establish that a character owns, equipped,
or unlocked that ID. Ambiguous nested equipment arrays remain candidates. The
complete build-scoped results and ordered backlog live under
`plugins/games/blue-protocol-star-resonance/research/game-file-inventory/global/steam-24252055/schemas/`.

The machine-readable bridge at
`plugins/games/blue-protocol-star-resonance/protocol-references/profile-bridges/global-steam-24252055.json`
records this
boundary for each canonical profile surface.

## What the original projects tell us

The historical native parser is useful as a schema inventory:

- `CharBaseInfo` carried public character identity, appearance, `UserUnion`,
  and `AvatarInfo` alongside prohibited account/open identifiers and private
  timestamps.
- `AvatarInfo` carried avatar ID, profile and half-body pictures,
  business-card style ID, and avatar-frame ID.
- `SocialData` carried character basics, avatar, profession, equipment,
  fashion, attributes, union data, personal-zone display, ranks, and other
  character-facing surfaces.
- `UnionData` carried union ID, name, and union-hunt rank.
- guild-space data included electronic-screen photo records, photo
  descriptions, owner information, and image URLs.

The historical website confirms which fields became character UI. It derives
the character portrait from `AvatarInfo.Profile.Url`, also renders
`AvatarInfo.HalfBody.Url`, and projects equipment, profession, fight-point,
statistics, dungeon, currency, and progression-shaped data. Discord avatar
URLs are a separate login/account-linking surface and are not game character
pictures.

The historical website tree does not expose union/guild profile fields, but
RLogs explicitly allows public guild identity and display data: guild ID/name,
the displayed badge, and a character's public guild role/rank. Exact current
routes still have to prove each field before publication. Guild chat,
applications, invitations, member-management state, and permissions remain
out of scope. Guild photo-screen structures are a separate
user-generated-content surface and are never promoted automatically.

## Source priority

Profile automation is built in this order:

1. `WorldNtf/SyncContainerData` supplies the owner-character snapshot.
2. `WorldNtf/SyncContainerDirtyData` supplies reviewed incremental updates.
3. `SocialNtf/NotifySocialData` supplies public display data for other
   characters after its current shape is verified.
4. `UnionNtf` supplies guild identity and display changes after exact methods
   and current schemas are proven.
5. Current game descriptors and data tables provide IDs, schemas,
   localization, icon relations, and exact-build provenance.

For every field, runtime evidence establishes character state and static game
data supplies its reviewed definition. Neither source substitutes for the
other.

World-load login, anti-cheat, account-control, payment, private-chat, and mail
content routes are never alternate sources for profiles.

## Image publication policy

IDs such as avatar, card style, and frame are normal opt-in profile fields.
Image URLs require a separate gate:

- the character owner explicitly enables image publication;
- only HTTPS URLs from reviewed game/CDN hosts are accepted;
- the website fetches through a bounded image proxy, validates size and
  content type, and caches or rehosts the result;
- query credentials, unexpected hosts, redirects, and non-image responses are
  rejected;
- ordinary plugins receive image references only with the character-profile
  capability;
- public guild identity/badge data may appear on profiles; guild photos and
  descriptions remain a separately reviewed user-generated-content domain.

This preserves game profile pictures without turning a packet-supplied string
into an arbitrary remote URL in a public page.

## Explicitly excluded

- account ID, open ID, publisher/platform ID, and Discord ID from packets;
- passwords, tokens, login/session payloads, and anti-cheat login data;
- online/offline timestamps, total playtime, moderation state, and payment;
- direct/private messages and mail contents;
- exact position history in a public profile;
- raw protobuf containers or packet bytes in a website submission.
