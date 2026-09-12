//! Read-only decoding of the observed current-build ground-marker request.
//!
//! This boundary exists only to preserve evidence from already captured
//! `World.UseSlot` requests. It is intentionally separate from combat action
//! decoding, canonical events, run timing, and every outbound transport.

use aes::Aes128;
use cbc::cipher::{BlockDecryptMut, KeyIvInit, block_padding::Pkcs7};
use hmac::{Hmac, Mac};
use serde::Serialize;
use sha2::Sha256;
use thiserror::Error;

use crate::ProtocolPack;

type Aes128CbcDecryptor = cbc::Decryptor<Aes128>;
type HmacSha256 = Hmac<Sha256>;

pub const AUTOMARKER_REQUEST_BUILD: &str = "25247556";
pub const AUTOMARKER_REQUEST_PACK_DIGEST: &str =
    "sha256:480f928cca6baf19c1ebaf85e8c52f2f1852096167043260503cca6d46133a60";

const IV_LENGTH: usize = 16;
const MAC_LENGTH: usize = 32;
const ENVELOPE_PREFIX_LENGTH: usize = IV_LENGTH + MAC_LENGTH;
const AES_BLOCK_LENGTH: usize = 16;
const MAX_ABS_MARKER_COORDINATE: f32 = 1_000_000.0;

// Current observations prove continuity of these gameplay-envelope keys from
// the reviewed source build. They authenticate only the captured marker
// telemetry decoded here; their presence grants no request-generation right.
const SKILL_AES_KEY: [u8; 16] = [
    0x3d, 0x09, 0xd6, 0x69, 0x1d, 0xd9, 0x7a, 0x7c, 0xf9, 0xae, 0x12, 0x2c, 0x06, 0xef, 0x3c, 0x84,
];
const SKILL_HMAC_KEY: [u8; 32] = [
    0x24, 0x4b, 0x9a, 0xab, 0x92, 0x60, 0x5e, 0xbd, 0xf4, 0x6b, 0x7f, 0x32, 0x1a, 0x18, 0x70, 0x9a,
    0xff, 0x63, 0x3c, 0x03, 0x86, 0x30, 0xb7, 0xea, 0xc2, 0xbc, 0x95, 0xec, 0xd6, 0xa9, 0xc3, 0x36,
];

/// Four-float `Zproto.Position` carried by an observed marker request.
///
/// The fourth value is expressed in degrees in the current marker samples. A
/// distinct type prevents it from inheriting the canonical skill decoder's
/// historical `direction_radians` label.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct AutomarkerRequestPosition {
    pub x: f32,
    pub y: f32,
    pub z: f32,
    pub heading_degrees: f32,
}

/// Authenticated plaintext attached to a current-build marker request.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct AutomarkerRequestAttributes {
    pub timestamp: u64,
    pub velocity: f32,
    pub attack_speed_pct: i32,
    pub cast_speed_pct: i32,
    pub charge_speed_pct: Option<i32>,
    /// Protobuf field 6. Its meaning and native generation rule remain unknown.
    pub opaque_current_build_scalar: f32,
}

/// A decoded request observation. This type cannot encode or transmit itself.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct ObservedAutomarkerRequest {
    pub marker_number: u8,
    pub slot_id: i32,
    pub skill_uuid: i32,
    pub skill_id: i32,
    pub skill_level: i32,
    pub begin_time: i64,
    pub target_position: AutomarkerRequestPosition,
    pub current_position: AutomarkerRequestPosition,
    /// `UseSlotRequest.sessionSequence` (protobuf field 5). This is observed
    /// state only; the decoder does not manufacture a session sequence.
    pub session_sequence: u32,
    pub attributes: AutomarkerRequestAttributes,
}

#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum AutomarkerRequestDecodeError {
    #[error(
        "automarker request requires exact build {AUTOMARKER_REQUEST_BUILD} and reviewed pack digest"
    )]
    UnsupportedProtocolIdentity,
    #[error("{message} ended in the middle of a field")]
    Truncated { message: &'static str },
    #[error("{message} protobuf varint overflows 64 bits")]
    VarintOverflow { message: &'static str },
    #[error("{message} protobuf tag is invalid")]
    InvalidTag { message: &'static str },
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
    #[error("{message} is missing required field {field}")]
    MissingField { message: &'static str, field: u32 },
    #[error(
        "marker slot {slot_id} and skill {skill_id} are outside or disagree with the observed 1-through-6 mapping"
    )]
    InvalidMarkerIdentity { slot_id: i32, skill_id: i32 },
    #[error("marker request has skill UUID {skill_uuid}, outside the observed action namespace")]
    InvalidMarkerSkillUuid { skill_uuid: i32 },
    #[error("marker position field {field} is non-finite or outside its verified domain")]
    InvalidPosition { field: u32 },
    #[error("marker request field {field} has unsupported value {value}")]
    UnsupportedValue { field: u32, value: i64 },
    #[error("marker attribute envelope length {actual} is invalid")]
    InvalidEnvelopeLength { actual: usize },
    #[error("marker attribute HMAC-SHA256 verification failed")]
    MacMismatch,
    #[error("marker attribute AES-CBC PKCS#7 padding is invalid")]
    InvalidPadding,
    #[error("{message} int32 field {field} is outside the protobuf int32 domain")]
    Int32Overflow { message: &'static str, field: u32 },
}

pub fn supports_observed_automarker_requests(pack: &ProtocolPack) -> bool {
    pack.definition().target.build_id == AUTOMARKER_REQUEST_BUILD
        && pack.digest() == AUTOMARKER_REQUEST_PACK_DIGEST
}

/// Decodes one already-observed current-build marker request.
///
/// This API has no encoder and no transport access. Unknown fields and any
/// non-marker `UseSlot` identity fail closed.
pub fn decode_observed_automarker_request_into(
    pack: &ProtocolPack,
    payload: &[u8],
    scratch: &mut Vec<u8>,
) -> Result<ObservedAutomarkerRequest, AutomarkerRequestDecodeError> {
    if !supports_observed_automarker_requests(pack) {
        return Err(AutomarkerRequestDecodeError::UnsupportedProtocolIdentity);
    }
    let request = one_message(payload, "Zproto.World.Types.UseSlot", 1)?;
    decode_request(request, scratch)
}

fn decode_request(
    raw: &[u8],
    scratch: &mut Vec<u8>,
) -> Result<ObservedAutomarkerRequest, AutomarkerRequestDecodeError> {
    const MESSAGE: &str = "Zproto.UseSlotRequest";
    let mut cursor = 0;
    let mut slot_id = None;
    let mut use_type = None;
    let mut extra_data = None;
    let mut attr_data = None;
    let mut session_sequence = None;
    while cursor < raw.len() {
        let (field, wire) = tag(raw, &mut cursor, MESSAGE)?;
        match field {
            1 | 2 | 5 => {
                require_wire(MESSAGE, field, wire, 0)?;
                let value = varint(raw, &mut cursor, MESSAGE)?;
                match field {
                    1 => set_once(&mut slot_id, as_i32(value, MESSAGE, field)?, MESSAGE, field)?,
                    2 => set_once(
                        &mut use_type,
                        as_i32(value, MESSAGE, field)?,
                        MESSAGE,
                        field,
                    )?,
                    5 => set_once(
                        &mut session_sequence,
                        u32::try_from(value).map_err(|_| {
                            AutomarkerRequestDecodeError::UnsupportedValue {
                                field,
                                value: value as i64,
                            }
                        })?,
                        MESSAGE,
                        field,
                    )?,
                    _ => unreachable!(),
                }
            }
            3 | 4 => {
                require_wire(MESSAGE, field, wire, 2)?;
                let value = bytes(raw, &mut cursor, MESSAGE)?;
                if field == 3 {
                    set_once(&mut extra_data, value, MESSAGE, field)?;
                } else {
                    set_once(&mut attr_data, value, MESSAGE, field)?;
                }
            }
            _ => {
                return Err(AutomarkerRequestDecodeError::UnknownField {
                    message: MESSAGE,
                    field,
                });
            }
        }
    }
    let slot_id = required(slot_id, MESSAGE, 1)?;
    let use_type = required(use_type, MESSAGE, 2)?;
    if use_type != 1 {
        return Err(AutomarkerRequestDecodeError::UnsupportedValue {
            field: 2,
            value: i64::from(use_type),
        });
    }
    let param = decode_param(required(extra_data, MESSAGE, 3)?)?;
    let marker_number = marker_number(slot_id, param.skill_id)?;
    // The action UUID is allocated independently of the marker number. All
    // current-build UseSlot observations use the 0x6....... action namespace;
    // never freeze the six sequential values from one capture as marker IDs.
    if (param.skill_uuid as u32) & 0xf000_0000 != 0x6000_0000 {
        return Err(AutomarkerRequestDecodeError::InvalidMarkerSkillUuid {
            skill_uuid: param.skill_uuid,
        });
    }
    let attributes = decode_attributes(required(attr_data, MESSAGE, 4)?, scratch)?;
    if param.begin_time < 0 || attributes.timestamp != param.begin_time as u64 {
        return Err(AutomarkerRequestDecodeError::UnsupportedValue {
            field: 4,
            value: param.begin_time,
        });
    }
    Ok(ObservedAutomarkerRequest {
        marker_number,
        slot_id,
        skill_uuid: param.skill_uuid,
        skill_id: param.skill_id,
        skill_level: param.skill_level,
        begin_time: param.begin_time,
        target_position: param.target_position,
        current_position: param.current_position,
        session_sequence: required(session_sequence, MESSAGE, 5)?,
        attributes,
    })
}

#[derive(Clone, Copy)]
struct MarkerParam {
    skill_uuid: i32,
    skill_id: i32,
    skill_level: i32,
    begin_time: i64,
    target_position: AutomarkerRequestPosition,
    current_position: AutomarkerRequestPosition,
}

fn decode_param(raw: &[u8]) -> Result<MarkerParam, AutomarkerRequestDecodeError> {
    const MESSAGE: &str = "Zproto.UseSkillParam";
    let mut cursor = 0;
    let (mut uuid, mut skill, mut level, mut begin) = (None, None, None, None);
    let (mut target, mut current) = (None, None);
    let (mut passive, mut roulette) = (None, None);
    while cursor < raw.len() {
        let (field, wire) = tag(raw, &mut cursor, MESSAGE)?;
        match field {
            1..=3 => {
                require_wire(MESSAGE, field, wire, 0)?;
                let value = as_i32(varint(raw, &mut cursor, MESSAGE)?, MESSAGE, field)?;
                match field {
                    1 => set_once(&mut uuid, value, MESSAGE, field)?,
                    2 => set_once(&mut skill, value, MESSAGE, field)?,
                    3 => set_once(&mut level, value, MESSAGE, field)?,
                    _ => unreachable!(),
                }
            }
            4 => {
                require_wire(MESSAGE, field, wire, 0)?;
                set_once(
                    &mut begin,
                    varint(raw, &mut cursor, MESSAGE)? as i64,
                    MESSAGE,
                    field,
                )?;
            }
            6 | 7 => {
                require_wire(MESSAGE, field, wire, 2)?;
                let value = decode_position(bytes(raw, &mut cursor, MESSAGE)?)?;
                if field == 6 {
                    set_once(&mut target, value, MESSAGE, field)?;
                } else {
                    set_once(&mut current, value, MESSAGE, field)?;
                }
            }
            10 | 11 => {
                require_wire(MESSAGE, field, wire, 0)?;
                let value = varint(raw, &mut cursor, MESSAGE)?;
                if value > 1 {
                    return Err(AutomarkerRequestDecodeError::UnsupportedValue {
                        field,
                        value: value as i64,
                    });
                }
                if field == 10 {
                    set_once(&mut passive, value == 1, MESSAGE, field)?;
                } else {
                    set_once(&mut roulette, value == 1, MESSAGE, field)?;
                }
            }
            // Captured marker requests omit target UUID, target part, and target-part position.
            _ => {
                return Err(AutomarkerRequestDecodeError::UnknownField {
                    message: MESSAGE,
                    field,
                });
            }
        }
    }
    let level = required(level, MESSAGE, 3)?;
    if level != 1 {
        return Err(AutomarkerRequestDecodeError::UnsupportedValue {
            field: 3,
            value: i64::from(level),
        });
    }
    for (field, value) in [
        (10, required(passive, MESSAGE, 10)?),
        (11, required(roulette, MESSAGE, 11)?),
    ] {
        if !value {
            return Err(AutomarkerRequestDecodeError::UnsupportedValue { field, value: 0 });
        }
    }
    Ok(MarkerParam {
        skill_uuid: required(uuid, MESSAGE, 1)?,
        skill_id: required(skill, MESSAGE, 2)?,
        skill_level: level,
        begin_time: required(begin, MESSAGE, 4)?,
        target_position: required(target, MESSAGE, 6)?,
        current_position: required(current, MESSAGE, 7)?,
    })
}

fn marker_number(slot_id: i32, skill_id: i32) -> Result<u8, AutomarkerRequestDecodeError> {
    let from_slot = slot_id.checked_sub(200);
    let from_skill = skill_id.checked_sub(1100);
    if from_slot != from_skill || !matches!(from_slot, Some(1..=6)) {
        return Err(AutomarkerRequestDecodeError::InvalidMarkerIdentity { slot_id, skill_id });
    }
    Ok(from_slot.unwrap() as u8)
}

fn decode_position(raw: &[u8]) -> Result<AutomarkerRequestPosition, AutomarkerRequestDecodeError> {
    const MESSAGE: &str = "Zproto.Position";
    let mut cursor = 0;
    let mut values = [None; 4];
    while cursor < raw.len() {
        let (field, wire) = tag(raw, &mut cursor, MESSAGE)?;
        if !(1..=4).contains(&field) {
            return Err(AutomarkerRequestDecodeError::UnknownField {
                message: MESSAGE,
                field,
            });
        }
        require_wire(MESSAGE, field, wire, 5)?;
        set_once(
            &mut values[field as usize - 1],
            fixed32(raw, &mut cursor, MESSAGE)?,
            MESSAGE,
            field,
        )?;
    }
    let position = AutomarkerRequestPosition {
        x: required(values[0], MESSAGE, 1)?,
        y: required(values[1], MESSAGE, 2)?,
        z: required(values[2], MESSAGE, 3)?,
        heading_degrees: required(values[3], MESSAGE, 4)?,
    };
    for (field, coordinate) in [(1, position.x), (2, position.y), (3, position.z)] {
        if !coordinate.is_finite() || coordinate.abs() > MAX_ABS_MARKER_COORDINATE {
            return Err(AutomarkerRequestDecodeError::InvalidPosition { field });
        }
    }
    if !position.heading_degrees.is_finite() || !(0.0..360.0).contains(&position.heading_degrees) {
        return Err(AutomarkerRequestDecodeError::InvalidPosition { field: 4 });
    }
    Ok(position)
}

fn decode_attributes(
    envelope: &[u8],
    scratch: &mut Vec<u8>,
) -> Result<AutomarkerRequestAttributes, AutomarkerRequestDecodeError> {
    if envelope.len() < ENVELOPE_PREFIX_LENGTH + AES_BLOCK_LENGTH
        || (envelope.len() - ENVELOPE_PREFIX_LENGTH) % AES_BLOCK_LENGTH != 0
    {
        return Err(AutomarkerRequestDecodeError::InvalidEnvelopeLength {
            actual: envelope.len(),
        });
    }
    let (iv, rest) = envelope.split_at(IV_LENGTH);
    let (expected_mac, ciphertext) = rest.split_at(MAC_LENGTH);
    let mut mac = <HmacSha256 as Mac>::new_from_slice(&SKILL_HMAC_KEY).unwrap();
    mac.update(iv);
    mac.update(ciphertext);
    mac.verify_slice(expected_mac)
        .map_err(|_| AutomarkerRequestDecodeError::MacMismatch)?;
    scratch.clear();
    scratch.extend_from_slice(ciphertext);
    let length = match Aes128CbcDecryptor::new_from_slices(&SKILL_AES_KEY, iv)
        .unwrap()
        .decrypt_padded_mut::<Pkcs7>(scratch)
    {
        Ok(plaintext) => plaintext.len(),
        Err(_) => {
            scratch.fill(0);
            scratch.clear();
            return Err(AutomarkerRequestDecodeError::InvalidPadding);
        }
    };
    let result = decode_attribute_plaintext(&scratch[..length]);
    scratch.fill(0);
    scratch.clear();
    result
}

fn decode_attribute_plaintext(
    raw: &[u8],
) -> Result<AutomarkerRequestAttributes, AutomarkerRequestDecodeError> {
    const MESSAGE: &str = "Zproto.UseSkillAttrPlaintext";
    let mut cursor = 0;
    let (mut timestamp, mut velocity, mut attack, mut cast, mut charge, mut opaque) =
        (None, None, None, None, None, None);
    while cursor < raw.len() {
        let (field, wire) = tag(raw, &mut cursor, MESSAGE)?;
        match field {
            1 | 3 | 4 | 5 => {
                require_wire(MESSAGE, field, wire, 0)?;
                let value = varint(raw, &mut cursor, MESSAGE)?;
                match field {
                    1 => set_once(&mut timestamp, value, MESSAGE, field)?,
                    3 => set_once(&mut attack, as_i32(value, MESSAGE, field)?, MESSAGE, field)?,
                    4 => set_once(&mut cast, as_i32(value, MESSAGE, field)?, MESSAGE, field)?,
                    5 => set_once(&mut charge, as_i32(value, MESSAGE, field)?, MESSAGE, field)?,
                    _ => unreachable!(),
                }
            }
            2 | 6 => {
                require_wire(MESSAGE, field, wire, 5)?;
                let value = fixed32(raw, &mut cursor, MESSAGE)?;
                if field == 2 {
                    set_once(&mut velocity, value, MESSAGE, field)?;
                } else {
                    set_once(&mut opaque, value, MESSAGE, field)?;
                }
            }
            _ => {
                return Err(AutomarkerRequestDecodeError::UnknownField {
                    message: MESSAGE,
                    field,
                });
            }
        }
    }
    let opaque_current_build_scalar = required(opaque, MESSAGE, 6)?;
    if !opaque_current_build_scalar.is_finite() || opaque_current_build_scalar != 1.0 {
        return Err(AutomarkerRequestDecodeError::UnsupportedValue {
            field: 6,
            value: i64::from(opaque_current_build_scalar.to_bits()),
        });
    }
    Ok(AutomarkerRequestAttributes {
        timestamp: required(timestamp, MESSAGE, 1)?,
        velocity: required(velocity, MESSAGE, 2)?,
        attack_speed_pct: required(attack, MESSAGE, 3)?,
        cast_speed_pct: required(cast, MESSAGE, 4)?,
        charge_speed_pct: charge,
        opaque_current_build_scalar,
    })
}

fn one_message<'a>(
    raw: &'a [u8],
    message: &'static str,
    expected_field: u32,
) -> Result<&'a [u8], AutomarkerRequestDecodeError> {
    let mut cursor = 0;
    let mut value = None;
    while cursor < raw.len() {
        let (field, wire) = tag(raw, &mut cursor, message)?;
        if field != expected_field {
            return Err(AutomarkerRequestDecodeError::UnknownField { message, field });
        }
        require_wire(message, field, wire, 2)?;
        set_once(
            &mut value,
            bytes(raw, &mut cursor, message)?,
            message,
            field,
        )?;
    }
    required(value, message, expected_field)
}

fn tag(
    raw: &[u8],
    cursor: &mut usize,
    message: &'static str,
) -> Result<(u32, u8), AutomarkerRequestDecodeError> {
    let value = varint(raw, cursor, message)?;
    let field = u32::try_from(value >> 3)
        .map_err(|_| AutomarkerRequestDecodeError::InvalidTag { message })?;
    if field == 0 {
        return Err(AutomarkerRequestDecodeError::InvalidTag { message });
    }
    Ok((field, (value & 7) as u8))
}

fn varint(
    raw: &[u8],
    cursor: &mut usize,
    message: &'static str,
) -> Result<u64, AutomarkerRequestDecodeError> {
    let mut value = 0_u64;
    for shift in (0..70).step_by(7) {
        let byte = *raw
            .get(*cursor)
            .ok_or(AutomarkerRequestDecodeError::Truncated { message })?;
        *cursor += 1;
        if shift == 63 && byte > 1 {
            return Err(AutomarkerRequestDecodeError::VarintOverflow { message });
        }
        value |= u64::from(byte & 0x7f) << shift;
        if byte & 0x80 == 0 {
            return Ok(value);
        }
    }
    Err(AutomarkerRequestDecodeError::VarintOverflow { message })
}

fn bytes<'a>(
    raw: &'a [u8],
    cursor: &mut usize,
    message: &'static str,
) -> Result<&'a [u8], AutomarkerRequestDecodeError> {
    let length = usize::try_from(varint(raw, cursor, message)?)
        .map_err(|_| AutomarkerRequestDecodeError::Truncated { message })?;
    let end = cursor
        .checked_add(length)
        .filter(|end| *end <= raw.len())
        .ok_or(AutomarkerRequestDecodeError::Truncated { message })?;
    let result = &raw[*cursor..end];
    *cursor = end;
    Ok(result)
}

fn fixed32(
    raw: &[u8],
    cursor: &mut usize,
    message: &'static str,
) -> Result<f32, AutomarkerRequestDecodeError> {
    let end = cursor
        .checked_add(4)
        .filter(|end| *end <= raw.len())
        .ok_or(AutomarkerRequestDecodeError::Truncated { message })?;
    let value = f32::from_bits(u32::from_le_bytes(raw[*cursor..end].try_into().unwrap()));
    *cursor = end;
    Ok(value)
}

fn require_wire(
    message: &'static str,
    field: u32,
    observed: u8,
    expected: u8,
) -> Result<(), AutomarkerRequestDecodeError> {
    if observed == expected {
        Ok(())
    } else {
        Err(AutomarkerRequestDecodeError::WrongWireType {
            message,
            field,
            observed,
            expected,
        })
    }
}

fn required<T>(
    value: Option<T>,
    message: &'static str,
    field: u32,
) -> Result<T, AutomarkerRequestDecodeError> {
    value.ok_or(AutomarkerRequestDecodeError::MissingField { message, field })
}

fn set_once<T>(
    slot: &mut Option<T>,
    value: T,
    message: &'static str,
    field: u32,
) -> Result<(), AutomarkerRequestDecodeError> {
    if slot.is_some() {
        Err(AutomarkerRequestDecodeError::DuplicateField { message, field })
    } else {
        *slot = Some(value);
        Ok(())
    }
}

fn as_i32(
    value: u64,
    message: &'static str,
    field: u32,
) -> Result<i32, AutomarkerRequestDecodeError> {
    if value <= i32::MAX as u64 || value >= u64::MAX - i32::MAX as u64 {
        Ok(value as i32)
    } else {
        Err(AutomarkerRequestDecodeError::Int32Overflow { message, field })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use cbc::Encryptor;
    use cbc::cipher::{BlockEncryptMut, block_padding::Pkcs7};

    type Aes128CbcEncryptor = Encryptor<Aes128>;

    fn source_pack() -> ProtocolPack {
        ProtocolPack::from_json(include_bytes!(
            "../protocol-packs/global/steam-24687926/pack.json"
        ))
        .unwrap()
    }

    fn current_pack(build: &str) -> ProtocolPack {
        crate::compatibility_epoch::retarget_protocol_pack(
            &source_pack(),
            "compatibility-fallback",
            "global",
            "steam",
            build,
        )
        .unwrap()
    }

    fn push_varint(out: &mut Vec<u8>, mut value: u64) {
        while value >= 0x80 {
            out.push((value as u8) | 0x80);
            value >>= 7;
        }
        out.push(value as u8);
    }

    fn push_v(out: &mut Vec<u8>, field: u8, value: u64) {
        out.push(field << 3);
        push_varint(out, value);
    }

    fn push_f(out: &mut Vec<u8>, field: u8, value: f32) {
        out.push((field << 3) | 5);
        out.extend_from_slice(&value.to_le_bytes());
    }

    fn push_b(out: &mut Vec<u8>, field: u8, value: &[u8]) {
        out.push((field << 3) | 2);
        push_varint(out, value.len() as u64);
        out.extend_from_slice(value);
    }

    fn position(value: [f32; 4]) -> Vec<u8> {
        let mut out = vec![];
        for (index, value) in value.into_iter().enumerate() {
            push_f(&mut out, index as u8 + 1, value);
        }
        out
    }

    fn envelope(timestamp: u64, opaque: f32) -> Vec<u8> {
        let mut plaintext = vec![];
        push_v(&mut plaintext, 1, timestamp);
        push_f(&mut plaintext, 2, 4.0);
        push_v(&mut plaintext, 3, 4493);
        push_v(&mut plaintext, 4, 4168);
        push_f(&mut plaintext, 6, opaque);
        let iv = [0x5a; 16];
        let mut buffer = plaintext.clone();
        let original = buffer.len();
        buffer.resize(original + 16, 0);
        let ciphertext = Aes128CbcEncryptor::new_from_slices(&SKILL_AES_KEY, &iv)
            .unwrap()
            .encrypt_padded_mut::<Pkcs7>(&mut buffer, original)
            .unwrap();
        let mut mac = <HmacSha256 as Mac>::new_from_slice(&SKILL_HMAC_KEY).unwrap();
        mac.update(&iv);
        mac.update(ciphertext);
        let mut out = iv.to_vec();
        out.extend_from_slice(&mac.finalize().into_bytes());
        out.extend_from_slice(ciphertext);
        out
    }

    fn request(
        marker: u8,
        target: [f32; 4],
        current: [f32; 4],
        begin: u64,
        counter: u64,
    ) -> Vec<u8> {
        request_with(
            marker,
            target,
            current,
            begin,
            counter,
            1_610_614_693 + u64::from(marker),
            1.0,
        )
    }

    fn request_with(
        marker: u8,
        target: [f32; 4],
        current: [f32; 4],
        begin: u64,
        counter: u64,
        skill_uuid: u64,
        current_build_scalar: f32,
    ) -> Vec<u8> {
        let mut param = vec![];
        push_v(&mut param, 1, skill_uuid);
        push_v(&mut param, 2, 1100 + u64::from(marker));
        push_v(&mut param, 3, 1);
        push_v(&mut param, 4, begin);
        push_b(&mut param, 6, &position(target));
        push_b(&mut param, 7, &position(current));
        push_v(&mut param, 10, 1);
        push_v(&mut param, 11, 1);
        let mut inner = vec![];
        push_v(&mut inner, 1, 200 + u64::from(marker));
        push_v(&mut inner, 2, 1);
        push_b(&mut inner, 3, &param);
        push_b(&mut inner, 4, &envelope(begin, current_build_scalar));
        push_v(&mut inner, 5, counter);
        let mut outer = vec![];
        push_b(&mut outer, 1, &inner);
        outer
    }

    #[test]
    fn exact_gate_rejects_neighbor_build_and_wrong_digest() {
        let current = current_pack(AUTOMARKER_REQUEST_BUILD);
        assert_eq!(current.digest(), AUTOMARKER_REQUEST_PACK_DIGEST);
        assert!(supports_observed_automarker_requests(&current));
        assert!(!supports_observed_automarker_requests(&current_pack(
            "25247557"
        )));
        let mut definition = current.definition().clone();
        definition.pack_id.push_str("-different");
        assert!(!supports_observed_automarker_requests(
            &ProtocolPack::build(definition).unwrap()
        ));
    }

    #[test]
    fn decodes_all_six_observed_marker_identities_and_positions() {
        let samples = [
            (
                1,
                [250.35721, 118.0, -64.2384, 250.49268],
                [250.44351, 118.02, -61.48509, 250.49268],
                1_789_176_498_286,
                607,
            ),
            (
                2,
                [246.87148, 118.0, -64.09218, 270.9394],
                [246.95949, 118.02, -61.450314, 270.9394],
                1_789_176_505_357,
                696,
            ),
            (
                3,
                [242.48721, 118.12358, -63.51843, 271.98438],
                [242.60005, 118.02, -61.302887, 271.98438],
                1_789_176_510_696,
                762,
            ),
            (
                4,
                [238.84279, 118.0, -63.759544, 271.9853],
                [238.9814, 118.02, -61.174927, 271.9853],
                1_789_176_515_414,
                822,
            ),
            (
                5,
                [235.98499, 118.03505, -63.687, 273.0794],
                [235.28726, 118.02, -60.974487, 273.0794],
                1_789_176_517_633,
                850,
            ),
            (
                6,
                [232.51979, 118.01601, -63.925617, 273.08215],
                [232.34154, 118.02, -60.81542, 273.08215],
                1_789_176_520_665,
                887,
            ),
        ];
        let pack = current_pack(AUTOMARKER_REQUEST_BUILD);
        let mut scratch = vec![];
        for (marker, target, current, begin, counter) in samples {
            let decoded = decode_observed_automarker_request_into(
                &pack,
                &request(marker, target, current, begin, counter),
                &mut scratch,
            )
            .unwrap();
            assert_eq!(decoded.marker_number, marker);
            assert_eq!(decoded.slot_id, 200 + i32::from(marker));
            assert_eq!(decoded.skill_id, 1100 + i32::from(marker));
            assert_eq!(decoded.skill_uuid, 1_610_614_693 + i32::from(marker));
            assert_eq!(
                decoded.target_position,
                AutomarkerRequestPosition {
                    x: target[0],
                    y: target[1],
                    z: target[2],
                    heading_degrees: target[3]
                }
            );
            assert_eq!(
                decoded.current_position,
                AutomarkerRequestPosition {
                    x: current[0],
                    y: current[1],
                    z: current[2],
                    heading_degrees: current[3]
                }
            );
            assert_eq!(decoded.session_sequence, counter as u32);
            assert_eq!(decoded.attributes.timestamp, begin);
            assert_eq!(decoded.attributes.opaque_current_build_scalar, 1.0);
            assert_eq!(decoded.attributes.charge_speed_pct, None);
            assert!(scratch.is_empty());
        }
    }

    #[test]
    fn mismatched_marker_identity_and_tampered_envelope_fail_closed() {
        let pack = current_pack(AUTOMARKER_REQUEST_BUILD);
        let mut payload = request(1, [1.0, 2.0, 3.0, 4.0], [5.0, 6.0, 7.0, 8.0], 99, 10);
        // Slot 201 is encoded as c9 01; make it slot 202 while skill remains 1101.
        let slot = payload
            .windows(2)
            .position(|window| window == [0xc9, 0x01])
            .unwrap();
        payload[slot] = 0xca;
        assert!(matches!(
            decode_observed_automarker_request_into(&pack, &payload, &mut vec![]),
            Err(AutomarkerRequestDecodeError::InvalidMarkerIdentity { .. })
        ));

        let mut tampered = request(1, [1.0, 2.0, 3.0, 4.0], [5.0, 6.0, 7.0, 8.0], 99, 10);
        let iv = tampered
            .windows(16)
            .position(|window| window == [0x5a; 16])
            .unwrap();
        tampered[iv] ^= 1;
        assert!(matches!(
            decode_observed_automarker_request_into(&pack, &tampered, &mut vec![]),
            Err(AutomarkerRequestDecodeError::MacMismatch)
        ));
    }

    #[test]
    fn rejects_unverified_marker_skill_uuid_namespace() {
        let pack = current_pack(AUTOMARKER_REQUEST_BUILD);
        let payload = request_with(
            1,
            [1.0, 2.0, 3.0, 4.0],
            [5.0, 6.0, 7.0, 8.0],
            99,
            10,
            0x5000_0001,
            1.0,
        );
        assert!(matches!(
            decode_observed_automarker_request_into(&pack, &payload, &mut vec![]),
            Err(AutomarkerRequestDecodeError::InvalidMarkerSkillUuid {
                skill_uuid: 0x5000_0001
            })
        ));
    }

    #[test]
    fn rejects_nonfinite_or_out_of_bounds_positions_and_headings() {
        let pack = current_pack(AUTOMARKER_REQUEST_BUILD);
        for (target, expected_field) in [
            ([f32::NAN, 2.0, 3.0, 4.0], 1),
            ([MAX_ABS_MARKER_COORDINATE + 1.0, 2.0, 3.0, 4.0], 1),
            ([1.0, 2.0, 3.0, f32::NAN], 4),
            ([1.0, 2.0, 3.0, 360.0], 4),
        ] {
            let payload = request(1, target, [5.0, 6.0, 7.0, 8.0], 99, 10);
            assert_eq!(
                decode_observed_automarker_request_into(&pack, &payload, &mut vec![]),
                Err(AutomarkerRequestDecodeError::InvalidPosition {
                    field: expected_field
                })
            );
        }
    }

    #[test]
    fn rejects_nonfinite_or_nonunit_authenticated_current_build_scalar() {
        let pack = current_pack(AUTOMARKER_REQUEST_BUILD);
        for scalar in [f32::NAN, 0.5] {
            let payload = request_with(
                1,
                [1.0, 2.0, 3.0, 4.0],
                [5.0, 6.0, 7.0, 8.0],
                99,
                10,
                1_610_614_694,
                scalar,
            );
            assert!(matches!(
                decode_observed_automarker_request_into(&pack, &payload, &mut vec![]),
                Err(AutomarkerRequestDecodeError::UnsupportedValue { field: 6, .. })
            ));
        }
    }

    #[test]
    fn rejects_negative_begin_time_before_timestamp_comparison() {
        let pack = current_pack(AUTOMARKER_REQUEST_BUILD);
        let payload = request(1, [1.0, 2.0, 3.0, 4.0], [5.0, 6.0, 7.0, 8.0], u64::MAX, 10);
        assert_eq!(
            decode_observed_automarker_request_into(&pack, &payload, &mut vec![]),
            Err(AutomarkerRequestDecodeError::UnsupportedValue {
                field: 4,
                value: -1
            })
        );
    }
}
