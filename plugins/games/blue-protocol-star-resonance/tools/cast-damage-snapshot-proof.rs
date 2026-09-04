use std::{
    collections::{BTreeMap, BTreeSet, HashMap, VecDeque},
    env,
    ffi::OsString,
    fs::File,
    io::{BufReader, BufWriter, Write},
    path::{Path, PathBuf},
};

use rlogs_events::{
    CanonicalEvent, CastState, EntityAttributeValue, StatusEffectInstanceId, StatusState,
    TimelineEventKind,
};
use rlogs_game_bpsr::decode_known_entity_attribute_value;
use rlogs_log_format::{RlogLimits, RlogReader};
use serde::{Deserialize, Serialize};

const SCHEMA_VERSION: u16 = 1;
const DEFAULT_MAX_CAST_AGE_MILLIS: u64 = 15_000;
const INSPIRATION_EFFECT_ID: i64 = 2_202_041;
const RELEVANT_ATTRIBUTE_IDS: [i32; 8] = [
    11_330, // attack
    11_710, // critical chance
    11_780, // lucky chance
    11_840, // external damage increase
    11_940, // mastery
    11_950, // versatility
    12_510, // critical damage
    12_530, // lucky damage
];

#[derive(Debug)]
struct Arguments {
    identity_surface: PathBuf,
    rlogs: Vec<PathBuf>,
    output: PathBuf,
    max_cast_age_micros: u64,
}

#[derive(Debug, Deserialize)]
struct IdentitySurface {
    schema_version: u16,
    source: serde_json::Value,
    rows: IdentityRows,
}

#[derive(Debug, Deserialize)]
struct IdentityRows {
    skill_table: Vec<SkillRow>,
    skill_effect_table: Vec<EffectRow>,
    skill_fight_level_table: Vec<FightRow>,
    damage_attr_table: Vec<DamageRow>,
}

#[derive(Debug, Deserialize)]
struct SkillRow {
    skill_id: i64,
    parent_skill_id: Option<i64>,
}

#[derive(Debug, Deserialize)]
struct EffectRow {
    effect_id: i64,
    skill_id: Option<i64>,
}

#[derive(Debug, Deserialize)]
struct FightRow {
    fight_id: i64,
    skill_id: Option<i64>,
    effect_id: Option<i64>,
}

#[derive(Debug, Deserialize)]
struct DamageRow {
    damage_attr_id: serde_json::Value,
    linked_id: Option<i64>,
    hit_event_suffix: i32,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct DamageKey {
    ability_id: i64,
    hit_event_id: i32,
}

#[derive(Debug, Default)]
struct IdentityIndex {
    skills: HashMap<i64, SkillRow>,
    effects_by_id: HashMap<i64, Vec<EffectRow>>,
    effects_by_skill: HashMap<i64, Vec<i64>>,
    fights_by_id: HashMap<i64, Vec<FightRow>>,
    fights_by_skill: HashMap<i64, Vec<i64>>,
    damage_by_linked: HashMap<i64, Vec<DamageKey>>,
}

#[derive(Debug, Clone)]
struct InspirationWindow {
    provider_entity_uuid: Option<i64>,
    instance_id: Option<StatusEffectInstanceId>,
}

#[derive(Debug, Clone)]
struct CastSnapshot {
    sequence: u64,
    observed_micros: u64,
    ability_id: i64,
    source_entity_uuid: i64,
    attributes: BTreeMap<i32, i64>,
    inspiration_windows: Vec<InspirationWindow>,
}

#[derive(Debug, Default)]
struct CastAccumulator {
    cast_events: u64,
    sources: BTreeSet<i64>,
    identity_nodes: BTreeSet<i64>,
    table_damage_keys: BTreeSet<DamageKey>,
    matched_damage_events: u64,
    ambiguous_damage_events: u64,
    temporal_only_damage_events: u64,
    matched_damage_keys: BTreeMap<DamageKey, PairAccumulator>,
}

#[derive(Debug, Default)]
struct PairAccumulator {
    events: u64,
    minimum_delay_micros: Option<u64>,
    maximum_delay_micros: Option<u64>,
}

#[derive(Debug, Default, Serialize)]
struct Coverage {
    cast_events: u64,
    cast_abilities: usize,
    damage_events: u64,
    damage_events_with_ability_and_hit: u64,
    damage_events_with_same_source_recent_cast: u64,
    table_linked_unique_cast_snapshot: u64,
    table_linked_ambiguous_cast_snapshot: u64,
    no_table_linked_recent_cast: u64,
    no_recent_cast: u64,
    unique_snapshot_with_inspiration: u64,
    unique_snapshot_with_external_inspiration: u64,
    unique_snapshot_attributes_changed_before_damage: u64,
    unique_snapshot_attack_changed_before_damage: u64,
    unique_snapshot_mastery_changed_before_damage: u64,
    unique_snapshot_versatility_changed_before_damage: u64,
    unique_snapshot_external_damage_changed_before_damage: u64,
}

#[derive(Debug, Serialize)]
struct AuditBundle {
    schema_version: u16,
    generated_by: &'static str,
    policy: AuditPolicy,
    identity_surface_schema_version: u16,
    identity_surface_source: serde_json::Value,
    max_cast_age_micros: u64,
    sessions: Vec<SessionReport>,
    coverage: Coverage,
    cast_abilities: Vec<CastAbilityReport>,
}

#[derive(Debug, Serialize)]
struct AuditPolicy {
    runtime_formula_authority: bool,
    table_identity_authority: &'static str,
    temporal_proximity_authority: bool,
    accepted_snapshot_rule: &'static str,
    unresolved_evidence_is_hidden: bool,
    promotion_requirement: &'static str,
}

#[derive(Debug, Serialize)]
struct SessionReport {
    rlog: String,
    session_id: String,
    cast_events: u64,
    damage_events: u64,
    unique_snapshot_matches: u64,
    ambiguous_snapshot_matches: u64,
}

#[derive(Debug, Serialize)]
struct CastAbilityReport {
    cast_ability_id: i64,
    cast_events: u64,
    source_entities: Vec<i64>,
    identity_nodes: Vec<i64>,
    table_linked_damage_keys: Vec<DamageKeyReport>,
    matched_damage_events: u64,
    ambiguous_damage_events: u64,
    temporal_only_damage_events: u64,
    matched_damage_keys: Vec<MatchedDamageKeyReport>,
}

#[derive(Debug, Serialize)]
struct DamageKeyReport {
    ability_id: i64,
    hit_event_id: i32,
}

#[derive(Debug, Serialize)]
struct MatchedDamageKeyReport {
    ability_id: i64,
    hit_event_id: i32,
    events: u64,
    minimum_delay_micros: Option<u64>,
    maximum_delay_micros: Option<u64>,
}

fn main() {
    if let Err(error) = run() {
        eprintln!("cast/damage snapshot proof failed: {error}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), Box<dyn std::error::Error>> {
    let args = arguments()?;
    let surface: IdentitySurface =
        serde_json::from_reader(BufReader::new(File::open(&args.identity_surface)?))?;
    let index = IdentityIndex::new(surface.rows);
    let mut coverage = Coverage::default();
    let mut cast_accumulators = BTreeMap::<i64, CastAccumulator>::new();
    let mut sessions = Vec::new();

    for rlog in &args.rlogs {
        sessions.push(read_session(
            rlog,
            &index,
            args.max_cast_age_micros,
            &mut coverage,
            &mut cast_accumulators,
        )?);
    }
    coverage.cast_abilities = cast_accumulators.len();

    let cast_abilities = cast_accumulators
        .into_iter()
        .map(|(cast_ability_id, value)| CastAbilityReport {
            cast_ability_id,
            cast_events: value.cast_events,
            source_entities: value.sources.into_iter().collect(),
            identity_nodes: value.identity_nodes.into_iter().collect(),
            table_linked_damage_keys: value
                .table_damage_keys
                .into_iter()
                .map(DamageKeyReport::from)
                .collect(),
            matched_damage_events: value.matched_damage_events,
            ambiguous_damage_events: value.ambiguous_damage_events,
            temporal_only_damage_events: value.temporal_only_damage_events,
            matched_damage_keys: value
                .matched_damage_keys
                .into_iter()
                .map(|(key, pair)| MatchedDamageKeyReport {
                    ability_id: key.ability_id,
                    hit_event_id: key.hit_event_id,
                    events: pair.events,
                    minimum_delay_micros: pair.minimum_delay_micros,
                    maximum_delay_micros: pair.maximum_delay_micros,
                })
                .collect(),
        })
        .collect();

    let bundle = AuditBundle {
        schema_version: SCHEMA_VERSION,
        generated_by: "rlogs-bpsr-cast-damage-snapshot-proof",
        policy: AuditPolicy {
            runtime_formula_authority: false,
            table_identity_authority: "direct current-build CTB identifier fields only",
            temporal_proximity_authority: false,
            accepted_snapshot_rule: "same attributed source, damage key reachable through current-build identifier edges, and exactly one compatible recent cast instance",
            unresolved_evidence_is_hidden: false,
            promotion_requirement: "prove formula-stage arithmetic and rounding against accepted snapshots before enabling rDPS transfer",
        },
        identity_surface_schema_version: surface.schema_version,
        identity_surface_source: surface.source,
        max_cast_age_micros: args.max_cast_age_micros,
        sessions,
        coverage,
        cast_abilities,
    };

    let mut writer = BufWriter::new(File::create(&args.output)?);
    serde_json::to_writer_pretty(&mut writer, &bundle)?;
    writer.write_all(b"\n")?;
    writer.flush()?;
    println!("Wrote {}", args.output.display());
    Ok(())
}

fn read_session(
    rlog: &Path,
    index: &IdentityIndex,
    max_cast_age_micros: u64,
    coverage: &mut Coverage,
    cast_accumulators: &mut BTreeMap<i64, CastAccumulator>,
) -> Result<SessionReport, Box<dyn std::error::Error>> {
    let mut reader = RlogReader::new(BufReader::new(File::open(rlog)?), RlogLimits::default())?;
    let mut recent_casts = HashMap::<i64, VecDeque<CastSnapshot>>::new();
    let mut attributes = HashMap::<i64, BTreeMap<i32, i64>>::new();
    let mut inspiration = HashMap::<i64, Vec<InspirationWindow>>::new();
    let mut session_id = String::new();
    let mut session_casts = 0_u64;
    let mut session_damage = 0_u64;
    let mut session_unique = 0_u64;
    let mut session_ambiguous = 0_u64;

    while let Some(envelope) = reader.next_event()? {
        session_id = envelope.session_id.clone();
        expire_casts(
            &mut recent_casts,
            envelope.time.observed_micros,
            max_cast_age_micros,
        );
        let CanonicalEvent::Timeline(timeline) = &envelope.event else {
            continue;
        };
        match &timeline.kind {
            TimelineEventKind::EntityAttributes(update) => {
                let values = attributes.entry(update.actor.entity_uuid.0).or_default();
                for attribute in &update.attributes {
                    if !RELEVANT_ATTRIBUTE_IDS.contains(&attribute.attribute_id) {
                        continue;
                    }
                    let decoded = attribute.decoded.clone().or_else(|| {
                        decode_known_entity_attribute_value(
                            attribute.attribute_id,
                            &attribute.raw_value,
                        )
                    });
                    if let Some(EntityAttributeValue::Integer(value)) = decoded {
                        values.insert(attribute.attribute_id, value);
                    }
                }
            }
            TimelineEventKind::Status(status) if status.effect.0 == INSPIRATION_EFFECT_ID => {
                let target = status.target.entity_uuid.0;
                let windows = inspiration.entry(target).or_default();
                match status.state {
                    StatusState::Applied | StatusState::Refreshed | StatusState::Stacked => {
                        if let Some(instance_id) = status.instance_id {
                            windows.retain(|window| window.instance_id != Some(instance_id));
                        }
                        windows.push(InspirationWindow {
                            provider_entity_uuid: status.source.map(|source| source.entity_uuid.0),
                            instance_id: status.instance_id,
                        });
                    }
                    StatusState::Consumed | StatusState::Removed => {
                        if let Some(instance_id) = status.instance_id {
                            windows.retain(|window| window.instance_id != Some(instance_id));
                        } else if let Some(provider) =
                            status.source.map(|source| source.entity_uuid.0)
                        {
                            windows.retain(|window| window.provider_entity_uuid != Some(provider));
                        } else {
                            windows.clear();
                        }
                    }
                }
            }
            TimelineEventKind::Cast(cast) if cast.state == CastState::Started => {
                coverage.cast_events += 1;
                session_casts += 1;
                let source = cast.source.entity_uuid.0;
                let ability = cast.ability.0;
                let (identity_nodes, table_damage_keys) = index.cast_damage_candidates(ability);
                let accumulator = cast_accumulators.entry(ability).or_default();
                accumulator.cast_events += 1;
                accumulator.sources.insert(source);
                accumulator.identity_nodes.extend(identity_nodes);
                accumulator.table_damage_keys.extend(table_damage_keys);
                recent_casts
                    .entry(source)
                    .or_default()
                    .push_back(CastSnapshot {
                        sequence: envelope.sequence,
                        observed_micros: envelope.time.observed_micros,
                        ability_id: ability,
                        source_entity_uuid: source,
                        attributes: attributes.get(&source).cloned().unwrap_or_default(),
                        inspiration_windows: inspiration.get(&source).cloned().unwrap_or_default(),
                    });
            }
            TimelineEventKind::Damage(damage) => {
                coverage.damage_events += 1;
                session_damage += 1;
                let (Some(ability), Some(hit_event_id)) =
                    (damage.ability.map(|value| value.0), damage.hit_event_id)
                else {
                    continue;
                };
                coverage.damage_events_with_ability_and_hit += 1;
                let key = DamageKey {
                    ability_id: ability,
                    hit_event_id,
                };
                let source = damage.source.entity_uuid.0;
                let Some(casts) = recent_casts.get(&source) else {
                    coverage.no_recent_cast += 1;
                    continue;
                };
                if casts.is_empty() {
                    coverage.no_recent_cast += 1;
                    continue;
                }
                coverage.damage_events_with_same_source_recent_cast += 1;
                let matches = casts
                    .iter()
                    .filter(|cast| index.cast_matches_damage(cast.ability_id, &key))
                    .collect::<Vec<_>>();
                if matches.is_empty() {
                    coverage.no_table_linked_recent_cast += 1;
                    if let Some(latest) = casts.back() {
                        cast_accumulators
                            .entry(latest.ability_id)
                            .or_default()
                            .temporal_only_damage_events += 1;
                    }
                    continue;
                }
                if matches.len() > 1 {
                    coverage.table_linked_ambiguous_cast_snapshot += 1;
                    session_ambiguous += 1;
                    for cast in matches {
                        cast_accumulators
                            .entry(cast.ability_id)
                            .or_default()
                            .ambiguous_damage_events += 1;
                    }
                    continue;
                }

                coverage.table_linked_unique_cast_snapshot += 1;
                session_unique += 1;
                let cast = matches[0];
                debug_assert_eq!(cast.source_entity_uuid, source);
                debug_assert!(cast.sequence < envelope.sequence);
                let delay = envelope
                    .time
                    .observed_micros
                    .checked_sub(cast.observed_micros)
                    .ok_or("damage time preceded accepted cast snapshot")?;
                let accumulator = cast_accumulators.entry(cast.ability_id).or_default();
                accumulator.matched_damage_events += 1;
                let pair = accumulator.matched_damage_keys.entry(key).or_default();
                pair.events += 1;
                pair.minimum_delay_micros = Some(
                    pair.minimum_delay_micros
                        .map_or(delay, |current| current.min(delay)),
                );
                pair.maximum_delay_micros = Some(
                    pair.maximum_delay_micros
                        .map_or(delay, |current| current.max(delay)),
                );

                if !cast.inspiration_windows.is_empty() {
                    coverage.unique_snapshot_with_inspiration += 1;
                }
                if cast.inspiration_windows.iter().any(|window| {
                    window
                        .provider_entity_uuid
                        .is_some_and(|provider| provider != source)
                }) {
                    coverage.unique_snapshot_with_external_inspiration += 1;
                }
                let current = attributes.get(&source);
                let changed = |attribute_id: i32| {
                    cast.attributes.get(&attribute_id)
                        != current.and_then(|values| values.get(&attribute_id))
                };
                if RELEVANT_ATTRIBUTE_IDS.into_iter().any(changed) {
                    coverage.unique_snapshot_attributes_changed_before_damage += 1;
                }
                if changed(11_330) {
                    coverage.unique_snapshot_attack_changed_before_damage += 1;
                }
                if changed(11_940) {
                    coverage.unique_snapshot_mastery_changed_before_damage += 1;
                }
                if changed(11_950) {
                    coverage.unique_snapshot_versatility_changed_before_damage += 1;
                }
                if changed(11_840) {
                    coverage.unique_snapshot_external_damage_changed_before_damage += 1;
                }
            }
            _ => {}
        }
    }

    Ok(SessionReport {
        rlog: rlog.display().to_string(),
        session_id,
        cast_events: session_casts,
        damage_events: session_damage,
        unique_snapshot_matches: session_unique,
        ambiguous_snapshot_matches: session_ambiguous,
    })
}

fn expire_casts(
    recent_casts: &mut HashMap<i64, VecDeque<CastSnapshot>>,
    now_micros: u64,
    max_cast_age_micros: u64,
) {
    for casts in recent_casts.values_mut() {
        while casts.front().is_some_and(|cast| {
            now_micros
                .checked_sub(cast.observed_micros)
                .is_some_and(|age| age > max_cast_age_micros)
        }) {
            casts.pop_front();
        }
    }
}

impl IdentityIndex {
    fn new(rows: IdentityRows) -> Self {
        let mut index = Self::default();
        for skill in rows.skill_table {
            index.skills.insert(skill.skill_id, skill);
        }
        for effect in rows.skill_effect_table {
            if let Some(skill_id) = effect.skill_id {
                index
                    .effects_by_skill
                    .entry(skill_id)
                    .or_default()
                    .push(effect.effect_id);
            }
            index
                .effects_by_id
                .entry(effect.effect_id)
                .or_default()
                .push(effect);
        }
        for fight in rows.skill_fight_level_table {
            if let Some(skill_id) = fight.skill_id {
                index
                    .fights_by_skill
                    .entry(skill_id)
                    .or_default()
                    .push(fight.fight_id);
            }
            index
                .fights_by_id
                .entry(fight.fight_id)
                .or_default()
                .push(fight);
        }
        for damage in rows.damage_attr_table {
            let _ = &damage.damage_attr_id;
            if let Some(linked_id) = damage.linked_id {
                index
                    .damage_by_linked
                    .entry(linked_id)
                    .or_default()
                    .push(DamageKey {
                        ability_id: linked_id,
                        hit_event_id: damage.hit_event_suffix,
                    });
            }
        }
        index
    }

    fn cast_damage_candidates(&self, cast_id: i64) -> (BTreeSet<i64>, BTreeSet<DamageKey>) {
        let mut nodes = BTreeSet::from([cast_id]);
        let mut skills = BTreeSet::new();

        if self.skills.contains_key(&cast_id) {
            skills.insert(cast_id);
        }
        if let Some(fights) = self.fights_by_id.get(&cast_id) {
            for fight in fights {
                if let Some(skill_id) = fight.skill_id {
                    skills.insert(skill_id);
                    nodes.insert(skill_id);
                }
                if let Some(effect_id) = fight.effect_id {
                    nodes.insert(effect_id);
                }
            }
        }
        if let Some(effects) = self.effects_by_id.get(&cast_id) {
            for effect in effects {
                if let Some(skill_id) = effect.skill_id {
                    skills.insert(skill_id);
                    nodes.insert(skill_id);
                }
            }
        }

        let initial_skills = skills.iter().copied().collect::<Vec<_>>();
        for skill_id in initial_skills {
            if let Some(skill) = self.skills.get(&skill_id) {
                if let Some(parent_id) = skill.parent_skill_id {
                    skills.insert(parent_id);
                    nodes.insert(parent_id);
                }
            }
        }
        for skill_id in skills {
            nodes.insert(skill_id);
            if let Some(effects) = self.effects_by_skill.get(&skill_id) {
                nodes.extend(effects.iter().copied());
            }
            if let Some(fights) = self.fights_by_skill.get(&skill_id) {
                nodes.extend(fights.iter().copied());
            }
        }

        let mut damage_keys = BTreeSet::new();
        for node in &nodes {
            if let Some(keys) = self.damage_by_linked.get(node) {
                damage_keys.extend(keys.iter().cloned());
            }
        }
        (nodes, damage_keys)
    }

    fn cast_matches_damage(&self, cast_id: i64, damage: &DamageKey) -> bool {
        self.cast_damage_candidates(cast_id).1.contains(damage)
    }
}

impl From<DamageKey> for DamageKeyReport {
    fn from(value: DamageKey) -> Self {
        Self {
            ability_id: value.ability_id,
            hit_event_id: value.hit_event_id,
        }
    }
}

fn arguments() -> Result<Arguments, String> {
    let mut values = env::args_os().skip(1).collect::<Vec<_>>();
    let identity_surface = PathBuf::from(take_value(&mut values, "--identity-surface")?);
    let output = PathBuf::from(take_value(&mut values, "--output")?);
    let max_cast_age_millis = take_optional_value(&mut values, "--max-cast-age-millis")
        .map(|value| parse_u64(value, "--max-cast-age-millis"))
        .transpose()?
        .unwrap_or(DEFAULT_MAX_CAST_AGE_MILLIS);
    let mut rlogs = Vec::new();
    while let Some(value) = take_optional_value(&mut values, "--rlog") {
        rlogs.push(PathBuf::from(value));
    }
    if rlogs.is_empty() {
        return Err(format!("at least one --rlog is required\n{}", usage()));
    }
    if !values.is_empty() {
        return Err(usage());
    }
    Ok(Arguments {
        identity_surface,
        rlogs,
        output,
        max_cast_age_micros: max_cast_age_millis
            .checked_mul(1_000)
            .ok_or("--max-cast-age-millis is too large")?,
    })
}

fn take_value(values: &mut Vec<OsString>, flag: &str) -> Result<OsString, String> {
    take_optional_value(values, flag).ok_or_else(usage)
}

fn take_optional_value(values: &mut Vec<OsString>, flag: &str) -> Option<OsString> {
    let position = values.iter().position(|value| value == flag)?;
    if position + 1 >= values.len() {
        return None;
    }
    let value = values.remove(position + 1);
    values.remove(position);
    Some(value)
}

fn parse_u64(value: OsString, flag: &str) -> Result<u64, String> {
    value
        .to_string_lossy()
        .parse::<u64>()
        .map_err(|_| format!("{flag} requires a non-negative integer"))
}

fn usage() -> String {
    "usage: rlogs-bpsr-cast-damage-snapshot-proof --identity-surface <CastDamageIdentitySurface.json> --rlog <sealed.rlog> [--rlog <sealed.rlog> ...] --output <proof.json> [--max-cast-age-millis <n>]".to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exact_fight_to_skill_to_effect_to_damage_edges_are_retained() {
        let index = IdentityIndex::new(IdentityRows {
            skill_table: vec![SkillRow {
                skill_id: 10,
                parent_skill_id: None,
            }],
            skill_effect_table: vec![EffectRow {
                effect_id: 20,
                skill_id: Some(10),
            }],
            skill_fight_level_table: vec![FightRow {
                fight_id: 30,
                skill_id: Some(10),
                effect_id: Some(20),
            }],
            damage_attr_table: vec![DamageRow {
                damage_attr_id: serde_json::json!(2007),
                linked_id: Some(20),
                hit_event_suffix: 7,
            }],
        });

        let (nodes, damage) = index.cast_damage_candidates(30);
        assert_eq!(nodes, BTreeSet::from([10, 20, 30]));
        assert!(damage.contains(&DamageKey {
            ability_id: 20,
            hit_event_id: 7,
        }));
    }

    #[test]
    fn display_names_and_numeric_proximity_never_create_edges() {
        let index = IdentityIndex::new(IdentityRows {
            skill_table: vec![SkillRow {
                skill_id: 100,
                parent_skill_id: None,
            }],
            skill_effect_table: vec![],
            skill_fight_level_table: vec![],
            damage_attr_table: vec![DamageRow {
                damage_attr_id: serde_json::json!(10101),
                linked_id: Some(101),
                hit_event_suffix: 1,
            }],
        });
        assert!(index.cast_damage_candidates(100).1.is_empty());
    }
}
