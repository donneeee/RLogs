# Parser reference registry

RLogs is an independent implementation. Public parsers are used to compare
observable behavior, protocol coverage, update strategies, workflows, and user
expectations. They are not runtime dependencies or upstream trees.

The registry separates parser lineages from forks so that agreement between
two related projects is not mistaken for independent protocol confirmation.
Source is not copied without a deliberate license and provenance review.

## Current public reference set

This snapshot was refreshed on 2026-07-28. Commits are immutable audit pins;
popularity and activity are only discovery signals and will change.

| Project | Audit pin | Primary reference value |
| --- | --- | --- |
| [StarResonanceDamageCounter](https://github.com/dmlgzs/StarResonanceDamageCounter) | `feccad955f8097933f79167883310ca5f861104d` | Largest public meter lineage; capture, packet coverage, UI expectations, and update workflow |
| [bpsr-logs](https://github.com/winjwinj/bpsr-logs) | `f5d01aa9ba01175528ac2bc77c862d767703571b` | Independent Rust parser/log implementation and website-facing workflows |
| [BPSR-Meter](https://github.com/mrsnakke/BPSR-Meter) | `a393f1a9c0651ddf74ce5d64ab295c95cc116d63` | Active StarResonanceDamageCounter-derived feature lineage |
| [BPSR-ZDPS](https://github.com/Blue-Protocol-Source/BPSR-ZDPS) | stable `v0.1.7.3` at `c0a375cc8b858562e6d381a4357e10955a80340c` | Independent C# framing, TCP reconstruction, message routing, and broad feature coverage |
| [resonance-logs-cn](https://github.com/fudiyangjin/resonance-logs-cn) | `0.2.0` at `ccdeef23c7806be5072f95a9e80b103794af3544` | Pinned CN-region protocol and feature baseline |
| [resonance-logs](https://github.com/resonance-logs/resonance-logs) | `f4aff36e573674e04db1bb09216c603ddf9fb7f6` | Historical native meter and character projection evidence: profile identity, level, fight power, gear/equipment structures, progression, encounter upload, and website integration |
| [resonance-website](https://github.com/resonance-logs/resonance-website) | `0baff9e4b625a11d09c9d579af19285695d38e12` | Historical website consumer contract: game avatar/profile images, detailed character views, Discord account linking, and upload/API behavior |
| [BPSR-Meter by Denoder](https://github.com/Denoder/BPSR-Meter) | `c8c4518c36de9145362b1cabee7652905559aa99` | StarResonanceDamageCounter-derived behavior and compatibility changes |
| [BlueMeter](https://github.com/caaatto/BlueMeter) | `c29251637b2a74567ca21028fe57ed415e3fc7aa` | Independent C# combat meter and desktop workflow |
| [StarResonanceDps](https://github.com/DannyDog/StarResonanceDps) | `1063994fc531633c40a8164a39f3bfc0d97545bb` | C# parser behavior and English-localization branch |
| [BPSR ACT Plugin](https://github.com/Garash2k/BPSR_ACT_Plugin) | `e33636098cd635635ce8d292883ac0db8dabd2d8` | ACT interoperability and plugin user expectations; no detected source license |
| [BPSR-PSO-SX](https://github.com/Sola-Ray/BPSR-PSO-SX) | `d24d58d28633ee8a82304b23ab37fd3a61e329c7` | Compact StarResonanceDamageCounter-derived UX variant |
| [Star-Resonance-Dps](https://github.com/asgharkapk/Star-Resonance-Dps) | `b365a7519cca5c8f4b1c16ca9bd3741a6e59a851` | Distribution/branch aggregation for identifying active downstream variants |

Projects discovered later are added with an immutable pin and lineage note
before their behavior is treated as evidence.

The original Resonance Logs pin is historical only. Its last public release
predates the current game by months, so its character fields—including owned
Imagines, equipment, level, seasonal progression/power, and related profile
surfaces—are extraction requirements and schema hypotheses until reverified
against a current client build. RLogs does not fork or import its parser.

The website pin is also a behavioral reference, not a server implementation
dependency. Its game character portrait comes from the character
`AvatarInfo`, while Discord avatars belong to the separate linked website
account. Its broad JSON upload model is not adopted; RLogs uses typed,
privacy-reviewed profile patches.

## Current framing consensus and variants

The audited lineages agree that an ordinary BPSR frame begins with a
big-endian length that includes its own four bytes, followed by a big-endian
16-bit fragment type whose high bit marks zstd compression. Notify uses a
16-byte routing header, and FrameDown uses a four-byte wrapper prefix followed
by nested frames.

Not every build or parser agrees beyond that boundary:

- CN 0.2.0 and ZDPS use a Call header containing service, stub, call, and
  method IDs (20 bytes); some older meter lineages assume no call ID
  (16 bytes).
- CN 0.2.0 treats FrameUp as a four-byte prefix plus nested ordinary frames;
  ZDPS exposes a distinct embedded-record structure.
- observed Return header handling ranges from four to twelve bytes.

RLogs models these as explicit region/build framing layouts. The default
research profile follows the pinned CN/ZDPS Call layout, the ZDPS Return
layout, and keeps FrameUp opaque. Full wire bytes are retained for every
variant so a later protocol pack can reinterpret them without recapture.

## RLogs Global feature baseline

The user's Global tree is a desired-feature and packet-evidence reference:

- repository: [donneeee/resonance-logs-global](https://github.com/donneeee/resonance-logs-global)
- baseline commit: `77380cabdc8505267a8971022e38859b9400dd28`
- baseline date: 2026-07-08

Its useful coverage includes scene and dungeon identity, nearby and local
entity deltas, full attribute/value forms including positions, damage taken,
healing, deaths, skill casts and cooldowns, active buffs, effect/factor
sources, modifier windows and replay evidence, equipment, gear sets, passive
skills, profession skills and talents, party state, class/spec inference, and
character-profile projections.

RLogs will reimplement required behavior above canonical events and protocol
packs. It will not transplant Global's runtime architecture, UI coupling,
blocking probes, or high-frequency state fan-out.

## Evidence rules

1. A parser observation is a hypothesis until confirmed by a sanitized capture,
   game data, or an independent implementation.
2. Region, channel, and client build are always recorded with the observation.
3. Unknown routes and undecoded fields remain lossless evidence.
4. Conflicts are preserved as protocol-pack variants instead of being forced
   into one global interpretation.
5. Credential, login, token, payment, and private account routes are prohibited
   research domains. Character and public gameplay data require an explicit
   allowlist.
