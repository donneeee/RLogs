use std::{
    collections::{BTreeMap, BTreeSet},
    env,
    ffi::OsString,
    fs::{self, File},
    io::{BufReader, BufWriter, Read, Write},
    path::{Path, PathBuf},
};

use rlogs_events::{
    ActorState, CanonicalEvent, EntityAttributeUpdateKind, EntityAttributeValue, EntityRef,
    TimelineEventKind,
};
use rlogs_game_bpsr::decode_known_entity_attribute_value;
use rlogs_log_format::{RlogLimits, RlogReader};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

const SCHEMA_VERSION: u16 = 4;
const EXPECTED_BUILD: &str = "24687926";
const EXPECTED_EFFECT_ID: i64 = 31_602;
const ATTACK_SPEED_ATTRIBUTE_ID: i32 = 11_720;
const CAST_SPEED_ATTRIBUTE_ID: i32 = 11_730;
const NORMAL_LANE: &str = "normal_attack_speed_attr_11720_plus_temporary_700";
const GUIDE_LANE: &str = "guide_speed_attr_11730_plus_temporary_710";

#[derive(Debug)]
struct Arguments {
    membership: PathBuf,
    fight_attr_table: PathBuf,
    source_manifest: PathBuf,
    rlogs: Vec<PathBuf>,
    output: PathBuf,
    build: String,
}

#[derive(Debug, Deserialize)]
struct MembershipBundle {
    schema_version: u16,
    game_build: String,
    effect_id: i64,
    damage_action_memberships: Vec<Membership>,
}

#[derive(Debug, Deserialize)]
struct Membership {
    session_id: String,
    sequence: u64,
    effect_provider_actor_id: String,
    effect_provider_entity_uuid: String,
    effect_endpoint_actor_id: String,
    effect_endpoint_entity_uuid: String,
    damage_actor_id: String,
    damage_actor_entity_uuid: String,
    damage_route: DamageRoute,
    ordinary_damage: DamageTotals,
}

#[derive(Debug, Deserialize)]
struct DamageRoute {
    speed_lane: Option<String>,
    candidate_skill_id: Option<i64>,
}

#[derive(Debug, Deserialize)]
struct DamageTotals {
    reported_amount_units: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
struct EntityKey {
    actor_id: u64,
    entity_uuid: i64,
}

impl From<EntityRef> for EntityKey {
    fn from(value: EntityRef) -> Self {
        Self {
            actor_id: value.actor_id.0,
            entity_uuid: value.entity_uuid.0,
        }
    }
}

#[derive(Debug, Default)]
struct AttributeState {
    snapshot_after_boundary: bool,
    snapshot_sequence: Option<u64>,
    values: BTreeMap<i32, ObservedValue>,
}

#[derive(Debug, Clone, Copy)]
struct ObservedValue {
    value: Option<i64>,
    sequence: u64,
    update_kind: EntityAttributeUpdateKind,
}

#[derive(Debug, Serialize)]
struct Report {
    schema_version: u16,
    generated_by: &'static str,
    game: &'static str,
    deployment: &'static str,
    game_build: String,
    effect_id: i64,
    proof_state: &'static str,
    inputs: Inputs,
    relationship_model: RelationshipModel,
    policy: Policy,
    current_build_fight_attribute_definitions: Vec<FightAttributeDefinition>,
    rows: Vec<StateRow>,
    summary: Summary,
    blockers: Vec<&'static str>,
}

#[derive(Debug, Serialize)]
struct Inputs {
    damage_action_membership_ledger: Receipt,
    current_build_fight_attr_table: Receipt,
    complete_build_source_manifest: Receipt,
    sealed_rlogs: Vec<RlogReceipt>,
}

#[derive(Debug, Serialize)]
struct Receipt {
    path: String,
    bytes: u64,
    sha256: String,
}

#[derive(Debug, Serialize)]
struct RlogReceipt {
    path: String,
    session_id: String,
    replay_content_sha256: String,
    event_count: u64,
}

#[derive(Debug, Serialize)]
struct RelationshipModel {
    provider_edge: &'static str,
    damage_edge: &'static str,
    selected_effect_endpoint_damage_role: &'static str,
}

#[derive(Debug, Serialize)]
struct Policy {
    exact_numeric_ids_and_build_are_authoritative: bool,
    remote_player_cast_packets_required: bool,
    damage_event_time_is_action_start_time: bool,
    missing_snapshot_or_attribute_is_zero: bool,
    current_character_snapshot_backfill_allowed: bool,
    ordinary_damage_totals_unchanged: bool,
    provider_rdps_credit_allowed: bool,
}

#[derive(Debug, Serialize)]
struct FightAttributeDefinition {
    attribute_id: i32,
    enum_name: String,
    value_type: String,
    lower_limit: i64,
    upper_limit: i64,
    base_value: i64,
    exact_current_build_static_data: bool,
}

#[derive(Debug, Clone, Serialize)]
struct StateRow {
    session_id: String,
    sequence: u64,
    effect_provider_actor_id: u64,
    effect_provider_entity_uuid: i64,
    effect_endpoint_actor_id: u64,
    effect_endpoint_entity_uuid: i64,
    damage_actor_id: u64,
    damage_actor_entity_uuid: i64,
    root_skill_id: i64,
    speed_lane: String,
    required_attribute_id: i32,
    damage_event_time_state_resolution: &'static str,
    snapshot_observed_after_boundary: bool,
    snapshot_sequence: Option<u64>,
    attribute_present_in_state: bool,
    attribute_value: Option<i64>,
    attribute_observation_sequence: Option<u64>,
    attribute_observation_update_kind: Option<&'static str>,
    reported_damage_units: String,
    action_start_time_state_proven: bool,
    formula_authority: bool,
}

#[derive(Debug, Default, Serialize)]
struct Summary {
    responsive_damage_action_memberships: u64,
    responsive_reported_damage_units: String,
    exact_damage_time_value_after_observation_memberships: u64,
    exact_damage_time_value_after_observation_reported_damage_units: String,
    exact_damage_time_absence_after_snapshot_memberships: u64,
    exact_damage_time_absence_after_snapshot_reported_damage_units: String,
    unresolved_no_snapshot_memberships: u64,
    unresolved_no_snapshot_reported_damage_units: String,
    unresolved_undecoded_value_memberships: u64,
    unresolved_undecoded_value_reported_damage_units: String,
    action_start_time_state_proven_memberships: u64,
    provider_rdps_credit_allowed: bool,
    runtime_promotion_allowed: bool,
    observed_damage_reassigned_to_provider: i64,
}

#[derive(Debug, Default)]
struct Totals {
    memberships: u64,
    damage: i128,
}

fn main() {
    if let Err(error) = run() {
        eprintln!("action-speed damage-time state proof failed: {error}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), Box<dyn std::error::Error>> {
    let args = arguments()?;
    if args.build != EXPECTED_BUILD {
        return Err(format!("this proof supports exact build {EXPECTED_BUILD}").into());
    }
    if args.output.exists() {
        return Err(format!("refusing to overwrite {}", args.output.display()).into());
    }
    let membership_bytes = fs::read(&args.membership)?;
    let fight_attr_bytes = fs::read(&args.fight_attr_table)?;
    let source_manifest_bytes = fs::read(&args.source_manifest)?;
    let fight_attribute_definitions =
        validate_fight_attribute_inputs(&fight_attr_bytes, &source_manifest_bytes, &args.build)?;
    let membership: MembershipBundle = serde_json::from_slice(&membership_bytes)?;
    if membership.schema_version != 8
        || membership.game_build != EXPECTED_BUILD
        || membership.effect_id != EXPECTED_EFFECT_ID
    {
        return Err("membership ledger is not the reviewed exact-build frontier".into());
    }

    let mut selected = Vec::<SelectedMembership>::new();
    for row in membership.damage_action_memberships {
        let Some((speed_lane, required_attribute_id)) = row
            .damage_route
            .speed_lane
            .as_deref()
            .and_then(required_attribute_for_lane)
        else {
            continue;
        };
        let effect_provider = EntityKey {
            actor_id: row.effect_provider_actor_id.parse()?,
            entity_uuid: row.effect_provider_entity_uuid.parse()?,
        };
        let effect_endpoint = EntityKey {
            actor_id: row.effect_endpoint_actor_id.parse()?,
            entity_uuid: row.effect_endpoint_entity_uuid.parse()?,
        };
        let actor = EntityKey {
            actor_id: row.damage_actor_id.parse()?,
            entity_uuid: row.damage_actor_entity_uuid.parse()?,
        };
        if effect_endpoint != actor {
            return Err("source-side effect endpoint is not the damage actor".into());
        }
        selected.push(SelectedMembership {
            session_id: row.session_id,
            sequence: row.sequence,
            effect_provider,
            effect_endpoint,
            actor,
            root_skill_id: row
                .damage_route
                .candidate_skill_id
                .ok_or("responsive membership lost exact root skill ID")?,
            speed_lane: speed_lane.to_owned(),
            required_attribute_id,
            reported_damage: row.ordinary_damage.reported_amount_units.parse()?,
        });
    }
    let mut lookup = BTreeMap::<(String, u64), usize>::new();
    for (index, row) in selected.iter().enumerate() {
        if lookup
            .insert((row.session_id.clone(), row.sequence), index)
            .is_some()
        {
            return Err("responsive membership key is not unique".into());
        }
    }

    let mut results = vec![None; selected.len()];
    let mut receipts = Vec::new();
    let mut seen_sessions = BTreeSet::new();
    for rlog in &args.rlogs {
        let receipt = replay_rlog(rlog, &selected, &lookup, &mut results)?;
        if !seen_sessions.insert(receipt.session_id.clone()) {
            return Err(format!("duplicate RLOG session {}", receipt.session_id).into());
        }
        receipts.push(receipt);
    }
    let missing = results.iter().filter(|row| row.is_none()).count();
    if missing > 0 {
        return Err(
            format!("{missing} responsive memberships were not found in supplied RLOGs").into(),
        );
    }
    let rows = results.into_iter().flatten().collect::<Vec<_>>();
    let summary = summarize(&rows)?;
    if summary.responsive_damage_action_memberships as usize != selected.len() {
        return Err("responsive membership conservation failed".into());
    }

    let report = Report {
        schema_version: SCHEMA_VERSION,
        generated_by: "rlogs-bpsr-action-speed-damage-time-state-proof",
        game: "blue-protocol-star-resonance",
        deployment: "global",
        game_build: args.build,
        effect_id: EXPECTED_EFFECT_ID,
        proof_state: "exact-recipient-damage-event-time-ordinary-speed-state-audited-remote-action-start-open",
        inputs: Inputs {
            damage_action_membership_ledger: Receipt {
                path: display_path(&args.membership),
                bytes: membership_bytes.len() as u64,
                sha256: hex_digest(&membership_bytes),
            },
            current_build_fight_attr_table: Receipt {
                path: display_path(&args.fight_attr_table),
                bytes: fight_attr_bytes.len() as u64,
                sha256: hex_digest(&fight_attr_bytes),
            },
            complete_build_source_manifest: Receipt {
                path: display_path(&args.source_manifest),
                bytes: source_manifest_bytes.len() as u64,
                sha256: hex_digest(&source_manifest_bytes),
            },
            sealed_rlogs: receipts,
        },
        relationship_model: RelationshipModel {
            provider_edge: "provider -> effect/status lifecycle -> recipient or enemy target",
            damage_edge: "recipient damage action -> recipient or enemy target",
            selected_effect_endpoint_damage_role: "damage_actor",
        },
        policy: Policy {
            exact_numeric_ids_and_build_are_authoritative: true,
            remote_player_cast_packets_required: false,
            damage_event_time_is_action_start_time: false,
            missing_snapshot_or_attribute_is_zero: false,
            current_character_snapshot_backfill_allowed: false,
            ordinary_damage_totals_unchanged: true,
            provider_rdps_credit_allowed: false,
        },
        current_build_fight_attribute_definitions: fight_attribute_definitions,
        rows,
        summary,
        blockers: vec![
            "remote damage packets do not establish the exact action-start timestamp",
            "an attribute absent after a complete snapshot is retained as absence and is not promoted to numeric zero",
            "provider-removed opportunity, operation order, integer rounding, and final conservation remain open",
            "current-build protocol-pack identity and required replay gates remain missing",
        ],
    };
    if let Some(parent) = args.output.parent() {
        fs::create_dir_all(parent)?;
    }
    let mut writer = BufWriter::new(File::create(&args.output)?);
    serde_json::to_writer_pretty(&mut writer, &report)?;
    writer.write_all(b"\n")?;
    writer.flush()?;
    println!(
        "audited {} responsive memberships; {} have exact damage-time values after observations; provider credit=false",
        report.summary.responsive_damage_action_memberships,
        report
            .summary
            .exact_damage_time_value_after_observation_memberships
    );
    println!("wrote {}", args.output.display());
    Ok(())
}

#[derive(Debug)]
struct SelectedMembership {
    session_id: String,
    sequence: u64,
    effect_provider: EntityKey,
    effect_endpoint: EntityKey,
    actor: EntityKey,
    root_skill_id: i64,
    speed_lane: String,
    required_attribute_id: i32,
    reported_damage: i128,
}

fn required_attribute_for_lane(lane: &str) -> Option<(&str, i32)> {
    match lane {
        NORMAL_LANE => Some((NORMAL_LANE, ATTACK_SPEED_ATTRIBUTE_ID)),
        GUIDE_LANE => Some((GUIDE_LANE, CAST_SPEED_ATTRIBUTE_ID)),
        _ => None,
    }
}

fn replay_rlog(
    path: &Path,
    selected: &[SelectedMembership],
    lookup: &BTreeMap<(String, u64), usize>,
    results: &mut [Option<StateRow>],
) -> Result<RlogReceipt, Box<dyn std::error::Error>> {
    let mut reader = RlogReader::new(BufReader::new(File::open(path)?), RlogLimits::default())?;
    let mut states = BTreeMap::<EntityKey, AttributeState>::new();
    let mut session_id = String::new();
    while let Some(envelope) = reader.next_event()? {
        if session_id.is_empty() {
            session_id = envelope.session_id.clone();
        }
        let CanonicalEvent::Timeline(timeline) = &envelope.event else {
            continue;
        };
        match &timeline.kind {
            TimelineEventKind::DataGap(_)
            | TimelineEventKind::RecorderPause(_)
            | TimelineEventKind::RunBoundary { .. } => states.clear(),
            TimelineEventKind::Actor(actor) if actor.state == ActorState::Despawned => {
                states.remove(&EntityKey::from(actor.actor));
            }
            TimelineEventKind::EntityAttributes(event) => {
                let state = states.entry(EntityKey::from(event.actor)).or_default();
                match event.update_kind {
                    EntityAttributeUpdateKind::Snapshot => {
                        state.values.clear();
                        state.snapshot_after_boundary = true;
                        state.snapshot_sequence = Some(envelope.sequence);
                    }
                    EntityAttributeUpdateKind::Unknown => {
                        state.snapshot_after_boundary = false;
                        state.snapshot_sequence = None;
                    }
                    EntityAttributeUpdateKind::Delta => {}
                }
                for attribute in &event.attributes {
                    if attribute.attribute_id != ATTACK_SPEED_ATTRIBUTE_ID
                        && attribute.attribute_id != CAST_SPEED_ATTRIBUTE_ID
                    {
                        continue;
                    }
                    let decoded = attribute
                        .decoded
                        .clone()
                        .or_else(|| {
                            decode_known_entity_attribute_value(
                                attribute.attribute_id,
                                &attribute.raw_value,
                            )
                        })
                        .or_else(|| {
                            decode_exact_varint_i64(&attribute.raw_value)
                                .map(EntityAttributeValue::Integer)
                        });
                    let value = match decoded {
                        Some(EntityAttributeValue::Integer(value)) => Some(value),
                        _ => None,
                    };
                    state.values.insert(
                        attribute.attribute_id,
                        ObservedValue {
                            value,
                            sequence: envelope.sequence,
                            update_kind: event.update_kind,
                        },
                    );
                }
            }
            TimelineEventKind::Damage(damage) => {
                let Some(&index) = lookup.get(&(envelope.session_id.clone(), envelope.sequence))
                else {
                    continue;
                };
                let selected = &selected[index];
                if selected.actor != EntityKey::from(damage.source)
                    || i128::from(damage.amount) != selected.reported_damage
                {
                    return Err(format!(
                        "membership mismatch at {} sequence {}",
                        envelope.session_id, envelope.sequence
                    )
                    .into());
                }
                if results[index].is_some() {
                    return Err("membership was observed more than once".into());
                }
                results[index] = Some(project_state(selected, states.get(&selected.actor)));
            }
            _ => {}
        }
    }
    let replay = reader
        .summary()
        .ok_or("sealed RLOG replay summary is missing")?;
    Ok(RlogReceipt {
        path: display_path(path),
        session_id,
        replay_content_sha256: replay.content_sha256.clone(),
        event_count: replay.event_count,
    })
}

fn project_state(selected: &SelectedMembership, state: Option<&AttributeState>) -> StateRow {
    let snapshot_observed = state.is_some_and(|value| value.snapshot_after_boundary);
    let observed = state.and_then(|value| value.values.get(&selected.required_attribute_id));
    let resolution = if observed.and_then(|value| value.value).is_some() {
        "exact_damage_time_value_after_observation"
    } else if observed.is_some() {
        "unresolved_undecoded_value"
    } else if snapshot_observed {
        "exact_damage_time_absence_after_snapshot"
    } else {
        "unresolved_no_snapshot"
    };
    StateRow {
        session_id: selected.session_id.clone(),
        sequence: selected.sequence,
        effect_provider_actor_id: selected.effect_provider.actor_id,
        effect_provider_entity_uuid: selected.effect_provider.entity_uuid,
        effect_endpoint_actor_id: selected.effect_endpoint.actor_id,
        effect_endpoint_entity_uuid: selected.effect_endpoint.entity_uuid,
        damage_actor_id: selected.actor.actor_id,
        damage_actor_entity_uuid: selected.actor.entity_uuid,
        root_skill_id: selected.root_skill_id,
        speed_lane: selected.speed_lane.clone(),
        required_attribute_id: selected.required_attribute_id,
        damage_event_time_state_resolution: resolution,
        snapshot_observed_after_boundary: snapshot_observed,
        snapshot_sequence: state.and_then(|value| value.snapshot_sequence),
        attribute_present_in_state: observed.is_some(),
        attribute_value: observed.and_then(|value| value.value),
        attribute_observation_sequence: observed.map(|value| value.sequence),
        attribute_observation_update_kind: observed.map(|value| match value.update_kind {
            EntityAttributeUpdateKind::Snapshot => "snapshot",
            EntityAttributeUpdateKind::Delta => "delta",
            EntityAttributeUpdateKind::Unknown => "unknown",
        }),
        reported_damage_units: selected.reported_damage.to_string(),
        action_start_time_state_proven: false,
        formula_authority: false,
    }
}

fn summarize(rows: &[StateRow]) -> Result<Summary, Box<dyn std::error::Error>> {
    let mut all = Totals::default();
    let mut exact = Totals::default();
    let mut absent = Totals::default();
    let mut no_snapshot = Totals::default();
    let mut undecoded = Totals::default();
    for row in rows {
        let damage: i128 = row.reported_damage_units.parse()?;
        add(&mut all, damage);
        match row.damage_event_time_state_resolution {
            "exact_damage_time_value_after_observation" => add(&mut exact, damage),
            "exact_damage_time_absence_after_snapshot" => add(&mut absent, damage),
            "unresolved_no_snapshot" => add(&mut no_snapshot, damage),
            "unresolved_undecoded_value" => add(&mut undecoded, damage),
            other => return Err(format!("unknown resolution {other}").into()),
        }
    }
    if all.memberships
        != exact.memberships + absent.memberships + no_snapshot.memberships + undecoded.memberships
        || all.damage != exact.damage + absent.damage + no_snapshot.damage + undecoded.damage
    {
        return Err("damage-time state partition does not conserve memberships and damage".into());
    }
    Ok(Summary {
        responsive_damage_action_memberships: all.memberships,
        responsive_reported_damage_units: all.damage.to_string(),
        exact_damage_time_value_after_observation_memberships: exact.memberships,
        exact_damage_time_value_after_observation_reported_damage_units: exact.damage.to_string(),
        exact_damage_time_absence_after_snapshot_memberships: absent.memberships,
        exact_damage_time_absence_after_snapshot_reported_damage_units: absent.damage.to_string(),
        unresolved_no_snapshot_memberships: no_snapshot.memberships,
        unresolved_no_snapshot_reported_damage_units: no_snapshot.damage.to_string(),
        unresolved_undecoded_value_memberships: undecoded.memberships,
        unresolved_undecoded_value_reported_damage_units: undecoded.damage.to_string(),
        action_start_time_state_proven_memberships: 0,
        provider_rdps_credit_allowed: false,
        runtime_promotion_allowed: false,
        observed_damage_reassigned_to_provider: 0,
    })
}

fn add(total: &mut Totals, damage: i128) {
    total.memberships = total.memberships.saturating_add(1);
    total.damage += damage;
}

fn validate_fight_attribute_inputs(
    table_bytes: &[u8],
    manifest_bytes: &[u8],
    build: &str,
) -> Result<Vec<FightAttributeDefinition>, Box<dyn std::error::Error>> {
    let manifest: serde_json::Value = serde_json::from_slice(manifest_bytes)?;
    let manifest_build_matches = manifest
        .get("gameBuild")
        .and_then(serde_json::Value::as_str)
        .is_some_and(|value| value == build)
        || manifest
            .get("gameBuild")
            .and_then(serde_json::Value::as_i64)
            .is_some_and(|value| value.to_string() == build);
    if !manifest_build_matches {
        return Err("complete-build source manifest build changed".into());
    }
    let entry = manifest
        .get("files")
        .and_then(serde_json::Value::as_array)
        .and_then(|files| {
            files.iter().find(|entry| {
                entry.get("id").and_then(serde_json::Value::as_str)
                    == Some("decoded-game-tables:FightAttrTable.json")
            })
        })
        .ok_or("complete-build source manifest omits FightAttrTable")?;
    if entry.get("authority").and_then(serde_json::Value::as_str)
        != Some("exact-current-build-static-data")
        || entry.get("bytes").and_then(serde_json::Value::as_u64) != Some(table_bytes.len() as u64)
        || entry.get("sha256").and_then(serde_json::Value::as_str)
            != Some(hex_digest(table_bytes).as_str())
    {
        return Err("FightAttrTable does not match its exact-current-build manifest entry".into());
    }
    let table: serde_json::Value = serde_json::from_slice(table_bytes)?;
    let mut definitions = Vec::new();
    for (attribute_id, expected_enum) in [
        (ATTACK_SPEED_ATTRIBUTE_ID, "AttrAttackSpeedPCT"),
        (CAST_SPEED_ATTRIBUTE_ID, "AttrCastSpeedPCT"),
    ] {
        let row = table
            .get(attribute_id.to_string())
            .ok_or("FightAttrTable speed row is missing")?;
        if row.get("Id").and_then(serde_json::Value::as_i64) != Some(i64::from(attribute_id))
            || row.get("EnumName").and_then(serde_json::Value::as_str) != Some(expected_enum)
            || row.get("Type").and_then(serde_json::Value::as_str) != Some("int32")
        {
            return Err(
                format!("FightAttrTable row {attribute_id} identity or type changed").into(),
            );
        }
        definitions.push(FightAttributeDefinition {
            attribute_id,
            enum_name: expected_enum.to_owned(),
            value_type: "int32 protobuf scalar bytes".to_owned(),
            lower_limit: row
                .get("AttrLowerLimit")
                .and_then(serde_json::Value::as_i64)
                .ok_or("FightAttrTable lower limit is missing")?,
            upper_limit: row
                .get("AttrUpperLimit")
                .and_then(serde_json::Value::as_i64)
                .ok_or("FightAttrTable upper limit is missing")?,
            base_value: row
                .get("BaseAttr")
                .and_then(serde_json::Value::as_i64)
                .ok_or("FightAttrTable base value is missing")?,
            exact_current_build_static_data: true,
        });
    }
    Ok(definitions)
}

fn decode_exact_varint_i64(bytes: &[u8]) -> Option<i64> {
    if bytes.is_empty() {
        return Some(0);
    }
    let mut value = 0_u64;
    for (index, byte) in bytes.iter().copied().take(10).enumerate() {
        if index == 9 && byte > 1 {
            return None;
        }
        value |= u64::from(byte & 0x7f) << (index * 7);
        if byte & 0x80 == 0 {
            return (index + 1 == bytes.len()).then_some(value as i64);
        }
    }
    None
}

fn arguments() -> Result<Arguments, String> {
    let mut values = env::args_os().skip(1).collect::<Vec<_>>();
    let membership = PathBuf::from(take_value(&mut values, "--membership")?);
    let fight_attr_table = PathBuf::from(take_value(&mut values, "--fight-attr-table")?);
    let source_manifest = PathBuf::from(take_value(&mut values, "--source-manifest")?);
    let output = PathBuf::from(take_value(&mut values, "--output")?);
    let build = take_value(&mut values, "--build")?
        .into_string()
        .map_err(|_| "--build is not valid Unicode")?;
    let mut rlogs = Vec::new();
    while let Some(position) = values.iter().position(|value| value == "--rlog") {
        if position + 1 >= values.len() {
            return Err("--rlog requires a value".to_owned());
        }
        rlogs.push(PathBuf::from(values.remove(position + 1)));
        values.remove(position);
    }
    if rlogs.is_empty() {
        return Err("at least one --rlog is required".to_owned());
    }
    if !values.is_empty() {
        return Err(format!(
            "unexpected argument: {}",
            values[0].to_string_lossy()
        ));
    }
    Ok(Arguments {
        membership,
        fight_attr_table,
        source_manifest,
        rlogs,
        output,
        build,
    })
}

fn take_value(values: &mut Vec<OsString>, flag: &str) -> Result<OsString, String> {
    let position = values
        .iter()
        .position(|value| value == flag)
        .ok_or_else(|| format!("missing {flag}"))?;
    if position + 1 >= values.len() {
        return Err(format!("{flag} requires a value"));
    }
    let value = values.remove(position + 1);
    values.remove(position);
    Ok(value)
}

fn display_path(path: &Path) -> String {
    path.display().to_string().replace('\\', "/")
}

fn hex_digest(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    format!("{:x}", hasher.finalize())
}

#[allow(dead_code)]
fn stream_digest(path: &Path) -> Result<String, Box<dyn std::error::Error>> {
    let mut reader = BufReader::new(File::open(path)?);
    let mut hasher = Sha256::new();
    let mut buffer = [0_u8; 64 * 1024];
    loop {
        let read = reader.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    Ok(format!("{:x}", hasher.finalize()))
}
