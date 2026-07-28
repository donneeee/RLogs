use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::{CaptureAdapter, CaptureRecord, CaptureRecordDraft};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GameBuild {
    /// Deployment family such as `global`, `china`, or `unknown`.
    pub region: String,
    /// Distribution channel such as `steam`, `standalone`, or `unknown`.
    pub channel: String,
    /// Exact launcher/client build identifier.
    pub build_id: String,
    pub executable_version: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CaptureSession {
    pub format_version: u16,
    pub capture_id: String,
    pub started_unix_micros: Option<i64>,
    pub game_build: GameBuild,
    pub adapter: CaptureAdapter,
    pub protocol_pack_digest: Option<String>,
}

/// Append-only lossless protocol records for one capture session.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProtocolJournal {
    session: CaptureSession,
    records: Vec<CaptureRecord>,
}

impl ProtocolJournal {
    pub fn new(session: CaptureSession) -> Self {
        Self {
            session,
            records: Vec::new(),
        }
    }

    pub fn session(&self) -> &CaptureSession {
        &self.session
    }

    pub fn records(&self) -> &[CaptureRecord] {
        &self.records
    }

    pub fn len(&self) -> usize {
        self.records.len()
    }

    pub fn is_empty(&self) -> bool {
        self.records.is_empty()
    }

    pub fn push(&mut self, draft: CaptureRecordDraft) -> Result<u64, JournalError> {
        if let Some(previous) = self.records.last() {
            if draft.observed_micros < previous.observed_micros {
                return Err(JournalError::ObservedTimeMovedBackward {
                    previous_micros: previous.observed_micros,
                    next_micros: draft.observed_micros,
                });
            }
        }

        let sequence = self.records.len() as u64 + 1;
        self.records.push(CaptureRecord {
            sequence,
            observed_micros: draft.observed_micros,
            wall_clock_unix_micros: draft.wall_clock_unix_micros,
            kind: draft.kind,
        });
        Ok(sequence)
    }

    pub fn validate(&self) -> Result<(), JournalError> {
        let mut previous_time = None;

        for (index, record) in self.records.iter().enumerate() {
            let expected_sequence = index as u64 + 1;
            if record.sequence != expected_sequence {
                return Err(JournalError::InvalidSequence {
                    expected: expected_sequence,
                    actual: record.sequence,
                });
            }

            if let Some(previous_micros) = previous_time {
                if record.observed_micros < previous_micros {
                    return Err(JournalError::ObservedTimeMovedBackward {
                        previous_micros,
                        next_micros: record.observed_micros,
                    });
                }
            }

            previous_time = Some(record.observed_micros);
        }

        Ok(())
    }
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum JournalError {
    #[error("observed capture time moved backward from {previous_micros}us to {next_micros}us")]
    ObservedTimeMovedBackward {
        previous_micros: u64,
        next_micros: u64,
    },

    #[error("capture sequence should be {expected}, but was {actual}")]
    InvalidSequence { expected: u64, actual: u64 },
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        CaptureGap, CaptureGapKind, CaptureRecordKind, CompressionState, PacketDirection,
        PacketEnvelope, PacketPayload, RouteKey, RoutedMessage,
    };

    fn session() -> CaptureSession {
        CaptureSession {
            format_version: 1,
            capture_id: "test-capture".into(),
            started_unix_micros: Some(1_000),
            game_build: GameBuild {
                region: "global".into(),
                channel: "steam".into(),
                build_id: "24252055".into(),
                executable_version: None,
            },
            adapter: CaptureAdapter {
                name: "fixture".into(),
                version: None,
            },
            protocol_pack_digest: None,
        }
    }

    fn unknown_packet(observed_micros: u64) -> CaptureRecordDraft {
        CaptureRecordDraft {
            observed_micros,
            wall_clock_unix_micros: None,
            kind: CaptureRecordKind::Packet(PacketEnvelope {
                connection_id: 10,
                stream_id: 2,
                source: None,
                destination: None,
                direction: PacketDirection::ServerToClient,
                fragment: Some(crate::FragmentKind::Notify),
                route: Some(RoutedMessage {
                    key: RouteKey::new(
                        PacketDirection::ServerToClient,
                        crate::FragmentKind::Notify,
                        999,
                        123,
                    ),
                    stub_id: 0,
                    call_id: None,
                }),
                compression: CompressionState::NotCompressed,
                payload: PacketPayload {
                    wire_bytes: vec![1, 2, 3],
                    application_bytes: Some(vec![3]),
                },
            }),
        }
    }

    #[test]
    fn arbitrary_unknown_routes_and_bytes_are_preserved() {
        let mut journal = ProtocolJournal::new(session());
        assert_eq!(journal.push(unknown_packet(10)), Ok(1));

        let CaptureRecordKind::Packet(packet) = &journal.records()[0].kind else {
            panic!("expected packet");
        };
        assert_eq!(packet.route.expect("route").key.service_id, 999);
        assert_eq!(packet.payload.wire_bytes, [1, 2, 3]);
        assert_eq!(packet.payload.decode_input(), Some([3].as_slice()));
    }

    #[test]
    fn capture_gaps_are_first_class_records() {
        let mut journal = ProtocolJournal::new(session());
        journal
            .push(CaptureRecordDraft {
                observed_micros: 20,
                wall_clock_unix_micros: None,
                kind: CaptureRecordKind::Gap(CaptureGap {
                    kind: CaptureGapKind::QueueDrop,
                    connection_id: Some(10),
                    stream_id: Some(2),
                    lost_bytes: None,
                    detail: "bounded queue was full".into(),
                }),
            })
            .unwrap();

        assert!(matches!(
            journal.records()[0].kind,
            CaptureRecordKind::Gap(CaptureGap {
                kind: CaptureGapKind::QueueDrop,
                ..
            })
        ));
    }

    #[test]
    fn backward_time_is_rejected_without_mutation() {
        let mut journal = ProtocolJournal::new(session());
        journal.push(unknown_packet(20)).unwrap();

        assert_eq!(
            journal.push(unknown_packet(19)),
            Err(JournalError::ObservedTimeMovedBackward {
                previous_micros: 20,
                next_micros: 19,
            })
        );
        assert_eq!(journal.len(), 1);
    }

    #[test]
    fn journal_round_trips_through_json() {
        let mut journal = ProtocolJournal::new(session());
        journal.push(unknown_packet(10)).unwrap();

        let json = serde_json::to_string(&journal).unwrap();
        let decoded: ProtocolJournal = serde_json::from_str(&json).unwrap();

        assert_eq!(decoded, journal);
        assert_eq!(decoded.validate(), Ok(()));
    }
}
