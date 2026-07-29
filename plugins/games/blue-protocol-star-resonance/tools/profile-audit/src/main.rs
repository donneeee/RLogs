use std::{
    collections::BTreeSet,
    ffi::{OsStr, OsString},
    fs::File,
    io::BufReader,
    path::PathBuf,
};

use rlogs_events::{CanonicalEvent, RegionIdentity, WorldContext};
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
    let mut summary = ProfileAuditSummary::default();
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
            match event.event {
                CanonicalEvent::CharacterProfileObserved { profile } => {
                    summary.profile_events = summary.profile_events.saturating_add(1);
                    let profile = CharacterProfilePatch::from_game_event(&profile)?;
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
    serde_json::to_writer_pretty(std::io::stdout().lock(), &summary)?;
    println!();
    Ok(())
}

#[derive(Debug, Default, Serialize)]
struct ProfileAuditSummary {
    candidate_packets: u64,
    decoded_packets: u64,
    decode_failed_packets: u64,
    profile_events: u64,
    world_events: u64,
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
    level: bool,
    combat_power: bool,
}

impl ProfileFieldPresence {
    fn observe(&mut self, profile: &CharacterProfilePatch) {
        self.character_uid = !profile.character.character_id.is_empty();
        self.display_name |= profile.display_name.is_some();
        self.display_uid |= profile.display_id.is_some();
        self.server_id |= profile.server_id.is_some();
        self.class_id |= profile.class_id.is_some();
        self.level |= profile.level.is_some();
        self.combat_power |= profile.combat_power.is_some();
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
}

fn arguments() -> Result<Arguments, String> {
    parse_arguments(std::env::args_os().skip(1))
}

fn parse_arguments(arguments: impl IntoIterator<Item = OsString>) -> Result<Arguments, String> {
    let mut private_research = false;
    let mut pack = None;
    let mut journal = None;
    let mut arguments = arguments.into_iter();

    while let Some(argument) = arguments.next() {
        if argument == OsStr::new("--private-research") {
            private_research = true;
        } else if argument == OsStr::new("--pack") {
            pack = unique_value(pack, arguments.next(), "--pack")?;
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
    "usage: rlogs-profile-audit --private-research --pack <pack.json> <journal.jsonl>".into()
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
