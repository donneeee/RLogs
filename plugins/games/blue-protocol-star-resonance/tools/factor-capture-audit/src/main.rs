use std::{
    collections::{BTreeMap, BTreeSet, HashMap},
    ffi::{OsStr, OsString},
    fs::File,
    io::{BufReader, BufWriter, Write},
    path::PathBuf,
};

use prost::Message;
use rlogs_game_bpsr::{
    CaptureRecordKind, FragmentKind, JsonlJournalReader, PacketDirection, RouteKey,
};
use serde::{Deserialize, Serialize};

const DIRTY_ROUTE: RouteKey = RouteKey::new(
    PacketDirection::ServerToClient,
    FragmentKind::Notify,
    1_664_308_034,
    22,
);
const FULL_SYNC_ROUTE: RouteKey = RouteKey::new(
    PacketDirection::ServerToClient,
    FragmentKind::Notify,
    1_664_308_034,
    21,
);
const DIRTY_TREE_DELIMITER: u32 = 0xDEAD_BEEF;

fn main() {
    if let Err(error) = run() {
        eprintln!("factor capture audit failed: {error}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), Box<dyn std::error::Error>> {
    let arguments = arguments()?;
    let factor_catalog: FactorCatalog =
        serde_json::from_reader(BufReader::new(File::open(&arguments.factors)?))?;
    let reference_summary = factor_catalog.summary.clone();
    let factor_items = factor_catalog
        .factor_items_by_id
        .into_iter()
        .filter_map(|(key, item)| key.parse::<i64>().ok().map(|id| (id, item)))
        .collect::<HashMap<_, _>>();
    let journal =
        JsonlJournalReader::new(BufReader::new(File::open(&arguments.journal)?)).read()?;

    let mut summary = AuditSummary {
        capture_id: journal.session().capture_id.clone(),
        game_build: journal.session().game_build.build_id.clone(),
        reference_source: reference_summary.source,
        reference_source_path: reference_summary.source_path,
        reference_declared_build: reference_summary.client_build,
        reference_is_authoritative: false,
        factor_catalog_items: factor_items.len(),
        ..AuditSummary::default()
    };
    let mut matches: BTreeMap<MatchKey, MatchedFactorNode> = BTreeMap::new();
    let mut raw_matches: BTreeMap<RawMatchKey, RawMatchedFactor> = BTreeMap::new();
    let mut owned_items: BTreeMap<OwnedItemKey, OwnedFactorItem> = BTreeMap::new();
    let mut full_sync_proto_matches: BTreeMap<FullSyncProtoMatchKey, FullSyncProtoMatch> =
        BTreeMap::new();
    let mut cultivation_middle_nodes: BTreeMap<CultivationNodeKey, CultivationMiddleNodeAudit> =
        BTreeMap::new();
    let mut cultivation_big_nodes: BTreeMap<CultivationNodeKey, CultivationBigNodeAudit> =
        BTreeMap::new();
    let mut cultivation_areas: BTreeMap<CultivationAreaKey, CultivationAreaAudit> = BTreeMap::new();

    for record in journal.records() {
        let CaptureRecordKind::Packet(packet) = &record.kind else {
            continue;
        };
        let Some(route) = packet.route.map(|route| route.key) else {
            continue;
        };
        if route != DIRTY_ROUTE && route != FULL_SYNC_ROUTE {
            continue;
        }
        let Some(payload) = packet.payload.decode_input() else {
            summary.missing_payload_packets = summary.missing_payload_packets.saturating_add(1);
            continue;
        };
        if route == FULL_SYNC_ROUTE {
            summary.full_sync_packets = summary.full_sync_packets.saturating_add(1);
            let message = match SyncContainerData::decode(payload) {
                Ok(message) => message,
                Err(_) => {
                    summary.full_sync_protobuf_failures =
                        summary.full_sync_protobuf_failures.saturating_add(1);
                    continue;
                }
            };
            let mut snapshot_owned_items = Vec::new();
            if let Some(character) = message.character {
                let current_season_id = character
                    .season_center
                    .as_ref()
                    .and_then(|season| season.season_id);
                if let Some(season_id) = current_season_id {
                    *summary
                        .full_sync_current_season_id_counts
                        .entry(season_id)
                        .or_default() += 1;
                }
                if let Some(cultivation) = character.season_cultivation.as_ref() {
                    summary.full_sync_packets_with_cultivation =
                        summary.full_sync_packets_with_cultivation.saturating_add(1);
                    collect_cultivation_nodes(
                        cultivation,
                        current_season_id,
                        record.sequence,
                        &factor_items,
                        &mut cultivation_areas,
                        &mut cultivation_middle_nodes,
                        &mut cultivation_big_nodes,
                    );
                }
                if let Some(packages) = character.item_package {
                    for (package_key, package) in packages.packages {
                        for item in package.items.into_values() {
                            let Some(item_config_id) = item.item_id.map(i64::from) else {
                                continue;
                            };
                            let Some(factor) = factor_items.get(&item_config_id) else {
                                continue;
                            };
                            let item_uuid = item.uuid.unwrap_or_default();
                            snapshot_owned_items.push(ProtoFactorNeedleSource {
                                item_config_id,
                                item_uuid,
                                family_id: factor.family_id,
                                primary_buff_id: factor.primary_buff_id,
                                grade: factor.grade,
                                slot_category: factor.slot_category.clone(),
                            });
                            let key = OwnedItemKey {
                                item_config_id,
                                item_uuid,
                                package_key,
                            };
                            owned_items
                                .entry(key)
                                .and_modify(|entry| {
                                    entry.occurrences = entry.occurrences.saturating_add(1);
                                    entry.last_sequence = record.sequence;
                                })
                                .or_insert_with(|| OwnedFactorItem {
                                    item_config_id,
                                    item_uuid,
                                    package_key,
                                    family_id: factor.family_id,
                                    family_name: factor.family_name.clone(),
                                    grade: factor.grade,
                                    slot_category: factor.slot_category.clone(),
                                    runtime_role: factor.runtime_role.clone(),
                                    primary_buff_id: factor.primary_buff_id,
                                    occurrences: 1,
                                    first_sequence: record.sequence,
                                    last_sequence: record.sequence,
                                    proves_selection: false,
                                });
                        }
                    }
                }
            }
            let needles = factor_proto_needles(&snapshot_owned_items);
            if let Some(packet_matches) =
                collect_proto_matches(payload, 0, payload.len(), "$", 0, 14, 1_000_000, &needles)
            {
                for packet_match in packet_matches {
                    for needle in packet_match.needles {
                        let key = FullSyncProtoMatchKey {
                            kind: needle.kind.clone(),
                            value: needle.value,
                            path: packet_match.path.clone(),
                        };
                        full_sync_proto_matches
                            .entry(key)
                            .and_modify(|entry| {
                                entry.occurrences = entry.occurrences.saturating_add(1);
                                entry.last_sequence = record.sequence;
                            })
                            .or_insert_with(|| FullSyncProtoMatch {
                                kind: needle.kind,
                                value: needle.value,
                                item_config_id: needle.item_config_id,
                                family_id: needle.family_id,
                                primary_buff_id: needle.primary_buff_id,
                                grade: needle.grade,
                                slot_category: needle.slot_category,
                                path: packet_match.path.clone(),
                                context: classify_full_sync_proto_path(&packet_match.path).into(),
                                outside_known_inventory_path: !is_known_inventory_path(
                                    &packet_match.path,
                                ),
                                proves_selection: false,
                                occurrences: 1,
                                first_sequence: record.sequence,
                                last_sequence: record.sequence,
                            });
                    }
                }
            }
            continue;
        }
        summary.route_packets = summary.route_packets.saturating_add(1);
        let message = match SyncContainerDirtyData::decode(payload) {
            Ok(message) => message,
            Err(_) => {
                summary.protobuf_failures = summary.protobuf_failures.saturating_add(1);
                continue;
            }
        };
        let Some(stream) = message.data else {
            summary.missing_stream_packets = summary.missing_stream_packets.saturating_add(1);
            continue;
        };
        let stream_type = stream.stream_type.unwrap_or_default();
        *summary.stream_type_counts.entry(stream_type).or_default() += 1;
        let Some(buffer) = stream.buffer.filter(|buffer| !buffer.is_empty()) else {
            summary.empty_buffer_packets = summary.empty_buffer_packets.saturating_add(1);
            continue;
        };
        *summary
            .buffer_length_counts
            .entry(buffer.len())
            .or_default() += 1;

        let tokens = dirty_tree_tokens(&buffer);
        if !tokens.is_empty() {
            summary.dirty_tree_packets = summary.dirty_tree_packets.saturating_add(1);
            summary.dirty_tree_scalar_tokens = summary
                .dirty_tree_scalar_tokens
                .saturating_add(tokens.len() as u64);
        }
        let nodes = dirty_tree_value_nodes(&tokens);
        summary.dirty_tree_value_nodes = summary
            .dirty_tree_value_nodes
            .saturating_add(nodes.len() as u64);
        for node in nodes {
            let Some(item) = factor_items.get(&node.value) else {
                continue;
            };
            let key = MatchKey {
                item_config_id: node.value,
                path: node.path.clone(),
                tree_signature: node.tree_signature.clone(),
                offset: node.offset,
                buffer_length: buffer.len(),
            };
            matches
                .entry(key)
                .and_modify(|entry| {
                    entry.occurrences = entry.occurrences.saturating_add(1);
                    entry.last_sequence = record.sequence;
                })
                .or_insert_with(|| MatchedFactorNode {
                    item_config_id: node.value,
                    family_id: item.family_id,
                    family_name: item.family_name.clone(),
                    grade: item.grade,
                    slot_category: item.slot_category.clone(),
                    runtime_role: item.runtime_role.clone(),
                    path: node.path,
                    tree_signature: node.tree_signature,
                    offset: node.offset,
                    buffer_length: buffer.len(),
                    occurrences: 1,
                    first_sequence: record.sequence,
                    last_sequence: record.sequence,
                });
        }

        for offset in 0..buffer.len().saturating_sub(3) {
            let bytes: [u8; 4] = buffer[offset..offset + 4]
                .try_into()
                .expect("four-byte scan window");
            let item_config_id = i64::from(u32::from_le_bytes(bytes));
            let Some(item) = factor_items.get(&item_config_id) else {
                continue;
            };
            let key = RawMatchKey {
                item_config_id,
                offset,
                buffer_length: buffer.len(),
            };
            raw_matches
                .entry(key)
                .and_modify(|entry| {
                    entry.occurrences = entry.occurrences.saturating_add(1);
                    entry.last_sequence = record.sequence;
                })
                .or_insert_with(|| RawMatchedFactor {
                    item_config_id,
                    family_id: item.family_id,
                    family_name: item.family_name.clone(),
                    grade: item.grade,
                    slot_category: item.slot_category.clone(),
                    runtime_role: item.runtime_role.clone(),
                    offset,
                    buffer_length: buffer.len(),
                    occurrences: 1,
                    first_sequence: record.sequence,
                    last_sequence: record.sequence,
                });
        }
    }

    let dirty_needles = factor_binary_needles(&owned_items, &factor_items);
    let mut dirty_binary_matches: BTreeMap<DirtyBinaryMatchKey, DirtyBinaryMatch> = BTreeMap::new();
    for record in journal.records() {
        let CaptureRecordKind::Packet(packet) = &record.kind else {
            continue;
        };
        if packet.route.map(|route| route.key) != Some(DIRTY_ROUTE) {
            continue;
        }
        let Some(payload) = packet.payload.decode_input() else {
            continue;
        };
        let Ok(message) = SyncContainerDirtyData::decode(payload) else {
            continue;
        };
        let Some(stream) = message.data else {
            continue;
        };
        let stream_type = stream.stream_type.unwrap_or_default();
        let Some(buffer) = stream.buffer.filter(|buffer| !buffer.is_empty()) else {
            continue;
        };
        for needle in &dirty_needles {
            for offset in find_all_subslices(&buffer, &needle.encoded) {
                let key = DirtyBinaryMatchKey {
                    kind: needle.kind.clone(),
                    value: needle.value,
                    encoding: needle.encoding.clone(),
                    offset,
                    buffer_length: buffer.len(),
                    stream_type,
                };
                dirty_binary_matches
                    .entry(key)
                    .and_modify(|entry| {
                        entry.occurrences = entry.occurrences.saturating_add(1);
                        entry.last_sequence = record.sequence;
                    })
                    .or_insert_with(|| DirtyBinaryMatch {
                        kind: needle.kind.clone(),
                        value: needle.value,
                        item_config_id: needle.item_config_id,
                        family_id: needle.family_id,
                        primary_buff_id: needle.primary_buff_id,
                        grade: needle.grade,
                        slot_category: needle.slot_category.clone(),
                        encoding: needle.encoding.clone(),
                        offset,
                        buffer_length: buffer.len(),
                        stream_type,
                        proves_selection: false,
                        occurrences: 1,
                        first_sequence: record.sequence,
                        last_sequence: record.sequence,
                    });
            }
        }
    }

    summary.matched_nodes = matches.into_values().collect();
    summary.unique_matched_item_ids = summary
        .matched_nodes
        .iter()
        .map(|node| node.item_config_id)
        .collect::<std::collections::BTreeSet<_>>()
        .len();
    summary.raw_candidate_matches = raw_matches.into_values().collect();
    summary.unique_raw_candidate_item_ids = summary
        .raw_candidate_matches
        .iter()
        .map(|node| node.item_config_id)
        .collect::<std::collections::BTreeSet<_>>()
        .len();
    summary.owned_factor_items = owned_items.into_values().collect();
    summary.unique_owned_factor_item_ids = summary
        .owned_factor_items
        .iter()
        .map(|item| item.item_config_id)
        .collect::<std::collections::BTreeSet<_>>()
        .len();
    summary.full_sync_proto_matches = full_sync_proto_matches.into_values().collect();
    summary.full_sync_proto_match_count = summary.full_sync_proto_matches.len();
    summary.full_sync_proto_matches_outside_inventory = summary
        .full_sync_proto_matches
        .iter()
        .filter(|row| row.outside_known_inventory_path)
        .count();
    summary.full_sync_proto_match_context_counts =
        summary
            .full_sync_proto_matches
            .iter()
            .fold(BTreeMap::new(), |mut counts, row| {
                *counts.entry(row.context.clone()).or_default() += 1;
                counts
            });
    summary.dirty_binary_matches = dirty_binary_matches.into_values().collect();
    summary.dirty_binary_match_count = summary.dirty_binary_matches.len();
    summary.cultivation_middle_nodes = cultivation_middle_nodes.into_values().collect();
    summary.cultivation_big_nodes = cultivation_big_nodes.into_values().collect();
    summary.cultivation_areas = cultivation_areas.into_values().collect();
    if let Some(output) = arguments.output {
        let mut writer = BufWriter::new(File::create(output)?);
        serde_json::to_writer_pretty(&mut writer, &summary)?;
        writeln!(writer)?;
    } else {
        serde_json::to_writer_pretty(std::io::stdout().lock(), &summary)?;
        println!();
    }
    Ok(())
}

#[derive(Debug, Deserialize)]
struct FactorCatalog {
    #[serde(default)]
    summary: FactorCatalogSummary,
    #[serde(default, rename = "factorItemsById")]
    factor_items_by_id: HashMap<String, FactorItem>,
}

#[derive(Debug, Clone, Default, Deserialize)]
struct FactorCatalogSummary {
    #[serde(default)]
    source: Option<String>,
    #[serde(default, rename = "sourcePath")]
    source_path: Option<String>,
    #[serde(default, rename = "clientBuild")]
    client_build: Option<String>,
}

#[derive(Debug, Deserialize)]
struct FactorItem {
    #[serde(default, rename = "familyId")]
    family_id: Option<i64>,
    #[serde(default, rename = "familyName")]
    family_name: Option<String>,
    #[serde(default)]
    grade: Option<i32>,
    #[serde(default, rename = "slotCategory")]
    slot_category: Option<String>,
    #[serde(default, rename = "runtimeRole")]
    runtime_role: Option<String>,
    #[serde(default, rename = "primaryBuffId")]
    primary_buff_id: Option<i64>,
}

#[derive(Clone, PartialEq, Message)]
struct SyncContainerData {
    #[prost(message, optional, tag = "1")]
    character: Option<AuditCharacterSerialize>,
}

#[derive(Clone, PartialEq, Message)]
struct AuditCharacterSerialize {
    #[prost(message, optional, tag = "7")]
    item_package: Option<AuditItemPackage>,
    #[prost(message, optional, tag = "50")]
    season_center: Option<AuditSeasonCenter>,
    #[prost(message, optional, tag = "101")]
    season_cultivation: Option<AuditSeasonCultivateLineData>,
}

#[derive(Clone, Copy, PartialEq, Message)]
struct AuditSeasonCenter {
    #[prost(int32, optional, tag = "1")]
    season_id: Option<i32>,
}

#[derive(Clone, PartialEq, Message)]
struct AuditSeasonCultivateLineData {
    #[prost(map = "int32, message", tag = "1")]
    seasons: HashMap<i32, AuditCultivateLine>,
}

#[derive(Clone, PartialEq, Message)]
struct AuditCultivateLine {
    #[prost(map = "int32, message", tag = "1")]
    lines: HashMap<i32, AuditCultivateLineSubtype>,
}

#[derive(Clone, PartialEq, Message)]
struct AuditCultivateLineSubtype {
    #[prost(map = "int32, message", tag = "1")]
    areas: HashMap<i32, AuditCultivateArea>,
    #[prost(int32, repeated, tag = "2")]
    area_ids: Vec<i32>,
}

#[derive(Clone, PartialEq, Message)]
struct AuditCultivateArea {
    #[prost(map = "int32, message", tag = "2")]
    middle_nodes: HashMap<i32, AuditCultivateMiddleNode>,
    #[prost(map = "int32, message", tag = "3")]
    big_nodes: HashMap<i32, AuditCultivateBigNode>,
    #[prost(int32, optional, tag = "4")]
    active_effect_score: Option<i32>,
    #[prost(bool, optional, tag = "5")]
    active: Option<bool>,
}

#[derive(Clone, Copy, PartialEq, Message)]
struct AuditCultivateMiddleNode {
    #[prost(int32, optional, tag = "1")]
    item_id: Option<i32>,
}

#[derive(Clone, Copy, PartialEq, Message)]
struct AuditCultivateBigNode {
    #[prost(int32, optional, tag = "1")]
    fantasy_id: Option<i32>,
}

#[derive(Clone, PartialEq, Message)]
struct AuditItemPackage {
    #[prost(map = "int32, message", tag = "1")]
    packages: HashMap<i32, AuditItemPackageSection>,
}

#[derive(Clone, PartialEq, Message)]
struct AuditItemPackageSection {
    #[prost(map = "int64, message", tag = "4")]
    items: HashMap<i64, AuditItemRecord>,
}

#[derive(Clone, Copy, PartialEq, Message)]
struct AuditItemRecord {
    #[prost(int64, optional, tag = "1")]
    uuid: Option<i64>,
    #[prost(int32, optional, tag = "2")]
    item_id: Option<i32>,
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

#[derive(Debug, Default, Serialize)]
struct AuditSummary {
    capture_id: String,
    game_build: String,
    reference_source: Option<String>,
    reference_source_path: Option<String>,
    reference_declared_build: Option<String>,
    reference_is_authoritative: bool,
    factor_catalog_items: usize,
    full_sync_packets: u64,
    full_sync_protobuf_failures: u64,
    full_sync_packets_with_cultivation: u64,
    full_sync_current_season_id_counts: BTreeMap<i32, u64>,
    route_packets: u64,
    missing_payload_packets: u64,
    protobuf_failures: u64,
    missing_stream_packets: u64,
    empty_buffer_packets: u64,
    dirty_tree_packets: u64,
    dirty_tree_scalar_tokens: u64,
    dirty_tree_value_nodes: u64,
    stream_type_counts: BTreeMap<i32, u64>,
    buffer_length_counts: BTreeMap<usize, u64>,
    unique_matched_item_ids: usize,
    matched_nodes: Vec<MatchedFactorNode>,
    unique_raw_candidate_item_ids: usize,
    raw_candidate_matches: Vec<RawMatchedFactor>,
    unique_owned_factor_item_ids: usize,
    owned_factor_items: Vec<OwnedFactorItem>,
    full_sync_proto_match_count: usize,
    full_sync_proto_matches_outside_inventory: usize,
    full_sync_proto_match_context_counts: BTreeMap<String, usize>,
    full_sync_proto_matches: Vec<FullSyncProtoMatch>,
    dirty_binary_match_count: usize,
    dirty_binary_matches: Vec<DirtyBinaryMatch>,
    cultivation_middle_nodes: Vec<CultivationMiddleNodeAudit>,
    cultivation_big_nodes: Vec<CultivationBigNodeAudit>,
    cultivation_areas: Vec<CultivationAreaAudit>,
}

#[derive(Debug, Clone)]
struct ProtoFactorNeedleSource {
    item_config_id: i64,
    item_uuid: i64,
    family_id: Option<i64>,
    primary_buff_id: Option<i64>,
    grade: Option<i32>,
    slot_category: Option<String>,
}

#[derive(Debug, Clone)]
struct ProtoNeedle {
    kind: String,
    value: u64,
    item_config_id: i64,
    family_id: Option<i64>,
    primary_buff_id: Option<i64>,
    grade: Option<i32>,
    slot_category: Option<String>,
}

#[derive(Debug)]
struct ProtoPacketMatch {
    path: String,
    needles: Vec<ProtoNeedle>,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct FullSyncProtoMatchKey {
    kind: String,
    value: u64,
    path: String,
}

#[derive(Debug, Serialize)]
struct FullSyncProtoMatch {
    kind: String,
    value: u64,
    item_config_id: i64,
    family_id: Option<i64>,
    primary_buff_id: Option<i64>,
    grade: Option<i32>,
    slot_category: Option<String>,
    path: String,
    context: String,
    outside_known_inventory_path: bool,
    proves_selection: bool,
    occurrences: u64,
    first_sequence: u64,
    last_sequence: u64,
}

#[derive(Debug, Clone)]
struct BinaryNeedle {
    kind: String,
    value: u64,
    item_config_id: i64,
    family_id: Option<i64>,
    primary_buff_id: Option<i64>,
    grade: Option<i32>,
    slot_category: Option<String>,
    encoding: String,
    encoded: Vec<u8>,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct DirtyBinaryMatchKey {
    kind: String,
    value: u64,
    encoding: String,
    offset: usize,
    buffer_length: usize,
    stream_type: i32,
}

#[derive(Debug, Serialize)]
struct DirtyBinaryMatch {
    kind: String,
    value: u64,
    item_config_id: i64,
    family_id: Option<i64>,
    primary_buff_id: Option<i64>,
    grade: Option<i32>,
    slot_category: Option<String>,
    encoding: String,
    offset: usize,
    buffer_length: usize,
    stream_type: i32,
    proves_selection: bool,
    occurrences: u64,
    first_sequence: u64,
    last_sequence: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct CultivationNodeKey {
    season_id: i32,
    line_type: i32,
    area_id: i32,
    node_id: i32,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct CultivationAreaKey {
    season_id: i32,
    line_type: i32,
    area_id: i32,
}

#[derive(Debug, Serialize)]
struct CultivationAreaAudit {
    season_id: i32,
    line_type: i32,
    area_id: i32,
    current_season: Option<bool>,
    listed_by_subtype: bool,
    active: Option<bool>,
    active_effect_score: Option<i32>,
    middle_node_count: usize,
    big_node_count: usize,
    occurrences: u64,
    first_sequence: u64,
    last_sequence: u64,
}

#[derive(Debug, Serialize)]
struct CultivationMiddleNodeAudit {
    season_id: i32,
    line_type: i32,
    area_id: i32,
    node_id: i32,
    current_season: Option<bool>,
    area_listed_by_subtype: bool,
    area_active: Option<bool>,
    item_id: i32,
    factor_catalog_match: bool,
    family_id: Option<i64>,
    family_name: Option<String>,
    grade: Option<i32>,
    slot_category: Option<String>,
    schema_selection_candidate: bool,
    proves_selection: bool,
    occurrences: u64,
    first_sequence: u64,
    last_sequence: u64,
}

#[derive(Debug, Serialize)]
struct CultivationBigNodeAudit {
    season_id: i32,
    line_type: i32,
    area_id: i32,
    node_id: i32,
    current_season: Option<bool>,
    area_listed_by_subtype: bool,
    area_active: Option<bool>,
    fantasy_id: i32,
    schema_selection_candidate: bool,
    proves_selection: bool,
    occurrences: u64,
    first_sequence: u64,
    last_sequence: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct MatchKey {
    item_config_id: i64,
    path: String,
    tree_signature: String,
    offset: usize,
    buffer_length: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct RawMatchKey {
    item_config_id: i64,
    offset: usize,
    buffer_length: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct OwnedItemKey {
    item_config_id: i64,
    item_uuid: i64,
    package_key: i32,
}

#[derive(Debug, Serialize)]
struct MatchedFactorNode {
    item_config_id: i64,
    family_id: Option<i64>,
    family_name: Option<String>,
    grade: Option<i32>,
    slot_category: Option<String>,
    runtime_role: Option<String>,
    path: String,
    tree_signature: String,
    offset: usize,
    buffer_length: usize,
    occurrences: u64,
    first_sequence: u64,
    last_sequence: u64,
}

#[derive(Debug, Serialize)]
struct RawMatchedFactor {
    item_config_id: i64,
    family_id: Option<i64>,
    family_name: Option<String>,
    grade: Option<i32>,
    slot_category: Option<String>,
    runtime_role: Option<String>,
    offset: usize,
    buffer_length: usize,
    occurrences: u64,
    first_sequence: u64,
    last_sequence: u64,
}

#[derive(Debug, Serialize)]
struct OwnedFactorItem {
    item_config_id: i64,
    item_uuid: i64,
    package_key: i32,
    family_id: Option<i64>,
    family_name: Option<String>,
    grade: Option<i32>,
    slot_category: Option<String>,
    runtime_role: Option<String>,
    primary_buff_id: Option<i64>,
    occurrences: u64,
    first_sequence: u64,
    last_sequence: u64,
    proves_selection: bool,
}

#[derive(Debug)]
struct DirtyTreeToken {
    value: i64,
    offset: usize,
    delimiter_offset: usize,
}

#[derive(Debug)]
struct DirtyTreeValueNode {
    value: i64,
    path: String,
    offset: usize,
    tree_signature: String,
}

fn collect_cultivation_nodes(
    cultivation: &AuditSeasonCultivateLineData,
    current_season_id: Option<i32>,
    sequence: u64,
    factor_items: &HashMap<i64, FactorItem>,
    areas: &mut BTreeMap<CultivationAreaKey, CultivationAreaAudit>,
    middle_nodes: &mut BTreeMap<CultivationNodeKey, CultivationMiddleNodeAudit>,
    big_nodes: &mut BTreeMap<CultivationNodeKey, CultivationBigNodeAudit>,
) {
    for (season_id, season) in &cultivation.seasons {
        for (line_type, line) in &season.lines {
            for (area_id, area) in &line.areas {
                let current_season = current_season_id.map(|current| current == *season_id);
                let listed_by_subtype = line.area_ids.contains(area_id);
                let area_key = CultivationAreaKey {
                    season_id: *season_id,
                    line_type: *line_type,
                    area_id: *area_id,
                };
                areas
                    .entry(area_key)
                    .and_modify(|entry| {
                        entry.occurrences = entry.occurrences.saturating_add(1);
                        entry.last_sequence = sequence;
                    })
                    .or_insert_with(|| CultivationAreaAudit {
                        season_id: *season_id,
                        line_type: *line_type,
                        area_id: *area_id,
                        current_season,
                        listed_by_subtype,
                        active: area.active,
                        active_effect_score: area.active_effect_score,
                        middle_node_count: area.middle_nodes.len(),
                        big_node_count: area.big_nodes.len(),
                        occurrences: 1,
                        first_sequence: sequence,
                        last_sequence: sequence,
                    });
                for (node_id, node) in &area.middle_nodes {
                    let Some(item_id) = node.item_id.filter(|item_id| *item_id != 0) else {
                        continue;
                    };
                    let key = CultivationNodeKey {
                        season_id: *season_id,
                        line_type: *line_type,
                        area_id: *area_id,
                        node_id: *node_id,
                    };
                    let factor = factor_items.get(&i64::from(item_id));
                    middle_nodes
                        .entry(key)
                        .and_modify(|entry| {
                            entry.occurrences = entry.occurrences.saturating_add(1);
                            entry.last_sequence = sequence;
                        })
                        .or_insert_with(|| CultivationMiddleNodeAudit {
                            season_id: *season_id,
                            line_type: *line_type,
                            area_id: *area_id,
                            node_id: *node_id,
                            current_season,
                            area_listed_by_subtype: listed_by_subtype,
                            area_active: area.active,
                            item_id,
                            factor_catalog_match: factor.is_some(),
                            family_id: factor.and_then(|factor| factor.family_id),
                            family_name: factor.and_then(|factor| factor.family_name.clone()),
                            grade: factor.and_then(|factor| factor.grade),
                            slot_category: factor.and_then(|factor| factor.slot_category.clone()),
                            schema_selection_candidate: true,
                            proves_selection: false,
                            occurrences: 1,
                            first_sequence: sequence,
                            last_sequence: sequence,
                        });
                }
                for (node_id, node) in &area.big_nodes {
                    let Some(fantasy_id) = node.fantasy_id.filter(|fantasy_id| *fantasy_id != 0)
                    else {
                        continue;
                    };
                    let key = CultivationNodeKey {
                        season_id: *season_id,
                        line_type: *line_type,
                        area_id: *area_id,
                        node_id: *node_id,
                    };
                    big_nodes
                        .entry(key)
                        .and_modify(|entry| {
                            entry.occurrences = entry.occurrences.saturating_add(1);
                            entry.last_sequence = sequence;
                        })
                        .or_insert_with(|| CultivationBigNodeAudit {
                            season_id: *season_id,
                            line_type: *line_type,
                            area_id: *area_id,
                            node_id: *node_id,
                            current_season,
                            area_listed_by_subtype: listed_by_subtype,
                            area_active: area.active,
                            fantasy_id,
                            schema_selection_candidate: true,
                            proves_selection: false,
                            occurrences: 1,
                            first_sequence: sequence,
                            last_sequence: sequence,
                        });
                }
            }
        }
    }
}

fn factor_proto_needles(sources: &[ProtoFactorNeedleSource]) -> HashMap<u64, Vec<ProtoNeedle>> {
    let mut needles: HashMap<u64, Vec<ProtoNeedle>> = HashMap::new();
    let mut seen = BTreeSet::new();
    for source in sources {
        let mut values = vec![("factor-grade-item-id", source.item_config_id)];
        if source.item_uuid > 0 {
            values.push(("owned-factor-item-uuid", source.item_uuid));
        }
        if let Some(family_id) = source.family_id {
            values.push(("factor-family-id", family_id));
        }
        if let Some(primary_buff_id) = source.primary_buff_id {
            values.push(("factor-buff-id", primary_buff_id));
        }
        for (kind, signed_value) in values {
            let Ok(value) = u64::try_from(signed_value) else {
                continue;
            };
            if !seen.insert((kind, value, source.item_config_id)) {
                continue;
            }
            needles.entry(value).or_default().push(ProtoNeedle {
                kind: kind.into(),
                value,
                item_config_id: source.item_config_id,
                family_id: source.family_id,
                primary_buff_id: source.primary_buff_id,
                grade: source.grade,
                slot_category: source.slot_category.clone(),
            });
        }
    }
    needles
}

fn factor_binary_needles(
    owned_items: &BTreeMap<OwnedItemKey, OwnedFactorItem>,
    factor_items: &HashMap<i64, FactorItem>,
) -> Vec<BinaryNeedle> {
    let mut needles = Vec::new();
    let mut seen = BTreeSet::new();
    for owned in owned_items.values() {
        let Some(factor) = factor_items.get(&owned.item_config_id) else {
            continue;
        };
        let mut values = vec![("factor-grade-item-id", owned.item_config_id)];
        if owned.item_uuid > 0 {
            values.push(("owned-factor-item-uuid", owned.item_uuid));
        }
        if let Some(family_id) = factor.family_id {
            values.push(("factor-family-id", family_id));
        }
        if let Some(primary_buff_id) = factor.primary_buff_id {
            values.push(("factor-buff-id", primary_buff_id));
        }
        for (kind, signed_value) in values {
            let Ok(value) = u64::try_from(signed_value) else {
                continue;
            };
            let mut encodings = vec![("protobuf-varint", encode_varint(value))];
            if let Ok(value32) = u32::try_from(value) {
                encodings.push(("little-endian-u32", value32.to_le_bytes().to_vec()));
            }
            if kind == "owned-factor-item-uuid" {
                encodings.push(("little-endian-u64", value.to_le_bytes().to_vec()));
            }
            for (encoding, encoded) in encodings {
                if !seen.insert((kind, value, owned.item_config_id, encoding)) {
                    continue;
                }
                needles.push(BinaryNeedle {
                    kind: kind.into(),
                    value,
                    item_config_id: owned.item_config_id,
                    family_id: factor.family_id,
                    primary_buff_id: factor.primary_buff_id,
                    grade: factor.grade,
                    slot_category: factor.slot_category.clone(),
                    encoding: encoding.into(),
                    encoded,
                });
            }
        }
    }
    needles
}

fn encode_varint(mut value: u64) -> Vec<u8> {
    let mut bytes = Vec::new();
    while value >= 0x80 {
        bytes.push((value as u8 & 0x7f) | 0x80);
        value >>= 7;
    }
    bytes.push(value as u8);
    bytes
}

fn find_all_subslices(buffer: &[u8], needle: &[u8]) -> Vec<usize> {
    if needle.is_empty() || needle.len() > buffer.len() {
        return Vec::new();
    }
    buffer
        .windows(needle.len())
        .enumerate()
        .filter_map(|(offset, bytes)| (bytes == needle).then_some(offset))
        .collect()
}

#[allow(clippy::too_many_arguments)]
fn collect_proto_matches(
    bytes: &[u8],
    start: usize,
    end: usize,
    path: &str,
    depth: usize,
    max_depth: usize,
    max_len: usize,
    needles: &HashMap<u64, Vec<ProtoNeedle>>,
) -> Option<Vec<ProtoPacketMatch>> {
    if start > end || end > bytes.len() {
        return None;
    }
    let mut position = start;
    let mut saw_field = false;
    let mut field_counts = HashMap::<u64, usize>::new();
    let mut matches = Vec::new();
    while position < end {
        let (key, next_position) = decode_varint(bytes, position, end)?;
        position = next_position;
        let field_number = key >> 3;
        let wire_type = key & 0x07;
        if field_number == 0 {
            return None;
        }
        let occurrence = field_counts.entry(field_number).or_default();
        let field_path = format!("{path}.{field_number}[{occurrence}]");
        *occurrence += 1;
        match wire_type {
            0 => {
                let (value, next_position) = decode_varint(bytes, position, end)?;
                position = next_position;
                if let Some(found) = needles.get(&value) {
                    matches.push(ProtoPacketMatch {
                        path: field_path,
                        needles: found.clone(),
                    });
                }
            }
            1 => {
                position = position.checked_add(8)?;
                if position > end {
                    return None;
                }
            }
            2 => {
                let (length, next_position) = decode_varint(bytes, position, end)?;
                position = next_position;
                let length = usize::try_from(length).ok()?;
                let child_start = position;
                let child_end = position.checked_add(length)?;
                if child_end > end {
                    return None;
                }
                if length > 0 && length <= max_len && depth < max_depth {
                    if let Some(mut nested) = collect_proto_matches(
                        bytes,
                        child_start,
                        child_end,
                        &field_path,
                        depth + 1,
                        max_depth,
                        max_len,
                        needles,
                    ) {
                        matches.append(&mut nested);
                    }
                }
                position = child_end;
            }
            5 => {
                position = position.checked_add(4)?;
                if position > end {
                    return None;
                }
            }
            _ => return None,
        }
        saw_field = true;
    }
    (saw_field && position == end).then_some(matches)
}

fn decode_varint(bytes: &[u8], mut position: usize, end: usize) -> Option<(u64, usize)> {
    let mut value = 0_u64;
    let mut shift = 0_u32;
    for _ in 0..10 {
        if position >= end {
            return None;
        }
        let byte = bytes[position];
        position += 1;
        value |= u64::from(byte & 0x7f) << shift;
        if byte & 0x80 == 0 {
            return Some((value, position));
        }
        shift += 7;
    }
    None
}

fn is_known_inventory_path(path: &str) -> bool {
    path.starts_with("$.1[0].7[")
}

fn classify_full_sync_proto_path(path: &str) -> &'static str {
    if is_known_inventory_path(path) {
        "item-package-ownership"
    } else if path.starts_with("$.1[0].6[") {
        "buff-state"
    } else if path.starts_with("$.1[0].12[") {
        "equipment-state"
    } else if path.starts_with("$.1[0].16[") {
        "attribute-state"
    } else if path.starts_with("$.1[0].25[") {
        "planet-memory-state"
    } else if path.starts_with("$.1[0].28[") {
        "resonance-state"
    } else if path.starts_with("$.1[0].50[") {
        "season-center-state"
    } else if path.starts_with("$.1[0].54[") {
        "season-activation-state"
    } else if path.starts_with("$.1[0].55[") {
        "slot-state"
    } else if path.starts_with("$.1[0].57[") {
        "module-state"
    } else if path.starts_with("$.1[0].61[") {
        "profession-state"
    } else {
        "other-full-sync-state"
    }
}

fn dirty_tree_tokens(buffer: &[u8]) -> Vec<DirtyTreeToken> {
    let mut tokens = Vec::new();
    let mut offset = 0usize;
    while offset < buffer.len() {
        let Some(delimiter_offset) = find_dirty_tree_delimiter(buffer, offset) else {
            break;
        };
        if let Some(value) = decode_dirty_tree_scalar(&buffer[offset..delimiter_offset]) {
            tokens.push(DirtyTreeToken {
                value,
                offset,
                delimiter_offset,
            });
        }
        offset = delimiter_offset.saturating_add(4);
    }
    tokens
}

fn find_dirty_tree_delimiter(buffer: &[u8], start_offset: usize) -> Option<usize> {
    buffer
        .get(start_offset..)?
        .windows(4)
        .position(|bytes| bytes == DIRTY_TREE_DELIMITER.to_le_bytes())
        .map(|relative| start_offset + relative)
}

fn decode_dirty_tree_scalar(segment: &[u8]) -> Option<i64> {
    match segment.len() {
        4 => {
            let bytes: [u8; 4] = segment.try_into().ok()?;
            let signed = i32::from_le_bytes(bytes);
            Some(if signed < 0 {
                i64::from(signed)
            } else {
                i64::from(u32::from_le_bytes(bytes))
            })
        }
        8 => {
            let low: [u8; 4] = segment[0..4].try_into().ok()?;
            let high: [u8; 4] = segment[4..8].try_into().ok()?;
            let high_signed = i32::from_le_bytes(high);
            let low_signed = i32::from_le_bytes(low);
            Some(if high_signed == 0 {
                i64::from(u32::from_le_bytes(low))
            } else if high_signed == -1 && low_signed < 0 {
                i64::from(low_signed)
            } else {
                i64::from_le_bytes(segment.try_into().ok()?)
            })
        }
        _ => None,
    }
}

fn dirty_tree_value_nodes(tokens: &[DirtyTreeToken]) -> Vec<DirtyTreeValueNode> {
    let mut cursor = 0usize;
    let mut nodes = Vec::new();
    parse_dirty_tree_children(tokens, &mut cursor, "", None, &[], &mut nodes);
    nodes
}

fn parse_dirty_tree_children(
    tokens: &[DirtyTreeToken],
    cursor: &mut usize,
    path_prefix: &str,
    body_end_offset: Option<usize>,
    ancestors: &[(String, i64)],
    nodes: &mut Vec<DirtyTreeValueNode>,
) {
    let mut child_index = 0usize;
    while *cursor < tokens.len() {
        let token = &tokens[*cursor];
        if body_end_offset.is_some_and(|end| token.offset >= end) {
            break;
        }
        if token.value == -3 {
            *cursor += 1;
            break;
        }
        let path = if path_prefix.is_empty() {
            child_index.to_string()
        } else {
            format!("{path_prefix}.{child_index}")
        };
        if token.value == -2 && *cursor + 1 < tokens.len() {
            let length = tokens[*cursor + 1].value;
            let body_start = tokens
                .get(*cursor + 2)
                .map(|next| next.offset)
                .unwrap_or_else(|| tokens[*cursor + 1].delimiter_offset.saturating_add(4));
            *cursor += 2;
            let block_end = usize::try_from(length)
                .ok()
                .and_then(|length| body_start.checked_add(length));
            let mut nested_ancestors = ancestors.to_vec();
            nested_ancestors.push((path.clone(), length));
            parse_dirty_tree_children(tokens, cursor, &path, block_end, &nested_ancestors, nodes);
            if tokens.get(*cursor).is_some_and(|next| next.value == -3) {
                *cursor += 1;
            }
        } else {
            nodes.push(DirtyTreeValueNode {
                value: token.value,
                path,
                offset: token.offset,
                tree_signature: ancestors
                    .iter()
                    .map(|(path, length)| format!("{path}:{length}"))
                    .collect::<Vec<_>>()
                    .join(">"),
            });
            *cursor += 1;
        }
        child_index += 1;
    }
}

struct Arguments {
    journal: PathBuf,
    factors: PathBuf,
    output: Option<PathBuf>,
}

fn arguments() -> Result<Arguments, String> {
    let mut values = std::env::args_os().skip(1).collect::<Vec<_>>();
    if values
        .first()
        .is_some_and(|value| value == OsStr::new("--private-research"))
    {
        values.remove(0);
    } else {
        return Err(usage());
    }
    let journal = take_value(&mut values, "--journal")?;
    let factors = take_value(&mut values, "--factors")?;
    let output = take_optional_value(&mut values, "--output")?;
    if !values.is_empty() {
        return Err(usage());
    }
    Ok(Arguments {
        journal: journal.into(),
        factors: factors.into(),
        output: output.map(Into::into),
    })
}

fn take_value(values: &mut Vec<OsString>, flag: &str) -> Result<OsString, String> {
    let Some(index) = values.iter().position(|value| value == OsStr::new(flag)) else {
        return Err(usage());
    };
    if index + 1 >= values.len() {
        return Err(usage());
    }
    values.remove(index);
    Ok(values.remove(index))
}

fn take_optional_value(values: &mut Vec<OsString>, flag: &str) -> Result<Option<OsString>, String> {
    let Some(index) = values.iter().position(|value| value == OsStr::new(flag)) else {
        return Ok(None);
    };
    if index + 1 >= values.len() {
        return Err(usage());
    }
    values.remove(index);
    Ok(Some(values.remove(index)))
}

fn usage() -> String {
    "usage: rlogs-bpsr-factor-capture-audit --private-research --journal <capture.jsonl> --factors <SeasonPhantomFactors.json> [--output <audit.json>]".into()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn push_token(buffer: &mut Vec<u8>, value: i32) {
        buffer.extend_from_slice(&value.to_le_bytes());
        buffer.extend_from_slice(&DIRTY_TREE_DELIMITER.to_le_bytes());
    }

    #[test]
    fn reports_nested_scalar_path_without_interpreting_unmatched_values() {
        let mut buffer = Vec::new();
        for token in [-2, 24, 7, -2, 8, 20010930, -3, -3] {
            push_token(&mut buffer, token);
        }
        let nodes = dirty_tree_value_nodes(&dirty_tree_tokens(&buffer));
        let selected = nodes
            .iter()
            .find(|node| node.value == 20_010_930)
            .expect("factor scalar");
        assert_eq!(selected.path, "0.1.0");
        assert_eq!(selected.tree_signature, "0:24>0.1:8");
    }

    #[test]
    fn reports_factor_uuid_paths_without_promoting_inventory_ownership() {
        let uuid = 987_654_321_u64;
        let mut item_package = vec![0x08];
        encode_varint_for_test(&mut item_package, uuid);
        let mut character = vec![0x3a];
        encode_varint_for_test(&mut character, item_package.len() as u64);
        character.extend_from_slice(&item_package);
        let mut bytes = vec![0x0a];
        encode_varint_for_test(&mut bytes, character.len() as u64);
        bytes.extend_from_slice(&character);
        let needles = HashMap::from([(
            uuid,
            vec![ProtoNeedle {
                kind: "owned-factor-item-uuid".into(),
                value: uuid,
                item_config_id: 20_010_930,
                family_id: Some(202_189),
                primary_buff_id: Some(3_058_050),
                grade: Some(10),
                slot_category: Some("Polarity".into()),
            }],
        )]);
        let matches =
            collect_proto_matches(&bytes, 0, bytes.len(), "$", 0, 14, 1_000_000, &needles)
                .expect("valid protobuf");
        assert_eq!(matches.len(), 1);
        assert_eq!(matches[0].path, "$.1[0].7[0].1[0]");
        assert!(is_known_inventory_path(&matches[0].path));
    }

    fn encode_varint_for_test(bytes: &mut Vec<u8>, mut value: u64) {
        while value >= 0x80 {
            bytes.push((value as u8 & 0x7f) | 0x80);
            value >>= 7;
        }
        bytes.push(value as u8);
    }
}
