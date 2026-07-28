use std::collections::BTreeSet;

use serde::{Deserialize, Deserializer, Serialize, de};
use thiserror::Error;

use crate::{
    LogChunkDescriptor, ServerReportReceipt, Sha256Digest, SubmissionMetadata, SubmissionMode,
    SubmissionState, UploadManifest,
};

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct SubmissionSession {
    mode: SubmissionMode,
    state: SubmissionState,
    metadata: SubmissionMetadata,
    chunks: Vec<LogChunkDescriptor>,
    acknowledged_chunks: BTreeSet<u64>,
    sealed_log_digest: Option<Sha256Digest>,
    receipt: Option<ServerReportReceipt>,
}

impl SubmissionSession {
    pub fn new_live(metadata: SubmissionMetadata) -> Self {
        Self::new(SubmissionMode::Live, metadata)
    }

    pub fn new_post_run(
        metadata: SubmissionMetadata,
        chunks: impl IntoIterator<Item = LogChunkDescriptor>,
        sealed_log_digest: Sha256Digest,
    ) -> Result<Self, SubmissionError> {
        let mut session = Self::new(SubmissionMode::PostRun, metadata);
        for chunk in chunks {
            session.queue_local_chunk(chunk)?;
        }
        session.seal_log(sealed_log_digest)?;
        session.validate()?;
        Ok(session)
    }

    fn new(mode: SubmissionMode, metadata: SubmissionMetadata) -> Self {
        Self {
            mode,
            state: SubmissionState::Draft,
            metadata,
            chunks: Vec::new(),
            acknowledged_chunks: BTreeSet::new(),
            sealed_log_digest: None,
            receipt: None,
        }
    }

    pub fn mode(&self) -> SubmissionMode {
        self.mode
    }

    pub fn state(&self) -> SubmissionState {
        self.state
    }

    pub fn receipt(&self) -> Option<&ServerReportReceipt> {
        self.receipt.as_ref()
    }

    pub fn manifest(&self) -> UploadManifest {
        UploadManifest {
            metadata: self.metadata.clone(),
            chunks: self.chunks.clone(),
            sealed_log_digest: self.sealed_log_digest.clone(),
        }
    }

    pub fn queue_local_chunk(&mut self, chunk: LogChunkDescriptor) -> Result<(), SubmissionError> {
        if self.sealed_log_digest.is_some() {
            return Err(SubmissionError::LogAlreadySealed);
        }
        if !matches!(
            self.state,
            SubmissionState::Draft | SubmissionState::Uploading
        ) {
            return Err(SubmissionError::InvalidState {
                operation: "queue a log chunk",
                actual: self.state,
            });
        }
        if chunk.byte_length == 0 {
            return Err(SubmissionError::EmptyChunk {
                sequence: chunk.sequence,
            });
        }
        if chunk.file_offset.checked_add(chunk.byte_length).is_none() {
            return Err(SubmissionError::ChunkLayoutOverflow {
                sequence: chunk.sequence,
            });
        }

        let expected_sequence = self.chunks.len() as u64;
        if chunk.sequence != expected_sequence {
            return Err(SubmissionError::UnexpectedChunkSequence {
                expected: expected_sequence,
                actual: chunk.sequence,
            });
        }

        let expected_offset = self
            .chunks
            .last()
            .map(|previous| {
                previous
                    .file_offset
                    .checked_add(previous.byte_length)
                    .ok_or(SubmissionError::ChunkLayoutOverflow {
                        sequence: previous.sequence,
                    })
            })
            .transpose()?
            .unwrap_or(0);
        if chunk.file_offset != expected_offset {
            return Err(SubmissionError::UnexpectedChunkOffset {
                expected: expected_offset,
                actual: chunk.file_offset,
            });
        }

        self.chunks.push(chunk);
        Ok(())
    }

    pub fn start_upload(&mut self) -> Result<(), SubmissionError> {
        if self.state != SubmissionState::Draft {
            return Err(SubmissionError::InvalidState {
                operation: "start upload",
                actual: self.state,
            });
        }
        if self.mode == SubmissionMode::PostRun && self.sealed_log_digest.is_none() {
            return Err(SubmissionError::PostRunLogNotSealed);
        }
        self.validate()?;

        self.state = SubmissionState::Uploading;
        Ok(())
    }

    pub fn pending_chunks(
        &self,
        limit: usize,
    ) -> Result<Vec<&LogChunkDescriptor>, SubmissionError> {
        if self.state != SubmissionState::Uploading {
            return Err(SubmissionError::InvalidState {
                operation: "read pending chunks",
                actual: self.state,
            });
        }

        Ok(self
            .chunks
            .iter()
            .filter(|chunk| !self.acknowledged_chunks.contains(&chunk.sequence))
            .take(limit)
            .collect())
    }

    pub fn acknowledge_chunk(
        &mut self,
        sequence: u64,
        server_digest: &Sha256Digest,
    ) -> Result<(), SubmissionError> {
        if self.state != SubmissionState::Uploading {
            return Err(SubmissionError::InvalidState {
                operation: "acknowledge a chunk",
                actual: self.state,
            });
        }

        let chunk = self
            .chunks
            .iter()
            .find(|chunk| chunk.sequence == sequence)
            .ok_or(SubmissionError::UnknownChunk { sequence })?;

        if chunk.sha256 != *server_digest {
            return Err(SubmissionError::ChunkDigestMismatch { sequence });
        }

        self.acknowledged_chunks.insert(sequence);
        Ok(())
    }

    pub fn seal_log(&mut self, digest: Sha256Digest) -> Result<(), SubmissionError> {
        if self.sealed_log_digest.is_some() {
            return Err(SubmissionError::LogAlreadySealed);
        }
        if !matches!(
            self.state,
            SubmissionState::Draft | SubmissionState::Uploading
        ) {
            return Err(SubmissionError::InvalidState {
                operation: "seal the local log",
                actual: self.state,
            });
        }
        if self.chunks.is_empty() {
            return Err(SubmissionError::CannotSealEmptyLog);
        }

        self.sealed_log_digest = Some(digest);
        Ok(())
    }

    pub fn begin_finalization(&mut self) -> Result<(), SubmissionError> {
        if self.state != SubmissionState::Uploading {
            return Err(SubmissionError::InvalidState {
                operation: "finalize upload",
                actual: self.state,
            });
        }
        if self.sealed_log_digest.is_none() {
            return Err(SubmissionError::LiveLogNotSealed);
        }

        let pending_count = self.chunks.len() - self.acknowledged_chunks.len();
        if pending_count != 0 {
            return Err(SubmissionError::ChunksNotAcknowledged { pending_count });
        }

        self.state = SubmissionState::Finalizing;
        Ok(())
    }

    pub fn complete(&mut self, receipt: ServerReportReceipt) -> Result<(), SubmissionError> {
        if self.state != SubmissionState::Finalizing {
            return Err(SubmissionError::InvalidState {
                operation: "complete submission",
                actual: self.state,
            });
        }

        if self.sealed_log_digest.as_ref() != Some(&receipt.accepted_log_digest) {
            return Err(SubmissionError::ReceiptDigestMismatch);
        }
        if receipt.report_id.trim().is_empty() {
            return Err(SubmissionError::EmptyReportId);
        }

        self.receipt = Some(receipt);
        self.state = SubmissionState::Submitted;
        Ok(())
    }

    pub fn validate(&self) -> Result<(), SubmissionValidationError> {
        if self.metadata.schema_version != crate::CURRENT_SUBMISSION_SCHEMA {
            return Err(SubmissionValidationError::UnsupportedSchema {
                expected: crate::CURRENT_SUBMISSION_SCHEMA,
                actual: self.metadata.schema_version,
            });
        }
        for (field, value) in [
            ("local_log_id", self.metadata.local_log_id.as_str()),
            (
                "capture_session_id",
                self.metadata.capture_session_id.as_str(),
            ),
            ("game_region", self.metadata.game_region.as_str()),
            ("client_build", self.metadata.client_build.as_str()),
        ] {
            if value.trim().is_empty() {
                return Err(SubmissionValidationError::EmptyMetadataField { field });
            }
        }
        if self.metadata.log_format_version == 0 {
            return Err(SubmissionValidationError::InvalidLogFormatVersion);
        }

        let mut expected_offset = 0u64;
        for (index, chunk) in self.chunks.iter().enumerate() {
            let expected_sequence = index as u64;
            if chunk.sequence != expected_sequence {
                return Err(SubmissionValidationError::UnexpectedChunkSequence {
                    expected: expected_sequence,
                    actual: chunk.sequence,
                });
            }
            if chunk.byte_length == 0 {
                return Err(SubmissionValidationError::EmptyChunk {
                    sequence: chunk.sequence,
                });
            }
            if chunk.file_offset != expected_offset {
                return Err(SubmissionValidationError::UnexpectedChunkOffset {
                    expected: expected_offset,
                    actual: chunk.file_offset,
                });
            }
            expected_offset = chunk.file_offset.checked_add(chunk.byte_length).ok_or(
                SubmissionValidationError::ChunkLayoutOverflow {
                    sequence: chunk.sequence,
                },
            )?;
        }

        if self.sealed_log_digest.is_some() && self.chunks.is_empty() {
            return Err(SubmissionValidationError::SealedLogIsEmpty);
        }
        if self.mode == SubmissionMode::PostRun && self.sealed_log_digest.is_none() {
            return Err(SubmissionValidationError::PostRunLogNotSealed);
        }
        for &sequence in &self.acknowledged_chunks {
            if !self.chunks.iter().any(|chunk| chunk.sequence == sequence) {
                return Err(SubmissionValidationError::UnknownAcknowledgedChunk { sequence });
            }
        }

        let all_chunks_acknowledged = self.acknowledged_chunks.len() == self.chunks.len();
        match self.state {
            SubmissionState::Draft => {
                if !self.acknowledged_chunks.is_empty() {
                    return Err(SubmissionValidationError::DraftHasAcknowledgements);
                }
                if self.receipt.is_some() {
                    return Err(SubmissionValidationError::ReceiptBeforeSubmission);
                }
            }
            SubmissionState::Uploading => {
                if self.receipt.is_some() {
                    return Err(SubmissionValidationError::ReceiptBeforeSubmission);
                }
            }
            SubmissionState::Finalizing => {
                if self.sealed_log_digest.is_none() {
                    return Err(SubmissionValidationError::FinalStateLogNotSealed);
                }
                if !all_chunks_acknowledged {
                    return Err(SubmissionValidationError::FinalStateHasPendingChunks);
                }
                if self.receipt.is_some() {
                    return Err(SubmissionValidationError::ReceiptBeforeSubmission);
                }
            }
            SubmissionState::Submitted => {
                if self.sealed_log_digest.is_none() {
                    return Err(SubmissionValidationError::FinalStateLogNotSealed);
                }
                if !all_chunks_acknowledged {
                    return Err(SubmissionValidationError::FinalStateHasPendingChunks);
                }
                let receipt = self
                    .receipt
                    .as_ref()
                    .ok_or(SubmissionValidationError::SubmittedWithoutReceipt)?;
                if receipt.report_id.trim().is_empty() {
                    return Err(SubmissionValidationError::EmptyReportId);
                }
                if self.sealed_log_digest.as_ref() != Some(&receipt.accepted_log_digest) {
                    return Err(SubmissionValidationError::ReceiptDigestMismatch);
                }
            }
        }

        Ok(())
    }
}

#[derive(Deserialize)]
struct SubmissionSessionData {
    mode: SubmissionMode,
    state: SubmissionState,
    metadata: SubmissionMetadata,
    chunks: Vec<LogChunkDescriptor>,
    acknowledged_chunks: BTreeSet<u64>,
    sealed_log_digest: Option<Sha256Digest>,
    receipt: Option<ServerReportReceipt>,
}

impl<'de> Deserialize<'de> for SubmissionSession {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let data = SubmissionSessionData::deserialize(deserializer)?;
        let session = Self {
            mode: data.mode,
            state: data.state,
            metadata: data.metadata,
            chunks: data.chunks,
            acknowledged_chunks: data.acknowledged_chunks,
            sealed_log_digest: data.sealed_log_digest,
            receipt: data.receipt,
        };
        session.validate().map_err(de::Error::custom)?;
        Ok(session)
    }
}

#[derive(Clone, Debug, Eq, Error, PartialEq)]
pub enum SubmissionError {
    #[error("cannot {operation} while submission is {actual:?}")]
    InvalidState {
        operation: &'static str,
        actual: SubmissionState,
    },
    #[error("chunk {sequence} is empty")]
    EmptyChunk { sequence: u64 },
    #[error("chunk {sequence} extends beyond the supported file offset range")]
    ChunkLayoutOverflow { sequence: u64 },
    #[error("expected chunk sequence {expected}, got {actual}")]
    UnexpectedChunkSequence { expected: u64, actual: u64 },
    #[error("expected chunk offset {expected}, got {actual}")]
    UnexpectedChunkOffset { expected: u64, actual: u64 },
    #[error("chunk {sequence} is not in the local log manifest")]
    UnknownChunk { sequence: u64 },
    #[error("server digest does not match local chunk {sequence}")]
    ChunkDigestMismatch { sequence: u64 },
    #[error("the local log is already sealed")]
    LogAlreadySealed,
    #[error("a post-run upload must reference a sealed local log")]
    PostRunLogNotSealed,
    #[error("the live log must be sealed before finalization")]
    LiveLogNotSealed,
    #[error("an empty local log cannot be sealed")]
    CannotSealEmptyLog,
    #[error("{pending_count} chunks have not been acknowledged")]
    ChunksNotAcknowledged { pending_count: usize },
    #[error("server receipt does not match the sealed local log")]
    ReceiptDigestMismatch,
    #[error("server receipt has an empty report ID")]
    EmptyReportId,
    #[error("submission session is invalid: {0}")]
    InvalidSession(#[from] SubmissionValidationError),
}

#[derive(Clone, Debug, Eq, Error, PartialEq)]
pub enum SubmissionValidationError {
    #[error("submission schema {actual} is unsupported; expected {expected}")]
    UnsupportedSchema { expected: u16, actual: u16 },
    #[error("submission metadata field {field} is empty")]
    EmptyMetadataField { field: &'static str },
    #[error("log format version must be greater than zero")]
    InvalidLogFormatVersion,
    #[error("expected chunk sequence {expected}, got {actual}")]
    UnexpectedChunkSequence { expected: u64, actual: u64 },
    #[error("chunk {sequence} is empty")]
    EmptyChunk { sequence: u64 },
    #[error("expected chunk offset {expected}, got {actual}")]
    UnexpectedChunkOffset { expected: u64, actual: u64 },
    #[error("chunk {sequence} extends beyond the supported file offset range")]
    ChunkLayoutOverflow { sequence: u64 },
    #[error("a sealed log contains no chunks")]
    SealedLogIsEmpty,
    #[error("a post-run submission does not reference a sealed log")]
    PostRunLogNotSealed,
    #[error("acknowledgement references unknown chunk {sequence}")]
    UnknownAcknowledgedChunk { sequence: u64 },
    #[error("a draft submission contains server acknowledgements")]
    DraftHasAcknowledgements,
    #[error("a server receipt exists before the submission is complete")]
    ReceiptBeforeSubmission,
    #[error("a finalizing or submitted log is not sealed")]
    FinalStateLogNotSealed,
    #[error("a finalizing or submitted log still has pending chunks")]
    FinalStateHasPendingChunks,
    #[error("a submitted session has no server receipt")]
    SubmittedWithoutReceipt,
    #[error("a submitted session has an empty report ID")]
    EmptyReportId,
    #[error("server receipt does not match the sealed local log")]
    ReceiptDigestMismatch,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{ReportVisibility, SubmissionMetadata, VerificationTier};

    fn digest(byte: &str) -> Sha256Digest {
        Sha256Digest::parse(byte.repeat(64)).unwrap()
    }

    fn metadata() -> SubmissionMetadata {
        SubmissionMetadata::new(
            "local-log-1",
            1,
            "capture-session-1",
            "global",
            "steam-24252055",
            digest("a"),
            digest("b"),
            ReportVisibility::Unlisted,
        )
    }

    fn chunks() -> Vec<LogChunkDescriptor> {
        vec![
            LogChunkDescriptor::new(0, 0, 100, digest("1")),
            LogChunkDescriptor::new(1, 100, 80, digest("2")),
        ]
    }

    #[test]
    fn live_and_post_run_produce_the_same_sealed_manifest() {
        let mut live = SubmissionSession::new_live(metadata());
        for chunk in chunks() {
            live.queue_local_chunk(chunk).unwrap();
        }
        live.seal_log(digest("c")).unwrap();

        let post_run = SubmissionSession::new_post_run(metadata(), chunks(), digest("c")).unwrap();

        assert_eq!(live.manifest(), post_run.manifest());
        assert_eq!(live.mode(), SubmissionMode::Live);
        assert_eq!(post_run.mode(), SubmissionMode::PostRun);
    }

    #[test]
    fn live_upload_can_start_before_the_first_chunk_is_written() {
        let mut session = SubmissionSession::new_live(metadata());
        session.start_upload().unwrap();
        session.queue_local_chunk(chunks().remove(0)).unwrap();

        assert_eq!(session.state(), SubmissionState::Uploading);
        assert_eq!(session.pending_chunks(10).unwrap().len(), 1);
    }

    #[test]
    fn chunks_must_match_the_local_file_layout() {
        let mut session = SubmissionSession::new_live(metadata());
        let error = session
            .queue_local_chunk(LogChunkDescriptor::new(1, 0, 100, digest("1")))
            .unwrap_err();
        assert_eq!(
            error,
            SubmissionError::UnexpectedChunkSequence {
                expected: 0,
                actual: 1
            }
        );

        let mut overflow = SubmissionSession::new_live(metadata());
        overflow
            .queue_local_chunk(LogChunkDescriptor::new(0, 0, u64::MAX, digest("1")))
            .unwrap();
        assert_eq!(
            overflow
                .queue_local_chunk(LogChunkDescriptor::new(1, u64::MAX, 1, digest("2")))
                .unwrap_err(),
            SubmissionError::ChunkLayoutOverflow { sequence: 1 }
        );

        session.queue_local_chunk(chunks().remove(0)).unwrap();
        let error = session
            .queue_local_chunk(LogChunkDescriptor::new(1, 99, 80, digest("2")))
            .unwrap_err();
        assert_eq!(
            error,
            SubmissionError::UnexpectedChunkOffset {
                expected: 100,
                actual: 99
            }
        );
    }

    #[test]
    fn server_acknowledgements_are_digest_checked_and_resumable() {
        let mut session =
            SubmissionSession::new_post_run(metadata(), chunks(), digest("c")).unwrap();
        session.start_upload().unwrap();

        assert_eq!(
            session.acknowledge_chunk(0, &digest("f")).unwrap_err(),
            SubmissionError::ChunkDigestMismatch { sequence: 0 }
        );
        session.acknowledge_chunk(0, &digest("1")).unwrap();

        let pending = session.pending_chunks(10).unwrap();
        assert_eq!(pending.len(), 1);
        assert_eq!(pending[0].sequence, 1);
    }

    #[test]
    fn finalization_requires_a_seal_and_every_chunk_acknowledgement() {
        let mut session = SubmissionSession::new_live(metadata());
        for chunk in chunks() {
            session.queue_local_chunk(chunk).unwrap();
        }
        session.start_upload().unwrap();

        assert_eq!(
            session.begin_finalization().unwrap_err(),
            SubmissionError::LiveLogNotSealed
        );

        session.seal_log(digest("c")).unwrap();
        session.acknowledge_chunk(0, &digest("1")).unwrap();
        assert_eq!(
            session.begin_finalization().unwrap_err(),
            SubmissionError::ChunksNotAcknowledged { pending_count: 1 }
        );

        session.acknowledge_chunk(1, &digest("2")).unwrap();
        session.begin_finalization().unwrap();
        assert_eq!(session.state(), SubmissionState::Finalizing);
    }

    #[test]
    fn completion_requires_a_receipt_for_the_same_local_artifact() {
        let mut session =
            SubmissionSession::new_post_run(metadata(), chunks(), digest("c")).unwrap();
        session.start_upload().unwrap();
        session.acknowledge_chunk(0, &digest("1")).unwrap();
        session.acknowledge_chunk(1, &digest("2")).unwrap();
        session.begin_finalization().unwrap();

        let wrong_receipt = ServerReportReceipt {
            report_id: "report-1".into(),
            accepted_log_digest: digest("d"),
            verification_tier: VerificationTier::Uploaded,
        };
        assert_eq!(
            session.complete(wrong_receipt).unwrap_err(),
            SubmissionError::ReceiptDigestMismatch
        );

        let receipt = ServerReportReceipt {
            report_id: "report-1".into(),
            accepted_log_digest: digest("c"),
            verification_tier: VerificationTier::Uploaded,
        };
        session.complete(receipt).unwrap();

        assert_eq!(session.state(), SubmissionState::Submitted);
        assert_eq!(session.receipt().unwrap().report_id, "report-1");
    }

    #[test]
    fn session_round_trips_for_crash_safe_resume() {
        let mut session =
            SubmissionSession::new_post_run(metadata(), chunks(), digest("c")).unwrap();
        session.start_upload().unwrap();
        session.acknowledge_chunk(0, &digest("1")).unwrap();

        let json = serde_json::to_string(&session).unwrap();
        let restored: SubmissionSession = serde_json::from_str(&json).unwrap();

        assert_eq!(restored, session);
        assert_eq!(restored.pending_chunks(10).unwrap()[0].sequence, 1);
    }

    #[test]
    fn persisted_sessions_cannot_bypass_state_invariants() {
        let mut session =
            SubmissionSession::new_post_run(metadata(), chunks(), digest("c")).unwrap();
        session.start_upload().unwrap();
        let mut value = serde_json::to_value(&session).unwrap();

        value["chunks"][1]["sequence"] = serde_json::json!(99);
        assert!(serde_json::from_value::<SubmissionSession>(value).is_err());

        let mut value = serde_json::to_value(&session).unwrap();
        value["acknowledged_chunks"] = serde_json::json!([99]);
        assert!(serde_json::from_value::<SubmissionSession>(value).is_err());

        let mut value = serde_json::to_value(&session).unwrap();
        value["state"] = serde_json::json!("submitted");
        assert!(serde_json::from_value::<SubmissionSession>(value).is_err());
    }
}
