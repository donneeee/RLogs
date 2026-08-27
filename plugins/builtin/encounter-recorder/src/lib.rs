//! Deterministic sealed-log run projection.

use std::collections::BTreeSet;

use rlogs_combat::{RunAnalysis, RunEventSequencePolicy, RunReducerConfig, RunSessionReducer};
use rlogs_events::{EventEnvelope, EventTopic};
use rlogs_log_format::RlogHeader;
use rlogs_plugin_api::PluginCapability;
use rlogs_plugin_runtime::{PluginFailure, PluginOutputSink, ReplayPlugin, ReplayPluginDescriptor};
use serde::{Deserialize, Serialize};

pub const ENCOUNTER_RECORDER_PLUGIN_ID: &str = "app.rlogs.encounter-recorder";
pub const RUN_PROJECTION_SCHEMA_ID: &str = "app.rlogs.encounter-recorder.runs";
pub const RUN_PROJECTION_SCHEMA_VERSION: u16 = 1;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RunProjectionSnapshot {
    pub schema_version: u16,
    pub session_id: String,
    pub deployment_id: String,
    pub region_id: String,
    pub world_id: Option<String>,
    pub client_build: String,
    pub protocol_pack_digest: String,
    pub runs: Vec<RunAnalysis>,
}

pub struct EncounterRecorderPlugin {
    config: RunReducerConfig,
    header: Option<RlogHeader>,
    reducer: Option<RunSessionReducer>,
}

impl EncounterRecorderPlugin {
    pub fn new(mut config: RunReducerConfig) -> Self {
        config.sequence_policy = RunEventSequencePolicy::MonotonicFiltered;
        Self {
            config,
            header: None,
            reducer: None,
        }
    }

    /// Starts an incremental projection for the live capture path.
    pub fn begin_live(&mut self, header: &RlogHeader) {
        self.header = Some(header.clone());
        self.reducer = Some(RunSessionReducer::new(self.config.clone()));
    }

    /// Applies one canonical event without replaying a sealed archive.
    pub fn observe_live(&mut self, envelope: &EventEnvelope) -> Result<(), PluginFailure> {
        self.reducer
            .as_mut()
            .ok_or_else(|| PluginFailure::Message("encounter recorder was not started".into()))?
            .on_event(envelope)
            .map_err(|error| PluginFailure::Message(error.to_string()))
    }

    /// Freezes a history projection while leaving the live reducer available.
    pub fn live_snapshot(&self) -> Result<RunProjectionSnapshot, PluginFailure> {
        let header = self
            .header
            .as_ref()
            .ok_or_else(|| PluginFailure::Message("encounter recorder has no log header".into()))?;
        let runs = self
            .reducer
            .as_ref()
            .ok_or_else(|| PluginFailure::Message("encounter recorder was not started".into()))?
            .clone()
            .finish();
        Ok(RunProjectionSnapshot {
            schema_version: RUN_PROJECTION_SCHEMA_VERSION,
            session_id: header.session_id.clone(),
            deployment_id: header.region.identity.deployment_id.clone(),
            region_id: header.region.identity.region_id.clone(),
            world_id: header.region.identity.world_id.clone(),
            client_build: header.region.client_build.clone(),
            protocol_pack_digest: header.region.protocol_pack_digest.clone(),
            runs,
        })
    }
}

impl Default for EncounterRecorderPlugin {
    fn default() -> Self {
        Self::new(RunReducerConfig::default())
    }
}

impl ReplayPlugin for EncounterRecorderPlugin {
    fn descriptor(&self) -> ReplayPluginDescriptor {
        ReplayPluginDescriptor {
            id: ENCOUNTER_RECORDER_PLUGIN_ID.into(),
            name: "Encounter recorder".into(),
            version: env!("CARGO_PKG_VERSION").into(),
            capabilities: BTreeSet::from([
                PluginCapability::EventsRead,
                PluginCapability::EncountersRead,
            ]),
            subscriptions: BTreeSet::from([
                EventTopic::World,
                EventTopic::Actor,
                EventTopic::Combat,
                EventTopic::Encounter,
                EventTopic::Dungeon,
                EventTopic::DataQuality,
            ]),
        }
    }

    fn begin(
        &mut self,
        header: &RlogHeader,
        _: &mut PluginOutputSink<'_>,
    ) -> Result<(), PluginFailure> {
        self.header = Some(header.clone());
        self.reducer = Some(RunSessionReducer::new(self.config.clone()));
        Ok(())
    }

    fn on_event(
        &mut self,
        envelope: &EventEnvelope,
        _: &mut PluginOutputSink<'_>,
    ) -> Result<(), PluginFailure> {
        self.reducer
            .as_mut()
            .ok_or_else(|| PluginFailure::Message("encounter recorder was not started".into()))?
            .on_event(envelope)
            .map_err(|error| PluginFailure::Message(error.to_string()))
    }

    fn finish(&mut self, output: &mut PluginOutputSink<'_>) -> Result<(), PluginFailure> {
        let header = self
            .header
            .take()
            .ok_or_else(|| PluginFailure::Message("encounter recorder has no log header".into()))?;
        let runs = self
            .reducer
            .take()
            .ok_or_else(|| PluginFailure::Message("encounter recorder was not started".into()))?
            .finish();
        output.snapshot(
            RUN_PROJECTION_SCHEMA_ID,
            RUN_PROJECTION_SCHEMA_VERSION,
            &RunProjectionSnapshot {
                schema_version: RUN_PROJECTION_SCHEMA_VERSION,
                session_id: header.session_id,
                deployment_id: header.region.identity.deployment_id,
                region_id: header.region.identity.region_id,
                world_id: header.region.identity.world_id,
                client_build: header.region.client_build,
                protocol_pack_digest: header.region.protocol_pack_digest,
                runs,
            },
        )
    }
}

#[cfg(test)]
mod tests {
    use std::io::{BufReader, Cursor};

    use rlogs_combat::{ActivityKind, RunSubmissionDisposition};
    use rlogs_events::{
        CanonicalEventDraft, CanonicalEventDraftKind, DataGapEvent, DataGapKind, DungeonEvent,
        DungeonEventKind, DungeonId, EventEnvelopeFactory, EventProvenance, EventSensitivity,
        EventTime, RecorderPauseEvent, RegionContext, RegionIdentity, TimelineEventKind,
        WorldContext,
    };
    use rlogs_log_format::{RlogLimits, RlogWriter};
    use rlogs_plugin_runtime::{PluginOutput, PluginRunLimits, replay_rlog};

    use super::*;

    fn region() -> RegionContext {
        RegionContext {
            identity: RegionIdentity {
                deployment_id: "global".into(),
                region_id: "north-america".into(),
                realm_id: None,
                world_id: Some("asteria".into()),
            },
            client_build: "fixture".into(),
            protocol_pack_digest: "sha256:fixture".into(),
            evidence: Vec::new(),
        }
    }

    #[test]
    fn sealed_projection_retains_pause_and_gap_evidence_across_filtered_events() {
        let region = region();
        let header = RlogHeader::new("run-fixture", region.clone(), "unit-test");
        let mut writer = RlogWriter::new(Vec::new(), header.clone()).unwrap();
        let mut events = EventEnvelopeFactory::new("run-fixture", region);
        let drafts = [
            CanonicalEventDraft {
                time: EventTime {
                    observed_micros: 1_000_000,
                    game_time_millis: None,
                },
                provenance: EventProvenance::wire(1, 1, 1),
                sensitivity: EventSensitivity::PublicGameplay,
                kind: CanonicalEventDraftKind::Dungeon(DungeonEvent {
                    kind: DungeonEventKind::Started,
                    dungeon_id: Some(DungeonId(7001)),
                    instance_id: Some("instance-1".into()),
                    difficulty_id: Some(3),
                    objective_map_key: None,
                    objective_id: None,
                    objective_value: None,
                    objective_complete: None,
                    objective_catalog: None,
                    flow: None,
                }),
            },
            CanonicalEventDraft {
                time: EventTime {
                    observed_micros: 2_000_000,
                    game_time_millis: None,
                },
                provenance: EventProvenance::wire(2, 1, 1),
                sensitivity: EventSensitivity::PublicGameplay,
                kind: CanonicalEventDraftKind::WorldChanged(WorldContext {
                    scene_id: None,
                    map_id: None,
                    line_id: None,
                    scene_instance_id: None,
                    dungeon_instance_id: Some("instance-1".into()),
                }),
            },
            CanonicalEventDraft {
                time: EventTime {
                    observed_micros: 6_000_000,
                    game_time_millis: None,
                },
                provenance: EventProvenance::manual("user requested capture pause"),
                sensitivity: EventSensitivity::PublicGameplay,
                kind: CanonicalEventDraftKind::Timeline(TimelineEventKind::RecorderPause(
                    RecorderPauseEvent {
                        started_micros: 4_000_000,
                        resumed_micros: 6_000_000,
                    },
                )),
            },
            CanonicalEventDraft {
                time: EventTime {
                    observed_micros: 7_000_000,
                    game_time_millis: None,
                },
                provenance: EventProvenance::wire(3, 1, 1),
                sensitivity: EventSensitivity::PublicGameplay,
                kind: CanonicalEventDraftKind::Timeline(TimelineEventKind::DataGap(DataGapEvent {
                    kind: DataGapKind::CaptureDrop,
                    connection_id: Some(1),
                    stream_id: Some(1),
                    detail: "fixture drop".into(),
                })),
            },
            CanonicalEventDraft {
                time: EventTime {
                    observed_micros: 10_000_000,
                    game_time_millis: None,
                },
                provenance: EventProvenance::wire(4, 1, 1),
                sensitivity: EventSensitivity::PublicGameplay,
                kind: CanonicalEventDraftKind::Dungeon(DungeonEvent {
                    kind: DungeonEventKind::Completed,
                    dungeon_id: Some(DungeonId(7001)),
                    instance_id: Some("instance-1".into()),
                    difficulty_id: Some(3),
                    objective_map_key: None,
                    objective_id: None,
                    objective_value: None,
                    objective_complete: None,
                    objective_catalog: None,
                    flow: None,
                }),
            },
        ];
        let mut emitted = Vec::new();
        for draft in drafts {
            let envelope = events.emit(draft).unwrap();
            writer.push(&envelope).unwrap();
            emitted.push(envelope);
        }
        let bytes = writer.finish().unwrap();

        let report = replay_rlog(
            BufReader::new(Cursor::new(bytes)),
            EncounterRecorderPlugin::new(RunReducerConfig {
                activity_kind: ActivityKind::Dungeon,
                ..RunReducerConfig::default()
            }),
            RlogLimits::default(),
            PluginRunLimits::default(),
        )
        .unwrap();

        assert_eq!(report.metrics.events_seen, 5);
        assert_eq!(report.metrics.events_delivered, 5);
        let PluginOutput::Snapshot { payload, .. } = &report.outputs[0] else {
            panic!("expected run projection snapshot");
        };
        let snapshot: RunProjectionSnapshot = serde_json::from_value(payload.clone()).unwrap();
        let mut live = EncounterRecorderPlugin::new(RunReducerConfig {
            activity_kind: ActivityKind::Dungeon,
            ..RunReducerConfig::default()
        });
        live.begin_live(&header);
        for envelope in &emitted {
            live.observe_live(envelope).unwrap();
        }
        assert_eq!(live.live_snapshot().unwrap(), snapshot);
        assert_eq!(snapshot.runs.len(), 1);
        let run = &snapshot.runs[0];
        assert_eq!(run.timing.manual_pause_micros, 2_000_000);
        assert_eq!(run.data_gap_count, 1);
        assert_eq!(
            run.submission_disposition,
            RunSubmissionDisposition::CompletedNeedsReview
        );
    }
}
