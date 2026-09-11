# External references

Other combat meters, packet tools, game projects, ACT, and FFLogs may be
studied to understand user expectations, observable behavior, and useful
workflows.

References are not runtime dependencies. Source code is not copied into RLogs
without an explicit provenance and license review. Behavioral observations and
independently produced packet captures must be documented as research
evidence, not represented as original protocol certainty.

The maintained parser catalog and immutable audit pins are in
[`PARSER_REFERENCES.md`](PARSER_REFERENCES.md).

## Resonance Logs CN reference baseline

For current observable-feature and protocol research, use Resonance Logs CN
`0.2.4` plus its post-release monster-catalog correction, pinned at commit
`bd71d2dfd3c7289e6398c4cd042f4357d4f35721` (reviewed 2026-09-11). The full
source-level feature and dependency inventory is recorded in
[`RESONANCE_LOGS_CN_REFERENCE_AUDIT.md`](RESONANCE_LOGS_CN_REFERENCE_AUDIT.md).
The attributed module-optimizer
compatibility port remains pinned to the separately reviewed `0.2.0` tree at
`ccdeef23c7806be5072f95a9e80b103794af3544`; updating the research audit does
not silently update that licensed compatibility baseline.

This pin is a research baseline, not an upstream relationship. The reference is
AGPL-3.0-only. RLogs does not merge from that tree or include its code, assets,
generated tables, or binaries as a build or runtime dependency. Features are
reimplemented from independently described behavior and RLogs-owned evidence.
Moving the baseline requires a deliberate reference review and a newly recorded
commit.

References are also deployment-scoped. Numeric agreement is not semantic
agreement across CN and Global builds. The reviewed `0.2.4` CN source labels entity
attributes 51, 53, and 70 as Defense Power, Gear Tier, and Base Strength,
whereas exact Global build 24687926 metadata identifies those IDs as
`AttrTargetDir`, `AttrTargetPos`, and `AttrVelocity`. RLogs records that
conflict and rejects the CN names as Global decoder and formula authority.
