# RLogs shared catalog

This canonical, human-readable catalog is shared across player regions. Exact
deployment/channel/client-build availability is recorded on each definition.

It contains reviewed combat, talent, Battle Imagine, profile-display, public
guild-badge, equipment, entity, and world definitions. Static records do not
prove character ownership, equipment, guild membership, or live entity UUIDs.
Ambiguous relations remain `shared`, `unclassified`, or explicitly unresolved.

The runtime profile contract now exposes typed module instances and selected
talent-node state. Website rendering must join those IDs to this exact-build
catalog. Module item definitions live under `modules/<type>/`, with separate
human-readable `module-effects/`, `module-types/`, `module-slots/`, and
`module-link-effects/` domains. Talent records remain under
`talents/<class>/<spec>/`. Their shared icon paths mirror those domains under
`assets/blue-protocol-star-resonance/shared/icons/`.

The current profile-display slice contains all 12 module configurations, 21
effects with seven exact link thresholds each, 121 total-link score rows, five
slots, six types, 1,350 talent nodes with exact coordinates and both graph
directions, and 640 unique exported module/talent PNGs. Only the 990 nodes
referenced by a current `TalentStageTable` row are marked active for normal
board rendering; 360 unassigned rows remain explicit rather than being
silently discarded. Unnamed module-initialization roll dimensions and the
generic assessment screen remain evidence-only outside the parser catalog.

The current Battle Imagine slice is organized as one human-readable JSON file
per item under `imagines/battle/`, with one matching human-readable icon under
`assets/blue-protocol-star-resonance/shared/icons/imagines/battle/`. It contains
86 current-build records, 73 exact item-to-skill relations, all five
enhancement rows for those skills, and official text from all 11 shipped
locales. Thirteen items remain unresolved rather than inheriting older
page/icon guesses. Item descriptions remain pending until their exact current
`ItemTable` field is proven.

Official text is colocated only during mapping for exhaustive validation. Once
stable, it moves unchanged into data-only add-ons under
`plugins/builtin/localization/<locale>/games/app.rlogs.game.blue-protocol-star-resonance/game/`;
`en-US/ui/` will contain RLogs
interface text. Asset acquisition and extraction remain outside RLogs.
