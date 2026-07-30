//! Native live and post-run submission state for RLogs.

mod artifact;
mod mock;
mod model;
mod queue;
mod session;
mod website;

pub use artifact::{
    ArtifactBuildError, ArtifactBuildLimits, DEFAULT_MAXIMUM_LOG_BYTES, DEFAULT_UPLOAD_CHUNK_BYTES,
    LocalLogArtifact, MAXIMUM_UPLOAD_CHUNK_BYTES, build_sealed_log_artifact,
};
pub use mock::{
    MAXIMUM_MOCK_RECEIVER_CHUNKS, MOCK_RECEIVER_SCHEMA_VERSION, MockChunkReceipt,
    MockReceiverError, MockSubmissionReceiver,
};
pub use model::{
    CURRENT_SUBMISSION_SCHEMA, DigestError, LogChunkDescriptor, ReportVisibility,
    ServerReportReceipt, Sha256Digest, SubmissionMetadata, SubmissionMode, SubmissionState,
    UploadManifest, VerificationTier,
};
pub use queue::{
    MAXIMUM_LOCAL_ARTIFACT_PATH_BYTES, QUEUED_SUBMISSION_SCHEMA_VERSION,
    QueuedArtifactVerificationError, QueuedSubmission, QueuedSubmissionError,
    QueuedSubmissionValidationError,
};
pub use session::{SubmissionError, SubmissionSession, SubmissionValidationError};
pub use website::{
    WEBSITE_PAYLOAD_SCHEMA_VERSION, WebsitePayloadEnvelope, WebsitePayloadError,
    WebsitePayloadRequest,
};
