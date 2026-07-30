# Bundled plugins

These are RLogs' native user-facing features. Each will remain independently
testable and replaceable through the public plugin contracts.

`localization/` is intentionally data-only. Its packages use the add-on
contract but cannot execute code or request plugin capabilities.

`combat-meter/` is the first executable built-in. It reduces canonical events
from a verified `.rlog` into deterministic encounter and actor summaries. It
does not parse packets, know game opcodes, or calculate rDPS; those remain
separate game-integration and attribution responsibilities.
