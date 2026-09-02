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
selective decoder verifies character UID, display UID, name, current
profession, level/experience, prior-season maximum level, combat power, full
face/color/avatar appearance, equipped gear identity/quality/refinement/
attributes/enchantments, equipped modules and module inventory, combat-
profession skill and talent loadouts, life professions, and collection
ownership.

In the saved snapshot this comprises 11 equipped items, 5 equipped module
slots, 649 module instances, 1,937 module parts, 9 combat professions, 11
active skills, 70 current-profession talent nodes, 4 profession talent
loadouts containing 200 nodes, 9 life professions, 29 Battle Imagine skills
with 2 equipped slots, 193 fashion IDs with 10 equipped slots, 11 mount IDs, 4
weapon-skin IDs, 1 ride and ride skin, 53 emojis, 2 vanity pets, 2 Fantasy
Atlas stages, 440 handbook entries, 133 master-mode dungeon rows, weekly-tower
progress, 7 seasonal medal holes, 8 medal nodes, 2 cultivation seasons with 4
lines and 16 areas, one reputation row, and a combat-power breakdown with 6
top-level and 4 nested components. Current-season experience, total talent
points, and the current profession project are also present. The audit reports
only field presence and counts; it does not print captured character values.

The module projection reads only package 5 and the character's module state.
It emits config IDs, browser-safe string instance IDs, equipped-slot joins,
quality/type/level, part IDs, initial link values, upgrade outcomes, and the
reported success rate. It deliberately omits acquisition/expiration times,
binding/source state, currencies, and arbitrary effect-parameter strings.
Static names, icons, effect definitions, and scoring rules are joined by the
website from the exact-build game-data catalog rather than duplicated in every
profile.

Seven `SocialNtf/NotifySocialData` packets in the same capture now pass a
second selective decoder. The privacy-reviewed projection verifies public
character/display IDs, name and level, gender/body/avatar/frame/card IDs,
current profession and weapon skin, equipped item IDs, combat power, season
level/strength, guild ID/name, titles/medals, and world/line context. Equipment
is sorted before comparison so protobuf collection order cannot invent profile
changes; profile and world-context updates are deduplicated independently.

The structural audit also verifies that character-base tag `25` still has
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

## Opt-in profile-sync plug-in boundary

The direct implementation path is now proven well enough to separate profile
publication from parsing:

- the BPSR game plug-in captures and decodes approved character state into
  local `CharacterProfile` events;
- a separate `bpsr-profile-sync` plug-in subscribes only to those canonical
  events, assembles the website profile, and owns the explicit network
  capability;
- disabling or removing `bpsr-profile-sync` stops profile assembly,
  persistence, and submission without disabling packet parsing, combat logs,
  or other ACT-style plug-ins;
- the profile-sync plug-in never receives raw packets, journals, login
  messages, account identifiers, or authentication material;
- localization and ID/icon resolution remain shared BPSR assets instead of
  being copied into the profile-sync plug-in.

The existing website-payload projection will move behind this plug-in boundary
before profile submission is enabled. Its first baseline will use the already
reviewed `SyncContainerData`, `SyncContainerDirtyData`, `NotifySocialData`, and
`SyncSeason` events. Additional surfaces are added only after their individual
field policies pass privacy tests.

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

Its module page also demonstrates the useful behavioral split RLogs retains:
character module state comes from the parser, while module names, icons,
attribute definitions, scoring presets, and optimization logic live in the
website. RLogs does not retain the historical implementation's whole-container
storage/upload path. The website receives only the typed allowlist described
above.

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
2. `WorldNtf/SyncContainerDirtyData` supplies reviewed incremental updates
   through a bounded proprietary-stream decoder. It consumes known private
   fields without retaining them and abandons a bounded object when an unknown
   field type cannot be skipped safely. In the current five-packet world-load
   sample, all updates were empty, quest/story-only, or private time/internal
   serial changes, so they intentionally produced no profile events.
3. `SocialNtf/NotifySocialData` supplies its now-verified public character,
   appearance, profession, equipment, power, season, guild, and display data.
4. `UnionNtf` supplies guild identity and display changes after exact methods
   and current schemas are proven. The resolved `NotifyUnionActivity` and
   `NotifyMemberOnline` routes do not supply approved guild identity/display
   fields and remain opaque; member presence is not website-bound profile data.
5. `WorldNtf/NotifyUserAllValidBattlePassData` is now structurally verified,
   but remains opaque because progression is mixed with unlock, purchase, and
   reward-claim state. Any future projection belongs to the explicit opt-in
   profile-sync field policy.
6. Current game descriptors and data tables provide IDs, schemas,
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
- URL query strings and fragments are rejected before a reference can enter
  the publication pipeline;
- the website fetches through a bounded image proxy, validates size and
  content type, and caches or rehosts the result;
- query credentials, unexpected hosts, redirects, and non-image responses are
  rejected;
- ordinary plugins receive image references only with the character-profile
  capability;
- public guild identity/badge data may appear on profiles; guild photos and
  descriptions remain a separately reviewed user-generated-content domain.

An exact live Photo Wall reference that passes those gates is retained in a
bounded, local-only retry ledger under `runtime-data/profile-sync`. The ledger
is not a canonical event and is never written to an `.rlog` or website profile
payload. It exists so an account/network outage or app restart does not require
the owner to reopen the same Photo Wall image. A successful server publication
atomically removes only the matching character/photo/version reference; a
newer observation cannot be cleared by an older receipt.

This preserves game profile pictures without turning a packet-supplied string
into an arbitrary remote URL in a public page.

## Explicitly excluded

- account ID, open ID, publisher/platform ID, and Discord ID from packets;
- passwords, tokens, login/session payloads, and anti-cheat login data;
- online/offline timestamps, total playtime, moderation state, and payment;
- direct/private messages and mail contents;
- exact position history in a public profile;
- raw protobuf containers or packet bytes in a website submission.
