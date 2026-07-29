# Named CTB schema inventory

This directory is the sanitized, build-scoped schema review layer for every
named CTB in Global Steam build 24252055.

- `domains/` keeps tables in human-readable subject folders.
- `review-worklist.json` orders unresolved fields without calling them decoded.
- `semantic_fields` are corroborated on this exact client build.
- `strong_candidates` are exact local pool references, not final field names.
- `index.json.pool_models` records container-wide pool interpretations and
  keeps unresolved pool types explicitly opaque.

Rows are packed. Offsets are byte offsets and are not assumed to be
four-byte-aligned. Localization membership and cross-table integer matches stay
private candidate evidence until their meaning is corroborated.

This inventory is research metadata. It is not loaded by the live parser.
