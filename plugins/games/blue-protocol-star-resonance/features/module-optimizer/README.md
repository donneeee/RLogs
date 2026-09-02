# BPSR module optimizer

This crate is the portable module optimizer shared by the local rLogs desktop
surface and future website/WASM consumers for Blue Protocol: Star Resonance.
It is deliberately separate from packet capture, character decoding, and
presentation code.

The scoring and search behavior is a Rust port of the module optimizer in
`fudiyangjin/resonance-logs-cn` version `0.2.0`, commit
`ccdeef23c7806be5072f95a9e80b103794af3544`, used under AGPL-3.0. The port:

- preserves CN's module-part aggregation, preference-weighted ranking, minimum
  attribute constraints, thresholds, total-link scoring, and 4/5-module search;
- reads thresholds and fight values from the exact-build RLogs catalog instead
  of freezing current game values in the website;
- uses browser-safe string instance IDs;
- returns actual unweighted power separately from the preference score used to
  rank recommendations, plus an optional scored current-equipment baseline;
- keeps that current baseline outside `max_solutions`, so requesting 20
  results returns 20 alternative recommendations plus Current;
- uses reviewed optimizer aliases without overwriting exact game-client
  localization strings in the shared catalog;
- supplies deterministic tie-breaking, an exact verification mode, and a
  bounded beam mode for full inventories (512 states by default, adjustable
  through the API);
- runs complementary attribute-cluster, special-effect, and total-link
  candidate orderings with feasible greedy completion scoring so threshold
  synergies survive early pruning;
- parallelizes every beam frontier across the available Rayon worker pool,
  allowing large inventories to use far more than the three ordering tasks;
- optionally runs exact enumeration through a dynamically loaded OpenCL 1.2
  backend for NVIDIA and AMD GPUs, with an exact multi-core CPU fallback;
- combines the full-inventory CPU beam with an exact OpenCL companion search
  for large inventories, preserving the CPU quality floor while the GPU
  explores a diverse bounded shortlist;
- compiles and retains a successful OpenCL runtime only after the explicit GPU
  check, performs device-side radix top-result reduction, and keeps small exact
  searches on CPU when GPU dispatch overhead would be slower;
- lets browser clients choose a conservative 256-2,048-state beam width from
  the device's reported CPU concurrency without changing the scoring rules or
  blocking the browser's main thread;
- contains no packet capture, account, login, password, or token handling.

The desktop host exposes this crate through a loopback-only JSON API and the
local Module Optimizer add-on. A future standalone site can use the same
request/response contract from a server worker or a WASM wrapper without
adopting the desktop UI.
