//! Crate-private contract for asking the game to emit one game-owned marker carrier.
//!
//! This operation is explicitly post-prearm: the supervisor must already have
//! installed a source-unbound interceptor for `prearm_attempt_id`. This boundary
//! carries only the exact saved target and reviewed marker-skill identity. It has
//! no packet/socket/process identity, send operation, UI/input operation,
//! player-position dependency, or memory operation. `Issued` only means an
//! adapter accepted the request; child confirmation remains mandatory.

#![allow(dead_code)] // Deliberately unwired until a native trigger route is proven.

use crate::automarker_presets::AutomarkerPoint;

pub(crate) const fn marker_skill_id(marker_number: u8) -> Option<i32> {
    if marker_number >= 1 && marker_number <= 6 {
        Some(1_100 + marker_number as i32)
    } else {
        None
    }
}

/// Bit-preserving, bounded world target shared by prearm and native trigger.
/// Keeping the IEEE-754 bits makes boundary equality exact, including signed
/// zero, rather than relying on approximate float comparison.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct NativeMarkerTarget {
    pub marker_number: u8,
    x_bits: u32,
    y_bits: u32,
    z_bits: u32,
}

impl NativeMarkerTarget {
    pub(crate) fn from_saved_point(point: &AutomarkerPoint) -> Option<Self> {
        let axes = [point.x, point.y, point.z];
        if marker_skill_id(point.marker_number).is_none()
            || axes
                .into_iter()
                .any(|axis| !axis.is_finite() || axis.abs() > 1_000_000.0)
            || axes.into_iter().all(|axis| axis == 0.0)
        {
            return None;
        }
        Some(Self {
            marker_number: point.marker_number,
            x_bits: point.x.to_bits(),
            y_bits: point.y.to_bits(),
            z_bits: point.z.to_bits(),
        })
    }

    pub(crate) fn coordinates(self) -> [f32; 3] {
        [
            f32::from_bits(self.x_bits),
            f32::from_bits(self.y_bits),
            f32::from_bits(self.z_bits),
        ]
    }

    pub(crate) fn coordinate_bits(self) -> [u32; 3] {
        [self.x_bits, self.y_bits, self.z_bits]
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct NativeCarrierTriggerRequest {
    pub prearm_attempt_id: u64,
    pub marker_number: u8,
    pub marker_skill_id: i32,
    pub target: NativeMarkerTarget,
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

#[cfg(test)]
mod tests {
    use super::*;

    fn point(marker_number: u8, x: f32, y: f32, z: f32) -> AutomarkerPoint {
        AutomarkerPoint {
            marker_number,
            x,
            y,
            z,
        }
    }

    #[test]
    fn reviewed_marker_skill_mapping_is_closed_and_exact() {
        assert_eq!(marker_skill_id(0), None);
        for marker_number in 1..=6 {
            assert_eq!(
                marker_skill_id(marker_number),
                Some(1_100 + i32::from(marker_number))
            );
        }
        assert_eq!(marker_skill_id(7), None);
    }

    #[test]
    fn saved_target_preserves_bits_and_rejects_unusable_values() {
        let source = point(1, -0.0, 118.25, -7.5);
        let target = NativeMarkerTarget::from_saved_point(&source).unwrap();
        let [x, y, z] = target.coordinates();
        assert_eq!(x.to_bits(), source.x.to_bits());
        assert_eq!(y.to_bits(), source.y.to_bits());
        assert_eq!(z.to_bits(), source.z.to_bits());

        assert!(NativeMarkerTarget::from_saved_point(&point(0, 1.0, 2.0, 3.0)).is_none());
        assert!(NativeMarkerTarget::from_saved_point(&point(1, 0.0, -0.0, 0.0)).is_none());
        assert!(NativeMarkerTarget::from_saved_point(&point(1, f32::NAN, 2.0, 3.0)).is_none());
        assert!(NativeMarkerTarget::from_saved_point(&point(1, 1_000_001.0, 2.0, 3.0)).is_none());
    }
}
