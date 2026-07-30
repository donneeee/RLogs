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
    pub world: DirtyWorldUpdate,
    pub root_fields: Vec<i32>,
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
pub(crate) enum DirtyBlobError {
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
    if bytes.len() > MAX_BLOB_BYTES {
        return Err(DirtyBlobError::BlobTooLarge);
    }
    let stream_safe = match stream_type {
        0 => true,
        1 => false,
        other => return Err(DirtyBlobError::UnsupportedStreamType(other)),
    };
    let mut reader = BlobReader::new(bytes, stream_safe);
    let root_end = reader.begin_object()?;
    let mut update = DirtyCharacterUpdate::default();

    while let Some(field) = reader.next_field(root_end)? {
        update.root_fields.push(field);
        match field {
            1 => update.character_id = Some(i64::from(reader.read_i32()?)),
            2 => parse_character_base(&mut reader, &mut update)?,
            3 => parse_scene(&mut reader, &mut update.world)?,
            8 | 16 | 49 | 102 | 116 => reader.skip_object()?,
            22 => parse_role_level(&mut reader, &mut update)?,
            61 => parse_profession_list(&mut reader, &mut update)?,
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
            // Later fields contain nested maps/lists and are not needed to
            // establish the current public class.
            _ => reader.skip_object_body(end),
        }
    }
    reader.finish_object(end)
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

struct BlobReader<'a> {
    bytes: &'a [u8],
    offset: usize,
    stream_safe: bool,
}

impl<'a> BlobReader<'a> {
    fn new(bytes: &'a [u8], stream_safe: bool) -> Self {
        Self {
            bytes,
            offset: 0,
            stream_safe,
        }
    }

    fn is_finished(&self) -> bool {
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

    fn read_i32(&mut self) -> Result<i32, DirtyBlobError> {
        Ok(i32::from_le_bytes(
            self.read_raw(4)?
                .try_into()
                .expect("bounded four-byte slice"),
        ))
    }

    fn read_u32(&mut self) -> Result<u32, DirtyBlobError> {
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

    fn begin_object(&mut self) -> Result<usize, DirtyBlobError> {
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

    fn next_field(&mut self, object_end: usize) -> Result<Option<i32>, DirtyBlobError> {
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

    fn skip_object_body(&mut self, object_end: usize) {
        self.offset = object_end;
    }

    fn finish_object(&mut self, object_end: usize) -> Result<(), DirtyBlobError> {
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
        let count = self.read_collection_count()?;
        for _ in 0..count {
            self.read_i32()?;
        }
        Ok(())
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
        let counts = self.read_map_counts()?;
        for _ in 0..counts.add {
            self.read_i32()?;
            self.read_u8()?;
        }
        for _ in 0..counts.remove {
            self.read_i32()?;
        }
        for _ in 0..counts.update {
            self.read_i32()?;
            self.read_u8()?;
        }
        Ok(())
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

    fn read_map_counts(&mut self) -> Result<MapCounts, DirtyBlobError> {
        let first = self.read_i32()?;
        if first == EMPTY_COLLECTION {
            return Ok(MapCounts::default());
        }
        let (add, remove, update) = if first == REPLACE_COLLECTION {
            (self.read_i32()?, 0, 0)
        } else {
            (first, self.read_i32()?, self.read_i32()?)
        };
        let counts = MapCounts {
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
struct MapCounts {
    add: usize,
    remove: usize,
    update: usize,
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
