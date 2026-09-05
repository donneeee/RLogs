//! Strict decoder for the current-build gameplay skill-attribute envelope.
//!
//! The client attaches this payload to `World.UseSlot` method `0x3D002` when
//! `UseSlotType` is `Skill`. It contains only action-time gameplay telemetry:
//! a timestamp, velocity, and attack/cast/charge speed snapshots. It does not
//! contain login, password, account-authentication, or character-profile data.

use aes::Aes128;
use cbc::cipher::{BlockDecryptMut, KeyIvInit, block_padding::Pkcs7};
use hmac::{Hmac, Mac};
use rlogs_events::{AbilityId, ActionTimingSnapshot, CastEvent, CastState, EntityRef, EntityUuid};
use serde::Serialize;
use sha2::Sha256;
use thiserror::Error;

type Aes128CbcDecryptor = cbc::Decryptor<Aes128>;
type HmacSha256 = Hmac<Sha256>;

/// First exact Steam client build from which this contract and its
/// gameplay-only keys were recovered.
pub const BPSR_USE_SKILL_ATTR_BUILD: &str = "24609362";

/// Current Steam client build whose native protobuf branches, `World.UseSlot`
/// route, and authenticated-envelope keys were independently verified.
pub const BPSR_CURRENT_USE_SKILL_ATTR_BUILD: &str = "24687926";

const BPSR_SUPPORTED_USE_SKILL_ATTR_BUILDS: [&str; 2] =
    [BPSR_USE_SKILL_ATTR_BUILD, BPSR_CURRENT_USE_SKILL_ATTR_BUILD];
const BPSR_SUPPORTED_USE_SKILL_ATTR_BUILD_LABEL: &str = "24609362 or 24687926";

const IV_LENGTH: usize = 16;
const MAC_LENGTH: usize = 32;
const AES_BLOCK_LENGTH: usize = 16;
const ENVELOPE_PREFIX_LENGTH: usize = IV_LENGTH + MAC_LENGTH;

// Panda.ZGame.ZBattleUtils.skillKey_. This key authenticates no account data;
// it decrypts only the five gameplay fields documented above.
const SKILL_AES_KEY: [u8; 16] = [
    0x3d, 0x09, 0xd6, 0x69, 0x1d, 0xd9, 0x7a, 0x7c, 0xf9, 0xae, 0x12, 0x2c, 0x06, 0xef, 0x3c, 0x84,
];

// Panda.ZGame.ZBattleUtils.skillMacKey_.
const SKILL_HMAC_KEY: [u8; 32] = [
    0x24, 0x4b, 0x9a, 0xab, 0x92, 0x60, 0x5e, 0xbd, 0xf4, 0x6b, 0x7f, 0x32, 0x1a, 0x18, 0x70, 0x9a,
    0xff, 0x63, 0x3c, 0x03, 0x86, 0x30, 0xb7, 0xea, 0xc2, 0xbc, 0x95, 0xec, 0xd6, 0xa9, 0xc3, 0x36,
];

/// Exact action-time values serialized by
/// `Panda.ZGame.ZBattleUtils.buildUseSkillAttrPlaintext`.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct UseSkillAttributes {
    /// Protobuf field 1, client action timestamp.
    pub timestamp: u64,
    /// Protobuf field 2, fixed32 client velocity.
    pub velocity: f32,
    /// Protobuf field 3, attack-speed percentage in client fixed-point units.
    pub attack_speed_pct: i32,
    /// Protobuf field 4, cast-speed percentage in client fixed-point units.
    pub cast_speed_pct: i32,
    /// Protobuf field 5, charge-speed percentage in client fixed-point units.
    pub charge_speed_pct: i32,
}

/// Exact four-float `Zproto.Position` value embedded in `UseSkillParam`.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct UseSkillPosition {
    pub x: f32,
    pub y: f32,
    pub z: f32,
    pub direction_radians: f32,
}

/// Exact action identity and target fields serialized in
/// `UseSlotRequest.extra_data` for a skill use.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct UseSkillParamSnapshot {
    pub skill_uuid: i32,
    pub skill_id: i32,
    pub skill_level: i32,
    pub begin_time: i64,
    pub target_uuid: i64,
    pub target_position: UseSkillPosition,
    pub current_position: UseSkillPosition,
    pub target_part_id: i32,
    pub target_part_position: UseSkillPosition,
    pub is_passive: bool,
    pub is_activate_roulette: bool,
}

/// One exact local skill-use snapshot from `World.UseSlot` method `0x3D002`.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct UseSkillActionSnapshot {
    pub slot_id: i32,
    pub param: UseSkillParamSnapshot,
    pub attributes: Option<UseSkillAttributes>,
}

impl UseSkillActionSnapshot {
    /// Converts the exact current-build BPSR request into the compact
    /// game-neutral action-time snapshot stored on canonical cast events.
    ///
    /// This conversion does not authorize the client route or infer a server
    /// cast. It only preserves already decoded gameplay values without loss.
    pub fn canonical_action_timing(self) -> Option<ActionTimingSnapshot> {
        let attributes = self.attributes?;
        Some(ActionTimingSnapshot {
            action_instance_id: i64::from(self.param.skill_uuid),
            base_ability: AbilityId(i64::from(self.param.skill_id)),
            ability_level: self.param.skill_level,
            slot_id: self.slot_id,
            client_timestamp_raw: attributes.timestamp,
            begin_time_raw: self.param.begin_time,
            attack_speed_basis_points: attributes.attack_speed_pct,
            cast_speed_basis_points: attributes.cast_speed_pct,
            charge_speed_basis_points: attributes.charge_speed_pct,
            passive: self.param.is_passive,
            activated_roulette: self.param.is_activate_roulette,
            target_part_id: self.param.target_part_id,
        })
    }

    /// Builds the canonical local action-start event once the caller has
    /// resolved the implicit local source and exact packet target through the
    /// bounded entity registry.
    ///
    /// A positive packet target must resolve to that exact entity UUID. The
    /// conversion returns `None` rather than emitting a guessed or mismatched
    /// target. This helper is intentionally not registered to a live route;
    /// matching-build packet replay and client/server action correlation must
    /// succeed first.
    pub fn canonical_cast_started(
        self,
        source: EntityRef,
        target: Option<EntityRef>,
    ) -> Option<CastEvent> {
        match (self.param.target_uuid, target) {
            (packet_target, Some(resolved))
                if packet_target > 0 && resolved.entity_uuid == EntityUuid(packet_target) => {}
            (packet_target, None) if packet_target <= 0 => {}
            _ => return None,
        }
        Some(CastEvent {
            source,
            ability: AbilityId(i64::from(self.param.skill_id)),
            target,
            state: CastState::Started,
            action_timing: self.canonical_action_timing(),
        })
    }
}

/// Exact current-build `World.Types.SyncSkillStageTrigger` request.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct ClientSkillStageTriggerSnapshot {
    pub trigger_type: i32,
    pub time: i64,
    pub skill_uuid: i32,
}

/// Exact current-build `World.Types.ClientStageEnd` request.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct ClientSkillStageEndSnapshot {
    pub current_stage_index: i32,
    pub next_stage_index: i32,
    pub time: i64,
    pub condition_id: i32,
    pub skill_uuid: i32,
    pub trigger_index: i32,
}

/// Exact current-build `WorldNtf.Types.SyncServerSkillStageEnd` notification.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct ServerSkillStageEndSnapshot {
    pub skill_uuid: i32,
    pub stage_id: u32,
    pub new_stage_id: u32,
    pub condition_id: u32,
}

#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum UseSkillAttrDecodeError {
    #[error("unsupported BPSR skill-attribute build {observed}; expected {expected}")]
    UnsupportedBuild {
        observed: String,
        expected: &'static str,
    },
    #[error("skill-attribute envelope is too short: {actual} bytes")]
    EnvelopeTooShort { actual: usize },
    #[error("skill-attribute ciphertext length {actual} is not a non-zero AES block multiple")]
    InvalidCiphertextLength { actual: usize },
    #[error("skill-attribute HMAC-SHA256 verification failed")]
    MacMismatch,
    #[error("skill-attribute AES-CBC PKCS#7 padding is invalid")]
    InvalidPadding,
    #[error("skill-attribute plaintext ended in the middle of a field")]
    TruncatedPlaintext,
    #[error("skill-attribute protobuf varint overflows 64 bits")]
    VarintOverflow,
    #[error("skill-attribute field {field} uses wire type {observed}, expected {expected}")]
    WrongWireType {
        field: u32,
        observed: u8,
        expected: u8,
    },
    #[error("skill-attribute field {field} appears more than once")]
    DuplicateField { field: u32 },
    #[error("skill-attribute plaintext contains unsupported field {field}")]
    UnknownField { field: u32 },
    #[error("skill-attribute protobuf tag is invalid")]
    InvalidTag,
    #[error("skill-attribute int32 field {field} is outside the protobuf int32 domain")]
    Int32Overflow { field: u32 },
}

#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum UseSkillActionDecodeError {
    #[error(transparent)]
    AttributeEnvelope(#[from] UseSkillAttrDecodeError),
    #[error("{message} ended in the middle of a field")]
    Truncated { message: &'static str },
    #[error("{message} protobuf varint overflows 64 bits")]
    VarintOverflow { message: &'static str },
    #[error("{message} field {field} uses wire type {observed}, expected {expected}")]
    WrongWireType {
        message: &'static str,
        field: u32,
        observed: u8,
        expected: u8,
    },
    #[error("{message} field {field} appears more than once")]
    DuplicateField { message: &'static str, field: u32 },
    #[error("{message} contains unsupported field {field}")]
    UnknownField { message: &'static str, field: u32 },
    #[error("{message} protobuf tag is invalid")]
    InvalidTag { message: &'static str },
    #[error("{message} int32 field {field} is outside the protobuf int32 domain")]
    Int32Overflow { message: &'static str, field: u32 },
    #[error("{message} uint32 field {field} is outside the protobuf uint32 domain")]
    Uint32Overflow { message: &'static str, field: u32 },
    #[error("{message} is missing required field {field}")]
    MissingField { message: &'static str, field: u32 },
}

/// Verifies, decrypts, and strictly decodes one reviewed-build `AttrData`
/// envelope using caller-owned reusable storage.
///
/// The HMAC is verified before any CBC decryption. `scratch` is reused across
/// calls and scrubbed before return, so the hot path performs no allocation
/// after it has reached the largest observed ciphertext size. Unknown or
/// duplicate fields fail closed to make client schema drift visible.
pub fn decode_use_skill_attr_into(
    game_build: &str,
    envelope: &[u8],
    scratch: &mut Vec<u8>,
) -> Result<UseSkillAttributes, UseSkillAttrDecodeError> {
    if !BPSR_SUPPORTED_USE_SKILL_ATTR_BUILDS.contains(&game_build) {
        return Err(UseSkillAttrDecodeError::UnsupportedBuild {
            observed: game_build.to_owned(),
            expected: BPSR_SUPPORTED_USE_SKILL_ATTR_BUILD_LABEL,
        });
    }
    if envelope.len() < ENVELOPE_PREFIX_LENGTH + AES_BLOCK_LENGTH {
        return Err(UseSkillAttrDecodeError::EnvelopeTooShort {
            actual: envelope.len(),
        });
    }

    let iv = &envelope[..IV_LENGTH];
    let expected_mac = &envelope[IV_LENGTH..ENVELOPE_PREFIX_LENGTH];
    let ciphertext = &envelope[ENVELOPE_PREFIX_LENGTH..];
    if ciphertext.is_empty() || ciphertext.len() % AES_BLOCK_LENGTH != 0 {
        return Err(UseSkillAttrDecodeError::InvalidCiphertextLength {
            actual: ciphertext.len(),
        });
    }

    let mut mac = <HmacSha256 as Mac>::new_from_slice(&SKILL_HMAC_KEY)
        .expect("HMAC-SHA256 accepts a 32-byte key");
    mac.update(iv);
    mac.update(ciphertext);
    mac.verify_slice(expected_mac)
        .map_err(|_| UseSkillAttrDecodeError::MacMismatch)?;

    scratch.clear();
    scratch.extend_from_slice(ciphertext);
    let plaintext_length = match Aes128CbcDecryptor::new_from_slices(&SKILL_AES_KEY, iv)
        .expect("AES-128 CBC accepts fixed-size key and IV")
        .decrypt_padded_mut::<Pkcs7>(scratch)
    {
        Ok(plaintext) => plaintext.len(),
        Err(_) => {
            scratch.fill(0);
            scratch.clear();
            return Err(UseSkillAttrDecodeError::InvalidPadding);
        }
    };

    let decoded = decode_plaintext(&scratch[..plaintext_length]);
    scratch.fill(0);
    scratch.clear();
    decoded
}

/// Strictly decodes the current-build `World.Types.UseSlot` request and, when
/// it represents a skill use, returns the exact action identity, target, and
/// optional authenticated action-time speed snapshot.
///
/// Other `UseSlotType` values share the RPC and return `Ok(None)`. The borrowed
/// protobuf slices and reusable decrypt scratch keep this boundary allocation
/// free after the scratch capacity has warmed up.
pub fn decode_world_use_slot_skill_action_into(
    game_build: &str,
    payload: &[u8],
    scratch: &mut Vec<u8>,
) -> Result<Option<UseSkillActionSnapshot>, UseSkillActionDecodeError> {
    if !BPSR_SUPPORTED_USE_SKILL_ATTR_BUILDS.contains(&game_build) {
        return Err(UseSkillAttrDecodeError::UnsupportedBuild {
            observed: game_build.to_owned(),
            expected: BPSR_SUPPORTED_USE_SKILL_ATTR_BUILD_LABEL,
        }
        .into());
    }

    let request = decode_world_use_slot(payload)?;
    if request.use_type != 1 {
        return Ok(None);
    }
    let extra_data = request
        .extra_data
        .ok_or(UseSkillActionDecodeError::MissingField {
            message: "Zproto.UseSlotRequest",
            field: 3,
        })?;
    let attributes = match request.attr_data {
        Some(attr_data) => Some(decode_use_skill_attr_into(game_build, attr_data, scratch)?),
        None => None,
    };

    Ok(Some(UseSkillActionSnapshot {
        slot_id: request.slot_id,
        param: decode_use_skill_param(extra_data)?,
        attributes,
    }))
}

/// Strictly decodes one current-build local stage-trigger request.
pub fn decode_client_skill_stage_trigger(
    raw: &[u8],
) -> Result<ClientSkillStageTriggerSnapshot, UseSkillActionDecodeError> {
    const MESSAGE: &str = "Zproto.World.Types.SyncSkillStageTrigger";
    let mut cursor = 0;
    let mut trigger_type = None;
    let mut time = None;
    let mut skill_uuid = None;
    while cursor < raw.len() {
        let (field, wire_type) = decode_action_tag(raw, &mut cursor, MESSAGE)?;
        require_action_wire_type(MESSAGE, field, wire_type, 0)?;
        match field {
            1 => set_action_once(
                &mut trigger_type,
                decode_action_int32(raw, &mut cursor, MESSAGE, field)?,
                MESSAGE,
                field,
            )?,
            2 => set_action_once(
                &mut time,
                decode_action_varint(raw, &mut cursor, MESSAGE)? as i64,
                MESSAGE,
                field,
            )?,
            3 => set_action_once(
                &mut skill_uuid,
                decode_action_int32(raw, &mut cursor, MESSAGE, field)?,
                MESSAGE,
                field,
            )?,
            _ => {
                return Err(UseSkillActionDecodeError::UnknownField {
                    message: MESSAGE,
                    field,
                });
            }
        }
    }
    Ok(ClientSkillStageTriggerSnapshot {
        trigger_type: trigger_type.unwrap_or_default(),
        time: time.unwrap_or_default(),
        skill_uuid: skill_uuid.unwrap_or_default(),
    })
}

/// Strictly decodes one current-build local stage-end request.
pub fn decode_client_skill_stage_end(
    raw: &[u8],
) -> Result<ClientSkillStageEndSnapshot, UseSkillActionDecodeError> {
    const MESSAGE: &str = "Zproto.World.Types.ClientStageEnd";
    let mut cursor = 0;
    let mut values = [None; 6];
    while cursor < raw.len() {
        let (field, wire_type) = decode_action_tag(raw, &mut cursor, MESSAGE)?;
        require_action_wire_type(MESSAGE, field, wire_type, 0)?;
        if !(1..=6).contains(&field) {
            return Err(UseSkillActionDecodeError::UnknownField {
                message: MESSAGE,
                field,
            });
        }
        let value = if field == 3 {
            decode_action_varint(raw, &mut cursor, MESSAGE)? as i64
        } else {
            i64::from(decode_action_int32(raw, &mut cursor, MESSAGE, field)?)
        };
        set_action_once(&mut values[field as usize - 1], value, MESSAGE, field)?;
    }
    Ok(ClientSkillStageEndSnapshot {
        current_stage_index: values[0].unwrap_or_default() as i32,
        next_stage_index: values[1].unwrap_or_default() as i32,
        time: values[2].unwrap_or_default(),
        condition_id: values[3].unwrap_or_default() as i32,
        skill_uuid: values[4].unwrap_or_default() as i32,
        trigger_index: values[5].unwrap_or_default() as i32,
    })
}

/// Strictly decodes one current-build server-selected stage transition.
pub fn decode_server_skill_stage_end(
    raw: &[u8],
) -> Result<ServerSkillStageEndSnapshot, UseSkillActionDecodeError> {
    const OUTER: &str = "Zproto.WorldNtf.Types.SyncServerSkillStageEnd";
    const INNER: &str = "Zproto.ServerSkillStageEnd";
    let mut cursor = 0;
    let mut inner = None;
    while cursor < raw.len() {
        let (field, wire_type) = decode_action_tag(raw, &mut cursor, OUTER)?;
        if field != 1 {
            return Err(UseSkillActionDecodeError::UnknownField {
                message: OUTER,
                field,
            });
        }
        require_action_wire_type(OUTER, field, wire_type, 2)?;
        let value = decode_length_delimited(raw, &mut cursor, OUTER)?;
        set_action_once(&mut inner, value, OUTER, field)?;
    }
    let raw = inner.ok_or(UseSkillActionDecodeError::MissingField {
        message: OUTER,
        field: 1,
    })?;
    let mut cursor = 0;
    let mut skill_uuid = None;
    let mut stage_id = None;
    let mut new_stage_id = None;
    let mut condition_id = None;
    while cursor < raw.len() {
        let (field, wire_type) = decode_action_tag(raw, &mut cursor, INNER)?;
        require_action_wire_type(INNER, field, wire_type, 0)?;
        match field {
            1 => set_action_once(
                &mut skill_uuid,
                decode_action_int32(raw, &mut cursor, INNER, field)?,
                INNER,
                field,
            )?,
            2 => set_action_once(
                &mut stage_id,
                decode_action_uint32(raw, &mut cursor, INNER, field)?,
                INNER,
                field,
            )?,
            3 => set_action_once(
                &mut new_stage_id,
                decode_action_uint32(raw, &mut cursor, INNER, field)?,
                INNER,
                field,
            )?,
            4 => set_action_once(
                &mut condition_id,
                decode_action_uint32(raw, &mut cursor, INNER, field)?,
                INNER,
                field,
            )?,
            _ => {
                return Err(UseSkillActionDecodeError::UnknownField {
                    message: INNER,
                    field,
                });
            }
        }
    }
    Ok(ServerSkillStageEndSnapshot {
        skill_uuid: skill_uuid.unwrap_or_default(),
        stage_id: stage_id.unwrap_or_default(),
        new_stage_id: new_stage_id.unwrap_or_default(),
        condition_id: condition_id.unwrap_or_default(),
    })
}

#[derive(Debug, Clone, Copy)]
struct BorrowedUseSlotRequest<'a> {
    slot_id: i32,
    use_type: i32,
    extra_data: Option<&'a [u8]>,
    attr_data: Option<&'a [u8]>,
}

fn decode_world_use_slot(
    raw: &[u8],
) -> Result<BorrowedUseSlotRequest<'_>, UseSkillActionDecodeError> {
    const MESSAGE: &str = "Zproto.World.Types.UseSlot";
    let mut cursor = 0;
    let mut request = None;
    while cursor < raw.len() {
        let (field, wire_type) = decode_action_tag(raw, &mut cursor, MESSAGE)?;
        match field {
            1 => {
                require_action_wire_type(MESSAGE, field, wire_type, 2)?;
                let value = decode_length_delimited(raw, &mut cursor, MESSAGE)?;
                set_action_once(&mut request, value, MESSAGE, field)?;
            }
            _ => {
                return Err(UseSkillActionDecodeError::UnknownField {
                    message: MESSAGE,
                    field,
                });
            }
        }
    }
    let request = request.ok_or(UseSkillActionDecodeError::MissingField {
        message: MESSAGE,
        field: 1,
    })?;
    decode_use_slot_request(request)
}

fn decode_use_slot_request(
    raw: &[u8],
) -> Result<BorrowedUseSlotRequest<'_>, UseSkillActionDecodeError> {
    const MESSAGE: &str = "Zproto.UseSlotRequest";
    let mut cursor = 0;
    let mut slot_id = None;
    let mut use_type = None;
    let mut extra_data = None;
    let mut attr_data = None;
    while cursor < raw.len() {
        let (field, wire_type) = decode_action_tag(raw, &mut cursor, MESSAGE)?;
        match field {
            1 => {
                require_action_wire_type(MESSAGE, field, wire_type, 0)?;
                let value = decode_action_int32(raw, &mut cursor, MESSAGE, field)?;
                set_action_once(&mut slot_id, value, MESSAGE, field)?;
            }
            2 => {
                require_action_wire_type(MESSAGE, field, wire_type, 0)?;
                let value = decode_action_int32(raw, &mut cursor, MESSAGE, field)?;
                set_action_once(&mut use_type, value, MESSAGE, field)?;
            }
            3 => {
                require_action_wire_type(MESSAGE, field, wire_type, 2)?;
                let value = decode_length_delimited(raw, &mut cursor, MESSAGE)?;
                set_action_once(&mut extra_data, value, MESSAGE, field)?;
            }
            4 => {
                require_action_wire_type(MESSAGE, field, wire_type, 2)?;
                let value = decode_length_delimited(raw, &mut cursor, MESSAGE)?;
                set_action_once(&mut attr_data, value, MESSAGE, field)?;
            }
            _ => {
                return Err(UseSkillActionDecodeError::UnknownField {
                    message: MESSAGE,
                    field,
                });
            }
        }
    }
    Ok(BorrowedUseSlotRequest {
        slot_id: slot_id.unwrap_or_default(),
        use_type: use_type.unwrap_or_default(),
        extra_data,
        attr_data,
    })
}

fn decode_use_skill_param(raw: &[u8]) -> Result<UseSkillParamSnapshot, UseSkillActionDecodeError> {
    const MESSAGE: &str = "Zproto.UseSkillParam";
    let mut cursor = 0;
    let mut skill_uuid = None;
    let mut skill_id = None;
    let mut skill_level = None;
    let mut begin_time = None;
    let mut target_uuid = None;
    let mut target_position = None;
    let mut current_position = None;
    let mut target_part_id = None;
    let mut target_part_position = None;
    let mut is_passive = None;
    let mut is_activate_roulette = None;

    while cursor < raw.len() {
        let (field, wire_type) = decode_action_tag(raw, &mut cursor, MESSAGE)?;
        match field {
            1 | 2 | 3 | 8 => {
                require_action_wire_type(MESSAGE, field, wire_type, 0)?;
                let value = decode_action_int32(raw, &mut cursor, MESSAGE, field)?;
                match field {
                    1 => set_action_once(&mut skill_uuid, value, MESSAGE, field)?,
                    2 => set_action_once(&mut skill_id, value, MESSAGE, field)?,
                    3 => set_action_once(&mut skill_level, value, MESSAGE, field)?,
                    8 => set_action_once(&mut target_part_id, value, MESSAGE, field)?,
                    _ => unreachable!(),
                }
            }
            4 | 5 => {
                require_action_wire_type(MESSAGE, field, wire_type, 0)?;
                let value = decode_action_varint(raw, &mut cursor, MESSAGE)? as i64;
                if field == 4 {
                    set_action_once(&mut begin_time, value, MESSAGE, field)?;
                } else {
                    set_action_once(&mut target_uuid, value, MESSAGE, field)?;
                }
            }
            6 | 7 | 9 => {
                require_action_wire_type(MESSAGE, field, wire_type, 2)?;
                let nested = decode_length_delimited(raw, &mut cursor, MESSAGE)?;
                let value = decode_use_skill_position(nested)?;
                match field {
                    6 => set_action_once(&mut target_position, value, MESSAGE, field)?,
                    7 => set_action_once(&mut current_position, value, MESSAGE, field)?,
                    9 => set_action_once(&mut target_part_position, value, MESSAGE, field)?,
                    _ => unreachable!(),
                }
            }
            10 | 11 => {
                require_action_wire_type(MESSAGE, field, wire_type, 0)?;
                let value = decode_action_varint(raw, &mut cursor, MESSAGE)? != 0;
                if field == 10 {
                    set_action_once(&mut is_passive, value, MESSAGE, field)?;
                } else {
                    set_action_once(&mut is_activate_roulette, value, MESSAGE, field)?;
                }
            }
            _ => {
                return Err(UseSkillActionDecodeError::UnknownField {
                    message: MESSAGE,
                    field,
                });
            }
        }
    }

    Ok(UseSkillParamSnapshot {
        skill_uuid: skill_uuid.unwrap_or_default(),
        skill_id: skill_id.unwrap_or_default(),
        skill_level: skill_level.unwrap_or_default(),
        begin_time: begin_time.unwrap_or_default(),
        target_uuid: target_uuid.unwrap_or_default(),
        target_position: target_position.unwrap_or_default(),
        current_position: current_position.unwrap_or_default(),
        target_part_id: target_part_id.unwrap_or_default(),
        target_part_position: target_part_position.unwrap_or_default(),
        is_passive: is_passive.unwrap_or_default(),
        is_activate_roulette: is_activate_roulette.unwrap_or_default(),
    })
}

impl Default for UseSkillPosition {
    fn default() -> Self {
        Self {
            x: 0.0,
            y: 0.0,
            z: 0.0,
            direction_radians: 0.0,
        }
    }
}

fn decode_use_skill_position(raw: &[u8]) -> Result<UseSkillPosition, UseSkillActionDecodeError> {
    const MESSAGE: &str = "Zproto.Position";
    let mut cursor = 0;
    let mut values = [None; 4];
    while cursor < raw.len() {
        let (field, wire_type) = decode_action_tag(raw, &mut cursor, MESSAGE)?;
        if !(1..=4).contains(&field) {
            return Err(UseSkillActionDecodeError::UnknownField {
                message: MESSAGE,
                field,
            });
        }
        require_action_wire_type(MESSAGE, field, wire_type, 5)?;
        let end = cursor
            .checked_add(4)
            .filter(|end| *end <= raw.len())
            .ok_or(UseSkillActionDecodeError::Truncated { message: MESSAGE })?;
        let value = f32::from_bits(u32::from_le_bytes(raw[cursor..end].try_into().unwrap()));
        cursor = end;
        set_action_once(&mut values[field as usize - 1], value, MESSAGE, field)?;
    }
    Ok(UseSkillPosition {
        x: values[0].unwrap_or_default(),
        y: values[1].unwrap_or_default(),
        z: values[2].unwrap_or_default(),
        direction_radians: values[3].unwrap_or_default(),
    })
}

fn decode_action_tag(
    raw: &[u8],
    cursor: &mut usize,
    message: &'static str,
) -> Result<(u32, u8), UseSkillActionDecodeError> {
    let tag = decode_action_varint(raw, cursor, message)?;
    let field =
        u32::try_from(tag >> 3).map_err(|_| UseSkillActionDecodeError::InvalidTag { message })?;
    if field == 0 {
        return Err(UseSkillActionDecodeError::InvalidTag { message });
    }
    Ok((field, (tag & 0x07) as u8))
}

fn decode_action_varint(
    raw: &[u8],
    cursor: &mut usize,
    message: &'static str,
) -> Result<u64, UseSkillActionDecodeError> {
    let mut value = 0_u64;
    for shift in (0..70).step_by(7) {
        let byte = *raw
            .get(*cursor)
            .ok_or(UseSkillActionDecodeError::Truncated { message })?;
        *cursor += 1;
        if shift == 63 && byte > 1 {
            return Err(UseSkillActionDecodeError::VarintOverflow { message });
        }
        value |= u64::from(byte & 0x7f) << shift;
        if byte & 0x80 == 0 {
            return Ok(value);
        }
    }
    Err(UseSkillActionDecodeError::VarintOverflow { message })
}

fn decode_action_int32(
    raw: &[u8],
    cursor: &mut usize,
    message: &'static str,
    field: u32,
) -> Result<i32, UseSkillActionDecodeError> {
    let value = decode_action_varint(raw, cursor, message)?;
    if value <= i32::MAX as u64 || value >= u64::MAX - i32::MAX as u64 {
        Ok(value as i32)
    } else {
        Err(UseSkillActionDecodeError::Int32Overflow { message, field })
    }
}

fn decode_action_uint32(
    raw: &[u8],
    cursor: &mut usize,
    message: &'static str,
    field: u32,
) -> Result<u32, UseSkillActionDecodeError> {
    u32::try_from(decode_action_varint(raw, cursor, message)?)
        .map_err(|_| UseSkillActionDecodeError::Uint32Overflow { message, field })
}

fn decode_length_delimited<'a>(
    raw: &'a [u8],
    cursor: &mut usize,
    message: &'static str,
) -> Result<&'a [u8], UseSkillActionDecodeError> {
    let length = usize::try_from(decode_action_varint(raw, cursor, message)?)
        .map_err(|_| UseSkillActionDecodeError::Truncated { message })?;
    let end = cursor
        .checked_add(length)
        .filter(|end| *end <= raw.len())
        .ok_or(UseSkillActionDecodeError::Truncated { message })?;
    let value = &raw[*cursor..end];
    *cursor = end;
    Ok(value)
}

fn require_action_wire_type(
    message: &'static str,
    field: u32,
    observed: u8,
    expected: u8,
) -> Result<(), UseSkillActionDecodeError> {
    if observed == expected {
        Ok(())
    } else {
        Err(UseSkillActionDecodeError::WrongWireType {
            message,
            field,
            observed,
            expected,
        })
    }
}

fn set_action_once<T>(
    slot: &mut Option<T>,
    value: T,
    message: &'static str,
    field: u32,
) -> Result<(), UseSkillActionDecodeError> {
    if slot.is_some() {
        Err(UseSkillActionDecodeError::DuplicateField { message, field })
    } else {
        *slot = Some(value);
        Ok(())
    }
}

fn decode_plaintext(raw: &[u8]) -> Result<UseSkillAttributes, UseSkillAttrDecodeError> {
    let mut cursor = 0_usize;
    let mut timestamp = None;
    let mut velocity = None;
    let mut attack_speed_pct = None;
    let mut cast_speed_pct = None;
    let mut charge_speed_pct = None;

    while cursor < raw.len() {
        let tag = decode_varint(raw, &mut cursor)?;
        let field = u32::try_from(tag >> 3).map_err(|_| UseSkillAttrDecodeError::InvalidTag)?;
        let wire_type = (tag & 0x07) as u8;
        if field == 0 {
            return Err(UseSkillAttrDecodeError::InvalidTag);
        }
        match field {
            1 => {
                require_wire_type(field, wire_type, 0)?;
                set_once(&mut timestamp, decode_varint(raw, &mut cursor)?, field)?;
            }
            2 => {
                require_wire_type(field, wire_type, 5)?;
                let end = cursor
                    .checked_add(4)
                    .filter(|end| *end <= raw.len())
                    .ok_or(UseSkillAttrDecodeError::TruncatedPlaintext)?;
                let bits = u32::from_le_bytes(raw[cursor..end].try_into().unwrap());
                cursor = end;
                set_once(&mut velocity, f32::from_bits(bits), field)?;
            }
            3 => {
                require_wire_type(field, wire_type, 0)?;
                let value = decode_int32(raw, &mut cursor, field)?;
                set_once(&mut attack_speed_pct, value, field)?;
            }
            4 => {
                require_wire_type(field, wire_type, 0)?;
                let value = decode_int32(raw, &mut cursor, field)?;
                set_once(&mut cast_speed_pct, value, field)?;
            }
            5 => {
                require_wire_type(field, wire_type, 0)?;
                let value = decode_int32(raw, &mut cursor, field)?;
                set_once(&mut charge_speed_pct, value, field)?;
            }
            _ => return Err(UseSkillAttrDecodeError::UnknownField { field }),
        }
    }

    Ok(UseSkillAttributes {
        timestamp: timestamp.unwrap_or_default(),
        velocity: velocity.unwrap_or_default(),
        attack_speed_pct: attack_speed_pct.unwrap_or_default(),
        cast_speed_pct: cast_speed_pct.unwrap_or_default(),
        charge_speed_pct: charge_speed_pct.unwrap_or_default(),
    })
}

fn require_wire_type(
    field: u32,
    observed: u8,
    expected: u8,
) -> Result<(), UseSkillAttrDecodeError> {
    if observed == expected {
        Ok(())
    } else {
        Err(UseSkillAttrDecodeError::WrongWireType {
            field,
            observed,
            expected,
        })
    }
}

fn set_once<T>(slot: &mut Option<T>, value: T, field: u32) -> Result<(), UseSkillAttrDecodeError> {
    if slot.is_some() {
        Err(UseSkillAttrDecodeError::DuplicateField { field })
    } else {
        *slot = Some(value);
        Ok(())
    }
}

fn decode_int32(
    raw: &[u8],
    cursor: &mut usize,
    field: u32,
) -> Result<i32, UseSkillAttrDecodeError> {
    let value = decode_varint(raw, cursor)?;
    if value <= i32::MAX as u64 || value >= u64::MAX - i32::MAX as u64 {
        Ok(value as i32)
    } else {
        Err(UseSkillAttrDecodeError::Int32Overflow { field })
    }
}

fn decode_varint(raw: &[u8], cursor: &mut usize) -> Result<u64, UseSkillAttrDecodeError> {
    let mut value = 0_u64;
    for shift in (0..70).step_by(7) {
        let byte = *raw
            .get(*cursor)
            .ok_or(UseSkillAttrDecodeError::TruncatedPlaintext)?;
        *cursor += 1;
        if shift == 63 && byte > 1 {
            return Err(UseSkillAttrDecodeError::VarintOverflow);
        }
        value |= u64::from(byte & 0x7f) << shift;
        if byte & 0x80 == 0 {
            return Ok(value);
        }
    }
    Err(UseSkillAttrDecodeError::VarintOverflow)
}

#[cfg(test)]
pub(crate) mod tests {
    use cbc::Encryptor;
    use cbc::cipher::{BlockEncryptMut, block_padding::Pkcs7};
    use rlogs_events::ActorId;

    use super::*;

    type Aes128CbcEncryptor = Encryptor<Aes128>;

    fn encode_varint(mut value: u64, output: &mut Vec<u8>) {
        while value >= 0x80 {
            output.push((value as u8 & 0x7f) | 0x80);
            value >>= 7;
        }
        output.push(value as u8);
    }

    fn encode_length_delimited(tag: u8, value: &[u8], output: &mut Vec<u8>) {
        output.push(tag);
        encode_varint(value.len() as u64, output);
        output.extend_from_slice(value);
    }

    fn encode_position(value: UseSkillPosition) -> Vec<u8> {
        let mut raw = Vec::new();
        for (tag, component) in [
            (0x0d, value.x),
            (0x15, value.y),
            (0x1d, value.z),
            (0x25, value.direction_radians),
        ] {
            if component != 0.0 {
                raw.push(tag);
                raw.extend_from_slice(&component.to_le_bytes());
            }
        }
        raw
    }

    fn exact_plaintext() -> Vec<u8> {
        let mut raw = Vec::new();
        raw.push(0x08);
        encode_varint(1_786_202_388_123, &mut raw);
        raw.push(0x15);
        raw.extend_from_slice(&4.25_f32.to_le_bytes());
        raw.push(0x18);
        encode_varint(230, &mut raw);
        raw.push(0x20);
        encode_varint(382, &mut raw);
        raw.push(0x28);
        encode_varint(145, &mut raw);
        raw
    }

    fn envelope(plaintext: &[u8]) -> Vec<u8> {
        let iv = [
            0x00, 0x11, 0x22, 0x33, 0x44, 0x55, 0x66, 0x77, 0x88, 0x99, 0xaa, 0xbb, 0xcc, 0xdd,
            0xee, 0xff,
        ];
        let mut ciphertext = vec![0_u8; plaintext.len() + AES_BLOCK_LENGTH];
        ciphertext[..plaintext.len()].copy_from_slice(plaintext);
        let ciphertext = Aes128CbcEncryptor::new_from_slices(&SKILL_AES_KEY, &iv)
            .unwrap()
            .encrypt_padded_mut::<Pkcs7>(&mut ciphertext, plaintext.len())
            .unwrap()
            .to_vec();
        let mut mac = <HmacSha256 as Mac>::new_from_slice(&SKILL_HMAC_KEY).unwrap();
        mac.update(&iv);
        mac.update(&ciphertext);
        let tag = mac.finalize().into_bytes();

        let mut envelope = Vec::with_capacity(IV_LENGTH + MAC_LENGTH + ciphertext.len());
        envelope.extend_from_slice(&iv);
        envelope.extend_from_slice(&tag);
        envelope.extend_from_slice(&ciphertext);
        envelope
    }

    fn world_skill_use_payload_with_attributes(attr_data: Option<&[u8]>) -> Vec<u8> {
        let target_position = UseSkillPosition {
            x: 12.5,
            y: -4.0,
            z: 81.25,
            direction_radians: 1.75,
        };
        let current_position = UseSkillPosition {
            x: 8.0,
            y: 3.5,
            z: 79.0,
            direction_radians: -0.5,
        };
        let target_part_position = UseSkillPosition {
            x: 13.0,
            y: -3.75,
            z: 82.0,
            direction_radians: 1.8,
        };
        let mut param = Vec::new();
        for (tag, value) in [
            (0x08, 9_001_u64),
            (0x10, 2_233),
            (0x18, 5),
            (0x20, 1_786_202_388_120),
            (0x28, 216_009_015_936),
        ] {
            param.push(tag);
            encode_varint(value, &mut param);
        }
        encode_length_delimited(0x32, &encode_position(target_position), &mut param);
        encode_length_delimited(0x3a, &encode_position(current_position), &mut param);
        param.extend_from_slice(&[0x40, 0x03]);
        encode_length_delimited(0x4a, &encode_position(target_part_position), &mut param);
        param.extend_from_slice(&[0x50, 0x01, 0x58, 0x01]);

        let mut request = Vec::new();
        request.extend_from_slice(&[0x08, 0x15, 0x10, 0x01]);
        encode_length_delimited(0x1a, &param, &mut request);
        if let Some(attr_data) = attr_data {
            encode_length_delimited(0x22, attr_data, &mut request);
        }

        let mut outer = Vec::new();
        encode_length_delimited(0x0a, &request, &mut outer);
        outer
    }

    pub(crate) fn world_skill_use_payload() -> Vec<u8> {
        let attr_data = envelope(&exact_plaintext());
        world_skill_use_payload_with_attributes(Some(&attr_data))
    }

    pub(crate) fn world_skill_use_payload_without_attributes() -> Vec<u8> {
        world_skill_use_payload_with_attributes(None)
    }

    #[test]
    fn decodes_exact_current_build_gameplay_fields() {
        let mut scratch = Vec::new();
        let decoded = decode_use_skill_attr_into(
            BPSR_CURRENT_USE_SKILL_ATTR_BUILD,
            &envelope(&exact_plaintext()),
            &mut scratch,
        )
        .unwrap();

        assert_eq!(
            decoded,
            UseSkillAttributes {
                timestamp: 1_786_202_388_123,
                velocity: 4.25,
                attack_speed_pct: 230,
                cast_speed_pct: 382,
                charge_speed_pct: 145,
            }
        );
        assert!(scratch.is_empty());
        assert!(scratch.capacity() >= AES_BLOCK_LENGTH);
    }

    #[test]
    fn decodes_exact_world_use_slot_skill_identity_target_and_speed_snapshot() {
        let mut scratch = Vec::new();
        let decoded = decode_world_use_slot_skill_action_into(
            BPSR_CURRENT_USE_SKILL_ATTR_BUILD,
            &world_skill_use_payload(),
            &mut scratch,
        )
        .unwrap()
        .expect("UseSlotType Skill should produce an action");

        assert_eq!(decoded.slot_id, 21);
        assert_eq!(decoded.param.skill_uuid, 9_001);
        assert_eq!(decoded.param.skill_id, 2_233);
        assert_eq!(decoded.param.skill_level, 5);
        assert_eq!(decoded.param.begin_time, 1_786_202_388_120);
        assert_eq!(decoded.param.target_uuid, 216_009_015_936);
        assert_eq!(decoded.param.target_part_id, 3);
        assert!(decoded.param.is_passive);
        assert!(decoded.param.is_activate_roulette);
        assert_eq!(decoded.param.target_position.x, 12.5);
        assert_eq!(decoded.param.current_position.direction_radians, -0.5);
        assert_eq!(decoded.param.target_part_position.z, 82.0);
        let attributes = decoded
            .attributes
            .expect("authenticated action-speed snapshot");
        assert_eq!(attributes.attack_speed_pct, 230);
        assert_eq!(attributes.cast_speed_pct, 382);
        assert_eq!(attributes.charge_speed_pct, 145);
        let canonical = decoded
            .canonical_action_timing()
            .expect("authenticated canonical action timing");
        assert_eq!(canonical.action_instance_id, 9_001);
        assert_eq!(canonical.base_ability, AbilityId(2_233));
        assert_eq!(canonical.ability_level, 5);
        assert_eq!(canonical.slot_id, 21);
        assert_eq!(canonical.client_timestamp_raw, 1_786_202_388_123);
        assert_eq!(canonical.begin_time_raw, 1_786_202_388_120);
        assert_eq!(canonical.attack_speed_basis_points, 230);
        assert_eq!(canonical.cast_speed_basis_points, 382);
        assert_eq!(canonical.charge_speed_basis_points, 145);
        assert!(canonical.passive);
        assert!(canonical.activated_roulette);
        assert_eq!(canonical.target_part_id, 3);

        let source = EntityRef {
            actor_id: ActorId(7),
            entity_uuid: EntityUuid(3_296_036),
        };
        let target = EntityRef {
            actor_id: ActorId(8),
            entity_uuid: EntityUuid(216_009_015_936),
        };
        let cast = decoded
            .canonical_cast_started(source, Some(target))
            .expect("exact packet target should convert");
        assert_eq!(cast.source, source);
        assert_eq!(cast.target, Some(target));
        assert_eq!(cast.ability, AbilityId(2_233));
        assert_eq!(cast.state, CastState::Started);
        assert_eq!(cast.action_timing, Some(canonical));
        assert!(
            decoded
                .canonical_cast_started(
                    source,
                    Some(EntityRef {
                        actor_id: ActorId(9),
                        entity_uuid: EntityUuid(216_009_015_937),
                    }),
                )
                .is_none()
        );
        assert!(scratch.is_empty());
    }

    #[test]
    fn decodes_skill_identity_when_optional_action_speed_attributes_are_absent() {
        let mut scratch = vec![0xaa; 16];
        let decoded = decode_world_use_slot_skill_action_into(
            BPSR_CURRENT_USE_SKILL_ATTR_BUILD,
            &world_skill_use_payload_without_attributes(),
            &mut scratch,
        )
        .unwrap()
        .expect("UseSlotType Skill should produce an action without field 4");

        assert_eq!(decoded.slot_id, 21);
        assert_eq!(decoded.param.skill_uuid, 9_001);
        assert_eq!(decoded.param.skill_id, 2_233);
        assert_eq!(decoded.param.target_uuid, 216_009_015_936);
        assert_eq!(decoded.attributes, None);
        assert_eq!(decoded.canonical_action_timing(), None);

        let source = EntityRef {
            actor_id: ActorId(7),
            entity_uuid: EntityUuid(3_296_036),
        };
        let target = EntityRef {
            actor_id: ActorId(8),
            entity_uuid: EntityUuid(216_009_015_936),
        };
        let cast = decoded
            .canonical_cast_started(source, Some(target))
            .expect("exact packet identity should convert without timing metadata");
        assert_eq!(cast.ability, AbilityId(2_233));
        assert_eq!(cast.state, CastState::Started);
        assert_eq!(cast.action_timing, None);
        assert_eq!(scratch, vec![0xaa; 16]);
    }

    #[test]
    fn malformed_present_action_speed_attributes_still_fail_closed() {
        let payload = world_skill_use_payload_with_attributes(Some(&[0x01, 0x02, 0x03]));
        let mut scratch = vec![0xaa; 16];
        let error = decode_world_use_slot_skill_action_into(
            BPSR_CURRENT_USE_SKILL_ATTR_BUILD,
            &payload,
            &mut scratch,
        )
        .unwrap_err();

        assert_eq!(
            error,
            UseSkillActionDecodeError::AttributeEnvelope(
                UseSkillAttrDecodeError::EnvelopeTooShort { actual: 3 }
            )
        );
        assert_eq!(scratch, vec![0xaa; 16]);
    }

    #[test]
    fn reviewed_builds_decode_the_same_authenticated_skill_action() {
        let payload = world_skill_use_payload();
        let mut scratch = Vec::new();
        let historical = decode_world_use_slot_skill_action_into(
            BPSR_USE_SKILL_ATTR_BUILD,
            &payload,
            &mut scratch,
        )
        .unwrap();
        let current = decode_world_use_slot_skill_action_into(
            BPSR_CURRENT_USE_SKILL_ATTR_BUILD,
            &payload,
            &mut scratch,
        )
        .unwrap();

        assert_eq!(historical, current);
        let error = decode_world_use_slot_skill_action_into("unreviewed", &payload, &mut scratch)
            .unwrap_err();
        assert!(matches!(
            error,
            UseSkillActionDecodeError::AttributeEnvelope(
                UseSkillAttrDecodeError::UnsupportedBuild { .. }
            )
        ));
    }

    #[test]
    fn decodes_exact_client_stage_trigger_and_stage_end_messages() {
        let mut trigger = Vec::new();
        for (tag, value) in [(0x08, 105_u64), (0x10, 1_786_202_388_555), (0x18, 9_001)] {
            trigger.push(tag);
            encode_varint(value, &mut trigger);
        }
        assert_eq!(
            decode_client_skill_stage_trigger(&trigger).unwrap(),
            ClientSkillStageTriggerSnapshot {
                trigger_type: 105,
                time: 1_786_202_388_555,
                skill_uuid: 9_001,
            }
        );

        let mut stage_end = Vec::new();
        for (tag, value) in [
            (0x08, 2_u64),
            (0x10, 3),
            (0x18, 1_786_202_388_777),
            (0x20, 41),
            (0x28, 9_001),
            (0x30, 7),
        ] {
            stage_end.push(tag);
            encode_varint(value, &mut stage_end);
        }
        assert_eq!(
            decode_client_skill_stage_end(&stage_end).unwrap(),
            ClientSkillStageEndSnapshot {
                current_stage_index: 2,
                next_stage_index: 3,
                time: 1_786_202_388_777,
                condition_id: 41,
                skill_uuid: 9_001,
                trigger_index: 7,
            }
        );
    }

    #[test]
    fn decodes_exact_server_selected_stage_transition() {
        let mut inner = Vec::new();
        for (tag, value) in [(0x08, 9_001_u64), (0x10, 2), (0x18, 3), (0x20, 41)] {
            inner.push(tag);
            encode_varint(value, &mut inner);
        }
        let mut outer = Vec::new();
        encode_length_delimited(0x0a, &inner, &mut outer);
        assert_eq!(
            decode_server_skill_stage_end(&outer).unwrap(),
            ServerSkillStageEndSnapshot {
                skill_uuid: 9_001,
                stage_id: 2,
                new_stage_id: 3,
                condition_id: 41,
            }
        );
    }

    #[test]
    fn stage_decoders_fail_closed_on_current_build_schema_drift() {
        assert!(matches!(
            decode_client_skill_stage_trigger(&[0x20, 0x01]),
            Err(UseSkillActionDecodeError::UnknownField {
                message: "Zproto.World.Types.SyncSkillStageTrigger",
                field: 4,
            })
        ));
        assert!(matches!(
            decode_client_skill_stage_end(&[0x0a, 0x00]),
            Err(UseSkillActionDecodeError::WrongWireType {
                message: "Zproto.World.Types.ClientStageEnd",
                field: 1,
                observed: 2,
                expected: 0,
            })
        ));
        assert!(matches!(
            decode_server_skill_stage_end(&[]),
            Err(UseSkillActionDecodeError::MissingField {
                message: "Zproto.WorldNtf.Types.SyncServerSkillStageEnd",
                field: 1,
            })
        ));
    }

    #[test]
    fn shared_use_slot_route_ignores_non_skill_types_without_decrypting() {
        let request = [0x08, 0x07, 0x10, 0x02];
        let mut outer = Vec::new();
        encode_length_delimited(0x0a, &request, &mut outer);
        let mut scratch = vec![0xaa; 16];
        let decoded = decode_world_use_slot_skill_action_into(
            BPSR_USE_SKILL_ATTR_BUILD,
            &outer,
            &mut scratch,
        )
        .unwrap();
        assert_eq!(decoded, None);
        assert_eq!(scratch, vec![0xaa; 16]);
    }

    #[test]
    fn omitted_default_fields_decode_as_zero() {
        let mut scratch = Vec::new();
        let decoded =
            decode_use_skill_attr_into(BPSR_USE_SKILL_ATTR_BUILD, &envelope(&[]), &mut scratch)
                .unwrap();
        assert_eq!(
            decoded,
            UseSkillAttributes {
                timestamp: 0,
                velocity: 0.0,
                attack_speed_pct: 0,
                cast_speed_pct: 0,
                charge_speed_pct: 0,
            }
        );
    }

    #[test]
    fn rejects_unknown_build_before_decryption() {
        let mut scratch = Vec::new();
        let error = decode_use_skill_attr_into("24252055", &[], &mut scratch).unwrap_err();
        assert!(matches!(
            error,
            UseSkillAttrDecodeError::UnsupportedBuild { .. }
        ));
    }

    #[test]
    fn rejects_mac_tampering_before_decryption() {
        let mut envelope = envelope(&exact_plaintext());
        *envelope.last_mut().unwrap() ^= 0x40;
        let mut scratch = vec![0xaa; 32];
        let error = decode_use_skill_attr_into(BPSR_USE_SKILL_ATTR_BUILD, &envelope, &mut scratch)
            .unwrap_err();
        assert_eq!(error, UseSkillAttrDecodeError::MacMismatch);
        assert_eq!(scratch, vec![0xaa; 32]);
    }

    #[test]
    fn rejects_authenticated_bad_padding_and_scrubs_scratch() {
        let iv = [0x5a; IV_LENGTH];
        let ciphertext = [0_u8; AES_BLOCK_LENGTH];
        let mut mac = <HmacSha256 as Mac>::new_from_slice(&SKILL_HMAC_KEY).unwrap();
        mac.update(&iv);
        mac.update(&ciphertext);
        let tag = mac.finalize().into_bytes();
        let mut envelope = Vec::new();
        envelope.extend_from_slice(&iv);
        envelope.extend_from_slice(&tag);
        envelope.extend_from_slice(&ciphertext);

        let mut scratch = Vec::new();
        let error = decode_use_skill_attr_into(BPSR_USE_SKILL_ATTR_BUILD, &envelope, &mut scratch)
            .unwrap_err();
        assert_eq!(error, UseSkillAttrDecodeError::InvalidPadding);
        assert!(scratch.is_empty());
    }

    #[test]
    fn rejects_authenticated_schema_drift() {
        let mut raw = exact_plaintext();
        raw.push(0x30);
        raw.push(0x01);
        let mut scratch = Vec::new();
        let error =
            decode_use_skill_attr_into(BPSR_USE_SKILL_ATTR_BUILD, &envelope(&raw), &mut scratch)
                .unwrap_err();
        assert_eq!(error, UseSkillAttrDecodeError::UnknownField { field: 6 });
        assert!(scratch.is_empty());
    }

    #[test]
    fn rejects_duplicate_fields() {
        let mut raw = exact_plaintext();
        raw.extend_from_slice(&[0x18, 0x01]);
        let mut scratch = Vec::new();
        let error =
            decode_use_skill_attr_into(BPSR_USE_SKILL_ATTR_BUILD, &envelope(&raw), &mut scratch)
                .unwrap_err();
        assert_eq!(error, UseSkillAttrDecodeError::DuplicateField { field: 3 });
    }
}
