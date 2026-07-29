# Game-file research inventory

The first RLogs read-only client scan targets Global Steam build `24252055`
(app `3681810`). Its sanitized review inventory is
`plugins/games/blue-protocol-star-resonance/research/game-file-inventory/global/steam-24252055/`.

## Coverage

| Layer | Result |
| --- | ---: |
| Physical client files SHA-256 hashed | 741 / 741 |
| Physical bytes | 41,786,827,248 (38.917 GiB) |
| Files that changed during the scan | 0 |
| `meta.pkg` internal entries hashed | 205,400 |
| Internal payload bytes | 37,407,815,288 |
| UnityFS entries | 186,273 |
| Non-bundle data entries | 19,127 |
| Asset addresses | 215,756 |
| Distinct addressed bundle hashes | 154,625 |
| Sprite-to-atlas records | 9,019 |
| Confirmed CTB tables | 4,239 |
| Confirmed CTB rows | 329,764 |
| Distinct CTB row IDs | 153,905 |
| CTBs with a current human name | 565 |
| CTBs retained by unresolved hash | 3,674 |
| Game locales present | 11 / 11 |
| Union localization IDs | 171,474 |
| Top-level Unity objects inventoried | 18,766 |
| Internal UnityFS bundles loaded | 186,273 / 186,273 |
| Internal UnityFS load failures | 0 |
| Internal UnityFS objects inventoried | 10,206,944 |
| Searchable named Unity objects inventoried | 130,700 |
| Deterministic inventory edges | 12,961,290 |
| Named CTB schemas evaluated | 565 / 565 |
| Named CTB rows evaluated for packed fields | 260,189 |
| Corroborated semantic fields | 864 |
| Exact local-pool field candidates | 1,448 |

One 242-byte payload aligned with the loose CTB header pattern but did not have
the client’s observed pool sequence `1..10`. It is preserved in quarantine and
excluded from table/recount claims. Every row from every confirmed CTB is
accounted for.

Unity `MonoScript` identities supplied 565 exact `.ctb` hash matches with no
hash collisions. Seventeen of those independently agree with the pre-existing
reviewed extractor vocabulary.

## Evidence classes

Deterministic edges are structural: physical file to digest, `meta.pkg` entry
to package extent, address to bundle hash, sprite to atlas address, CTB to row,
localization index to locale entry, validated string-pool offset to text, and
top-level serialized file to Unity object.

Membership matches such as “this 32-bit value is also a localization ID” are
only corroborating candidates until a table schema or runtime observation
confirms the field. Cross-table row-ID matches and pool-offset alignments are
weak candidates. A shared integer is never promoted to a foreign key by
itself.

## What “full scan” currently means

Every physical file and every `meta.pkg` extent has a stable location, size,
classification, and digest. Every catalog address, sprite relation, confirmed
CTB shape/row ID, locale index, top-level Unity object identity, and internal
UnityFS bundle/object-type set is inventoried. The internal bundle pass loaded
all 186,273 bundles in 279.2 seconds and counted 10,206,944 objects without
retaining their payloads.

A second targeted identity pass covered all 58,727 bundles containing
`MonoScript`, `TextAsset`, `Texture2D`, `Sprite`, or `SpriteAtlas` objects. It
recorded names for all 130,700 selected objects with zero bundle failures and
zero sensitive-name redactions: 85,982 TextAssets, 34,718 textures, 9,130
sprites, 211 atlases, and 659 bundle-local scripts. It found no additional CTB
hash matches beyond the 565 names already recovered from top-level script
metadata.

This does not claim that Unity object payload values/field semantics, all 3,674
unknown CTB schemas, protected IL2CPP registrations, or network message
meanings are decoded. Those are follow-on mapping layers. The inventory makes
them enumerable and diffable without guessing.

The first follow-on layer is now active. Every named CTB is evaluated at every
byte offset, including packed unaligned fields. The sanitized per-domain
results and review worklist are under `schemas/`; the evidence rules and first
reviewed profile/equipment/world/entity fields are documented in
`docs/CTB_SCHEMA_RESEARCH.md`.

## Privacy boundary

The scan does not export raw game bytes or arbitrary executable strings. It
does not decode credentials, passwords, password-encryption material, account
or login payloads, tokens, private messages, or anti-cheat payloads. Character
and public gameplay identity remain allowed only under the rules in
`docs/PRIVACY.md`.

Private acquisition scripts and full raw research indexes remain outside this
repository. The checked-in inventory has no absolute install path or
acquisition command.
