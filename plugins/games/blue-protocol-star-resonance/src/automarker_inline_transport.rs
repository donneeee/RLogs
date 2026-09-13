//! Offline IPv4/TCP adapter for a future Windows automarker interception boundary.
//!
//! This module deliberately owns no WinDivert/WFP handle and exposes no receive,
//! block, inject, or send operation.  It accepts copied network-layer packets,
//! binds them to an exact process-owned four-tuple and connection epoch, delegates
//! payload edits to the exact-frame TCP rewrite ledger, and returns another owned
//! copy.  It is therefore usable for deterministic adapter tests without making
//! the product capable of altering live traffic.

use std::net::Ipv4Addr;

use crate::{
    OfflineAutomarkerTcpRewriteLedger, OfflineAutomarkerTcpSegmentReason,
    OfflineAutomarkerTcpSegmentResult,
};

const IPV4_MIN_HEADER_BYTES: usize = 20;
const TCP_MIN_HEADER_BYTES: usize = 20;
const TCP_PROTOCOL: u8 = 6;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct AutomarkerIpv4Endpoint {
    pub address: Ipv4Addr,
    pub port: u16,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct AutomarkerOwnedTcpConnection {
    pub process_id: u32,
    pub local: AutomarkerIpv4Endpoint,
    pub remote: AutomarkerIpv4Endpoint,
}

/// Opaque proof that one connection epoch was observed from SYN and matched
/// exactly one socket owned by the requested game process.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OfflineAutomarkerConnectionEpochBinding {
    process_id: u32,
    connection: AutomarkerOwnedTcpConnection,
    epoch: u64,
}

impl OfflineAutomarkerConnectionEpochBinding {
    pub fn process_id(&self) -> u32 {
        self.process_id
    }

    pub fn connection_epoch(&self) -> u64 {
        self.epoch
    }

    pub fn connection(&self) -> AutomarkerOwnedTcpConnection {
        self.connection
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OfflineAutomarkerBindingError {
    InvalidProcessId,
    SynNotObserved,
    NoExactOwnedSocket,
    AmbiguousOwnedSocket,
}

/// Creates an offline epoch binding only from an exact process-id and
/// four-tuple match. A process name, executable path, protocol signature, or
/// mirrored observation is insufficient.
pub fn bind_offline_automarker_connection_epoch(
    process_id: u32,
    connection: AutomarkerOwnedTcpConnection,
    epoch: u64,
    syn_observed: bool,
    owned_socket_snapshot: &[AutomarkerOwnedTcpConnection],
) -> Result<OfflineAutomarkerConnectionEpochBinding, OfflineAutomarkerBindingError> {
    if process_id == 0 {
        return Err(OfflineAutomarkerBindingError::InvalidProcessId);
    }
    if !syn_observed {
        return Err(OfflineAutomarkerBindingError::SynNotObserved);
    }
    let matches = owned_socket_snapshot
        .iter()
        .filter(|candidate| **candidate == connection && candidate.process_id == process_id)
        .count();
    match matches {
        0 => Err(OfflineAutomarkerBindingError::NoExactOwnedSocket),
        1 => Ok(OfflineAutomarkerConnectionEpochBinding {
            process_id,
            connection,
            epoch,
        }),
        _ => Err(OfflineAutomarkerBindingError::AmbiguousOwnedSocket),
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OfflineAutomarkerIpv4TcpReason {
    EpochMismatch { expected: u64, actual: u64 },
    PacketTooShort,
    NotIpv4,
    InvalidIpv4Header,
    LengthMismatch,
    FragmentedIpv4,
    NotTcp,
    InvalidTcpHeader,
    NotExactOutboundConnection,
    InvalidIpv4Checksum,
    InvalidTcpChecksum,
    Ledger(OfflineAutomarkerTcpSegmentReason),
    ConservationFailure,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OfflineAutomarkerIpv4TcpProof {
    pub packet_length_bytes: usize,
    pub payload_length_bytes: usize,
    pub overlapped_payload_bytes: usize,
    pub changed_payload_bytes: usize,
    pub operations_touched: usize,
    pub packet_length_preserved: bool,
    pub ipv4_header_preserved: bool,
    pub tcp_header_preserved_except_checksum: bool,
    pub ipv4_checksum_valid: bool,
    pub tcp_checksum_recalculated_and_valid: bool,
    pub live_interception_performed: bool,
    pub packet_transmission_performed: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OfflineAutomarkerIpv4TcpResult {
    Rewritten {
        packet: Vec<u8>,
        proof: OfflineAutomarkerIpv4TcpProof,
    },
    OriginalUnchanged {
        packet: Vec<u8>,
        reason: OfflineAutomarkerIpv4TcpReason,
    },
}

impl OfflineAutomarkerIpv4TcpResult {
    pub fn packet(&self) -> &[u8] {
        match self {
            Self::Rewritten { packet, .. } | Self::OriginalUnchanged { packet, .. } => packet,
        }
    }
}

/// Pure copied-packet adapter corresponding to WinDivert's NETWORK-layer IPv4
/// packet shape. No driver dependency or operating-system handle is present.
pub struct OfflineAutomarkerIpv4TcpAdapter {
    binding: OfflineAutomarkerConnectionEpochBinding,
    ledger: OfflineAutomarkerTcpRewriteLedger,
}

impl OfflineAutomarkerIpv4TcpAdapter {
    pub fn new(binding: OfflineAutomarkerConnectionEpochBinding) -> Self {
        Self {
            ledger: OfflineAutomarkerTcpRewriteLedger::new(binding.connection_epoch()),
            binding,
        }
    }

    pub fn ledger_mut(&mut self) -> &mut OfflineAutomarkerTcpRewriteLedger {
        &mut self.ledger
    }

    /// Rewrites a copied, complete IPv4/TCP packet. Every rejection returns the
    /// original packet byte-for-byte. IPv4 fragments and checksum-offload
    /// ambiguity are rejected rather than inferred.
    pub fn rewrite_copied_outbound_packet(
        &mut self,
        connection_epoch: u64,
        packet: &[u8],
    ) -> OfflineAutomarkerIpv4TcpResult {
        let unchanged = |reason| OfflineAutomarkerIpv4TcpResult::OriginalUnchanged {
            packet: packet.to_vec(),
            reason,
        };
        if connection_epoch != self.binding.epoch {
            return unchanged(OfflineAutomarkerIpv4TcpReason::EpochMismatch {
                expected: self.binding.epoch,
                actual: connection_epoch,
            });
        }
        let layout = match parse_ipv4_tcp(packet) {
            Ok(layout) => layout,
            Err(reason) => return unchanged(reason),
        };
        if layout.source != self.binding.connection.local
            || layout.destination != self.binding.connection.remote
        {
            return unchanged(OfflineAutomarkerIpv4TcpReason::NotExactOutboundConnection);
        }
        if checksum(&packet[..layout.ip_header_len]) != 0xffff {
            return unchanged(OfflineAutomarkerIpv4TcpReason::InvalidIpv4Checksum);
        }
        if tcp_checksum(packet, &layout) != 0xffff {
            return unchanged(OfflineAutomarkerIpv4TcpReason::InvalidTcpChecksum);
        }

        let payload = &packet[layout.payload_start..];
        let rewritten =
            match self
                .ledger
                .rewrite_segment(connection_epoch, layout.sequence, payload)
            {
                OfflineAutomarkerTcpSegmentResult::Rewritten {
                    payload,
                    overlapped_bytes,
                    changed_bytes,
                    operations_touched,
                } => (payload, overlapped_bytes, changed_bytes, operations_touched),
                OfflineAutomarkerTcpSegmentResult::OriginalUnchanged { reason, .. } => {
                    return unchanged(OfflineAutomarkerIpv4TcpReason::Ledger(reason));
                }
            };

        let mut output = packet.to_vec();
        output[layout.payload_start..].copy_from_slice(&rewritten.0);
        output[layout.tcp_start + 16] = 0;
        output[layout.tcp_start + 17] = 0;
        let tcp_sum = !tcp_checksum(&output, &layout);
        output[layout.tcp_start + 16..layout.tcp_start + 18]
            .copy_from_slice(&tcp_sum.to_be_bytes());

        let ipv4_preserved = output[..layout.ip_header_len] == packet[..layout.ip_header_len];
        let tcp_header_preserved = (0..layout.tcp_header_len).all(|offset| {
            matches!(offset, 16 | 17)
                || output[layout.tcp_start + offset] == packet[layout.tcp_start + offset]
        });
        if output.len() != packet.len()
            || !ipv4_preserved
            || !tcp_header_preserved
            || checksum(&output[..layout.ip_header_len]) != 0xffff
            || tcp_checksum(&output, &layout) != 0xffff
        {
            return unchanged(OfflineAutomarkerIpv4TcpReason::ConservationFailure);
        }
        OfflineAutomarkerIpv4TcpResult::Rewritten {
            packet: output,
            proof: OfflineAutomarkerIpv4TcpProof {
                packet_length_bytes: packet.len(),
                payload_length_bytes: payload.len(),
                overlapped_payload_bytes: rewritten.1,
                changed_payload_bytes: rewritten.2,
                operations_touched: rewritten.3,
                packet_length_preserved: true,
                ipv4_header_preserved: true,
                tcp_header_preserved_except_checksum: true,
                ipv4_checksum_valid: true,
                tcp_checksum_recalculated_and_valid: true,
                live_interception_performed: false,
                packet_transmission_performed: false,
            },
        }
    }
}

#[derive(Debug, Clone, Copy)]
struct Ipv4TcpLayout {
    source: AutomarkerIpv4Endpoint,
    destination: AutomarkerIpv4Endpoint,
    sequence: u32,
    ip_header_len: usize,
    tcp_start: usize,
    tcp_header_len: usize,
    payload_start: usize,
}

fn parse_ipv4_tcp(packet: &[u8]) -> Result<Ipv4TcpLayout, OfflineAutomarkerIpv4TcpReason> {
    if packet.len() < IPV4_MIN_HEADER_BYTES {
        return Err(OfflineAutomarkerIpv4TcpReason::PacketTooShort);
    }
    if packet[0] >> 4 != 4 {
        return Err(OfflineAutomarkerIpv4TcpReason::NotIpv4);
    }
    let ip_header_len = usize::from(packet[0] & 0x0f) * 4;
    if ip_header_len < IPV4_MIN_HEADER_BYTES || packet.len() < ip_header_len + TCP_MIN_HEADER_BYTES
    {
        return Err(OfflineAutomarkerIpv4TcpReason::InvalidIpv4Header);
    }
    let total_length = usize::from(u16::from_be_bytes([packet[2], packet[3]]));
    if total_length != packet.len() {
        return Err(OfflineAutomarkerIpv4TcpReason::LengthMismatch);
    }
    let fragmentation = u16::from_be_bytes([packet[6], packet[7]]);
    if fragmentation & 0x3fff != 0 {
        return Err(OfflineAutomarkerIpv4TcpReason::FragmentedIpv4);
    }
    if packet[9] != TCP_PROTOCOL {
        return Err(OfflineAutomarkerIpv4TcpReason::NotTcp);
    }
    let tcp_start = ip_header_len;
    let tcp_header_len = usize::from(packet[tcp_start + 12] >> 4) * 4;
    if tcp_header_len < TCP_MIN_HEADER_BYTES || packet.len() < tcp_start + tcp_header_len {
        return Err(OfflineAutomarkerIpv4TcpReason::InvalidTcpHeader);
    }
    Ok(Ipv4TcpLayout {
        source: AutomarkerIpv4Endpoint {
            address: Ipv4Addr::new(packet[12], packet[13], packet[14], packet[15]),
            port: u16::from_be_bytes([packet[tcp_start], packet[tcp_start + 1]]),
        },
        destination: AutomarkerIpv4Endpoint {
            address: Ipv4Addr::new(packet[16], packet[17], packet[18], packet[19]),
            port: u16::from_be_bytes([packet[tcp_start + 2], packet[tcp_start + 3]]),
        },
        sequence: u32::from_be_bytes(
            packet[tcp_start + 4..tcp_start + 8]
                .try_into()
                .expect("validated TCP sequence"),
        ),
        ip_header_len,
        tcp_start,
        tcp_header_len,
        payload_start: tcp_start + tcp_header_len,
    })
}

fn checksum(bytes: &[u8]) -> u16 {
    let mut sum = 0_u32;
    for pair in bytes.chunks(2) {
        let word = if pair.len() == 2 {
            u16::from_be_bytes([pair[0], pair[1]])
        } else {
            u16::from_be_bytes([pair[0], 0])
        };
        sum += u32::from(word);
        while sum > 0xffff {
            sum = (sum & 0xffff) + (sum >> 16);
        }
    }
    sum as u16
}

fn tcp_checksum(packet: &[u8], layout: &Ipv4TcpLayout) -> u16 {
    let tcp_length = packet.len() - layout.tcp_start;
    let mut bytes = Vec::with_capacity(12 + tcp_length + 1);
    bytes.extend_from_slice(&packet[12..20]);
    bytes.push(0);
    bytes.push(TCP_PROTOCOL);
    bytes.extend_from_slice(&(tcp_length as u16).to_be_bytes());
    bytes.extend_from_slice(&packet[layout.tcp_start..]);
    checksum(&bytes)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        AutomarkerRequestXyz, OfflineAutomarkerTcpArmResult, ProtocolPack,
        substitute_offline_automarker_frame,
    };

    fn connection() -> AutomarkerOwnedTcpConnection {
        AutomarkerOwnedTcpConnection {
            process_id: 42,
            local: AutomarkerIpv4Endpoint {
                address: Ipv4Addr::new(10, 0, 0, 2),
                port: 50_000,
            },
            remote: AutomarkerIpv4Endpoint {
                address: Ipv4Addr::new(203, 0, 113, 7),
                port: 44_321,
            },
        }
    }

    fn binding() -> OfflineAutomarkerConnectionEpochBinding {
        bind_offline_automarker_connection_epoch(42, connection(), 7, true, &[connection()])
            .unwrap()
    }

    fn packet(sequence: u32, payload: &[u8]) -> Vec<u8> {
        let connection = connection();
        let mut packet = vec![0_u8; 40 + payload.len()];
        packet[0] = 0x45;
        let packet_len = packet.len() as u16;
        packet[2..4].copy_from_slice(&packet_len.to_be_bytes());
        packet[6..8].copy_from_slice(&0x4000_u16.to_be_bytes());
        packet[8] = 64;
        packet[9] = TCP_PROTOCOL;
        packet[12..16].copy_from_slice(&connection.local.address.octets());
        packet[16..20].copy_from_slice(&connection.remote.address.octets());
        packet[20..22].copy_from_slice(&connection.local.port.to_be_bytes());
        packet[22..24].copy_from_slice(&connection.remote.port.to_be_bytes());
        packet[24..28].copy_from_slice(&sequence.to_be_bytes());
        packet[32] = 5 << 4;
        packet[33] = 0x18;
        packet[34..36].copy_from_slice(&65_535_u16.to_be_bytes());
        packet[40..].copy_from_slice(payload);
        let ip_sum = !checksum(&packet[..20]);
        packet[10..12].copy_from_slice(&ip_sum.to_be_bytes());
        let layout = parse_ipv4_tcp(&packet).unwrap();
        let tcp_sum = !tcp_checksum(&packet, &layout);
        packet[36..38].copy_from_slice(&tcp_sum.to_be_bytes());
        packet
    }

    fn pack() -> ProtocolPack {
        let source = ProtocolPack::from_json(include_bytes!(
            "../protocol-packs/global/steam-24687926/pack.json"
        ))
        .unwrap();
        crate::compatibility_epoch::retarget_protocol_pack(
            &source,
            "compatibility-fallback",
            "global",
            "steam",
            crate::AUTOMARKER_REQUEST_BUILD,
        )
        .unwrap()
    }

    fn frame() -> Vec<u8> {
        // Reuse the exact synthetic-frame constructor already guarded by the
        // substitution core rather than embedding captured traffic here.
        let original = crate::automarker_request::tests_support::synthetic_frame_for_adapter();
        assert!(
            substitute_offline_automarker_frame(
                &pack(),
                &original,
                2,
                AutomarkerRequestXyz {
                    x: 4.0,
                    y: 5.0,
                    z: 6.0
                }
            )
            .was_substituted()
        );
        original
    }

    #[test]
    fn binding_requires_unique_exact_process_owned_tuple_and_syn() {
        assert_eq!(
            bind_offline_automarker_connection_epoch(42, connection(), 7, false, &[connection()]),
            Err(OfflineAutomarkerBindingError::SynNotObserved)
        );
        assert_eq!(
            bind_offline_automarker_connection_epoch(41, connection(), 7, true, &[connection()]),
            Err(OfflineAutomarkerBindingError::NoExactOwnedSocket)
        );
        assert_eq!(
            bind_offline_automarker_connection_epoch(
                42,
                connection(),
                7,
                true,
                &[connection(), connection()]
            ),
            Err(OfflineAutomarkerBindingError::AmbiguousOwnedSocket)
        );
    }

    #[test]
    fn copied_packet_rewrite_preserves_headers_and_repairs_tcp_checksum() {
        let frame = frame();
        let sequence = 1000;
        let mut adapter = OfflineAutomarkerIpv4TcpAdapter::new(binding());
        assert!(matches!(
            adapter.ledger_mut().arm_frame(
                7,
                sequence,
                &pack(),
                &frame,
                6,
                AutomarkerRequestXyz {
                    x: 11.0,
                    y: 12.0,
                    z: 13.0
                }
            ),
            OfflineAutomarkerTcpArmResult::Armed { .. }
        ));
        let original = packet(sequence, &frame);
        let result = adapter.rewrite_copied_outbound_packet(7, &original);
        let OfflineAutomarkerIpv4TcpResult::Rewritten { packet, proof } = result else {
            panic!("expected rewritten packet")
        };
        assert_eq!(packet.len(), original.len());
        assert_eq!(&packet[..20], &original[..20]);
        assert_eq!(&packet[20..36], &original[20..36]);
        assert_eq!(&packet[38..40], &original[38..40]);
        assert_ne!(&packet[36..38], &original[36..38]);
        assert!(proof.tcp_checksum_recalculated_and_valid);
        assert!(!proof.live_interception_performed);
        assert!(!proof.packet_transmission_performed);
    }

    #[test]
    fn segmented_and_retransmitted_packets_receive_the_same_registered_replacement() {
        let frame = frame();
        let replacement = match substitute_offline_automarker_frame(
            &pack(),
            &frame,
            5,
            AutomarkerRequestXyz {
                x: -101.25,
                y: 202.5,
                z: -303.75,
            },
        ) {
            crate::OfflineAutomarkerFrameSubstitution::Substituted { frame, .. } => frame,
            outcome => panic!("exact test frame rejected: {outcome:?}"),
        };
        let sequence = u32::MAX - 90;
        let split = 113;
        let mut adapter = OfflineAutomarkerIpv4TcpAdapter::new(binding());
        assert!(matches!(
            adapter.ledger_mut().arm_frame(
                7,
                sequence,
                &pack(),
                &frame,
                5,
                AutomarkerRequestXyz {
                    x: -101.25,
                    y: 202.5,
                    z: -303.75,
                }
            ),
            OfflineAutomarkerTcpArmResult::Armed { .. }
        ));

        let tail = packet(sequence.wrapping_add(split as u32), &frame[split..]);
        let head = packet(sequence, &frame[..split]);
        let retransmitted_head = head.clone();
        for (candidate, expected) in [
            (tail, &replacement[split..]),
            (head, &replacement[..split]),
            (retransmitted_head, &replacement[..split]),
        ] {
            let result = adapter.rewrite_copied_outbound_packet(7, &candidate);
            let OfflineAutomarkerIpv4TcpResult::Rewritten { packet, .. } = result else {
                panic!("expected rewritten packet")
            };
            assert_eq!(&packet[40..], expected);
            let layout = parse_ipv4_tcp(&packet).unwrap();
            assert_eq!(tcp_checksum(&packet, &layout), 0xffff);
        }
    }

    #[test]
    fn wrong_epoch_tuple_fragment_and_bad_checksum_fail_open() {
        let original = packet(1000, &[1, 2, 3]);
        let mut adapter = OfflineAutomarkerIpv4TcpAdapter::new(binding());
        for (candidate, epoch, expected) in [
            (
                original.clone(),
                8,
                OfflineAutomarkerIpv4TcpReason::EpochMismatch {
                    expected: 7,
                    actual: 8,
                },
            ),
            (
                {
                    let mut value = original.clone();
                    value[20..22].copy_from_slice(&49_999_u16.to_be_bytes());
                    value
                },
                7,
                OfflineAutomarkerIpv4TcpReason::NotExactOutboundConnection,
            ),
            (
                {
                    let mut value = original.clone();
                    value[6..8].copy_from_slice(&0x2000_u16.to_be_bytes());
                    value
                },
                7,
                OfflineAutomarkerIpv4TcpReason::FragmentedIpv4,
            ),
            (
                {
                    let mut value = original.clone();
                    value[40] ^= 1;
                    value
                },
                7,
                OfflineAutomarkerIpv4TcpReason::InvalidTcpChecksum,
            ),
        ] {
            let result = adapter.rewrite_copied_outbound_packet(epoch, &candidate);
            assert_eq!(result.packet(), candidate);
            assert!(matches!(
                result,
                OfflineAutomarkerIpv4TcpResult::OriginalUnchanged { reason, .. } if reason == expected
            ));
        }
    }
}
