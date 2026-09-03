//! Public Rust authoring and deterministic fixture-test surface for rLogs plug-ins.
//!
//! The production host remains the authority for sandboxing. This crate gives
//! plug-in authors the same canonical event, manifest, sealed-log, and replay
//! contracts without exposing host-private capture or protocol internals.

use std::collections::{BTreeMap, BTreeSet};
use std::io::{BufReader, Cursor};

use rlogs_log_format::{RlogError, RlogHeader, RlogLimits, RlogReader, RlogReplaySummary};
use rlogs_plugin_api::{ManifestError, PluginCapability, PluginManifest, PluginRuntime};
use rlogs_plugin_runtime::{
    PluginOutput, PluginRunLimits, PluginRuntimeError, ReplayPlugin, ReplayPluginDescriptor,
    ReplayRunError, replay_rlog,
};
use serde::{Deserialize, Serialize};
use thiserror::Error;

pub use rlogs_events as events;
pub use rlogs_log_format as log_format;
pub use rlogs_plugin_api as plugin_api;
pub use rlogs_plugin_runtime as runtime;

/// Common imports for a Rust replay plug-in.
pub mod prelude {
    pub use rlogs_events::{CanonicalEvent, EventEnvelope, EventTopic};
    pub use rlogs_log_format::RlogHeader;
    pub use rlogs_plugin_api::{PluginCapability, PluginManifest};
    pub use rlogs_plugin_runtime::{
        PluginDiagnosticLevel, PluginFailure, PluginOutput, PluginOutputSink, ReplayPlugin,
        ReplayPluginDescriptor,
    };

    pub use crate::{
        FixtureCase, FixtureExpectations, FixtureRunReport, FixtureSuiteReport,
        OutputSchemaExpectation, PluginCompatibilityError, run_fixture_case, run_fixture_suite,
        validate_manifest_descriptor,
    };
}

/// Proves that the package manifest and compiled replay descriptor describe
/// exactly the same identity, permissions, and event subscriptions.
pub fn validate_manifest_descriptor(
    manifest: &PluginManifest,
    descriptor: &ReplayPluginDescriptor,
) -> Result<(), PluginCompatibilityError> {
    manifest.validate()?;
    descriptor.validate()?;
    if !matches!(
        manifest.runtime,
        PluginRuntime::WasmComponent | PluginRuntime::NativeDeveloper
    ) {
        return Err(PluginCompatibilityError::UnsupportedReplayRuntime {
            runtime: manifest.runtime,
        });
    }
    if manifest.id != descriptor.id {
        return Err(PluginCompatibilityError::IdentityMismatch {
            manifest: manifest.id.clone(),
            descriptor: descriptor.id.clone(),
        });
    }
    if manifest.name != descriptor.name {
        return Err(PluginCompatibilityError::NameMismatch {
            manifest: manifest.name.clone(),
            descriptor: descriptor.name.clone(),
        });
    }
    if manifest.version != descriptor.version {
        return Err(PluginCompatibilityError::VersionMismatch {
            manifest: manifest.version.clone(),
            descriptor: descriptor.version.clone(),
        });
    }
    if manifest.capabilities != descriptor.capabilities {
        return Err(PluginCompatibilityError::CapabilityMismatch {
            manifest: manifest.capabilities.clone(),
            descriptor: descriptor.capabilities.clone(),
        });
    }
    if manifest.subscriptions != descriptor.subscriptions {
        return Err(PluginCompatibilityError::SubscriptionMismatch {
            manifest: manifest.subscriptions.clone(),
            descriptor: descriptor.subscriptions.clone(),
        });
    }
    Ok(())
}

#[derive(Debug, Error)]
pub enum PluginCompatibilityError {
    #[error("plug-in manifest is invalid: {0}")]
    Manifest(#[from] ManifestError),

    #[error("plug-in replay descriptor is invalid: {0}")]
    Descriptor(#[from] PluginRuntimeError),

    #[error("{runtime:?} packages cannot consume the in-process replay fixture contract")]
    UnsupportedReplayRuntime { runtime: PluginRuntime },

    #[error("manifest plug-in id {manifest:?} does not match descriptor id {descriptor:?}")]
    IdentityMismatch {
        manifest: String,
        descriptor: String,
    },

    #[error("manifest name {manifest:?} does not match descriptor name {descriptor:?}")]
    NameMismatch {
        manifest: String,
        descriptor: String,
    },

    #[error("manifest version {manifest:?} does not match descriptor version {descriptor:?}")]
    VersionMismatch {
        manifest: String,
        descriptor: String,
    },

    #[error(
        "manifest capabilities {manifest:?} do not match compiled replay capabilities {descriptor:?}"
    )]
    CapabilityMismatch {
        manifest: BTreeSet<PluginCapability>,
        descriptor: BTreeSet<PluginCapability>,
    },

    #[error(
        "manifest subscriptions {manifest:?} do not match compiled replay subscriptions {descriptor:?}"
    )]
    SubscriptionMismatch {
        manifest: BTreeSet<rlogs_events::EventTopic>,
        descriptor: BTreeSet<rlogs_events::EventTopic>,
    },
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct FixtureExpectations {
    pub session_id: Option<String>,
    pub deployment_id: Option<String>,
    pub region_id: Option<String>,
    pub client_build: Option<String>,
    pub protocol_pack_digest: Option<String>,
    pub producer: Option<String>,
    pub event_count: Option<u64>,
    pub delivered_event_count: Option<u64>,
    pub first_observed_micros: Option<u64>,
    pub last_observed_micros: Option<u64>,
    pub content_sha256: Option<String>,
    #[serde(default)]
    pub output_schemas: Vec<OutputSchemaExpectation>,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct OutputSchemaExpectation {
    pub schema_id: String,
    pub schema_version: u16,
    pub count: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FixtureCase<'a> {
    pub name: &'a str,
    pub bytes: &'a [u8],
    pub expectations: FixtureExpectations,
}

/// A replay report with nondeterministic wall-clock measurements removed.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FixtureRunReport {
    pub header: RlogHeader,
    pub descriptor: ReplayPluginDescriptor,
    pub rlog: RlogReplaySummary,
    pub events_seen: u64,
    pub events_delivered: u64,
    pub outputs_emitted: usize,
    pub output_bytes: usize,
    pub outputs: Vec<PluginOutput>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FixtureCaseReport {
    pub name: String,
    pub report: FixtureRunReport,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FixtureSuiteReport {
    pub plugin_id: String,
    pub cases: Vec<FixtureCaseReport>,
}

pub fn run_fixture_case<P: ReplayPlugin>(
    manifest: &PluginManifest,
    bytes: &[u8],
    plugin: P,
    expectations: &FixtureExpectations,
) -> Result<FixtureRunReport, FixtureError> {
    run_fixture_case_with_limits(
        manifest,
        bytes,
        plugin,
        expectations,
        RlogLimits::default(),
        PluginRunLimits::default(),
    )
}

pub fn run_fixture_case_with_limits<P: ReplayPlugin>(
    manifest: &PluginManifest,
    bytes: &[u8],
    plugin: P,
    expectations: &FixtureExpectations,
    rlog_limits: RlogLimits,
    plugin_limits: PluginRunLimits,
) -> Result<FixtureRunReport, FixtureError> {
    validate_expectations(expectations)?;
    let reader = RlogReader::new(BufReader::new(Cursor::new(bytes)), rlog_limits)?;
    let header = reader.header().clone();
    drop(reader);
    validate_header(&header, expectations)?;

    let descriptor = plugin.descriptor();
    validate_manifest_descriptor(manifest, &descriptor)?;
    let report = replay_rlog(
        BufReader::new(Cursor::new(bytes)),
        plugin,
        rlog_limits,
        plugin_limits,
    )?;
    let normalized = FixtureRunReport {
        header,
        descriptor: report.descriptor,
        rlog: report.rlog,
        events_seen: report.metrics.events_seen,
        events_delivered: report.metrics.events_delivered,
        outputs_emitted: report.metrics.outputs_emitted,
        output_bytes: report.metrics.output_bytes,
        outputs: report.outputs,
    };
    validate_report(&normalized, expectations)?;
    Ok(normalized)
}

/// Runs every named fixture twice with fresh plug-in instances and rejects any
/// difference in normalized reports. Case names must be unique and non-empty.
pub fn run_fixture_suite<'a, P, F>(
    manifest: &PluginManifest,
    cases: impl IntoIterator<Item = FixtureCase<'a>>,
    mut plugin_factory: F,
) -> Result<FixtureSuiteReport, FixtureError>
where
    P: ReplayPlugin,
    F: FnMut() -> P,
{
    let mut names = BTreeSet::new();
    let mut reports = Vec::new();
    for case in cases {
        let name = case.name.trim();
        if name.is_empty() {
            return Err(FixtureError::EmptyCaseName);
        }
        if !names.insert(name.to_owned()) {
            return Err(FixtureError::DuplicateCaseName {
                name: name.to_owned(),
            });
        }
        let first = run_fixture_case(manifest, case.bytes, plugin_factory(), &case.expectations)?;
        let second = run_fixture_case(manifest, case.bytes, plugin_factory(), &case.expectations)?;
        if first != second {
            return Err(FixtureError::NondeterministicReplay {
                case: name.to_owned(),
            });
        }
        reports.push(FixtureCaseReport {
            name: name.to_owned(),
            report: first,
        });
    }
    if reports.is_empty() {
        return Err(FixtureError::EmptySuite);
    }
    Ok(FixtureSuiteReport {
        plugin_id: manifest.id.clone(),
        cases: reports,
    })
}

fn validate_expectations(expectations: &FixtureExpectations) -> Result<(), FixtureError> {
    let mut schemas = BTreeSet::new();
    for schema in &expectations.output_schemas {
        if schema.schema_id.trim().is_empty() || schema.schema_version == 0 || schema.count == 0 {
            return Err(FixtureError::InvalidOutputSchemaExpectation);
        }
        if !schemas.insert((schema.schema_id.as_str(), schema.schema_version)) {
            return Err(FixtureError::DuplicateOutputSchemaExpectation {
                schema_id: schema.schema_id.clone(),
                schema_version: schema.schema_version,
            });
        }
    }
    Ok(())
}

fn validate_header(
    header: &RlogHeader,
    expectations: &FixtureExpectations,
) -> Result<(), FixtureError> {
    expect_text(
        "session_id",
        expectations.session_id.as_deref(),
        &header.session_id,
    )?;
    expect_text(
        "deployment_id",
        expectations.deployment_id.as_deref(),
        &header.region.identity.deployment_id,
    )?;
    expect_text(
        "region_id",
        expectations.region_id.as_deref(),
        &header.region.identity.region_id,
    )?;
    expect_text(
        "client_build",
        expectations.client_build.as_deref(),
        &header.region.client_build,
    )?;
    expect_text(
        "protocol_pack_digest",
        expectations.protocol_pack_digest.as_deref(),
        &header.region.protocol_pack_digest,
    )?;
    expect_text(
        "producer",
        expectations.producer.as_deref(),
        &header.producer,
    )
}

fn validate_report(
    report: &FixtureRunReport,
    expectations: &FixtureExpectations,
) -> Result<(), FixtureError> {
    expect_number(
        "event_count",
        expectations.event_count,
        report.rlog.event_count,
    )?;
    expect_number(
        "delivered_event_count",
        expectations.delivered_event_count,
        report.events_delivered,
    )?;
    expect_optional_number(
        "first_observed_micros",
        expectations.first_observed_micros,
        report.rlog.first_observed_micros,
    )?;
    expect_optional_number(
        "last_observed_micros",
        expectations.last_observed_micros,
        report.rlog.last_observed_micros,
    )?;
    expect_text(
        "content_sha256",
        expectations.content_sha256.as_deref(),
        &report.rlog.content_sha256,
    )?;

    let mut actual = BTreeMap::<(String, u16), usize>::new();
    for output in &report.outputs {
        if let PluginOutput::Snapshot {
            schema_id,
            schema_version,
            ..
        } = output
        {
            *actual
                .entry((schema_id.clone(), *schema_version))
                .or_default() += 1;
        }
    }
    for expected in &expectations.output_schemas {
        let count = actual
            .get(&(expected.schema_id.clone(), expected.schema_version))
            .copied()
            .unwrap_or_default();
        if count != expected.count {
            return Err(FixtureError::OutputSchemaCountMismatch {
                schema_id: expected.schema_id.clone(),
                schema_version: expected.schema_version,
                expected: expected.count,
                actual: count,
            });
        }
    }
    if !expectations.output_schemas.is_empty() {
        let expected = expectations
            .output_schemas
            .iter()
            .map(|schema| (schema.schema_id.clone(), schema.schema_version))
            .collect::<BTreeSet<_>>();
        if let Some(((schema_id, schema_version), count)) = actual
            .iter()
            .find(|(schema, _)| !expected.contains(*schema))
        {
            return Err(FixtureError::UnexpectedOutputSchema {
                schema_id: schema_id.clone(),
                schema_version: *schema_version,
                count: *count,
            });
        }
    }
    Ok(())
}

fn expect_text(
    field: &'static str,
    expected: Option<&str>,
    actual: &str,
) -> Result<(), FixtureError> {
    if let Some(expected) = expected
        && expected != actual
    {
        return Err(FixtureError::ExpectationMismatch {
            field,
            expected: expected.to_owned(),
            actual: actual.to_owned(),
        });
    }
    Ok(())
}

fn expect_number(
    field: &'static str,
    expected: Option<u64>,
    actual: u64,
) -> Result<(), FixtureError> {
    if let Some(expected) = expected
        && expected != actual
    {
        return Err(FixtureError::ExpectationMismatch {
            field,
            expected: expected.to_string(),
            actual: actual.to_string(),
        });
    }
    Ok(())
}

fn expect_optional_number(
    field: &'static str,
    expected: Option<u64>,
    actual: Option<u64>,
) -> Result<(), FixtureError> {
    if let Some(expected) = expected
        && Some(expected) != actual
    {
        return Err(FixtureError::ExpectationMismatch {
            field,
            expected: expected.to_string(),
            actual: actual.map_or_else(|| "none".to_owned(), |value| value.to_string()),
        });
    }
    Ok(())
}

#[derive(Debug, Error)]
pub enum FixtureError {
    #[error(transparent)]
    Rlog(#[from] RlogError),

    #[error(transparent)]
    Replay(#[from] ReplayRunError),

    #[error(transparent)]
    Compatibility(#[from] PluginCompatibilityError),

    #[error("fixture case name cannot be empty")]
    EmptyCaseName,

    #[error("fixture suite must contain at least one case")]
    EmptySuite,

    #[error("duplicate fixture case name {name:?}")]
    DuplicateCaseName { name: String },

    #[error("fixture expectation has an empty schema id, zero version, or zero count")]
    InvalidOutputSchemaExpectation,

    #[error("duplicate fixture output schema expectation {schema_id} v{schema_version}")]
    DuplicateOutputSchemaExpectation {
        schema_id: String,
        schema_version: u16,
    },

    #[error("fixture {field} mismatch: expected {expected:?}, got {actual:?}")]
    ExpectationMismatch {
        field: &'static str,
        expected: String,
        actual: String,
    },

    #[error(
        "fixture output {schema_id} v{schema_version} count mismatch: expected {expected}, got {actual}"
    )]
    OutputSchemaCountMismatch {
        schema_id: String,
        schema_version: u16,
        expected: usize,
        actual: usize,
    },

    #[error("fixture emitted unexpected output {schema_id} v{schema_version} {count} time(s)")]
    UnexpectedOutputSchema {
        schema_id: String,
        schema_version: u16,
        count: usize,
    },

    #[error("fixture case {case:?} produced different normalized reports across two fresh runs")]
    NondeterministicReplay { case: String },
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use rlogs_events::{EventEnvelope, EventTopic};
    use rlogs_plugin_api::{PluginCapability, PluginManifest};
    use rlogs_plugin_runtime::{
        PluginFailure, PluginOutputSink, ReplayPlugin, ReplayPluginDescriptor,
    };

    use super::*;

    const REFERENCE_COMBAT: &[u8] =
        include_bytes!("../../../tests/fixtures/replay/reference-combat.rlog");

    #[derive(Default)]
    struct CountingPlugin {
        received: u64,
        output_offset: u64,
        emit_unexpected: bool,
    }

    impl ReplayPlugin for CountingPlugin {
        fn descriptor(&self) -> ReplayPluginDescriptor {
            ReplayPluginDescriptor {
                id: "app.rlogs.fixture-counter".into(),
                name: "Fixture Counter".into(),
                version: "1.2.3".into(),
                capabilities: capabilities(),
                subscriptions: subscriptions(),
            }
        }

        fn begin(
            &mut self,
            _: &RlogHeader,
            _: &mut PluginOutputSink<'_>,
        ) -> Result<(), PluginFailure> {
            Ok(())
        }

        fn on_event(
            &mut self,
            _: &EventEnvelope,
            _: &mut PluginOutputSink<'_>,
        ) -> Result<(), PluginFailure> {
            self.received += 1;
            Ok(())
        }

        fn finish(&mut self, output: &mut PluginOutputSink<'_>) -> Result<(), PluginFailure> {
            output.snapshot(
                "app.rlogs.fixture-counter.summary",
                1,
                &(self.received + self.output_offset),
            )?;
            if self.emit_unexpected {
                output.snapshot("app.rlogs.fixture-counter.unexpected", 1, &true)?;
            }
            Ok(())
        }
    }

    fn capabilities() -> BTreeSet<PluginCapability> {
        BTreeSet::from([
            PluginCapability::EventsRead,
            PluginCapability::EncountersRead,
        ])
    }

    fn subscriptions() -> BTreeSet<EventTopic> {
        BTreeSet::from([EventTopic::Combat, EventTopic::Encounter, EventTopic::Actor])
    }

    fn manifest() -> PluginManifest {
        PluginManifest::from_toml(
            br#"
schema_version = 1
id = "app.rlogs.fixture-counter"
name = "Fixture Counter"
version = "1.2.3"
api_version = 1
runtime = "wasm_component"
entrypoint = "bin/plugin.wasm"
capabilities = ["events_read", "encounters_read"]
subscriptions = ["combat", "encounter", "actor"]
allowed_network_domains = []
"#,
        )
        .unwrap()
    }

    fn expectations() -> FixtureExpectations {
        FixtureExpectations {
            session_id: Some("fixture-reference-combat".into()),
            deployment_id: Some("global".into()),
            region_id: Some("north-america".into()),
            client_build: Some("fixture-not-for-live-use".into()),
            protocol_pack_digest: Some("sha256:fixture-not-for-live-use".into()),
            producer: Some("rlogs-rlog-build/fixture".into()),
            event_count: Some(13),
            delivered_event_count: Some(13),
            first_observed_micros: Some(1_000_000),
            last_observed_micros: Some(12_100_000),
            content_sha256: Some(
                "sha256:be0791004d71f1c1a5270487cfde52e7b3535942f7ecae2bdf67275d03fe0b83".into(),
            ),
            output_schemas: vec![OutputSchemaExpectation {
                schema_id: "app.rlogs.fixture-counter.summary".into(),
                schema_version: 1,
                count: 1,
            }],
        }
    }

    #[test]
    fn reference_fixture_is_sealed_expected_and_deterministic() {
        let suite = run_fixture_suite(
            &manifest(),
            [FixtureCase {
                name: "reference-combat",
                bytes: REFERENCE_COMBAT,
                expectations: expectations(),
            }],
            CountingPlugin::default,
        )
        .unwrap();

        let report = &suite.cases[0].report;
        assert_eq!(report.events_seen, 13);
        assert_eq!(report.events_delivered, 13);
        assert_eq!(report.outputs_emitted, 1);
        assert_eq!(
            report.rlog.content_sha256,
            "sha256:be0791004d71f1c1a5270487cfde52e7b3535942f7ecae2bdf67275d03fe0b83"
        );
        assert_eq!(
            serde_json::to_value(&report.outputs[0]).unwrap()["payload"],
            13
        );
    }

    #[test]
    fn manifest_descriptor_drift_fails_closed_before_replay() {
        let mut drifted_manifest = manifest();
        drifted_manifest.version = "1.2.4".into();
        let error = run_fixture_case(
            &drifted_manifest,
            REFERENCE_COMBAT,
            CountingPlugin::default(),
            &expectations(),
        )
        .unwrap_err();
        assert!(matches!(
            error,
            FixtureError::Compatibility(PluginCompatibilityError::VersionMismatch { .. })
        ));

        let mut drifted_manifest = manifest();
        drifted_manifest.subscriptions.remove(&EventTopic::Actor);
        let error = run_fixture_case(
            &drifted_manifest,
            REFERENCE_COMBAT,
            CountingPlugin::default(),
            &expectations(),
        )
        .unwrap_err();
        assert!(matches!(
            error,
            FixtureError::Compatibility(PluginCompatibilityError::SubscriptionMismatch { .. })
        ));
    }

    #[test]
    fn incorrect_fixture_identity_and_output_contracts_are_explicit() {
        let mut wrong_identity = expectations();
        wrong_identity.client_build = Some("wrong-build".into());
        let error = run_fixture_case(
            &manifest(),
            REFERENCE_COMBAT,
            CountingPlugin::default(),
            &wrong_identity,
        )
        .unwrap_err();
        assert!(matches!(
            error,
            FixtureError::ExpectationMismatch {
                field: "client_build",
                ..
            }
        ));

        let mut wrong_output = expectations();
        wrong_output.output_schemas[0].count = 2;
        let error = run_fixture_case(
            &manifest(),
            REFERENCE_COMBAT,
            CountingPlugin::default(),
            &wrong_output,
        )
        .unwrap_err();
        assert!(matches!(
            error,
            FixtureError::OutputSchemaCountMismatch { .. }
        ));
    }

    #[test]
    fn empty_suite_cannot_report_a_false_success() {
        let error = run_fixture_suite(
            &manifest(),
            std::iter::empty::<FixtureCase<'static>>(),
            CountingPlugin::default,
        )
        .unwrap_err();
        assert!(matches!(error, FixtureError::EmptySuite));
    }

    #[test]
    fn unexpected_output_schema_fails_closed() {
        let error = run_fixture_case(
            &manifest(),
            REFERENCE_COMBAT,
            CountingPlugin {
                emit_unexpected: true,
                ..CountingPlugin::default()
            },
            &expectations(),
        )
        .unwrap_err();
        assert!(matches!(error, FixtureError::UnexpectedOutputSchema { .. }));
    }

    #[test]
    fn suite_rejects_nondeterministic_fresh_instances() {
        let mut offset = 0;
        let error = run_fixture_suite(
            &manifest(),
            [FixtureCase {
                name: "reference-combat",
                bytes: REFERENCE_COMBAT,
                expectations: expectations(),
            }],
            || {
                let plugin = CountingPlugin {
                    output_offset: offset,
                    ..CountingPlugin::default()
                };
                offset += 1;
                plugin
            },
        )
        .unwrap_err();
        assert!(matches!(error, FixtureError::NondeterministicReplay { .. }));
    }
}
