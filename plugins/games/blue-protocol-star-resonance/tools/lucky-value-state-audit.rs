use std::{
    collections::BTreeMap,
    ffi::OsString,
    fs::File,
    io::{BufReader, BufWriter},
    path::PathBuf,
};

use prost::Message;
use rlogs_game_bpsr::{
    CaptureRecordKind, JsonlJournalReader, PacketDirection, decode_lucky_value_update,
};
use serde::Serialize;

const WORLD_NTF_SERVICE_ID: u64 = 1_664_308_034;
const SYNC_CONTAINER_DATA_METHOD_ID: u32 = 21;
const SYNC_CONTAINER_DIRTY_DATA_METHOD_ID: u32 = 22;

fn main() {
    if let Err(error) = run() {
        eprintln!("lucky-value state audit failed: {error}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), Box<dyn std::error::Error>> {
    let arguments = arguments()?;
    let journal =
        JsonlJournalReader::new(BufReader::new(File::open(&arguments.journal)?)).read()?;
    let mut route_counts = BTreeMap::<String, u64>::new();
    let mut snapshots = Vec::new();
    let mut dirty_snapshots = Vec::new();
    let mut dirty_decode_errors = Vec::new();

    for record in journal.records() {
        let CaptureRecordKind::Packet(packet) = &record.kind else {
            continue;
        };
        let Some(route) = packet.route.map(|route| route.key) else {
            continue;
        };
        if route.direction != PacketDirection::ServerToClient
            || route.service_id != WORLD_NTF_SERVICE_ID
            || !matches!(
                route.method_id,
                SYNC_CONTAINER_DATA_METHOD_ID | SYNC_CONTAINER_DIRTY_DATA_METHOD_ID
            )
        {
            continue;
        }
        let Some(payload) = packet.payload.decode_input() else {
            continue;
        };
        *route_counts.entry(route.method_id.to_string()).or_default() += 1;

        if route.method_id == SYNC_CONTAINER_DATA_METHOD_ID {
            let message = SyncContainerData::decode(payload)?;
            let Some(character) = message.character else {
                continue;
            };
            let Some(manager) = character.lucky_value_mgr else {
                continue;
            };
            let mut values = manager
                .luck_value_info
                .into_iter()
                .map(|(map_key, value)| LuckyValueEntry {
                    map_key,
                    luck_id: value.luck_id,
                    luck_value: value.luck_value,
                    next_time: value.next_time,
                })
                .collect::<Vec<_>>();
            values.sort_unstable_by_key(|value| (value.map_key, value.luck_id));
            snapshots.push(LuckyValueSnapshot {
                record_sequence: record.sequence,
                observed_micros: record.observed_micros,
                character_id: character.character_id,
                init_value: manager.init_value,
                values,
            });
        } else {
            let message = SyncContainerDirtyData::decode(payload)?;
            let Some(stream) = message.data else {
                continue;
            };
            let Some(buffer) = stream.buffer.filter(|buffer| !buffer.is_empty()) else {
                continue;
            };
            match decode_lucky_value_update(&buffer, stream.stream_type.unwrap_or_default()) {
                Ok(Some(update)) => dirty_snapshots.push(DirtyLuckyValueSnapshot {
                    record_sequence: record.sequence,
                    observed_micros: record.observed_micros,
                    replace: update.replace,
                    init_value: update.init_value,
                    upserts: update
                        .upserts
                        .into_iter()
                        .map(|entry| LuckyValueEntry {
                            map_key: entry.map_key,
                            luck_id: entry.luck_id,
                            luck_value: entry.luck_value,
                            next_time: entry.next_time,
                        })
                        .collect(),
                    removals: update.removals,
                }),
                Ok(None) => {}
                Err(error) => dirty_decode_errors.push(DirtyDecodeError {
                    record_sequence: record.sequence,
                    observed_micros: record.observed_micros,
                    error: error.to_string(),
                }),
            }
        }
    }

    let report = AuditReport {
        schema_version: 1,
        journal: arguments.journal.display().to_string(),
        capture_id: journal.session().capture_id.clone(),
        game_build: journal.session().game_build.build_id.clone(),
        route_counts,
        snapshots,
        dirty_snapshots,
        dirty_decode_errors,
        interpretation_policy: InterpretationPolicy {
            lucky_value_mgr_is_combat_critical_or_lucky_state: false,
            absent_snapshot_means_absent_gameplay_state: false,
            dirty_updates_are_decoded: true,
            unresolved_state_is_hidden: false,
        },
    };
    serde_json::to_writer_pretty(BufWriter::new(File::create(&arguments.output)?), &report)?;
    println!(
        "wrote {} full and {} dirty LuckyValueMgr snapshot(s) to {}",
        report.snapshots.len(),
        report.dirty_snapshots.len(),
        arguments.output.display()
    );
    Ok(())
}

#[derive(Clone, PartialEq, Message)]
struct SyncContainerData {
    #[prost(message, optional, tag = "1")]
    character: Option<CharacterSerialize>,
}

#[derive(Clone, PartialEq, Message)]
struct CharacterSerialize {
    #[prost(int64, optional, tag = "1")]
    character_id: Option<i64>,
    #[prost(message, optional, tag = "88")]
    lucky_value_mgr: Option<LuckyValueMgr>,
}

#[derive(Clone, PartialEq, Message)]
struct SyncContainerDirtyData {
    #[prost(message, optional, tag = "1")]
    data: Option<BufferStream>,
}

#[derive(Clone, PartialEq, Message)]
struct BufferStream {
    #[prost(bytes = "vec", optional, tag = "1")]
    buffer: Option<Vec<u8>>,
    #[prost(int32, optional, tag = "2")]
    stream_type: Option<i32>,
}

#[derive(Clone, PartialEq, Message)]
struct LuckyValueMgr {
    #[prost(map = "int32, message", tag = "1")]
    luck_value_info: std::collections::HashMap<i32, LuckyValueInfo>,
    #[prost(bool, optional, tag = "2")]
    init_value: Option<bool>,
}

#[derive(Clone, Copy, PartialEq, Message)]
struct LuckyValueInfo {
    #[prost(int32, optional, tag = "1")]
    luck_id: Option<i32>,
    #[prost(int32, optional, tag = "2")]
    luck_value: Option<i32>,
    #[prost(int64, optional, tag = "3")]
    next_time: Option<i64>,
}

#[derive(Serialize)]
struct AuditReport {
    schema_version: u32,
    journal: String,
    capture_id: String,
    game_build: String,
    route_counts: BTreeMap<String, u64>,
    snapshots: Vec<LuckyValueSnapshot>,
    dirty_snapshots: Vec<DirtyLuckyValueSnapshot>,
    dirty_decode_errors: Vec<DirtyDecodeError>,
    interpretation_policy: InterpretationPolicy,
}

#[derive(Serialize)]
struct LuckyValueSnapshot {
    record_sequence: u64,
    observed_micros: u64,
    character_id: Option<i64>,
    init_value: Option<bool>,
    values: Vec<LuckyValueEntry>,
}

#[derive(Serialize)]
struct DirtyLuckyValueSnapshot {
    record_sequence: u64,
    observed_micros: u64,
    replace: bool,
    init_value: Option<bool>,
    upserts: Vec<LuckyValueEntry>,
    removals: Vec<i32>,
}

#[derive(Serialize)]
struct DirtyDecodeError {
    record_sequence: u64,
    observed_micros: u64,
    error: String,
}

#[derive(Serialize)]
struct LuckyValueEntry {
    map_key: i32,
    luck_id: Option<i32>,
    luck_value: Option<i32>,
    next_time: Option<i64>,
}

#[derive(Serialize)]
struct InterpretationPolicy {
    lucky_value_mgr_is_combat_critical_or_lucky_state: bool,
    absent_snapshot_means_absent_gameplay_state: bool,
    dirty_updates_are_decoded: bool,
    unresolved_state_is_hidden: bool,
}

struct Arguments {
    journal: PathBuf,
    output: PathBuf,
}

fn arguments() -> Result<Arguments, Box<dyn std::error::Error>> {
    let mut values = std::env::args_os().skip(1);
    let journal = required(&mut values, "journal")?;
    let output = required(&mut values, "output")?;
    if values.next().is_some() {
        return Err(
            "usage: rlogs-bpsr-lucky-value-state-audit <journal.jsonl> <output.json>".into(),
        );
    }
    Ok(Arguments {
        journal: PathBuf::from(journal),
        output: PathBuf::from(output),
    })
}

fn required(
    values: &mut impl Iterator<Item = OsString>,
    name: &str,
) -> Result<OsString, Box<dyn std::error::Error>> {
    values
        .next()
        .ok_or_else(|| format!("missing {name}").into())
}
