use std::{
    collections::{BTreeMap, BTreeSet},
    env,
    fs::File,
    io::{BufReader, BufWriter, Write},
    path::PathBuf,
};

use rlogs_game_bpsr::{
    BpsrFightSourceKind, RDPS_AUDIT_SCHEMA_VERSION, RdpsAuditProviderRecipientExample,
    RdpsAuditProviderRecipientExampleClass, RdpsAuditProviderRecipientMatrix, RdpsAuditReport,
};
use serde::{Deserialize, Serialize};

const ORIGIN_CATALOG_SCHEMA_VERSION: u16 = 3;
const MAXIMUM_PROVIDER_RECIPIENT_EXAMPLES_PER_CLASS: usize = 4;

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct AuditBundle {
    schema_version: u16,
    client_build: String,
    deployment_id: String,
    sources: Vec<AuditSource>,
    reports: Vec<RdpsAuditReport>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct AuditSource {
    file_name: String,
    session_id: String,
    client_build: String,
    deployment_id: String,
    producer: String,
}

#[derive(Debug, Serialize)]
struct OriginCatalog {
    schema_version: u16,
    game_build: String,
    policy: &'static str,
    summary: CatalogSummary,
    effects: Vec<ObservedEffect>,
    relations: Vec<ObservedOriginRelation>,
}

#[derive(Debug, Serialize)]
struct CatalogSummary {
    source_sessions: usize,
    observed_effects: usize,
    effects_with_packet_origin: usize,
    status_events: u64,
    packet_origin_observations: u64,
    distinct_effect_source_relations: usize,
    source_type_counts: Vec<SourceTypeCount>,
}

#[derive(Debug, Serialize)]
struct SourceTypeCount {
    source_type_id: i32,
    source_kind: Option<&'static str>,
    configured_source_table: Option<&'static str>,
    distinct_config_ids: usize,
    distinct_effect_source_relations: usize,
    observations: u64,
}

#[derive(Debug, Serialize)]
struct ObservedEffect {
    effect_id: i64,
    status_events: u64,
    window_count: u64,
    cross_actor_window_count: u64,
    source_missing_window_count: u64,
    source_player_window_count: u64,
    target_player_window_count: u64,
    target_monster_window_count: u64,
    cross_actor_provider_recipient_windows: RdpsAuditProviderRecipientMatrix,
    provider_recipient_examples: Vec<ObservedProviderRecipientExample>,
    applied: u64,
    refreshed: u64,
    stacked: u64,
    consumed: u64,
    removed: u64,
    minimum_stacks: Option<u32>,
    maximum_stacks: Option<u32>,
    packet_origin_observations: u64,
    source_relation_count: usize,
    observed_sessions: Vec<String>,
}

#[derive(Debug, Serialize)]
struct ObservedProviderRecipientExample {
    #[serde(flatten)]
    example: RdpsAuditProviderRecipientExample,
    observed_sessions: Vec<String>,
}

#[derive(Debug, Serialize)]
struct ObservedOriginRelation {
    effect_id: i64,
    source_type_id: i32,
    source_kind: Option<&'static str>,
    configured_source_table: Option<&'static str>,
    source_config_id: i64,
    observation_count: u64,
    observed_sessions: Vec<String>,
}

#[derive(Debug, Default)]
struct EffectAccumulator {
    status_events: u64,
    window_count: u64,
    cross_actor_window_count: u64,
    source_missing_window_count: u64,
    source_player_window_count: u64,
    target_player_window_count: u64,
    target_monster_window_count: u64,
    cross_actor_provider_recipient_windows: RdpsAuditProviderRecipientMatrix,
    provider_recipient_examples: BTreeMap<RdpsAuditProviderRecipientExample, BTreeSet<String>>,
    applied: u64,
    refreshed: u64,
    stacked: u64,
    consumed: u64,
    removed: u64,
    minimum_stacks: Option<u32>,
    maximum_stacks: Option<u32>,
    packet_origin_observations: u64,
    source_relations: BTreeSet<(i32, i64)>,
    sessions: BTreeSet<String>,
}

#[derive(Debug, Default)]
struct RelationAccumulator {
    observation_count: u64,
    sessions: BTreeSet<String>,
}

#[derive(Debug, Default)]
struct SourceTypeAccumulator {
    config_ids: BTreeSet<i64>,
    relations: u64,
    observations: u64,
}

fn main() {
    if let Err(error) = run() {
        eprintln!("BPSR origin catalog failed: {error}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), Box<dyn std::error::Error>> {
    let mut arguments = env::args_os().skip(1);
    let input = arguments.next().map(PathBuf::from).ok_or_else(usage)?;
    let output = arguments.next().map(PathBuf::from).ok_or_else(usage)?;
    if arguments.next().is_some() {
        return Err(usage().into());
    }

    let bundle: AuditBundle = serde_json::from_reader(BufReader::new(File::open(input)?))?;
    if bundle.schema_version != RDPS_AUDIT_SCHEMA_VERSION
        || bundle.reports.is_empty()
        || bundle
            .reports
            .iter()
            .any(|report| report.schema_version != RDPS_AUDIT_SCHEMA_VERSION)
    {
        return Err("input is not a non-empty current rDPS audit bundle".into());
    }

    if bundle.client_build.trim().is_empty()
        || bundle.deployment_id.trim().is_empty()
        || bundle.sources.len() != bundle.reports.len()
        || bundle.sources.iter().any(|source| {
            source.client_build != bundle.client_build
                || source.deployment_id != bundle.deployment_id
                || source.file_name.trim().is_empty()
                || source.session_id.trim().is_empty()
                || source.producer.trim().is_empty()
        })
    {
        return Err("input audit bundle has invalid or mixed-build provenance".into());
    }

    let catalog = build_catalog(bundle.client_build, bundle.reports);
    let mut writer = BufWriter::new(File::create(output)?);
    serde_json::to_writer_pretty(&mut writer, &catalog)?;
    writer.write_all(b"\n")?;
    writer.flush()?;
    Ok(())
}

fn build_catalog(game_build: String, reports: Vec<RdpsAuditReport>) -> OriginCatalog {
    let source_sessions = reports.len();
    let mut effects = BTreeMap::<i64, EffectAccumulator>::new();
    let mut relations = BTreeMap::<(i64, i32, i64), RelationAccumulator>::new();

    for report in reports {
        let session_id = report.session_id;
        for effect in report.effects {
            let accumulator = effects.entry(effect.effect_id).or_default();
            accumulator.status_events = accumulator
                .status_events
                .saturating_add(effect.status_events);
            accumulator.window_count = accumulator.window_count.saturating_add(effect.window_count);
            accumulator.cross_actor_window_count = accumulator
                .cross_actor_window_count
                .saturating_add(effect.cross_actor_window_count);
            accumulator.source_missing_window_count = accumulator
                .source_missing_window_count
                .saturating_add(effect.source_missing_window_count);
            accumulator.source_player_window_count = accumulator
                .source_player_window_count
                .saturating_add(effect.source_player_window_count);
            accumulator.target_player_window_count = accumulator
                .target_player_window_count
                .saturating_add(effect.target_player_window_count);
            accumulator.target_monster_window_count = accumulator
                .target_monster_window_count
                .saturating_add(effect.target_monster_window_count);
            merge_provider_recipient_windows(
                &mut accumulator.cross_actor_provider_recipient_windows,
                &effect.cross_actor_provider_recipient_windows,
            );
            for example in effect.provider_recipient_examples {
                accumulator
                    .provider_recipient_examples
                    .entry(example)
                    .or_default()
                    .insert(session_id.clone());
            }
            accumulator.applied = accumulator.applied.saturating_add(effect.applied);
            accumulator.refreshed = accumulator.refreshed.saturating_add(effect.refreshed);
            accumulator.stacked = accumulator.stacked.saturating_add(effect.stacked);
            accumulator.consumed = accumulator.consumed.saturating_add(effect.consumed);
            accumulator.removed = accumulator.removed.saturating_add(effect.removed);
            accumulator.minimum_stacks =
                minimum_option(accumulator.minimum_stacks, effect.minimum_stacks);
            accumulator.maximum_stacks =
                maximum_option(accumulator.maximum_stacks, effect.maximum_stacks);
            accumulator.packet_origin_observations = accumulator
                .packet_origin_observations
                .saturating_add(effect.packet_origin_observation_count);
            accumulator.sessions.insert(session_id.clone());

            for origin in effect.packet_origins {
                accumulator
                    .source_relations
                    .insert((origin.source_type_id, origin.source_config_id));
                let relation = relations
                    .entry((
                        effect.effect_id,
                        origin.source_type_id,
                        origin.source_config_id,
                    ))
                    .or_default();
                relation.observation_count = relation
                    .observation_count
                    .saturating_add(origin.observation_count);
                relation.sessions.insert(session_id.clone());
            }
        }
    }

    let status_events = effects.values().map(|effect| effect.status_events).sum();
    let packet_origin_observations = relations
        .values()
        .map(|relation| relation.observation_count)
        .sum();
    let effects_with_packet_origin = effects
        .values()
        .filter(|effect| !effect.source_relations.is_empty())
        .count();

    let effect_rows = effects
        .into_iter()
        .map(|(effect_id, effect)| {
            let mut examples_by_class = BTreeMap::<
                RdpsAuditProviderRecipientExampleClass,
                Vec<ObservedProviderRecipientExample>,
            >::new();
            for (example, sessions) in effect.provider_recipient_examples {
                examples_by_class.entry(example.class).or_default().push(
                    ObservedProviderRecipientExample {
                        example,
                        observed_sessions: sessions.into_iter().collect(),
                    },
                );
            }
            let provider_recipient_examples = examples_by_class
                .into_values()
                .flat_map(|examples| {
                    examples
                        .into_iter()
                        .take(MAXIMUM_PROVIDER_RECIPIENT_EXAMPLES_PER_CLASS)
                })
                .collect();
            ObservedEffect {
                effect_id,
                status_events: effect.status_events,
                window_count: effect.window_count,
                cross_actor_window_count: effect.cross_actor_window_count,
                source_missing_window_count: effect.source_missing_window_count,
                source_player_window_count: effect.source_player_window_count,
                target_player_window_count: effect.target_player_window_count,
                target_monster_window_count: effect.target_monster_window_count,
                cross_actor_provider_recipient_windows: effect
                    .cross_actor_provider_recipient_windows,
                provider_recipient_examples,
                applied: effect.applied,
                refreshed: effect.refreshed,
                stacked: effect.stacked,
                consumed: effect.consumed,
                removed: effect.removed,
                minimum_stacks: effect.minimum_stacks,
                maximum_stacks: effect.maximum_stacks,
                packet_origin_observations: effect.packet_origin_observations,
                source_relation_count: effect.source_relations.len(),
                observed_sessions: effect.sessions.into_iter().collect(),
            }
        })
        .collect::<Vec<_>>();

    let relation_rows = relations
        .into_iter()
        .map(
            |((effect_id, source_type_id, source_config_id), relation)| {
                let kind = BpsrFightSourceKind::from_protocol_id(source_type_id);
                ObservedOriginRelation {
                    effect_id,
                    source_type_id,
                    source_kind: kind.map(BpsrFightSourceKind::as_str),
                    configured_source_table: configured_source_table(kind),
                    source_config_id,
                    observation_count: relation.observation_count,
                    observed_sessions: relation.sessions.into_iter().collect(),
                }
            },
        )
        .collect::<Vec<_>>();

    let mut source_types = BTreeMap::<i32, SourceTypeAccumulator>::new();
    for relation in &relation_rows {
        let source_type = source_types.entry(relation.source_type_id).or_default();
        source_type.config_ids.insert(relation.source_config_id);
        source_type.relations = source_type.relations.saturating_add(1);
        source_type.observations = source_type
            .observations
            .saturating_add(relation.observation_count);
    }
    let source_type_counts = source_types
        .into_iter()
        .map(|(source_type_id, source_type)| {
            let kind = BpsrFightSourceKind::from_protocol_id(source_type_id);
            SourceTypeCount {
                source_type_id,
                source_kind: kind.map(BpsrFightSourceKind::as_str),
                configured_source_table: configured_source_table(kind),
                distinct_config_ids: source_type.config_ids.len(),
                distinct_effect_source_relations: usize::try_from(source_type.relations)
                    .unwrap_or(usize::MAX),
                observations: source_type.observations,
            }
        })
        .collect();

    OriginCatalog {
        schema_version: ORIGIN_CATALOG_SCHEMA_VERSION,
        game_build,
        policy: "packet_observed_relationships_only_no_inferred_origins",
        summary: CatalogSummary {
            source_sessions,
            observed_effects: effect_rows.len(),
            effects_with_packet_origin,
            status_events,
            packet_origin_observations,
            distinct_effect_source_relations: relation_rows.len(),
            source_type_counts,
        },
        effects: effect_rows,
        relations: relation_rows,
    }
}

fn configured_source_table(kind: Option<BpsrFightSourceKind>) -> Option<&'static str> {
    match kind {
        Some(BpsrFightSourceKind::Buff) => Some("BuffTable.ctb"),
        Some(BpsrFightSourceKind::Bullet) => Some("BulletTable.ctb"),
        Some(BpsrFightSourceKind::Affix) => Some("AffixTable.ctb"),
        _ => None,
    }
}

fn minimum_option(left: Option<u32>, right: Option<u32>) -> Option<u32> {
    match (left, right) {
        (Some(left), Some(right)) => Some(left.min(right)),
        (left, right) => left.or(right),
    }
}

fn maximum_option(left: Option<u32>, right: Option<u32>) -> Option<u32> {
    match (left, right) {
        (Some(left), Some(right)) => Some(left.max(right)),
        (left, right) => left.or(right),
    }
}

fn usage() -> &'static str {
    "usage: rlogs-bpsr-origin-catalog <rdps-audit.json> <output.json>"
}

fn merge_provider_recipient_windows(
    target: &mut RdpsAuditProviderRecipientMatrix,
    source: &RdpsAuditProviderRecipientMatrix,
) {
    target.resolved_player_to_player = target
        .resolved_player_to_player
        .saturating_add(source.resolved_player_to_player);
    target.resolved_same_owner_player_to_player = target
        .resolved_same_owner_player_to_player
        .saturating_add(source.resolved_same_owner_player_to_player);
    target.resolved_external_player_to_player = target
        .resolved_external_player_to_player
        .saturating_add(source.resolved_external_player_to_player);
    target.resolved_player_to_monster = target
        .resolved_player_to_monster
        .saturating_add(source.resolved_player_to_monster);
    target.resolved_player_to_other = target
        .resolved_player_to_other
        .saturating_add(source.resolved_player_to_other);
    target.non_player_to_player = target
        .non_player_to_player
        .saturating_add(source.non_player_to_player);
    target.non_player_to_monster = target
        .non_player_to_monster
        .saturating_add(source.non_player_to_monster);
    target.non_player_to_other = target
        .non_player_to_other
        .saturating_add(source.non_player_to_other);
    target.unresolved_to_player = target
        .unresolved_to_player
        .saturating_add(source.unresolved_to_player);
    target.unresolved_to_monster = target
        .unresolved_to_monster
        .saturating_add(source.unresolved_to_monster);
    target.unresolved_to_other = target
        .unresolved_to_other
        .saturating_add(source.unresolved_to_other);
}

#[cfg(test)]
mod tests {
    use super::*;
    use rlogs_game_bpsr::{
        RdpsAuditPacketOrigin, RdpsAuditProviderClass, RdpsAuditRecipientClass, RdpsEffectAudit,
    };

    fn provider_recipient_example() -> RdpsAuditProviderRecipientExample {
        RdpsAuditProviderRecipientExample {
            class: RdpsAuditProviderRecipientExampleClass::ExternalPlayerToPlayer,
            raw_source_actor_id: Some(1),
            raw_target_actor_id: 2,
            raw_source_entity_uuid: Some(100),
            resolved_source_entity_uuid: Some(100),
            raw_target_entity_uuid: 200,
            resolved_target_entity_uuid: 200,
            provider_class: RdpsAuditProviderClass::ResolvedPlayer,
            recipient_class: RdpsAuditRecipientClass::Player,
            cross_actor: true,
            same_resolved_owner: false,
        }
    }

    fn effect(effect_id: i64, origin: (i32, i64, u64)) -> RdpsEffectAudit {
        RdpsEffectAudit {
            effect_id,
            localized_name: None,
            technical_name: None,
            presentation_resolution: None,
            status_events: 2,
            window_count: 1,
            cross_actor_window_count: 0,
            source_missing_window_count: 0,
            source_player_window_count: 1,
            source_resolved_player_window_count: 1,
            source_owner_resolved_window_count: 0,
            target_player_window_count: 1,
            target_monster_window_count: 0,
            cross_actor_provider_recipient_windows: Default::default(),
            provider_recipient_examples: vec![provider_recipient_example()],
            applied: 1,
            refreshed: 0,
            stacked: 0,
            consumed: 0,
            removed: 1,
            minimum_stacks: Some(1),
            maximum_stacks: Some(1),
            distinct_provider_entities: 1,
            distinct_resolved_provider_entities: 1,
            distinct_recipient_entities: 1,
            cross_actor_recipient_outgoing: Default::default(),
            cross_actor_recipient_incoming: Default::default(),
            packet_origin_observation_count: origin.2,
            packet_origins: vec![RdpsAuditPacketOrigin {
                source_type_id: origin.0,
                source_config_id: origin.1,
                observation_count: origin.2,
                fingerprint_match_kind: Default::default(),
                endpoint_resolution: Default::default(),
                owner_resolution: Default::default(),
                candidate_sources: Vec::new(),
                unresolved_terminal_ids: Vec::new(),
            }],
            origin_observation_count: 0,
            uncorrelated_origin_observation_count: 0,
            ambiguous_origin_observation_count: 0,
            originating_abilities: Vec::new(),
        }
    }

    #[test]
    fn packet_relations_are_merged_without_losing_competing_sources() {
        let reports = vec![
            RdpsAuditReport {
                schema_version: RDPS_AUDIT_SCHEMA_VERSION,
                session_id: "a".into(),
                first_observed_micros: None,
                last_observed_micros: None,
                damage_events: 0,
                effects: vec![effect(55, (1, 100, 2)), effect(55, (1, 101, 3))],
            },
            RdpsAuditReport {
                schema_version: RDPS_AUDIT_SCHEMA_VERSION,
                session_id: "b".into(),
                first_observed_micros: None,
                last_observed_micros: None,
                damage_events: 0,
                effects: vec![effect(55, (1, 100, 5))],
            },
        ];

        let catalog = build_catalog("build".into(), reports);
        assert_eq!(catalog.summary.observed_effects, 1);
        assert_eq!(catalog.summary.distinct_effect_source_relations, 2);
        assert_eq!(catalog.summary.packet_origin_observations, 10);
        assert_eq!(catalog.effects[0].source_relation_count, 2);
        assert_eq!(catalog.relations[0].source_config_id, 100);
        assert_eq!(catalog.relations[0].observation_count, 7);
        assert_eq!(catalog.relations[0].observed_sessions, ["a", "b"]);
        assert_eq!(catalog.relations[1].source_config_id, 101);
        assert_eq!(catalog.relations[1].observation_count, 3);
        assert_eq!(catalog.effects[0].provider_recipient_examples.len(), 1);
        let example = &catalog.effects[0].provider_recipient_examples[0];
        assert_eq!(
            example.example.class,
            RdpsAuditProviderRecipientExampleClass::ExternalPlayerToPlayer
        );
        assert_eq!(example.observed_sessions, ["a", "b"]);
    }
}
