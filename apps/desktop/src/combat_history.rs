use std::collections::{HashMap, HashSet};
use std::fs::OpenOptions;
use std::io::{BufWriter, Write};
use std::path::{Path, PathBuf};

use rlogs_plugin_combat_meter::{
    COMBAT_HISTORY_SCHEMA_VERSION, CombatHistorySnapshot, HistoryLoadoutSlot,
};
use serde::{Deserialize, Serialize};

const CATALOG_SCHEMA_VERSION: u16 = 1;
const CATALOG_SUMMARY_VERSION: u16 = 2;
const MAXIMUM_HISTORY_ENTRIES: usize = 2_048;
const MAXIMUM_HISTORY_DETAIL_BYTES: u64 = 128 * 1024 * 1024;
const MAXIMUM_HISTORY_INDEX_BYTES: u64 = 16 * 1024 * 1024;
const MAXIMUM_PARTICIPANTS_PER_RUN: usize = 40;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CombatHistoryCatalog {
    pub schema_version: u16,
    pub entries: Vec<CombatHistoryCatalogEntry>,
}

impl Default for CombatHistoryCatalog {
    fn default() -> Self {
        Self {
            schema_version: CATALOG_SCHEMA_VERSION,
            entries: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CombatHistoryCatalogEntry {
    pub history_id: String,
    #[serde(default)]
    pub is_favorite: bool,
    pub session_id: String,
    pub run_index: u32,
    pub captured_unix_millis: u64,
    pub activity_id: Option<String>,
    pub activity_family_id: Option<String>,
    pub scene_id: Option<i32>,
    #[serde(default)]
    pub presentation_scene_name: Option<String>,
    pub difficulty_family: Option<String>,
    pub difficulty_tier: Option<u32>,
    pub terminal_state: String,
    pub game_time_micros: Option<u64>,
    #[serde(default)]
    pub total_run_time_micros: Option<u64>,
    pub active_combat_micros: u64,
    pub player_count: usize,
    #[serde(default)]
    pub deployment_id: String,
    #[serde(default)]
    pub region_id: String,
    #[serde(default)]
    pub world_id: Option<String>,
    #[serde(default)]
    pub team_damage: i64,
    #[serde(default)]
    pub team_dps: f64,
    #[serde(default)]
    pub team_encounter_dps: f64,
    #[serde(default)]
    pub true_time_micros: Option<u64>,
    #[serde(default)]
    pub retry_count: u32,
    #[serde(default)]
    pub boss_retry_count: u32,
    #[serde(default)]
    pub wipe_count: u32,
    #[serde(default)]
    pub cleared_encounter_count: u32,
    #[serde(default)]
    pub last_encounter_terminal_state: Option<String>,
    #[serde(default)]
    pub participants: Vec<CombatHistoryParticipant>,
    #[serde(default)]
    summary_version: u16,
}

#[derive(Debug, Clone, Serialize)]
pub struct CombatHistoryDeleteResult {
    pub requested_count: usize,
    pub deleted_count: usize,
    pub preserved_favorite_count: usize,
    pub unknown_history_id_count: usize,
    pub cleanup_warnings: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CombatHistoryParticipant {
    pub actor_id: String,
    pub entity_uuid: String,
    pub display_name: Option<String>,
    pub actor_kind: Option<String>,
    pub class_id: Option<i32>,
    #[serde(default)]
    pub specialization_id: Option<i32>,
    #[serde(default)]
    pub presentation_class_name: Option<String>,
    #[serde(default)]
    pub presentation_specialization_name: Option<String>,
    pub level: Option<u32>,
    #[serde(default)]
    pub ability_score: Option<i64>,
    #[serde(default)]
    pub weapon_item_id: Option<i64>,
    #[serde(default)]
    pub weapon_breakthrough_count: Option<u32>,
    #[serde(default)]
    pub weapon_icon_asset_path: Option<String>,
    #[serde(default)]
    pub weapon_presentation_name: Option<String>,
    #[serde(default)]
    pub weapon_level: Option<u32>,
    #[serde(default)]
    pub weapon_level_min: Option<u32>,
    #[serde(default)]
    pub weapon_level_max: Option<u32>,
    #[serde(default)]
    pub weapon_badge_kind: Option<String>,
    #[serde(default)]
    pub seasonal_score: Option<i64>,
    #[serde(default)]
    pub primary_loadout: Vec<HistoryLoadoutSlot>,
    #[serde(default)]
    pub auxiliary_loadout: Vec<HistoryLoadoutSlot>,
    pub damage: i64,
    pub dps: f64,
    pub encounter_dps: f64,
    #[serde(default)]
    pub character_id: Option<String>,
    #[serde(default)]
    pub presentation_name: Option<String>,
    #[serde(default)]
    pub presentation_kind: Option<String>,
    #[serde(default)]
    pub icon_asset_path: Option<String>,
    #[serde(default)]
    pub presentation_role: Option<String>,
    #[serde(default)]
    pub presentation_accent: Option<String>,
}

#[derive(Debug)]
pub struct CombatHistoryStore {
    directory: PathBuf,
    catalog: CombatHistoryCatalog,
}

impl CombatHistoryStore {
    pub fn open(directory: PathBuf) -> Result<Self, String> {
        std::fs::create_dir_all(&directory)
            .map_err(|error| format!("could not create combat history directory: {error}"))?;
        let catalog = load_catalog(&directory.join("index.v1.json"))?;
        let mut store = Self { directory, catalog };
        if store.backfill_compact_summaries()? {
            store.persist_catalog()?;
        }
        Ok(store)
    }

    pub fn catalog(&self) -> CombatHistoryCatalog {
        self.catalog.clone()
    }

    pub fn detail(&self, session_id: &str) -> Result<CombatHistorySnapshot, String> {
        validate_session_id(session_id)?;
        let path = self.directory.join(detail_file_name(session_id));
        read_snapshot(&path, session_id)
    }

    /// Atomically replaces only the derived rDPS projection in an existing
    /// history artifact. The saved ordinary combat cube remains authoritative;
    /// replay must match it exactly before any formula result is accepted.
    pub fn refresh_rdps_projection(
        &self,
        projection: &CombatHistorySnapshot,
    ) -> Result<CombatHistorySnapshot, String> {
        validate_session_id(&projection.session_id)?;
        let path = self
            .directory
            .join(detail_file_name(&projection.session_id));
        let existing = read_snapshot(&path, &projection.session_id)?;
        let refreshed = merge_rdps_projection(&existing, projection)?;
        write_json_atomic(&path, &refreshed, MAXIMUM_HISTORY_DETAIL_BYTES)?;
        Ok(refreshed)
    }

    pub fn set_favorite(
        &mut self,
        history_id: &str,
        is_favorite: bool,
    ) -> Result<CombatHistoryCatalog, String> {
        let Some(index) = self
            .catalog
            .entries
            .iter()
            .position(|entry| entry.history_id == history_id)
        else {
            return Err("combat history entry is unavailable".into());
        };
        let previous = self.catalog.entries[index].is_favorite;
        if previous == is_favorite {
            return Ok(self.catalog());
        }
        self.catalog.entries[index].is_favorite = is_favorite;
        if let Err(error) = self.persist_catalog() {
            self.catalog.entries[index].is_favorite = previous;
            return Err(error);
        }
        Ok(self.catalog())
    }

    pub fn delete_entries(
        &mut self,
        history_ids: &[String],
    ) -> Result<CombatHistoryDeleteResult, String> {
        if history_ids.len() > MAXIMUM_HISTORY_ENTRIES {
            return Err("too many combat history entries were selected".into());
        }
        let requested = history_ids.iter().collect::<HashSet<_>>();
        let matched = self
            .catalog
            .entries
            .iter()
            .filter(|entry| requested.contains(&entry.history_id))
            .collect::<Vec<_>>();
        let preserved_favorite_count = matched.iter().filter(|entry| entry.is_favorite).count();
        let deleted_ids = matched
            .iter()
            .filter(|entry| !entry.is_favorite)
            .map(|entry| entry.history_id.clone())
            .collect::<HashSet<_>>();
        let affected_sessions = matched
            .iter()
            .filter(|entry| deleted_ids.contains(&entry.history_id))
            .map(|entry| entry.session_id.clone())
            .collect::<HashSet<_>>();
        let unknown_history_id_count = requested.len().saturating_sub(matched.len());
        if deleted_ids.is_empty() {
            return Ok(CombatHistoryDeleteResult {
                requested_count: requested.len(),
                deleted_count: 0,
                preserved_favorite_count,
                unknown_history_id_count,
                cleanup_warnings: Vec::new(),
            });
        }

        let mut next_catalog = self.catalog.clone();
        next_catalog
            .entries
            .retain(|entry| !deleted_ids.contains(&entry.history_id));
        write_json_atomic(
            &self.directory.join("index.v1.json"),
            &next_catalog,
            MAXIMUM_HISTORY_INDEX_BYTES,
        )?;
        self.catalog = next_catalog;

        let mut cleanup_warnings = Vec::new();
        for session_id in affected_sessions {
            let retained_run_indices = self
                .catalog
                .entries
                .iter()
                .filter(|entry| entry.session_id == session_id)
                .map(|entry| entry.run_index)
                .collect::<HashSet<_>>();
            let path = self.directory.join(detail_file_name(&session_id));
            if retained_run_indices.is_empty() {
                match std::fs::remove_file(&path) {
                    Ok(()) => {}
                    Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                    Err(error) => cleanup_warnings.push(format!(
                        "could not remove the detail artifact for {session_id}: {error}"
                    )),
                }
                continue;
            }

            match read_snapshot(&path, &session_id) {
                Ok(mut snapshot) => {
                    let prior_count = snapshot.runs.len();
                    snapshot
                        .runs
                        .retain(|run| retained_run_indices.contains(&run.run_index));
                    if snapshot.runs.len() != prior_count
                        && let Err(error) =
                            write_json_atomic(&path, &snapshot, MAXIMUM_HISTORY_DETAIL_BYTES)
                    {
                        cleanup_warnings.push(error);
                    }
                }
                Err(error) => cleanup_warnings.push(error),
            }
        }

        Ok(CombatHistoryDeleteResult {
            requested_count: requested.len(),
            deleted_count: deleted_ids.len(),
            preserved_favorite_count,
            unknown_history_id_count,
            cleanup_warnings,
        })
    }

    pub fn record(
        &mut self,
        snapshot: &CombatHistorySnapshot,
        captured_unix_millis: u64,
    ) -> Result<CombatHistoryCatalog, String> {
        validate_snapshot(snapshot)?;
        validate_session_id(&snapshot.session_id)?;
        let detail_path = self.directory.join(detail_file_name(&snapshot.session_id));
        write_json_atomic(&detail_path, snapshot, MAXIMUM_HISTORY_DETAIL_BYTES)?;

        let favorites = self
            .catalog
            .entries
            .iter()
            .filter(|entry| entry.session_id == snapshot.session_id && entry.is_favorite)
            .map(|entry| (entry.history_id.clone(), true))
            .collect::<HashMap<_, _>>();
        self.catalog
            .entries
            .retain(|entry| entry.session_id != snapshot.session_id);
        for run in &snapshot.runs {
            let all = run.views.iter().find(|view| view.id == "all");
            let mut entry = CombatHistoryCatalogEntry {
                history_id: format!("{}:{}", snapshot.session_id, run.run_index),
                is_favorite: false,
                session_id: snapshot.session_id.clone(),
                run_index: run.run_index,
                captured_unix_millis,
                activity_id: run.activity_id.clone(),
                activity_family_id: run.activity_family_id.clone(),
                scene_id: run.scene_id,
                presentation_scene_name: None,
                difficulty_family: run.difficulty_family.clone(),
                difficulty_tier: run.difficulty_tier,
                terminal_state: run.terminal_state.clone(),
                game_time_micros: run.game_time_micros,
                total_run_time_micros: run.total_run_time_micros.or_else(|| {
                    run.entered_micros
                        .zip(run.ended_micros)
                        .map(|(entered, ended)| ended.saturating_sub(entered))
                }),
                active_combat_micros: all.map_or(0, |view| view.active_combat_micros),
                player_count: all.map_or(0, |view| {
                    view.actors
                        .iter()
                        .filter(|actor| actor.actor_kind.as_deref() == Some("player"))
                        .count()
                }),
                deployment_id: snapshot.deployment_id.clone(),
                region_id: snapshot.region_id.clone(),
                world_id: snapshot.world_id.clone(),
                team_damage: 0,
                team_dps: 0.0,
                team_encounter_dps: 0.0,
                true_time_micros: run.true_time_micros,
                retry_count: run.retry_count,
                boss_retry_count: run.boss_retry_count,
                wipe_count: run.wipe_count,
                cleared_encounter_count: run.cleared_encounter_count,
                last_encounter_terminal_state: run.last_encounter_terminal_state.clone(),
                participants: Vec::new(),
                summary_version: CATALOG_SUMMARY_VERSION,
            };
            entry.is_favorite = favorites.get(&entry.history_id).copied().unwrap_or(false);
            populate_compact_summary(&mut entry, run);
            self.catalog.entries.push(entry);
        }
        self.catalog.entries.sort_by(|left, right| {
            right
                .captured_unix_millis
                .cmp(&left.captured_unix_millis)
                .then_with(|| right.history_id.cmp(&left.history_id))
        });
        self.catalog.entries.truncate(MAXIMUM_HISTORY_ENTRIES);
        self.persist_catalog()?;
        Ok(self.catalog())
    }

    fn backfill_compact_summaries(&mut self) -> Result<bool, String> {
        let mut changed = false;
        for entry in self
            .catalog
            .entries
            .iter_mut()
            .filter(|entry| entry.summary_version < CATALOG_SUMMARY_VERSION)
        {
            let path = self.directory.join(detail_file_name(&entry.session_id));
            let snapshot = read_snapshot(&path, &entry.session_id)?;
            let Some(run) = snapshot
                .runs
                .iter()
                .find(|run| run.run_index == entry.run_index)
            else {
                return Err(format!(
                    "combat history {} is missing indexed run {}",
                    entry.session_id, entry.run_index
                ));
            };
            entry.deployment_id.clone_from(&snapshot.deployment_id);
            entry.region_id.clone_from(&snapshot.region_id);
            entry.world_id.clone_from(&snapshot.world_id);
            entry.game_time_micros = run.game_time_micros;
            entry.total_run_time_micros = run.total_run_time_micros.or_else(|| {
                run.entered_micros
                    .zip(run.ended_micros)
                    .map(|(entered, ended)| ended.saturating_sub(entered))
            });
            entry.true_time_micros = run.true_time_micros;
            entry.retry_count = run.retry_count;
            entry.boss_retry_count = run.boss_retry_count;
            entry.wipe_count = run.wipe_count;
            entry.cleared_encounter_count = run.cleared_encounter_count;
            entry
                .last_encounter_terminal_state
                .clone_from(&run.last_encounter_terminal_state);
            populate_compact_summary(entry, run);
            entry.summary_version = CATALOG_SUMMARY_VERSION;
            changed = true;
        }
        Ok(changed)
    }

    fn persist_catalog(&self) -> Result<(), String> {
        write_json_atomic(
            &self.directory.join("index.v1.json"),
            &self.catalog,
            MAXIMUM_HISTORY_INDEX_BYTES,
        )
    }
}

fn populate_compact_summary(
    entry: &mut CombatHistoryCatalogEntry,
    run: &rlogs_plugin_combat_meter::CombatRunHistory,
) {
    let Some(view) = run.views.iter().find(|view| view.id == "all") else {
        return;
    };
    let mut participants = view
        .actors
        .iter()
        .filter(|actor| actor.actor_kind.as_deref() == Some("player"))
        .map(|actor| CombatHistoryParticipant {
            actor_id: actor.actor_id.clone(),
            entity_uuid: actor.entity_uuid.clone(),
            display_name: actor.display_name.clone(),
            actor_kind: actor.actor_kind.clone(),
            class_id: actor.class_id,
            specialization_id: actor.specialization_id,
            presentation_class_name: actor.presentation_class_name.clone(),
            presentation_specialization_name: actor.presentation_specialization_name.clone(),
            level: actor.level,
            ability_score: actor.ability_score,
            weapon_item_id: actor.weapon_item_id,
            weapon_breakthrough_count: actor.weapon_breakthrough_count,
            weapon_icon_asset_path: actor.weapon_icon_asset_path.clone(),
            weapon_presentation_name: actor.weapon_presentation_name.clone(),
            weapon_level: actor.weapon_level,
            weapon_level_min: actor.weapon_level_min,
            weapon_level_max: actor.weapon_level_max,
            weapon_badge_kind: actor.weapon_badge_kind.clone(),
            seasonal_score: actor.seasonal_score,
            primary_loadout: actor.primary_loadout.clone(),
            auxiliary_loadout: actor.auxiliary_loadout.clone(),
            damage: actor.damage,
            dps: finite_or_zero(actor.dps),
            encounter_dps: finite_or_zero(actor.encounter_dps),
            character_id: None,
            presentation_name: None,
            presentation_kind: None,
            icon_asset_path: None,
            presentation_role: None,
            presentation_accent: None,
        })
        .collect::<Vec<_>>();
    participants.sort_by(|left, right| {
        right
            .damage
            .cmp(&left.damage)
            .then_with(|| left.actor_id.cmp(&right.actor_id))
    });
    participants.truncate(MAXIMUM_PARTICIPANTS_PER_RUN);
    entry.player_count = participants.len();
    entry.team_damage = participants.iter().map(|actor| actor.damage).sum();
    entry.team_dps = participants.iter().map(|actor| actor.dps).sum();
    entry.team_encounter_dps = participants.iter().map(|actor| actor.encounter_dps).sum();
    entry.participants = participants;
}

fn finite_or_zero(value: f64) -> f64 {
    if value.is_finite() { value } else { 0.0 }
}

fn read_snapshot(path: &Path, session_id: &str) -> Result<CombatHistorySnapshot, String> {
    let metadata = std::fs::metadata(path)
        .map_err(|error| format!("combat history {session_id} is unavailable: {error}"))?;
    if metadata.len() > MAXIMUM_HISTORY_DETAIL_BYTES {
        return Err(format!(
            "combat history {session_id} exceeds the {} MiB safety limit",
            MAXIMUM_HISTORY_DETAIL_BYTES / 1024 / 1024
        ));
    }
    let snapshot: CombatHistorySnapshot = serde_json::from_slice(
        &std::fs::read(path)
            .map_err(|error| format!("could not read combat history {session_id}: {error}"))?,
    )
    .map_err(|error| format!("combat history {session_id} is invalid: {error}"))?;
    validate_snapshot(&snapshot)?;
    Ok(snapshot)
}

fn validate_snapshot(snapshot: &CombatHistorySnapshot) -> Result<(), String> {
    if snapshot.schema_version != COMBAT_HISTORY_SCHEMA_VERSION {
        return Err(format!(
            "unsupported combat history schema {}; expected {COMBAT_HISTORY_SCHEMA_VERSION}",
            snapshot.schema_version
        ));
    }
    if snapshot.runs.len() > 64 {
        return Err("combat history contains too many projected runs".into());
    }
    Ok(())
}

pub(crate) fn merge_rdps_projection(
    existing: &CombatHistorySnapshot,
    projection: &CombatHistorySnapshot,
) -> Result<CombatHistorySnapshot, String> {
    validate_snapshot(existing)?;
    validate_snapshot(projection)?;
    if projection.rdps_formula_identity.is_none() {
        return Err("replayed combat history has no rDPS formula identity".into());
    }
    if existing.session_id != projection.session_id
        || existing.deployment_id != projection.deployment_id
        || existing.region_id != projection.region_id
        || existing.world_id != projection.world_id
        || existing.client_build != projection.client_build
        || existing.protocol_pack_digest != projection.protocol_pack_digest
    {
        return Err("replayed combat history identity does not match the saved artifact".into());
    }

    let projected_runs = projection
        .runs
        .iter()
        .map(|run| (run.run_index, run))
        .collect::<HashMap<_, _>>();
    if projected_runs.len() != projection.runs.len() {
        return Err("replayed combat history contains duplicate run indices".into());
    }

    let mut refreshed = existing.clone();
    refreshed
        .rdps_formula_identity
        .clone_from(&projection.rdps_formula_identity);
    for run in &mut refreshed.runs {
        let projected = projected_runs.get(&run.run_index).ok_or_else(|| {
            format!(
                "replayed combat history is missing retained run {}",
                run.run_index
            )
        })?;
        // Activity labels/families may be filled by newer exact-build run
        // rules, but they are presentation metadata and are never copied.
        // Scene, instance, and packet-time boundaries remain authoritative.
        if run.scene_id != projected.scene_id
            || run.instance_id != projected.instance_id
            || run.started_micros != projected.started_micros
            || run.ended_micros != projected.ended_micros
        {
            return Err(format!(
                "replayed combat history run {} does not match the saved boundary",
                run.run_index
            ));
        }
        let projected_views = projected
            .views
            .iter()
            .map(|view| (view.id.as_str(), view))
            .collect::<HashMap<_, _>>();
        if projected_views.len() != projected.views.len() {
            return Err(format!(
                "replayed combat history run {} contains duplicate view IDs",
                run.run_index
            ));
        }
        for view in &mut run.views {
            let projected_view = projected_views.get(view.id.as_str()).ok_or_else(|| {
                format!(
                    "replayed combat history run {} is missing retained view {}",
                    run.run_index, view.id
                )
            })?;
            if view.kind != projected_view.kind
                || view.segment_indices != projected_view.segment_indices
                || view.elapsed_micros != projected_view.elapsed_micros
            {
                return Err(format!(
                    "replayed combat history run {} view {} does not match the saved boundary",
                    run.run_index, view.id
                ));
            }
            validate_rdps_conservation(projected_view).map_err(|error| {
                format!(
                    "replayed combat history run {} view {} is invalid: {error}",
                    run.run_index, view.id
                )
            })?;

            let projected_actors = projected_view
                .actors
                .iter()
                .map(|actor| ((actor.actor_id.as_str(), actor.entity_uuid.as_str()), actor))
                .collect::<HashMap<_, _>>();
            if projected_actors.len() != projected_view.actors.len() {
                return Err(format!(
                    "replayed combat history run {} view {} contains duplicate actor identities",
                    run.run_index, view.id
                ));
            }
            let existing_actor_keys = view
                .actors
                .iter()
                .map(|actor| (actor.actor_id.clone(), actor.entity_uuid.clone()))
                .collect::<HashSet<_>>();
            for actor in &mut view.actors {
                let key = (actor.actor_id.as_str(), actor.entity_uuid.as_str());
                let Some(projected_actor) = projected_actors.get(&key) else {
                    if actor.damage != 0 {
                        return Err(format!(
                            "replayed combat history run {} view {} is missing saved damaging actor {}",
                            run.run_index, view.id, actor.actor_id
                        ));
                    }
                    actor.rdps = None;
                    actor.rdps_damage = None;
                    actor.rdps_contribution_given = None;
                    actor.rdps_contribution_received = None;
                    continue;
                };
                if actor.damage != projected_actor.damage {
                    return Err(format!(
                        "replayed combat history run {} view {} changed ordinary damage for actor {}",
                        run.run_index, view.id, actor.actor_id
                    ));
                }
                actor.rdps = projected_actor.rdps;
                actor.rdps_damage = projected_actor.rdps_damage;
                actor.rdps_contribution_given = projected_actor.rdps_contribution_given;
                actor.rdps_contribution_received = projected_actor.rdps_contribution_received;
            }
            if let Some(actor) = projected_view.actors.iter().find(|actor| {
                !existing_actor_keys.contains(&(actor.actor_id.clone(), actor.entity_uuid.clone()))
                    && (actor.rdps_contribution_given.unwrap_or_default() != 0
                        || actor.rdps_contribution_received.unwrap_or_default() != 0)
            }) {
                return Err(format!(
                    "replayed combat history run {} view {} introduced contributing actor {}",
                    run.run_index, view.id, actor.actor_id
                ));
            }
            view.damage_influences
                .clone_from(&projected_view.damage_influences);
        }
        run.rdps_status.clone_from(&projected.rdps_status);
    }
    Ok(refreshed)
}

fn validate_rdps_conservation(
    view: &rlogs_plugin_combat_meter::CombatHistoryView,
) -> Result<(), String> {
    let mut ordinary_damage = 0_i128;
    let mut rdps_damage = 0_i128;
    let mut contribution_given = 0_i128;
    let mut contribution_received = 0_i128;
    for actor in &view.actors {
        let (adjusted, given, received) = match (
            actor.rdps_damage,
            actor.rdps_contribution_given,
            actor.rdps_contribution_received,
        ) {
            (Some(adjusted), Some(given), Some(received)) => (adjusted, given, received),
            (None, None, None) if actor.damage == 0 => (0, 0, 0),
            _ => {
                return Err(format!(
                    "actor {} has an incomplete exact rDPS tuple",
                    actor.actor_id
                ));
            }
        };
        if given < 0 || received < 0 {
            return Err(format!(
                "actor {} has a negative contribution transfer",
                actor.actor_id
            ));
        }
        let expected = i128::from(actor.damage)
            .checked_add(i128::from(given))
            .and_then(|value| value.checked_sub(i128::from(received)))
            .ok_or_else(|| format!("actor {} rDPS arithmetic overflowed", actor.actor_id))?;
        if expected != i128::from(adjusted) {
            return Err(format!(
                "actor {} rDPS does not equal damage + given - received",
                actor.actor_id
            ));
        }
        ordinary_damage = ordinary_damage
            .checked_add(i128::from(actor.damage))
            .ok_or_else(|| "ordinary damage sum overflowed".to_string())?;
        rdps_damage = rdps_damage
            .checked_add(i128::from(adjusted))
            .ok_or_else(|| "rDPS damage sum overflowed".to_string())?;
        contribution_given = contribution_given
            .checked_add(i128::from(given))
            .ok_or_else(|| "contribution-given sum overflowed".to_string())?;
        contribution_received = contribution_received
            .checked_add(i128::from(received))
            .ok_or_else(|| "contribution-received sum overflowed".to_string())?;
    }
    if contribution_given != contribution_received || ordinary_damage != rdps_damage {
        return Err("rDPS transfer does not conserve the view's ordinary damage".into());
    }
    Ok(())
}

fn validate_session_id(session_id: &str) -> Result<(), String> {
    if session_id.is_empty()
        || session_id.len() > 128
        || !session_id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.'))
    {
        return Err("combat history session ID is invalid".into());
    }
    Ok(())
}

fn detail_file_name(session_id: &str) -> String {
    format!("{session_id}.combat-history.v1.json")
}

fn load_catalog(path: &Path) -> Result<CombatHistoryCatalog, String> {
    let metadata = match std::fs::metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Ok(CombatHistoryCatalog::default());
        }
        Err(error) => return Err(format!("could not inspect combat history index: {error}")),
    };
    if metadata.len() > MAXIMUM_HISTORY_INDEX_BYTES {
        return Err("combat history index exceeds its safety limit".into());
    }
    let catalog: CombatHistoryCatalog = serde_json::from_slice(
        &std::fs::read(path)
            .map_err(|error| format!("could not read combat history index: {error}"))?,
    )
    .map_err(|error| format!("combat history index is invalid: {error}"))?;
    if catalog.schema_version != CATALOG_SCHEMA_VERSION
        || catalog.entries.len() > MAXIMUM_HISTORY_ENTRIES
    {
        return Err("combat history index has an unsupported shape".into());
    }
    for entry in &catalog.entries {
        validate_session_id(&entry.session_id)?;
    }
    Ok(catalog)
}

fn write_json_atomic(
    path: &Path,
    value: &impl Serialize,
    maximum_bytes: u64,
) -> Result<(), String> {
    let mut encoded = serde_json::to_vec(value)
        .map_err(|error| format!("could not encode combat history: {error}"))?;
    encoded.push(b'\n');
    if encoded.len() as u64 > maximum_bytes {
        return Err(format!(
            "combat history artifact exceeds its {maximum_bytes}-byte safety limit"
        ));
    }
    let partial = path.with_extension("partial");
    match std::fs::remove_file(&partial) {
        Ok(()) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => return Err(format!("could not replace combat history partial: {error}")),
    }
    let file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&partial)
        .map_err(|error| format!("could not create combat history partial: {error}"))?;
    let write_result = (|| {
        let mut writer = BufWriter::new(file);
        writer.write_all(&encoded)?;
        writer.flush()?;
        writer.get_ref().sync_all()
    })();
    if let Err(error) = write_result {
        let _ = std::fs::remove_file(&partial);
        return Err(format!("could not persist combat history: {error}"));
    }
    let backup = path.with_extension("backup");
    let had_existing = path.exists();
    if had_existing {
        let _ = std::fs::remove_file(&backup);
        std::fs::rename(path, &backup)
            .map_err(|error| format!("could not stage prior combat history artifact: {error}"))?;
    }
    if let Err(error) = std::fs::rename(&partial, path) {
        if had_existing {
            let _ = std::fs::rename(&backup, path);
        }
        let _ = std::fs::remove_file(&partial);
        return Err(format!(
            "could not publish combat history artifact: {error}"
        ));
    }
    if had_existing {
        std::fs::remove_file(&backup)
            .map_err(|error| format!("could not remove combat history backup: {error}"))?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::time::{SystemTime, UNIX_EPOCH};

    use rlogs_plugin_combat_meter::{CombatHistoryView, CombatRunHistory};

    use super::*;

    fn fixture_actor(
        actor_id: &str,
        entity_uuid: &str,
        damage: i64,
    ) -> rlogs_plugin_combat_meter::HistoryActorSummary {
        serde_json::from_value(serde_json::json!({
            "actor_id": actor_id,
            "entity_uuid": entity_uuid,
            "monster_id": null,
            "character_id": null,
            "display_name": format!("captured-{actor_id}"),
            "actor_kind": "player",
            "presentation_name": format!("saved-{actor_id}"),
            "presentation_kind": null,
            "class_id": null,
            "specialization_id": null,
            "presentation_class_name": null,
            "presentation_specialization_name": null,
            "icon_asset_path": null,
            "presentation_role": null,
            "presentation_accent": null,
            "level": null,
            "damage": damage,
            "effective_damage": damage,
            "damage_taken": 0,
            "healing": 0,
            "effective_healing": 0,
            "shielding": 0,
            "hits": 1,
            "critical_hits": 0,
            "deaths": 0,
            "dps": damage as f64,
            "encounter_dps": damage as f64,
            "hps": 0.0,
            "tps": 0.0,
            "rdps": damage as f64,
            "rdps_damage": damage,
            "rdps_contribution_given": 0,
            "rdps_contribution_received": 0,
            "apm": null,
            "observed_cast_events": 0,
            "abilities": [],
            "targets": [],
            "effects": [],
            "series": []
        }))
        .unwrap()
    }

    fn fixture_snapshot(session_id: &str, run_indices: &[u32]) -> CombatHistorySnapshot {
        CombatHistorySnapshot {
            schema_version: COMBAT_HISTORY_SCHEMA_VERSION,
            session_id: session_id.into(),
            deployment_id: "global".into(),
            region_id: "global".into(),
            world_id: Some("asteria".into()),
            client_build: "fixture".into(),
            protocol_pack_digest: "fixture".into(),
            rdps_formula_identity: None,
            runs: run_indices
                .iter()
                .map(|run_index| CombatRunHistory {
                    run_index: *run_index,
                    activity_id: Some("scene.6525".into()),
                    activity_family_id: Some("mech-facility".into()),
                    scene_id: Some(6525),
                    presentation_scene_name: Some("Chaotic - Mech Facility".into()),
                    instance_id: Some(format!("fixture-instance-{run_index}")),
                    difficulty_family: Some("master".into()),
                    difficulty_tier: Some(5),
                    terminal_state: "completed".into(),
                    entered_micros: Some(10),
                    started_micros: 20,
                    first_combat_micros: Some(30),
                    ended_micros: Some(100),
                    load_time_micros: Some(10),
                    precombat_time_micros: Some(10),
                    total_run_time_micros: Some(90),
                    game_time_micros: Some(80),
                    true_time_micros: None,
                    retry_count: 0,
                    boss_retry_count: 0,
                    wipe_count: 0,
                    cleared_encounter_count: 1,
                    last_encounter_terminal_state: Some("cleared".into()),
                    rdps_status: "unavailable".into(),
                    apm_status: "unavailable".into(),
                    views: vec![CombatHistoryView {
                        id: "all".into(),
                        label: "Entire run".into(),
                        kind: "all".into(),
                        segment_indices: Vec::new(),
                        elapsed_micros: 80,
                        active_combat_micros: 70,
                        actors: Vec::new(),
                        targets: Vec::new(),
                        damage_influences: Vec::new(),
                    }],
                })
                .collect(),
        }
    }

    #[test]
    fn rejects_path_like_session_ids() {
        assert!(validate_session_id("monitor-123.run-0001").is_ok());
        assert!(validate_session_id("../history").is_err());
        assert!(validate_session_id("folder\\history").is_err());
    }

    #[test]
    fn rdps_refresh_preserves_saved_combat_and_capture_identity() {
        let mut saved = fixture_snapshot("monitor.formula-refresh", &[0]);
        saved.rdps_formula_identity = Some("sha256:old".into());
        saved.runs[0].views[0].actors =
            vec![fixture_actor("1", "101", 40), fixture_actor("2", "102", 60)];
        let saved_copy = saved.clone();

        let mut projection = saved.clone();
        projection.rdps_formula_identity = Some("sha256:new".into());
        projection.runs[0].rdps_status = "partial_packet_proven_rules".into();
        projection.runs[0].views[0].actors[0].presentation_name =
            Some("replayed-presentation-must-not-replace-saved".into());
        projection.runs[0].views[0].actors[0].rdps = Some(50.0);
        projection.runs[0].views[0].actors[0].rdps_damage = Some(50);
        projection.runs[0].views[0].actors[0].rdps_contribution_given = Some(10);
        projection.runs[0].views[0].actors[1].rdps = Some(50.0);
        projection.runs[0].views[0].actors[1].rdps_damage = Some(50);
        projection.runs[0].views[0].actors[1].rdps_contribution_received = Some(10);

        let refreshed = merge_rdps_projection(&saved, &projection).unwrap();
        assert_eq!(
            refreshed.rdps_formula_identity.as_deref(),
            Some("sha256:new")
        );
        assert_eq!(refreshed.runs[0].rdps_status, "partial_packet_proven_rules");
        assert_eq!(refreshed.runs[0].views[0].actors[0].damage, 40);
        assert_eq!(refreshed.runs[0].views[0].actors[0].rdps_damage, Some(50));
        assert_eq!(
            refreshed.runs[0].views[0].actors[0].presentation_name,
            saved_copy.runs[0].views[0].actors[0].presentation_name
        );
        let mut without_rdps = refreshed.clone();
        without_rdps.rdps_formula_identity = saved_copy.rdps_formula_identity.clone();
        without_rdps.runs[0].rdps_status = saved_copy.runs[0].rdps_status.clone();
        for (actual, expected) in without_rdps.runs[0].views[0]
            .actors
            .iter_mut()
            .zip(&saved_copy.runs[0].views[0].actors)
        {
            actual.rdps = expected.rdps;
            actual.rdps_damage = expected.rdps_damage;
            actual.rdps_contribution_given = expected.rdps_contribution_given;
            actual.rdps_contribution_received = expected.rdps_contribution_received;
        }
        assert_eq!(without_rdps, saved_copy);
    }

    #[test]
    fn rdps_refresh_rejects_nonconserving_or_changed_damage() {
        let mut saved = fixture_snapshot("monitor.formula-reject", &[0]);
        saved.runs[0].views[0].actors =
            vec![fixture_actor("1", "101", 40), fixture_actor("2", "102", 60)];
        let mut projection = saved.clone();
        projection.rdps_formula_identity = Some("sha256:new".into());
        projection.runs[0].views[0].actors[0].rdps_damage = Some(50);
        projection.runs[0].views[0].actors[0].rdps_contribution_given = Some(10);
        projection.runs[0].views[0].actors[1].rdps_damage = Some(51);
        projection.runs[0].views[0].actors[1].rdps_contribution_received = Some(9);
        assert!(merge_rdps_projection(&saved, &projection).is_err());

        projection.runs[0].views[0].actors[1].rdps_damage = Some(50);
        projection.runs[0].views[0].actors[1].rdps_contribution_received = Some(10);
        projection.runs[0].views[0].actors[1].damage = 61;
        projection.runs[0].views[0].actors[1].rdps_damage = Some(51);
        assert!(merge_rdps_projection(&saved, &projection).is_err());
    }

    #[test]
    fn records_exited_attempts_in_local_history() {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!(
            "rlogs-combat-history-exited-{}-{unique}",
            std::process::id()
        ));
        let mut store = CombatHistoryStore::open(root.clone()).unwrap();
        let snapshot = CombatHistorySnapshot {
            schema_version: COMBAT_HISTORY_SCHEMA_VERSION,
            session_id: "monitor.run-exited".into(),
            deployment_id: "global".into(),
            region_id: "global".into(),
            world_id: Some("asteria".into()),
            client_build: "fixture".into(),
            protocol_pack_digest: "fixture".into(),
            rdps_formula_identity: None,
            runs: vec![CombatRunHistory {
                run_index: 0,
                activity_id: Some("scene.6525".into()),
                activity_family_id: Some("mech-facility".into()),
                scene_id: Some(6525),
                presentation_scene_name: Some("Chaotic - Mech Facility".into()),
                instance_id: Some("fixture-instance".into()),
                difficulty_family: Some("master".into()),
                difficulty_tier: Some(5),
                terminal_state: "exited".into(),
                entered_micros: Some(10),
                started_micros: 20,
                first_combat_micros: Some(30),
                ended_micros: Some(100),
                load_time_micros: Some(10),
                precombat_time_micros: Some(10),
                total_run_time_micros: Some(90),
                game_time_micros: Some(80),
                true_time_micros: None,
                retry_count: 0,
                boss_retry_count: 0,
                wipe_count: 1,
                cleared_encounter_count: 0,
                last_encounter_terminal_state: Some("wiped".into()),
                rdps_status: "unavailable".into(),
                apm_status: "unavailable".into(),
                views: vec![CombatHistoryView {
                    id: "all".into(),
                    label: "Entire run".into(),
                    kind: "all".into(),
                    segment_indices: Vec::new(),
                    elapsed_micros: 80,
                    active_combat_micros: 70,
                    actors: Vec::new(),
                    targets: Vec::new(),
                    damage_influences: Vec::new(),
                }],
            }],
        };

        let catalog = store.record(&snapshot, 123).unwrap();
        assert_eq!(catalog.entries.len(), 1);
        assert_eq!(catalog.entries[0].terminal_state, "exited");
        assert_eq!(
            catalog.entries[0].activity_family_id.as_deref(),
            Some("mech-facility")
        );
        assert_eq!(catalog.entries[0].difficulty_tier, Some(5));
        assert!(
            root.join("monitor.run-exited.combat-history.v1.json")
                .is_file()
        );

        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn favorites_survive_reprojection_and_reload() {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!(
            "rlogs-combat-history-favorite-{}-{unique}",
            std::process::id()
        ));
        let snapshot = fixture_snapshot("monitor.favorite", &[0]);
        let mut store = CombatHistoryStore::open(root.clone()).unwrap();
        store.record(&snapshot, 123).unwrap();
        store.set_favorite("monitor.favorite:0", true).unwrap();
        store.record(&snapshot, 124).unwrap();
        drop(store);

        let reopened = CombatHistoryStore::open(root.clone()).unwrap();
        assert!(reopened.catalog().entries[0].is_favorite);

        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn bulk_delete_preserves_favorites_and_prunes_selected_runs() {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!(
            "rlogs-combat-history-delete-{}-{unique}",
            std::process::id()
        ));
        let mut store = CombatHistoryStore::open(root.clone()).unwrap();
        store
            .record(&fixture_snapshot("monitor.favorite", &[0]), 123)
            .unwrap();
        store
            .record(&fixture_snapshot("monitor.multiple", &[0, 1]), 124)
            .unwrap();
        store.set_favorite("monitor.favorite:0", true).unwrap();

        let result = store
            .delete_entries(&["monitor.favorite:0".into(), "monitor.multiple:0".into()])
            .unwrap();
        assert_eq!(result.deleted_count, 1);
        assert_eq!(result.preserved_favorite_count, 1);
        assert!(result.cleanup_warnings.is_empty());
        assert_eq!(store.catalog().entries.len(), 2);
        assert!(
            store
                .catalog()
                .entries
                .iter()
                .any(|entry| entry.history_id == "monitor.favorite:0" && entry.is_favorite)
        );
        assert!(
            store
                .catalog()
                .entries
                .iter()
                .any(|entry| entry.history_id == "monitor.multiple:1")
        );
        let detail = store.detail("monitor.multiple").unwrap();
        assert_eq!(
            detail
                .runs
                .iter()
                .map(|run| run.run_index)
                .collect::<Vec<_>>(),
            vec![1]
        );

        std::fs::remove_dir_all(root).unwrap();
    }
}
