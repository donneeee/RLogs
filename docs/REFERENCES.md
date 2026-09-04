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
`0.2.3` pinned at commit
`7d956e41fb37b4ba0577a9acd8ab16121906a6d0`. The attributed module-optimizer
compatibility port remains pinned to the separately reviewed `0.2.0` tree at
`ccdeef23c7806be5072f95a9e80b103794af3544`; updating the research audit does
not silently update that licensed compatibility baseline.

This pin is a research baseline, not an upstream relationship. RLogs does not
merge from that tree or include it as a build or runtime dependency. Moving the
baseline requires a deliberate reference review and a newly recorded commit.

References are also deployment-scoped. Numeric agreement is not semantic
agreement across CN and Global builds. The `0.2.3` CN source labels entity
attributes 51, 53, and 70 as Defense Power, Gear Tier, and Base Strength,
whereas exact Global build 24687926 metadata identifies those IDs as
`AttrTargetDir`, `AttrTargetPos`, and `AttrVelocity`. RLogs records that
conflict and rejects the CN names as Global decoder and formula authority.
