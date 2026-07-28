//! Lossless packet evidence and route coverage for RLogs.

mod catalog;
mod coverage;
mod decoder;
mod framer_set;
mod framing;
mod game_schema_v1;
mod journal;
mod pack;
mod packet;
mod privacy;
mod region;
mod route;
mod stream;

pub use catalog::{
    MappingConfidence, MappingProvenance, RouteCatalog, RouteCatalogError, RouteDefinition,
};
pub use coverage::{
    CoverageReport, CoverageSummary, FragmentCoverage, ProtocolFeatureCoverage,
    ProtocolPackCoverageSummary, RouteCoverage,
};
pub use decoder::{
    DecoderKind, ProtocolDecodeBatch, ProtocolDecodeStatus, ProtocolRuntime, ProtocolRuntimeConfig,
    ProtocolRuntimeError,
};
pub use framer_set::{
    BpsrFramerSet, BpsrFramerSetConfig, BpsrFramerSetConfigError, BpsrFramerSetMetrics,
};
pub use framing::{
    BpsrCallLayout, BpsrFrame, BpsrFrameUpLayout, BpsrFramingConfig, BpsrFramingConfigError,
    BpsrFramingEvent, BpsrFramingIssue, BpsrFramingIssueReason, BpsrFramingMetrics,
    BpsrReturnLayout, BpsrStreamFramer,
};
pub use journal::{CaptureSession, GameBuild, JournalError, ProtocolJournal};
pub use pack::{
    PROTOCOL_PACK_SCHEMA_VERSION, ProtocolFeature, ProtocolPack, ProtocolPackDefinition,
    ProtocolPackError, ProtocolPackRegistry, ProtocolPackRegistryError, ProtocolPackRoute,
    ProtocolPackRouteDisposition, ProtocolPackTarget,
};
pub use packet::{
    CaptureAdapter, CaptureGap, CaptureGapKind, CaptureRecord, CaptureRecordDraft,
    CaptureRecordKind, CompressionState, NetworkEndpoint, PacketEnvelope, PacketPayload,
};
pub use privacy::{
    AllowedDataDomain, DecodeDisposition, PrivacyPolicyError, ProhibitedDataClass,
    ProtocolPrivacyPolicy,
};
pub use region::{RegionEndpointRule, RegionResolver, RegionResolverError, ResolvedRegion};
pub use route::{FragmentKind, PacketDirection, RouteKey, RoutedMessage};
pub use stream::{JsonlJournalError, JsonlJournalReader, JsonlJournalSummary, JsonlJournalWriter};
