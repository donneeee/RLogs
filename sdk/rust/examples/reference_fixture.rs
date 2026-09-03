use std::collections::BTreeSet;
use std::error::Error;

use rlogs_plugin_sdk::prelude::*;

const REFERENCE_COMBAT: &[u8] =
    include_bytes!("../../../tests/fixtures/replay/reference-combat.rlog");

#[derive(Default)]
struct FixtureCounter {
    events: u64,
}

impl ReplayPlugin for FixtureCounter {
    fn descriptor(&self) -> ReplayPluginDescriptor {
        ReplayPluginDescriptor {
            id: "app.rlogs.fixture-counter".into(),
            name: "Fixture Counter".into(),
            version: "1.2.3".into(),
            capabilities: BTreeSet::from([
                PluginCapability::EventsRead,
                PluginCapability::EncountersRead,
            ]),
            subscriptions: BTreeSet::from([
                EventTopic::Combat,
                EventTopic::Encounter,
                EventTopic::Actor,
            ]),
        }
    }

    fn begin(&mut self, _: &RlogHeader, _: &mut PluginOutputSink<'_>) -> Result<(), PluginFailure> {
        Ok(())
    }

    fn on_event(
        &mut self,
        _: &EventEnvelope,
        _: &mut PluginOutputSink<'_>,
    ) -> Result<(), PluginFailure> {
        self.events += 1;
        Ok(())
    }

    fn finish(&mut self, output: &mut PluginOutputSink<'_>) -> Result<(), PluginFailure> {
        output.snapshot("app.rlogs.fixture-counter.summary", 1, &self.events)
    }
}

fn main() -> Result<(), Box<dyn Error>> {
    let manifest = PluginManifest::from_toml(
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
    )?;
    let expectations = FixtureExpectations {
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
            "sha256:c22fa0f9be17522b3a4f08df49aa2b329edf06cbdabcdf5e79ddfaee9bcd9c24".into(),
        ),
        output_schemas: vec![OutputSchemaExpectation {
            schema_id: "app.rlogs.fixture-counter.summary".into(),
            schema_version: 1,
            count: 1,
        }],
    };
    let report = run_fixture_suite(
        &manifest,
        [FixtureCase {
            name: "reference-combat",
            bytes: REFERENCE_COMBAT,
            expectations,
        }],
        FixtureCounter::default,
    )?;

    println!("{report:#?}");
    Ok(())
}
