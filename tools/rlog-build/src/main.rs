use std::fs;
use std::path::PathBuf;

use rlogs_events::{
    CanonicalEventDraft, CanonicalEventDraftKind, EventEnvelopeFactory, EventProvenance,
    EventSensitivity, EventTime, RegionContext,
};
use rlogs_log_format::{RlogHeader, RlogWriter};
use serde::Deserialize;

const FIXTURE_SOURCE_SCHEMA_VERSION: u16 = 1;

#[derive(Debug, Deserialize)]
struct FixtureSource {
    schema_version: u16,
    session_id: String,
    producer: String,
    region: RegionContext,
    events: Vec<FixtureEvent>,
}

#[derive(Debug, Deserialize)]
struct FixtureEvent {
    observed_micros: u64,
    game_time_millis: Option<i64>,
    kind: CanonicalEventDraftKind,
}

fn main() {
    if let Err(error) = run() {
        eprintln!("rlog build failed: {error}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), Box<dyn std::error::Error>> {
    let mut arguments = std::env::args_os().skip(1);
    let source_path = arguments.next().map(PathBuf::from).ok_or_else(usage)?;
    let output_path = arguments.next().map(PathBuf::from).ok_or_else(usage)?;
    if arguments.next().is_some() {
        return Err(usage().into());
    }
    if output_path.exists() {
        return Err(format!(
            "output already exists; choose a new path: {}",
            output_path.display()
        )
        .into());
    }
    let source: FixtureSource = serde_json::from_slice(&fs::read(&source_path)?)?;
    if source.schema_version != FIXTURE_SOURCE_SCHEMA_VERSION {
        return Err(format!(
            "fixture source schema {} is unsupported",
            source.schema_version
        )
        .into());
    }
    if source.events.is_empty() {
        return Err("fixture source must contain at least one event".into());
    }

    let header = RlogHeader::new(
        source.session_id.clone(),
        source.region.clone(),
        source.producer,
    );
    let mut factory = EventEnvelopeFactory::new(source.session_id, source.region);
    let mut writer = RlogWriter::new(Vec::new(), header)?;
    for (index, event) in source.events.into_iter().enumerate() {
        let capture_sequence = u64::try_from(index)?.saturating_add(1);
        let envelope = factory.emit(CanonicalEventDraft {
            time: EventTime {
                observed_micros: event.observed_micros,
                game_time_millis: event.game_time_millis,
            },
            provenance: EventProvenance::wire(capture_sequence, 1, 1),
            sensitivity: EventSensitivity::PublicGameplay,
            kind: event.kind,
        })?;
        writer.push(&envelope)?;
    }
    let bytes = writer.finish()?;
    if let Some(parent) = output_path.parent()
        && !parent.as_os_str().is_empty()
    {
        fs::create_dir_all(parent)?;
    }
    fs::write(&output_path, bytes)?;
    println!(
        "wrote {} from {}",
        output_path.display(),
        source_path.display()
    );
    Ok(())
}

fn usage() -> String {
    "usage: rlogs-rlog-build <fixture-source.json> <output.rlog>".into()
}
