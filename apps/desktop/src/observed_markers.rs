use std::sync::Mutex;

use rlogs_game_bpsr::{LocalMapMarker, LocalMapMarkerSnapshotError};
use serde::Serialize;

pub const OBSERVED_MARKER_SNAPSHOT_SCHEMA_VERSION: u16 = 2;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ObservedMarkerSessionStamp {
    pub session_id: String,
    pub deployment_id: String,
    pub client_build: String,
    pub protocol_pack_digest: String,
    pub protocol_supported: bool,
    pub request_observer_supported: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ObservedMarkerPoint {
    pub marker_number: u8,
    pub x: f32,
    pub y: f32,
    pub z: f32,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ObservedMarkerSnapshot {
    pub schema_version: u16,
    pub revision: u64,
    pub capture_active: bool,
    pub protocol_supported: bool,
    pub request_observer_supported: bool,
    pub verified_request_count: u32,
    pub last_verified_request_marker_number: Option<u8>,
    pub last_verified_request_observed_micros: Option<u64>,
    pub reason: &'static str,
    pub session_id: Option<String>,
    pub deployment_id: Option<String>,
    pub client_build: Option<String>,
    pub protocol_pack_digest: Option<String>,
    pub scene_id: Option<i32>,
    pub map_id: Option<u32>,
    pub observed_micros: Option<u64>,
    pub markers: Vec<ObservedMarkerPoint>,
}

impl Default for ObservedMarkerSnapshot {
    fn default() -> Self {
        Self {
            schema_version: OBSERVED_MARKER_SNAPSHOT_SCHEMA_VERSION,
            revision: 0,
            capture_active: false,
            protocol_supported: false,
            request_observer_supported: false,
            verified_request_count: 0,
            last_verified_request_marker_number: None,
            last_verified_request_observed_micros: None,
            reason: "live_capture_not_running",
            session_id: None,
            deployment_id: None,
            client_build: None,
            protocol_pack_digest: None,
            scene_id: None,
            map_id: None,
            observed_micros: None,
            markers: Vec::new(),
        }
    }
}

/// Read-only bridge between the process-owned packet reducer and automarker UI.
///
/// This feed never decodes packets and never grants protocol authority. Its
/// caller must derive `protocol_supported` from the exact marker decoder gate.
/// Coordinates remain unavailable unless that gate is true and both scene and
/// map were observed in the same active capture session.
#[derive(Debug, Default)]
pub struct ObservedMarkerFeed {
    snapshot: Mutex<ObservedMarkerSnapshot>,
}

impl ObservedMarkerFeed {
    pub fn begin_session(&self, stamp: ObservedMarkerSessionStamp) {
        let reason = if stamp.protocol_supported {
            "waiting_for_packet_observed_scene_and_map"
        } else {
            "marker_protocol_not_verified_for_build_pack"
        };
        self.replace(ObservedMarkerSnapshot {
            schema_version: OBSERVED_MARKER_SNAPSHOT_SCHEMA_VERSION,
            revision: 0,
            capture_active: true,
            protocol_supported: stamp.protocol_supported,
            request_observer_supported: stamp.request_observer_supported,
            verified_request_count: 0,
            last_verified_request_marker_number: None,
            last_verified_request_observed_micros: None,
            reason,
            session_id: Some(stamp.session_id),
            deployment_id: Some(stamp.deployment_id),
            client_build: Some(stamp.client_build),
            protocol_pack_digest: Some(stamp.protocol_pack_digest),
            scene_id: None,
            map_id: None,
            observed_micros: None,
            markers: Vec::new(),
        });
    }

    pub fn publish(
        &self,
        session_id: &str,
        scene_id: Option<i32>,
        map_id: Option<u32>,
        observed_micros: Option<u64>,
        marker_snapshot: Result<Vec<LocalMapMarker>, LocalMapMarkerSnapshotError>,
    ) {
        let mut current = self
            .snapshot
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if !current.capture_active
            || !current.protocol_supported
            || current.session_id.as_deref() != Some(session_id)
        {
            return;
        }
        let (Some(scene_id), Some(map_id)) = (scene_id, map_id) else {
            let next = ObservedMarkerSnapshot {
                revision: current.revision,
                reason: "waiting_for_packet_observed_scene_and_map",
                scene_id: None,
                map_id: None,
                observed_micros: None,
                markers: Vec::new(),
                ..current.clone()
            };
            if *current != next {
                let mut next = next;
                next.revision = current.revision.saturating_add(1);
                *current = next;
            }
            return;
        };
        let (markers, mut reason) = match marker_snapshot {
            Ok(markers) => (markers, "observed_markers_available"),
            Err(LocalMapMarkerSnapshotError::Empty) => {
                (Vec::new(), "no_fully_positioned_markers_observed")
            }
            Err(_) => (Vec::new(), "observed_marker_snapshot_invalid"),
        };
        let mut numbers = [false; 6];
        let mut points = Vec::with_capacity(markers.len());
        for marker in markers {
            let (Some(x), Some(y), Some(z)) = (marker.x, marker.y, marker.z) else {
                reason = "observed_marker_snapshot_invalid";
                points.clear();
                break;
            };
            if !(1..=6).contains(&marker.marker_number)
                || !x.is_finite()
                || !y.is_finite()
                || !z.is_finite()
                || numbers[usize::from(marker.marker_number.saturating_sub(1))]
            {
                reason = "observed_marker_snapshot_invalid";
                points.clear();
                break;
            }
            numbers[usize::from(marker.marker_number - 1)] = true;
            points.push(ObservedMarkerPoint {
                marker_number: marker.marker_number,
                x,
                y,
                z,
            });
        }
        points.sort_by_key(|point| point.marker_number);
        let next = ObservedMarkerSnapshot {
            revision: current.revision,
            reason,
            scene_id: Some(scene_id),
            map_id: Some(map_id),
            observed_micros,
            markers: points,
            ..current.clone()
        };
        if *current != next {
            let mut next = next;
            next.revision = current.revision.saturating_add(1);
            *current = next;
        }
    }

    pub fn finish_session(&self, session_id: &str) {
        let mut current = self
            .snapshot
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if current.session_id.as_deref() != Some(session_id) {
            return;
        }
        let revision = current.revision.saturating_add(1);
        *current = ObservedMarkerSnapshot {
            revision,
            capture_active: false,
            protocol_supported: false,
            request_observer_supported: false,
            verified_request_count: 0,
            last_verified_request_marker_number: None,
            last_verified_request_observed_micros: None,
            reason: "live_capture_not_running",
            scene_id: None,
            map_id: None,
            observed_micros: None,
            markers: Vec::new(),
            ..current.clone()
        };
    }

    pub fn current(&self) -> ObservedMarkerSnapshot {
        self.snapshot
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone()
    }

    /// Records only a bounded acknowledgement that the exact current-build
    /// outbound request decoder recognized a normal marker placement. Request
    /// bytes, encrypted attributes, session sequences, and coordinates never
    /// cross into this feed.
    pub fn observe_verified_request(
        &self,
        session_id: &str,
        marker_number: u8,
        observed_micros: u64,
    ) {
        let mut current = self
            .snapshot
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if !current.capture_active
            || !current.request_observer_supported
            || current.session_id.as_deref() != Some(session_id)
            || !(1..=6).contains(&marker_number)
        {
            return;
        }
        current.verified_request_count = current.verified_request_count.saturating_add(1);
        current.last_verified_request_marker_number = Some(marker_number);
        current.last_verified_request_observed_micros = Some(observed_micros);
        current.revision = current.revision.saturating_add(1);
    }

    fn replace(&self, mut next: ObservedMarkerSnapshot) {
        let mut current = self
            .snapshot
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        next.revision = current.revision.saturating_add(1);
        *current = next;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn stamp(session_id: &str, protocol_supported: bool) -> ObservedMarkerSessionStamp {
        ObservedMarkerSessionStamp {
            session_id: session_id.into(),
            deployment_id: "global".into(),
            client_build: "24687926".into(),
            protocol_pack_digest: "sha256:reviewed".into(),
            protocol_supported,
            request_observer_supported: protocol_supported,
        }
    }

    fn marker(number: u8) -> LocalMapMarker {
        LocalMapMarker {
            passive_instance_id: i64::from(number),
            related_entity_uuid: None,
            marker_number: number,
            x: Some(f32::from(number)),
            y: Some(2.0),
            z: Some(3.0),
        }
    }

    #[test]
    fn unsupported_protocol_never_exposes_coordinates() {
        let feed = ObservedMarkerFeed::default();
        feed.begin_session(stamp("one", false));
        feed.publish("one", Some(100), Some(200), Some(300), Ok(vec![marker(1)]));
        let snapshot = feed.current();
        assert!(snapshot.capture_active);
        assert!(!snapshot.protocol_supported);
        assert_eq!(
            snapshot.reason,
            "marker_protocol_not_verified_for_build_pack"
        );
        assert!(snapshot.markers.is_empty());
        assert_eq!(snapshot.scene_id, None);
        assert_eq!(snapshot.map_id, None);
    }

    #[test]
    fn context_and_session_are_required_and_stale_workers_cannot_mutate_feed() {
        let feed = ObservedMarkerFeed::default();
        feed.begin_session(stamp("one", true));
        feed.publish("one", Some(100), None, Some(300), Ok(vec![marker(1)]));
        assert!(feed.current().markers.is_empty());

        feed.begin_session(stamp("two", true));
        feed.publish("one", Some(100), Some(200), Some(300), Ok(vec![marker(1)]));
        assert!(feed.current().markers.is_empty());
        feed.publish(
            "two",
            Some(100),
            Some(200),
            Some(301),
            Ok(vec![marker(2), marker(1)]),
        );
        let snapshot = feed.current();
        assert_eq!(snapshot.session_id.as_deref(), Some("two"));
        assert_eq!(snapshot.scene_id, Some(100));
        assert_eq!(snapshot.map_id, Some(200));
        assert_eq!(snapshot.observed_micros, Some(301));
        assert_eq!(
            snapshot
                .markers
                .iter()
                .map(|point| point.marker_number)
                .collect::<Vec<_>>(),
            vec![1, 2]
        );

        feed.finish_session("one");
        assert!(feed.current().capture_active);
        feed.finish_session("two");
        let snapshot = feed.current();
        assert!(!snapshot.capture_active);
        assert!(snapshot.markers.is_empty());
        assert_eq!(snapshot.scene_id, None);
        assert_eq!(snapshot.map_id, None);
    }

    #[test]
    fn incomplete_or_invalid_points_are_not_exposed() {
        let feed = ObservedMarkerFeed::default();
        feed.begin_session(stamp("one", true));
        feed.publish(
            "one",
            Some(100),
            Some(200),
            Some(300),
            Err(LocalMapMarkerSnapshotError::MissingCoordinate),
        );
        let snapshot = feed.current();
        assert!(snapshot.markers.is_empty());
        assert_eq!(snapshot.reason, "observed_marker_snapshot_invalid");
    }

    #[test]
    fn verified_request_diagnostic_is_bounded_to_the_active_session() {
        let feed = ObservedMarkerFeed::default();
        feed.begin_session(stamp("one", true));
        feed.observe_verified_request("stale", 1, 10);
        feed.observe_verified_request("one", 0, 11);
        assert_eq!(feed.current().verified_request_count, 0);

        feed.observe_verified_request("one", 3, 12);
        let snapshot = feed.current();
        assert_eq!(snapshot.verified_request_count, 1);
        assert_eq!(snapshot.last_verified_request_marker_number, Some(3));
        assert_eq!(snapshot.last_verified_request_observed_micros, Some(12));

        feed.begin_session(stamp("two", true));
        let reset = feed.current();
        assert_eq!(reset.verified_request_count, 0);
        assert_eq!(reset.last_verified_request_marker_number, None);
        assert_eq!(reset.last_verified_request_observed_micros, None);
        feed.finish_session("two");
        let finished = feed.current();
        assert!(!finished.request_observer_supported);
        assert_eq!(finished.verified_request_count, 0);
    }
}
