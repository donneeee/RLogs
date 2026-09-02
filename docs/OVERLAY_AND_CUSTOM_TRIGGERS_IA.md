# Overlay and Custom Triggers information architecture

Status: the workspace menus and cross-workspace navigation are implemented.
The bounded Event Inspector is the first working Custom Triggers component.
The Overlay Trackers page now has a working local Combat Stats component backed
by the exact build-stamped Fight Attribute catalog. Rule execution and actual
game-overlay rendering remain disabled while their contracts are reviewed.

## Product boundary

The **Overlay** workspace owns presentation. It decides what is visible, where
it is placed, how it looks, and which Setup Profile it belongs to.

The **Custom Triggers** workspace owns automation. It decides which verified
event begins a rule, which optional conditions must be true, and which ordered
actions are requested.

The destination owns presentation. A trigger may request that an overlay
widget be shown, a sound be played, or a map marker be updated, but it does not
embed or silently replace the destination's visual configuration.

## Overlay menu

1. **Overview** — entry point and short explanation of the workspace.
2. **Setups** — My Setups, browsing, import, and sharing of Setup Profiles.
3. **Editor** — visual canvas, widget library, display groups, and visibility.
4. **Trackers** — combat stats, skills and cooldowns, effects and auras, energy
   and gauges, party and support, encounter timeline, and tracker groups.
5. **Mechanics Map** — live map, encounter guides, markers, role-filtered
   mechanic layers, and map appearance.
6. **Settings** — general behavior, performance, hotkeys, accessibility, and
   localization.

Combat Stats is the first working tracker. It long-polls a bounded native feed,
shows the latest complete snapshot separately from temporary packet-observed
changes, and exposes only localized members of the exact 906-member catalog for
the active BPSR build. Internal `AttrLevel` members and raw numeric identifiers
are not part of the normal presentation. The website profile generator consumes
that same catalog, while profiles persist only complete snapshots.

## Custom Triggers menu

1. **Overview** — plain-language automation summary.
2. **Rules** — guided `When -> If (optional) -> Then` builder and rule groups.
3. **Event Inspector** — bounded live event discovery, decoded field details,
   pin/compare, and Create Trigger from the selected event or field.
4. **Library** — encounter packs, timelines, class packs, imports, and utility
   patterns.
5. **Settings** — editor mode, execution behavior, testing, sharing, and
   localization.

Variables, regex, raw IDs, counters, timers, delays, looping, and ordered
multi-action controls remain inside a rule's **Advanced** section. They are not
top-level tabs. The complete rule must always remain readable as a sentence.

The Event Inspector is not the developer-only Session Tools page nested under
Settings. It is a focused, local trigger-authoring tool with a strict memory budget. Its default
view follows privacy-reviewed canonical events; selected protocol details are
decoded lazily and never grant shared trigger packs access to raw packets.

Triggernometry is the primary workflow reference for Custom Triggers. The
source-level preserve/adapt/reject decisions are recorded in
[`TRIGGERNOMETRY_REFERENCE_AUDIT.md`](TRIGGERNOMETRY_REFERENCE_AUDIT.md).

## Setup Profile sharing

“Setup Profile” is distinct from a public character profile. A Setup Profile
may include selected:

- overlay layouts and display groups;
- tracker and aura configurations;
- mechanics-map appearance and layers;
- localized alert text and referenced sound assets;
- trigger packs or explicit rule dependencies.

Import must present a plain-language manifest before enabling anything. The
primary beginner action is **Use this setup**. Imported triggers remain visible
and reviewable; no import may silently enable arbitrary code or undeclared
network access.

## Reference patterns

- [Triggernometry](https://github.com/paissaheavyindustries/Triggernometry):
  nested organization, stateful variables, conditions, and diverse actions.
- [cactbot](https://github.com/OverlayPlugin/cactbot): encounter-scoped trigger
  sets, timelines, localized alerts, and independently placed overlay modules.
- [Deadly Boss Mods](https://github.com/DeadlyBossMods/DeadlyBossMods):
  encounter modules, concise action-oriented warnings, role filtering,
  per-mechanic options, timers, sounds, and at-a-glance information.
- [BigWigs](https://github.com/BigWigsMods/BigWigs): modular encounter scripts
  and distinct messages, bars, sounds, markers, proximity, and info outputs.
- [JobBars](https://github.com/0ceal0t/JobBars) and
  [ReBuff](https://github.com/WesBosch/ReBuff): small trackable units, icon/bar
  representations, conditions, grouping, and import/export.

These projects are design references. rLogs keeps its own build-versioned BPSR
event model, localization evidence, permissions, and verification boundaries.

## Beginner and advanced modes

The default experience must not require knowledge of regex, packet fields,
numeric UIDs, or programming concepts. A beginner can browse a Setup Profile,
preview it, select **Use this setup**, and make visual adjustments.

Advanced controls use progressive disclosure inside the relevant editor. They
do not replace the guided path or create additional top-level workspaces.

## Localization boundary

All workspace labels, generated rule summaries, alert text, descriptions, and
shared-profile metadata use localization keys. The target locale set is:

`de-DE`, `en-US`, `es-ES`, `fr-FR`, `id-ID`, `ja-JP`, `ko-KR`, `pt-BR`,
`th-TH`, `zh-CN`, and `zh-TW`.
