//! Crate-private contract for asking the game to emit one game-owned marker carrier.
//!
//! This operation is explicitly post-prearm: the supervisor must already have
//! installed a source-unbound interceptor for `prearm_attempt_id`. This boundary
//! has no coordinates, packet/socket/process identity, send operation, UI/input
//! operation, player-position dependency, or memory operation. `Issued` only
//! means an adapter accepted the request; child confirmation remains mandatory.

#![allow(dead_code)] // Deliberately unwired until a native trigger route is proven.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct NativeCarrierTriggerRequest {
    pub prearm_attempt_id: u64,
    pub marker_number: u8,
    pub context_generation: u64,
    pub requested_micros: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum NativeCarrierTriggerReceipt {
    Issued { attempt_id: u64, issued_micros: u64 },
    Unavailable,
    Rejected,
}

pub(crate) trait NativeAutomarkerCarrierTrigger {
    fn request_game_owned_carrier_after_prearm(
        &mut self,
        request: NativeCarrierTriggerRequest,
    ) -> NativeCarrierTriggerReceipt;

    fn cancel(&mut self, _attempt_id: u64) {}
}

/// Production-safe placeholder until an exact-build, no-menu, game-owned
/// trigger route is proven. It performs no native action and cannot send.
#[derive(Debug, Default)]
pub(crate) struct UnavailableNativeCarrierTrigger;

impl NativeAutomarkerCarrierTrigger for UnavailableNativeCarrierTrigger {
    fn request_game_owned_carrier_after_prearm(
        &mut self,
        _request: NativeCarrierTriggerRequest,
    ) -> NativeCarrierTriggerReceipt {
        NativeCarrierTriggerReceipt::Unavailable
    }
}
