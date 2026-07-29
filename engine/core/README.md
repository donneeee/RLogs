# Core pipeline

This crate owns the game-neutral captured-frame to reconstructed-TCP-stream
orchestration used by offline replay and live adapters.

Research input must provide an exact allowlist of game TCP connections. Frames
outside those client/server endpoint pairs never enter TCP reconstruction.
Output is delivered to a trusted game plug-in; Core contains no game-specific
framing, opcodes, routes, game-data IDs, or character schema.
