use std::{
    collections::HashMap,
    net::{Ipv4Addr, Ipv6Addr},
};

use bytes::{Bytes, BytesMut};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(tag = "version", rename_all = "snake_case")]
pub enum IpFragmentKey {
    Ipv4 {
        source: Ipv4Addr,
        destination: Ipv4Addr,
        protocol: u8,
        identification: u16,
    },
    Ipv6 {
        source: Ipv6Addr,
        destination: Ipv6Addr,
        next_header: u8,
        identification: u32,
    },
}

impl IpFragmentKey {
    pub const fn is_ipv6(self) -> bool {
        matches!(self, Self::Ipv6 { .. })
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct IpFragment {
    pub key: IpFragmentKey,
    /// Position within the fragmentable IP payload, in bytes.
    pub offset: u32,
    pub more_fragments: bool,
    pub capture_sequence: u64,
    pub observed_micros: u64,
    /// Shared view into the captured frame.
    pub payload: Bytes,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReassembledIpDatagram {
    pub key: IpFragmentKey,
    pub completed_capture_sequence: u64,
    pub completed_observed_micros: u64,
    /// Fragmented datagrams require one exact-size coalescing allocation.
    pub payload: Bytes,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum IpFragmentDropReason {
    InvalidAlignment,
    EmptyFragment,
    DatagramTooLarge,
    ConflictingFinalLength,
    Overlap,
    FragmentLimit,
    DatagramLimit,
    MemoryLimit,
    Timeout,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct IpFragmentDrop {
    pub key: IpFragmentKey,
    pub reason: IpFragmentDropReason,
    pub discarded_fragments: u32,
    pub discarded_bytes: u64,
    pub capture_sequence: u64,
    pub observed_micros: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "data", rename_all = "snake_case")]
pub enum IpFragmentEvent {
    Datagram(ReassembledIpDatagram),
    Dropped(IpFragmentDrop),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct IpFragmentConfig {
    pub max_active_datagrams: usize,
    pub max_fragments_per_datagram: usize,
    pub max_datagram_bytes: usize,
    pub max_total_buffered_bytes: usize,
    pub timeout_micros: u64,
}

impl Default for IpFragmentConfig {
    fn default() -> Self {
        Self {
            max_active_datagrams: 1_024,
            max_fragments_per_datagram: 128,
            max_datagram_bytes: 65_535,
            max_total_buffered_bytes: 32 * 1024 * 1024,
            timeout_micros: 60 * 1_000_000,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IpFragmentConfigError {
    ZeroActiveDatagrams,
    ZeroFragmentsPerDatagram,
    ZeroDatagramBytes,
    DatagramExceedsIpMaximum,
    ZeroTotalBytes,
    DatagramExceedsTotal,
    ZeroTimeout,
}

impl std::fmt::Display for IpFragmentConfigError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(match self {
            Self::ZeroActiveDatagrams => "max_active_datagrams must be greater than zero",
            Self::ZeroFragmentsPerDatagram => {
                "max_fragments_per_datagram must be greater than zero"
            }
            Self::ZeroDatagramBytes => "max_datagram_bytes must be greater than zero",
            Self::DatagramExceedsIpMaximum => {
                "max_datagram_bytes cannot exceed the 65,535-byte IP payload limit"
            }
            Self::ZeroTotalBytes => "max_total_buffered_bytes must be greater than zero",
            Self::DatagramExceedsTotal => {
                "max_datagram_bytes cannot exceed max_total_buffered_bytes"
            }
            Self::ZeroTimeout => "fragment timeout must be greater than zero",
        })
    }
}

impl std::error::Error for IpFragmentConfigError {}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct IpFragmentMetrics {
    pub fragments_seen: u64,
    pub fragment_bytes_seen: u64,
    pub active_datagrams: u64,
    pub buffered_fragments: u64,
    pub buffered_bytes: u64,
    pub buffered_bytes_high_water: u64,
    pub datagrams_completed: u64,
    pub datagram_bytes_completed: u64,
    pub exact_duplicate_fragments: u64,
    pub datagrams_dropped: u64,
    pub fragments_discarded: u64,
    pub bytes_discarded: u64,
    pub overlap_drops: u64,
    pub limit_drops: u64,
    pub timeout_drops: u64,
}

#[derive(Debug)]
struct FragmentPiece {
    offset: usize,
    more_fragments: bool,
    payload: Bytes,
}

impl FragmentPiece {
    fn end(&self) -> usize {
        self.offset + self.payload.len()
    }
}

#[derive(Debug)]
struct DatagramState {
    pieces: Vec<FragmentPiece>,
    final_length: Option<usize>,
    buffered_bytes: usize,
    first_capture_sequence: u64,
    first_observed_micros: u64,
    last_capture_sequence: u64,
    last_observed_micros: u64,
}

impl DatagramState {
    fn new(fragment: &IpFragment) -> Self {
        Self {
            pieces: Vec::new(),
            final_length: None,
            buffered_bytes: 0,
            first_capture_sequence: fragment.capture_sequence,
            first_observed_micros: fragment.observed_micros,
            last_capture_sequence: fragment.capture_sequence,
            last_observed_micros: fragment.observed_micros,
        }
    }
}

#[derive(Debug)]
pub struct IpFragmentReassembler {
    config: IpFragmentConfig,
    datagrams: HashMap<IpFragmentKey, DatagramState>,
    total_buffered_fragments: usize,
    total_buffered_bytes: usize,
    metrics: IpFragmentMetrics,
}

impl Default for IpFragmentReassembler {
    fn default() -> Self {
        Self::new()
    }
}

impl IpFragmentReassembler {
    pub fn new() -> Self {
        Self::try_with_config(IpFragmentConfig::default())
            .expect("the built-in IP fragment configuration is valid")
    }

    pub fn try_with_config(config: IpFragmentConfig) -> Result<Self, IpFragmentConfigError> {
        validate_config(config)?;
        Ok(Self {
            config,
            datagrams: HashMap::new(),
            total_buffered_fragments: 0,
            total_buffered_bytes: 0,
            metrics: IpFragmentMetrics::default(),
        })
    }

    pub fn config(&self) -> IpFragmentConfig {
        self.config
    }

    pub fn metrics(&self) -> &IpFragmentMetrics {
        &self.metrics
    }

    pub fn process(&mut self, fragment: IpFragment, mut emit: impl FnMut(IpFragmentEvent)) {
        self.metrics.fragments_seen = self.metrics.fragments_seen.saturating_add(1);
        self.metrics.fragment_bytes_seen = self
            .metrics
            .fragment_bytes_seen
            .saturating_add(fragment.payload.len() as u64);

        let Some((offset, end)) = validate_fragment(&fragment, self.config) else {
            let reason = invalid_reason(&fragment, self.config);
            self.drop_with_incoming(fragment, reason, &mut emit);
            return;
        };

        if !self.datagrams.contains_key(&fragment.key)
            && self.datagrams.len() >= self.config.max_active_datagrams
        {
            self.evict_oldest(
                None,
                IpFragmentDropReason::DatagramLimit,
                fragment.capture_sequence,
                fragment.observed_micros,
                &mut emit,
            );
        }

        while self
            .total_buffered_bytes
            .saturating_add(fragment.payload.len())
            > self.config.max_total_buffered_bytes
        {
            if !self.evict_oldest(
                Some(fragment.key),
                IpFragmentDropReason::MemoryLimit,
                fragment.capture_sequence,
                fragment.observed_micros,
                &mut emit,
            ) {
                self.drop_with_incoming(fragment, IpFragmentDropReason::MemoryLimit, &mut emit);
                return;
            }
        }

        let disposition = {
            let state = self
                .datagrams
                .entry(fragment.key)
                .or_insert_with(|| DatagramState::new(&fragment));

            match overlap_disposition(
                fragment.key,
                &state.pieces,
                offset,
                end,
                fragment.more_fragments,
                &fragment.payload,
            ) {
                OverlapDisposition::ExactDuplicate => FragmentDisposition::ExactDuplicate,
                OverlapDisposition::Reject => {
                    FragmentDisposition::Reject(IpFragmentDropReason::Overlap)
                }
                OverlapDisposition::Distinct => {
                    if state.pieces.len() >= self.config.max_fragments_per_datagram {
                        FragmentDisposition::Reject(IpFragmentDropReason::FragmentLimit)
                    } else {
                        let conflicting_final = if fragment.more_fragments {
                            state
                                .final_length
                                .is_some_and(|final_length| end >= final_length)
                        } else {
                            state.final_length.is_some_and(|existing| existing != end)
                                || state.pieces.iter().any(|piece| piece.end() > end)
                        };
                        if conflicting_final {
                            FragmentDisposition::Reject(
                                IpFragmentDropReason::ConflictingFinalLength,
                            )
                        } else {
                            FragmentDisposition::Insert
                        }
                    }
                }
            }
        };

        match disposition {
            FragmentDisposition::ExactDuplicate => {
                self.metrics.exact_duplicate_fragments =
                    self.metrics.exact_duplicate_fragments.saturating_add(1);
                return;
            }
            FragmentDisposition::Reject(reason) => {
                self.drop_with_incoming(fragment, reason, &mut emit);
                return;
            }
            FragmentDisposition::Insert => {}
        }

        let payload_len = fragment.payload.len();
        let key = fragment.key;
        let complete = {
            let state = self
                .datagrams
                .get_mut(&key)
                .expect("datagram state was inserted");
            if !fragment.more_fragments {
                state.final_length = Some(end);
            }
            state.last_capture_sequence = fragment.capture_sequence;
            state.last_observed_micros = fragment.observed_micros;
            state.buffered_bytes = state.buffered_bytes.saturating_add(payload_len);
            let insertion = state.pieces.partition_point(|piece| piece.offset < offset);
            state.pieces.insert(
                insertion,
                FragmentPiece {
                    offset,
                    more_fragments: fragment.more_fragments,
                    payload: fragment.payload,
                },
            );
            datagram_complete(state)
        };
        self.total_buffered_fragments = self.total_buffered_fragments.saturating_add(1);
        self.total_buffered_bytes = self.total_buffered_bytes.saturating_add(payload_len);
        self.update_gauges();

        if complete {
            self.complete(key, &mut emit);
        }
    }

    pub fn expire(&mut self, observed_micros: u64, mut emit: impl FnMut(IpFragmentEvent)) {
        let expired: Vec<_> = self
            .datagrams
            .iter()
            .filter_map(|(key, state)| {
                (observed_micros.saturating_sub(state.first_observed_micros)
                    >= self.config.timeout_micros)
                    .then_some(*key)
            })
            .collect();

        for key in expired {
            self.drop_state(
                key,
                IpFragmentDropReason::Timeout,
                0,
                observed_micros,
                &mut emit,
            );
        }
    }

    fn complete(&mut self, key: IpFragmentKey, emit: &mut impl FnMut(IpFragmentEvent)) {
        let state = self
            .datagrams
            .remove(&key)
            .expect("completed datagram exists");
        let final_length = state
            .final_length
            .expect("completed datagram has final length");
        let mut payload = BytesMut::with_capacity(final_length);
        let fragment_count = state.pieces.len();
        for piece in state.pieces {
            payload.extend_from_slice(&piece.payload);
        }

        self.total_buffered_bytes = self
            .total_buffered_bytes
            .saturating_sub(state.buffered_bytes);
        self.total_buffered_fragments =
            self.total_buffered_fragments.saturating_sub(fragment_count);
        self.metrics.datagrams_completed = self.metrics.datagrams_completed.saturating_add(1);
        self.metrics.datagram_bytes_completed = self
            .metrics
            .datagram_bytes_completed
            .saturating_add(final_length as u64);
        self.update_gauges();

        emit(IpFragmentEvent::Datagram(ReassembledIpDatagram {
            key,
            completed_capture_sequence: state.last_capture_sequence,
            completed_observed_micros: state.last_observed_micros,
            payload: payload.freeze(),
        }));
    }

    fn drop_with_incoming(
        &mut self,
        fragment: IpFragment,
        reason: IpFragmentDropReason,
        emit: &mut impl FnMut(IpFragmentEvent),
    ) {
        let mut discarded_fragments = 1usize;
        let mut discarded_bytes = fragment.payload.len();
        let mut capture_sequence = fragment.capture_sequence;
        let mut observed_micros = fragment.observed_micros;
        if let Some(state) = self.datagrams.remove(&fragment.key) {
            self.total_buffered_bytes = self
                .total_buffered_bytes
                .saturating_sub(state.buffered_bytes);
            self.total_buffered_fragments = self
                .total_buffered_fragments
                .saturating_sub(state.pieces.len());
            discarded_fragments = discarded_fragments.saturating_add(state.pieces.len());
            discarded_bytes = discarded_bytes.saturating_add(state.buffered_bytes);
            capture_sequence = capture_sequence.max(state.last_capture_sequence);
            observed_micros = observed_micros.max(state.last_observed_micros);
        }
        self.observe_drop(reason, discarded_fragments, discarded_bytes);
        self.update_gauges();
        emit(IpFragmentEvent::Dropped(IpFragmentDrop {
            key: fragment.key,
            reason,
            discarded_fragments: discarded_fragments as u32,
            discarded_bytes: discarded_bytes as u64,
            capture_sequence,
            observed_micros,
        }));
    }

    fn drop_state(
        &mut self,
        key: IpFragmentKey,
        reason: IpFragmentDropReason,
        capture_sequence: u64,
        observed_micros: u64,
        emit: &mut impl FnMut(IpFragmentEvent),
    ) {
        let Some(state) = self.datagrams.remove(&key) else {
            return;
        };
        self.total_buffered_bytes = self
            .total_buffered_bytes
            .saturating_sub(state.buffered_bytes);
        self.total_buffered_fragments = self
            .total_buffered_fragments
            .saturating_sub(state.pieces.len());
        self.observe_drop(reason, state.pieces.len(), state.buffered_bytes);
        self.update_gauges();
        emit(IpFragmentEvent::Dropped(IpFragmentDrop {
            key,
            reason,
            discarded_fragments: state.pieces.len() as u32,
            discarded_bytes: state.buffered_bytes as u64,
            capture_sequence: capture_sequence.max(state.last_capture_sequence),
            observed_micros: observed_micros.max(state.last_observed_micros),
        }));
    }

    fn evict_oldest(
        &mut self,
        except: Option<IpFragmentKey>,
        reason: IpFragmentDropReason,
        capture_sequence: u64,
        observed_micros: u64,
        emit: &mut impl FnMut(IpFragmentEvent),
    ) -> bool {
        let oldest = self
            .datagrams
            .iter()
            .filter(|(key, _)| except != Some(**key))
            .min_by_key(|(key, state)| {
                (
                    state.first_observed_micros,
                    state.first_capture_sequence,
                    **key,
                )
            })
            .map(|(key, _)| *key);
        let Some(key) = oldest else {
            return false;
        };
        self.drop_state(key, reason, capture_sequence, observed_micros, emit);
        true
    }

    fn observe_drop(&mut self, reason: IpFragmentDropReason, fragments: usize, bytes: usize) {
        self.metrics.datagrams_dropped = self.metrics.datagrams_dropped.saturating_add(1);
        self.metrics.fragments_discarded = self
            .metrics
            .fragments_discarded
            .saturating_add(fragments as u64);
        self.metrics.bytes_discarded = self.metrics.bytes_discarded.saturating_add(bytes as u64);
        match reason {
            IpFragmentDropReason::Overlap => {
                self.metrics.overlap_drops = self.metrics.overlap_drops.saturating_add(1);
            }
            IpFragmentDropReason::DatagramLimit
            | IpFragmentDropReason::MemoryLimit
            | IpFragmentDropReason::FragmentLimit
            | IpFragmentDropReason::DatagramTooLarge => {
                self.metrics.limit_drops = self.metrics.limit_drops.saturating_add(1);
            }
            IpFragmentDropReason::Timeout => {
                self.metrics.timeout_drops = self.metrics.timeout_drops.saturating_add(1);
            }
            IpFragmentDropReason::InvalidAlignment
            | IpFragmentDropReason::EmptyFragment
            | IpFragmentDropReason::ConflictingFinalLength => {}
        }
    }

    fn update_gauges(&mut self) {
        self.metrics.active_datagrams = self.datagrams.len() as u64;
        self.metrics.buffered_fragments = self.total_buffered_fragments as u64;
        self.metrics.buffered_bytes = self.total_buffered_bytes as u64;
        self.metrics.buffered_bytes_high_water = self
            .metrics
            .buffered_bytes_high_water
            .max(self.metrics.buffered_bytes);
    }
}

fn validate_config(config: IpFragmentConfig) -> Result<(), IpFragmentConfigError> {
    if config.max_active_datagrams == 0 {
        return Err(IpFragmentConfigError::ZeroActiveDatagrams);
    }
    if config.max_fragments_per_datagram == 0 {
        return Err(IpFragmentConfigError::ZeroFragmentsPerDatagram);
    }
    if config.max_datagram_bytes == 0 {
        return Err(IpFragmentConfigError::ZeroDatagramBytes);
    }
    if config.max_datagram_bytes > 65_535 {
        return Err(IpFragmentConfigError::DatagramExceedsIpMaximum);
    }
    if config.max_total_buffered_bytes == 0 {
        return Err(IpFragmentConfigError::ZeroTotalBytes);
    }
    if config.max_datagram_bytes > config.max_total_buffered_bytes {
        return Err(IpFragmentConfigError::DatagramExceedsTotal);
    }
    if config.timeout_micros == 0 {
        return Err(IpFragmentConfigError::ZeroTimeout);
    }
    Ok(())
}

fn validate_fragment(fragment: &IpFragment, config: IpFragmentConfig) -> Option<(usize, usize)> {
    if fragment.payload.is_empty() {
        return None;
    }
    if fragment.offset % 8 != 0 || (fragment.more_fragments && fragment.payload.len() % 8 != 0) {
        return None;
    }
    let offset = usize::try_from(fragment.offset).ok()?;
    let end = offset.checked_add(fragment.payload.len())?;
    (end <= config.max_datagram_bytes).then_some((offset, end))
}

fn invalid_reason(fragment: &IpFragment, config: IpFragmentConfig) -> IpFragmentDropReason {
    if fragment.payload.is_empty() {
        IpFragmentDropReason::EmptyFragment
    } else if fragment.offset % 8 != 0
        || (fragment.more_fragments && fragment.payload.len() % 8 != 0)
    {
        IpFragmentDropReason::InvalidAlignment
    } else {
        debug_assert!(
            usize::try_from(fragment.offset)
                .ok()
                .and_then(|offset| offset.checked_add(fragment.payload.len()))
                .is_none_or(|end| end > config.max_datagram_bytes)
        );
        IpFragmentDropReason::DatagramTooLarge
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum OverlapDisposition {
    Distinct,
    ExactDuplicate,
    Reject,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum FragmentDisposition {
    Insert,
    ExactDuplicate,
    Reject(IpFragmentDropReason),
}

fn overlap_disposition(
    key: IpFragmentKey,
    pieces: &[FragmentPiece],
    offset: usize,
    end: usize,
    more_fragments: bool,
    payload: &[u8],
) -> OverlapDisposition {
    for piece in pieces {
        if offset < piece.end() && piece.offset < end {
            let exact_duplicate = offset == piece.offset
                && end == piece.end()
                && more_fragments == piece.more_fragments
                && payload == piece.payload;
            if exact_duplicate && !key.is_ipv6() {
                return OverlapDisposition::ExactDuplicate;
            }
            return OverlapDisposition::Reject;
        }
    }
    OverlapDisposition::Distinct
}

fn datagram_complete(state: &DatagramState) -> bool {
    let Some(final_length) = state.final_length else {
        return false;
    };
    let mut next = 0usize;
    for piece in &state.pieces {
        if piece.offset != next {
            return false;
        }
        next = piece.end();
    }
    next == final_length
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key_v4() -> IpFragmentKey {
        IpFragmentKey::Ipv4 {
            source: Ipv4Addr::new(10, 0, 0, 1),
            destination: Ipv4Addr::new(10, 0, 0, 2),
            protocol: 6,
            identification: 7,
        }
    }

    fn key_v6() -> IpFragmentKey {
        IpFragmentKey::Ipv6 {
            source: Ipv6Addr::LOCALHOST,
            destination: Ipv6Addr::UNSPECIFIED,
            next_header: 6,
            identification: 7,
        }
    }

    fn fragment(
        key: IpFragmentKey,
        offset: u32,
        more_fragments: bool,
        payload: &'static [u8],
        sequence: u64,
    ) -> IpFragment {
        IpFragment {
            key,
            offset,
            more_fragments,
            capture_sequence: sequence,
            observed_micros: sequence,
            payload: Bytes::from_static(payload),
        }
    }

    #[test]
    fn out_of_order_ipv4_fragments_coalesce_once_complete() {
        let mut reassembler = IpFragmentReassembler::new();
        let mut events = Vec::new();

        reassembler.process(fragment(key_v4(), 8, false, b"ijkl", 1), |event| {
            events.push(event)
        });
        reassembler.process(fragment(key_v4(), 0, true, b"abcdefgh", 2), |event| {
            events.push(event)
        });

        let IpFragmentEvent::Datagram(datagram) = &events[0] else {
            panic!("expected completed datagram");
        };
        assert_eq!(datagram.payload, b"abcdefghijkl".as_slice());
        assert_eq!(reassembler.metrics().datagrams_completed, 1);
        assert_eq!(reassembler.metrics().buffered_bytes, 0);
    }

    #[test]
    fn exact_ipv4_duplicates_are_ignored_without_double_counting_bytes() {
        let config = IpFragmentConfig {
            max_fragments_per_datagram: 1,
            ..IpFragmentConfig::default()
        };
        let mut reassembler = IpFragmentReassembler::try_with_config(config).unwrap();
        reassembler.process(fragment(key_v4(), 0, true, b"abcdefgh", 1), |_| {});
        reassembler.process(fragment(key_v4(), 0, true, b"abcdefgh", 2), |_| {});

        assert_eq!(reassembler.metrics().exact_duplicate_fragments, 1);
        assert_eq!(reassembler.metrics().buffered_fragments, 1);
        assert_eq!(reassembler.metrics().buffered_bytes, 8);
    }

    #[test]
    fn same_ipv4_range_with_different_final_flag_is_not_a_duplicate() {
        let mut reassembler = IpFragmentReassembler::new();
        let mut events = Vec::new();
        reassembler.process(fragment(key_v4(), 0, true, b"abcdefgh", 1), |_| {});
        reassembler.process(fragment(key_v4(), 0, false, b"abcdefgh", 2), |event| {
            events.push(event)
        });

        assert!(matches!(
            events.as_slice(),
            [IpFragmentEvent::Dropped(IpFragmentDrop {
                reason: IpFragmentDropReason::Overlap,
                discarded_fragments: 2,
                ..
            })]
        ));
        assert_eq!(reassembler.metrics().exact_duplicate_fragments, 0);
    }

    #[test]
    fn ipv6_overlap_discards_the_entire_datagram() {
        let mut reassembler = IpFragmentReassembler::new();
        let mut events = Vec::new();
        reassembler.process(fragment(key_v6(), 0, true, b"abcdefgh", 1), |event| {
            events.push(event)
        });
        reassembler.process(fragment(key_v6(), 0, true, b"abcdefgh", 2), |event| {
            events.push(event)
        });

        assert!(matches!(
            events.as_slice(),
            [IpFragmentEvent::Dropped(IpFragmentDrop {
                reason: IpFragmentDropReason::Overlap,
                ..
            })]
        ));
        assert_eq!(reassembler.metrics().active_datagrams, 0);
        assert_eq!(reassembler.metrics().overlap_drops, 1);
    }

    #[test]
    fn nonfinal_fragments_must_end_on_an_eight_byte_boundary() {
        let mut reassembler = IpFragmentReassembler::new();
        let mut events = Vec::new();
        reassembler.process(fragment(key_v4(), 0, true, b"seven!!", 1), |event| {
            events.push(event)
        });

        assert!(matches!(
            events.as_slice(),
            [IpFragmentEvent::Dropped(IpFragmentDrop {
                reason: IpFragmentDropReason::InvalidAlignment,
                ..
            })]
        ));
    }

    #[test]
    fn every_fragment_offset_must_be_on_an_eight_byte_boundary() {
        let mut reassembler = IpFragmentReassembler::new();
        let mut events = Vec::new();
        reassembler.process(fragment(key_v4(), 1, false, b"final", 1), |event| {
            events.push(event)
        });

        assert!(matches!(
            events.as_slice(),
            [IpFragmentEvent::Dropped(IpFragmentDrop {
                reason: IpFragmentDropReason::InvalidAlignment,
                ..
            })]
        ));
    }

    #[test]
    fn expiration_releases_partial_datagrams_with_evidence() {
        let config = IpFragmentConfig {
            timeout_micros: 10,
            ..IpFragmentConfig::default()
        };
        let mut reassembler = IpFragmentReassembler::try_with_config(config).unwrap();
        let mut events = Vec::new();
        reassembler.process(fragment(key_v4(), 0, true, b"abcdefgh", 1), |_| {});
        reassembler.expire(11, |event| events.push(event));

        assert!(matches!(
            events.as_slice(),
            [IpFragmentEvent::Dropped(IpFragmentDrop {
                reason: IpFragmentDropReason::Timeout,
                ..
            })]
        ));
        assert_eq!(reassembler.metrics().buffered_bytes, 0);
    }
}
