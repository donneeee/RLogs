use std::{
    collections::{BTreeMap, BTreeSet},
    ffi::{OsStr, OsString},
    fs::File,
    io::{BufReader, BufWriter, Write},
    path::PathBuf,
};

use rlogs_events::{CanonicalEvent, EvidenceSource, RegionIdentity, WorldContext};
use rlogs_game_bpsr::{
    AllowedDataDomain, CaptureRecordKind, CharacterProfilePatch, DecoderKind, FragmentKind,
    JsonlJournalReader, MappingConfidence, PacketDirection, ProtocolDecodeStatus, ProtocolPack,
    ProtocolPackRouteDisposition, ProtocolRuntime, ProtocolRuntimeConfig, RouteKey,
};
use serde::Serialize;

const PROFILE_ROUTE: RouteKey = RouteKey::new(
    PacketDirection::ServerToClient,
    FragmentKind::Notify,
    1_664_308_034,
    21,
);

fn main() {
    if let Err(error) = run() {
        eprintln!("profile audit failed: {error}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), Box<dyn std::error::Error>> {
    let arguments = arguments()?;
    let pack = ProtocolPack::from_json(&std::fs::read(&arguments.pack)?)?;
    let mut candidate_definition = pack.definition().clone();
    let candidate_route = candidate_definition
        .routes
        .iter_mut()
        .find(|route| route.route == PROFILE_ROUTE)
        .ok_or("protocol pack has no WorldNtf/SyncContainerData route")?;
    match candidate_route.disposition {
        ProtocolPackRouteDisposition::Opaque => {
            candidate_route.disposition = ProtocolPackRouteDisposition::Allowed {
                domain: AllowedDataDomain::CharacterProfile,
                decoder: DecoderKind::SyncContainerDataV1,
            };
        }
        ProtocolPackRouteDisposition::Allowed {
            domain: AllowedDataDomain::CharacterProfile,
            decoder: DecoderKind::SyncContainerDataV1,
        } => {}
        _ => {
            return Err("profile route has an incompatible or prohibited disposition".into());
        }
    }
    candidate_route.confidence = MappingConfidence::Verified;
    let candidate_pack = ProtocolPack::build(candidate_definition)?;

    let journal =
        JsonlJournalReader::new(BufReader::new(File::open(&arguments.journal)?)).read()?;
    let build = &journal.session().game_build;
    let region = RegionIdentity {
        deployment_id: build.deployment_id.clone(),
        region_id: build
            .region_id
            .clone()
            .unwrap_or_else(|| "audit-unresolved".into()),
        realm_id: None,
        world_id: None,
    };
    let mut runtime = ProtocolRuntime::new(
        &candidate_pack,
        "private-profile-audit",
        build,
        region,
        Vec::new(),
        ProtocolRuntimeConfig::default(),
    )?;
    let mut summary = ProfileAuditSummary {
        schema_version: 1,
        generated_by: "rlogs-profile-audit",
        game_build: build.build_id.clone(),
        capture_id: journal.session().capture_id.clone(),
        source_protocol_pack_digest: journal.session().protocol_pack_digest.clone(),
        audit_protocol_pack_digest: candidate_pack.digest().to_owned(),
        source_record_count: journal.records().len(),
        source_journal: arguments.journal.display().to_string(),
        target_talent_node_id: arguments.target_talent_node,
        target_talent_node_selected: arguments.target_talent_node.map(|_| false),
        target_profession_id: arguments.target_profession,
        target_character_id: arguments.target_character_id.clone(),
        target_character_id_matched: arguments.target_character_id.as_ref().map(|_| false),
        ..ProfileAuditSummary::default()
    };
    let mut structure = None;

    for record in journal.records() {
        let CaptureRecordKind::Packet(packet) = &record.kind else {
            continue;
        };
        if packet.route.map(|route| route.key) != Some(PROFILE_ROUTE) {
            continue;
        }
        summary.candidate_packets = summary.candidate_packets.saturating_add(1);
        if structure.is_none() {
            let payload = packet
                .payload
                .decode_input()
                .ok_or("profile packet has no application payload")?;
            structure = Some(structural_audit(payload)?);
        }

        let batch = runtime.process(record)?;
        match batch.status {
            ProtocolDecodeStatus::Decoded => {
                summary.decoded_packets = summary.decoded_packets.saturating_add(1)
            }
            ProtocolDecodeStatus::DecodeFailed => {
                summary.decode_failed_packets = summary.decode_failed_packets.saturating_add(1)
            }
            status => {
                return Err(format!(
                    "candidate profile packet returned unexpected status {status:?}"
                )
                .into());
            }
        }
        for event in batch.events {
            let event_sequence = event.sequence;
            let observed_micros = event.time.observed_micros;
            let capture_sequence = match &event.provenance.source {
                EvidenceSource::Wire {
                    capture_sequence, ..
                } => Some(*capture_sequence),
                _ => None,
            };
            match event.event {
                CanonicalEvent::CharacterProfileObserved { profile } => {
                    summary.profile_events = summary.profile_events.saturating_add(1);
                    let profile = CharacterProfilePatch::from_game_event(&profile)?;
                    if let (Some(target), Some(matched)) = (
                        summary.target_character_id.as_deref(),
                        summary.target_character_id_matched.as_mut(),
                    ) {
                        *matched |= profile.character.character_id == target;
                    }
                    if let (Some(target), Some(selected)) = (
                        summary.target_talent_node_id,
                        summary.target_talent_node_selected.as_mut(),
                    ) {
                        *selected |=
                            profile
                                .combat_professions
                                .as_ref()
                                .is_some_and(|professions| {
                                    professions.iter().any(|profession| {
                                        profession.talent_node_ids.contains(&target)
                                    })
                                });
                    }
                    if let Some(target_profession) = summary.target_profession_id {
                        if let Some(profession) =
                            profile.combat_professions.as_ref().and_then(|professions| {
                                professions.iter().find(|profession| {
                                    profession.profession_id == target_profession
                                })
                            })
                        {
                            summary
                                .target_profession_talent_node_ids
                                .extend(profession.talent_node_ids.iter().copied());
                        }
                    }
                    summary.profile_observations.push(profile_observation(
                        event_sequence,
                        capture_sequence,
                        observed_micros,
                        &profile,
                    ));
                    summary.profile_fields.observe(&profile);
                }
                CanonicalEvent::WorldChanged(context) => {
                    summary.world_events = summary.world_events.saturating_add(1);
                    summary.world_fields.observe(&context);
                }
                _ => {}
            }
        }
    }

    summary.structure = structure.ok_or("journal contains no profile snapshot packet")?;
    summary.privacy = PrivacyAudit {
        decoder_declares_account_id: false,
        decoder_declares_open_id: false,
        raw_values_rendered: false,
    };
    if let Some(output) = arguments.output {
        let mut writer = BufWriter::new(File::create(output)?);
        serde_json::to_writer_pretty(&mut writer, &summary)?;
        writer.write_all(b"\n")?;
    } else {
        serde_json::to_writer_pretty(std::io::stdout().lock(), &summary)?;
        println!();
    }
    Ok(())
}

#[derive(Debug, Default, Serialize)]
struct ProfileAuditSummary {
    schema_version: u32,
    generated_by: &'static str,
    game_build: String,
    capture_id: String,
    source_protocol_pack_digest: Option<String>,
    audit_protocol_pack_digest: String,
    source_record_count: usize,
    source_journal: String,
    candidate_packets: u64,
    decoded_packets: u64,
    decode_failed_packets: u64,
    profile_events: u64,
    profile_observations: Vec<ProfileEventObservation>,
    world_events: u64,
    target_talent_node_id: Option<i64>,
    target_talent_node_selected: Option<bool>,
    target_profession_id: Option<i32>,
    target_profession_talent_node_ids: BTreeSet<i64>,
    target_character_id: Option<String>,
    target_character_id_matched: Option<bool>,
    profile_fields: ProfileFieldPresence,
    world_fields: WorldFieldPresence,
    structure: StructuralAudit,
    privacy: PrivacyAudit,
}

#[derive(Debug, Default, Serialize)]
struct ProfileFieldPresence {
    character_uid: bool,
    display_name: bool,
    display_uid: bool,
    server_id: bool,
    class_id: bool,
    specialization_id: bool,
    level: bool,
    progression: bool,
    combat_power: bool,
    season_strength: bool,
    appearance: bool,
    face_option_count: usize,
    color_option_count: usize,
    unlocked_profile_image_count: usize,
    unlocked_face_item_count: usize,
    unlocked_voice_count: usize,
    equipment: bool,
    equipment_count: usize,
    equipment_with_attributes: usize,
    equipment_with_enchantments: usize,
    equipment_items: BTreeSet<EquipmentItemAudit>,
    equipment_suit_entry_count: usize,
    equipment_suit_entries: BTreeSet<EquipmentSuitEntryAudit>,
    modules: bool,
    equipped_module_slot_count: usize,
    module_inventory_count: usize,
    module_part_count: usize,
    module_upgrade_record_count: usize,
    modules_with_initial_link_points: usize,
    module_link_point_audit: ModuleLinkPointAudit,
    equipped_modules: BTreeSet<EquippedModuleAudit>,
    combat_power_component_count: usize,
    combat_power_subcomponent_count: usize,
    season_profile: bool,
    season_experience: bool,
    owned_imagine_count: usize,
    equipped_owned_imagine_count: usize,
    battle_imagine_skill_count: usize,
    equipped_battle_imagine_skill_count: usize,
    active_skill_count: usize,
    talent_count: usize,
    talent_progress: bool,
    total_talent_points: bool,
    total_talent_reset_count: bool,
    profession_talent_loadout_count: usize,
    selected_talent_node_count: usize,
    talent_loadouts_with_used_points: usize,
    talent_loadouts_with_stage_config: usize,
    combat_profession_count: usize,
    life_profession_count: usize,
    cosmetic_count: usize,
    collection_summary: bool,
    equipped_fashion_count: usize,
    owned_fashion_count: usize,
    owned_mount_count: usize,
    owned_weapon_skin_count: usize,
    owned_dye_count: usize,
    unlocked_module_count: usize,
    ride_count: usize,
    ride_skin_count: usize,
    unlocked_emoji_count: usize,
    vanity_pet_count: usize,
    summoned_vanity_pet: bool,
    fantasy_atlas_stage_count: usize,
    handbook: bool,
    handbook_entry_count: usize,
    activity_progress: bool,
    challenge_dungeon_count: usize,
    challenge_target_count: usize,
    master_mode_dungeon_count: usize,
    weekly_tower: bool,
    season_medals: bool,
    season_medal_hole_count: usize,
    season_medal_node_count: usize,
    current_season_ids: BTreeSet<i64>,
    season_cultivation_count: usize,
    cultivation_line_count: usize,
    cultivation_area_count: usize,
    current_active_cultivation_area_count: usize,
    current_active_cultivation_areas: BTreeSet<CultivationAreaAudit>,
    current_active_middle_node_item_ids: BTreeSet<i64>,
    current_active_big_node_fantasy_ids: BTreeSet<i64>,
    reputation_count: usize,
    current_profession_project: bool,
    social_display: bool,
    guild_id: bool,
    guild_name: bool,
    title_count: usize,
    medal_count: usize,
    medal_slot_count: usize,
    profile_theme: bool,
}

impl ProfileFieldPresence {
    fn observe(&mut self, profile: &CharacterProfilePatch) {
        self.character_uid = !profile.character.character_id.is_empty();
        self.display_name |= profile.display_name.is_some();
        self.display_uid |= profile.display_id.is_some();
        self.server_id |= profile.server_id.is_some();
        self.class_id |= profile.class_id.is_some();
        self.specialization_id |= profile.specialization_id.is_some();
        self.level |= profile.level.is_some();
        self.progression |= profile.progression.is_some();
        self.combat_power |= profile.combat_power.is_some();
        self.season_strength |= profile.season_strength.is_some();
        self.appearance |= profile.appearance.is_some();
        if let Some(appearance) = &profile.appearance {
            self.face_option_count = self.face_option_count.max(appearance.face_options.len());
            self.color_option_count = self.color_option_count.max(appearance.color_options.len());
            self.unlocked_profile_image_count = self
                .unlocked_profile_image_count
                .max(appearance.unlocked_profile_image_ids.len());
            self.unlocked_face_item_count = self
                .unlocked_face_item_count
                .max(appearance.unlocked_face_item_ids.len());
            self.unlocked_voice_count = self
                .unlocked_voice_count
                .max(appearance.unlocked_voice_ids.len());
        }
        self.equipment |= profile.equipment.is_some();
        if let Some(equipment) = &profile.equipment {
            self.equipment_count = self.equipment_count.max(equipment.len());
            self.equipment_items
                .extend(equipment.iter().map(|item| EquipmentItemAudit {
                    slot_id: item.slot_id,
                    item_id: item.item_id,
                }));
            self.equipment_with_attributes = self.equipment_with_attributes.max(
                equipment
                    .iter()
                    .filter(|item| item.attributes.is_some())
                    .count(),
            );
            self.equipment_with_enchantments = self.equipment_with_enchantments.max(
                equipment
                    .iter()
                    .filter(|item| !item.enchantments.is_empty())
                    .count(),
            );
        }
        if let Some(entries) = &profile.equipment_suit_entries {
            self.equipment_suit_entry_count = self.equipment_suit_entry_count.max(entries.len());
            self.equipment_suit_entries
                .extend(entries.iter().map(|entry| {
                    EquipmentSuitEntryAudit {
                        map_key: entry.map_key,
                        attribute_type: entry.attribute_type,
                        attributes: entry
                            .attributes
                            .iter()
                            .map(|(attribute_id, value)| (*attribute_id, *value))
                            .collect(),
                    }
                }));
        }
        self.modules |= profile.modules.is_some();
        if let Some(modules) = &profile.modules {
            self.equipped_module_slot_count = self
                .equipped_module_slot_count
                .max(modules.equipped_slots.len());
            self.module_inventory_count = self.module_inventory_count.max(modules.inventory.len());
            self.module_part_count = self.module_part_count.max(
                modules
                    .inventory
                    .iter()
                    .map(|module| module.parts.len())
                    .sum(),
            );
            self.module_upgrade_record_count = self.module_upgrade_record_count.max(
                modules
                    .inventory
                    .iter()
                    .map(|module| module.upgrade_records.len())
                    .sum(),
            );
            self.modules_with_initial_link_points = self.modules_with_initial_link_points.max(
                modules
                    .inventory
                    .iter()
                    .filter(|module| {
                        module
                            .parts
                            .iter()
                            .any(|part| part.initial_link_points.is_some())
                    })
                    .count(),
            );
            let module_link_point_audit = ModuleLinkPointAudit::from_modules(modules);
            if module_link_point_audit.part_records >= self.module_link_point_audit.part_records {
                self.module_link_point_audit = module_link_point_audit;
            }
            for (slot_id, instance_id) in &modules.equipped_slots {
                let Some(module) = modules
                    .inventory
                    .iter()
                    .find(|module| &module.instance_id == instance_id)
                else {
                    continue;
                };
                let parts = module
                    .parts
                    .iter()
                    .map(|part| {
                        let matching_upgrades = module
                            .upgrade_records
                            .iter()
                            .filter(|upgrade| upgrade.part_id == part.part_id);
                        ModulePartAudit {
                            part_id: part.part_id,
                            initial_link_points: part.initial_link_points,
                            successful_upgrades: matching_upgrades
                                .clone()
                                .filter(|upgrade| upgrade.succeeded == Some(true))
                                .count(),
                            failed_upgrades: matching_upgrades
                                .filter(|upgrade| upgrade.succeeded == Some(false))
                                .count(),
                        }
                    })
                    .collect();
                self.equipped_modules.insert(EquippedModuleAudit {
                    slot_id: *slot_id,
                    config_id: module.config_id,
                    module_type: module.module_type,
                    level: module.level,
                    quality: module.quality,
                    load_flag: module.load_flag,
                    success_rate: module.success_rate,
                    parts,
                });
            }
        }
        if let Some(power) = &profile.combat_power_breakdown {
            self.combat_power_component_count = self
                .combat_power_component_count
                .max(power.components.len());
            self.combat_power_subcomponent_count = self.combat_power_subcomponent_count.max(
                power
                    .components
                    .iter()
                    .map(|component| component.subcomponents.len())
                    .sum(),
            );
        }
        self.season_profile |= profile.season.is_some();
        let current_season_id = profile
            .season
            .as_ref()
            .and_then(|season| season.season_id)
            .filter(|season_id| *season_id > 0);
        if let Some(season_id) = current_season_id {
            self.current_season_ids.insert(season_id);
        }
        self.season_experience |= profile
            .season
            .as_ref()
            .and_then(|season| season.experience)
            .is_some();
        if let Some(imagines) = &profile.owned_imagines {
            self.owned_imagine_count = self.owned_imagine_count.max(imagines.len());
            self.equipped_owned_imagine_count = self.equipped_owned_imagine_count.max(
                imagines
                    .iter()
                    .filter(|imagine| imagine.equipped_slot.is_some())
                    .count(),
            );
        }
        if let Some(skills) = &profile.battle_imagine_skills {
            self.battle_imagine_skill_count = self.battle_imagine_skill_count.max(skills.len());
            self.equipped_battle_imagine_skill_count =
                self.equipped_battle_imagine_skill_count.max(
                    skills
                        .iter()
                        .filter(|skill| skill.equipped_slot.is_some())
                        .count(),
                );
        }
        self.active_skill_count = self
            .active_skill_count
            .max(profile.active_skills.as_ref().map_or(0, Vec::len));
        self.talent_count = self
            .talent_count
            .max(profile.talents.as_ref().map_or(0, Vec::len));
        self.talent_progress |= profile.talent_progress.is_some();
        if let Some(progress) = &profile.talent_progress {
            self.total_talent_points |= progress.total_points.is_some();
            self.total_talent_reset_count |= progress.total_reset_count.is_some();
        }
        self.combat_profession_count = self
            .combat_profession_count
            .max(profile.combat_professions.as_ref().map_or(0, Vec::len));
        if let Some(professions) = &profile.combat_professions {
            self.profession_talent_loadout_count = self.profession_talent_loadout_count.max(
                professions
                    .iter()
                    .filter(|profession| {
                        !profession.talent_node_ids.is_empty()
                            || profession.talent_points_used.is_some()
                            || profession.talent_stage_config_id.is_some()
                    })
                    .count(),
            );
            self.selected_talent_node_count = self.selected_talent_node_count.max(
                professions
                    .iter()
                    .map(|profession| profession.talent_node_ids.len())
                    .sum(),
            );
            self.talent_loadouts_with_used_points = self.talent_loadouts_with_used_points.max(
                professions
                    .iter()
                    .filter(|profession| profession.talent_points_used.is_some())
                    .count(),
            );
            self.talent_loadouts_with_stage_config = self.talent_loadouts_with_stage_config.max(
                professions
                    .iter()
                    .filter(|profession| profession.talent_stage_config_id.is_some())
                    .count(),
            );
        }
        self.life_profession_count = self
            .life_profession_count
            .max(profile.life_professions.as_ref().map_or(0, Vec::len));
        self.cosmetic_count = self
            .cosmetic_count
            .max(profile.cosmetics.as_ref().map_or(0, Vec::len));
        self.collection_summary |= profile.collection_summary.is_some();
        if let Some(collection) = &profile.collection_summary {
            self.equipped_fashion_count = self
                .equipped_fashion_count
                .max(collection.equipped_fashion_ids.len());
            self.owned_fashion_count = self
                .owned_fashion_count
                .max(collection.owned_fashion_ids.len());
            self.owned_mount_count = self.owned_mount_count.max(collection.owned_mount_ids.len());
            self.owned_weapon_skin_count = self
                .owned_weapon_skin_count
                .max(collection.owned_weapon_skin_ids.len());
            self.owned_dye_count = self.owned_dye_count.max(collection.owned_dye_ids.len());
            self.unlocked_module_count = self
                .unlocked_module_count
                .max(collection.unlocked_module_ids.len());
            self.ride_count = self.ride_count.max(collection.ride_ids.len());
            self.ride_skin_count = self.ride_skin_count.max(collection.ride_skin_ids.len());
            self.unlocked_emoji_count = self
                .unlocked_emoji_count
                .max(collection.unlocked_emoji_ids.len());
            self.vanity_pet_count = self.vanity_pet_count.max(collection.vanity_pet_ids.len());
            self.summoned_vanity_pet |= collection.summoned_vanity_pet_id.is_some();
            self.fantasy_atlas_stage_count = self
                .fantasy_atlas_stage_count
                .max(collection.fantasy_atlas_stages.len());
            self.handbook |= collection.handbook.is_some();
            if let Some(handbook) = &collection.handbook {
                self.handbook_entry_count = self.handbook_entry_count.max(
                    handbook.important_people_ids.len()
                        + handbook.reading_book_ids.len()
                        + handbook.dictionary_entry_ids.len()
                        + handbook.postcard_ids.len()
                        + handbook.monthly_card_ids.len(),
                );
            }
        }
        self.activity_progress |= profile.activity_progress.is_some();
        if let Some(activity) = &profile.activity_progress {
            self.challenge_dungeon_count = self
                .challenge_dungeon_count
                .max(activity.challenge_dungeons.len());
            self.challenge_target_count = self
                .challenge_target_count
                .max(activity.challenge_targets.len());
            self.master_mode_dungeon_count = self
                .master_mode_dungeon_count
                .max(activity.master_mode_dungeons.len());
            self.weekly_tower |= activity.weekly_tower.is_some();
        }
        self.season_medals |= profile.season_medals.is_some();
        if let Some(medals) = &profile.season_medals {
            self.season_medal_hole_count = self
                .season_medal_hole_count
                .max(medals.normal_holes.len() + usize::from(medals.core_hole.is_some()));
            self.season_medal_node_count =
                self.season_medal_node_count.max(medals.core_nodes.len());
        }
        if let Some(seasons) = &profile.season_cultivation {
            self.season_cultivation_count = self.season_cultivation_count.max(seasons.len());
            self.cultivation_line_count = self.cultivation_line_count.max(
                seasons
                    .iter()
                    .map(|season| season.lines.len())
                    .sum::<usize>(),
            );
            self.cultivation_area_count = self.cultivation_area_count.max(
                seasons
                    .iter()
                    .flat_map(|season| &season.lines)
                    .map(|line| line.areas.len())
                    .sum::<usize>(),
            );
            let mut active_area_count = 0;
            for season in seasons {
                if current_season_id != Some(i64::from(season.season_id)) {
                    continue;
                }
                for line in &season.lines {
                    for area in &line.areas {
                        if area.active != Some(true) || !line.area_ids.contains(&area.area_id) {
                            continue;
                        }
                        active_area_count += 1;
                        self.current_active_cultivation_areas
                            .insert(CultivationAreaAudit {
                                season_id: season.season_id,
                                line_type_id: line.line_type_id,
                                area_id: area.area_id,
                                active_effect_score: area.active_effect_score,
                            });
                        self.current_active_middle_node_item_ids.extend(
                            area.middle_node_item_ids
                                .values()
                                .copied()
                                .filter(|item_id| *item_id > 0),
                        );
                        self.current_active_big_node_fantasy_ids.extend(
                            area.big_node_fantasy_ids
                                .values()
                                .copied()
                                .filter(|fantasy_id| *fantasy_id > 0),
                        );
                    }
                }
            }
            self.current_active_cultivation_area_count = self
                .current_active_cultivation_area_count
                .max(active_area_count);
        }
        self.reputation_count = self
            .reputation_count
            .max(profile.reputations.as_ref().map_or(0, Vec::len));
        self.current_profession_project |= profile.current_profession_project_id.is_some();
        self.social_display |= profile.social_display.is_some();
        if let Some(social) = &profile.social_display {
            self.guild_id |= social.guild_id.is_some();
            self.guild_name |= social.guild_name.is_some();
            self.title_count = self.title_count.max(social.title_ids.len());
            self.medal_count = self.medal_count.max(social.medal_ids.len());
            self.medal_slot_count = self.medal_slot_count.max(social.medal_slots.len());
            self.profile_theme |= social.profile_theme_id.is_some();
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize)]
struct EquipmentItemAudit {
    slot_id: i32,
    item_id: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize)]
struct EquipmentSuitEntryAudit {
    map_key: i32,
    attribute_type: Option<i32>,
    attributes: Vec<(i32, i32)>,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize)]
struct EquippedModuleAudit {
    slot_id: i32,
    config_id: i32,
    module_type: Option<i32>,
    level: Option<u32>,
    quality: Option<i32>,
    load_flag: Option<i32>,
    success_rate: Option<i32>,
    parts: Vec<ModulePartAudit>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
struct ModulePartAudit {
    part_id: i32,
    initial_link_points: Option<i32>,
    successful_upgrades: usize,
    failed_upgrades: usize,
}

#[derive(Debug, Default, Serialize)]
struct ModuleLinkPointAudit {
    part_records: usize,
    parts_with_initial_link_points: usize,
    parts_without_initial_link_points: usize,
    parts_initial_equals_successes: usize,
    parts_initial_greater_than_successes: usize,
    parts_initial_less_than_successes: usize,
    successful_upgrade_records: usize,
    failed_upgrade_records: usize,
    initial_link_points_min: Option<i32>,
    initial_link_points_max: Option<i32>,
    successful_upgrades_per_part_min: Option<usize>,
    successful_upgrades_per_part_max: Option<usize>,
    initial_success_failure_distribution: BTreeMap<String, usize>,
}

impl ModuleLinkPointAudit {
    fn from_modules(modules: &rlogs_game_bpsr::ModuleProfile) -> Self {
        let mut audit = Self::default();
        for module in &modules.inventory {
            for part in &module.parts {
                audit.part_records = audit.part_records.saturating_add(1);
                let successful_upgrades = module
                    .upgrade_records
                    .iter()
                    .filter(|upgrade| {
                        upgrade.part_id == part.part_id && upgrade.succeeded == Some(true)
                    })
                    .count();
                let failed_upgrades = module
                    .upgrade_records
                    .iter()
                    .filter(|upgrade| {
                        upgrade.part_id == part.part_id && upgrade.succeeded == Some(false)
                    })
                    .count();
                audit.successful_upgrade_records = audit
                    .successful_upgrade_records
                    .saturating_add(successful_upgrades);
                audit.failed_upgrade_records =
                    audit.failed_upgrade_records.saturating_add(failed_upgrades);
                audit.successful_upgrades_per_part_min = Some(
                    audit
                        .successful_upgrades_per_part_min
                        .map_or(successful_upgrades, |value| value.min(successful_upgrades)),
                );
                audit.successful_upgrades_per_part_max = Some(
                    audit
                        .successful_upgrades_per_part_max
                        .map_or(successful_upgrades, |value| value.max(successful_upgrades)),
                );

                let Some(initial_link_points) = part.initial_link_points else {
                    audit.parts_without_initial_link_points =
                        audit.parts_without_initial_link_points.saturating_add(1);
                    let key = format!("missing:{successful_upgrades}:{failed_upgrades}");
                    *audit
                        .initial_success_failure_distribution
                        .entry(key)
                        .or_default() += 1;
                    continue;
                };
                audit.parts_with_initial_link_points =
                    audit.parts_with_initial_link_points.saturating_add(1);
                audit.initial_link_points_min = Some(
                    audit
                        .initial_link_points_min
                        .map_or(initial_link_points, |value| value.min(initial_link_points)),
                );
                audit.initial_link_points_max = Some(
                    audit
                        .initial_link_points_max
                        .map_or(initial_link_points, |value| value.max(initial_link_points)),
                );
                match i64::from(initial_link_points).cmp(&(successful_upgrades as i64)) {
                    std::cmp::Ordering::Equal => {
                        audit.parts_initial_equals_successes =
                            audit.parts_initial_equals_successes.saturating_add(1)
                    }
                    std::cmp::Ordering::Greater => {
                        audit.parts_initial_greater_than_successes =
                            audit.parts_initial_greater_than_successes.saturating_add(1)
                    }
                    std::cmp::Ordering::Less => {
                        audit.parts_initial_less_than_successes =
                            audit.parts_initial_less_than_successes.saturating_add(1)
                    }
                }
                let key = format!("{initial_link_points}:{successful_upgrades}:{failed_upgrades}");
                *audit
                    .initial_success_failure_distribution
                    .entry(key)
                    .or_default() += 1;
            }
        }
        audit
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize)]
struct CultivationAreaAudit {
    season_id: i32,
    line_type_id: i32,
    area_id: i32,
    active_effect_score: Option<i32>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
struct ProfileEventObservation {
    event_sequence: u64,
    capture_sequence: Option<u64>,
    observed_micros: u64,
    character_id: String,
    current_season_id: Option<i64>,
    battle_imagine_skills: Vec<BattleImagineSkillAudit>,
    fantasy_atlas_stages: BTreeMap<i64, u32>,
    active_cultivation_areas: Vec<CultivationAreaSelectionAudit>,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize)]
struct BattleImagineSkillAudit {
    skill_id: i64,
    base_skill_id: Option<i64>,
    level: Option<u32>,
    remodel_level: Option<u32>,
    equipped_slot: Option<i32>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
struct CultivationAreaSelectionAudit {
    season_id: i32,
    line_type_id: i32,
    area_id: i32,
    active_effect_score: Option<i32>,
    middle_node_item_ids: BTreeMap<i32, i64>,
    big_node_fantasy_ids: BTreeMap<i32, i64>,
}

fn profile_observation(
    event_sequence: u64,
    capture_sequence: Option<u64>,
    observed_micros: u64,
    profile: &CharacterProfilePatch,
) -> ProfileEventObservation {
    let current_season_id = profile.season.as_ref().and_then(|season| season.season_id);
    let mut battle_imagine_skills = profile
        .battle_imagine_skills
        .iter()
        .flatten()
        .map(|skill| BattleImagineSkillAudit {
            skill_id: skill.skill_id,
            base_skill_id: skill.base_skill_id,
            level: skill.level,
            remodel_level: skill.remodel_level,
            equipped_slot: skill.equipped_slot,
        })
        .collect::<Vec<_>>();
    battle_imagine_skills.sort();
    let fantasy_atlas_stages = profile
        .collection_summary
        .as_ref()
        .map(|collection| collection.fantasy_atlas_stages.clone())
        .unwrap_or_default();
    let mut active_cultivation_areas = profile
        .season_cultivation
        .iter()
        .flatten()
        .filter(|season| current_season_id == Some(i64::from(season.season_id)))
        .flat_map(|season| {
            season.lines.iter().flat_map(move |line| {
                line.areas.iter().filter_map(move |area| {
                    (area.active == Some(true) && line.area_ids.contains(&area.area_id)).then(
                        || CultivationAreaSelectionAudit {
                            season_id: season.season_id,
                            line_type_id: line.line_type_id,
                            area_id: area.area_id,
                            active_effect_score: area.active_effect_score,
                            middle_node_item_ids: area.middle_node_item_ids.clone(),
                            big_node_fantasy_ids: area.big_node_fantasy_ids.clone(),
                        },
                    )
                })
            })
        })
        .collect::<Vec<_>>();
    active_cultivation_areas.sort_by_key(|area| (area.season_id, area.line_type_id, area.area_id));
    ProfileEventObservation {
        event_sequence,
        capture_sequence,
        observed_micros,
        character_id: profile.character.character_id.clone(),
        current_season_id,
        battle_imagine_skills,
        fantasy_atlas_stages,
        active_cultivation_areas,
    }
}

#[derive(Debug, Default, Serialize)]
struct WorldFieldPresence {
    scene_id: bool,
    map_id: bool,
    line_id: bool,
    scene_instance_id: bool,
    dungeon_instance_id: bool,
}

impl WorldFieldPresence {
    fn observe(&mut self, context: &WorldContext) {
        self.scene_id |= context.scene_id.is_some();
        self.map_id |= context.map_id.is_some();
        self.line_id |= context.line_id.is_some();
        self.scene_instance_id |= context.scene_instance_id.is_some();
        self.dungeon_instance_id |= context.dungeon_instance_id.is_some();
    }
}

#[derive(Debug, Default, Serialize)]
struct StructuralAudit {
    envelope_tags: Vec<u32>,
    character_section_tags: Vec<u32>,
    character_base_tags: Vec<u32>,
    avatar: AvatarStructuralAudit,
    prohibited_account_id_tag_present: bool,
    prohibited_open_id_tag_present: bool,
}

#[derive(Debug, Default, Serialize)]
struct AvatarStructuralAudit {
    avatar_info_tag_present: bool,
    avatar_info_tags: Vec<u32>,
    avatar_id_tag_present: bool,
    business_card_style_id_tag_present: bool,
    avatar_frame_id_tag_present: bool,
    profile: PictureStructuralAudit,
    half_body: PictureStructuralAudit,
}

#[derive(Debug, Default, Serialize)]
struct PictureStructuralAudit {
    picture_tag_present: bool,
    picture_info_tags: Vec<u32>,
    url_tag_present: bool,
    verification_tag_present: bool,
    verification_tags: Vec<u32>,
}

#[derive(Debug, Default, Serialize)]
struct PrivacyAudit {
    decoder_declares_account_id: bool,
    decoder_declares_open_id: bool,
    raw_values_rendered: bool,
}

fn structural_audit(payload: &[u8]) -> Result<StructuralAudit, WireAuditError> {
    let envelope = fields(payload)?;
    let character = envelope
        .iter()
        .find_map(|field| (field.number == 1).then_some(field.bytes).flatten())
        .ok_or(WireAuditError::MissingCharacter)?;
    let character_fields = fields(character)?;
    let character_base = character_fields
        .iter()
        .find_map(|field| (field.number == 2).then_some(field.bytes).flatten());
    let base_fields = character_base.map(fields).transpose()?.unwrap_or_default();
    let avatar_info = nested_fields(&base_fields, 25)?;
    let avatar = AvatarStructuralAudit {
        avatar_info_tag_present: !avatar_info.is_empty(),
        avatar_info_tags: tags(&avatar_info),
        avatar_id_tag_present: has_tag(&avatar_info, 1),
        business_card_style_id_tag_present: has_tag(&avatar_info, 4),
        avatar_frame_id_tag_present: has_tag(&avatar_info, 5),
        profile: picture_audit(&avatar_info, 2)?,
        half_body: picture_audit(&avatar_info, 3)?,
    };

    Ok(StructuralAudit {
        envelope_tags: tags(&envelope),
        character_section_tags: tags(&character_fields),
        character_base_tags: tags(&base_fields),
        avatar,
        prohibited_account_id_tag_present: base_fields.iter().any(|field| field.number == 2),
        prohibited_open_id_tag_present: base_fields.iter().any(|field| field.number == 27),
    })
}

fn picture_audit(
    avatar_fields: &[WireField<'_>],
    picture_tag: u32,
) -> Result<PictureStructuralAudit, WireAuditError> {
    let picture_fields = nested_fields(avatar_fields, picture_tag)?;
    let verification_fields = nested_fields(&picture_fields, 2)?;
    Ok(PictureStructuralAudit {
        picture_tag_present: !picture_fields.is_empty(),
        picture_info_tags: tags(&picture_fields),
        url_tag_present: has_tag(&picture_fields, 1),
        verification_tag_present: !verification_fields.is_empty(),
        verification_tags: tags(&verification_fields),
    })
}

fn nested_fields<'a>(
    parent: &[WireField<'a>],
    number: u32,
) -> Result<Vec<WireField<'a>>, WireAuditError> {
    parent
        .iter()
        .find_map(|field| (field.number == number).then_some(field.bytes).flatten())
        .map(fields)
        .transpose()
        .map(Option::unwrap_or_default)
}

fn has_tag(fields: &[WireField<'_>], number: u32) -> bool {
    fields.iter().any(|field| field.number == number)
}

fn tags(fields: &[WireField<'_>]) -> Vec<u32> {
    fields
        .iter()
        .map(|field| field.number)
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect()
}

#[derive(Debug, Clone, Copy)]
struct WireField<'a> {
    number: u32,
    bytes: Option<&'a [u8]>,
}

fn fields(mut input: &[u8]) -> Result<Vec<WireField<'_>>, WireAuditError> {
    let mut output = Vec::new();
    while !input.is_empty() {
        let key = read_varint(&mut input)?;
        let number = u32::try_from(key >> 3).map_err(|_| WireAuditError::InvalidField)?;
        if number == 0 {
            return Err(WireAuditError::InvalidField);
        }
        let wire_type = (key & 7) as u8;
        let bytes = match wire_type {
            0 => {
                read_varint(&mut input)?;
                None
            }
            1 => {
                take(&mut input, 8)?;
                None
            }
            2 => {
                let length = usize::try_from(read_varint(&mut input)?)
                    .map_err(|_| WireAuditError::LengthOverflow)?;
                Some(take(&mut input, length)?)
            }
            5 => {
                take(&mut input, 4)?;
                None
            }
            _ => return Err(WireAuditError::UnsupportedWireType(wire_type)),
        };
        output.push(WireField { number, bytes });
    }
    Ok(output)
}

fn read_varint(input: &mut &[u8]) -> Result<u64, WireAuditError> {
    let mut value = 0_u64;
    for shift in (0..70).step_by(7) {
        let byte = *input.first().ok_or(WireAuditError::Truncated)?;
        *input = &input[1..];
        if shift == 63 && byte > 1 {
            return Err(WireAuditError::InvalidVarint);
        }
        value |= u64::from(byte & 0x7f) << shift;
        if byte & 0x80 == 0 {
            return Ok(value);
        }
    }
    Err(WireAuditError::InvalidVarint)
}

fn take<'a>(input: &mut &'a [u8], length: usize) -> Result<&'a [u8], WireAuditError> {
    let (value, remaining) = input
        .split_at_checked(length)
        .ok_or(WireAuditError::Truncated)?;
    *input = remaining;
    Ok(value)
}

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
enum WireAuditError {
    #[error("protobuf field is truncated")]
    Truncated,
    #[error("protobuf varint is invalid")]
    InvalidVarint,
    #[error("protobuf field number is invalid")]
    InvalidField,
    #[error("protobuf length does not fit in memory")]
    LengthOverflow,
    #[error("protobuf wire type {0} is unsupported")]
    UnsupportedWireType(u8),
    #[error("profile envelope has no character section")]
    MissingCharacter,
}

#[derive(Debug, PartialEq, Eq)]
struct Arguments {
    pack: PathBuf,
    journal: PathBuf,
    target_talent_node: Option<i64>,
    target_profession: Option<i32>,
    target_character_id: Option<String>,
    output: Option<PathBuf>,
}

fn arguments() -> Result<Arguments, String> {
    parse_arguments(std::env::args_os().skip(1))
}

fn parse_arguments(arguments: impl IntoIterator<Item = OsString>) -> Result<Arguments, String> {
    let mut private_research = false;
    let mut pack = None;
    let mut journal = None;
    let mut target_talent_node = None;
    let mut target_profession = None;
    let mut target_character_id = None;
    let mut output = None;
    let mut arguments = arguments.into_iter();

    while let Some(argument) = arguments.next() {
        if argument == OsStr::new("--private-research") {
            private_research = true;
        } else if argument == OsStr::new("--pack") {
            pack = unique_value(pack, arguments.next(), "--pack")?;
        } else if argument == OsStr::new("--target-talent-node") {
            let value =
                unique_value(None, arguments.next(), "--target-talent-node")?.ok_or_else(usage)?;
            let parsed = value
                .to_str()
                .ok_or_else(usage)?
                .parse::<i64>()
                .map_err(|_| usage())?;
            if parsed <= 0 || target_talent_node.replace(parsed).is_some() {
                return Err(usage());
            }
        } else if argument == OsStr::new("--target-profession") {
            let value =
                unique_value(None, arguments.next(), "--target-profession")?.ok_or_else(usage)?;
            let parsed = value
                .to_str()
                .ok_or_else(usage)?
                .parse::<i32>()
                .map_err(|_| usage())?;
            if parsed <= 0 || target_profession.replace(parsed).is_some() {
                return Err(usage());
            }
        } else if argument == OsStr::new("--output") {
            output = unique_value(output, arguments.next(), "--output")?;
        } else if argument == OsStr::new("--target-character-id") {
            let value = unique_value(None, arguments.next(), "--target-character-id")?
                .ok_or_else(usage)?
                .into_string()
                .map_err(|_| usage())?;
            if value.is_empty()
                || !value.bytes().all(|byte| byte.is_ascii_digit())
                || target_character_id.replace(value).is_some()
            {
                return Err(usage());
            }
        } else if argument.to_string_lossy().starts_with('-') || journal.is_some() {
            return Err(usage());
        } else {
            journal = Some(PathBuf::from(argument));
        }
    }
    if !private_research {
        return Err(usage());
    }
    Ok(Arguments {
        pack: pack.map(PathBuf::from).ok_or_else(usage)?,
        journal: journal.ok_or_else(usage)?,
        target_talent_node,
        target_profession,
        target_character_id,
        output: output.map(PathBuf::from),
    })
}

fn unique_value(
    current: Option<OsString>,
    next: Option<OsString>,
    flag: &str,
) -> Result<Option<OsString>, String> {
    if current.is_some() {
        return Err(format!("{flag} may be supplied only once"));
    }
    next.map(Some)
        .ok_or_else(|| format!("{flag} requires a value"))
}

fn usage() -> String {
    "usage: rlogs-profile-audit --private-research --pack <pack.json> [--target-talent-node <id>] [--target-profession <id>] [--target-character-id <id>] [--output <audit.json>] <journal.jsonl>".into()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(field: u32, wire: u8) -> Vec<u8> {
        encode_varint((u64::from(field) << 3) | u64::from(wire))
    }

    fn length_field(field: u32, value: &[u8]) -> Vec<u8> {
        let mut encoded = key(field, 2);
        encoded.extend(encode_varint(value.len() as u64));
        encoded.extend(value);
        encoded
    }

    fn encode_varint(mut value: u64) -> Vec<u8> {
        let mut encoded = Vec::new();
        loop {
            let mut byte = (value & 0x7f) as u8;
            value >>= 7;
            if value != 0 {
                byte |= 0x80;
            }
            encoded.push(byte);
            if value == 0 {
                return encoded;
            }
        }
    }

    #[test]
    fn structural_audit_reports_tags_without_values() {
        let mut base = length_field(2, b"private-account");
        base.extend(length_field(5, b"Character Name"));
        base.extend(length_field(27, b"private-open-id"));
        let mut verify = key(1, 0);
        verify.extend(encode_varint(123));
        let mut profile = length_field(1, b"https://private.invalid/profile.png");
        profile.extend(length_field(2, &verify));
        let half_body = length_field(1, b"https://private.invalid/half-body.png");
        let mut avatar = key(1, 0);
        avatar.extend(encode_varint(91));
        avatar.extend(length_field(2, &profile));
        avatar.extend(length_field(3, &half_body));
        avatar.extend(key(4, 0));
        avatar.extend(encode_varint(8));
        avatar.extend(key(5, 0));
        avatar.extend(encode_varint(7));
        base.extend(length_field(25, &avatar));
        let mut character = key(1, 0);
        character.extend(encode_varint(42));
        character.extend(length_field(2, &base));
        character.extend(length_field(22, &[8, 60]));
        let envelope = length_field(1, &character);

        let audit = structural_audit(&envelope).unwrap();

        assert_eq!(audit.envelope_tags, vec![1]);
        assert_eq!(audit.character_section_tags, vec![1, 2, 22]);
        assert_eq!(audit.character_base_tags, vec![2, 5, 25, 27]);
        assert_eq!(audit.avatar.avatar_info_tags, vec![1, 2, 3, 4, 5]);
        assert!(audit.avatar.avatar_id_tag_present);
        assert!(audit.avatar.profile.url_tag_present);
        assert!(audit.avatar.profile.verification_tag_present);
        assert!(audit.avatar.half_body.url_tag_present);
        assert!(audit.avatar.business_card_style_id_tag_present);
        assert!(audit.avatar.avatar_frame_id_tag_present);
        assert!(audit.prohibited_account_id_tag_present);
        assert!(audit.prohibited_open_id_tag_present);
        let json = serde_json::to_string(&audit).unwrap();
        assert!(!json.contains("private-account"));
        assert!(!json.contains("Character Name"));
        assert!(!json.contains("private-open-id"));
        assert!(!json.contains("private.invalid"));
    }

    #[test]
    fn private_acknowledgement_is_required() {
        assert!(parse_arguments([OsString::from("journal.jsonl")]).is_err());
    }
}
