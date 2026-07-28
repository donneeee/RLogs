use std::collections::BTreeMap;

use crate::{
    CaptureGapKind, CaptureRecord, CaptureRecordKind, CompressionState, FragmentKind,
    PacketDirection, PacketEnvelope, RouteCatalog, RouteKey,
};

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct RouteCoverage {
    pub packet_count: u64,
    pub wire_bytes: u64,
    pub application_bytes: u64,
    pub first_sequence: u64,
    pub last_sequence: u64,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct FragmentCoverage {
    pub packet_count: u64,
    pub wire_bytes: u64,
    pub application_bytes: u64,
    pub first_sequence: u64,
    pub last_sequence: u64,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CoverageReport {
    routes: BTreeMap<RouteKey, RouteCoverage>,
    fragments: BTreeMap<(PacketDirection, FragmentKind), FragmentCoverage>,
    compression: BTreeMap<CompressionState, u64>,
    gaps: BTreeMap<CaptureGapKind, u64>,
    pub packet_count: u64,
    pub gap_count: u64,
    pub unrouted_packet_count: u64,
    pub unclassified_fragment_packet_count: u64,
    pub wire_bytes: u64,
    pub application_bytes: u64,
}

impl CoverageReport {
    pub fn observe(&mut self, record: &CaptureRecord) {
        match &record.kind {
            CaptureRecordKind::Packet(packet) => {
                self.packet_count += 1;
                self.wire_bytes += packet.payload.wire_bytes.len() as u64;
                self.application_bytes += packet
                    .payload
                    .application_bytes
                    .as_ref()
                    .map_or(0, |bytes| bytes.len() as u64);
                *self.compression.entry(packet.compression).or_default() += 1;

                let direction = packet_direction(packet);
                if let Some(fragment) = packet_fragment(packet) {
                    let coverage =
                        self.fragments
                            .entry((direction, fragment))
                            .or_insert(FragmentCoverage {
                                first_sequence: record.sequence,
                                ..FragmentCoverage::default()
                            });
                    coverage.packet_count += 1;
                    coverage.wire_bytes += packet.payload.wire_bytes.len() as u64;
                    coverage.application_bytes += packet
                        .payload
                        .application_bytes
                        .as_ref()
                        .map_or(0, |bytes| bytes.len() as u64);
                    coverage.last_sequence = record.sequence;
                } else {
                    self.unclassified_fragment_packet_count += 1;
                }

                let Some(routed) = packet.route else {
                    self.unrouted_packet_count += 1;
                    return;
                };

                let coverage = self.routes.entry(routed.key).or_insert(RouteCoverage {
                    first_sequence: record.sequence,
                    ..RouteCoverage::default()
                });
                coverage.packet_count += 1;
                coverage.wire_bytes += packet.payload.wire_bytes.len() as u64;
                coverage.application_bytes += packet
                    .payload
                    .application_bytes
                    .as_ref()
                    .map_or(0, |bytes| bytes.len() as u64);
                coverage.last_sequence = record.sequence;
            }
            CaptureRecordKind::Gap(gap) => {
                self.gap_count += 1;
                *self.gaps.entry(gap.kind).or_default() += 1;
            }
        }
    }

    pub fn route(&self, route: &RouteKey) -> Option<&RouteCoverage> {
        self.routes.get(route)
    }

    pub fn routes(&self) -> &BTreeMap<RouteKey, RouteCoverage> {
        &self.routes
    }

    pub fn fragments(&self) -> &BTreeMap<(PacketDirection, FragmentKind), FragmentCoverage> {
        &self.fragments
    }

    pub fn compression(&self) -> &BTreeMap<CompressionState, u64> {
        &self.compression
    }

    pub fn gaps(&self) -> &BTreeMap<CaptureGapKind, u64> {
        &self.gaps
    }

    pub fn summarize(&self, catalog: &RouteCatalog) -> CoverageSummary {
        let mut summary = CoverageSummary::default();

        for (route, coverage) in &self.routes {
            if catalog.contains(route) {
                summary.known_routes += 1;
                summary.known_packets += coverage.packet_count;
            } else {
                summary.unknown_routes += 1;
                summary.unknown_packets += coverage.packet_count;
            }
        }

        summary
    }
}

fn packet_direction(packet: &PacketEnvelope) -> PacketDirection {
    if packet.direction != PacketDirection::Unknown {
        return packet.direction;
    }

    packet
        .route
        .map_or(PacketDirection::Unknown, |route| route.key.direction)
}

fn packet_fragment(packet: &PacketEnvelope) -> Option<FragmentKind> {
    packet
        .fragment
        .or_else(|| packet.route.map(|route| route.key.fragment))
        .or_else(|| {
            let bytes = packet.payload.wire_bytes.get(4..6)?;
            let packet_type = u16::from_be_bytes(bytes.try_into().ok()?);
            Some(FragmentKind::from_wire_id(packet_type & 0x7fff))
        })
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct CoverageSummary {
    pub known_routes: u64,
    pub unknown_routes: u64,
    pub known_packets: u64,
    pub unknown_packets: u64,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        CaptureRecordKind, CompressionState, FragmentKind, PacketDirection, PacketEnvelope,
        PacketPayload, RouteDefinition, RoutedMessage,
    };

    fn record(sequence: u64, route: Option<RouteKey>, wire_len: usize) -> CaptureRecord {
        CaptureRecord {
            sequence,
            observed_micros: sequence,
            wall_clock_unix_micros: None,
            kind: CaptureRecordKind::Packet(PacketEnvelope {
                connection_id: 1,
                stream_id: 1,
                source: None,
                destination: None,
                direction: route.map_or(PacketDirection::Unknown, |key| key.direction),
                fragment: route.map(|key| key.fragment),
                route: route.map(|key| RoutedMessage {
                    key,
                    stub_id: 0,
                    call_id: None,
                }),
                compression: CompressionState::NotCompressed,
                payload: PacketPayload {
                    wire_bytes: vec![0; wire_len],
                    application_bytes: Some(vec![0; wire_len.saturating_sub(1)]),
                },
            }),
        }
    }

    #[test]
    fn coverage_keeps_known_unknown_and_unrouted_packets_separate() {
        let known = RouteKey::new(PacketDirection::ServerToClient, FragmentKind::Notify, 10, 1);
        let unknown = RouteKey::new(PacketDirection::ServerToClient, FragmentKind::Notify, 10, 2);
        let mut catalog = RouteCatalog::new();
        catalog
            .insert(RouteDefinition {
                route: known,
                service_name: "WorldNtf".into(),
                method_name: "Known".into(),
                message_name: None,
                confidence: crate::MappingConfidence::Verified,
                provenance: Vec::new(),
            })
            .unwrap();

        let mut report = CoverageReport::default();
        report.observe(&record(1, Some(known), 5));
        report.observe(&record(2, Some(unknown), 7));
        report.observe(&record(3, None, 3));

        assert_eq!(report.packet_count, 3);
        assert_eq!(report.unrouted_packet_count, 1);
        assert_eq!(report.fragments().len(), 1);
        assert_eq!(
            report
                .fragments()
                .get(&(PacketDirection::ServerToClient, FragmentKind::Notify))
                .unwrap()
                .packet_count,
            2
        );
        assert_eq!(report.unclassified_fragment_packet_count, 1);
        assert_eq!(
            report.compression().get(&CompressionState::NotCompressed),
            Some(&3)
        );
        assert_eq!(report.wire_bytes, 15);
        assert_eq!(
            report.summarize(&catalog),
            CoverageSummary {
                known_routes: 1,
                unknown_routes: 1,
                known_packets: 1,
                unknown_packets: 1,
            }
        );
        assert_eq!(report.route(&unknown).unwrap().first_sequence, 2);
    }

    #[test]
    fn coverage_recovers_fragment_identity_from_legacy_wire_bytes() {
        let mut legacy = record(1, None, 10);
        let CaptureRecordKind::Packet(packet) = &mut legacy.kind else {
            unreachable!();
        };
        packet.payload.wire_bytes[4..6].copy_from_slice(&8u16.to_be_bytes());

        let mut report = CoverageReport::default();
        report.observe(&legacy);

        assert_eq!(
            report
                .fragments()
                .get(&(PacketDirection::Unknown, FragmentKind::Unknown(8)))
                .unwrap()
                .packet_count,
            1
        );
        assert_eq!(report.unclassified_fragment_packet_count, 0);
    }
}
