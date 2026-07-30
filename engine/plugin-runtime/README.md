# Replay plug-in runtime

This crate is the bounded execution boundary between sealed `.rlog` canonical
events and independently versioned analysis plug-ins.

The first adapter runs trusted bundled Rust plug-ins synchronously for
deterministic replay. It enforces declared subscriptions and capabilities,
event/output/serialized-byte budgets, callback and total execution budgets,
and panic isolation. Outputs are typed, versioned snapshots or diagnostics.

Community native code is not loaded through this adapter. A future WebAssembly
component adapter will implement the same `ReplayPlugin` lifecycle with
preemptive fuel and memory isolation. Until that adapter exists, ordinary
`wasm_component` packages remain discoverable but non-executable.
