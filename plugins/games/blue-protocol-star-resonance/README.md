# Blue Protocol: Star Resonance game plug-in

This is the bundled, trusted BPSR integration for the game-neutral rLogs
runtime. It is an original implementation informed by public behavioral
references; it is not a fork of another parser.

The plug-in owns everything whose meaning depends on BPSR:

- executable/process selectors for Windows and Linux;
- message framing, compression, routes, opcodes, and exact-build packs;
- region/build resolution;
- privacy-reviewed protocol decoding;
- scene, map, entity, monster, UUID, skill, status, equipment, cosmetic,
  Imagine, guild, and localization mappings;
- the typed BPSR character-profile schema;
- projection of a public character profile into Core's website-payload
  envelope and relative profile endpoint;
- sanitized mapping evidence and game-file inventories.

Core owns the reusable pcap input, network/IP/TCP reconstruction, neutral event
carrier, plug-in host contracts, website base URL/authentication, retry state,
and final transport. The BPSR plug-in cannot choose a remote host or receive
website credentials.

## Human-readable layout

```text
blue-protocol-star-resonance/
  plugin.toml                 trusted plug-in manifest and shared exports
  src/
    pipeline.rs               Core TCP streams -> BPSR frames
    framing.rs                BPSR wire framing
    decoder.rs                reviewed message -> canonical event drafts
    dirty_blob_v1.rs          bounded selective dirty-container reader
    profile.rs                typed public character profile
    offline_recording.rs      pcap-decoded events -> sealed rlog and coverage
    website.rs                BPSR profile -> neutral website request
  protocol-packs/
    <deployment>/
      server-realms.json      readable realm names and verified endpoint rules
      <build>/                exact client-build route knowledge
  protocol-references/
    manifests/                pinned public reference evidence
    profile-bridges/          reviewed route-to-profile evidence
  game-data/
    catalog/
      skills/<class>/<spec>/
      statuses/<class>/<spec>/
      talents/<class>/<spec>/
      modules/<type>/
      entities/
      equipment/
      profile/
      localization/
  research/
    game-file-inventory/
      <deployment>/<build>/   sanitized schemas and mapping worklists
  tools/
    offline-recorder/         private pcap -> canonical rlog and safe coverage
    protocol-journal/         BPSR private packet journal
    protocol-coverage/        BPSR route and byte coverage
    profile-audit/            BPSR profile-field/privacy audit

RLogs/assets/blue-protocol-star-resonance/shared/
  icons/                      canonical game icons exported for reuse
```

The catalog is sharded for bounded memory and human navigation. Numeric
identity remains authoritative; readable names are presentation metadata.
The icon bytes live once in the game plug-in's provider-owned shared asset
namespace. Catalog records keep portable `icons/...` paths, and consumers
import the versioned `icons` resource instead of hard-coding its install path.
Raw game files, private acquisition scripts, packet captures, account data,
passwords, tokens, private chat, and login messages are not part of this
plug-in.

For Global, `global` is the deployment while Asteria and Bahamar are separate
realms. Geographic region remains unset until independently verified. Realm
endpoint rules begin as exact user-confirmed observations; they are not widened
into guessed address ranges. The narrow `NotifyEnterWorld` decoder declares
only scene host and port fields; protobuf tags containing the account ID and
login token are intentionally absent from the compiled schema. Its endpoint
observation stays inside the trusted game plug-in and is not emitted as a
canonical website event.

The offline recorder deliberately keeps two artifacts separate: raw and opaque
packet evidence stays in private research files, while `.rlog` receives only
events accepted by the protocol pack and canonical privacy boundary. The
coverage sidecar records numeric route and outcome counts without copying
payloads or network endpoints.

The current Global build pack has reviewed decoders for world entry, scene
entry, nearby entity snapshots, public social-profile snapshots, the
owner-character snapshot and its incremental dirty stream, complete dungeon
snapshot timeline fields, current season identity, authoritative server time,
nearby deltas, and local-player deltas. The dirty-stream reader is bounded,
consumes known private account/time fields without retaining them, and stops
at an unknown field whose width is not proven. The social-profile decoder
retains public character/display IDs,
name/level, character-facing avatar IDs, profession and weapon-skin IDs,
equipped item IDs, combat power, season level/strength, guild ID/name,
titles/medals, and world context. Equivalent equipment sets are sorted before
comparison, and profile deduplication is independent from real scene/line
changes.

The complete owner snapshot additionally maps full face/color/avatar
appearance, level progression, detailed equipped item identity and attributes,
combat-profession skills/loadouts/talents, talent point totals, equipped
modules and the package-5 module inventory, life professions, and fashion,
mount, weapon-skin, and dye collections. It joins item instances to equipped
gear/module slots locally and emits only the resulting privacy-reviewed
character patch. Module instance IDs are emitted as strings for browser-safe
joins. Unrelated inventory, acquisition/expiration times, binding/source
metadata, currencies, arbitrary effect strings, and account-shaped subtrees
are not declared.

Account ID/data subtrees and user-supplied profile/half-body image URLs are
absent from the compiled social schema. A synthetic full-envelope test proves
that those skipped values cannot reach canonical events.
Dungeon snapshots retain public scene-instance, difficulty, objective ID,
objective value, and completion state. Only explicit flow transitions create
run events: `Playing` starts a run and `End` ends it without guessing whether
the outcome was a clear or failure. A missing or `Null` flow state never
invents a dungeon start.

## Loss rule

An unknown route is still a valid local packet record. Queue drops, TCP gaps,
decompression failures, malformed frames, and uncertain mappings are explicit
evidence. They do not disappear merely because the current decoder does not
understand them.
