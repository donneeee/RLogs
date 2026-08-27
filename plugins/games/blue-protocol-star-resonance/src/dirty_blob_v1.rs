//! Bounded reader for the proprietary dirty-container stream used by BPSR.
//!
//! This decoder is deliberately selective. It consumes private/account fields
//! without materializing them and only returns fields approved for character
//! profiles or public world context.

use thiserror::Error;

const OBJECT_BEGIN: i32 = -2;
const OBJECT_END: i32 = -3;
const EMPTY_COLLECTION: i32 = -4;
const REPLACE_COLLECTION: i32 = -1;
const MAX_BLOB_BYTES: usize = 4 * 1024 * 1024;
const MAX_STRING_BYTES: usize = 64 * 1024;
const MAX_COLLECTION_ITEMS: usize = 65_536;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct DirtyCharacterUpdate {
    pub character_id: Option<i64>,
    pub display_id: Option<i64>,
    pub server_id: Option<u32>,
    pub display_name: Option<String>,
    pub class_id: Option<i32>,
    pub level: Option<u32>,
    pub current_experience: Option<i64>,
    pub previous_season_max_level: Option<u32>,
    pub combat_power: Option<i64>,
    pub gender_id: Option<i32>,
    pub body_size_id: Option<i32>,
    pub avatar_id: Option<i32>,
    pub business_card_style_id: Option<i32>,
    pub avatar_frame_id: Option<i32>,
    pub lucky_value_mgr: Option<DirtyLuckyValueUpdate>,
    /// Exact IEEE-754 payload of `UserFightAttr.origin_energy`.
    pub origin_energy_raw_bits: Option<u32>,
    /// Exact local combat-resource arrays. They remain parallel so a protocol
    /// change cannot be hidden by truncating a mismatched pair.
    pub resource_ids: Vec<u32>,
    pub resource_values: Vec<u32>,
    pub cooldowns: Vec<DirtySkillCooldownUpdate>,
    /// Exact dirty update for `CharSerialize.slots.slots`.
    ///
    /// Slot IDs are retained as packet evidence. Classification into primary
    /// Imagine slots (7/8) and auxiliary actions (21-24) happens in the shared
    /// loadout projection, never in this wire decoder.
    pub action_slots: Option<DirtyActionSlotUpdate>,
    /// Exact dirty update for `ProfessionList.AoyiSkillInfoMap`.
    ///
    /// These records carry the runtime remodel level used as the equipped
    /// Battle Imagine tier. They are kept separate from the action-slot map so
    /// an in-place tier or skin change can update the shared profile without
    /// fabricating a slot assignment.
    pub battle_imagine_skills: Option<DirtyBattleImagineSkillUpdate>,
    pub world: DirtyWorldUpdate,
    pub root_fields: Vec<i32>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct DirtyActionSlotUpdate {
    pub replace: bool,
    pub upserts: Vec<DirtyActionSlotEntry>,
    pub removals: Vec<i32>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct DirtyActionSlotEntry {
    pub map_key: i32,
    pub slot_id: Option<i32>,
    pub skill_id: Option<i32>,
    pub auto_battle_disabled: Option<bool>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct DirtyBattleImagineSkillUpdate {
    pub replace: bool,
    pub upserts: Vec<DirtyBattleImagineSkillEntry>,
    pub removals: Vec<i32>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct DirtyBattleImagineSkillEntry {
    pub map_key: i32,
    pub skill_id: Option<i32>,
    pub level: Option<i32>,
    pub replacement_skill_ids: Option<Vec<i32>>,
    pub remodel_level: Option<i32>,
    pub current_skin_id: Option<i32>,
    pub unlocked_skin_ids: Option<DirtyEnabledIdUpdate>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct DirtyEnabledIdUpdate {
    pub replace: bool,
    pub upserts: Vec<(i32, bool)>,
    pub removals: Vec<i32>,
}

/// Exact gameplay-only fields from `UserFightAttr.CdInfo` in a dirty
/// character-container update. Reduction and acceleration values deliberately
/// remain raw until packet-observed transitions prove their units.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct DirtySkillCooldownUpdate {
    pub skill_level_id: Option<i32>,
    pub skill_begin_time: Option<i64>,
    pub duration: Option<i32>,
    pub cooldown_type: Option<u32>,
    pub profession_hold_begin_time: Option<i64>,
    pub charge_count: Option<i32>,
    pub valid_cooldown_time: Option<i32>,
    pub sub_cooldown_ratio: Option<i32>,
    pub sub_cooldown_fixed: Option<i64>,
    pub accelerate_cooldown_ratio: Option<i32>,
}

/// Gameplay-only dirty update for the character-container LuckyValueMgr.
///
/// The names are inherited from the current protobuf schema. They do not by
/// themselves prove that this state drives combat Critical or Lucky outcomes.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DirtyLuckyValueUpdate {
    pub replace: bool,
    pub init_value: Option<bool>,
    pub upserts: Vec<DirtyLuckyValueEntry>,
    pub removals: Vec<i32>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DirtyLuckyValueEntry {
    pub map_key: i32,
    pub luck_id: Option<i32>,
    pub luck_value: Option<i32>,
    pub next_time: Option<i64>,
}

impl DirtyCharacterUpdate {
    pub(crate) fn has_public_profile_fields(&self) -> bool {
        self.display_id.is_some()
            || self.server_id.is_some()
            || self.display_name.is_some()
            || self.class_id.is_some()
            || self.level.is_some()
            || self.current_experience.is_some()
            || self.previous_season_max_level.is_some()
            || self.combat_power.is_some()
            || self.gender_id.is_some()
            || self.body_size_id.is_some()
            || self.avatar_id.is_some()
            || self.business_card_style_id.is_some()
            || self.avatar_frame_id.is_some()
    }

    pub(crate) fn has_loadout_fields(&self) -> bool {
        self.action_slots.is_some() || self.battle_imagine_skills.is_some()
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct DirtyWorldUpdate {
    pub map_id: Option<u32>,
    pub channel_id: Option<u32>,
    pub level_map_id: Option<u32>,
    pub line_id: Option<u32>,
    pub scene_instance_id: Option<String>,
    pub dungeon_instance_id: Option<String>,
}

impl DirtyWorldUpdate {
    pub(crate) fn has_public_fields(&self) -> bool {
        self.map_id.is_some()
            || self.channel_id.is_some()
            || self.level_map_id.is_some()
            || self.line_id.is_some()
            || self.scene_instance_id.is_some()
            || self.dungeon_instance_id.is_some()
    }
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum DirtyBlobError {
    #[error("dirty-container stream type {0} is unsupported")]
    UnsupportedStreamType(i32),

    #[error("dirty-container blob exceeds the {MAX_BLOB_BYTES}-byte limit")]
    BlobTooLarge,

    #[error("dirty-container blob ended unexpectedly")]
    Truncated,

    #[error("dirty-container object has invalid marker {0}")]
    InvalidObjectMarker(i32),

    #[error("dirty-container object size is invalid")]
    InvalidObjectSize,

    #[error("dirty-container field id {0} is invalid")]
    InvalidFieldId(i32),

    #[error("dirty-container string exceeds the {MAX_STRING_BYTES}-byte limit")]
    StringTooLarge,

    #[error("dirty-container public string is not valid UTF-8")]
    InvalidUtf8,

    #[error("dirty-container collection encoding is invalid")]
    InvalidCollection,

    #[error("dirty-container contains trailing bytes")]
    TrailingBytes,
}

pub(crate) fn decode_character_update(
    bytes: &[u8],
    stream_type: i32,
) -> Result<DirtyCharacterUpdate, DirtyBlobError> {
    let mut reader = BlobReader::for_stream(bytes, stream_type)?;
    let root_end = reader.begin_object()?;
    let mut update = DirtyCharacterUpdate::default();

    while let Some(field) = reader.next_field(root_end)? {
        update.root_fields.push(field);
        match field {
            1 => update.character_id = Some(i64::from(reader.read_i32()?)),
            2 => parse_character_base(&mut reader, &mut update)?,
            3 => parse_scene(&mut reader, &mut update.world)?,
            8 | 49 | 102 | 116 => reader.skip_object()?,
            16 => parse_user_fight_attr(&mut reader, &mut update)?,
            22 => parse_role_level(&mut reader, &mut update)?,
            55 => update.action_slots = Some(parse_action_slots(&mut reader)?),
            61 => parse_profession_list(&mut reader, &mut update)?,
            88 => update.lucky_value_mgr = Some(parse_lucky_value_mgr(&mut reader)?),
            96 => parse_fight_point(&mut reader, &mut update)?,
            // Internal persistence serial. Consume it but never retain it.
            104 => {
                reader.read_i64()?;
            }
            // Unknown root types cannot be skipped safely without their schema.
            // Abandon only this bounded object rather than guessing a width.
            _ => reader.skip_object_body(root_end),
        }
    }
    reader.finish_object(root_end)?;
    if !reader.is_finished() {
        return Err(DirtyBlobError::TrailingBytes);
    }
    Ok(update)
}

fn parse_action_slots(
    reader: &mut BlobReader<'_>,
) -> Result<DirtyActionSlotUpdate, DirtyBlobError> {
    let end = reader.begin_object()?;
    let mut update = DirtyActionSlotUpdate::default();
    while let Some(field) = reader.next_field(end)? {
        match field {
            1 => {
                let counts = reader.read_map_counts()?;
                update.replace = counts.replace;
                for _ in 0..counts.add {
                    let map_key = reader.read_i32()?;
                    update.upserts.push(parse_action_slot(reader, map_key)?);
                }
                for _ in 0..counts.remove {
                    update.removals.push(reader.read_i32()?);
                }
                for _ in 0..counts.update {
                    let map_key = reader.read_i32()?;
                    update.upserts.push(parse_action_slot(reader, map_key)?);
                }
            }
            _ => reader.skip_object_body(end),
        }
    }
    reader.finish_object(end)?;
    Ok(update)
}

fn parse_action_slot(
    reader: &mut BlobReader<'_>,
    map_key: i32,
) -> Result<DirtyActionSlotEntry, DirtyBlobError> {
    let end = reader.begin_object()?;
    let mut entry = DirtyActionSlotEntry {
        map_key,
        ..DirtyActionSlotEntry::default()
    };
    while let Some(field) = reader.next_field(end)? {
        match field {
            1 => entry.slot_id = Some(reader.read_i32()?),
            2 => entry.skill_id = Some(reader.read_i32()?),
            3 => entry.auto_battle_disabled = Some(reader.read_u8()? != 0),
            _ => reader.skip_object_body(end),
        }
    }
    reader.finish_object(end)?;
    Ok(entry)
}

fn parse_user_fight_attr(
    reader: &mut BlobReader<'_>,
    update: &mut DirtyCharacterUpdate,
) -> Result<(), DirtyBlobError> {
    let end = reader.begin_object()?;
    while let Some(field) = reader.next_field(end)? {
        match field {
            1 | 2 | 7 => {
                reader.read_i64()?;
            }
            3 => update.origin_energy_raw_bits = Some(reader.read_f32()?.to_bits()),
            4 => update.resource_ids = reader.read_u32_list()?,
            5 => update.resource_values = reader.read_u32_list()?,
            6 | 8 => {
                reader.read_i32()?;
            }
            // Dirty repeated-message updates encode the changed element as a
            // nested object at the repeated field, rather than as a protobuf
            // collection. Retain every occurrence in wire order.
            9 => update.cooldowns.push(parse_skill_cooldown_info(reader)?),
            _ => reader.skip_object_body(end),
        }
    }
    reader.finish_object(end)
}

fn parse_skill_cooldown_info(
    reader: &mut BlobReader<'_>,
) -> Result<DirtySkillCooldownUpdate, DirtyBlobError> {
    let end = reader.begin_object()?;
    let mut update = DirtySkillCooldownUpdate::default();
    while let Some(field) = reader.next_field(end)? {
        match field {
            1 => update.skill_level_id = Some(reader.read_i32()?),
            2 => update.skill_begin_time = Some(reader.read_i64()?),
            3 => update.duration = Some(reader.read_i32()?),
            4 => update.cooldown_type = Some(reader.read_u32()?),
            6 => update.profession_hold_begin_time = Some(reader.read_i64()?),
            7 => update.charge_count = Some(reader.read_i32()?),
            8 => update.valid_cooldown_time = Some(reader.read_i32()?),
            9 => update.sub_cooldown_ratio = Some(reader.read_i32()?),
            10 => update.sub_cooldown_fixed = Some(reader.read_i64()?),
            11 => update.accelerate_cooldown_ratio = Some(reader.read_i32()?),
            _ => reader.skip_object_body(end),
        }
    }
    reader.finish_object(end)?;
    Ok(update)
}

pub fn decode_lucky_value_update(
    bytes: &[u8],
    stream_type: i32,
) -> Result<Option<DirtyLuckyValueUpdate>, DirtyBlobError> {
    decode_character_update(bytes, stream_type).map(|update| update.lucky_value_mgr)
}

fn parse_lucky_value_mgr(
    reader: &mut BlobReader<'_>,
) -> Result<DirtyLuckyValueUpdate, DirtyBlobError> {
    let end = reader.begin_object()?;
    let mut update = DirtyLuckyValueUpdate::default();
    while let Some(field) = reader.next_field(end)? {
        match field {
            1 => {
                let counts = reader.read_map_counts()?;
                update.replace = counts.replace;
                for _ in 0..counts.add {
                    let map_key = reader.read_i32()?;
                    update
                        .upserts
                        .push(parse_lucky_value_info(reader, map_key)?);
                }
                for _ in 0..counts.remove {
                    update.removals.push(reader.read_i32()?);
                }
                for _ in 0..counts.update {
                    let map_key = reader.read_i32()?;
                    update
                        .upserts
                        .push(parse_lucky_value_info(reader, map_key)?);
                }
            }
            2 => update.init_value = Some(reader.read_u8()? != 0),
            _ => reader.skip_object_body(end),
        }
    }
    reader.finish_object(end)?;
    update.upserts.sort_unstable_by_key(|entry| entry.map_key);
    update.removals.sort_unstable();
    Ok(update)
}

fn parse_lucky_value_info(
    reader: &mut BlobReader<'_>,
    map_key: i32,
) -> Result<DirtyLuckyValueEntry, DirtyBlobError> {
    let end = reader.begin_object()?;
    let mut entry = DirtyLuckyValueEntry {
        map_key,
        ..DirtyLuckyValueEntry::default()
    };
    while let Some(field) = reader.next_field(end)? {
        match field {
            1 => entry.luck_id = Some(reader.read_i32()?),
            2 => entry.luck_value = Some(reader.read_i32()?),
            3 => entry.next_time = Some(reader.read_i64()?),
            _ => reader.skip_object_body(end),
        }
    }
    reader.finish_object(end)?;
    Ok(entry)
}

fn parse_character_base(
    reader: &mut BlobReader<'_>,
    update: &mut DirtyCharacterUpdate,
) -> Result<(), DirtyBlobError> {
    let end = reader.begin_object()?;
    while let Some(field) = reader.next_field(end)? {
        match field {
            1 => update.character_id = Some(reader.read_i64()?),
            // Account ID: consume without allocating or retaining.
            2 => reader.skip_string()?,
            3 => update.display_id = Some(reader.read_i64()?),
            4 => update.server_id = Some(reader.read_u32()?),
            5 => update.display_name = Some(reader.read_public_string()?),
            6 => update.gender_id = Some(reader.read_i32()?),
            7..=9 => {
                reader.read_u8()?;
            }
            10..=13 => {
                reader.read_f32()?;
            }
            14 => reader.skip_object()?,
            15 => {
                reader.read_u32()?;
            }
            16..=18 => {
                reader.read_i64()?;
            }
            19 | 20 => reader.skip_object()?,
            21 => {
                reader.read_u64()?;
            }
            22 => update.body_size_id = Some(reader.read_i32()?),
            23 => reader.skip_object()?,
            24 => reader.skip_i32_list()?,
            25 => parse_avatar(reader, update)?,
            // Total online time is private behavioral data.
            26 => {
                reader.read_u64()?;
            }
            // Open/platform ID: consume without allocating or retaining.
            27 => reader.skip_string()?,
            28 | 29 => {
                reader.read_i32()?;
            }
            31 => update.class_id = Some(reader.read_i32()?),
            // Last calculated online-time total is private behavioral data.
            32 => {
                reader.read_u64()?;
            }
            33 => {
                reader.read_i32()?;
            }
            34 => reader.skip_string()?,
            35 => update.combat_power = Some(i64::from(reader.read_i32()?)),
            36 => {
                reader.read_i64()?;
            }
            37 => reader.skip_string()?,
            38 => {
                reader.read_i64()?;
            }
            39 => {
                reader.read_i32()?;
            }
            40 | 42 => {
                reader.read_i64()?;
            }
            _ => reader.skip_object_body(end),
        }
    }
    reader.finish_object(end)
}

fn parse_avatar(
    reader: &mut BlobReader<'_>,
    update: &mut DirtyCharacterUpdate,
) -> Result<(), DirtyBlobError> {
    let end = reader.begin_object()?;
    while let Some(field) = reader.next_field(end)? {
        match field {
            1 => update.avatar_id = Some(reader.read_i32()?),
            // Picture subtrees can carry user-supplied URLs.
            2 | 3 => reader.skip_object()?,
            4 => update.business_card_style_id = Some(reader.read_i32()?),
            5 => update.avatar_frame_id = Some(reader.read_i32()?),
            _ => reader.skip_object_body(end),
        }
    }
    reader.finish_object(end)
}

fn parse_scene(
    reader: &mut BlobReader<'_>,
    world: &mut DirtyWorldUpdate,
) -> Result<(), DirtyBlobError> {
    let end = reader.begin_object()?;
    while let Some(field) = reader.next_field(end)? {
        match field {
            1 => world.map_id = Some(reader.read_u32()?),
            2 => world.channel_id = Some(reader.read_u32()?),
            3 | 5 | 12 | 17 => reader.skip_object()?,
            4 => {
                reader.read_i64()?;
            }
            6 => world.level_map_id = Some(reader.read_u32()?),
            7 | 9 | 10 | 16 => {
                reader.read_u32()?;
            }
            8 => reader.skip_u32_map()?,
            11 => {
                reader.read_u8()?;
            }
            13 => world.scene_instance_id = Some(reader.read_public_string()?),
            14 => world.dungeon_instance_id = Some(reader.read_public_string()?),
            15 => world.line_id = Some(reader.read_u32()?),
            18..=21 => {
                reader.read_i32()?;
            }
            _ => reader.skip_object_body(end),
        }
    }
    reader.finish_object(end)
}

fn parse_role_level(
    reader: &mut BlobReader<'_>,
    update: &mut DirtyCharacterUpdate,
) -> Result<(), DirtyBlobError> {
    let end = reader.begin_object()?;
    while let Some(field) = reader.next_field(end)? {
        match field {
            1 => update.level = positive_u32(reader.read_i32()?),
            2 => update.current_experience = Some(reader.read_i64()?),
            3 => reader.skip_i32_bool_map()?,
            4 => reader.skip_object()?,
            5 => reader.skip_i32_i64_map()?,
            6 => {
                reader.read_i32()?;
            }
            7..=10 => {
                reader.read_i64()?;
            }
            11 => update.previous_season_max_level = positive_u32(reader.read_i32()?),
            _ => reader.skip_object_body(end),
        }
    }
    reader.finish_object(end)
}

fn parse_profession_list(
    reader: &mut BlobReader<'_>,
    update: &mut DirtyCharacterUpdate,
) -> Result<(), DirtyBlobError> {
    let end = reader.begin_object()?;
    while let Some(field) = reader.next_field(end)? {
        match field {
            1 => update.class_id = Some(reader.read_i32()?),
            3 => reader.skip_i32_list()?,
            7 => update.battle_imagine_skills = Some(parse_battle_imagine_skill_map(reader)?),
            // Other later fields contain profession/talent state. They remain
            // outside this bounded loadout slice.
            _ => reader.skip_object_body(end),
        }
    }
    reader.finish_object(end)
}

fn parse_battle_imagine_skill_map(
    reader: &mut BlobReader<'_>,
) -> Result<DirtyBattleImagineSkillUpdate, DirtyBlobError> {
    let counts = reader.read_map_counts()?;
    let mut update = DirtyBattleImagineSkillUpdate {
        replace: counts.replace,
        ..DirtyBattleImagineSkillUpdate::default()
    };
    for _ in 0..counts.add {
        let map_key = reader.read_i32()?;
        update
            .upserts
            .push(parse_battle_imagine_skill(reader, map_key)?);
    }
    for _ in 0..counts.remove {
        update.removals.push(reader.read_i32()?);
    }
    for _ in 0..counts.update {
        let map_key = reader.read_i32()?;
        update
            .upserts
            .push(parse_battle_imagine_skill(reader, map_key)?);
    }
    Ok(update)
}

fn parse_battle_imagine_skill(
    reader: &mut BlobReader<'_>,
    map_key: i32,
) -> Result<DirtyBattleImagineSkillEntry, DirtyBlobError> {
    let end = reader.begin_object()?;
    let mut entry = DirtyBattleImagineSkillEntry {
        map_key,
        ..DirtyBattleImagineSkillEntry::default()
    };
    while let Some(field) = reader.next_field(end)? {
        match field {
            1 => entry.skill_id = Some(reader.read_i32()?),
            2 => entry.level = Some(reader.read_i32()?),
            3 => entry.replacement_skill_ids = Some(reader.read_i32_list()?),
            4 => entry.remodel_level = Some(reader.read_i32()?),
            5 => entry.current_skin_id = Some(reader.read_i32()?),
            6 => entry.unlocked_skin_ids = Some(reader.read_i32_bool_map()?),
            _ => reader.skip_object_body(end),
        }
    }
    reader.finish_object(end)?;
    Ok(entry)
}

fn parse_fight_point(
    reader: &mut BlobReader<'_>,
    update: &mut DirtyCharacterUpdate,
) -> Result<(), DirtyBlobError> {
    let end = reader.begin_object()?;
    while let Some(field) = reader.next_field(end)? {
        match field {
            1 => update.combat_power = Some(i64::from(reader.read_i32()?)),
            _ => reader.skip_object_body(end),
        }
    }
    reader.finish_object(end)
}

fn positive_u32(value: i32) -> Option<u32> {
    (value > 0).then(|| u32::try_from(value).expect("positive i32 always fits u32"))
}

pub(crate) struct BlobReader<'a> {
    bytes: &'a [u8],
    offset: usize,
    stream_safe: bool,
}

impl<'a> BlobReader<'a> {
    pub(crate) fn for_stream(bytes: &'a [u8], stream_type: i32) -> Result<Self, DirtyBlobError> {
        if bytes.len() > MAX_BLOB_BYTES {
            return Err(DirtyBlobError::BlobTooLarge);
        }
        let stream_safe = match stream_type {
            0 => true,
            1 => false,
            other => return Err(DirtyBlobError::UnsupportedStreamType(other)),
        };
        Ok(Self {
            bytes,
            offset: 0,
            stream_safe,
        })
    }

    pub(crate) fn is_finished(&self) -> bool {
        self.offset == self.bytes.len()
    }

    fn read_raw(&mut self, length: usize) -> Result<&'a [u8], DirtyBlobError> {
        let data_end = self
            .offset
            .checked_add(length)
            .ok_or(DirtyBlobError::Truncated)?;
        let next = data_end
            .checked_add(if self.stream_safe { 4 } else { 0 })
            .ok_or(DirtyBlobError::Truncated)?;
        if next > self.bytes.len() {
            return Err(DirtyBlobError::Truncated);
        }
        let value = &self.bytes[self.offset..data_end];
        self.offset = next;
        Ok(value)
    }

    fn skip_raw(&mut self, length: usize) -> Result<(), DirtyBlobError> {
        self.read_raw(length).map(|_| ())
    }

    fn read_u8(&mut self) -> Result<u8, DirtyBlobError> {
        Ok(self.read_raw(1)?[0])
    }

    pub(crate) fn read_i32(&mut self) -> Result<i32, DirtyBlobError> {
        Ok(i32::from_le_bytes(
            self.read_raw(4)?
                .try_into()
                .expect("bounded four-byte slice"),
        ))
    }

    pub(crate) fn read_u32(&mut self) -> Result<u32, DirtyBlobError> {
        Ok(u32::from_le_bytes(
            self.read_raw(4)?
                .try_into()
                .expect("bounded four-byte slice"),
        ))
    }

    fn read_i64(&mut self) -> Result<i64, DirtyBlobError> {
        Ok(i64::from_le_bytes(
            self.read_raw(8)?
                .try_into()
                .expect("bounded eight-byte slice"),
        ))
    }

    fn read_u64(&mut self) -> Result<u64, DirtyBlobError> {
        Ok(u64::from_le_bytes(
            self.read_raw(8)?
                .try_into()
                .expect("bounded eight-byte slice"),
        ))
    }

    fn read_f32(&mut self) -> Result<f32, DirtyBlobError> {
        Ok(f32::from_le_bytes(
            self.read_raw(4)?
                .try_into()
                .expect("bounded four-byte slice"),
        ))
    }

    fn string_length(&mut self) -> Result<usize, DirtyBlobError> {
        let length =
            usize::try_from(self.read_u32()?).map_err(|_| DirtyBlobError::StringTooLarge)?;
        if length > MAX_STRING_BYTES {
            return Err(DirtyBlobError::StringTooLarge);
        }
        Ok(length)
    }

    fn skip_string(&mut self) -> Result<(), DirtyBlobError> {
        let length = self.string_length()?;
        self.skip_raw(length)
    }

    fn read_public_string(&mut self) -> Result<String, DirtyBlobError> {
        let length = self.string_length()?;
        let bytes = self.read_raw(length)?;
        std::str::from_utf8(bytes)
            .map(str::to_owned)
            .map_err(|_| DirtyBlobError::InvalidUtf8)
    }

    pub(crate) fn begin_object(&mut self) -> Result<usize, DirtyBlobError> {
        let marker = self.read_i32()?;
        if marker != OBJECT_BEGIN {
            return Err(DirtyBlobError::InvalidObjectMarker(marker));
        }
        let size = self.read_i32()?;
        let size = usize::try_from(size).map_err(|_| DirtyBlobError::InvalidObjectSize)?;
        let end = self
            .offset
            .checked_add(size)
            .ok_or(DirtyBlobError::InvalidObjectSize)?;
        if end > self.bytes.len() {
            return Err(DirtyBlobError::InvalidObjectSize);
        }
        Ok(end)
    }

    pub(crate) fn next_field(&mut self, object_end: usize) -> Result<Option<i32>, DirtyBlobError> {
        if self.offset == object_end {
            return Ok(None);
        }
        if self.offset > object_end {
            return Err(DirtyBlobError::InvalidObjectSize);
        }
        let field = self.read_i32()?;
        if self.offset > object_end {
            return Err(DirtyBlobError::InvalidObjectSize);
        }
        if field <= 0 {
            return Err(DirtyBlobError::InvalidFieldId(field));
        }
        Ok(Some(field))
    }

    pub(crate) fn skip_object_body(&mut self, object_end: usize) {
        self.offset = object_end;
    }

    pub(crate) fn finish_object(&mut self, object_end: usize) -> Result<(), DirtyBlobError> {
        if self.offset != object_end {
            return Err(DirtyBlobError::InvalidObjectSize);
        }
        let marker = self.read_i32()?;
        if marker != OBJECT_END {
            return Err(DirtyBlobError::InvalidObjectMarker(marker));
        }
        Ok(())
    }

    fn skip_object(&mut self) -> Result<(), DirtyBlobError> {
        let end = self.begin_object()?;
        self.skip_object_body(end);
        self.finish_object(end)
    }

    fn skip_i32_list(&mut self) -> Result<(), DirtyBlobError> {
        self.read_i32_list().map(drop)
    }

    fn read_i32_list(&mut self) -> Result<Vec<i32>, DirtyBlobError> {
        let count = self.read_collection_count()?;
        let mut values = Vec::with_capacity(count);
        for _ in 0..count {
            values.push(self.read_i32()?);
        }
        Ok(values)
    }

    fn read_u32_list(&mut self) -> Result<Vec<u32>, DirtyBlobError> {
        let count = self.read_collection_count()?;
        let mut values = Vec::with_capacity(count);
        for _ in 0..count {
            values.push(self.read_u32()?);
        }
        Ok(values)
    }

    fn skip_u32_map(&mut self) -> Result<(), DirtyBlobError> {
        let counts = self.read_map_counts()?;
        for _ in 0..counts.add {
            self.read_u32()?;
            self.read_u32()?;
        }
        for _ in 0..counts.remove {
            self.read_u32()?;
        }
        for _ in 0..counts.update {
            self.read_u32()?;
            self.read_u32()?;
        }
        Ok(())
    }

    fn skip_i32_bool_map(&mut self) -> Result<(), DirtyBlobError> {
        self.read_i32_bool_map().map(drop)
    }

    fn read_i32_bool_map(&mut self) -> Result<DirtyEnabledIdUpdate, DirtyBlobError> {
        let counts = self.read_map_counts()?;
        let mut update = DirtyEnabledIdUpdate {
            replace: counts.replace,
            ..DirtyEnabledIdUpdate::default()
        };
        for _ in 0..counts.add {
            update
                .upserts
                .push((self.read_i32()?, self.read_u8()? != 0));
        }
        for _ in 0..counts.remove {
            update.removals.push(self.read_i32()?);
        }
        for _ in 0..counts.update {
            update
                .upserts
                .push((self.read_i32()?, self.read_u8()? != 0));
        }
        Ok(update)
    }

    fn skip_i32_i64_map(&mut self) -> Result<(), DirtyBlobError> {
        let counts = self.read_map_counts()?;
        for _ in 0..counts.add {
            self.read_i32()?;
            self.read_i64()?;
        }
        for _ in 0..counts.remove {
            self.read_i32()?;
        }
        for _ in 0..counts.update {
            self.read_i32()?;
            self.read_i64()?;
        }
        Ok(())
    }

    fn read_collection_count(&mut self) -> Result<usize, DirtyBlobError> {
        let count = self.read_i32()?;
        if count == EMPTY_COLLECTION {
            return Ok(0);
        }
        checked_collection_count(count)
    }

    pub(crate) fn read_map_counts(&mut self) -> Result<MapCounts, DirtyBlobError> {
        let first = self.read_i32()?;
        if first == EMPTY_COLLECTION {
            return Ok(MapCounts::default());
        }
        let replace = first == REPLACE_COLLECTION;
        let (add, remove, update) = if replace {
            (self.read_i32()?, 0, 0)
        } else {
            (first, self.read_i32()?, self.read_i32()?)
        };
        let counts = MapCounts {
            replace,
            add: checked_collection_count(add)?,
            remove: checked_collection_count(remove)?,
            update: checked_collection_count(update)?,
        };
        let total = counts
            .add
            .checked_add(counts.remove)
            .and_then(|value| value.checked_add(counts.update))
            .ok_or(DirtyBlobError::InvalidCollection)?;
        if total > MAX_COLLECTION_ITEMS {
            return Err(DirtyBlobError::InvalidCollection);
        }
        Ok(counts)
    }
}

#[derive(Default)]
pub(crate) struct MapCounts {
    pub(crate) replace: bool,
    pub(crate) add: usize,
    pub(crate) remove: usize,
    pub(crate) update: usize,
}

fn checked_collection_count(value: i32) -> Result<usize, DirtyBlobError> {
    let value = usize::try_from(value).map_err(|_| DirtyBlobError::InvalidCollection)?;
    if value > MAX_COLLECTION_ITEMS {
        return Err(DirtyBlobError::InvalidCollection);
    }
    Ok(value)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn i32_value(value: i32) -> Vec<u8> {
        value.to_le_bytes().to_vec()
    }

    fn u32_value(value: u32) -> Vec<u8> {
        value.to_le_bytes().to_vec()
    }

    fn i64_value(value: i64) -> Vec<u8> {
        value.to_le_bytes().to_vec()
    }

    fn u64_value(value: u64) -> Vec<u8> {
        value.to_le_bytes().to_vec()
    }

    fn u8_value(value: u8) -> Vec<u8> {
        vec![value]
    }

    fn string_value(value: &str) -> Vec<u8> {
        let mut bytes = u32_value(u32::try_from(value.len()).unwrap());
        bytes.extend_from_slice(value.as_bytes());
        bytes
    }

    fn object(fields: Vec<(i32, Vec<u8>)>) -> Vec<u8> {
        let mut body = Vec::new();
        for (field, value) in fields {
            body.extend_from_slice(&field.to_le_bytes());
            body.extend(value);
        }
        let mut bytes = i32_value(OBJECT_BEGIN);
        bytes.extend(i32_value(i32::try_from(body.len()).unwrap()));
        bytes.extend(body);
        bytes.extend(i32_value(OBJECT_END));
        bytes
    }

    #[test]
    fn empty_dirty_object_is_valid_and_has_no_public_fields() {
        let update = decode_character_update(&object(Vec::new()), 1).unwrap();
        assert_eq!(update, DirtyCharacterUpdate::default());
        assert!(!update.has_public_profile_fields());
        assert!(!update.world.has_public_fields());
    }

    #[test]
    fn private_time_fields_and_save_serial_are_consumed_but_not_returned() {
        let base = object(vec![
            (26, u64_value(10_252_790)),
            (32, u64_value(1_785_287_883)),
        ]);
        let update =
            decode_character_update(&object(vec![(2, base), (104, i64_value(137_201))]), 1)
                .unwrap();

        assert_eq!(update.root_fields, vec![2, 104]);
        assert!(!update.has_public_profile_fields());
        assert!(!update.world.has_public_fields());
    }

    #[test]
    fn quest_and_story_subtrees_are_skipped_exactly() {
        let quest = object(vec![(1, i32_value(123))]);
        let story = object(vec![(9, i64_value(456))]);
        let update = decode_character_update(&object(vec![(8, quest), (116, story)]), 1).unwrap();

        assert_eq!(update.root_fields, vec![8, 116]);
        assert!(!update.has_public_profile_fields());
    }

    #[test]
    fn lucky_value_manager_replace_state_is_preserved_without_interpretation() {
        let entry = object(vec![
            (1, i32_value(101)),
            (2, i32_value(-48_106)),
            (3, i64_value(123_456)),
        ]);
        let mut values = i32_value(REPLACE_COLLECTION);
        values.extend(i32_value(1));
        values.extend(i32_value(101));
        values.extend(entry);
        let manager = object(vec![(1, values), (2, u8_value(1))]);

        let update = decode_character_update(&object(vec![(88, manager)]), 1).unwrap();
        assert_eq!(update.root_fields, vec![88]);
        assert_eq!(
            update.lucky_value_mgr,
            Some(DirtyLuckyValueUpdate {
                replace: true,
                init_value: Some(true),
                upserts: vec![DirtyLuckyValueEntry {
                    map_key: 101,
                    luck_id: Some(101),
                    luck_value: Some(-48_106),
                    next_time: Some(123_456),
                }],
                removals: Vec::new(),
            })
        );
    }

    #[test]
    fn action_slot_replace_preserves_exact_primary_and_auxiliary_bindings() {
        let slot_7 = object(vec![
            (1, i32_value(7)),
            (2, i32_value(3_948)),
            (3, u8_value(1)),
        ]);
        let slot_8 = object(vec![(1, i32_value(8)), (2, i32_value(3_982))]);
        let mut values = i32_value(REPLACE_COLLECTION);
        values.extend(i32_value(2));
        values.extend(i32_value(7));
        values.extend(slot_7);
        values.extend(i32_value(8));
        values.extend(slot_8);
        let slots = object(vec![(1, values)]);

        let update = decode_character_update(&object(vec![(55, slots)]), 1).unwrap();

        assert!(update.has_loadout_fields());
        assert_eq!(
            update.action_slots,
            Some(DirtyActionSlotUpdate {
                replace: true,
                upserts: vec![
                    DirtyActionSlotEntry {
                        map_key: 7,
                        slot_id: Some(7),
                        skill_id: Some(3_948),
                        auto_battle_disabled: Some(true),
                    },
                    DirtyActionSlotEntry {
                        map_key: 8,
                        slot_id: Some(8),
                        skill_id: Some(3_982),
                        auto_battle_disabled: None,
                    },
                ],
                removals: Vec::new(),
            })
        );
    }

    #[test]
    fn action_slot_delta_preserves_removals_and_partial_upserts() {
        let changed_slot = object(vec![(2, i32_value(3_982))]);
        let mut values = i32_value(0); // additions
        values.extend(i32_value(1)); // removals
        values.extend(i32_value(1)); // updates
        values.extend(i32_value(21));
        values.extend(i32_value(8));
        values.extend(changed_slot);
        let slots = object(vec![(1, values)]);

        let update = decode_character_update(&object(vec![(55, slots)]), 1).unwrap();

        assert_eq!(
            update.action_slots,
            Some(DirtyActionSlotUpdate {
                replace: false,
                upserts: vec![DirtyActionSlotEntry {
                    map_key: 8,
                    slot_id: None,
                    skill_id: Some(3_982),
                    auto_battle_disabled: None,
                }],
                removals: vec![21],
            })
        );
    }

    #[test]
    fn battle_imagine_skill_replace_preserves_runtime_tier_and_presentation_fields() {
        let mut replacements = i32_value(2);
        replacements.extend(i32_value(4_001));
        replacements.extend(i32_value(4_002));
        let mut unlocked = i32_value(REPLACE_COLLECTION);
        unlocked.extend(i32_value(2));
        unlocked.extend(i32_value(71));
        unlocked.extend(u8_value(1));
        unlocked.extend(i32_value(72));
        unlocked.extend(u8_value(0));
        let lucy = object(vec![
            (1, i32_value(3_982)),
            (2, i32_value(60)),
            (3, replacements),
            (4, i32_value(4)),
            (5, i32_value(71)),
            (6, unlocked),
        ]);
        let mut skills = i32_value(REPLACE_COLLECTION);
        skills.extend(i32_value(1));
        skills.extend(i32_value(3_982));
        skills.extend(lucy);
        let professions = object(vec![(1, i32_value(11)), (7, skills)]);

        let update = decode_character_update(&object(vec![(61, professions)]), 1).unwrap();

        assert!(update.has_loadout_fields());
        assert_eq!(
            update.battle_imagine_skills,
            Some(DirtyBattleImagineSkillUpdate {
                replace: true,
                upserts: vec![DirtyBattleImagineSkillEntry {
                    map_key: 3_982,
                    skill_id: Some(3_982),
                    level: Some(60),
                    replacement_skill_ids: Some(vec![4_001, 4_002]),
                    remodel_level: Some(4),
                    current_skin_id: Some(71),
                    unlocked_skin_ids: Some(DirtyEnabledIdUpdate {
                        replace: true,
                        upserts: vec![(71, true), (72, false)],
                        removals: Vec::new(),
                    }),
                }],
                removals: Vec::new(),
            })
        );
    }

    #[test]
    fn battle_imagine_skill_delta_preserves_partial_tier_update_and_removal() {
        let lucy_tier = object(vec![(4, i32_value(5))]);
        let mut skills = i32_value(0); // additions
        skills.extend(i32_value(1)); // removals
        skills.extend(i32_value(1)); // updates
        skills.extend(i32_value(3_969));
        skills.extend(i32_value(3_982));
        skills.extend(lucy_tier);
        let professions = object(vec![(7, skills)]);

        let update = decode_character_update(&object(vec![(61, professions)]), 1).unwrap();

        assert_eq!(
            update.battle_imagine_skills,
            Some(DirtyBattleImagineSkillUpdate {
                replace: false,
                upserts: vec![DirtyBattleImagineSkillEntry {
                    map_key: 3_982,
                    remodel_level: Some(5),
                    ..DirtyBattleImagineSkillEntry::default()
                }],
                removals: vec![3_969],
            })
        );
    }

    #[test]
    fn user_fight_attr_preserves_every_exact_cooldown_field() {
        let cooldown = object(vec![
            (1, i32_value(3921)),
            (2, i64_value(1_234_567)),
            (3, i32_value(60_000)),
            (4, u32_value(2)),
            (6, i64_value(1_230_000)),
            (7, i32_value(3)),
            (8, i32_value(58_000)),
            (9, i32_value(5_000)),
            (10, i64_value(2_000)),
            (11, i32_value(750)),
        ]);
        let fight_attributes = object(vec![(9, cooldown)]);
        let update = decode_character_update(&object(vec![(16, fight_attributes)]), 1).unwrap();

        assert_eq!(update.root_fields, vec![16]);
        assert_eq!(
            update.cooldowns,
            vec![DirtySkillCooldownUpdate {
                skill_level_id: Some(3921),
                skill_begin_time: Some(1_234_567),
                duration: Some(60_000),
                cooldown_type: Some(2),
                profession_hold_begin_time: Some(1_230_000),
                charge_count: Some(3),
                valid_cooldown_time: Some(58_000),
                sub_cooldown_ratio: Some(5_000),
                sub_cooldown_fixed: Some(2_000),
                accelerate_cooldown_ratio: Some(750),
            }]
        );
        assert!(!update.has_public_profile_fields());
    }

    #[test]
    fn user_fight_attr_preserves_energy_and_parallel_resource_arrays() {
        let mut resource_ids = i32_value(3);
        resource_ids.extend(u32_value(11));
        resource_ids.extend(u32_value(22));
        resource_ids.extend(u32_value(u32::MAX));
        let mut resource_values = i32_value(2);
        resource_values.extend(u32_value(101));
        resource_values.extend(u32_value(202));
        let fight_attributes = object(vec![
            (3, 12.5_f32.to_le_bytes().to_vec()),
            (4, resource_ids),
            (5, resource_values),
        ]);

        let update = decode_character_update(&object(vec![(16, fight_attributes)]), 1).unwrap();

        assert_eq!(update.origin_energy_raw_bits, Some(12.5_f32.to_bits()));
        assert_eq!(update.resource_ids, vec![11, 22, u32::MAX]);
        assert_eq!(update.resource_values, vec![101, 202]);
    }

    #[test]
    fn public_profile_and_world_fields_are_decoded_without_private_strings() {
        let avatar = object(vec![
            (1, i32_value(42)),
            (4, i32_value(8)),
            (5, i32_value(9)),
        ]);
        let base = object(vec![
            (1, i64_value(987_654)),
            (2, string_value("private-account-value")),
            (3, i64_value(123_456)),
            (4, u32_value(7)),
            (5, string_value("Profile Name")),
            (6, i32_value(2)),
            (22, i32_value(1)),
            (25, avatar),
            (27, string_value("private-open-id-value")),
            (31, i32_value(5)),
            (35, i32_value(42_000)),
        ]);
        let scene = object(vec![
            (1, u32_value(8)),
            (2, u32_value(4)),
            (6, u32_value(9)),
            (13, string_value("scene-instance")),
            (14, string_value("dungeon-instance")),
            (15, u32_value(5)),
        ]);
        let level = object(vec![
            (1, i32_value(60)),
            (2, i64_value(12_345)),
            (11, i32_value(55)),
        ]);
        let profession = object(vec![(1, i32_value(6))]);
        let fight_point = object(vec![(1, i32_value(43_000))]);
        let update = decode_character_update(
            &object(vec![
                (1, i32_value(987_654)),
                (2, base),
                (3, scene),
                (22, level),
                (61, profession),
                (96, fight_point),
            ]),
            1,
        )
        .unwrap();

        assert_eq!(update.character_id, Some(987_654));
        assert_eq!(update.display_id, Some(123_456));
        assert_eq!(update.server_id, Some(7));
        assert_eq!(update.display_name.as_deref(), Some("Profile Name"));
        assert_eq!(update.class_id, Some(6));
        assert_eq!(update.level, Some(60));
        assert_eq!(update.current_experience, Some(12_345));
        assert_eq!(update.previous_season_max_level, Some(55));
        assert_eq!(update.combat_power, Some(43_000));
        assert_eq!(update.avatar_id, Some(42));
        assert_eq!(update.world.map_id, Some(8));
        assert_eq!(update.world.line_id, Some(5));
        assert_eq!(
            update.world.scene_instance_id.as_deref(),
            Some("scene-instance")
        );
        let debug = format!("{update:?}");
        assert!(!debug.contains("private-account-value"));
        assert!(!debug.contains("private-open-id-value"));
    }

    #[test]
    fn malformed_or_unsupported_blobs_are_rejected() {
        assert_eq!(
            decode_character_update(&object(Vec::new()), 2),
            Err(DirtyBlobError::UnsupportedStreamType(2))
        );
        assert!(matches!(
            decode_character_update(&[0xfe, 0xff], 1),
            Err(DirtyBlobError::Truncated)
        ));
        let mut invalid_size = i32_value(OBJECT_BEGIN);
        invalid_size.extend(i32_value(i32::MAX));
        assert_eq!(
            decode_character_update(&invalid_size, 1),
            Err(DirtyBlobError::InvalidObjectSize)
        );
    }
}
