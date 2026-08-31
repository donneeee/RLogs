use std::{
    collections::{BTreeMap, BTreeSet},
    env,
    ffi::OsString,
    fs::{self, File},
    io::{BufReader, BufWriter, Read, Write},
    path::{Path, PathBuf},
};

use rlogs_events::{
    CanonicalEvent, EntityAttributeUpdateKind, EntityAttributeValue, TimelineEventKind,
};
use rlogs_log_format::{RlogLimits, RlogReader};
use serde::Serialize;
use sha2::{Digest, Sha256};

const SCHEMA_VERSION: u16 = 4;

#[derive(Debug, Default, Serialize)]
struct AttributeSummary {
    observation_count: u64,
    actors: BTreeMap<i64, u64>,
    rlogs: BTreeMap<String, u64>,
    update_kinds: BTreeMap<String, u64>,
    raw_values: BTreeMap<String, RawValueSummary>,
    samples: Vec<AttributeSample>,
    dropped_samples: u64,
}

#[derive(Debug, Serialize)]
struct AttributeSample {
    rlog: String,
    sequence: u64,
    observed_micros: u64,
    actor_entity_uuid: i64,
    attribute_id: i32,
    update_kind: String,
    raw_value: String,
    decoded: Option<String>,
    protobuf_varint_unsigned: Option<u64>,
    protobuf_varint_signed_twos_complement: Option<i64>,
    protobuf_varint_zigzag: Option<i64>,
    protobuf_packed_field_1_varints: Option<Vec<u64>>,
}

#[derive(Debug, Default, Serialize)]
struct RawValueSummary {
    count: u64,
    decoded: BTreeSet<String>,
    protobuf_varint_unsigned: Option<u64>,
    protobuf_varint_signed_twos_complement: Option<i64>,
    protobuf_varint_zigzag: Option<i64>,
    protobuf_packed_field_1_varints: Option<Vec<u64>>,
}

#[derive(Debug, Serialize)]
struct Audit {
    schema_version: u16,
    policy: &'static str,
    expected_game_build: Option<String>,
    observed_game_builds: BTreeSet<String>,
    actor_filters: BTreeSet<i64>,
    actor_filter_semantics: &'static str,
    sample_limit_per_attribute: usize,
    inputs: Vec<InputReport>,
    attributes: BTreeMap<i32, AttributeSummary>,
}

#[derive(Debug, Serialize)]
struct InputReport {
    path: String,
    bytes: u64,
    sha256: String,
    session_id: Option<String>,
    game_build: String,
    canonical_events_scanned: u64,
    entity_attribute_events_scanned: u64,
    selected_actor_attribute_events_scanned: u64,
}

#[derive(Debug, PartialEq, Eq)]
struct Arguments {
    attribute_ids: BTreeSet<i32>,
    actor_filters: BTreeSet<i64>,
    expected_game_build: Option<String>,
    sample_limit_per_attribute: usize,
    rlogs: Vec<PathBuf>,
    output: PathBuf,
}

fn main() {
    if let Err(error) = run() {
        eprintln!("attribute scalar audit failed: {error}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), Box<dyn std::error::Error>> {
    let arguments = arguments()?;
    let mut attributes = arguments
        .attribute_ids
        .iter()
        .copied()
        .map(|attribute| (attribute, AttributeSummary::default()))
        .collect::<BTreeMap<_, _>>();

    let inputs = arguments
        .rlogs
        .iter()
        .map(|path| scan_rlog(path, &arguments, &mut attributes))
        .collect::<Result<Vec<_>, _>>()?;
    let observed_game_builds = inputs
        .iter()
        .map(|input| input.game_build.clone())
        .collect();

    let audit = Audit {
        schema_version: SCHEMA_VERSION,
        policy: "build_locked_complete_packet_exact_aggregates_with_input_hashes_bounded_ordered_samples_explicit_absence_and_scalar_or_packed_field_1_varint_interpretations",
        expected_game_build: arguments.expected_game_build,
        observed_game_builds,
        actor_filters: arguments.actor_filters,
        actor_filter_semantics: "an empty set selects every actor; otherwise only the exact numeric entity UUIDs listed are selected",
        sample_limit_per_attribute: arguments.sample_limit_per_attribute,
        inputs,
        attributes,
    };
    let mut writer = BufWriter::new(File::create(arguments.output)?);
    serde_json::to_writer_pretty(&mut writer, &audit)?;
    writer.write_all(b"\n")?;
    writer.flush()?;
    Ok(())
}

fn scan_rlog(
    path: &Path,
    arguments: &Arguments,
    attributes: &mut BTreeMap<i32, AttributeSummary>,
) -> Result<InputReport, Box<dyn std::error::Error>> {
    let bytes = fs::metadata(path)?.len();
    let sha256 = sha256_file(path)?;
    let mut reader = RlogReader::new(BufReader::new(File::open(path)?), RlogLimits::default())?;
    let game_build = reader.header().region.client_build.clone();
    if arguments
        .expected_game_build
        .as_ref()
        .is_some_and(|expected| expected != &game_build)
    {
        return Err(format!(
            "{} contains client build {game_build}, not requested build {}",
            display_path(path),
            arguments.expected_game_build.as_deref().unwrap_or_default()
        )
        .into());
    }
    let rlog = path
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| display_path(path));
    let mut session_id = None;
    let mut canonical_events_scanned = 0_u64;
    let mut entity_attribute_events_scanned = 0_u64;
    let mut selected_actor_attribute_events_scanned = 0_u64;
    while let Some(envelope) = reader.next_event()? {
        canonical_events_scanned = canonical_events_scanned.saturating_add(1);
        session_id.get_or_insert_with(|| envelope.session_id.clone());
        let CanonicalEvent::Timeline(timeline) = envelope.event else {
            continue;
        };
        let TimelineEventKind::EntityAttributes(event) = timeline.kind else {
            continue;
        };
        entity_attribute_events_scanned = entity_attribute_events_scanned.saturating_add(1);
        let actor_entity_uuid = event.actor.entity_uuid.0;
        if !arguments.actor_filters.is_empty()
            && !arguments.actor_filters.contains(&actor_entity_uuid)
        {
            continue;
        }
        selected_actor_attribute_events_scanned =
            selected_actor_attribute_events_scanned.saturating_add(1);
        for attribute in event.attributes {
            let Some(summary) = attributes.get_mut(&attribute.attribute_id) else {
                continue;
            };
            record_observation(
                summary,
                &rlog,
                envelope.sequence,
                envelope.time.observed_micros,
                actor_entity_uuid,
                attribute.attribute_id,
                event.update_kind,
                &attribute.raw_value,
                attribute.decoded.as_ref(),
                arguments.sample_limit_per_attribute,
            );
        }
    }
    Ok(InputReport {
        path: display_path(path),
        bytes,
        sha256,
        session_id,
        game_build,
        canonical_events_scanned,
        entity_attribute_events_scanned,
        selected_actor_attribute_events_scanned,
    })
}

#[allow(clippy::too_many_arguments)]
fn record_observation(
    summary: &mut AttributeSummary,
    rlog: &str,
    sequence: u64,
    observed_micros: u64,
    actor_entity_uuid: i64,
    attribute_id: i32,
    update_kind: EntityAttributeUpdateKind,
    raw_value: &[u8],
    decoded: Option<&EntityAttributeValue>,
    sample_limit: usize,
) {
    summary.observation_count = summary.observation_count.saturating_add(1);
    *summary.actors.entry(actor_entity_uuid).or_default() += 1;
    *summary.rlogs.entry(rlog.to_owned()).or_default() += 1;
    *summary
        .update_kinds
        .entry(update_kind_name(update_kind).to_owned())
        .or_default() += 1;

    let rendered_raw = hex(raw_value);
    let varint = decode_varint(raw_value);
    let packed_varints = decode_packed_field_1_varints(raw_value);
    let rendered_decoded = decoded.map(render_decoded);
    let raw = summary
        .raw_values
        .entry(rendered_raw.clone())
        .or_insert_with(|| RawValueSummary {
            protobuf_varint_unsigned: varint,
            protobuf_varint_signed_twos_complement: varint.map(|value| value as i64),
            protobuf_varint_zigzag: varint.map(zigzag),
            protobuf_packed_field_1_varints: packed_varints.clone(),
            ..RawValueSummary::default()
        });
    raw.count = raw.count.saturating_add(1);
    if let Some(decoded) = rendered_decoded.as_ref() {
        raw.decoded.insert(decoded.clone());
    }

    if summary.samples.len() < sample_limit {
        summary.samples.push(AttributeSample {
            rlog: rlog.to_owned(),
            sequence,
            observed_micros,
            actor_entity_uuid,
            attribute_id,
            update_kind: update_kind_name(update_kind).to_owned(),
            raw_value: rendered_raw,
            decoded: rendered_decoded,
            protobuf_varint_unsigned: varint,
            protobuf_varint_signed_twos_complement: varint.map(|value| value as i64),
            protobuf_varint_zigzag: varint.map(zigzag),
            protobuf_packed_field_1_varints: packed_varints,
        });
    } else {
        summary.dropped_samples = summary.dropped_samples.saturating_add(1);
    }
}

fn update_kind_name(update_kind: EntityAttributeUpdateKind) -> &'static str {
    match update_kind {
        EntityAttributeUpdateKind::Snapshot => "snapshot",
        EntityAttributeUpdateKind::Delta => "delta",
        EntityAttributeUpdateKind::Unknown => "unknown",
    }
}

fn sha256_file(path: &Path) -> Result<String, Box<dyn std::error::Error>> {
    let mut reader = BufReader::new(File::open(path)?);
    let mut digest = Sha256::new();
    let mut buffer = [0_u8; 64 * 1024];
    loop {
        let read = reader.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        digest.update(&buffer[..read]);
    }
    Ok(format!("sha256:{:x}", digest.finalize()))
}

fn display_path(path: &Path) -> String {
    path.to_string_lossy().replace('\\', "/")
}

fn hex(bytes: &[u8]) -> String {
    let mut rendered = String::with_capacity(bytes.len().saturating_mul(2));
    for byte in bytes {
        use std::fmt::Write as _;
        let _ = write!(rendered, "{byte:02x}");
    }
    rendered
}

fn decode_varint(bytes: &[u8]) -> Option<u64> {
    if bytes.is_empty() {
        return Some(0);
    }
    decode_varint_prefix(bytes)
        .and_then(|(value, consumed)| (consumed == bytes.len()).then_some(value))
}

fn decode_varint_prefix(bytes: &[u8]) -> Option<(u64, usize)> {
    let mut value = 0_u64;
    for (index, byte) in bytes.iter().copied().take(10).enumerate() {
        if index == 9 && byte > 1 {
            return None;
        }
        value |= u64::from(byte & 0x7f) << (index * 7);
        if byte & 0x80 == 0 {
            return Some((value, index + 1));
        }
    }
    None
}

fn decode_packed_field_1_varints(bytes: &[u8]) -> Option<Vec<u64>> {
    if bytes.first().copied()? != 0x0a {
        return None;
    }
    let (payload_len, length_bytes) = decode_varint_prefix(&bytes[1..])?;
    let payload_start = 1_usize.checked_add(length_bytes)?;
    let payload_end = payload_start.checked_add(usize::try_from(payload_len).ok()?)?;
    if payload_end != bytes.len() {
        return None;
    }

    let mut values = Vec::new();
    let mut cursor = payload_start;
    while cursor < payload_end {
        let (value, consumed) = decode_varint_prefix(&bytes[cursor..payload_end])?;
        values.push(value);
        cursor = cursor.checked_add(consumed)?;
    }
    Some(values)
}

fn zigzag(value: u64) -> i64 {
    ((value >> 1) as i64) ^ -((value & 1) as i64)
}

fn render_decoded(value: &EntityAttributeValue) -> String {
    match value {
        EntityAttributeValue::Integer(value) => format!("integer:{value}"),
        EntityAttributeValue::Text(value) => format!("text:{value}"),
        EntityAttributeValue::Position {
            x,
            y,
            z,
            facing_radians,
        } => {
            format!("position:{x},{y},{z},{facing_radians:?}")
        }
    }
}

fn arguments() -> Result<Arguments, String> {
    arguments_from(env::args_os().skip(1).collect())
}

fn arguments_from(mut values: Vec<OsString>) -> Result<Arguments, String> {
    let output = PathBuf::from(take_value(&mut values, "--output")?);
    let expected_game_build =
        take_optional_value(&mut values, "--build")?.map(|raw| raw.to_string_lossy().into_owned());
    let mut actor_filters = BTreeSet::new();
    while let Some(position) = values.iter().position(|value| value == "--actor") {
        if position + 1 >= values.len() {
            return Err("--actor requires a numeric entity UUID".to_owned());
        }
        let raw = values.remove(position + 1);
        values.remove(position);
        actor_filters.insert(
            raw.to_string_lossy()
                .parse::<i64>()
                .map_err(|_| "--actor requires a numeric entity UUID".to_owned())?,
        );
    }
    let sample_limit_per_attribute = take_optional_value(&mut values, "--sample-limit")?
        .map(|raw| {
            raw.to_string_lossy()
                .parse::<usize>()
                .map_err(|_| "--sample-limit requires a positive integer".to_owned())
        })
        .transpose()?
        .unwrap_or(1_024);
    if sample_limit_per_attribute == 0 {
        return Err("--sample-limit requires a positive integer".to_owned());
    }
    let mut attributes = BTreeSet::new();
    while let Some(position) = values.iter().position(|value| value == "--attribute") {
        if position + 1 >= values.len() {
            return Err("--attribute requires a numeric attribute ID".to_owned());
        }
        let raw = values.remove(position + 1);
        values.remove(position);
        let parsed = raw
            .to_string_lossy()
            .parse::<i32>()
            .map_err(|_| "--attribute requires a numeric attribute ID".to_owned())?;
        attributes.insert(parsed);
    }
    let mut rlogs = Vec::new();
    while let Some(position) = values.iter().position(|value| value == "--rlog") {
        if position + 1 >= values.len() {
            return Err("--rlog requires a path".to_owned());
        }
        rlogs.push(PathBuf::from(values.remove(position + 1)));
        values.remove(position);
    }
    if attributes.is_empty() || rlogs.is_empty() || !values.is_empty() {
        return Err(usage());
    }
    Ok(Arguments {
        attribute_ids: attributes,
        actor_filters,
        expected_game_build,
        sample_limit_per_attribute,
        rlogs,
        output,
    })
}

fn take_value(values: &mut Vec<OsString>, flag: &str) -> Result<OsString, String> {
    let position = values
        .iter()
        .position(|value| value == flag)
        .ok_or_else(usage)?;
    if position + 1 >= values.len() {
        return Err(format!("{flag} requires a value"));
    }
    let value = values.remove(position + 1);
    values.remove(position);
    Ok(value)
}

fn take_optional_value(values: &mut Vec<OsString>, flag: &str) -> Result<Option<OsString>, String> {
    let Some(position) = values.iter().position(|value| value == flag) else {
        return Ok(None);
    };
    if position + 1 >= values.len() {
        return Err(format!("{flag} requires a value"));
    }
    let value = values.remove(position + 1);
    values.remove(position);
    Ok(Some(value))
}

fn usage() -> String {
    "usage: rlogs-bpsr-attribute-scalar-audit --attribute <id> [--attribute <id> ...] [--actor <entity-uuid> ...] [--build <client-build>] [--sample-limit <positive-count>] --rlog <sealed.rlog> [--rlog <sealed.rlog> ...] --output <audit.json>".to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn capped_samples_do_not_reduce_exact_aggregate_counts() {
        let mut summary = AttributeSummary::default();
        record_observation(
            &mut summary,
            "capture.rlog",
            10,
            20,
            30,
            11310,
            EntityAttributeUpdateKind::Snapshot,
            &[0x01],
            None,
            1,
        );
        record_observation(
            &mut summary,
            "capture.rlog",
            11,
            21,
            30,
            11310,
            EntityAttributeUpdateKind::Delta,
            &[0x02],
            None,
            1,
        );

        assert_eq!(summary.observation_count, 2);
        assert_eq!(summary.actors.get(&30), Some(&2));
        assert_eq!(summary.rlogs.get("capture.rlog"), Some(&2));
        assert_eq!(summary.update_kinds.get("snapshot"), Some(&1));
        assert_eq!(summary.update_kinds.get("delta"), Some(&1));
        assert_eq!(summary.raw_values.get("01").map(|raw| raw.count), Some(1));
        assert_eq!(summary.raw_values.get("02").map(|raw| raw.count), Some(1));
        assert_eq!(summary.samples.len(), 1);
        assert_eq!(summary.dropped_samples, 1);
    }

    #[test]
    fn arguments_accept_actor_filter_and_sample_limit() {
        let arguments = arguments_from(
            [
                "--attribute",
                "20010",
                "--actor",
                "1489989468800",
                "--actor",
                "349530030720",
                "--build",
                "24687926",
                "--sample-limit",
                "64",
                "--rlog",
                "capture.rlog",
                "--output",
                "audit.json",
            ]
            .into_iter()
            .map(OsString::from)
            .collect(),
        )
        .expect("valid arguments");

        assert_eq!(arguments.attribute_ids, BTreeSet::from([20_010]));
        assert_eq!(
            arguments.actor_filters,
            BTreeSet::from([349_530_030_720, 1_489_989_468_800])
        );
        assert_eq!(arguments.expected_game_build.as_deref(), Some("24687926"));
        assert_eq!(arguments.sample_limit_per_attribute, 64);
        assert_eq!(arguments.rlogs, vec![PathBuf::from("capture.rlog")]);
        assert_eq!(arguments.output, PathBuf::from("audit.json"));
    }

    #[test]
    fn decodes_exact_packed_field_one_varints_without_guessing_trailing_bytes() {
        let bytes = [
            0x0a, 0x10, 0x00, 0xc6, 0xfc, 0x15, 0x00, 0xc6, 0xfc, 0x15, 0x00, 0xc6, 0xfc, 0x15,
            0x00, 0x3c, 0x00, 0x05,
        ];

        assert_eq!(
            decode_packed_field_1_varints(&bytes),
            Some(vec![0, 360_006, 0, 360_006, 0, 360_006, 0, 60, 0, 5])
        );

        let mut trailing = bytes.to_vec();
        trailing.push(0xff);
        assert_eq!(decode_packed_field_1_varints(&trailing), None);
    }
}
