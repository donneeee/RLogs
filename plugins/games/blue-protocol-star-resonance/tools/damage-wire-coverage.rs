use std::{
    collections::BTreeMap,
    ffi::OsString,
    fs::File,
    io::{BufReader, BufWriter},
    path::{Path, PathBuf},
};

use rlogs_game_bpsr::{CaptureRecordKind, JsonlJournalReader};
use serde::Serialize;

const AOI_SERVICE_ID: u64 = 1_664_308_034;
const SYNC_NEAR_DELTA_METHOD_ID: u32 = 45;
const SYNC_TO_ME_DELTA_METHOD_ID: u32 = 46;
const MAXIMUM_REPORTED_EXAMPLES: usize = 64;

fn main() {
    if let Err(error) = run() {
        eprintln!("damage wire coverage failed: {error}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), Box<dyn std::error::Error>> {
    let arguments = arguments()?;
    let mut accumulator = AuditAccumulator::default();
    let mut journals = Vec::new();

    for path in &arguments.journals {
        let journal = JsonlJournalReader::new(BufReader::new(File::open(path)?)).read()?;
        journals.push(JournalIdentity {
            path: slash_path(path),
            capture_id: journal.session().capture_id.clone(),
            game_build: journal.session().game_build.build_id.clone(),
        });

        for record in journal.records() {
            let CaptureRecordKind::Packet(packet) = &record.kind else {
                continue;
            };
            let Some(route) = packet.route.map(|route| route.key) else {
                continue;
            };
            if route.service_id != AOI_SERVICE_ID
                || !matches!(
                    route.method_id,
                    SYNC_NEAR_DELTA_METHOD_ID | SYNC_TO_ME_DELTA_METHOD_ID
                )
            {
                continue;
            }
            let Some(payload) = packet.payload.decode_input() else {
                continue;
            };
            accumulator.route_packets = accumulator.route_packets.saturating_add(1);
            *accumulator
                .route_packets_by_method
                .entry(route.method_id)
                .or_default() += 1;

            let location = EvidenceLocation {
                journal: slash_path(path),
                record_sequence: record.sequence,
                method_id: route.method_id,
            };
            if let Err(error) =
                audit_route_payload(payload, route.method_id, &location, &mut accumulator)
            {
                accumulator.push_error(location, "route_payload", error);
            }
        }
    }

    let report = accumulator.finish(journals);
    serde_json::to_writer_pretty(BufWriter::new(File::create(&arguments.output)?), &report)?;
    println!(
        "audited {} damage message(s); {} unknown field observation(s), {} wire mismatch observation(s); wrote {}",
        report.damage_messages,
        report.unknown_field_observations,
        report.wire_type_mismatch_observations,
        arguments.output.display()
    );
    Ok(())
}

#[derive(Debug)]
struct Arguments {
    journals: Vec<PathBuf>,
    output: PathBuf,
}

fn arguments() -> Result<Arguments, String> {
    let mut values = std::env::args_os().skip(1);
    let mut journals = Vec::new();
    let mut output = None;
    while let Some(argument) = values.next() {
        match argument.to_string_lossy().as_ref() {
            "--journal" => journals.push(PathBuf::from(required(&mut values, "--journal")?)),
            "--output" => output = Some(PathBuf::from(required(&mut values, "--output")?)),
            other => return Err(format!("unknown argument {other}")),
        }
    }
    if journals.is_empty() {
        return Err("at least one --journal is required".to_owned());
    }
    Ok(Arguments {
        journals,
        output: output.ok_or("missing --output")?,
    })
}

fn required(values: &mut impl Iterator<Item = OsString>, flag: &str) -> Result<OsString, String> {
    values
        .next()
        .ok_or_else(|| format!("missing value for {flag}"))
}

#[derive(Debug, Clone, Serialize)]
struct JournalIdentity {
    path: String,
    capture_id: String,
    game_build: String,
}

#[derive(Debug, Clone, Serialize)]
struct EvidenceLocation {
    journal: String,
    record_sequence: u64,
    method_id: u32,
}

#[derive(Debug, Serialize)]
struct AuditReport {
    schema_version: u16,
    journals: Vec<JournalIdentity>,
    route_packets: u64,
    route_packets_by_method: BTreeMap<u32, u64>,
    damage_messages: u64,
    field_catalog: Vec<FieldObservation>,
    unknown_field_observations: u64,
    wire_type_mismatch_observations: u64,
    extraction_errors: u64,
    examples: Vec<AuditExample>,
    conclusion: AuditConclusion,
    evidence_policy: EvidencePolicy,
}

#[derive(Debug, Serialize)]
struct FieldObservation {
    scope: &'static str,
    field_number: u32,
    wire_type: u8,
    expected_wire_type: Option<u8>,
    observations: u64,
}

#[derive(Debug, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
enum AuditExample {
    UnknownField {
        location: EvidenceLocation,
        scope: &'static str,
        field_number: u32,
        wire_type: u8,
    },
    WireTypeMismatch {
        location: EvidenceLocation,
        scope: &'static str,
        field_number: u32,
        wire_type: u8,
        expected_wire_type: u8,
    },
    ExtractionError {
        location: EvidenceLocation,
        scope: &'static str,
        detail: String,
    },
}

#[derive(Debug, Serialize)]
struct AuditConclusion {
    every_observed_damage_field_is_declared: bool,
    every_observed_damage_field_has_expected_wire_type: bool,
    current_decoder_has_no_wire_visible_undeclared_damage_field: bool,
}

#[derive(Debug, Serialize)]
struct EvidencePolicy {
    unknown_fields_discarded: bool,
    malformed_payloads_discarded: bool,
    absence_of_a_field_is_treated_as_a_zero_value: bool,
    declared_schema_is_formula_authority: bool,
    packet_field_identity_is_formula_semantics: bool,
}

#[derive(Debug, Default)]
struct AuditAccumulator {
    route_packets: u64,
    route_packets_by_method: BTreeMap<u32, u64>,
    damage_messages: u64,
    fields: BTreeMap<(&'static str, u32, u8), u64>,
    unknown_field_observations: u64,
    wire_type_mismatch_observations: u64,
    extraction_errors: u64,
    examples: Vec<AuditExample>,
}

impl AuditAccumulator {
    fn observe_field(&mut self, scope: Scope, field: &WireField<'_>, location: &EvidenceLocation) {
        *self
            .fields
            .entry((scope.label(), field.number, field.wire_type))
            .or_default() += 1;
        match scope.expected_wire_type(field.number) {
            None => {
                self.unknown_field_observations = self.unknown_field_observations.saturating_add(1);
                self.push_example(AuditExample::UnknownField {
                    location: location.clone(),
                    scope: scope.label(),
                    field_number: field.number,
                    wire_type: field.wire_type,
                });
            }
            Some(expected) if expected != field.wire_type => {
                self.wire_type_mismatch_observations =
                    self.wire_type_mismatch_observations.saturating_add(1);
                self.push_example(AuditExample::WireTypeMismatch {
                    location: location.clone(),
                    scope: scope.label(),
                    field_number: field.number,
                    wire_type: field.wire_type,
                    expected_wire_type: expected,
                });
            }
            Some(_) => {}
        }
    }

    fn push_error(
        &mut self,
        location: EvidenceLocation,
        scope: &'static str,
        detail: impl Into<String>,
    ) {
        self.extraction_errors = self.extraction_errors.saturating_add(1);
        self.push_example(AuditExample::ExtractionError {
            location,
            scope,
            detail: detail.into(),
        });
    }

    fn push_example(&mut self, example: AuditExample) {
        if self.examples.len() < MAXIMUM_REPORTED_EXAMPLES {
            self.examples.push(example);
        }
    }

    fn finish(self, journals: Vec<JournalIdentity>) -> AuditReport {
        let field_catalog = self
            .fields
            .into_iter()
            .map(
                |((scope, field_number, wire_type), observations)| FieldObservation {
                    scope,
                    field_number,
                    wire_type,
                    expected_wire_type: Scope::from_label(scope)
                        .and_then(|scope| scope.expected_wire_type(field_number)),
                    observations,
                },
            )
            .collect();
        AuditReport {
            schema_version: 1,
            journals,
            route_packets: self.route_packets,
            route_packets_by_method: self.route_packets_by_method,
            damage_messages: self.damage_messages,
            field_catalog,
            unknown_field_observations: self.unknown_field_observations,
            wire_type_mismatch_observations: self.wire_type_mismatch_observations,
            extraction_errors: self.extraction_errors,
            examples: self.examples,
            conclusion: AuditConclusion {
                every_observed_damage_field_is_declared: self.unknown_field_observations == 0,
                every_observed_damage_field_has_expected_wire_type: self
                    .wire_type_mismatch_observations
                    == 0,
                current_decoder_has_no_wire_visible_undeclared_damage_field: self
                    .unknown_field_observations
                    == 0
                    && self.wire_type_mismatch_observations == 0
                    && self.extraction_errors == 0,
            },
            evidence_policy: EvidencePolicy {
                unknown_fields_discarded: false,
                malformed_payloads_discarded: false,
                absence_of_a_field_is_treated_as_a_zero_value: false,
                declared_schema_is_formula_authority: false,
                packet_field_identity_is_formula_semantics: false,
            },
        }
    }
}

#[derive(Debug, Clone, Copy)]
enum Scope {
    Damage,
    DamagePosition,
    HitPart,
    HitPartPosition,
    DamageWeight,
}

impl Scope {
    fn label(self) -> &'static str {
        match self {
            Self::Damage => "damage",
            Self::DamagePosition => "damage_position",
            Self::HitPart => "hit_part",
            Self::HitPartPosition => "hit_part_position",
            Self::DamageWeight => "damage_weight",
        }
    }

    fn from_label(label: &str) -> Option<Self> {
        match label {
            "damage" => Some(Self::Damage),
            "damage_position" => Some(Self::DamagePosition),
            "hit_part" => Some(Self::HitPart),
            "hit_part_position" => Some(Self::HitPartPosition),
            "damage_weight" => Some(Self::DamageWeight),
            _ => None,
        }
    }

    fn expected_wire_type(self, field_number: u32) -> Option<u8> {
        match self {
            Self::Damage => match field_number {
                1..=18 | 21 | 23..=25 => Some(0),
                19 | 20 | 22 => Some(2),
                _ => None,
            },
            Self::DamagePosition | Self::HitPartPosition => match field_number {
                1..=3 => Some(5),
                _ => None,
            },
            Self::HitPart => match field_number {
                1 | 3 => Some(0),
                2 => Some(2),
                _ => None,
            },
            Self::DamageWeight => match field_number {
                1..=2 => Some(5),
                _ => None,
            },
        }
    }
}

fn audit_route_payload(
    payload: &[u8],
    method_id: u32,
    location: &EvidenceLocation,
    accumulator: &mut AuditAccumulator,
) -> Result<(), String> {
    let top = parse_message(payload)?;
    match method_id {
        SYNC_NEAR_DELTA_METHOD_ID => {
            for delta in bytes_fields(&top, 1) {
                audit_delta(delta, location, accumulator)?;
            }
        }
        SYNC_TO_ME_DELTA_METHOD_ID => {
            for to_me in bytes_fields(&top, 1) {
                let message = parse_message(to_me)?;
                for delta in bytes_fields(&message, 1) {
                    audit_delta(delta, location, accumulator)?;
                }
            }
        }
        _ => return Err(format!("unsupported method {method_id}")),
    }
    Ok(())
}

fn audit_delta(
    bytes: &[u8],
    location: &EvidenceLocation,
    accumulator: &mut AuditAccumulator,
) -> Result<(), String> {
    let delta = parse_message(bytes)?;
    for skill_effect in bytes_fields(&delta, 7) {
        let skill_effect = parse_message(skill_effect)?;
        for damage in bytes_fields(&skill_effect, 2) {
            audit_damage(damage, location, accumulator)?;
        }
    }
    Ok(())
}

fn audit_damage(
    bytes: &[u8],
    location: &EvidenceLocation,
    accumulator: &mut AuditAccumulator,
) -> Result<(), String> {
    accumulator.damage_messages = accumulator.damage_messages.saturating_add(1);
    let fields = parse_message(bytes)?;
    for field in &fields {
        accumulator.observe_field(Scope::Damage, field, location);
        match (field.number, field.bytes) {
            (19, Some(value)) => audit_nested(value, Scope::DamagePosition, location, accumulator)?,
            (20, Some(value)) => {
                let hit_part = parse_message(value)?;
                for nested in &hit_part {
                    accumulator.observe_field(Scope::HitPart, nested, location);
                    if nested.number == 2
                        && let Some(position) = nested.bytes
                    {
                        audit_nested(position, Scope::HitPartPosition, location, accumulator)?;
                    }
                }
            }
            (22, Some(value)) => audit_nested(value, Scope::DamageWeight, location, accumulator)?,
            _ => {}
        }
    }
    Ok(())
}

fn audit_nested(
    bytes: &[u8],
    scope: Scope,
    location: &EvidenceLocation,
    accumulator: &mut AuditAccumulator,
) -> Result<(), String> {
    for field in parse_message(bytes)? {
        accumulator.observe_field(scope, &field, location);
    }
    Ok(())
}

#[derive(Debug)]
struct WireField<'a> {
    number: u32,
    wire_type: u8,
    bytes: Option<&'a [u8]>,
}

fn parse_message(bytes: &[u8]) -> Result<Vec<WireField<'_>>, String> {
    let mut offset = 0usize;
    let mut fields = Vec::new();
    while offset < bytes.len() {
        let field_offset = offset;
        let key = read_varint(bytes, &mut offset)
            .ok_or_else(|| format!("truncated field key at {field_offset}"))?;
        let number = u32::try_from(key >> 3)
            .map_err(|_| format!("field number overflow at {field_offset}"))?;
        let wire_type = (key & 7) as u8;
        if number == 0 {
            return Err(format!("field number zero at {field_offset}"));
        }
        let value = match wire_type {
            0 => {
                read_varint(bytes, &mut offset)
                    .ok_or_else(|| format!("truncated varint at {field_offset}"))?;
                None
            }
            1 => {
                take(bytes, &mut offset, 8)
                    .ok_or_else(|| format!("truncated fixed64 at {field_offset}"))?;
                None
            }
            2 => {
                let length = read_varint(bytes, &mut offset)
                    .and_then(|value| usize::try_from(value).ok())
                    .ok_or_else(|| format!("invalid length at {field_offset}"))?;
                Some(
                    take(bytes, &mut offset, length)
                        .ok_or_else(|| format!("truncated bytes at {field_offset}"))?,
                )
            }
            5 => {
                take(bytes, &mut offset, 4)
                    .ok_or_else(|| format!("truncated fixed32 at {field_offset}"))?;
                None
            }
            other => return Err(format!("unsupported wire type {other} at {field_offset}")),
        };
        fields.push(WireField {
            number,
            wire_type,
            bytes: value,
        });
    }
    Ok(fields)
}

fn bytes_fields<'a>(fields: &'a [WireField<'a>], number: u32) -> impl Iterator<Item = &'a [u8]> {
    fields
        .iter()
        .filter(move |field| field.number == number)
        .filter_map(|field| field.bytes)
}

fn read_varint(bytes: &[u8], offset: &mut usize) -> Option<u64> {
    let mut value = 0u64;
    for shift in (0..70).step_by(7) {
        let byte = *bytes.get(*offset)?;
        *offset = offset.saturating_add(1);
        if shift == 63 && byte > 1 {
            return None;
        }
        value |= u64::from(byte & 0x7f) << shift;
        if byte & 0x80 == 0 {
            return Some(value);
        }
    }
    None
}

fn take<'a>(bytes: &'a [u8], offset: &mut usize, length: usize) -> Option<&'a [u8]> {
    let end = offset.checked_add(length)?;
    let value = bytes.get(*offset..end)?;
    *offset = end;
    Some(value)
}

fn slash_path(path: &Path) -> String {
    path.to_string_lossy().replace('\\', "/")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn current_damage_schema_accepts_all_declared_wire_shapes() {
        for field_number in 1..=25 {
            assert!(Scope::Damage.expected_wire_type(field_number).is_some());
        }
        assert_eq!(Scope::Damage.expected_wire_type(26), None);
        assert_eq!(Scope::Damage.expected_wire_type(19), Some(2));
        assert_eq!(Scope::Damage.expected_wire_type(23), Some(0));
    }

    #[test]
    fn parses_nested_damage_wire_without_treating_payload_as_semantics() {
        let damage = [0x30, 0x9f, 0xbf, 0x05, 0x9a, 0x01, 0x05, 0x0d, 1, 2, 3, 4];
        let fields = parse_message(&damage).expect("valid wire");
        assert_eq!(fields.len(), 2);
        assert_eq!(fields[0].number, 6);
        assert_eq!(fields[0].wire_type, 0);
        assert_eq!(fields[1].number, 19);
        assert_eq!(fields[1].bytes, Some(&[0x0d, 1, 2, 3, 4][..]));
    }
}
