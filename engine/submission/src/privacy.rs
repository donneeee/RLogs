use std::collections::BTreeMap;
use std::io::{BufRead, Write};

use rlogs_events::{
    CanonicalEvent, EventEnvelope, EventSensitivity, EvidenceSource, TimelineEventKind,
};
use rlogs_log_format::{
    RLOG_SCHEMA_VERSION, RlogError, RlogLimits, RlogReader, RlogSeal, RlogWriter,
};
use serde_json::Value;
use sha2::{Digest, Sha256};
use thiserror::Error;

use crate::Sha256Digest;

pub const SUBMISSION_PRIVACY_POLICY_VERSION: u16 = 1;

const POLICY_DOCUMENT: &str = concat!(
    "rlogs-submission-privacy/v1\n",
    "source=canonical-rlog-only\n",
    "exclude=chat,local-sensitive,region-evidence\n",
    "allow-personal=character-gameplay-profile-only\n",
    "reject=account,authentication,credential,contact,payment-fields\n",
    "network-identifiers=not-representable\n",
);

const PROTECTED_PROFILE_KEYS: &[&str] = &[
    "access_token",
    "account",
    "account_data",
    "account_id",
    "auth",
    "auth_token",
    "authentication",
    "credential",
    "credentials",
    "email",
    "login",
    "open_id",
    "openid",
    "password",
    "passwd",
    "payment",
    "phone",
    "refresh_token",
    "session_token",
    "token",
];

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct SubmissionPrivacySummary {
    pub retained_events: u64,
    pub excluded_chat_events: u64,
    pub stripped_region_evidence_entries: u64,
}

pub fn submission_privacy_policy_digest() -> Sha256Digest {
    let digest = format!("{:x}", Sha256::digest(POLICY_DOCUMENT.as_bytes()));
    Sha256Digest::parse(digest).expect("the SHA-256 implementation returns a valid digest")
}

/// Creates a canonical upload copy without changing the complete local log.
///
/// Packet/link headers never exist in `.rlog` events. This additional boundary
/// removes chat and free-form region evidence, then rejects rather than stores
/// any profile payload that contains protected account or authentication keys.
pub fn write_privacy_filtered_submission_log<R: BufRead, W: Write>(
    input: R,
    output: W,
    limits: RlogLimits,
) -> Result<(W, RlogSeal, SubmissionPrivacySummary), SubmissionPrivacyError> {
    let mut reader = RlogReader::new(input, limits)?;
    let source_header = reader.header().clone();
    let mut header = source_header.clone();
    // Submission exports are newly written artifacts, so always use the
    // current compact container even when the immutable local source is a
    // legacy JSON-lines rlog. The canonical events are streamed, validated,
    // privacy-filtered, and resealed below; only their container encoding is
    // upgraded.
    header.schema_version = RLOG_SCHEMA_VERSION;
    let stripped_region_evidence_entries = header.region.evidence.len() as u64;
    header.region.evidence.clear();
    header.producer = format!(
        "{}/submission-privacy-v{}",
        header.producer, SUBMISSION_PRIVACY_POLICY_VERSION
    );

    let mut writer = RlogWriter::new(output, header.clone())?;
    let mut next_sequence = 1_u64;
    let mut next_timeline_sequence = 1_u64;
    let mut retained_sequence_map = BTreeMap::new();
    let mut summary = SubmissionPrivacySummary {
        stripped_region_evidence_entries,
        ..SubmissionPrivacySummary::default()
    };

    while let Some(mut envelope) = reader.next_event()? {
        if matches!(envelope.event, CanonicalEvent::Chat(_)) {
            summary.excluded_chat_events = summary.excluded_chat_events.saturating_add(1);
            continue;
        }
        envelope.region = header.region.clone();
        validate_submission_envelope(&envelope)?;

        let source_sequence = envelope.sequence;
        remap_local_skill_receipt_evidence(&mut envelope, &retained_sequence_map)?;
        envelope.sequence = next_sequence;
        retained_sequence_map.insert(source_sequence, next_sequence);
        next_sequence = next_sequence
            .checked_add(1)
            .ok_or(SubmissionPrivacyError::SequenceExhausted)?;
        if let CanonicalEvent::Timeline(timeline) = &mut envelope.event {
            timeline.sequence = next_timeline_sequence;
            next_timeline_sequence = next_timeline_sequence
                .checked_add(1)
                .ok_or(SubmissionPrivacyError::TimelineSequenceExhausted)?;
        }
        writer.push(&envelope)?;
        summary.retained_events = summary.retained_events.saturating_add(1);
    }

    let (output, seal) = writer.finish_with_seal()?;
    Ok((output, seal, summary))
}

// This preserves the honest client's observational-continuity proof across
// privacy resequencing; it is sequence integrity, not cryptographic producer
// attestation against a modified client.
fn remap_local_skill_receipt_evidence(
    envelope: &mut EventEnvelope,
    retained_sequence_map: &BTreeMap<u64, u64>,
) -> Result<(), SubmissionPrivacyError> {
    let CanonicalEvent::Timeline(timeline) = &mut envelope.event else {
        return Ok(());
    };
    if !matches!(
        timeline.kind,
        TimelineEventKind::LocalSkillObservationReceipt(_)
    ) {
        return Ok(());
    }
    let EvidenceSource::Derived {
        evidence_sequences, ..
    } = &mut envelope.provenance.source
    else {
        return Err(SubmissionPrivacyError::InvalidLocalSkillReceiptEvidence);
    };
    for sequence in evidence_sequences {
        *sequence = *retained_sequence_map.get(sequence).ok_or(
            SubmissionPrivacyError::UnretainedLocalSkillReceiptEvidence(*sequence),
        )?;
    }
    timeline.provenance = envelope.provenance.clone();
    Ok(())
}

/// Validates an event already present in a purported submission artifact.
pub fn validate_submission_envelope(
    envelope: &EventEnvelope,
) -> Result<(), SubmissionPrivacyError> {
    if envelope.sensitivity == EventSensitivity::LocalSensitive {
        return Err(SubmissionPrivacyError::LocalSensitiveEvent);
    }
    if matches!(envelope.event, CanonicalEvent::Chat(_)) {
        return Err(SubmissionPrivacyError::ChatEvent);
    }
    if envelope.sensitivity == EventSensitivity::PersonalGameplay
        && !matches!(
            envelope.event,
            CanonicalEvent::CharacterProfileObserved { .. }
        )
    {
        return Err(SubmissionPrivacyError::UnexpectedPersonalGameplayEvent);
    }
    if let CanonicalEvent::CharacterProfileObserved { profile } = &envelope.event {
        validate_profile_value(&profile.payload, "$profile")?;
    }
    if !envelope.region.evidence.is_empty() {
        return Err(SubmissionPrivacyError::FreeFormRegionEvidence);
    }
    Ok(())
}

fn validate_profile_value(value: &Value, path: &str) -> Result<(), SubmissionPrivacyError> {
    match value {
        Value::Object(fields) => {
            for (key, value) in fields {
                let normalized = key.to_ascii_lowercase();
                if PROTECTED_PROFILE_KEYS.contains(&normalized.as_str()) {
                    return Err(SubmissionPrivacyError::ProtectedProfileField {
                        path: format!("{path}.{key}"),
                    });
                }
                validate_profile_value(value, &format!("{path}.{key}"))?;
            }
        }
        Value::Array(items) => {
            for (index, item) in items.iter().enumerate() {
                validate_profile_value(item, &format!("{path}[{index}]"))?;
            }
        }
        _ => {}
    }
    Ok(())
}

#[derive(Debug, Error)]
pub enum SubmissionPrivacyError {
    #[error(transparent)]
    Rlog(#[from] RlogError),

    #[error("local-sensitive events cannot enter a submission artifact")]
    LocalSensitiveEvent,

    #[error("chat events cannot enter a submission artifact")]
    ChatEvent,

    #[error("personal gameplay is allowed only for a character profile observation")]
    UnexpectedPersonalGameplayEvent,

    #[error("submission profile contains protected field {path}")]
    ProtectedProfileField { path: String },

    #[error("submission region context contains free-form connection evidence")]
    FreeFormRegionEvidence,

    #[error("submission event sequence space is exhausted")]
    SequenceExhausted,

    #[error("submission timeline sequence space is exhausted")]
    TimelineSequenceExhausted,

    #[error("local skill observation receipt provenance is not derived evidence")]
    InvalidLocalSkillReceiptEvidence,

    #[error("local skill observation receipt references unretained event sequence {0}")]
    UnretainedLocalSkillReceiptEvidence(u64),
}

#[cfg(test)]
mod tests {
    use std::io::Cursor;

    use rlogs_events::{
        AbilityId, ActorId, BoundaryReason, CanonicalEvent, CastEvent, CastState,
        CharacterIdentity, ChatChannel, ChatEvent, EntityRef, EntityUuid, EventEnvelope,
        EventProvenance, EventSensitivity, EventTime, GameProfileEvent,
        LocalSkillObservationReceipt, RegionContext, RegionEvidence, RegionEvidenceKind,
        RegionIdentity, RunState, TimelineEvent, TimelineEventKind,
    };
    use rlogs_log_format::{RlogHeader, RlogReader, RlogWriter};
    use serde_json::json;

    use super::*;

    fn region() -> RegionContext {
        RegionContext {
            identity: RegionIdentity {
                deployment_id: "global".into(),
                region_id: "asteria".into(),
                realm_id: Some("asteria".into()),
                world_id: None,
            },
            client_build: "steam-test".into(),
            protocol_pack_digest: format!("sha256:{}", "a".repeat(64)),
            evidence: vec![RegionEvidence {
                kind: RegionEvidenceKind::ConnectionEndpoint,
                reference: "must-not-leave-device".into(),
            }],
        }
    }

    fn envelope(
        sequence: u64,
        event: CanonicalEvent,
        sensitivity: EventSensitivity,
    ) -> EventEnvelope {
        EventEnvelope {
            schema_version: rlogs_events::EVENT_SCHEMA_VERSION,
            session_id: "privacy-test".into(),
            sequence,
            region: region(),
            time: EventTime {
                observed_micros: sequence,
                game_time_millis: None,
            },
            provenance: EventProvenance::wire(sequence, 1, 1),
            sensitivity,
            event,
        }
    }

    fn profile(payload: Value) -> CanonicalEvent {
        let character = CharacterIdentity {
            region: region().identity,
            character_id: "3296036".into(),
        };
        CanonicalEvent::CharacterProfileObserved {
            profile: Box::new(GameProfileEvent {
                game_plugin_id: "app.rlogs.game.fixture".into(),
                payload_schema_id: "fixture.character".into(),
                payload_schema_version: 1,
                character,
                payload,
            }),
        }
    }

    fn timeline_envelope(
        sequence: u64,
        timeline_sequence: u64,
        kind: TimelineEventKind,
        provenance: EventProvenance,
    ) -> EventEnvelope {
        let time = EventTime {
            observed_micros: sequence,
            game_time_millis: None,
        };
        let mut envelope = envelope(
            sequence,
            CanonicalEvent::Timeline(TimelineEvent {
                sequence: timeline_sequence,
                time,
                provenance: provenance.clone(),
                kind,
            }),
            EventSensitivity::PublicGameplay,
        );
        envelope.provenance = provenance;
        envelope
    }

    #[test]
    fn export_removes_chat_and_region_evidence_but_keeps_character_gameplay() {
        let header = RlogHeader::new("privacy-test", region(), "fixture");
        let mut writer = RlogWriter::new(Vec::new(), header).unwrap();
        writer
            .push(&envelope(
                1,
                CanonicalEvent::Chat(ChatEvent {
                    channel: ChatChannel::Party,
                    sender: None,
                    sender_character: None,
                    message_id: None,
                    text: "private message".into(),
                }),
                EventSensitivity::PersonalGameplay,
            ))
            .unwrap();
        writer
            .push(&envelope(
                2,
                profile(json!({"display_name":"MarieRose","character_id":"3296036"})),
                EventSensitivity::PersonalGameplay,
            ))
            .unwrap();
        let input = writer.finish().unwrap();

        let (output, _, summary) = write_privacy_filtered_submission_log(
            Cursor::new(input),
            Vec::new(),
            RlogLimits::default(),
        )
        .unwrap();
        assert_eq!(summary.excluded_chat_events, 1);
        assert_eq!(summary.retained_events, 1);
        assert_eq!(summary.stripped_region_evidence_entries, 1);

        let mut reader = RlogReader::new(Cursor::new(output), RlogLimits::default()).unwrap();
        assert!(reader.header().region.evidence.is_empty());
        let event = reader.next_event().unwrap().unwrap();
        assert_eq!(event.sequence, 1);
        assert!(matches!(
            event.event,
            CanonicalEvent::CharacterProfileObserved { .. }
        ));
        assert!(reader.next_event().unwrap().is_none());
    }

    #[test]
    fn export_upgrades_legacy_sources_to_the_compact_submission_container() {
        let mut header = RlogHeader::new("privacy-test", region(), "fixture");
        header.schema_version = rlogs_log_format::LEGACY_RLOG_SCHEMA_VERSION;
        let mut writer = RlogWriter::new(Vec::new(), header).unwrap();
        writer
            .push(&envelope(
                1,
                profile(json!({"display_name":"MarieRose","character_id":"3296036"})),
                EventSensitivity::PersonalGameplay,
            ))
            .unwrap();
        let legacy_input = writer.finish().unwrap();

        let (output, _, summary) = write_privacy_filtered_submission_log(
            Cursor::new(legacy_input),
            Vec::new(),
            RlogLimits::default(),
        )
        .unwrap();

        assert_eq!(summary.retained_events, 1);
        let mut reader = RlogReader::new(Cursor::new(output), RlogLimits::default()).unwrap();
        assert_eq!(reader.header().schema_version, RLOG_SCHEMA_VERSION);
        assert!(reader.next_event().unwrap().is_some());
        assert!(reader.next_event().unwrap().is_none());
    }

    #[test]
    fn privacy_resequence_keeps_local_skill_receipt_bound_after_excluded_chat() {
        let source = EntityRef {
            actor_id: ActorId(8),
            entity_uuid: EntityUuid(80),
        };
        let receipt_provenance =
            EventProvenance::derived("bpsr.complete-local-skill-observation.v1", vec![3, 4, 5]);
        let receipt = timeline_envelope(
            6,
            4,
            TimelineEventKind::LocalSkillObservationReceipt(LocalSkillObservationReceipt {
                source,
                route: "world_use_slot_v1".into(),
                deployment_id: "global".into(),
                client_build: "steam-test".into(),
                protocol_pack_digest: format!("sha256:{}", "a".repeat(64)),
                run_started_micros: 3,
                run_ended_micros: 4,
                request_count: 1,
                decoded_count: 1,
                decode_failure_count: 0,
                capture_queue_saturation_count: 0,
                data_gap_count: 0,
                authoritative_start: true,
                authoritative_completion: true,
            }),
            receipt_provenance,
        );
        let mut local_sensitive = receipt.clone();
        local_sensitive.sensitivity = EventSensitivity::LocalSensitive;
        assert!(matches!(
            validate_submission_envelope(&local_sensitive),
            Err(SubmissionPrivacyError::LocalSensitiveEvent)
        ));

        let header = RlogHeader::new("privacy-test", region(), "fixture");
        let mut writer = RlogWriter::new(Vec::new(), header).unwrap();
        writer
            .push(&envelope(
                1,
                CanonicalEvent::Chat(ChatEvent {
                    channel: ChatChannel::Party,
                    sender: None,
                    sender_character: None,
                    message_id: None,
                    text: "excluded before receipt evidence".into(),
                }),
                EventSensitivity::PersonalGameplay,
            ))
            .unwrap();
        writer
            .push(&envelope(
                2,
                profile(json!({"display_name":"MarieRose","character_id":"3296036"})),
                EventSensitivity::PersonalGameplay,
            ))
            .unwrap();
        writer
            .push(&timeline_envelope(
                3,
                1,
                TimelineEventKind::Cast(CastEvent {
                    source,
                    ability: AbilityId(42),
                    target: None,
                    state: CastState::Started,
                    action_timing: None,
                }),
                EventProvenance::wire(3, 1, 1),
            ))
            .unwrap();
        writer
            .push(&timeline_envelope(
                4,
                2,
                TimelineEventKind::RunBoundary {
                    state: RunState::Completed,
                    scene_id: None,
                    reason: BoundaryReason::AuthoritativePacket,
                },
                EventProvenance::wire(4, 1, 1),
            ))
            .unwrap();
        writer
            .push(&timeline_envelope(
                5,
                3,
                TimelineEventKind::RunBoundary {
                    state: RunState::Exited,
                    scene_id: None,
                    reason: BoundaryReason::AuthoritativePacket,
                },
                EventProvenance::wire(5, 1, 1),
            ))
            .unwrap();
        writer.push(&receipt).unwrap();
        let input = writer.finish().unwrap();
        let (output, _, summary) = write_privacy_filtered_submission_log(
            Cursor::new(input),
            Vec::new(),
            RlogLimits::default(),
        )
        .unwrap();
        assert_eq!(summary.excluded_chat_events, 1);
        assert_eq!(summary.retained_events, 5);
        let mut reader = RlogReader::new(Cursor::new(output), RlogLimits::default()).unwrap();
        let mut retained = Vec::new();
        while let Some(event) = reader.next_event().unwrap() {
            retained.push(event);
        }
        assert_eq!(
            retained
                .iter()
                .map(|event| event.sequence)
                .collect::<Vec<_>>(),
            vec![1, 2, 3, 4, 5]
        );
        assert!(matches!(
            retained[1].event,
            CanonicalEvent::Timeline(TimelineEvent {
                kind: TimelineEventKind::Cast(_),
                ..
            })
        ));
        assert!(matches!(
            retained[2].event,
            CanonicalEvent::Timeline(TimelineEvent {
                kind: TimelineEventKind::RunBoundary {
                    state: RunState::Completed,
                    ..
                },
                ..
            })
        ));
        assert!(matches!(
            retained[3].event,
            CanonicalEvent::Timeline(TimelineEvent {
                kind: TimelineEventKind::RunBoundary {
                    state: RunState::Exited,
                    ..
                },
                ..
            })
        ));
        let EventProvenance {
            source: EvidenceSource::Derived {
                evidence_sequences, ..
            },
            ..
        } = &retained[4].provenance
        else {
            panic!("retained receipt must keep derived provenance")
        };
        assert_eq!(evidence_sequences, &[2, 3, 4]);
        let CanonicalEvent::Timeline(timeline) = &retained[4].event else {
            panic!("retained receipt must remain a timeline event")
        };
        assert_eq!(timeline.provenance, retained[4].provenance);
    }

    #[test]
    fn privacy_resequence_rejects_receipt_reference_to_excluded_event() {
        let source = EntityRef {
            actor_id: ActorId(8),
            entity_uuid: EntityUuid(80),
        };
        let header = RlogHeader::new("privacy-test", region(), "fixture");
        let mut writer = RlogWriter::new(Vec::new(), header).unwrap();
        writer
            .push(&envelope(
                1,
                CanonicalEvent::Chat(ChatEvent {
                    channel: ChatChannel::Party,
                    sender: None,
                    sender_character: None,
                    message_id: None,
                    text: "excluded evidence must not alias".into(),
                }),
                EventSensitivity::PersonalGameplay,
            ))
            .unwrap();
        writer
            .push(&timeline_envelope(
                2,
                1,
                TimelineEventKind::LocalSkillObservationReceipt(LocalSkillObservationReceipt {
                    source,
                    route: "world_use_slot_v1".into(),
                    deployment_id: "global".into(),
                    client_build: "steam-test".into(),
                    protocol_pack_digest: format!("sha256:{}", "a".repeat(64)),
                    run_started_micros: 1,
                    run_ended_micros: 1,
                    request_count: 0,
                    decoded_count: 0,
                    decode_failure_count: 0,
                    capture_queue_saturation_count: 0,
                    data_gap_count: 0,
                    authoritative_start: true,
                    authoritative_completion: true,
                }),
                EventProvenance::derived("bpsr.complete-local-skill-observation.v1", vec![1]),
            ))
            .unwrap();
        let input = writer.finish().unwrap();

        assert!(matches!(
            write_privacy_filtered_submission_log(
                Cursor::new(input),
                Vec::new(),
                RlogLimits::default(),
            ),
            Err(SubmissionPrivacyError::UnretainedLocalSkillReceiptEvidence(
                1
            ))
        ));
    }

    #[test]
    fn export_fails_closed_on_protected_profile_fields() {
        let event = envelope(
            1,
            profile(json!({"character_id":"3296036","nested":{"access_token":"secret"}})),
            EventSensitivity::PersonalGameplay,
        );
        let mut sanitized = event;
        sanitized.region.evidence.clear();
        assert!(matches!(
            validate_submission_envelope(&sanitized),
            Err(SubmissionPrivacyError::ProtectedProfileField { .. })
        ));
    }

    #[test]
    fn policy_digest_is_stable_sha256() {
        assert_eq!(submission_privacy_policy_digest().as_str().len(), 64);
    }
}
