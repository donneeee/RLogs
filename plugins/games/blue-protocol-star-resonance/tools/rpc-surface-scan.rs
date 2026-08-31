#![allow(clippy::collapsible_match)]

use std::{
    collections::{BTreeMap, BTreeSet},
    env,
    error::Error,
    fs::{self, File},
    io::{BufRead, BufReader, Read, Seek, SeekFrom},
    path::{Path, PathBuf},
};

use serde::{Deserialize, Serialize};

const SCHEMA_VERSION: u16 = 2;

#[derive(Debug)]
struct Arguments {
    dump: PathBuf,
    game_assembly: PathBuf,
    identity: PathBuf,
    output: PathBuf,
    route_proof: Option<PathBuf>,
}

#[derive(Debug, Deserialize)]
struct RouteProof {
    game_build: String,
    source_identity: RouteProofSourceIdentity,
    server_dispatcher: RouteProofDispatcher,
}

#[derive(Debug, Deserialize)]
struct RouteProofSourceIdentity {
    game_assembly: ArtifactIdentity,
}

#[derive(Debug, Deserialize)]
struct RouteProofDispatcher {
    routes: Vec<RouteProofMapping>,
}

#[derive(Debug, Deserialize)]
struct RouteProofMapping {
    interface_method_ordinal: u32,
    name: String,
    method_id_decimal: u32,
    method_id_hex: String,
}

#[derive(Debug, Deserialize)]
struct BuildIdentity {
    game: String,
    deployment: String,
    channel: String,
    game_build: String,
    distribution_app_id: String,
    metadata: ArtifactIdentity,
    game_assembly: ArtifactIdentity,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
struct ArtifactIdentity {
    byte_length: u64,
    sha256: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    metadata_version: Option<i32>,
}

#[derive(Debug, Serialize)]
struct RpcSurface {
    schema_version: u16,
    generated_by: &'static str,
    game: String,
    deployment: String,
    channel: String,
    distribution_app_id: String,
    build_id: String,
    source_identity: SourceIdentity,
    policy: Policy,
    summary: Summary,
    messages: Vec<MessageSurface>,
    enums: Vec<EnumSurface>,
    services: Vec<ServiceSurface>,
}

#[derive(Debug, Serialize)]
struct SourceIdentity {
    metadata: ArtifactIdentity,
    game_assembly: ArtifactIdentity,
}

#[derive(Debug, Serialize)]
struct Policy {
    absolute_paths_retained: bool,
    native_addresses_retained: bool,
    protobuf_tags_inferred_from_field_order: bool,
    message_field_order_state: &'static str,
    interface_method_ordinal_source: &'static str,
    wire_method_id_state: &'static str,
    service_id_source: &'static str,
    exact_build_packet_replay_required: bool,
    unknown_fields_hidden: bool,
}

#[derive(Debug, Serialize)]
struct Summary {
    messages: usize,
    message_fields: usize,
    enums: usize,
    enum_values: usize,
    services: usize,
    service_methods: usize,
    service_methods_with_exact_wire_id: usize,
    service_methods_without_exact_wire_id: usize,
    services_with_exact_id: usize,
    services_without_exact_id: usize,
}

#[derive(Debug, Clone, Serialize)]
struct MessageSurface {
    full_name: String,
    fields: Vec<MessageField>,
}

#[derive(Debug, Clone, Serialize)]
struct MessageField {
    order: usize,
    name: String,
    field_type: String,
    protobuf_tag: Option<u32>,
    tag_state: &'static str,
}

#[derive(Debug, Clone, Serialize)]
struct EnumSurface {
    full_name: String,
    values: Vec<EnumValue>,
}

#[derive(Debug, Clone, Serialize)]
struct EnumValue {
    name: String,
    value: String,
}

#[derive(Debug, Clone, Serialize)]
struct ServiceSurface {
    name: String,
    service_id: Option<u64>,
    service_id_hex: Option<String>,
    id_state: &'static str,
    methods: Vec<ServiceMethod>,
}

#[derive(Debug, Clone, Serialize)]
struct ServiceMethod {
    interface_method_ordinal: u32,
    wire_method_id: Option<u32>,
    wire_method_id_hex: Option<String>,
    wire_method_id_state: &'static str,
    name: String,
    return_type: String,
    parameters: Vec<ServiceParameter>,
}

#[derive(Debug, Clone, Serialize)]
struct ServiceParameter {
    order: usize,
    name: String,
    parameter_type: String,
    modifier: Option<String>,
}

#[derive(Debug)]
enum ActiveType {
    Message(MessageSurface),
    Enum(EnumSurface),
    Service(ServiceSurface),
    Factory { service: String },
    Other,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Section {
    None,
    Fields,
    Methods,
}

#[derive(Debug)]
struct ParsedDump {
    messages: Vec<MessageSurface>,
    enums: Vec<EnumSurface>,
    services: Vec<ServiceSurface>,
    factory_rvas: BTreeMap<String, u64>,
}

fn main() {
    if let Err(error) = run() {
        eprintln!("RPC surface scan failed: {error}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), Box<dyn Error>> {
    let arguments = parse_arguments(env::args().skip(1))?;
    let identity: BuildIdentity = serde_json::from_slice(&fs::read(&arguments.identity)?)?;
    let dump = parse_dump(&arguments.dump)?;
    let mut services = dump.services;
    let mut services_with_exact_id = 0usize;

    for service in &mut services {
        if let Some(rva) = dump.factory_rvas.get(&service.name) {
            if let Some(service_id) = read_constant_return(&arguments.game_assembly, *rva)? {
                service.service_id = Some(service_id);
                service.service_id_hex = Some(format!("0x{service_id:016x}"));
                service.id_state = "exact_native_factory_return";
                services_with_exact_id += 1;
            } else {
                service.id_state = "factory_present_native_body_not_constant_return";
            }
        }
    }

    if let Some(path) = &arguments.route_proof {
        let proof: RouteProof = serde_json::from_slice(&fs::read(path)?)?;
        if proof.game_build != identity.game_build {
            return Err(format!(
                "route proof build {} does not match identity build {}",
                proof.game_build, identity.game_build
            )
            .into());
        }
        validate_route_proof_identity(&proof, &identity)?;
        apply_route_proof(&mut services, proof)?;
    }

    services.sort_by(|left, right| left.name.cmp(&right.name));
    let message_fields = dump
        .messages
        .iter()
        .map(|message| message.fields.len())
        .sum();
    let enum_values = dump.enums.iter().map(|value| value.values.len()).sum();
    let service_methods = services.iter().map(|service| service.methods.len()).sum();
    let service_methods_with_exact_wire_id = services
        .iter()
        .flat_map(|service| &service.methods)
        .filter(|method| method.wire_method_id.is_some())
        .count();
    let summary = Summary {
        messages: dump.messages.len(),
        message_fields,
        enums: dump.enums.len(),
        enum_values,
        services: services.len(),
        service_methods,
        service_methods_with_exact_wire_id,
        service_methods_without_exact_wire_id: service_methods - service_methods_with_exact_wire_id,
        services_with_exact_id,
        services_without_exact_id: services.len() - services_with_exact_id,
    };

    let surface = RpcSurface {
        schema_version: SCHEMA_VERSION,
        generated_by: "rlogs-bpsr-rpc-surface-scan",
        game: identity.game,
        deployment: identity.deployment,
        channel: identity.channel,
        distribution_app_id: identity.distribution_app_id,
        build_id: identity.game_build,
        source_identity: SourceIdentity {
            metadata: identity.metadata,
            game_assembly: identity.game_assembly,
        },
        policy: Policy {
            absolute_paths_retained: false,
            native_addresses_retained: false,
            protobuf_tags_inferred_from_field_order: false,
            message_field_order_state: "exact_generated_instance_field_order_tag_unavailable",
            interface_method_ordinal_source: "exact_generated_stub_interface_slot",
            wire_method_id_state: "exact_native_build_bound_route_proof_or_unresolved",
            service_id_source: "exact_native_stub_factory_uuid_return",
            exact_build_packet_replay_required: true,
            unknown_fields_hidden: false,
        },
        summary,
        messages: dump.messages,
        enums: dump.enums,
        services,
    };

    if let Some(parent) = arguments.output.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(&arguments.output, serde_json::to_vec_pretty(&surface)?)?;
    println!(
        "wrote {} messages, {} enums, and {} services to {}",
        surface.summary.messages,
        surface.summary.enums,
        surface.summary.services,
        arguments.output.display()
    );
    Ok(())
}

fn parse_arguments(arguments: impl Iterator<Item = String>) -> Result<Arguments, Box<dyn Error>> {
    let mut values = BTreeMap::new();
    let mut arguments = arguments.peekable();
    while let Some(argument) = arguments.next() {
        let key = argument
            .strip_prefix("--")
            .ok_or_else(|| format!("unexpected positional argument {argument}"))?;
        let value = arguments
            .next()
            .ok_or_else(|| format!("missing value for --{key}"))?;
        values.insert(key.to_owned(), value);
    }
    let required = |key: &str| -> Result<PathBuf, Box<dyn Error>> {
        values
            .get(key)
            .map(PathBuf::from)
            .ok_or_else(|| format!("missing --{key}").into())
    };
    Ok(Arguments {
        dump: required("dump")?,
        game_assembly: required("game-assembly")?,
        identity: required("identity")?,
        output: required("output")?,
        route_proof: values.get("route-proof").map(PathBuf::from),
    })
}

fn apply_route_proof(
    services: &mut [ServiceSurface],
    proof: RouteProof,
) -> Result<(), Box<dyn Error>> {
    let service = services
        .iter_mut()
        .find(|service| service.name == "WorldNtf")
        .ok_or("route proof names WorldNtf, but the dump has no WorldNtf service")?;
    let mut ordinals = BTreeSet::new();
    let mut names = BTreeSet::new();
    let mut method_ids = BTreeSet::new();
    for mapping in proof.server_dispatcher.routes {
        if !ordinals.insert(mapping.interface_method_ordinal) {
            return Err(format!(
                "route proof repeats interface ordinal {}",
                mapping.interface_method_ordinal
            )
            .into());
        }
        if !names.insert(mapping.name.clone()) {
            return Err(format!("route proof repeats method name {}", mapping.name).into());
        }
        if !method_ids.insert(mapping.method_id_decimal) {
            return Err(format!(
                "route proof repeats wire method ID 0x{:X}",
                mapping.method_id_decimal
            )
            .into());
        }
        let method = service
            .methods
            .iter_mut()
            .find(|method| method.interface_method_ordinal == mapping.interface_method_ordinal)
            .ok_or_else(|| {
                format!(
                    "route proof ordinal {} is absent from WorldNtf",
                    mapping.interface_method_ordinal
                )
            })?;
        if method.name != mapping.name {
            return Err(format!(
                "route proof ordinal {} names {}, but the dump names {}",
                mapping.interface_method_ordinal, mapping.name, method.name
            )
            .into());
        }
        let expected_hex = format!("0x{:X}", mapping.method_id_decimal);
        if !mapping.method_id_hex.eq_ignore_ascii_case(&expected_hex) {
            return Err(format!(
                "route proof {} has inconsistent decimal {} and hex {}",
                mapping.name, mapping.method_id_decimal, mapping.method_id_hex
            )
            .into());
        }
        if method.wire_method_id.is_some() {
            return Err(format!(
                "route proof attempts to assign {} more than once",
                mapping.name
            )
            .into());
        }
        method.wire_method_id = Some(mapping.method_id_decimal);
        method.wire_method_id_hex = Some(mapping.method_id_hex);
        method.wire_method_id_state = "exact_native_build_bound_route_proof";
    }
    Ok(())
}

fn validate_route_proof_identity(
    proof: &RouteProof,
    identity: &BuildIdentity,
) -> Result<(), Box<dyn Error>> {
    let proof_assembly = &proof.source_identity.game_assembly;
    let identity_assembly = &identity.game_assembly;
    if proof_assembly.byte_length != identity_assembly.byte_length
        || !proof_assembly
            .sha256
            .eq_ignore_ascii_case(&identity_assembly.sha256)
    {
        return Err(format!(
            "route proof GameAssembly identity {} bytes / {} does not match current identity {} bytes / {}",
            proof_assembly.byte_length,
            proof_assembly.sha256,
            identity_assembly.byte_length,
            identity_assembly.sha256
        )
        .into());
    }
    Ok(())
}

fn parse_dump(path: &Path) -> Result<ParsedDump, Box<dyn Error>> {
    let reader = BufReader::new(File::open(path)?);
    let mut namespace = String::new();
    let mut active: Option<ActiveType> = None;
    let mut section = Section::None;
    let mut depth = 0i32;
    let mut pending_slot = None;
    let mut pending_rva = None;
    let mut messages = Vec::new();
    let mut enums = Vec::new();
    let mut services = Vec::new();
    let mut factory_rvas = BTreeMap::new();

    for line in reader.lines() {
        let line = line?;
        let trimmed = line.trim();
        if active.is_none() {
            if let Some(value) = trimmed.strip_prefix("// Namespace: ") {
                namespace = value.to_owned();
                continue;
            }
            active = begin_type(trimmed, &namespace);
            if active.is_some() {
                depth = brace_delta(trimmed);
                section = Section::None;
                pending_slot = None;
                pending_rva = None;
            }
            continue;
        }

        if trimmed == "// Fields" {
            section = Section::Fields;
        } else if trimmed == "// Methods" {
            section = Section::Methods;
        } else if trimmed.starts_with("// Properties") {
            section = Section::None;
        } else if let Some(rva) = parse_rva(trimmed) {
            pending_rva = Some(rva);
        } else if let Some(slot) = parse_slot(trimmed) {
            pending_slot = Some(slot);
        } else if let Some(active_type) = active.as_mut() {
            match active_type {
                ActiveType::Message(message) if section == Section::Fields => {
                    if let Some(field) = parse_message_field(trimmed, message.fields.len() + 1) {
                        message.fields.push(field);
                    }
                }
                ActiveType::Enum(value) => {
                    if let Some(entry) = parse_enum_value(trimmed) {
                        value.values.push(entry);
                    }
                }
                ActiveType::Service(service) if section == Section::Methods => {
                    if let (Some(slot), Some(method)) =
                        (pending_slot.take(), parse_service_method(trimmed))
                    {
                        service.methods.push(ServiceMethod {
                            interface_method_ordinal: slot,
                            wire_method_id: None,
                            wire_method_id_hex: None,
                            wire_method_id_state: "unresolved_in_rpc_surface_inventory",
                            ..method
                        });
                    }
                }
                ActiveType::Factory { service } if section == Section::Methods => {
                    if trimmed.starts_with("public ulong Uuid()") {
                        if let Some(rva) = pending_rva.take() {
                            factory_rvas.insert(service.clone(), rva);
                        }
                    }
                }
                _ => {}
            }
        }

        depth += brace_delta(trimmed);
        if depth == 0 && trimmed.contains('}') {
            match active.take().expect("active type") {
                ActiveType::Message(message) => messages.push(message),
                ActiveType::Enum(value) => enums.push(value),
                ActiveType::Service(mut service) => {
                    service
                        .methods
                        .sort_by_key(|method| method.interface_method_ordinal);
                    services.push(service);
                }
                ActiveType::Factory { .. } | ActiveType::Other => {}
            }
            section = Section::None;
            pending_slot = None;
            pending_rva = None;
        }
    }

    messages.sort_by(|left, right| left.full_name.cmp(&right.full_name));
    enums.sort_by(|left, right| left.full_name.cmp(&right.full_name));
    Ok(ParsedDump {
        messages,
        enums,
        services,
        factory_rvas,
    })
}

fn begin_type(line: &str, namespace: &str) -> Option<ActiveType> {
    if namespace.starts_with("Zproto")
        && line.starts_with("public sealed class ")
        && line.contains(" : IMessage<")
    {
        let name = line
            .strip_prefix("public sealed class ")?
            .split_whitespace()
            .next()?;
        return Some(ActiveType::Message(MessageSurface {
            full_name: qualify(namespace, name),
            fields: Vec::new(),
        }));
    }
    if namespace.starts_with("Zproto") && line.starts_with("public enum ") {
        let name = line
            .strip_prefix("public enum ")?
            .split_whitespace()
            .next()?;
        return Some(ActiveType::Enum(EnumSurface {
            full_name: qualify(namespace, name),
            values: Vec::new(),
        }));
    }
    if namespace == "Zservice" && line.starts_with("public interface I") && line.contains("Stub") {
        let interface = line
            .strip_prefix("public interface I")?
            .split_whitespace()
            .next()?;
        let service = interface.strip_suffix("Stub")?;
        return Some(ActiveType::Service(ServiceSurface {
            name: service.to_owned(),
            service_id: None,
            service_id_hex: None,
            id_state: "unresolved_factory_uuid",
            methods: Vec::new(),
        }));
    }
    if namespace == "Zservice"
        && line.starts_with("public class ")
        && line.contains("StubFactory : IStubFactory")
    {
        let class = line
            .strip_prefix("public class ")?
            .split_whitespace()
            .next()?;
        let service = class.strip_suffix("StubFactory")?;
        return Some(ActiveType::Factory {
            service: service.to_owned(),
        });
    }
    if line.starts_with("public ") || line.starts_with("internal ") || line.starts_with("private ")
    {
        return Some(ActiveType::Other);
    }
    None
}

fn qualify(namespace: &str, name: &str) -> String {
    if namespace.is_empty() {
        name.to_owned()
    } else {
        format!("{namespace}.{name}")
    }
}

fn brace_delta(line: &str) -> i32 {
    line.bytes().fold(0, |depth, byte| match byte {
        b'{' => depth + 1,
        b'}' => depth - 1,
        _ => depth,
    })
}

fn parse_slot(line: &str) -> Option<u32> {
    let value = line.split("Slot: ").nth(1)?;
    value
        .split(|character: char| !character.is_ascii_digit())
        .next()?
        .parse()
        .ok()
}

fn parse_rva(line: &str) -> Option<u64> {
    let value = line.split("RVA: 0x").nth(1)?.split_whitespace().next()?;
    u64::from_str_radix(value, 16).ok()
}

fn parse_message_field(line: &str, order: usize) -> Option<MessageField> {
    let declaration = line.strip_prefix("public ")?.split("; //").next()?;
    if declaration.starts_with("static ") || declaration.starts_with("const ") {
        return None;
    }
    let mut tokens = declaration.split_whitespace().collect::<Vec<_>>();
    let name = tokens.pop()?.to_owned();
    if tokens.is_empty() {
        return None;
    }
    Some(MessageField {
        order,
        name,
        field_type: tokens.join(" "),
        protobuf_tag: None,
        tag_state: "not_in_il2cpp_dummy_metadata",
    })
}

fn parse_enum_value(line: &str) -> Option<EnumValue> {
    let declaration = line.strip_prefix("public const ")?.trim_end_matches(';');
    let (typed_name, value) = declaration.split_once(" = ")?;
    let name = typed_name.split_whitespace().last()?;
    Some(EnumValue {
        name: name.trim().to_owned(),
        value: value.trim().to_owned(),
    })
}

fn parse_service_method(line: &str) -> Option<ServiceMethod> {
    let declaration = line.strip_prefix("public abstract ")?.trim_end_matches(';');
    let open = declaration.find('(')?;
    let close = declaration.rfind(')')?;
    let head = declaration[..open].trim();
    let mut head_tokens = head.split_whitespace().collect::<Vec<_>>();
    let name = head_tokens.pop()?.to_owned();
    let return_type = head_tokens.join(" ");
    let parameters = split_parameters(&declaration[open + 1..close])
        .into_iter()
        .enumerate()
        .filter_map(|(index, parameter)| parse_parameter(&parameter, index + 1))
        .collect();
    Some(ServiceMethod {
        interface_method_ordinal: 0,
        wire_method_id: None,
        wire_method_id_hex: None,
        wire_method_id_state: "unresolved_in_rpc_surface_inventory",
        name,
        return_type,
        parameters,
    })
}

fn split_parameters(value: &str) -> Vec<String> {
    let mut parameters = Vec::new();
    let mut start = 0usize;
    let mut depth = 0i32;
    for (index, character) in value.char_indices() {
        match character {
            '<' => depth += 1,
            '>' => depth -= 1,
            ',' if depth == 0 => {
                parameters.push(value[start..index].trim().to_owned());
                start = index + 1;
            }
            _ => {}
        }
    }
    let tail = value[start..].trim();
    if !tail.is_empty() {
        parameters.push(tail.to_owned());
    }
    parameters
}

fn parse_parameter(value: &str, order: usize) -> Option<ServiceParameter> {
    let mut tokens = value.split_whitespace().collect::<Vec<_>>();
    let name = tokens.pop()?.to_owned();
    let modifier = tokens
        .first()
        .filter(|value| matches!(**value, "in" | "out" | "ref"))
        .map(|value| (*value).to_owned());
    if modifier.is_some() {
        tokens.remove(0);
    }
    Some(ServiceParameter {
        order,
        name,
        parameter_type: tokens.join(" "),
        modifier,
    })
}

fn read_constant_return(path: &Path, rva: u64) -> Result<Option<u64>, Box<dyn Error>> {
    let mut file = File::open(path)?;
    let file_offset = rva_to_file_offset(&mut file, rva)?;
    file.seek(SeekFrom::Start(file_offset))?;
    let mut bytes = [0u8; 16];
    file.read_exact(&mut bytes)?;
    match bytes {
        [0xb8, a, b, c, d, 0xc3, ..] => Ok(Some(u32::from_le_bytes([a, b, c, d]) as u64)),
        [0x48, 0xb8, a, b, c, d, e, f, g, h, 0xc3, ..] => {
            Ok(Some(u64::from_le_bytes([a, b, c, d, e, f, g, h])))
        }
        _ => Ok(None),
    }
}

fn rva_to_file_offset(file: &mut File, rva: u64) -> Result<u64, Box<dyn Error>> {
    let mut dos = [0u8; 0x40];
    file.seek(SeekFrom::Start(0))?;
    file.read_exact(&mut dos)?;
    if &dos[..2] != b"MZ" {
        return Err("game assembly is not a PE image".into());
    }
    let pe_offset = u32::from_le_bytes(dos[0x3c..0x40].try_into()?) as u64;
    file.seek(SeekFrom::Start(pe_offset))?;
    let mut coff = [0u8; 24];
    file.read_exact(&mut coff)?;
    if &coff[..4] != b"PE\0\0" {
        return Err("game assembly has an invalid PE signature".into());
    }
    let section_count = u16::from_le_bytes(coff[6..8].try_into()?) as usize;
    let optional_header_size = u16::from_le_bytes(coff[20..22].try_into()?) as u64;
    file.seek(SeekFrom::Current(optional_header_size as i64))?;
    for _ in 0..section_count {
        let mut section = [0u8; 40];
        file.read_exact(&mut section)?;
        let virtual_size = u32::from_le_bytes(section[8..12].try_into()?) as u64;
        let virtual_address = u32::from_le_bytes(section[12..16].try_into()?) as u64;
        let raw_size = u32::from_le_bytes(section[16..20].try_into()?) as u64;
        let raw_offset = u32::from_le_bytes(section[20..24].try_into()?) as u64;
        if rva >= virtual_address && rva < virtual_address + virtual_size.max(raw_size) {
            return Ok(raw_offset + (rva - virtual_address));
        }
    }
    Err(format!("RVA 0x{rva:x} is outside every PE section").into())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn parses_message_fields_without_inventing_tags() {
        let path = temp_file(
            "// Namespace: Zproto\npublic sealed class Demo : IMessage<Demo>\n{\n\t// Fields\n\tprivate static readonly object _parser; // 0x0\n\tpublic int Id; // 0x10\n\tpublic RepeatedField<int> Values; // 0x18\n}\n",
        );
        let parsed = parse_dump(&path).expect("parse dump");
        fs::remove_file(path).ok();
        assert_eq!(parsed.messages.len(), 1);
        assert_eq!(parsed.messages[0].fields.len(), 2);
        assert_eq!(parsed.messages[0].fields[1].name, "Values");
        assert_eq!(parsed.messages[0].fields[1].protobuf_tag, None);
    }

    #[test]
    fn parses_stub_slots_as_interface_method_ordinals_only() {
        let path = temp_file(
            "// Namespace: Zservice\npublic interface IWorldNtfStub\n{\n\t// Methods\n\t// RVA: -1 Offset: -1 Slot: 21\n\tpublic abstract void SyncContainerData(IStubCall call, CharSerialize vData);\n}\n",
        );
        let parsed = parse_dump(&path).expect("parse dump");
        fs::remove_file(path).ok();
        assert_eq!(parsed.services.len(), 1);
        assert_eq!(parsed.services[0].methods[0].interface_method_ordinal, 21);
        assert_eq!(parsed.services[0].methods[0].wire_method_id, None);
        assert_eq!(parsed.services[0].methods[0].parameters.len(), 2);
    }

    #[test]
    fn parses_factory_rva() {
        let path = temp_file(
            "// Namespace: Zservice\npublic class WorldNtfStubFactory : IStubFactory\n{\n\t// Methods\n\t// RVA: 0x588E660 Offset: 0x0 VA: 0x0\n\tpublic ulong Uuid() { }\n}\n",
        );
        let parsed = parse_dump(&path).expect("parse dump");
        fs::remove_file(path).ok();
        assert_eq!(parsed.factory_rvas.get("WorldNtf"), Some(&0x588e660));
    }

    #[test]
    fn reviewed_route_proof_requires_matching_ordinal_and_name() {
        let mut services = vec![ServiceSurface {
            name: "WorldNtf".to_owned(),
            service_id: None,
            service_id_hex: None,
            id_state: "unresolved_factory_uuid",
            methods: vec![ServiceMethod {
                interface_method_ordinal: 42,
                wire_method_id: None,
                wire_method_id_hex: None,
                wire_method_id_state: "unresolved_in_rpc_surface_inventory",
                name: "SyncServerSkillStageEnd".to_owned(),
                return_type: "void".to_owned(),
                parameters: Vec::new(),
            }],
        }];
        let proof = RouteProof {
            game_build: "test".to_owned(),
            source_identity: RouteProofSourceIdentity {
                game_assembly: ArtifactIdentity {
                    byte_length: 1,
                    sha256: "test".to_owned(),
                    metadata_version: None,
                },
            },
            server_dispatcher: RouteProofDispatcher {
                routes: vec![RouteProofMapping {
                    interface_method_ordinal: 42,
                    name: "SyncServerSkillStageEnd".to_owned(),
                    method_id_decimal: 0x3004,
                    method_id_hex: "0x3004".to_owned(),
                }],
            },
        };
        apply_route_proof(&mut services, proof).expect("apply exact route proof");
        let method = &services[0].methods[0];
        assert_eq!(method.wire_method_id, Some(0x3004));
        assert_eq!(
            method.wire_method_id_state,
            "exact_native_build_bound_route_proof"
        );
    }

    #[test]
    fn route_proof_rejects_duplicate_wire_ids() {
        let mut services = vec![ServiceSurface {
            name: "WorldNtf".to_owned(),
            service_id: None,
            service_id_hex: None,
            id_state: "unresolved_factory_uuid",
            methods: vec![
                test_service_method(1, "First"),
                test_service_method(2, "Second"),
            ],
        }];
        let proof = RouteProof {
            game_build: "test".to_owned(),
            source_identity: RouteProofSourceIdentity {
                game_assembly: ArtifactIdentity {
                    byte_length: 1,
                    sha256: "test".to_owned(),
                    metadata_version: None,
                },
            },
            server_dispatcher: RouteProofDispatcher {
                routes: vec![test_route(1, "First", 3), test_route(2, "Second", 3)],
            },
        };
        let error = apply_route_proof(&mut services, proof)
            .expect_err("duplicate wire ID must fail")
            .to_string();
        assert!(error.contains("repeats wire method ID 0x3"));
    }

    #[test]
    fn splits_generic_parameters_without_breaking_map_types() {
        let values = split_parameters(
            "IStubCall call, MapField<int, ProfessionInfo> infos, in ReadOnlySequence<byte> data",
        );
        assert_eq!(values.len(), 3);
        let parsed = parse_parameter(&values[2], 3).expect("parameter");
        assert_eq!(parsed.modifier.as_deref(), Some("in"));
        assert_eq!(parsed.parameter_type, "ReadOnlySequence<byte>");
    }

    fn temp_file(contents: &str) -> PathBuf {
        let path = env::temp_dir().join(format!(
            "rlogs-rpc-surface-{}-{}.cs",
            std::process::id(),
            contents.len()
        ));
        let mut file = File::create(&path).expect("create temp dump");
        file.write_all(contents.as_bytes())
            .expect("write temp dump");
        path
    }

    fn test_service_method(ordinal: u32, name: &str) -> ServiceMethod {
        ServiceMethod {
            interface_method_ordinal: ordinal,
            wire_method_id: None,
            wire_method_id_hex: None,
            wire_method_id_state: "unresolved_in_rpc_surface_inventory",
            name: name.to_owned(),
            return_type: "void".to_owned(),
            parameters: Vec::new(),
        }
    }

    fn test_route(ordinal: u32, name: &str, method_id: u32) -> RouteProofMapping {
        RouteProofMapping {
            interface_method_ordinal: ordinal,
            name: name.to_owned(),
            method_id_decimal: method_id,
            method_id_hex: format!("0x{method_id:X}"),
        }
    }
}
