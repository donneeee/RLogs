//! Bounded, deterministic replay execution for trusted bundled plug-ins.

use std::collections::BTreeSet;
use std::io::BufRead;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::time::{Duration, Instant};

use rlogs_events::{EventEnvelope, EventSensitivity, EventTopic};
use rlogs_log_format::{RlogError, RlogHeader, RlogLimits, RlogReader, RlogReplaySummary};
use rlogs_plugin_api::PluginCapability;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use thiserror::Error;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReplayPluginDescriptor {
    pub id: String,
    pub name: String,
    pub version: String,
    pub capabilities: BTreeSet<PluginCapability>,
    pub subscriptions: BTreeSet<EventTopic>,
}

impl ReplayPluginDescriptor {
    pub fn validate(&self) -> Result<(), PluginRuntimeError> {
        if self.id.trim().is_empty()
            || self.name.trim().is_empty()
            || self.version.trim().is_empty()
        {
            return Err(PluginRuntimeError::InvalidDescriptor);
        }
        if !self.subscriptions.is_empty()
            && !self.capabilities.contains(&PluginCapability::EventsRead)
        {
            return Err(PluginRuntimeError::SubscriptionsWithoutEventAccess {
                plugin_id: self.id.clone(),
            });
        }
        if (self.subscriptions.contains(&EventTopic::Encounter)
            || self.subscriptions.contains(&EventTopic::Dungeon))
            && !self
                .capabilities
                .contains(&PluginCapability::EncountersRead)
        {
            return Err(PluginRuntimeError::EncounterSubscriptionWithoutCapability {
                plugin_id: self.id.clone(),
            });
        }
        if self.subscriptions.contains(&EventTopic::CharacterProfile)
            && !self
                .capabilities
                .contains(&PluginCapability::CharacterProfilesRead)
        {
            return Err(PluginRuntimeError::CharacterSubscriptionWithoutCapability {
                plugin_id: self.id.clone(),
            });
        }
        if self.subscriptions.contains(&EventTopic::Chat)
            && !self.capabilities.contains(&PluginCapability::LocalChatRead)
        {
            return Err(PluginRuntimeError::ChatSubscriptionWithoutCapability {
                plugin_id: self.id.clone(),
            });
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PluginRunLimits {
    pub maximum_delivered_events: u64,
    pub maximum_outputs: usize,
    pub maximum_output_bytes: usize,
    pub maximum_callback_micros: u64,
    pub maximum_total_plugin_micros: u64,
}

impl Default for PluginRunLimits {
    fn default() -> Self {
        Self {
            maximum_delivered_events: 2_000_000,
            maximum_outputs: 1_024,
            maximum_output_bytes: 16 * 1024 * 1024,
            maximum_callback_micros: 250_000,
            maximum_total_plugin_micros: 30_000_000,
        }
    }
}

impl PluginRunLimits {
    fn validate(self) -> Result<Self, PluginRuntimeError> {
        if self.maximum_delivered_events == 0
            || self.maximum_outputs == 0
            || self.maximum_output_bytes == 0
            || self.maximum_callback_micros == 0
            || self.maximum_total_plugin_micros == 0
        {
            return Err(PluginRuntimeError::InvalidLimits);
        }
        Ok(self)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PluginDiagnosticLevel {
    Info,
    Warning,
    Error,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum PluginOutput {
    Snapshot {
        schema_id: String,
        schema_version: u16,
        payload: Value,
    },
    Diagnostic {
        level: PluginDiagnosticLevel,
        code: String,
        message: String,
    },
}

pub struct PluginOutputSink<'a> {
    outputs: &'a mut Vec<PluginOutput>,
    total_bytes: &'a mut usize,
    limits: PluginRunLimits,
}

impl PluginOutputSink<'_> {
    pub fn emit(&mut self, output: PluginOutput) -> Result<(), PluginFailure> {
        if self.outputs.len() >= self.limits.maximum_outputs {
            return Err(PluginFailure::OutputCountLimit {
                maximum: self.limits.maximum_outputs,
            });
        }
        let bytes = serde_json::to_vec(&output)?.len();
        let next = self
            .total_bytes
            .checked_add(bytes)
            .ok_or(PluginFailure::OutputByteLimit {
                maximum: self.limits.maximum_output_bytes,
            })?;
        if next > self.limits.maximum_output_bytes {
            return Err(PluginFailure::OutputByteLimit {
                maximum: self.limits.maximum_output_bytes,
            });
        }
        *self.total_bytes = next;
        self.outputs.push(output);
        Ok(())
    }

    pub fn snapshot<T: Serialize>(
        &mut self,
        schema_id: impl Into<String>,
        schema_version: u16,
        value: &T,
    ) -> Result<(), PluginFailure> {
        self.emit(PluginOutput::Snapshot {
            schema_id: schema_id.into(),
            schema_version,
            payload: serde_json::to_value(value)?,
        })
    }

    pub fn diagnostic(
        &mut self,
        level: PluginDiagnosticLevel,
        code: impl Into<String>,
        message: impl Into<String>,
    ) -> Result<(), PluginFailure> {
        self.emit(PluginOutput::Diagnostic {
            level,
            code: code.into(),
            message: message.into(),
        })
    }
}

pub trait ReplayPlugin {
    fn descriptor(&self) -> ReplayPluginDescriptor;

    fn begin(
        &mut self,
        header: &RlogHeader,
        output: &mut PluginOutputSink<'_>,
    ) -> Result<(), PluginFailure>;

    fn on_event(
        &mut self,
        envelope: &EventEnvelope,
        output: &mut PluginOutputSink<'_>,
    ) -> Result<(), PluginFailure>;

    fn finish(&mut self, output: &mut PluginOutputSink<'_>) -> Result<(), PluginFailure>;
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PluginRunMetrics {
    pub events_seen: u64,
    pub events_delivered: u64,
    pub outputs_emitted: usize,
    pub output_bytes: usize,
    pub plugin_elapsed_micros: u64,
    pub wall_elapsed_micros: u64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PluginRunReport {
    pub descriptor: ReplayPluginDescriptor,
    pub rlog: RlogReplaySummary,
    pub metrics: PluginRunMetrics,
    pub outputs: Vec<PluginOutput>,
}

pub fn replay_rlog<R: BufRead, P: ReplayPlugin>(
    input: R,
    plugin: P,
    rlog_limits: RlogLimits,
    plugin_limits: PluginRunLimits,
) -> Result<PluginRunReport, ReplayRunError> {
    let reader = RlogReader::new(input, rlog_limits)?;
    let header = reader.header().clone();
    let mut host = ReplayPluginHost::new(plugin, plugin_limits)?;
    let wall_started = Instant::now();
    host.begin(&header)?;

    let mut runtime_failure = None;
    let rlog_result = reader.replay(|envelope| match host.deliver(envelope) {
        Ok(()) => Ok(()),
        Err(error) => {
            let detail = error.to_string();
            runtime_failure = Some(error);
            Err(detail)
        }
    });
    if let Some(error) = runtime_failure {
        return Err(ReplayRunError::Plugin(error));
    }
    let rlog = rlog_result?;
    host.finish(rlog, wall_started.elapsed())
        .map_err(ReplayRunError::Plugin)
}

struct ReplayPluginHost<P: ReplayPlugin> {
    plugin: P,
    descriptor: ReplayPluginDescriptor,
    limits: PluginRunLimits,
    outputs: Vec<PluginOutput>,
    output_bytes: usize,
    events_seen: u64,
    events_delivered: u64,
    plugin_elapsed: Duration,
}

impl<P: ReplayPlugin> ReplayPluginHost<P> {
    fn new(plugin: P, limits: PluginRunLimits) -> Result<Self, PluginRuntimeError> {
        let limits = limits.validate()?;
        let descriptor = plugin.descriptor();
        descriptor.validate()?;
        Ok(Self {
            plugin,
            descriptor,
            limits,
            outputs: Vec::new(),
            output_bytes: 0,
            events_seen: 0,
            events_delivered: 0,
            plugin_elapsed: Duration::ZERO,
        })
    }

    fn begin(&mut self, header: &RlogHeader) -> Result<(), PluginRuntimeError> {
        let started = Instant::now();
        let result = catch_unwind(AssertUnwindSafe(|| {
            let mut output = PluginOutputSink {
                outputs: &mut self.outputs,
                total_bytes: &mut self.output_bytes,
                limits: self.limits,
            };
            self.plugin.begin(header, &mut output)
        }));
        self.record_callback("begin", started.elapsed(), result)
    }

    fn deliver(&mut self, envelope: &EventEnvelope) -> Result<(), PluginRuntimeError> {
        self.events_seen = self
            .events_seen
            .checked_add(1)
            .ok_or(PluginRuntimeError::EventCounterOverflow)?;
        if !self.should_deliver(envelope) {
            return Ok(());
        }
        if self.events_delivered >= self.limits.maximum_delivered_events {
            return Err(PluginRuntimeError::DeliveredEventLimit {
                plugin_id: self.descriptor.id.clone(),
                maximum: self.limits.maximum_delivered_events,
            });
        }
        self.events_delivered += 1;

        let started = Instant::now();
        let result = catch_unwind(AssertUnwindSafe(|| {
            let mut output = PluginOutputSink {
                outputs: &mut self.outputs,
                total_bytes: &mut self.output_bytes,
                limits: self.limits,
            };
            self.plugin.on_event(envelope, &mut output)
        }));
        self.record_callback("on_event", started.elapsed(), result)
    }

    fn finish(
        mut self,
        rlog: RlogReplaySummary,
        wall_elapsed: Duration,
    ) -> Result<PluginRunReport, PluginRuntimeError> {
        let started = Instant::now();
        let result = catch_unwind(AssertUnwindSafe(|| {
            let mut output = PluginOutputSink {
                outputs: &mut self.outputs,
                total_bytes: &mut self.output_bytes,
                limits: self.limits,
            };
            self.plugin.finish(&mut output)
        }));
        self.record_callback("finish", started.elapsed(), result)?;

        Ok(PluginRunReport {
            descriptor: self.descriptor,
            rlog,
            metrics: PluginRunMetrics {
                events_seen: self.events_seen,
                events_delivered: self.events_delivered,
                outputs_emitted: self.outputs.len(),
                output_bytes: self.output_bytes,
                plugin_elapsed_micros: duration_micros(self.plugin_elapsed),
                wall_elapsed_micros: duration_micros(wall_elapsed),
            },
            outputs: self.outputs,
        })
    }

    fn should_deliver(&self, envelope: &EventEnvelope) -> bool {
        let topic = envelope.event.topic();
        if !self.descriptor.subscriptions.contains(&topic)
            || !self
                .descriptor
                .capabilities
                .contains(&PluginCapability::EventsRead)
        {
            return false;
        }
        if envelope.sensitivity == EventSensitivity::LocalSensitive
            && !self
                .descriptor
                .capabilities
                .contains(&PluginCapability::LocalChatRead)
        {
            return false;
        }
        match topic {
            EventTopic::Encounter => self
                .descriptor
                .capabilities
                .contains(&PluginCapability::EncountersRead),
            EventTopic::CharacterProfile => self
                .descriptor
                .capabilities
                .contains(&PluginCapability::CharacterProfilesRead),
            EventTopic::Chat => self
                .descriptor
                .capabilities
                .contains(&PluginCapability::LocalChatRead),
            _ => true,
        }
    }

    fn record_callback(
        &mut self,
        operation: &'static str,
        elapsed: Duration,
        result: Result<Result<(), PluginFailure>, Box<dyn std::any::Any + Send>>,
    ) -> Result<(), PluginRuntimeError> {
        self.plugin_elapsed = self.plugin_elapsed.saturating_add(elapsed);
        if duration_micros(elapsed) > self.limits.maximum_callback_micros {
            return Err(PluginRuntimeError::CallbackBudgetExceeded {
                plugin_id: self.descriptor.id.clone(),
                operation,
                actual_micros: duration_micros(elapsed),
                maximum_micros: self.limits.maximum_callback_micros,
            });
        }
        if duration_micros(self.plugin_elapsed) > self.limits.maximum_total_plugin_micros {
            return Err(PluginRuntimeError::TotalBudgetExceeded {
                plugin_id: self.descriptor.id.clone(),
                actual_micros: duration_micros(self.plugin_elapsed),
                maximum_micros: self.limits.maximum_total_plugin_micros,
            });
        }
        match result {
            Ok(Ok(())) => Ok(()),
            Ok(Err(source)) => Err(PluginRuntimeError::PluginFailure {
                plugin_id: self.descriptor.id.clone(),
                operation,
                source,
            }),
            Err(_) => Err(PluginRuntimeError::PluginPanicked {
                plugin_id: self.descriptor.id.clone(),
                operation,
            }),
        }
    }
}

fn duration_micros(duration: Duration) -> u64 {
    duration.as_micros().try_into().unwrap_or(u64::MAX)
}

#[derive(Debug, Error)]
pub enum PluginFailure {
    #[error("{0}")]
    Message(String),

    #[error("plug-in output JSON failed: {0}")]
    Json(#[from] serde_json::Error),

    #[error("plug-in exceeded the {maximum}-output limit")]
    OutputCountLimit { maximum: usize },

    #[error("plug-in exceeded the {maximum}-byte output limit")]
    OutputByteLimit { maximum: usize },
}

#[derive(Debug, Error)]
pub enum PluginRuntimeError {
    #[error("plug-in descriptor fields cannot be empty")]
    InvalidDescriptor,

    #[error("plug-in runtime limits must be greater than zero")]
    InvalidLimits,

    #[error("{plugin_id} subscribes to events without events_read")]
    SubscriptionsWithoutEventAccess { plugin_id: String },

    #[error("{plugin_id} subscribes to encounters without encounters_read")]
    EncounterSubscriptionWithoutCapability { plugin_id: String },

    #[error("{plugin_id} subscribes to character profiles without character_profiles_read")]
    CharacterSubscriptionWithoutCapability { plugin_id: String },

    #[error("{plugin_id} subscribes to chat without local_chat_read")]
    ChatSubscriptionWithoutCapability { plugin_id: String },

    #[error("plug-in event counter space is exhausted")]
    EventCounterOverflow,

    #[error("{plugin_id} exceeded the {maximum}-event delivery limit")]
    DeliveredEventLimit { plugin_id: String, maximum: u64 },

    #[error("{plugin_id} {operation} callback took {actual_micros}us; limit is {maximum_micros}us")]
    CallbackBudgetExceeded {
        plugin_id: String,
        operation: &'static str,
        actual_micros: u64,
        maximum_micros: u64,
    },

    #[error("{plugin_id} used {actual_micros}us total; limit is {maximum_micros}us")]
    TotalBudgetExceeded {
        plugin_id: String,
        actual_micros: u64,
        maximum_micros: u64,
    },

    #[error("{plugin_id} failed during {operation}: {source}")]
    PluginFailure {
        plugin_id: String,
        operation: &'static str,
        source: PluginFailure,
    },

    #[error("{plugin_id} panicked during {operation}")]
    PluginPanicked {
        plugin_id: String,
        operation: &'static str,
    },
}

#[derive(Debug, Error)]
pub enum ReplayRunError {
    #[error(transparent)]
    Rlog(#[from] RlogError),

    #[error(transparent)]
    Plugin(#[from] PluginRuntimeError),
}

#[cfg(test)]
mod tests {
    use std::io::{BufReader, Cursor};

    use rlogs_events::{
        BoundaryReason, CanonicalEvent, CombatState, EVENT_SCHEMA_VERSION, EventProvenance,
        EventTime, RegionContext, RegionIdentity, TimelineEvent, TimelineEventKind,
    };
    use rlogs_log_format::{RlogHeader, RlogWriter};

    use super::*;

    fn region() -> RegionContext {
        RegionContext {
            identity: RegionIdentity {
                deployment_id: "global".into(),
                region_id: "test".into(),
                realm_id: None,
                world_id: None,
            },
            client_build: "fixture".into(),
            protocol_pack_digest: "sha256:fixture".into(),
            evidence: Vec::new(),
        }
    }

    fn event(sequence: u64, kind: TimelineEventKind) -> EventEnvelope {
        let time = EventTime {
            observed_micros: sequence * 1_000,
            game_time_millis: None,
        };
        let provenance = EventProvenance::wire(sequence, 1, 1);
        EventEnvelope {
            schema_version: EVENT_SCHEMA_VERSION,
            session_id: "runtime-test".into(),
            sequence,
            region: region(),
            time,
            provenance: provenance.clone(),
            sensitivity: EventSensitivity::PublicGameplay,
            event: CanonicalEvent::Timeline(TimelineEvent {
                sequence,
                time,
                provenance,
                kind,
            }),
        }
    }

    fn rlog() -> Vec<u8> {
        let mut writer = RlogWriter::new(
            Vec::new(),
            RlogHeader::new("runtime-test", region(), "unit-test"),
        )
        .unwrap();
        writer
            .push(&event(
                1,
                TimelineEventKind::CombatBoundary {
                    state: CombatState::Started,
                    reason: BoundaryReason::Manual,
                },
            ))
            .unwrap();
        writer
            .push(&event(
                2,
                TimelineEventKind::CombatBoundary {
                    state: CombatState::Ended,
                    reason: BoundaryReason::Manual,
                },
            ))
            .unwrap();
        writer.finish().unwrap()
    }

    struct CountingPlugin {
        received: u64,
        output_every_event: bool,
    }

    impl ReplayPlugin for CountingPlugin {
        fn descriptor(&self) -> ReplayPluginDescriptor {
            ReplayPluginDescriptor {
                id: "app.rlogs.test.counter".into(),
                name: "Counter".into(),
                version: "0.1.0".into(),
                capabilities: BTreeSet::from([
                    PluginCapability::EventsRead,
                    PluginCapability::EncountersRead,
                ]),
                subscriptions: BTreeSet::from([EventTopic::Encounter]),
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
            output: &mut PluginOutputSink<'_>,
        ) -> Result<(), PluginFailure> {
            self.received += 1;
            if self.output_every_event {
                output.diagnostic(PluginDiagnosticLevel::Info, "event", "received")?;
            }
            Ok(())
        }

        fn finish(&mut self, output: &mut PluginOutputSink<'_>) -> Result<(), PluginFailure> {
            output.snapshot("app.rlogs.test.counter", 1, &self.received)
        }
    }

    #[test]
    fn replay_filters_and_delivers_subscribed_events() {
        let report = replay_rlog(
            BufReader::new(Cursor::new(rlog())),
            CountingPlugin {
                received: 0,
                output_every_event: false,
            },
            RlogLimits::default(),
            PluginRunLimits::default(),
        )
        .unwrap();

        assert_eq!(report.metrics.events_seen, 2);
        assert_eq!(report.metrics.events_delivered, 2);
        assert_eq!(report.metrics.outputs_emitted, 1);
        assert_eq!(
            report.outputs[0].schema_id(),
            Some("app.rlogs.test.counter")
        );
    }

    #[test]
    fn output_limits_fail_the_plugin_without_partial_success() {
        let result = replay_rlog(
            BufReader::new(Cursor::new(rlog())),
            CountingPlugin {
                received: 0,
                output_every_event: true,
            },
            RlogLimits::default(),
            PluginRunLimits {
                maximum_outputs: 1,
                ..PluginRunLimits::default()
            },
        );
        assert!(matches!(
            result,
            Err(ReplayRunError::Plugin(
                PluginRuntimeError::PluginFailure { .. }
            ))
        ));
    }

    trait OutputSchema {
        fn schema_id(&self) -> Option<&str>;
    }

    impl OutputSchema for PluginOutput {
        fn schema_id(&self) -> Option<&str> {
            match self {
                PluginOutput::Snapshot { schema_id, .. } => Some(schema_id),
                PluginOutput::Diagnostic { .. } => None,
            }
        }
    }
}
