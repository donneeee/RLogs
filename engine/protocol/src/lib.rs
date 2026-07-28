//! Lossless packet evidence and route coverage for RLogs.

mod catalog;
mod coverage;
mod journal;
mod packet;
mod privacy;
mod route;
mod stream;

pub use catalog::{
    MappingConfidence, MappingProvenance, RouteCatalog, RouteCatalogError, RouteDefinition,
};
pub use coverage::{CoverageReport, CoverageSummary, FragmentCoverage, RouteCoverage};
pub use journal::{CaptureSession, GameBuild, JournalError, ProtocolJournal};
pub use packet::{
    CaptureAdapter, CaptureGap, CaptureGapKind, CaptureRecord, CaptureRecordDraft,
    CaptureRecordKind, CompressionState, NetworkEndpoint, PacketEnvelope, PacketPayload,
};
pub use privacy::{
    AllowedDataDomain, DecodeDisposition, PrivacyPolicyError, ProhibitedDataClass,
    ProtocolPrivacyPolicy,
};
pub use route::{FragmentKind, PacketDirection, RouteKey, RoutedMessage};
pub use stream::{JsonlJournalError, JsonlJournalReader, JsonlJournalSummary, JsonlJournalWriter};
