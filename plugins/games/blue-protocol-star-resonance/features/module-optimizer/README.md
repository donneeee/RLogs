# BPSR module optimizer

This crate is the portable, website-facing module optimizer for Blue Protocol:
Star Resonance. It is deliberately separate from packet capture, character
decoding, and presentation code.

The scoring and search behavior is a Rust port of the module optimizer in
`fudiyangjin/resonance-logs-cn` version `0.2.0`, commit
`ccdeef23c7806be5072f95a9e80b103794af3544`, used under AGPL-3.0. The port:

- preserves CN's module-part aggregation, target/exclusion weighting, minimum
  attribute constraints, thresholds, total-link scoring, and 4/5-module search;
- reads thresholds and fight values from the exact-build RLogs catalog instead
  of freezing current game values in the website;
- uses browser-safe string instance IDs;
- supplies deterministic tie-breaking, an exact verification mode, and a
  bounded beam mode for full inventories (512 states by default, adjustable
  through the API);
- contains no packet capture, account, login, password, or token handling.

The Plugin Lab exposes this crate through a loopback-only JSON API and browser
screen. A future standalone site can use the same request/response contract
from a server worker or a WASM wrapper without adopting the desktop UI.
