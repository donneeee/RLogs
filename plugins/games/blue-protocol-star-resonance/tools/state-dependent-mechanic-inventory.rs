use std::{
    collections::{BTreeMap, BTreeSet},
    env,
    ffi::OsString,
    fs::File,
    io::{BufReader, BufWriter, Write},
    path::{Path, PathBuf},
};

use rlogs_events::{CanonicalEvent, TimelineEventKind};
use rlogs_log_format::{RlogLimits, RlogReader};
use serde::{
    Deserializer, Serialize,
    de::{DeserializeSeed, IgnoredAny, MapAccess, Visitor},
};
use serde_json::Value;

const SCHEMA_VERSION: u16 = 5;

#[derive(Debug)]
struct Arguments {
    input: PathBuf,
    source_index: PathBuf,
    rlogs: Vec<PathBuf>,
    output: PathBuf,
    client_build: String,
}

#[derive(Debug, Default)]
struct InputMetadata {
    schema_version: Option<u64>,
    generated_by: Option<String>,
    boundaries: Vec<String>,
}

#[derive(Debug, Default)]
struct PacketEvidence {
    sessions: BTreeSet<String>,
    ability_counts: BTreeMap<i64, u64>,
    damage_counts: BTreeMap<i64, u64>,
    healing_counts: BTreeMap<i64, u64>,
    shield_counts: BTreeMap<i64, u64>,
    cast_counts: BTreeMap<i64, u64>,
    cooldown_counts: BTreeMap<i64, u64>,
    effect_counts: BTreeMap<i64, u64>,
    status_origin_counts: BTreeMap<(i32, i64), u64>,
    attribute_counts: BTreeMap<i32, u64>,
}

#[derive(Debug, Serialize)]
struct Inventory {
    schema_version: u16,
    generated_by: &'static str,
    client_build: String,
    source: SourceMetadata,
    direct_source_index: SourceMetadata,
    policy: InventoryPolicy,
    packet_coverage: PacketCoverage,
    summary: InventorySummary,
    mechanics: Vec<Mechanic>,
    direct_sources: Vec<DirectSource>,
}

#[derive(Debug, Serialize)]
struct SourceMetadata {
    path: String,
    schema_version: Option<u64>,
    generated_by: Option<String>,
    boundaries: Vec<String>,
}

#[derive(Debug, Serialize)]
struct InventoryPolicy {
    design_text_is_runtime_formula_authority: bool,
    packet_events_are_runtime_occurrence_authority: bool,
    unresolved_packet_evidence_is_hidden: bool,
    health_shield_or_resource_state_is_discarded: bool,
    mixed_health_mechanic_roles_are_collapsed: bool,
    exact_formula_enablement: &'static str,
    temporal_interpretation: &'static str,
}

#[derive(Debug, Serialize)]
struct PacketCoverage {
    rlogs: Vec<String>,
    sessions: Vec<String>,
    distinct_ability_ids: usize,
    ability_events: u64,
    damage_events: u64,
    healing_events: u64,
    shield_events: u64,
    cast_events: u64,
    cooldown_events: u64,
    distinct_status_effect_ids: usize,
    status_events: u64,
    distinct_status_origins: usize,
    status_origin_observations: u64,
    distinct_attribute_ids: usize,
    attribute_values: u64,
}

#[derive(Debug, Default, Serialize)]
struct InventorySummary {
    formula_entries_scanned: u64,
    state_dependent_entries: usize,
    packet_observed_entries: usize,
    entries_with_packet_observed_relationships: usize,
    entries_by_signal: BTreeMap<String, usize>,
    packet_observed_entries_by_signal: BTreeMap<String, usize>,
    entries_by_category: BTreeMap<String, usize>,
    direct_sources_scanned: u64,
    state_dependent_direct_sources: usize,
    packet_observed_direct_sources: usize,
    direct_sources_by_signal: BTreeMap<String, usize>,
    packet_observed_direct_sources_by_signal: BTreeMap<String, usize>,
}

#[derive(Debug, Serialize)]
struct Mechanic {
    key: String,
    uid: Option<String>,
    category: Option<String>,
    runtime_kind: Option<String>,
    name: Option<String>,
    formula_readiness: Option<String>,
    formula_zone_ids: Vec<String>,
    scope_kinds: Vec<String>,
    stack_policy: Option<String>,
    signals: Vec<String>,
    relationship_ids: Vec<RelationshipIds>,
    packet_evidence: MechanicPacketEvidence,
    matching_text: Vec<TextEvidence>,
}

#[derive(Debug, Serialize)]
struct RelationshipIds {
    relationship: String,
    ids: Vec<i64>,
}

#[derive(Debug, Default, Serialize)]
struct MechanicPacketEvidence {
    direct_uid_ability_events: u64,
    direct_uid_damage_events: u64,
    direct_uid_healing_events: u64,
    direct_uid_shield_events: u64,
    direct_uid_cast_events: u64,
    direct_uid_cooldown_events: u64,
    direct_uid_status_events: u64,
    direct_uid_status_origin_observations: u64,
    related_ability_events: u64,
    related_damage_events: u64,
    related_healing_events: u64,
    related_shield_events: u64,
    related_cast_events: u64,
    related_cooldown_events: u64,
    related_status_events: u64,
    related_status_origin_observations: u64,
    observed_relationships: Vec<ObservedRelationship>,
}

impl MechanicPacketEvidence {
    fn is_observed(&self) -> bool {
        self.direct_uid_ability_events > 0
            || self.direct_uid_status_events > 0
            || self.direct_uid_status_origin_observations > 0
            || self.related_ability_events > 0
            || self.related_status_events > 0
            || self.related_status_origin_observations > 0
    }
}

#[derive(Debug, Serialize)]
struct ObservedRelationship {
    relationship: String,
    id: i64,
    ability_events: u64,
    damage_events: u64,
    healing_events: u64,
    shield_events: u64,
    cast_events: u64,
    cooldown_events: u64,
    status_events: u64,
    status_origin_observations: u64,
}

#[derive(Debug, Serialize)]
struct TextEvidence {
    text: String,
    json_paths: Vec<String>,
    occurrences: usize,
}

#[derive(Debug, Serialize)]
struct DirectSource {
    buff_id: i64,
    source_id: Option<String>,
    source_kind: Option<String>,
    source_type: Option<String>,
    source_entity_id: Option<i64>,
    source_name: Option<String>,
    icon_path: Option<String>,
    signals: Vec<String>,
    related_buff_ids: Vec<i64>,
    packet_evidence: DirectSourcePacketEvidence,
    matching_description_text: Vec<TextEvidence>,
}

#[derive(Debug, Default, Serialize)]
struct DirectSourcePacketEvidence {
    runtime_buff_status_events: u64,
    related_buff_status_events: u64,
    source_entity_ability_events: u64,
    source_entity_damage_events: u64,
    source_entity_healing_events: u64,
    source_entity_shield_events: u64,
    source_entity_cast_events: u64,
    source_entity_cooldown_events: u64,
    source_entity_status_events: u64,
    source_entity_status_origin_observations: u64,
}

impl DirectSourcePacketEvidence {
    fn is_observed(&self) -> bool {
        self.runtime_buff_status_events > 0
            || self.related_buff_status_events > 0
            || self.source_entity_ability_events > 0
            || self.source_entity_status_events > 0
            || self.source_entity_status_origin_observations > 0
    }
}

struct InventoryAccumulator<'a> {
    metadata: InputMetadata,
    packet: &'a PacketEvidence,
    scanned: u64,
    mechanics: Vec<Mechanic>,
}

struct DirectSourceAccumulator<'a> {
    metadata: InputMetadata,
    packet: &'a PacketEvidence,
    scanned: u64,
    sources: Vec<DirectSource>,
}

fn main() {
    if let Err(error) = run() {
        eprintln!("BPSR state-dependent mechanic inventory failed: {error}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), Box<dyn std::error::Error>> {
    let args = parse_arguments(env::args_os().skip(1))?;
    let packet = scan_rlogs(&args.rlogs)?;
    let file = File::open(&args.input)?;
    let mut deserializer = serde_json::Deserializer::from_reader(BufReader::new(file));
    let mut accumulator = InventoryAccumulator {
        metadata: InputMetadata::default(),
        packet: &packet,
        scanned: 0,
        mechanics: Vec::new(),
    };
    RootSeed {
        accumulator: &mut accumulator,
    }
    .deserialize(&mut deserializer)?;

    let mut direct_accumulator = scan_direct_source_index(&args.source_index, &packet)?;

    accumulator
        .mechanics
        .sort_by(|left, right| left.key.cmp(&right.key));
    direct_accumulator.sources.sort_by(|left, right| {
        left.buff_id
            .cmp(&right.buff_id)
            .then_with(|| left.source_id.cmp(&right.source_id))
    });
    let summary = build_summary(
        accumulator.scanned,
        &accumulator.mechanics,
        direct_accumulator.scanned,
        &direct_accumulator.sources,
    );
    let inventory = Inventory {
        schema_version: SCHEMA_VERSION,
        generated_by: "rlogs-bpsr-state-dependent-mechanic-inventory",
        client_build: args.client_build,
        source: SourceMetadata {
            path: display_path(&args.input),
            schema_version: accumulator.metadata.schema_version,
            generated_by: accumulator.metadata.generated_by,
            boundaries: accumulator.metadata.boundaries,
        },
        direct_source_index: SourceMetadata {
            path: display_path(&args.source_index),
            schema_version: direct_accumulator.metadata.schema_version,
            generated_by: direct_accumulator.metadata.generated_by,
            boundaries: direct_accumulator.metadata.boundaries,
        },
        policy: InventoryPolicy {
            design_text_is_runtime_formula_authority: false,
            packet_events_are_runtime_occurrence_authority: true,
            unresolved_packet_evidence_is_hidden: false,
            health_shield_or_resource_state_is_discarded: false,
            mixed_health_mechanic_roles_are_collapsed: false,
            exact_formula_enablement: "requires packet-observed state, exact effect or ability origin, numeric unit, operation order, provider, recipient, stacking, and marginal replay proof",
            temporal_interpretation: "a state change may be the preceding hit's output and the next hit's input, threshold, or resource predicate; timing alone never assigns the role",
        },
        packet_coverage: packet_coverage(&args.rlogs, &packet),
        summary,
        mechanics: accumulator.mechanics,
        direct_sources: direct_accumulator.sources,
    };

    let mut writer = BufWriter::new(File::create(args.output)?);
    serde_json::to_writer_pretty(&mut writer, &inventory)?;
    writer.write_all(b"\n")?;
    writer.flush()?;
    Ok(())
}

fn scan_direct_source_index<'a>(
    path: &Path,
    packet: &'a PacketEvidence,
) -> Result<DirectSourceAccumulator<'a>, Box<dyn std::error::Error>> {
    let file = File::open(path)?;
    let mut deserializer = serde_json::Deserializer::from_reader(BufReader::new(file));
    let mut accumulator = DirectSourceAccumulator {
        metadata: InputMetadata::default(),
        packet,
        scanned: 0,
        sources: Vec::new(),
    };
    DirectSourceRootSeed {
        accumulator: &mut accumulator,
    }
    .deserialize(&mut deserializer)?;
    Ok(accumulator)
}

struct DirectSourceRootSeed<'a, 'p> {
    accumulator: &'a mut DirectSourceAccumulator<'p>,
}

impl<'de> DeserializeSeed<'de> for DirectSourceRootSeed<'_, '_> {
    type Value = ();

    fn deserialize<D>(self, deserializer: D) -> Result<Self::Value, D::Error>
    where
        D: Deserializer<'de>,
    {
        deserializer.deserialize_map(DirectSourceRootVisitor {
            accumulator: self.accumulator,
        })
    }
}

struct DirectSourceRootVisitor<'a, 'p> {
    accumulator: &'a mut DirectSourceAccumulator<'p>,
}

impl<'de> Visitor<'de> for DirectSourceRootVisitor<'_, '_> {
    type Value = ();

    fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("a modifier source index object")
    }

    fn visit_map<M>(self, mut map: M) -> Result<Self::Value, M::Error>
    where
        M: MapAccess<'de>,
    {
        while let Some(key) = map.next_key::<String>()? {
            match key.as_str() {
                "schemaVersion" => {
                    self.accumulator.metadata.schema_version = map.next_value()?;
                }
                "generatedBy" => {
                    self.accumulator.metadata.generated_by = map.next_value()?;
                }
                "boundaries" => {
                    self.accumulator.metadata.boundaries = map.next_value()?;
                }
                "byBuffId" => {
                    map.next_value_seed(DirectSourcesSeed {
                        accumulator: self.accumulator,
                    })?;
                }
                _ => {
                    map.next_value::<IgnoredAny>()?;
                }
            }
        }
        Ok(())
    }
}

struct DirectSourcesSeed<'a, 'p> {
    accumulator: &'a mut DirectSourceAccumulator<'p>,
}

impl<'de> DeserializeSeed<'de> for DirectSourcesSeed<'_, '_> {
    type Value = ();

    fn deserialize<D>(self, deserializer: D) -> Result<Self::Value, D::Error>
    where
        D: Deserializer<'de>,
    {
        deserializer.deserialize_map(DirectSourcesVisitor {
            accumulator: self.accumulator,
        })
    }
}

struct DirectSourcesVisitor<'a, 'p> {
    accumulator: &'a mut DirectSourceAccumulator<'p>,
}

impl<'de> Visitor<'de> for DirectSourcesVisitor<'_, '_> {
    type Value = ();

    fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("the byBuffId object")
    }

    fn visit_map<M>(self, mut map: M) -> Result<Self::Value, M::Error>
    where
        M: MapAccess<'de>,
    {
        while let Some((buff_key, sources)) = map.next_entry::<String, Value>()? {
            let Ok(buff_id) = buff_key.parse::<i64>() else {
                continue;
            };
            let Value::Array(sources) = sources else {
                continue;
            };
            for source in sources {
                self.accumulator.scanned = self.accumulator.scanned.saturating_add(1);
                if let Some(source) = build_direct_source(buff_id, &source, self.accumulator.packet)
                {
                    self.accumulator.sources.push(source);
                }
            }
        }
        Ok(())
    }
}

fn build_direct_source(
    buff_id: i64,
    source: &Value,
    packet: &PacketEvidence,
) -> Option<DirectSource> {
    let object = source.as_object()?;
    let mut evidence_by_text = BTreeMap::<String, (BTreeSet<String>, usize)>::new();
    let mut signals = BTreeSet::<String>::new();

    // Only source-owned descriptions are formula-candidate text. Generated attribution
    // models and bridged page contexts are deliberately excluded because they can carry
    // broad class-page text onto unrelated runtime buff IDs.
    if let Some(description) = object.get("description").and_then(Value::as_str) {
        collect_text_evidence(
            &Value::String(description.to_owned()),
            "$.description",
            &mut evidence_by_text,
            &mut signals,
        );
    } else if let Some(description) = object
        .get("descriptions")
        .and_then(Value::as_object)
        .and_then(|descriptions| descriptions.get("en"))
    {
        collect_text_evidence(
            description,
            "$.descriptions.en",
            &mut evidence_by_text,
            &mut signals,
        );
    }
    if signals.is_empty() {
        return None;
    }

    let source_entity_id = numeric_value(object.get("sourceEntityId"));
    let mut related_buff_ids = BTreeSet::new();
    if let Some(value) = object.get("buffIds") {
        collect_numeric_ids(value, &mut related_buff_ids);
    }
    let packet_evidence =
        direct_source_packet_evidence(buff_id, source_entity_id, &related_buff_ids, packet);
    let matching_description_text = evidence_by_text
        .into_iter()
        .map(|(text, (paths, occurrences))| TextEvidence {
            text,
            json_paths: paths.into_iter().collect(),
            occurrences,
        })
        .collect();

    Some(DirectSource {
        buff_id,
        source_id: string_value(object.get("sourceId")),
        source_kind: string_value(object.get("sourceKind")),
        source_type: string_value(object.get("sourceType")),
        source_entity_id,
        source_name: string_value(object.get("sourceName")),
        icon_path: string_value(object.get("iconPath")),
        signals: signals.into_iter().collect(),
        related_buff_ids: related_buff_ids.into_iter().collect(),
        packet_evidence,
        matching_description_text,
    })
}

fn direct_source_packet_evidence(
    buff_id: i64,
    source_entity_id: Option<i64>,
    related_buff_ids: &BTreeSet<i64>,
    packet: &PacketEvidence,
) -> DirectSourcePacketEvidence {
    let mut evidence = DirectSourcePacketEvidence {
        runtime_buff_status_events: count(&packet.effect_counts, buff_id),
        ..DirectSourcePacketEvidence::default()
    };
    for related_buff_id in related_buff_ids {
        if *related_buff_id != buff_id {
            evidence.related_buff_status_events = evidence
                .related_buff_status_events
                .saturating_add(count(&packet.effect_counts, *related_buff_id));
        }
    }
    if let Some(source_entity_id) = source_entity_id {
        evidence.source_entity_ability_events = count(&packet.ability_counts, source_entity_id);
        evidence.source_entity_damage_events = count(&packet.damage_counts, source_entity_id);
        evidence.source_entity_healing_events = count(&packet.healing_counts, source_entity_id);
        evidence.source_entity_shield_events = count(&packet.shield_counts, source_entity_id);
        evidence.source_entity_cast_events = count(&packet.cast_counts, source_entity_id);
        evidence.source_entity_cooldown_events = count(&packet.cooldown_counts, source_entity_id);
        evidence.source_entity_status_events = count(&packet.effect_counts, source_entity_id);
        evidence.source_entity_status_origin_observations =
            status_origin_count(packet, source_entity_id);
    }
    evidence
}

struct RootSeed<'a, 'p> {
    accumulator: &'a mut InventoryAccumulator<'p>,
}

impl<'de> DeserializeSeed<'de> for RootSeed<'_, '_> {
    type Value = ();

    fn deserialize<D>(self, deserializer: D) -> Result<Self::Value, D::Error>
    where
        D: Deserializer<'de>,
    {
        deserializer.deserialize_map(RootVisitor {
            accumulator: self.accumulator,
        })
    }
}

struct RootVisitor<'a, 'p> {
    accumulator: &'a mut InventoryAccumulator<'p>,
}

impl<'de> Visitor<'de> for RootVisitor<'_, '_> {
    type Value = ();

    fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("a modifier formula term table object")
    }

    fn visit_map<M>(self, mut map: M) -> Result<Self::Value, M::Error>
    where
        M: MapAccess<'de>,
    {
        while let Some(key) = map.next_key::<String>()? {
            match key.as_str() {
                "schemaVersion" => {
                    self.accumulator.metadata.schema_version = map.next_value()?;
                }
                "generatedBy" => {
                    self.accumulator.metadata.generated_by = map.next_value()?;
                }
                "boundaries" => {
                    self.accumulator.metadata.boundaries = map.next_value()?;
                }
                "entriesByKey" => {
                    map.next_value_seed(EntriesSeed {
                        accumulator: self.accumulator,
                    })?;
                }
                _ => {
                    map.next_value::<IgnoredAny>()?;
                }
            }
        }
        Ok(())
    }
}

struct EntriesSeed<'a, 'p> {
    accumulator: &'a mut InventoryAccumulator<'p>,
}

impl<'de> DeserializeSeed<'de> for EntriesSeed<'_, '_> {
    type Value = ();

    fn deserialize<D>(self, deserializer: D) -> Result<Self::Value, D::Error>
    where
        D: Deserializer<'de>,
    {
        deserializer.deserialize_map(EntriesVisitor {
            accumulator: self.accumulator,
        })
    }
}

struct EntriesVisitor<'a, 'p> {
    accumulator: &'a mut InventoryAccumulator<'p>,
}

impl<'de> Visitor<'de> for EntriesVisitor<'_, '_> {
    type Value = ();

    fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("the entriesByKey object")
    }

    fn visit_map<M>(self, mut map: M) -> Result<Self::Value, M::Error>
    where
        M: MapAccess<'de>,
    {
        while let Some((key, entry)) = map.next_entry::<String, Value>()? {
            self.accumulator.scanned = self.accumulator.scanned.saturating_add(1);
            if let Some(mechanic) = build_mechanic(key, &entry, self.accumulator.packet) {
                self.accumulator.mechanics.push(mechanic);
            }
        }
        Ok(())
    }
}

fn build_mechanic(key: String, entry: &Value, packet: &PacketEvidence) -> Option<Mechanic> {
    let mut evidence_by_text = BTreeMap::<String, (BTreeSet<String>, usize)>::new();
    let mut signals = BTreeSet::<String>::new();
    collect_text_evidence(entry, "$", &mut evidence_by_text, &mut signals);
    if signals.is_empty() {
        return None;
    }

    let relationship_ids = collect_relationship_ids(entry.get("relationships"));
    let uid = string_value(entry.get("uid"));
    let packet_evidence = mechanic_packet_evidence(uid.as_deref(), &relationship_ids, packet);
    let matching_text = evidence_by_text
        .into_iter()
        .map(|(text, (paths, occurrences))| TextEvidence {
            text,
            json_paths: paths.into_iter().collect(),
            occurrences,
        })
        .collect();

    Some(Mechanic {
        key,
        uid,
        category: string_value(entry.get("category")),
        runtime_kind: string_value(entry.get("runtimeKind")),
        name: string_value(entry.get("name")),
        formula_readiness: string_value(entry.get("formulaReadiness")),
        formula_zone_ids: string_array(entry.get("formulaZoneIds")),
        scope_kinds: string_array(entry.get("scopeKinds")),
        stack_policy: string_value(entry.get("stackPolicy")),
        signals: signals.into_iter().collect(),
        relationship_ids,
        packet_evidence,
        matching_text,
    })
}

fn collect_text_evidence(
    value: &Value,
    path: &str,
    evidence: &mut BTreeMap<String, (BTreeSet<String>, usize)>,
    signals: &mut BTreeSet<String>,
) {
    match value {
        Value::Object(object) => {
            for (key, child) in object {
                collect_text_evidence(child, &format!("{path}.{key}"), evidence, signals);
            }
        }
        Value::Array(array) => {
            for (index, child) in array.iter().enumerate() {
                collect_text_evidence(child, &format!("{path}[{index}]"), evidence, signals);
            }
        }
        Value::String(text) => {
            let detected = detect_signals(text);
            if detected.is_empty() {
                return;
            }
            signals.extend(detected);
            let (paths, occurrences) = evidence.entry(text.clone()).or_default();
            paths.insert(path.to_owned());
            *occurrences = occurrences.saturating_add(1);
        }
        _ => {}
    }
}

fn detect_signals(text: &str) -> BTreeSet<String> {
    let lower = text.to_lowercase();
    let normalized = lower
        .chars()
        .map(|character| {
            if character.is_alphanumeric() || character == '%' {
                character
            } else {
                ' '
            }
        })
        .collect::<String>();
    let tokens = normalized.split_whitespace().collect::<Vec<_>>();
    let has_hp = tokens.contains(&"hp")
        || lower.contains("health")
        || lower.contains("maxhp")
        || lower.contains("currenthp")
        || lower.contains("missinghp");
    let has_damage = lower.contains("damage") || lower.contains("dmg");
    let has_percent = lower.contains('%') || lower.contains("percent");
    let mut signals = BTreeSet::new();

    if has_hp {
        signals.insert("health_state_general".to_owned());
        if contains_any(
            &lower,
            &["max hp", "maxhp", "maximum hp", "max. hp", "maximum health"],
        ) {
            signals.insert("maximum_health_scaling_or_gate".to_owned());
        }
        if contains_any(&lower, &["current hp", "current health", "remaining hp"]) {
            signals.insert("current_health_scaling_or_gate".to_owned());
        }
        if contains_any(
            &lower,
            &["missing hp", "missing health", "lost hp", "hp lost"],
        ) {
            signals.insert("missing_health_scaling_or_gate".to_owned());
        }
        if contains_any(
            &lower,
            &[
                "low hp",
                "hp is below",
                "hp below",
                "health is below",
                "health below",
                "hp is above",
                "hp above",
                "health is above",
                "health above",
            ],
        ) || (has_percent
            && contains_any(
                &lower,
                &["below", "above", "under", "over", "less than", "more than"],
            ))
        {
            signals.insert("health_threshold_or_gate".to_owned());
        }
        if has_damage {
            signals.insert("health_and_damage_cooccurrence".to_owned());
        }
        if is_health_dependent_damage_candidate(&lower, has_percent) {
            signals.insert("health_dependent_damage_candidate".to_owned());
            if has_percent {
                signals.insert("percentage_health_damage_candidate".to_owned());
            }
        }
        if is_health_dependent_healing_candidate(&lower) {
            signals.insert("health_dependent_healing_candidate".to_owned());
        }
        if is_health_dependent_shield_candidate(&lower) {
            signals.insert("health_dependent_shield_candidate".to_owned());
        }
        if is_health_stat_modifier(&lower) {
            signals.insert("health_stat_modifier".to_owned());
        }
        if contains_any(
            &lower,
            &["consume hp", "costs hp", "hp cost", "sacrifice hp"],
        ) {
            signals.insert("health_cost".to_owned());
        }
    }
    if contains_any(&lower, &["shield", "barrier"]) {
        signals.insert("shield_state_scaling_or_gate".to_owned());
        if has_damage {
            signals.insert("shield_and_damage_cooccurrence".to_owned());
        }
        if is_state_dependent_damage_candidate(&lower, &["shield", "barrier"]) {
            signals.insert("shield_dependent_damage_candidate".to_owned());
        }
    }
    if contains_any(
        &lower,
        &[
            " energy", "energy ", "gauge", "resource", " mana", "mana ", " rage", "rage ",
        ],
    ) {
        signals.insert("resource_state_scaling_or_gate".to_owned());
        if contains_any(&lower, &["consume", "cost", "spend", "drain"]) {
            signals.insert("resource_consumption".to_owned());
        }
        if contains_any(&lower, &["grant", "gain", "generate", "restore", "recover"]) {
            signals.insert("resource_generation".to_owned());
        }
        if has_damage {
            signals.insert("resource_and_damage_cooccurrence".to_owned());
        }
        if is_state_dependent_damage_candidate(
            &lower,
            &["energy", "gauge", "resource", "mana", "rage"],
        ) {
            signals.insert("resource_dependent_damage_candidate".to_owned());
        }
    }
    signals
}

fn is_health_dependent_damage_candidate(text: &str, _has_percent: bool) -> bool {
    health_is_first_formula_basis(text)
        || contains_any(
            text,
            &[
                "max hp as damage",
                "maximum hp as damage",
                "of max hp as damage",
                "of maximum hp as damage",
                "based on max hp",
                "based on maxhp",
                "based on maximum hp",
                "damage for each hp",
                "dmg for each hp",
            ],
        )
}

fn is_health_dependent_healing_candidate(text: &str) -> bool {
    health_is_first_output_basis(
        text,
        &[
            "heal equal to",
            "healing equal to",
            "restore hp equal to",
            "restores hp equal to",
            "recover hp equal to",
            "recovers hp equal to",
            "heal based on",
            "healing based on",
            "restore hp based on",
            "restores hp based on",
            "recover hp based on",
            "recovers hp based on",
            "restore ",
            "restores ",
            "recover ",
            "recovers ",
            "heal ",
            "heals ",
        ],
    ) || (contains_any(
        text,
        &[
            "of max hp",
            "of maximum hp",
            "of missing hp",
            "of missing health",
            "based on max hp",
            "based on maximum hp",
            "based on missing hp",
            "based on missing health",
        ],
    ) && contains_any(
        text,
        &[
            "heal",
            "healing",
            "restore ",
            "restores ",
            "restore hp",
            "restores hp",
            "recover ",
            "recovers ",
            "recover hp",
            "recovers hp",
        ],
    ))
}

fn is_health_dependent_shield_candidate(text: &str) -> bool {
    health_is_first_output_basis(
        text,
        &[
            "shield equal to",
            "barrier equal to",
            "shield based on",
            "barrier based on",
            "absorbs damage equal to",
            "absorbs dmg equal to",
            "absorb damage equal to",
            "absorb dmg equal to",
        ],
    ) || (contains_any(
        text,
        &[
            "of max hp",
            "of maximum hp",
            "based on max hp",
            "based on maximum hp",
        ],
    ) && contains_any(text, &["shield", "barrier", "absorb"]))
}

fn is_health_stat_modifier(text: &str) -> bool {
    contains_any(
        text,
        &[
            "increase max hp",
            "increases max hp",
            "increased max hp",
            "max hp increases",
            "max hp is increased",
            "decrease max hp",
            "decreases max hp",
            "decreased max hp",
            "max hp decreases",
            "max hp is decreased",
            "maximum hp increases",
            "maximum hp is increased",
            "maximum hp decreases",
            "maximum hp is decreased",
        ],
    )
}

fn health_is_first_output_basis(text: &str, relations: &[&str]) -> bool {
    for relation in relations {
        let mut remainder = text;
        while let Some(position) = remainder.find(relation) {
            let basis = &remainder[position + relation.len()..];
            let clause_end = basis.find(['.', ';', '\n']).unwrap_or(basis.len());
            let clause = &basis[..clause_end];
            if first_position(
                clause,
                &[
                    "max hp",
                    "maximum hp",
                    "current hp",
                    "missing hp",
                    "missing health",
                    " health",
                    " hp",
                ],
            )
            .is_some()
            {
                return true;
            }
            remainder = &basis[clause_end..];
            if remainder.is_empty() {
                break;
            }
        }
    }
    false
}

fn health_is_first_formula_basis(text: &str) -> bool {
    for relation in [
        "damage equal to",
        "dmg equal to",
        "damage based on",
        "dmg based on",
        "damage according to",
        "dmg according to",
    ] {
        let mut remainder = text;
        while let Some(position) = remainder.find(relation) {
            let basis = &remainder[position + relation.len()..];
            let clause_end = basis.find(['.', ';', '\n']).unwrap_or(basis.len());
            let clause = &basis[..clause_end];
            let health_position = first_position(
                clause,
                &[
                    "max hp",
                    "maximum hp",
                    "current hp",
                    "missing hp",
                    " health",
                    " hp",
                ],
            );
            let other_basis_position = first_position(
                clause,
                &[
                    " atk",
                    "attack",
                    " pdef",
                    "physical defense",
                    " mdef",
                    "magic defense",
                ],
            );
            if let Some(health_position) = health_position
                && other_basis_position.is_none_or(|other| health_position < other)
            {
                return true;
            }
            remainder = &basis[clause_end..];
            if remainder.is_empty() {
                break;
            }
        }
    }
    false
}

fn first_position(haystack: &str, needles: &[&str]) -> Option<usize> {
    needles
        .iter()
        .filter_map(|needle| haystack.find(needle))
        .min()
}

fn is_state_dependent_damage_candidate(text: &str, states: &[&str]) -> bool {
    let has_state = states.iter().any(|state| text.contains(state));
    if !has_state || !(text.contains("damage") || text.contains("dmg")) {
        return false;
    }
    contains_any(
        text,
        &[
            "damage equal to",
            "dmg equal to",
            "damage based on",
            "dmg based on",
            "damage according to",
            "dmg according to",
            "damage for each",
            "dmg for each",
            "consume",
            "spend",
            "drain",
        ],
    )
}

fn contains_any(haystack: &str, needles: &[&str]) -> bool {
    needles.iter().any(|needle| haystack.contains(needle))
}

fn collect_relationship_ids(value: Option<&Value>) -> Vec<RelationshipIds> {
    let Some(Value::Object(object)) = value else {
        return Vec::new();
    };
    object
        .iter()
        .filter_map(|(relationship, value)| {
            let mut ids = BTreeSet::new();
            collect_numeric_ids(value, &mut ids);
            (!ids.is_empty()).then(|| RelationshipIds {
                relationship: relationship.clone(),
                ids: ids.into_iter().collect(),
            })
        })
        .collect()
}

fn collect_numeric_ids(value: &Value, ids: &mut BTreeSet<i64>) {
    match value {
        Value::Number(number) => {
            if let Some(id) = number.as_i64() {
                ids.insert(id);
            }
        }
        Value::String(text) => {
            if let Ok(id) = text.parse::<i64>() {
                ids.insert(id);
            }
        }
        Value::Array(array) => {
            for child in array {
                collect_numeric_ids(child, ids);
            }
        }
        Value::Object(object) => {
            for child in object.values() {
                collect_numeric_ids(child, ids);
            }
        }
        _ => {}
    }
}

fn mechanic_packet_evidence(
    uid: Option<&str>,
    relationships: &[RelationshipIds],
    packet: &PacketEvidence,
) -> MechanicPacketEvidence {
    let mut evidence = MechanicPacketEvidence::default();
    if let Some(id) = uid.and_then(|value| value.parse::<i64>().ok()) {
        evidence.direct_uid_ability_events = count(&packet.ability_counts, id);
        evidence.direct_uid_damage_events = count(&packet.damage_counts, id);
        evidence.direct_uid_healing_events = count(&packet.healing_counts, id);
        evidence.direct_uid_shield_events = count(&packet.shield_counts, id);
        evidence.direct_uid_cast_events = count(&packet.cast_counts, id);
        evidence.direct_uid_cooldown_events = count(&packet.cooldown_counts, id);
        evidence.direct_uid_status_events = count(&packet.effect_counts, id);
        evidence.direct_uid_status_origin_observations = status_origin_count(packet, id);
    }
    for relationship in relationships {
        for id in &relationship.ids {
            let ability_events = count(&packet.ability_counts, *id);
            let damage_events = count(&packet.damage_counts, *id);
            let healing_events = count(&packet.healing_counts, *id);
            let shield_events = count(&packet.shield_counts, *id);
            let cast_events = count(&packet.cast_counts, *id);
            let cooldown_events = count(&packet.cooldown_counts, *id);
            let status_events = count(&packet.effect_counts, *id);
            let status_origin_observations = status_origin_count(packet, *id);
            evidence.related_ability_events = evidence
                .related_ability_events
                .saturating_add(ability_events);
            evidence.related_damage_events =
                evidence.related_damage_events.saturating_add(damage_events);
            evidence.related_healing_events = evidence
                .related_healing_events
                .saturating_add(healing_events);
            evidence.related_shield_events =
                evidence.related_shield_events.saturating_add(shield_events);
            evidence.related_cast_events = evidence.related_cast_events.saturating_add(cast_events);
            evidence.related_cooldown_events = evidence
                .related_cooldown_events
                .saturating_add(cooldown_events);
            evidence.related_status_events =
                evidence.related_status_events.saturating_add(status_events);
            evidence.related_status_origin_observations = evidence
                .related_status_origin_observations
                .saturating_add(status_origin_observations);
            if ability_events > 0 || status_events > 0 || status_origin_observations > 0 {
                evidence.observed_relationships.push(ObservedRelationship {
                    relationship: relationship.relationship.clone(),
                    id: *id,
                    ability_events,
                    damage_events,
                    healing_events,
                    shield_events,
                    cast_events,
                    cooldown_events,
                    status_events,
                    status_origin_observations,
                });
            }
        }
    }
    evidence
}

fn count(counts: &BTreeMap<i64, u64>, id: i64) -> u64 {
    counts.get(&id).copied().unwrap_or(0)
}

fn status_origin_count(packet: &PacketEvidence, id: i64) -> u64 {
    packet
        .status_origin_counts
        .iter()
        .filter_map(|((_, config_id), count)| (*config_id == id).then_some(*count))
        .sum()
}

fn scan_rlogs(paths: &[PathBuf]) -> Result<PacketEvidence, Box<dyn std::error::Error>> {
    let mut packet = PacketEvidence::default();
    for path in paths {
        let file = File::open(path)?;
        let mut reader = RlogReader::new(BufReader::new(file), RlogLimits::default())?;
        while let Some(envelope) = reader.next_event()? {
            packet.sessions.insert(envelope.session_id.clone());
            let CanonicalEvent::Timeline(timeline) = envelope.event else {
                continue;
            };
            match timeline.kind {
                TimelineEventKind::Damage(event) => {
                    if let Some(ability) = event.ability {
                        increment(&mut packet.ability_counts, ability.0);
                        increment(&mut packet.damage_counts, ability.0);
                    }
                }
                TimelineEventKind::Healing(event) => {
                    if let Some(ability) = event.ability {
                        increment(&mut packet.ability_counts, ability.0);
                        increment(&mut packet.healing_counts, ability.0);
                    }
                }
                TimelineEventKind::Shield(event) => {
                    increment(&mut packet.ability_counts, event.ability.0);
                    increment(&mut packet.shield_counts, event.ability.0);
                }
                TimelineEventKind::Cast(event) => {
                    increment(&mut packet.ability_counts, event.ability.0);
                    increment(&mut packet.cast_counts, event.ability.0);
                }
                TimelineEventKind::Cooldown(event) => {
                    increment(&mut packet.ability_counts, event.ability.0);
                    increment(&mut packet.cooldown_counts, event.ability.0);
                }
                TimelineEventKind::Status(event) => {
                    increment(&mut packet.effect_counts, event.effect.0);
                    if let Some(origin) = event.origin {
                        let count = packet
                            .status_origin_counts
                            .entry((origin.source_type_id, origin.source_config_id))
                            .or_default();
                        *count = count.saturating_add(1);
                    }
                }
                TimelineEventKind::EntityAttributes(event) => {
                    for attribute in event.attributes {
                        let count = packet
                            .attribute_counts
                            .entry(attribute.attribute_id)
                            .or_default();
                        *count = count.saturating_add(1);
                    }
                }
                _ => {}
            }
        }
    }
    Ok(packet)
}

fn increment(counts: &mut BTreeMap<i64, u64>, id: i64) {
    let count = counts.entry(id).or_default();
    *count = count.saturating_add(1);
}

fn packet_coverage(paths: &[PathBuf], packet: &PacketEvidence) -> PacketCoverage {
    PacketCoverage {
        rlogs: paths.iter().map(|path| display_path(path)).collect(),
        sessions: packet.sessions.iter().cloned().collect(),
        distinct_ability_ids: packet.ability_counts.len(),
        ability_events: packet.ability_counts.values().copied().sum(),
        damage_events: packet.damage_counts.values().copied().sum(),
        healing_events: packet.healing_counts.values().copied().sum(),
        shield_events: packet.shield_counts.values().copied().sum(),
        cast_events: packet.cast_counts.values().copied().sum(),
        cooldown_events: packet.cooldown_counts.values().copied().sum(),
        distinct_status_effect_ids: packet.effect_counts.len(),
        status_events: packet.effect_counts.values().copied().sum(),
        distinct_status_origins: packet.status_origin_counts.len(),
        status_origin_observations: packet.status_origin_counts.values().copied().sum(),
        distinct_attribute_ids: packet.attribute_counts.len(),
        attribute_values: packet.attribute_counts.values().copied().sum(),
    }
}

fn build_summary(
    scanned: u64,
    mechanics: &[Mechanic],
    direct_sources_scanned: u64,
    direct_sources: &[DirectSource],
) -> InventorySummary {
    let mut summary = InventorySummary {
        formula_entries_scanned: scanned,
        state_dependent_entries: mechanics.len(),
        direct_sources_scanned,
        state_dependent_direct_sources: direct_sources.len(),
        ..InventorySummary::default()
    };
    for mechanic in mechanics {
        let packet_observed = mechanic.packet_evidence.is_observed();
        if packet_observed {
            summary.packet_observed_entries = summary.packet_observed_entries.saturating_add(1);
        }
        if !mechanic.packet_evidence.observed_relationships.is_empty() {
            summary.entries_with_packet_observed_relationships = summary
                .entries_with_packet_observed_relationships
                .saturating_add(1);
        }
        for signal in &mechanic.signals {
            let count = summary.entries_by_signal.entry(signal.clone()).or_default();
            *count = count.saturating_add(1);
            if packet_observed {
                let observed_count = summary
                    .packet_observed_entries_by_signal
                    .entry(signal.clone())
                    .or_default();
                *observed_count = observed_count.saturating_add(1);
            }
        }
        let category = mechanic.category.as_deref().unwrap_or("unclassified");
        let count = summary
            .entries_by_category
            .entry(category.to_owned())
            .or_default();
        *count = count.saturating_add(1);
    }
    for source in direct_sources {
        let packet_observed = source.packet_evidence.is_observed();
        if packet_observed {
            summary.packet_observed_direct_sources =
                summary.packet_observed_direct_sources.saturating_add(1);
        }
        for signal in &source.signals {
            let count = summary
                .direct_sources_by_signal
                .entry(signal.clone())
                .or_default();
            *count = count.saturating_add(1);
            if packet_observed {
                let observed_count = summary
                    .packet_observed_direct_sources_by_signal
                    .entry(signal.clone())
                    .or_default();
                *observed_count = observed_count.saturating_add(1);
            }
        }
    }
    summary
}

fn string_value(value: Option<&Value>) -> Option<String> {
    value.and_then(Value::as_str).map(str::to_owned)
}

fn numeric_value(value: Option<&Value>) -> Option<i64> {
    match value? {
        Value::Number(number) => number.as_i64(),
        Value::String(text) => text.parse().ok(),
        _ => None,
    }
}

fn string_array(value: Option<&Value>) -> Vec<String> {
    value
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .map(str::to_owned)
        .collect()
}

fn parse_arguments<I>(mut arguments: I) -> Result<Arguments, String>
where
    I: Iterator<Item = OsString>,
{
    let mut input = None;
    let mut source_index = None;
    let mut rlogs = Vec::new();
    let mut output = None;
    let mut client_build = None;
    while let Some(flag) = arguments.next() {
        match flag.to_string_lossy().as_ref() {
            "--input" => input = Some(next_path(&mut arguments, "--input")?),
            "--source-index" => source_index = Some(next_path(&mut arguments, "--source-index")?),
            "--rlog" => rlogs.push(next_path(&mut arguments, "--rlog")?),
            "--output" => output = Some(next_path(&mut arguments, "--output")?),
            "--client-build" => {
                client_build = Some(
                    arguments
                        .next()
                        .ok_or_else(usage)?
                        .into_string()
                        .map_err(|_| "--client-build must be UTF-8".to_owned())?,
                );
            }
            _ => return Err(usage()),
        }
    }
    let args = Arguments {
        input: input.ok_or_else(usage)?,
        source_index: source_index.ok_or_else(usage)?,
        rlogs,
        output: output.ok_or_else(usage)?,
        client_build: client_build.ok_or_else(usage)?,
    };
    if args.rlogs.is_empty() {
        return Err("at least one --rlog is required for packet observation evidence".to_owned());
    }
    Ok(args)
}

fn next_path<I>(arguments: &mut I, flag: &str) -> Result<PathBuf, String>
where
    I: Iterator<Item = OsString>,
{
    arguments
        .next()
        .map(PathBuf::from)
        .ok_or_else(|| format!("missing path after {flag}\n{}", usage()))
}

fn usage() -> String {
    "usage: rlogs-bpsr-state-dependent-mechanic-inventory --input <ModifierFormulaTermTable.json> --source-index <ModifierSourceIndex.json> --rlog <current-decoder.rlog> [--rlog ...] --client-build <build> --output <inventory.json>".to_owned()
}

fn display_path(path: &Path) -> String {
    path.to_string_lossy().replace('\\', "/")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_health_formula_roles_without_discarding_general_hp() {
        let signals = detect_signals(
            "Explosion deals damage equal to 6% of your maximum HP while below 30% HP.",
        );
        assert!(signals.contains("health_state_general"));
        assert!(signals.contains("maximum_health_scaling_or_gate"));
        assert!(signals.contains("health_threshold_or_gate"));
        assert!(signals.contains("health_and_damage_cooccurrence"));
        assert!(signals.contains("health_dependent_damage_candidate"));
        assert!(signals.contains("percentage_health_damage_candidate"));
    }

    #[test]
    fn detects_resource_generation_and_consumption() {
        let signals = detect_signals(
            "Casting the skill grants 4 energy. Consume 10 energy to increase damage.",
        );
        assert!(signals.contains("resource_state_scaling_or_gate"));
        assert!(signals.contains("resource_generation"));
        assert!(signals.contains("resource_consumption"));
        assert!(signals.contains("resource_and_damage_cooccurrence"));
        assert!(signals.contains("resource_dependent_damage_candidate"));
    }

    #[test]
    fn max_health_and_damage_reduction_is_not_an_hp_damage_formula() {
        let signals = detect_signals(
            "While Heroic Melody is active, DMG Reduction increases by 25%. Each stack increases Max HP by 0.5%, up to 2.5%.",
        );
        assert!(signals.contains("health_state_general"));
        assert!(signals.contains("maximum_health_scaling_or_gate"));
        assert!(signals.contains("health_and_damage_cooccurrence"));
        assert!(!signals.contains("health_dependent_damage_candidate"));
    }

    #[test]
    fn atk_damage_and_max_health_recovery_are_separate_formula_bases() {
        let signals =
            detect_signals("Deals Attack DMG equal to 500% of ATK and restores 15% of Max HP.");
        assert!(signals.contains("health_state_general"));
        assert!(signals.contains("health_and_damage_cooccurrence"));
        assert!(!signals.contains("health_dependent_damage_candidate"));
        assert!(signals.contains("health_dependent_healing_candidate"));
    }

    #[test]
    fn mixed_max_health_damage_and_recovery_retains_both_roles() {
        let signals =
            detect_signals("Deals damage equal to 6% of Max HP and restores 15% of Max HP.");
        assert!(signals.contains("health_dependent_damage_candidate"));
        assert!(signals.contains("health_dependent_healing_candidate"));
    }

    #[test]
    fn max_health_shield_and_stat_modifier_are_retained_separately() {
        let signals =
            detect_signals("Increases Max HP by 5% and grants a shield equal to 20% of Max HP.");
        assert!(signals.contains("health_stat_modifier"));
        assert!(signals.contains("health_dependent_shield_candidate"));
    }

    #[test]
    fn max_health_is_recognized_as_the_damage_formula_basis() {
        let signals = detect_signals("The burst deals DMG equal to 6% of Max HP.");
        assert!(signals.contains("health_dependent_damage_candidate"));
        assert!(signals.contains("percentage_health_damage_candidate"));
    }

    #[test]
    fn direct_source_ignores_generated_attribution_and_bridged_context() {
        let source = serde_json::json!({
            "sourceId": "talent:1336",
            "sourceEntityId": 1336,
            "sourceName": "Indomitable Chord",
            "description": "Each stack increases Max HP by 0.5% and DMG Reduction by 5%.",
            "buffIds": [2207370],
            "attributionModel": {
                "formulaAttributionText": "Deals damage equal to 6% of Max HP."
            },
            "bridgedPageContexts": [
                "Deals damage equal to 8% of Max HP."
            ]
        });
        let packet = PacketEvidence::default();
        let direct = build_direct_source(2207370, &source, &packet).expect("direct source");
        assert!(direct.signals.contains(&"health_state_general".to_owned()));
        assert!(
            !direct
                .signals
                .contains(&"health_dependent_damage_candidate".to_owned())
        );
        assert_eq!(direct.matching_description_text.len(), 1);
        assert!(
            direct.matching_description_text[0]
                .text
                .contains("DMG Reduction")
        );
    }
}
