//! Selective, bounded decoder for `SyncDungeonDirtyData`.
//!
//! This parser is intentionally available before the route is enabled in an
//! exact-build protocol pack. Synthetic fixtures can verify the implementation,
//! but only a reviewed current-build dungeon capture can promote the Global
//! route from opaque to allowed.

use rlogs_events::{DungeonFlowPhase, DungeonFlowSnapshot};

use crate::dirty_blob_v1::{BlobReader, DirtyBlobError};

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct DirtyDungeonPatch {
    pub scene_uuid: Option<u32>,
    pub flow: Option<DungeonFlowSnapshot>,
    pub objectives: Vec<DirtyDungeonObjectiveMutation>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum DirtyDungeonObjectiveMutation {
    Upsert {
        map_key: i32,
        target_id: Option<i32>,
        value: Option<i32>,
        complete: Option<i32>,
    },
    Remove {
        map_key: i32,
    },
}

pub(crate) fn decode_dungeon_update(
    bytes: &[u8],
    stream_type: i32,
) -> Result<DirtyDungeonPatch, DirtyBlobError> {
    let mut reader = BlobReader::for_stream(bytes, stream_type)?;
    let root_end = reader.begin_object()?;
    let mut patch = DirtyDungeonPatch::default();

    while let Some(field) = reader.next_field(root_end)? {
        match field {
            1 => patch.scene_uuid = Some(reader.read_u32()?),
            2 => patch.flow = parse_flow(&mut reader)?,
            4 => patch.objectives = parse_targets(&mut reader)?,
            // Unknown root fields cannot be skipped without their schema.
            // Abandon this bounded object instead of guessing a field width.
            _ => reader.skip_object_body(root_end),
        }
    }
    reader.finish_object(root_end)?;
    if !reader.is_finished() {
        return Err(DirtyBlobError::TrailingBytes);
    }
    Ok(patch)
}

fn parse_flow(reader: &mut BlobReader<'_>) -> Result<Option<DungeonFlowSnapshot>, DirtyBlobError> {
    let end = reader.begin_object()?;
    let mut flow = DungeonFlowSnapshot::default();
    while let Some(field) = reader.next_field(end)? {
        match field {
            1 => {
                let state_id = reader.read_i32()?;
                flow.state_id = Some(state_id);
                flow.phase = Some(DungeonFlowPhase::from_protocol_id(state_id));
            }
            2 => flow.active_time_raw = Some(reader.read_i32()?),
            3 => flow.ready_time_raw = Some(reader.read_i32()?),
            4 => flow.play_time_raw = Some(reader.read_i32()?),
            5 => flow.end_time_raw = Some(reader.read_i32()?),
            6 => flow.settlement_time_raw = Some(reader.read_i32()?),
            7 => flow.dungeon_times_raw = Some(reader.read_i32()?),
            8 => flow.result_id = Some(reader.read_i32()?),
            _ => reader.skip_object_body(end),
        }
    }
    reader.finish_object(end)?;
    Ok(flow.has_evidence().then_some(flow))
}

fn parse_targets(
    reader: &mut BlobReader<'_>,
) -> Result<Vec<DirtyDungeonObjectiveMutation>, DirtyBlobError> {
    let end = reader.begin_object()?;
    let mut mutations = Vec::new();
    while let Some(field) = reader.next_field(end)? {
        match field {
            1 => mutations = parse_target_map(reader)?,
            _ => reader.skip_object_body(end),
        }
    }
    reader.finish_object(end)?;
    Ok(mutations)
}

fn parse_target_map(
    reader: &mut BlobReader<'_>,
) -> Result<Vec<DirtyDungeonObjectiveMutation>, DirtyBlobError> {
    let counts = reader.read_map_counts()?;
    let mut mutations = Vec::with_capacity(
        counts
            .add
            .saturating_add(counts.remove)
            .saturating_add(counts.update),
    );
    for _ in 0..counts.add {
        let map_key = reader.read_i32()?;
        mutations.push(parse_target_upsert(reader, map_key)?);
    }
    for _ in 0..counts.remove {
        mutations.push(DirtyDungeonObjectiveMutation::Remove {
            map_key: reader.read_i32()?,
        });
    }
    for _ in 0..counts.update {
        let map_key = reader.read_i32()?;
        mutations.push(parse_target_upsert(reader, map_key)?);
    }
    Ok(mutations)
}

fn parse_target_upsert(
    reader: &mut BlobReader<'_>,
    map_key: i32,
) -> Result<DirtyDungeonObjectiveMutation, DirtyBlobError> {
    let end = reader.begin_object()?;
    let mut target_id = None;
    let mut value = None;
    let mut complete = None;
    while let Some(field) = reader.next_field(end)? {
        match field {
            1 => target_id = Some(reader.read_i32()?),
            2 => value = Some(reader.read_i32()?),
            3 => complete = Some(reader.read_i32()?),
            _ => reader.skip_object_body(end),
        }
    }
    reader.finish_object(end)?;
    Ok(DirtyDungeonObjectiveMutation::Upsert {
        map_key,
        target_id,
        value,
        complete,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const OBJECT_BEGIN: i32 = -2;
    const OBJECT_END: i32 = -3;

    fn scalar(value: i32) -> Vec<u8> {
        let mut bytes = value.to_le_bytes().to_vec();
        bytes.extend_from_slice(&[0; 4]);
        bytes
    }

    fn object(fields: Vec<(i32, Vec<u8>)>) -> Vec<u8> {
        let mut body = Vec::new();
        for (field, value) in fields {
            body.extend(scalar(field));
            body.extend(value);
        }
        let mut bytes = scalar(OBJECT_BEGIN);
        bytes.extend(scalar(i32::try_from(body.len()).unwrap()));
        bytes.extend(body);
        bytes.extend(scalar(OBJECT_END));
        bytes
    }

    #[test]
    fn safe_dirty_patch_retains_every_flow_field_and_map_operation() {
        let flow = object(vec![
            (1, scalar(3)),
            (2, scalar(10)),
            (3, scalar(20)),
            (4, scalar(30)),
            (5, scalar(40)),
            (6, scalar(50)),
            (7, scalar(2)),
            (8, scalar(91)),
        ]);
        let add = object(vec![(1, scalar(700)), (2, scalar(4)), (3, scalar(0))]);
        let update = object(vec![(2, scalar(5)), (3, scalar(1))]);
        let mut map = scalar(1);
        map.extend(scalar(1));
        map.extend(scalar(1));
        map.extend(scalar(10));
        map.extend(add);
        map.extend(scalar(11));
        map.extend(scalar(12));
        map.extend(update);
        let targets = object(vec![(1, map)]);
        let patch = object(vec![(1, scalar(1234)), (2, flow), (4, targets)]);

        let decoded = decode_dungeon_update(&patch, 0).unwrap();
        assert_eq!(decoded.scene_uuid, Some(1234));
        assert_eq!(
            decoded.flow,
            Some(DungeonFlowSnapshot {
                state_id: Some(3),
                phase: Some(DungeonFlowPhase::Playing),
                active_time_raw: Some(10),
                ready_time_raw: Some(20),
                play_time_raw: Some(30),
                end_time_raw: Some(40),
                settlement_time_raw: Some(50),
                dungeon_times_raw: Some(2),
                result_id: Some(91),
            })
        );
        assert_eq!(
            decoded.objectives,
            vec![
                DirtyDungeonObjectiveMutation::Upsert {
                    map_key: 10,
                    target_id: Some(700),
                    value: Some(4),
                    complete: Some(0),
                },
                DirtyDungeonObjectiveMutation::Remove { map_key: 11 },
                DirtyDungeonObjectiveMutation::Upsert {
                    map_key: 12,
                    target_id: None,
                    value: Some(5),
                    complete: Some(1),
                },
            ]
        );
    }

    #[test]
    fn malformed_and_unsupported_dirty_patches_fail_closed() {
        assert_eq!(
            decode_dungeon_update(&[0; 4], 0),
            Err(DirtyBlobError::Truncated)
        );
        assert_eq!(
            decode_dungeon_update(&[], 2),
            Err(DirtyBlobError::UnsupportedStreamType(2))
        );
    }
}
