use std::io::{BufReader, Read, Seek, SeekFrom};

use rlogs_log_format::{RlogError, RlogHeader, RlogLimits, RlogReader, RlogReplaySummary};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use thiserror::Error;

use crate::validate_submission_envelope;
use crate::{LogChunkDescriptor, Sha256Digest};

pub const DEFAULT_UPLOAD_CHUNK_BYTES: usize = 4 * 1024 * 1024;
pub const MAXIMUM_UPLOAD_CHUNK_BYTES: usize = 16 * 1024 * 1024;
pub const DEFAULT_MAXIMUM_LOG_BYTES: u64 = 16 * 1024 * 1024 * 1024;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ArtifactBuildLimits {
    pub chunk_bytes: usize,
    pub maximum_file_bytes: u64,
}

impl Default for ArtifactBuildLimits {
    fn default() -> Self {
        Self {
            chunk_bytes: DEFAULT_UPLOAD_CHUNK_BYTES,
            maximum_file_bytes: DEFAULT_MAXIMUM_LOG_BYTES,
        }
    }
}

impl ArtifactBuildLimits {
    fn validate(self) -> Result<Self, ArtifactBuildError> {
        if self.chunk_bytes == 0
            || self.chunk_bytes > MAXIMUM_UPLOAD_CHUNK_BYTES
            || self.maximum_file_bytes == 0
        {
            return Err(ArtifactBuildError::InvalidLimits {
                chunk_bytes: self.chunk_bytes,
                maximum_file_bytes: self.maximum_file_bytes,
            });
        }
        Ok(self)
    }
}

/// Upload-ready metadata derived from an already sealed canonical `.rlog`.
///
/// `file_sha256` covers the exact bytes sent to the service. The nested rlog
/// summary separately retains the canonical-event digest from the seal.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct LocalLogArtifact {
    pub header: RlogHeader,
    pub rlog: RlogReplaySummary,
    pub file_byte_length: u64,
    pub file_sha256: Sha256Digest,
    pub chunks: Vec<LogChunkDescriptor>,
}

pub fn build_sealed_log_artifact<R: Read + Seek>(
    input: R,
    artifact_limits: ArtifactBuildLimits,
    rlog_limits: RlogLimits,
) -> Result<LocalLogArtifact, ArtifactBuildError> {
    build_log_artifact_with_validator(input, artifact_limits, rlog_limits, |_| Ok(()))
}

/// Builds upload metadata while enforcing the canonical submission privacy
/// boundary during the artifact's existing integrity replay.
pub fn build_privacy_verified_submission_artifact<R: Read + Seek>(
    input: R,
    artifact_limits: ArtifactBuildLimits,
    rlog_limits: RlogLimits,
) -> Result<LocalLogArtifact, ArtifactBuildError> {
    build_log_artifact_with_validator(input, artifact_limits, rlog_limits, |envelope| {
        validate_submission_envelope(envelope).map_err(|error| error.to_string())
    })
}

fn build_log_artifact_with_validator<R: Read + Seek>(
    mut input: R,
    artifact_limits: ArtifactBuildLimits,
    rlog_limits: RlogLimits,
    mut validate: impl FnMut(&rlogs_events::EventEnvelope) -> Result<(), String>,
) -> Result<LocalLogArtifact, ArtifactBuildError> {
    let artifact_limits = artifact_limits.validate()?;
    let declared_file_length = input.seek(SeekFrom::End(0))?;
    if declared_file_length > artifact_limits.maximum_file_bytes {
        return Err(ArtifactBuildError::FileTooLarge {
            actual_at_least: declared_file_length,
            maximum: artifact_limits.maximum_file_bytes,
        });
    }
    input.seek(SeekFrom::Start(0))?;
    let mut tracked = ArtifactTrackingReader::new(
        input,
        artifact_limits.chunk_bytes,
        artifact_limits.maximum_file_bytes,
    );
    let verification = (|| {
        let reader = RlogReader::new(BufReader::new(&mut tracked), rlog_limits)?;
        let header = reader.header().clone();
        let rlog = reader.replay(&mut validate)?;
        Ok::<_, RlogError>((header, rlog))
    })();
    if let Some(actual_at_least) = tracked.limit_exceeded_at {
        return Err(ArtifactBuildError::FileTooLarge {
            actual_at_least,
            maximum: artifact_limits.maximum_file_bytes,
        });
    }
    let (header, rlog) = verification?;
    let tracked = tracked.finish()?;
    if tracked.chunks.is_empty() {
        return Err(ArtifactBuildError::EmptyArtifact);
    }
    if tracked.file_byte_length != declared_file_length {
        return Err(ArtifactBuildError::FileLengthChanged {
            declared: declared_file_length,
            observed: tracked.file_byte_length,
        });
    }

    Ok(LocalLogArtifact {
        header,
        rlog,
        file_byte_length: tracked.file_byte_length,
        file_sha256: tracked.file_sha256,
        chunks: tracked.chunks,
    })
}

/// Builds byte and chunk metadata after the same open log has already passed a
/// complete `RlogReader` replay.
///
/// The caller must pass the header and summary produced by that successful
/// replay and rewind the unchanged file before calling this function. This
/// second pass hashes raw bytes only; it intentionally avoids parsing and
/// canonicalizing every event a second time.
pub fn build_preverified_log_artifact<R: Read + Seek>(
    input: R,
    header: RlogHeader,
    rlog: RlogReplaySummary,
    artifact_limits: ArtifactBuildLimits,
) -> Result<LocalLogArtifact, ArtifactBuildError> {
    let mut tracked = LogArtifactTrackingReader::new(input, artifact_limits)?;
    let copy_result = std::io::copy(&mut tracked, &mut std::io::sink());
    if let Some(actual_at_least) = tracked.inner.limit_exceeded_at {
        return Err(ArtifactBuildError::FileTooLarge {
            actual_at_least,
            maximum: tracked.inner.maximum_file_bytes,
        });
    }
    copy_result?;
    tracked.finish(header, rlog)
}

/// Read adapter that hashes and chunks the exact bytes consumed by an
/// integrity-validating `RlogReader`.
///
/// This lets plug-in replay and upload-artifact construction share one file
/// pass without weakening either the canonical seal or raw-file hashes.
pub struct LogArtifactTrackingReader<R> {
    inner: ArtifactTrackingReader<R>,
    declared_file_length: u64,
}

impl<R: Read + Seek> LogArtifactTrackingReader<R> {
    pub fn new(
        mut input: R,
        artifact_limits: ArtifactBuildLimits,
    ) -> Result<Self, ArtifactBuildError> {
        let artifact_limits = artifact_limits.validate()?;
        let declared_file_length = input.seek(SeekFrom::End(0))?;
        if declared_file_length > artifact_limits.maximum_file_bytes {
            return Err(ArtifactBuildError::FileTooLarge {
                actual_at_least: declared_file_length,
                maximum: artifact_limits.maximum_file_bytes,
            });
        }
        input.seek(SeekFrom::Start(0))?;
        Ok(Self {
            inner: ArtifactTrackingReader::new(
                input,
                artifact_limits.chunk_bytes,
                artifact_limits.maximum_file_bytes,
            ),
            declared_file_length,
        })
    }

    pub fn finish(
        self,
        header: RlogHeader,
        rlog: RlogReplaySummary,
    ) -> Result<LocalLogArtifact, ArtifactBuildError> {
        header.validate()?;
        if let Some(actual_at_least) = self.inner.limit_exceeded_at {
            return Err(ArtifactBuildError::FileTooLarge {
                actual_at_least,
                maximum: self.inner.maximum_file_bytes,
            });
        }
        let tracked = self.inner.finish()?;
        if tracked.chunks.is_empty() {
            return Err(ArtifactBuildError::EmptyArtifact);
        }
        if tracked.file_byte_length != self.declared_file_length {
            return Err(ArtifactBuildError::FileLengthChanged {
                declared: self.declared_file_length,
                observed: tracked.file_byte_length,
            });
        }
        Ok(LocalLogArtifact {
            header,
            rlog,
            file_byte_length: tracked.file_byte_length,
            file_sha256: tracked.file_sha256,
            chunks: tracked.chunks,
        })
    }
}

impl<R: Read> Read for LogArtifactTrackingReader<R> {
    fn read(&mut self, buffer: &mut [u8]) -> std::io::Result<usize> {
        self.inner.read(buffer)
    }
}

struct ArtifactTrackingReader<R> {
    inner: R,
    chunk_bytes: usize,
    maximum_file_bytes: u64,
    limit_exceeded_at: Option<u64>,
    file_hasher: Sha256,
    chunk_hasher: Sha256,
    file_byte_length: u64,
    chunk_offset: u64,
    chunk_length: usize,
    chunks: Vec<LogChunkDescriptor>,
}

impl<R> ArtifactTrackingReader<R> {
    fn new(inner: R, chunk_bytes: usize, maximum_file_bytes: u64) -> Self {
        Self {
            inner,
            chunk_bytes,
            maximum_file_bytes,
            limit_exceeded_at: None,
            file_hasher: Sha256::new(),
            chunk_hasher: Sha256::new(),
            file_byte_length: 0,
            chunk_offset: 0,
            chunk_length: 0,
            chunks: Vec::new(),
        }
    }

    fn observe(&mut self, bytes: &[u8]) -> Result<(), ArtifactBuildError> {
        let byte_length =
            u64::try_from(bytes.len()).map_err(|_| ArtifactBuildError::ByteCountOverflow)?;
        let next_file_byte_length = self
            .file_byte_length
            .checked_add(byte_length)
            .ok_or(ArtifactBuildError::ByteCountOverflow)?;
        if next_file_byte_length > self.maximum_file_bytes {
            self.limit_exceeded_at = Some(next_file_byte_length);
            return Err(ArtifactBuildError::FileTooLarge {
                actual_at_least: next_file_byte_length,
                maximum: self.maximum_file_bytes,
            });
        }
        self.file_byte_length = next_file_byte_length;
        self.file_hasher.update(bytes);

        let mut remaining = bytes;
        while !remaining.is_empty() {
            let available = self.chunk_bytes - self.chunk_length;
            let consumed = available.min(remaining.len());
            self.chunk_hasher.update(&remaining[..consumed]);
            self.chunk_length = self
                .chunk_length
                .checked_add(consumed)
                .ok_or(ArtifactBuildError::ByteCountOverflow)?;
            remaining = &remaining[consumed..];
            if self.chunk_length == self.chunk_bytes {
                self.finish_chunk()?;
            }
        }
        Ok(())
    }

    fn finish_chunk(&mut self) -> Result<(), ArtifactBuildError> {
        if self.chunk_length == 0 {
            return Ok(());
        }
        let sequence =
            u64::try_from(self.chunks.len()).map_err(|_| ArtifactBuildError::ChunkCountOverflow)?;
        let byte_length =
            u64::try_from(self.chunk_length).map_err(|_| ArtifactBuildError::ByteCountOverflow)?;
        let hasher = std::mem::take(&mut self.chunk_hasher);
        self.chunks.push(LogChunkDescriptor::new(
            sequence,
            self.chunk_offset,
            byte_length,
            digest_result(hasher.finalize()),
        ));
        self.chunk_offset = self
            .chunk_offset
            .checked_add(byte_length)
            .ok_or(ArtifactBuildError::ByteCountOverflow)?;
        self.chunk_length = 0;
        Ok(())
    }

    fn finish(mut self) -> Result<TrackedArtifact, ArtifactBuildError> {
        self.finish_chunk()?;
        Ok(TrackedArtifact {
            file_byte_length: self.file_byte_length,
            file_sha256: digest_result(self.file_hasher.finalize()),
            chunks: self.chunks,
        })
    }
}

impl<R: Read> Read for ArtifactTrackingReader<R> {
    fn read(&mut self, buffer: &mut [u8]) -> std::io::Result<usize> {
        let read = self.inner.read(buffer)?;
        self.observe(&buffer[..read])
            .map_err(std::io::Error::other)?;
        Ok(read)
    }
}

struct TrackedArtifact {
    file_byte_length: u64,
    file_sha256: Sha256Digest,
    chunks: Vec<LogChunkDescriptor>,
}

#[cfg(test)]
fn digest_bytes(bytes: &[u8]) -> Sha256Digest {
    digest_result(Sha256::digest(bytes))
}

fn digest_result(digest: impl std::fmt::LowerHex) -> Sha256Digest {
    Sha256Digest::parse(format!("{digest:x}"))
        .expect("SHA-256 formatting always produces a valid 64-character digest")
}

#[derive(Debug, Error)]
pub enum ArtifactBuildError {
    #[error(
        "invalid artifact limits: chunk_bytes={chunk_bytes}, maximum_file_bytes={maximum_file_bytes}"
    )]
    InvalidLimits {
        chunk_bytes: usize,
        maximum_file_bytes: u64,
    },
    #[error("sealed log artifact exceeds {maximum} bytes (observed at least {actual_at_least})")]
    FileTooLarge { actual_at_least: u64, maximum: u64 },
    #[error("sealed log artifact is empty")]
    EmptyArtifact,
    #[error(
        "sealed log artifact length changed during verification (declared {declared}, observed {observed})"
    )]
    FileLengthChanged { declared: u64, observed: u64 },
    #[error("sealed log byte count overflowed")]
    ByteCountOverflow,
    #[error("sealed log chunk count overflowed")]
    ChunkCountOverflow,
    #[error("sealed log artifact I/O failed: {0}")]
    Io(#[from] std::io::Error),
    #[error("sealed canonical log verification failed: {0}")]
    Rlog(#[from] RlogError),
}

#[cfg(test)]
mod tests {
    use std::io::{Cursor, Read, Seek, SeekFrom};

    use rlogs_events::{
        BoundaryReason, CanonicalEvent, CombatState, EVENT_SCHEMA_VERSION, EventEnvelope,
        EventProvenance, EventSensitivity, EventTime, RegionContext, RegionIdentity, TimelineEvent,
        TimelineEventKind,
    };
    use rlogs_log_format::{RlogHeader, RlogWriter};

    use crate::{ReportVisibility, SubmissionMetadata, SubmissionSession};

    use super::*;

    fn region() -> RegionContext {
        RegionContext {
            identity: RegionIdentity {
                deployment_id: "global".into(),
                region_id: "north-america".into(),
                realm_id: None,
                world_id: Some("asteria".into()),
            },
            client_build: "fixture-build".into(),
            protocol_pack_digest: format!("sha256:{}", "a".repeat(64)),
            evidence: Vec::new(),
        }
    }

    fn sealed_log() -> Vec<u8> {
        let header = RlogHeader::new("artifact-session", region(), "unit-test");
        let provenance = EventProvenance::wire(1, 1, 1);
        let envelope = EventEnvelope {
            schema_version: EVENT_SCHEMA_VERSION,
            session_id: "artifact-session".into(),
            sequence: 1,
            region: region(),
            time: EventTime {
                observed_micros: 10,
                game_time_millis: None,
            },
            provenance: provenance.clone(),
            sensitivity: EventSensitivity::PublicGameplay,
            event: CanonicalEvent::Timeline(TimelineEvent {
                sequence: 1,
                time: EventTime {
                    observed_micros: 10,
                    game_time_millis: None,
                },
                provenance,
                kind: TimelineEventKind::CombatBoundary {
                    state: CombatState::Started,
                    reason: BoundaryReason::HostileAction,
                },
            }),
        };
        let mut writer = RlogWriter::new(Vec::new(), header).unwrap();
        writer.push(&envelope).unwrap();
        writer.finish().unwrap()
    }

    fn limits(chunk_bytes: usize, maximum_file_bytes: u64) -> ArtifactBuildLimits {
        ArtifactBuildLimits {
            chunk_bytes,
            maximum_file_bytes,
        }
    }

    struct MisreportedLength {
        inner: Cursor<Vec<u8>>,
        declared_length: u64,
    }

    impl Read for MisreportedLength {
        fn read(&mut self, buffer: &mut [u8]) -> std::io::Result<usize> {
            self.inner.read(buffer)
        }
    }

    impl Seek for MisreportedLength {
        fn seek(&mut self, position: SeekFrom) -> std::io::Result<u64> {
            match position {
                SeekFrom::End(0) => Ok(self.declared_length),
                _ => self.inner.seek(position),
            }
        }
    }

    #[test]
    fn verified_log_is_chunked_deterministically_without_loading_it_as_one_payload() {
        let bytes = sealed_log();
        let artifact = build_sealed_log_artifact(
            Cursor::new(bytes.clone()),
            limits(64, 1024 * 1024),
            RlogLimits::default(),
        )
        .unwrap();

        assert_eq!(artifact.header.session_id, "artifact-session");
        assert_eq!(artifact.rlog.event_count, 1);
        assert_eq!(artifact.file_byte_length, bytes.len() as u64);
        assert_eq!(artifact.file_sha256, digest_bytes(&bytes));
        assert!(artifact.chunks.len() > 1);
        for (index, chunk) in artifact.chunks.iter().enumerate() {
            assert_eq!(chunk.sequence, index as u64);
            assert_eq!(chunk.file_offset, (index * 64) as u64);
            assert!(chunk.byte_length <= 64);
            let start = chunk.file_offset as usize;
            let end = start + chunk.byte_length as usize;
            assert_eq!(chunk.sha256, digest_bytes(&bytes[start..end]));
        }
        assert_eq!(
            artifact
                .chunks
                .iter()
                .map(|chunk| chunk.byte_length)
                .sum::<u64>(),
            artifact.file_byte_length
        );

        let preverified = build_preverified_log_artifact(
            Cursor::new(bytes),
            artifact.header.clone(),
            artifact.rlog.clone(),
            limits(64, 1024 * 1024),
        )
        .unwrap();
        assert_eq!(preverified, artifact);
    }

    #[test]
    fn tampered_or_truncated_logs_fail_before_an_upload_manifest_exists() {
        let bytes = sealed_log();
        let mut tampered = bytes.clone();
        // Compact rlog v2 event JSON is zstd-compressed, so searching the file
        // for an event-kind string is both format-dependent and ineffective.
        // Walk the public framing instead and corrupt the compressed payload.
        assert_eq!(&tampered[..8], b"RLOG\x02\r\n\x1a");
        let header_length = u32::from_le_bytes(tampered[8..12].try_into().unwrap()) as usize;
        let event_block_offset = 12 + header_length;
        assert_eq!(tampered[event_block_offset], 1);
        let compressed_length = u32::from_le_bytes(
            tampered[event_block_offset + 9..event_block_offset + 13]
                .try_into()
                .unwrap(),
        ) as usize;
        let compressed_offset = event_block_offset + 13;
        assert!(compressed_length > 0);
        let event_byte = compressed_offset + compressed_length / 2;
        tampered[event_byte] ^= 0x80;
        assert!(matches!(
            build_sealed_log_artifact(
                Cursor::new(tampered),
                limits(64, 1024 * 1024),
                RlogLimits::default()
            ),
            Err(ArtifactBuildError::Rlog(_))
        ));

        let truncated = &bytes[..bytes.len() - 10];
        assert!(matches!(
            build_sealed_log_artifact(
                Cursor::new(truncated),
                limits(64, 1024 * 1024),
                RlogLimits::default()
            ),
            Err(ArtifactBuildError::Rlog(_))
        ));
    }

    #[test]
    fn artifact_memory_and_file_limits_fail_closed() {
        let bytes = sealed_log();
        assert!(matches!(
            build_sealed_log_artifact(
                Cursor::new(bytes.clone()),
                limits(0, 1024),
                RlogLimits::default()
            ),
            Err(ArtifactBuildError::InvalidLimits { .. })
        ));
        assert!(matches!(
            build_sealed_log_artifact(
                Cursor::new(bytes.clone()),
                limits(MAXIMUM_UPLOAD_CHUNK_BYTES + 1, 1024),
                RlogLimits::default()
            ),
            Err(ArtifactBuildError::InvalidLimits { .. })
        ));
        assert!(matches!(
            build_sealed_log_artifact(Cursor::new(bytes), limits(64, 100), RlogLimits::default()),
            Err(ArtifactBuildError::FileTooLarge { maximum: 100, .. })
        ));
    }

    #[test]
    fn a_file_that_grows_after_the_initial_length_check_still_hits_the_hard_limit() {
        let bytes = sealed_log();
        let maximum = u64::try_from(bytes.len() - 1).unwrap();
        let source = MisreportedLength {
            inner: Cursor::new(bytes),
            declared_length: 1,
        };

        assert!(matches!(
            build_sealed_log_artifact(source, limits(64, maximum), RlogLimits::default()),
            Err(ArtifactBuildError::FileTooLarge {
                maximum: observed_maximum,
                ..
            }) if observed_maximum == maximum
        ));
    }

    #[test]
    fn a_file_length_change_during_verification_is_rejected() {
        let bytes = sealed_log();
        let observed = u64::try_from(bytes.len()).unwrap();
        let source = MisreportedLength {
            inner: Cursor::new(bytes),
            declared_length: observed - 1,
        };

        assert!(matches!(
            build_sealed_log_artifact(
                source,
                limits(64, observed + 1),
                RlogLimits::default()
            ),
            Err(ArtifactBuildError::FileLengthChanged {
                declared,
                observed: actual,
            }) if declared == observed - 1 && actual == observed
        ));
    }

    #[test]
    fn post_run_session_is_derived_from_the_verified_file_artifact() {
        let artifact = build_sealed_log_artifact(
            Cursor::new(sealed_log()),
            limits(64, 1024 * 1024),
            RlogLimits::default(),
        )
        .unwrap();
        let metadata = SubmissionMetadata::new(
            "app.rlogs.game.bpsr",
            "local-log-1",
            artifact.header.schema_version,
            artifact.header.session_id.clone(),
            "north-america",
            artifact.header.region.client_build.clone(),
            digest_bytes(b"protocol-pack"),
            digest_bytes(b"privacy-policy"),
            ReportVisibility::Unlisted,
        );
        let session =
            SubmissionSession::new_post_run_artifact(metadata.clone(), &artifact).unwrap();
        let manifest = session.manifest();
        assert_eq!(manifest.metadata, metadata);
        assert_eq!(manifest.chunks, artifact.chunks);
        assert_eq!(
            manifest.sealed_log_digest,
            Some(artifact.file_sha256.clone())
        );

        let mut wrong_session = metadata.clone();
        wrong_session.capture_session_id = "another-session".into();
        assert!(matches!(
            SubmissionSession::new_post_run_artifact(wrong_session, &artifact),
            Err(crate::SubmissionError::ArtifactSessionMismatch { .. })
        ));

        let mut wrong_format = metadata;
        wrong_format.log_format_version += 1;
        assert!(matches!(
            SubmissionSession::new_post_run_artifact(wrong_format, &artifact),
            Err(crate::SubmissionError::ArtifactFormatMismatch { .. })
        ));
    }
}
