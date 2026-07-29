# BPSR UID alias example

This data-only example imports the BPSR plug-in's canonical catalog and
publishes one small alias table. It does not copy skills, UUID mappings,
localization shards, or icons.

Its `localization_lookup` hook uses `after_core`, so the normal locale resolves
first and the alias may replace only the final presentation label. Canonical
IDs and submitted log data remain unchanged.

To try it later, copy this whole folder into `plugins/installed/`. The current
repository implements discovery, resource resolution, and hook ordering; the
desktop settings UI and live localization-transform executor are later work.
