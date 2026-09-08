//! Desktop-facing re-exports of the shared, replayable BPSR dummy validator.
//!
//! Keeping this module preserves the host's existing integration boundary
//! while ensuring the desktop and hosted verifier execute the same rules.

pub use rlogs_game_bpsr::{TrainingDummyController, TrainingDummyState};
