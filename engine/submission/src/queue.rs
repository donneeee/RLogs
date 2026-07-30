use serde::{Deserialize, Deserializer, Serialize, de};
use thiserror::Error;

use crate::{
    DigestError, LocalLogArtifact, ReportVisibility, Sha256Digest, SubmissionError,
    SubmissionMetadata, SubmissionMode, SubmissionSession, SubmissionState,
    SubmissionValidationError,
};

pub const QUEUED_SUBMISSION_SCHEMA_VERSION: u16 = 1;
pub const MAXIMUM_LOCAL_ARTIFACT_PATH_BYTES: usize = 4 * 1024;

/// Crash-safe local state for one verified log awaiting optional upload.
///
/// `local_artifact_path` is host-only state and is never copied into the
/// server upload manifest.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct QueuedSubmission {
    pub schema_version: u16,
    pub queue_id: Sha256Digest,
    pub created_unix_millis: u64,
    pub local_artifact_path: String,
    pub file_byte_length: u64,
    pub canonical_content_sha256: Sha256Digest,
    pub session: SubmissionSession,
}

impl QueuedSubmission {
    pub fn new_post_run(
        metadata: SubmissionMetadata,
        artifact: &LocalLogArtifact,
        local_artifact_path: impl Into<String>,
        created_unix_millis: u64,
    ) -> Result<Self, QueuedSubmissionError> {
        let canonical_content_sha256 = parse_prefixed_sha256(&artifact.rlog.content_sha256)?;
        let value = Self {
            schema_version: QUEUED_SUBMISSION_SCHEMA_VERSION,
            queue_id: artifact.file_sha256.clone(),
            created_unix_millis,
            local_artifact_path: local_artifact_path.into(),
            file_byte_length: artifact.file_byte_length,
            canonical_content_sha256,
            session: SubmissionSession::new_post_run_artifact(metadata, artifact)?,
        };
        value.validate()?;
        Ok(value)
    }

    pub fn validate(&self) -> Result<(), QueuedSubmissionValidationError> {
        if self.schema_version != QUEUED_SUBMISSION_SCHEMA_VERSION {
            return Err(QueuedSubmissionValidationError::UnsupportedSchema {
                expected: QUEUED_SUBMISSION_SCHEMA_VERSION,
                actual: self.schema_version,
            });
        }
        if self.created_unix_millis == 0 {
            return Err(QueuedSubmissionValidationError::MissingCreationTime);
        }
        let path = self.local_artifact_path.trim();
        if path.is_empty() {
            return Err(QueuedSubmissionValidationError::EmptyLocalArtifactPath);
        }
        if path.len() > MAXIMUM_LOCAL_ARTIFACT_PATH_BYTES || path.contains('\0') {
            return Err(QueuedSubmissionValidationError::InvalidLocalArtifactPath);
        }
        if self.file_byte_length == 0 {
            return Err(QueuedSubmissionValidationError::EmptyArtifact);
        }

        self.session.validate()?;
        if self.session.mode() != SubmissionMode::PostRun {
            return Err(QueuedSubmissionValidationError::NotPostRun);
        }
        let sealed_digest = self
            .session
            .sealed_log_digest()
            .ok_or(QueuedSubmissionValidationError::MissingSealedDigest)?;
        if sealed_digest != &self.queue_id {
            return Err(QueuedSubmissionValidationError::QueueDigestMismatch);
        }
        let manifest_length = self
            .session
            .chunks()
            .iter()
            .try_fold(0_u64, |total, chunk| {
                total
                    .checked_add(chunk.byte_length)
                    .ok_or(QueuedSubmissionValidationError::ArtifactLengthOverflow)
            })?;
        if manifest_length != self.file_byte_length {
            return Err(QueuedSubmissionValidationError::ArtifactLengthMismatch {
                declared: self.file_byte_length,
                manifest: manifest_length,
            });
        }
        Ok(())
    }

    pub fn capture_session_id(&self) -> &str {
        &self.session.metadata().capture_session_id
    }

    pub fn visibility(&self) -> ReportVisibility {
        self.session.metadata().visibility
    }

    pub fn state(&self) -> SubmissionState {
        self.session.state()
    }

    /// Confirms that a freshly re-read sealed artifact is still the exact file
    /// described by this queue entry.
    pub fn verify_artifact(
        &self,
        artifact: &LocalLogArtifact,
    ) -> Result<(), QueuedArtifactVerificationError> {
        self.validate()?;
        if artifact.file_sha256 != self.queue_id {
            return Err(QueuedArtifactVerificationError::FileDigestMismatch);
        }
        let canonical_content_sha256 = parse_prefixed_sha256(&artifact.rlog.content_sha256)?;
        if canonical_content_sha256 != self.canonical_content_sha256 {
            return Err(QueuedArtifactVerificationError::CanonicalDigestMismatch);
        }
        if artifact.file_byte_length != self.file_byte_length {
            return Err(QueuedArtifactVerificationError::FileLengthMismatch {
                queued: self.file_byte_length,
                artifact: artifact.file_byte_length,
            });
        }
        if artifact.chunks != self.session.chunks() {
            return Err(QueuedArtifactVerificationError::ChunkManifestMismatch);
        }

        let metadata = self.session.metadata();
        if artifact.header.schema_version != metadata.log_format_version {
            return Err(QueuedArtifactVerificationError::LogFormatMismatch);
        }
        if artifact.header.session_id != metadata.capture_session_id {
            return Err(QueuedArtifactVerificationError::CaptureSessionMismatch);
        }
        if artifact.header.region.identity.region_id != metadata.game_region {
            return Err(QueuedArtifactVerificationError::GameRegionMismatch);
        }
        if artifact.header.region.client_build != metadata.client_build {
            return Err(QueuedArtifactVerificationError::ClientBuildMismatch);
        }
        let protocol_pack_digest =
            parse_prefixed_sha256(&artifact.header.region.protocol_pack_digest)?;
        if protocol_pack_digest != metadata.protocol_pack_digest {
            return Err(QueuedArtifactVerificationError::ProtocolPackMismatch);
        }
        Ok(())
    }
}

fn parse_prefixed_sha256(value: &str) -> Result<Sha256Digest, DigestError> {
    Sha256Digest::parse(value.strip_prefix("sha256:").unwrap_or(value))
}

#[derive(Deserialize)]
struct QueuedSubmissionData {
    schema_version: u16,
    queue_id: Sha256Digest,
    created_unix_millis: u64,
    local_artifact_path: String,
    file_byte_length: u64,
    canonical_content_sha256: Sha256Digest,
    session: SubmissionSession,
}

impl<'de> Deserialize<'de> for QueuedSubmission {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let data = QueuedSubmissionData::deserialize(deserializer)?;
        let value = Self {
            schema_version: data.schema_version,
            queue_id: data.queue_id,
            created_unix_millis: data.created_unix_millis,
            local_artifact_path: data.local_artifact_path,
            file_byte_length: data.file_byte_length,
            canonical_content_sha256: data.canonical_content_sha256,
            session: data.session,
        };
        value.validate().map_err(de::Error::custom)?;
        Ok(value)
    }
}

#[derive(Debug, Error)]
pub enum QueuedSubmissionError {
    #[error(transparent)]
    Digest(#[from] DigestError),
    #[error(transparent)]
    Submission(#[from] SubmissionError),
    #[error(transparent)]
    Validation(#[from] QueuedSubmissionValidationError),
}

#[derive(Clone, Debug, Eq, Error, PartialEq)]
pub enum QueuedSubmissionValidationError {
    #[error("queued submission schema {actual} is unsupported; expected {expected}")]
    UnsupportedSchema { expected: u16, actual: u16 },
    #[error("queued submission creation time is missing")]
    MissingCreationTime,
    #[error("queued submission local artifact path is empty")]
    EmptyLocalArtifactPath,
    #[error("queued submission local artifact path is invalid or too long")]
    InvalidLocalArtifactPath,
    #[error("queued submission artifact is empty")]
    EmptyArtifact,
    #[error("queued submission must contain a post-run session")]
    NotPostRun,
    #[error("queued submission is missing its sealed log digest")]
    MissingSealedDigest,
    #[error("queued submission ID does not match its sealed file digest")]
    QueueDigestMismatch,
    #[error("queued submission artifact length overflowed")]
    ArtifactLengthOverflow,
    #[error(
        "queued submission artifact length {declared} does not match chunk manifest length {manifest}"
    )]
    ArtifactLengthMismatch { declared: u64, manifest: u64 },
    #[error(transparent)]
    Submission(#[from] SubmissionValidationError),
}

#[derive(Clone, Debug, Eq, Error, PartialEq)]
pub enum QueuedArtifactVerificationError {
    #[error("queued artifact file SHA-256 no longer matches")]
    FileDigestMismatch,
    #[error("queued artifact canonical-content SHA-256 no longer matches")]
    CanonicalDigestMismatch,
    #[error("queued artifact length {queued} does not match verified length {artifact}")]
    FileLengthMismatch { queued: u64, artifact: u64 },
    #[error("queued artifact chunk manifest no longer matches")]
    ChunkManifestMismatch,
    #[error("queued artifact log-format version no longer matches")]
    LogFormatMismatch,
    #[error("queued artifact capture session no longer matches")]
    CaptureSessionMismatch,
    #[error("queued artifact game region no longer matches")]
    GameRegionMismatch,
    #[error("queued artifact client build no longer matches")]
    ClientBuildMismatch,
    #[error("queued artifact protocol-pack digest no longer matches")]
    ProtocolPackMismatch,
    #[error(transparent)]
    Digest(#[from] DigestError),
    #[error(transparent)]
    Queue(#[from] QueuedSubmissionValidationError),
}

#[cfg(test)]
mod tests {
    use rlogs_events::{RegionContext, RegionIdentity};
    use rlogs_log_format::{RLOG_SCHEMA_VERSION, RlogHeader, RlogReplaySummary};

    use crate::{LogChunkDescriptor, ReportVisibility};

    use super::*;

    fn digest(byte: &str) -> Sha256Digest {
        Sha256Digest::parse(byte.repeat(64)).unwrap()
    }

    fn artifact() -> LocalLogArtifact {
        LocalLogArtifact {
            header: RlogHeader {
                schema_version: RLOG_SCHEMA_VERSION,
                event_schema_version: 2,
                session_id: "capture-1".into(),
                region: RegionContext {
                    identity: RegionIdentity {
                        deployment_id: "global".into(),
                        region_id: "north-america".into(),
                        realm_id: None,
                        world_id: Some("asteria".into()),
                    },
                    client_build: "build-1".into(),
                    protocol_pack_digest: format!("sha256:{}", "a".repeat(64)),
                    evidence: Vec::new(),
                },
                producer: "test".into(),
            },
            rlog: RlogReplaySummary {
                event_count: 1,
                first_observed_micros: Some(1),
                last_observed_micros: Some(2),
                content_sha256: format!("sha256:{}", "b".repeat(64)),
            },
            file_byte_length: 12,
            file_sha256: digest("c"),
            chunks: vec![
                LogChunkDescriptor::new(0, 0, 8, digest("d")),
                LogChunkDescriptor::new(1, 8, 4, digest("e")),
            ],
        }
    }

    fn metadata() -> SubmissionMetadata {
        SubmissionMetadata::new(
            "app.rlogs.game.blue-protocol-star-resonance",
            "local-log-1",
            RLOG_SCHEMA_VERSION,
            "capture-1",
            "north-america",
            "build-1",
            digest("a"),
            digest("a"),
            ReportVisibility::Unlisted,
        )
    }

    #[test]
    fn queue_entry_round_trips_with_only_local_path_state_outside_the_manifest() {
        let entry = QueuedSubmission::new_post_run(
            metadata(),
            &artifact(),
            "C:/rlogs/logs/capture-1.rlog",
            1_700_000_000_000,
        )
        .unwrap();
        let json = serde_json::to_string_pretty(&entry).unwrap();
        let restored: QueuedSubmission = serde_json::from_str(&json).unwrap();

        assert_eq!(restored, entry);
        assert_eq!(restored.queue_id, digest("c"));
        assert_eq!(restored.canonical_content_sha256, digest("b"));
        assert_eq!(restored.state(), SubmissionState::Draft);
        assert!(
            !serde_json::to_string(&entry.session.manifest())
                .unwrap()
                .contains("capture-1.rlog")
        );
    }

    #[test]
    fn persisted_queue_entries_cannot_change_digest_length_or_local_path_invariants() {
        let entry = QueuedSubmission::new_post_run(
            metadata(),
            &artifact(),
            "C:/rlogs/logs/capture-1.rlog",
            1_700_000_000_000,
        )
        .unwrap();
        let mut value = serde_json::to_value(&entry).unwrap();
        value["queue_id"] = serde_json::json!("f".repeat(64));
        assert!(serde_json::from_value::<QueuedSubmission>(value).is_err());

        let mut value = serde_json::to_value(&entry).unwrap();
        value["file_byte_length"] = serde_json::json!(13);
        assert!(serde_json::from_value::<QueuedSubmission>(value).is_err());

        let mut value = serde_json::to_value(&entry).unwrap();
        value["local_artifact_path"] = serde_json::json!("");
        assert!(serde_json::from_value::<QueuedSubmission>(value).is_err());
    }

    #[test]
    fn reverified_artifacts_must_match_every_immutable_queue_identity() {
        let artifact = artifact();
        let entry = QueuedSubmission::new_post_run(
            metadata(),
            &artifact,
            "C:/rlogs/logs/capture-1.rlog",
            1_700_000_000_000,
        )
        .unwrap();
        entry.verify_artifact(&artifact).unwrap();

        let mut changed_file = artifact.clone();
        changed_file.file_sha256 = digest("f");
        assert_eq!(
            entry.verify_artifact(&changed_file),
            Err(QueuedArtifactVerificationError::FileDigestMismatch)
        );

        let mut changed_chunks = artifact.clone();
        changed_chunks.chunks[0].sha256 = digest("f");
        assert_eq!(
            entry.verify_artifact(&changed_chunks),
            Err(QueuedArtifactVerificationError::ChunkManifestMismatch)
        );

        let mut changed_region = artifact;
        changed_region.header.region.identity.region_id = "another-region".into();
        assert_eq!(
            entry.verify_artifact(&changed_region),
            Err(QueuedArtifactVerificationError::GameRegionMismatch)
        );
    }
}
