use std::collections::BTreeSet;

use serde::{Deserialize, Deserializer, Serialize, de};
use sha2::{Digest, Sha256};
use thiserror::Error;

use crate::{
    ServerReportReceipt, Sha256Digest, SubmissionError, SubmissionSession, UploadManifest,
    VerificationTier,
};

pub const MOCK_RECEIVER_SCHEMA_VERSION: u16 = 1;
pub const MAXIMUM_MOCK_RECEIVER_CHUNKS: usize = 8_192;

/// In-process receiver used to prove the resumable upload contract without
/// credentials, sockets, or an external service.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct MockSubmissionReceiver {
    schema_version: u16,
    manifest: UploadManifest,
    acknowledged_chunks: BTreeSet<u64>,
    received_bytes: u64,
}

impl MockSubmissionReceiver {
    pub fn begin(manifest: UploadManifest) -> Result<Self, MockReceiverError> {
        let receiver = Self {
            schema_version: MOCK_RECEIVER_SCHEMA_VERSION,
            manifest,
            acknowledged_chunks: BTreeSet::new(),
            received_bytes: 0,
        };
        receiver.validate()?;
        Ok(receiver)
    }

    pub fn receive_chunk(
        &mut self,
        sequence: u64,
        bytes: &[u8],
    ) -> Result<MockChunkReceipt, MockReceiverError> {
        let chunk = self
            .manifest
            .chunks
            .iter()
            .find(|chunk| chunk.sequence == sequence)
            .ok_or(MockReceiverError::UnknownChunk { sequence })?;
        let actual_length =
            u64::try_from(bytes.len()).map_err(|_| MockReceiverError::ByteCountOverflow)?;
        if actual_length != chunk.byte_length {
            return Err(MockReceiverError::ChunkLengthMismatch {
                sequence,
                expected: chunk.byte_length,
                actual: actual_length,
            });
        }
        let digest = digest_bytes(bytes);
        if digest != chunk.sha256 {
            return Err(MockReceiverError::ChunkDigestMismatch { sequence });
        }
        let duplicate = !self.acknowledged_chunks.insert(sequence);
        if !duplicate {
            self.received_bytes = self
                .received_bytes
                .checked_add(actual_length)
                .ok_or(MockReceiverError::ByteCountOverflow)?;
        }
        Ok(MockChunkReceipt {
            sequence,
            sha256: digest,
            duplicate,
        })
    }

    pub fn finalize(&self) -> Result<ServerReportReceipt, MockReceiverError> {
        self.validate()?;
        let pending_count = self
            .manifest
            .chunks
            .len()
            .saturating_sub(self.acknowledged_chunks.len());
        if pending_count != 0 {
            return Err(MockReceiverError::ChunksMissing { pending_count });
        }
        let accepted_log_digest = self
            .manifest
            .sealed_log_digest
            .clone()
            .ok_or(MockReceiverError::MissingSealedDigest)?;
        Ok(ServerReportReceipt {
            report_id: format!("mock-report-{}", &accepted_log_digest.as_str()[..24]),
            accepted_log_digest,
            verification_tier: VerificationTier::Replayed,
        })
    }

    pub fn acknowledged_chunk_count(&self) -> usize {
        self.acknowledged_chunks.len()
    }

    pub fn received_bytes(&self) -> u64 {
        self.received_bytes
    }

    fn validate(&self) -> Result<(), MockReceiverError> {
        if self.schema_version != MOCK_RECEIVER_SCHEMA_VERSION {
            return Err(MockReceiverError::UnsupportedSchema {
                expected: MOCK_RECEIVER_SCHEMA_VERSION,
                actual: self.schema_version,
            });
        }
        if self.manifest.chunks.len() > MAXIMUM_MOCK_RECEIVER_CHUNKS {
            return Err(MockReceiverError::TooManyChunks {
                actual: self.manifest.chunks.len(),
                maximum: MAXIMUM_MOCK_RECEIVER_CHUNKS,
            });
        }
        let sealed_log_digest = self
            .manifest
            .sealed_log_digest
            .clone()
            .ok_or(MockReceiverError::MissingSealedDigest)?;
        SubmissionSession::new_post_run(
            self.manifest.metadata.clone(),
            self.manifest.chunks.clone(),
            sealed_log_digest,
        )?;

        let mut expected_received_bytes = 0_u64;
        for &sequence in &self.acknowledged_chunks {
            let chunk = self
                .manifest
                .chunks
                .iter()
                .find(|chunk| chunk.sequence == sequence)
                .ok_or(MockReceiverError::UnknownAcknowledgedChunk { sequence })?;
            expected_received_bytes = expected_received_bytes
                .checked_add(chunk.byte_length)
                .ok_or(MockReceiverError::ByteCountOverflow)?;
        }
        if expected_received_bytes != self.received_bytes {
            return Err(MockReceiverError::ReceivedByteCountMismatch {
                declared: self.received_bytes,
                expected: expected_received_bytes,
            });
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct MockChunkReceipt {
    pub sequence: u64,
    pub sha256: Sha256Digest,
    pub duplicate: bool,
}

#[derive(Deserialize)]
struct MockSubmissionReceiverData {
    schema_version: u16,
    manifest: UploadManifest,
    acknowledged_chunks: BTreeSet<u64>,
    received_bytes: u64,
}

impl<'de> Deserialize<'de> for MockSubmissionReceiver {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let data = MockSubmissionReceiverData::deserialize(deserializer)?;
        let receiver = Self {
            schema_version: data.schema_version,
            manifest: data.manifest,
            acknowledged_chunks: data.acknowledged_chunks,
            received_bytes: data.received_bytes,
        };
        receiver.validate().map_err(de::Error::custom)?;
        Ok(receiver)
    }
}

fn digest_bytes(bytes: &[u8]) -> Sha256Digest {
    Sha256Digest::parse(format!("{:x}", Sha256::digest(bytes)))
        .expect("SHA-256 formatting always produces a valid digest")
}

#[derive(Clone, Debug, Eq, Error, PartialEq)]
pub enum MockReceiverError {
    #[error("mock receiver schema {actual} is unsupported; expected {expected}")]
    UnsupportedSchema { expected: u16, actual: u16 },
    #[error("mock receiver requires a sealed post-run digest")]
    MissingSealedDigest,
    #[error("mock receiver has {actual} chunks; maximum is {maximum}")]
    TooManyChunks { actual: usize, maximum: usize },
    #[error("mock receiver does not know chunk {sequence}")]
    UnknownChunk { sequence: u64 },
    #[error("mock receiver acknowledgement references unknown chunk {sequence}")]
    UnknownAcknowledgedChunk { sequence: u64 },
    #[error("mock receiver chunk {sequence} has {actual} bytes; expected exactly {expected}")]
    ChunkLengthMismatch {
        sequence: u64,
        expected: u64,
        actual: u64,
    },
    #[error("mock receiver chunk {sequence} digest does not match")]
    ChunkDigestMismatch { sequence: u64 },
    #[error("mock receiver byte count overflowed")]
    ByteCountOverflow,
    #[error("mock receiver byte count {declared} does not match acknowledged bytes {expected}")]
    ReceivedByteCountMismatch { declared: u64, expected: u64 },
    #[error("mock receiver cannot finalize with {pending_count} missing chunks")]
    ChunksMissing { pending_count: usize },
    #[error(transparent)]
    Submission(#[from] SubmissionError),
}

#[cfg(test)]
mod tests {
    use crate::{
        CURRENT_SUBMISSION_SCHEMA, LogChunkDescriptor, ReportVisibility, SubmissionMetadata,
    };

    use super::*;

    fn digest(bytes: &[u8]) -> Sha256Digest {
        digest_bytes(bytes)
    }

    fn manifest() -> UploadManifest {
        let first = b"first";
        let second = b"second";
        UploadManifest {
            metadata: SubmissionMetadata {
                schema_version: CURRENT_SUBMISSION_SCHEMA,
                game_plugin_id: "app.rlogs.game.test".into(),
                local_log_id: "local-1".into(),
                log_format_version: 1,
                capture_session_id: "capture-1".into(),
                game_region: "test".into(),
                client_build: "build-1".into(),
                protocol_pack_digest: digest(b"pack"),
                privacy_policy_digest: digest(b"privacy"),
                visibility: ReportVisibility::Unlisted,
            },
            chunks: vec![
                LogChunkDescriptor::new(0, 0, first.len() as u64, digest(first)),
                LogChunkDescriptor::new(1, first.len() as u64, second.len() as u64, digest(second)),
            ],
            sealed_log_digest: Some(digest(b"sealed-log")),
        }
    }

    #[test]
    fn receiver_resumes_after_a_serialized_interruption_and_finalizes() {
        let mut receiver = MockSubmissionReceiver::begin(manifest()).unwrap();
        let receipt = receiver.receive_chunk(0, b"first").unwrap();
        assert!(!receipt.duplicate);
        let json = serde_json::to_vec(&receiver).unwrap();
        let mut receiver: MockSubmissionReceiver = serde_json::from_slice(&json).unwrap();

        assert_eq!(receiver.acknowledged_chunk_count(), 1);
        let duplicate = receiver.receive_chunk(0, b"first").unwrap();
        assert!(duplicate.duplicate);
        receiver.receive_chunk(1, b"second").unwrap();
        let receipt = receiver.finalize().unwrap();

        assert!(receipt.report_id.starts_with("mock-report-"));
        assert_eq!(receipt.verification_tier, VerificationTier::Replayed);
        assert_eq!(receiver.received_bytes(), 11);
    }

    #[test]
    fn receiver_rejects_tampering_missing_chunks_and_invalid_persisted_state() {
        let mut receiver = MockSubmissionReceiver::begin(manifest()).unwrap();
        assert_eq!(
            receiver.receive_chunk(0, b"other").unwrap_err(),
            MockReceiverError::ChunkDigestMismatch { sequence: 0 }
        );
        assert_eq!(
            receiver.finalize().unwrap_err(),
            MockReceiverError::ChunksMissing { pending_count: 2 }
        );

        let mut value = serde_json::to_value(receiver).unwrap();
        value["acknowledged_chunks"] = serde_json::json!([99]);
        assert!(serde_json::from_value::<MockSubmissionReceiver>(value).is_err());
    }
}
