# Protocol packs

Protocol packs are game-owned resources. Current BPSR packs live at
[`plugins/games/blue-protocol-star-resonance/protocol-packs/`](../plugins/games/blue-protocol-star-resonance/protocol-packs/).
This document defines the shared rules future game plug-ins must follow.

Protocol knowledge is immutable and addressed by game deployment, region,
client build, schema version, and content digest.

Packs define reviewed routes and typed decoders. They never contain
credentials or instructions to decode prohibited account data.

Selection is exact by deployment, channel, client build, and optional region
and executable version. A nearby build is never chosen as a fallback.

`global/reference-v1` is a format and research-coverage example only. Its fake
build ID prevents accidental live selection.

`global/steam-24252055` is an exact-build research pack. Controlled observations
may verify individual route identities, but every route stays opaque until its
privacy-reviewed selective decoder is proven by a sanitized fixture.
