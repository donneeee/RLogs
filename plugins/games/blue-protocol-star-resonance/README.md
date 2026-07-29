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
    profile.rs                typed public character profile
    website.rs                BPSR profile -> neutral website request
  protocol-packs/
    <deployment>/<build>/     exact client-build route knowledge
  protocol-references/
    manifests/                pinned public reference evidence
    profile-bridges/          reviewed route-to-profile evidence
  game-data/
    catalog/
      skills/<class>/<spec>/
      statuses/<class>/<spec>/
      entities/
      equipment/
      profile/
      localization/
  research/
    game-file-inventory/
      <deployment>/<build>/   sanitized schemas and mapping worklists
  tools/
    protocol-journal/         BPSR private packet journal
    protocol-coverage/        BPSR route and byte coverage
    profile-audit/            BPSR profile-field/privacy audit

RLogs/assets/shared/blue-protocol-star-resonance/
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

## Loss rule

An unknown route is still a valid local packet record. Queue drops, TCP gaps,
decompression failures, malformed frames, and uncertain mappings are explicit
evidence. They do not disappear merely because the current decoder does not
understand them.
