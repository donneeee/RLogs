# Protocol packs

Protocol knowledge is immutable and addressed by game deployment, region,
client build, schema version, and content digest.

Packs define reviewed routes and typed decoders. They never contain
credentials or instructions to decode prohibited account data.

Selection is exact by deployment, channel, client build, and optional region
and executable version. A nearby build is never chosen as a fallback.

`global/reference-v1` is a format and research-coverage example only. Its fake
build ID prevents accidental live selection.
