//! Exact, build-locked extraction of a generated IL2CPP RPC dispatch table.
//!
//! The generated BPSR stub returns obfuscated literals from `GetMethodName`.
//! Each literal is a single-byte XOR of `<service>::<method>`, so the known
//! service prefix proves the key and recovers the complete method name. This
//! offline tool combines that literal proof with the native low, special, and
//! high dispatcher branches, then requires a one-to-one match with the managed
//! service interface recorded by `rpc-surface-scan`.

use std::{
    collections::{BTreeMap, BTreeSet},
    env,
    error::Error,
    fs::{self, File},
    io::Write,
    path::PathBuf,
};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

const SCHEMA_VERSION: u16 = 2;

#[derive(Debug)]
struct Arguments {
    surface: PathBuf,
    dump: PathBuf,
    game_assembly: PathBuf,
    string_literals: PathBuf,
    string_literal_identity: Option<PathBuf>,
    string_literal_rva_delta: i64,
    identity: PathBuf,
    service: String,
    output: PathBuf,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
struct ArtifactIdentity {
    byte_length: u64,
    sha256: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    metadata_version: Option<i32>,
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

#[derive(Debug, Deserialize)]
struct RpcSurface {
    build_id: String,
    source_identity: RpcSourceIdentity,
    services: Vec<RpcService>,
}

#[derive(Debug, Deserialize)]
struct RpcSourceIdentity {
    game_assembly: ArtifactIdentity,
}

#[derive(Debug, Deserialize)]
struct RpcService {
    name: String,
    methods: Vec<RpcMethod>,
}

#[derive(Debug, Deserialize)]
struct RpcMethod {
    interface_method_ordinal: u32,
    name: String,
}

#[derive(Debug, Deserialize)]
struct StringLiteral {
    value: String,
    address: String,
}

#[derive(Debug, Serialize)]
struct Report {
    schema_version: u16,
    generated_by: &'static str,
    game: String,
    deployment: String,
    channel: String,
    distribution_app_id: String,
    game_build: String,
    purpose: &'static str,
    source_identity: SourceIdentity,
    policy: Policy,
    summary: Summary,
    server_dispatcher: DispatcherReport,
}

#[derive(Debug, Serialize)]
struct SourceIdentity {
    metadata: ArtifactIdentity,
    game_assembly: ArtifactIdentity,
    rpc_surface: ArtifactIdentity,
    il2cpp_dump: ArtifactIdentity,
    string_literals: ArtifactIdentity,
    string_literal_source_metadata: ArtifactIdentity,
    string_literal_rva_delta: i64,
}

#[derive(Debug, Serialize)]
struct Policy {
    offline_research_only: bool,
    exact_build_only: bool,
    declaration_order_used_for_identity: bool,
    obfuscated_literal_plaintext_proven: bool,
    unresolved_routes_guessed: bool,
    undefined_dispatch_ids_retained: bool,
    exact_build_packet_replay_required: bool,
    string_literal_rvas_rebased_from_identical_metadata: bool,
}

#[derive(Debug, Serialize)]
struct Summary {
    managed_interface_methods: usize,
    exact_wire_routes: usize,
    low_switch_routes: usize,
    special_routes: usize,
    high_routes: usize,
    undefined_method_ids: usize,
    missing_managed_methods: usize,
    extra_native_methods: usize,
}

#[derive(Debug, Serialize)]
struct DispatcherReport {
    managed_name: String,
    rva_hex: String,
    low_dispatch_max_id_decimal: u32,
    low_dispatch_max_id_hex: String,
    low_switch_max_id_decimal: u32,
    low_switch_max_id_hex: String,
    high_route_base_decimal: u32,
    high_route_base_hex: String,
    undefined_literal: String,
    decoding_basis: &'static str,
    routes: Vec<Route>,
    undefined_method_ids: Vec<WireId>,
    undispatched_managed_methods: Vec<UndispatchedManagedMethod>,
}

#[derive(Debug, Serialize)]
struct UndispatchedManagedMethod {
    interface_method_ordinal: u32,
    name: String,
}

#[derive(Debug, Clone, Serialize)]
struct Route {
    interface_method_ordinal: u32,
    name: String,
    method_id_decimal: u32,
    method_id_hex: String,
    dispatch_class: &'static str,
    return_stub_rva_hex: String,
    literal_slot_rva_hex: String,
    xor_key_decimal: u8,
    proof_state: &'static str,
}

#[derive(Debug, Serialize)]
struct WireId {
    method_id_decimal: u32,
    method_id_hex: String,
}

#[derive(Debug, Clone)]
struct NativeRoute {
    method_id: u32,
    name: String,
    class: &'static str,
    return_stub_rva: u64,
    literal_slot_rva: u64,
    xor_key: u8,
}

#[derive(Debug)]
struct PeImage {
    bytes: Vec<u8>,
    sections: Vec<PeSection>,
}

#[derive(Debug)]
struct PeSection {
    virtual_address: u64,
    virtual_size: u64,
    raw_offset: u64,
    raw_size: u64,
}

#[derive(Debug)]
struct DispatcherShape {
    low_top: u32,
    low_switch_max: u32,
    high_base: u32,
    routes: Vec<NativeRoute>,
    undefined_ids: Vec<u32>,
    undefined_literal: String,
}

fn main() {
    if let Err(error) = run() {
        eprintln!("RPC dispatch route proof failed: {error}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), Box<dyn Error>> {
    let args = arguments()?;
    let identity_bytes = fs::read(&args.identity)?;
    let identity: BuildIdentity = serde_json::from_slice(&identity_bytes)?;
    let surface_bytes = fs::read(&args.surface)?;
    let surface: RpcSurface = serde_json::from_slice(&surface_bytes)?;
    if surface.build_id != identity.game_build {
        return Err(format!(
            "RPC surface build {} does not match identity build {}",
            surface.build_id, identity.game_build
        )
        .into());
    }
    if surface.source_identity.game_assembly.byte_length != identity.game_assembly.byte_length
        || surface.source_identity.game_assembly.sha256 != identity.game_assembly.sha256
    {
        return Err("RPC surface and build identity name different GameAssembly inputs".into());
    }

    let assembly_bytes = fs::read(&args.game_assembly)?;
    validate_identity("GameAssembly", &assembly_bytes, &identity.game_assembly)?;
    let dump_bytes = fs::read(&args.dump)?;
    let dump_text = std::str::from_utf8(&dump_bytes)?;
    let string_literal_bytes = fs::read(&args.string_literals)?;
    let string_literals: Vec<StringLiteral> = serde_json::from_slice(&string_literal_bytes)?;
    let literal_source_metadata = if let Some(path) = &args.string_literal_identity {
        let source: BuildIdentity = serde_json::from_slice(&fs::read(path)?)?;
        if source.metadata.byte_length != identity.metadata.byte_length
            || source.metadata.sha256 != identity.metadata.sha256
            || source.metadata.metadata_version != identity.metadata.metadata_version
        {
            return Err(
                "string-literal source and current build have different metadata identities".into(),
            );
        }
        source.metadata
    } else {
        if args.string_literal_rva_delta != 0 {
            return Err(
                "a non-zero --string-literal-rva-delta requires --string-literal-identity".into(),
            );
        }
        identity.metadata.clone()
    };
    let literal_map = rebase_literal_map(
        parse_literal_map(string_literals)?,
        args.string_literal_rva_delta,
    )?;
    let dispatcher_rva = find_dispatcher_rva(dump_text, &args.service)?;
    let image = PeImage::parse(assembly_bytes)?;
    let shape = extract_dispatcher(&image, dispatcher_rva, &args.service, &literal_map)?;

    let managed_service = surface
        .services
        .iter()
        .find(|service| service.name == args.service)
        .ok_or_else(|| format!("RPC surface has no {} service", args.service))?;
    let managed_by_name: BTreeMap<_, _> = managed_service
        .methods
        .iter()
        .map(|method| (method.name.as_str(), method.interface_method_ordinal))
        .collect();
    if managed_by_name.len() != managed_service.methods.len() {
        return Err(format!("{} has duplicate managed method names", args.service).into());
    }
    let native_by_name: BTreeMap<_, _> = shape
        .routes
        .iter()
        .map(|route| (route.name.as_str(), route))
        .collect();
    if native_by_name.len() != shape.routes.len() {
        return Err(format!("{} dispatcher has duplicate decoded routes", args.service).into());
    }
    let managed_names: BTreeSet<_> = managed_by_name.keys().copied().collect();
    let native_names: BTreeSet<_> = native_by_name.keys().copied().collect();
    let missing: Vec<_> = managed_names.difference(&native_names).copied().collect();
    let extra: Vec<_> = native_names.difference(&managed_names).copied().collect();
    if !extra.is_empty() {
        return Err(format!(
            "native/managed route reconciliation failed; missing={missing:?}, extra={extra:?}"
        )
        .into());
    }
    let undispatched_managed_methods = missing
        .iter()
        .map(|name| UndispatchedManagedMethod {
            interface_method_ordinal: managed_by_name[*name],
            name: (*name).to_owned(),
        })
        .collect::<Vec<_>>();

    let mut routes = Vec::with_capacity(shape.routes.len());
    for native in &shape.routes {
        routes.push(Route {
            interface_method_ordinal: managed_by_name[native.name.as_str()],
            name: native.name.clone(),
            method_id_decimal: native.method_id,
            method_id_hex: format!("0x{:X}", native.method_id),
            dispatch_class: native.class,
            return_stub_rva_hex: format!("0x{:X}", native.return_stub_rva),
            literal_slot_rva_hex: format!("0x{:X}", native.literal_slot_rva),
            xor_key_decimal: native.xor_key,
            proof_state: "exact_native_dispatch_and_literal_plaintext",
        });
    }
    routes.sort_by_key(|route| route.interface_method_ordinal);
    let low_switch_routes = routes
        .iter()
        .filter(|route| route.dispatch_class == "low_switch")
        .count();
    let special_routes = routes
        .iter()
        .filter(|route| route.dispatch_class == "special")
        .count();
    let high_routes = routes
        .iter()
        .filter(|route| route.dispatch_class == "high")
        .count();
    let report = Report {
        schema_version: SCHEMA_VERSION,
        generated_by: "rlogs-bpsr-rpc-dispatch-route-proof",
        game: identity.game,
        deployment: identity.deployment,
        channel: identity.channel,
        distribution_app_id: identity.distribution_app_id,
        game_build: identity.game_build,
        purpose: "exact_build_locked_rpc_method_route_recovery",
        source_identity: SourceIdentity {
            metadata: identity.metadata,
            game_assembly: identity.game_assembly,
            rpc_surface: artifact_identity(&surface_bytes, None),
            il2cpp_dump: artifact_identity(&dump_bytes, None),
            string_literals: artifact_identity(&string_literal_bytes, None),
            string_literal_source_metadata: literal_source_metadata,
            string_literal_rva_delta: args.string_literal_rva_delta,
        },
        policy: Policy {
            offline_research_only: true,
            exact_build_only: true,
            declaration_order_used_for_identity: false,
            obfuscated_literal_plaintext_proven: true,
            unresolved_routes_guessed: false,
            undefined_dispatch_ids_retained: true,
            exact_build_packet_replay_required: true,
            string_literal_rvas_rebased_from_identical_metadata: args.string_literal_rva_delta != 0,
        },
        summary: Summary {
            managed_interface_methods: managed_service.methods.len(),
            exact_wire_routes: routes.len(),
            low_switch_routes,
            special_routes,
            high_routes,
            undefined_method_ids: shape.undefined_ids.len(),
            missing_managed_methods: missing.len(),
            extra_native_methods: 0,
        },
        server_dispatcher: DispatcherReport {
            managed_name: format!("Zservice.{}Stub.GetMethodName", args.service),
            rva_hex: format!("0x{dispatcher_rva:X}"),
            low_dispatch_max_id_decimal: shape.low_top,
            low_dispatch_max_id_hex: format!("0x{:X}", shape.low_top),
            low_switch_max_id_decimal: shape.low_switch_max,
            low_switch_max_id_hex: format!("0x{:X}", shape.low_switch_max),
            high_route_base_decimal: shape.high_base,
            high_route_base_hex: format!("0x{:X}", shape.high_base),
            undefined_literal: shape.undefined_literal,
            decoding_basis: "each defined returned literal has one XOR key for the exact `<service>::` prefix and remaining method-name plaintext; short opaque undefined sentinels are proven only by their shared low-switch return stub; every decoded native name must match exactly one managed interface method",
            routes,
            undefined_method_ids: shape
                .undefined_ids
                .into_iter()
                .map(|method_id| WireId {
                    method_id_decimal: method_id,
                    method_id_hex: format!("0x{method_id:X}"),
                })
                .collect(),
            undispatched_managed_methods,
        },
    };

    if let Some(parent) = args.output.parent() {
        fs::create_dir_all(parent)?;
    }
    let mut output = File::create(&args.output)?;
    serde_json::to_writer_pretty(&mut output, &report)?;
    output.write_all(b"\n")?;
    println!(
        "proved {} exact {} routes ({} low-switch, {} special, {} high); retained {} undefined IDs in {}",
        report.summary.exact_wire_routes,
        args.service,
        report.summary.low_switch_routes,
        report.summary.special_routes,
        report.summary.high_routes,
        report.summary.undefined_method_ids,
        args.output.display()
    );
    Ok(())
}

fn extract_dispatcher(
    image: &PeImage,
    dispatcher_rva: u64,
    service: &str,
    literals: &BTreeMap<u64, String>,
) -> Result<DispatcherShape, Box<dyn Error>> {
    let code = image.read_rva(dispatcher_rva, 0x1000)?;
    let branch_offset = find_pattern(code, &[0x83, 0xFB], 0)
        .filter(|offset| code.get(offset + 3) == Some(&0x76))
        .ok_or("dispatcher low-range branch was not found")?;
    let low_top = code[branch_offset + 2] as u32;
    let low_target = branch_target_rel8(dispatcher_rva, branch_offset + 3, code)?;
    if code.get(branch_offset + 5..branch_offset + 8) != Some(&[0x83, 0xFB, 0x52]) {
        return Err("dispatcher special-route comparison shape changed".into());
    }
    let special_id = code[branch_offset + 7] as u32;
    let special_stub = dispatcher_rva + branch_offset as u64 + 10;
    let special_route = decode_stub(
        image,
        special_stub,
        service,
        literals,
        special_id,
        "special",
    )?;

    let high_start = branch_target_rel8(dispatcher_rva, branch_offset + 8, code)?;
    let high_bytes = image.read_rva(high_start, low_target.saturating_sub(high_start) as usize)?;
    if high_bytes.get(0..2) != Some(&[0x8D, 0x8B]) {
        return Err("dispatcher high-route base instruction changed".into());
    }
    let high_displacement = read_i32(high_bytes, 2)?;
    let high_base = high_displacement
        .checked_neg()
        .ok_or("dispatcher high-route base overflow")? as u32;
    let mut high_stubs = Vec::new();
    let mut cursor = 0usize;
    while let Some(offset) = find_pattern(high_bytes, &[0x48, 0x8B, 0x05], cursor) {
        let stub_rva = high_start + offset as u64;
        if image.read_rva(stub_rva + 7, 6)? == [0x48, 0x83, 0xC4, 0x20, 0x5B, 0xC3] {
            high_stubs.push(stub_rva);
        }
        cursor = offset + 3;
    }
    if high_stubs.is_empty() {
        return Err("dispatcher high-route stubs were not found".into());
    }
    high_stubs.sort_unstable();
    high_stubs.reverse();
    let mut routes = Vec::new();
    for (index, stub) in high_stubs.into_iter().enumerate() {
        routes.push(decode_stub(
            image,
            stub,
            service,
            literals,
            high_base + index as u32,
            "high",
        )?);
    }

    let low = image.read_rva(low_target, 64)?;
    if low.get(0..6) != Some(&[0x8D, 0x43, 0xFF, 0x83, 0xF8, low[5]]) {
        return Err("dispatcher low-switch range instruction changed".into());
    }
    let low_switch_max = low[5] as u32 + 1;
    if low.get(6..8) != Some(&[0x0F, 0x87]) {
        return Err("dispatcher low-switch overflow branch changed".into());
    }
    let overflow_target = branch_target_rel32(low_target, 6, low)?;
    let table_pattern = [0x48, 0x8D, 0x15];
    let table_offset = find_pattern(low, &table_pattern, 0)
        .ok_or("dispatcher low-switch table base was not found")?;
    let base_displacement = read_i32(low, table_offset + 3)? as i64;
    let dispatch_base = (low_target + table_offset as u64 + 7)
        .checked_add_signed(base_displacement)
        .ok_or("dispatcher table base overflow")?;
    let selector_pattern = [0x0F, 0xB6, 0x84, 0x0A];
    let selector_offset = find_pattern(low, &selector_pattern, table_offset)
        .ok_or("dispatcher selector table instruction was not found")?;
    let selector_displacement = read_u32(low, selector_offset + 4)? as u64;
    let target_pattern = [0x8B, 0x8C, 0x82];
    let target_offset = find_pattern(low, &target_pattern, selector_offset)
        .ok_or("dispatcher target table instruction was not found")?;
    let target_displacement = read_u32(low, target_offset + 3)? as u64;
    let selector_table_rva = dispatch_base + selector_displacement;
    let target_table_rva = dispatch_base + target_displacement;
    let selectors = image.read_rva(selector_table_rva, low_switch_max as usize)?;
    let max_selector = selectors
        .iter()
        .copied()
        .max()
        .ok_or("dispatcher selector table is empty")? as usize;
    let targets = image.read_rva(target_table_rva, (max_selector + 1) * 4)?;

    let mut low_routes = Vec::with_capacity(selectors.len());
    for (index, selector) in selectors.iter().copied().enumerate() {
        let target = read_i32(targets, selector as usize * 4)? as i64;
        let stub_rva = dispatch_base
            .checked_add_signed(target)
            .ok_or("dispatcher switch target overflow")?;
        let method_id = index as u32 + 1;
        low_routes.push(decode_stub(
            image,
            stub_rva,
            service,
            literals,
            method_id,
            "low_switch",
        )?);
    }

    // Recent clients replace the old XOR-encoded `UndefinedMethod` sentinel
    // with a short opaque token. Prove that sentinel from the switch itself:
    // undefined IDs share one return stub/literal while defined routes are
    // unique. A single opaque token is never silently treated as undefined.
    let mut opaque_counts = BTreeMap::<String, usize>::new();
    for route in &low_routes {
        if opaque_literal(&route.name).is_some() {
            *opaque_counts.entry(route.name.clone()).or_default() += 1;
        }
    }
    let opaque_undefined = opaque_counts
        .into_iter()
        .max_by_key(|(_, count)| *count)
        .filter(|(_, count)| *count > 1)
        .map(|(name, _)| name);
    let mut undefined_literal = None;
    let mut undefined_ids = Vec::new();
    for route in low_routes {
        if route.name == "UndefinedMethod"
            || opaque_undefined.as_deref() == Some(route.name.as_str())
        {
            undefined_literal.get_or_insert_with(|| {
                opaque_literal(&route.name)
                    .unwrap_or(route.name.as_str())
                    .to_owned()
            });
            undefined_ids.push(route.method_id);
        } else if let Some(literal) = opaque_literal(&route.name) {
            return Err(format!(
                "defined low-switch route {:#X} has unproved opaque literal {:?}",
                route.method_id, literal
            )
            .into());
        } else {
            routes.push(route);
        }
    }

    let overflow = image.read_rva(overflow_target, 32)?;
    let low_top_route = decode_stub(
        image,
        overflow_target,
        service,
        literals,
        low_top,
        "special",
    )?;
    if overflow.get(7..10) != Some(&[0x83, 0xFB, low_top as u8]) {
        return Err("dispatcher low-top special comparison changed".into());
    }
    if low_top_route.name == "UndefinedMethod" {
        return Err("dispatcher low-top special route decoded as undefined".into());
    }
    routes.push(low_top_route);
    if special_route.name == "UndefinedMethod" {
        return Err("dispatcher adjacent special route decoded as undefined".into());
    }
    routes.push(special_route);
    for method_id in (low_switch_max + 1)..low_top {
        undefined_ids.push(method_id);
    }
    undefined_ids.sort_unstable();
    undefined_ids.dedup();
    routes.sort_by_key(|route| route.method_id);

    let undefined_literal = undefined_literal.ok_or("dispatcher has no undefined sentinel")?;
    if special_id != low_top + 1 {
        return Err(format!(
            "dispatcher special ID {special_id:#X} is not adjacent to low top {low_top:#X}"
        )
        .into());
    }
    Ok(DispatcherShape {
        low_top,
        low_switch_max,
        high_base,
        routes,
        undefined_ids,
        undefined_literal,
    })
}

fn decode_stub(
    image: &PeImage,
    stub_rva: u64,
    service: &str,
    literals: &BTreeMap<u64, String>,
    method_id: u32,
    class: &'static str,
) -> Result<NativeRoute, Box<dyn Error>> {
    let stub = image.read_rva(stub_rva, 7)?;
    if stub.get(0..3) != Some(&[0x48, 0x8B, 0x05]) {
        return Err(
            format!("route {method_id:#X} target {stub_rva:#X} is not a literal load").into(),
        );
    }
    let displacement = read_i32(stub, 3)? as i64;
    let slot_rva = (stub_rva + 7)
        .checked_add_signed(displacement)
        .ok_or("literal slot overflow")?;
    let encrypted = literals
        .get(&slot_rva)
        .ok_or_else(|| format!("stringliteral.json has no slot {slot_rva:#X}"))?;
    let (name, key) = match decode_literal(service, encrypted) {
        Ok(decoded) => decoded,
        Err(_) if encrypted.chars().count() < format!("{service}::").len() => {
            (format!("__opaque_literal__:{encrypted}"), 0)
        }
        Err(error) => {
            return Err(format!(
                "route {method_id:#X} slot {slot_rva:#X} literal_chars={} literal={:?}: {error}",
                encrypted.chars().count(),
                encrypted.escape_debug().to_string()
            )
            .into());
        }
    };
    Ok(NativeRoute {
        method_id,
        name,
        class,
        return_stub_rva: stub_rva,
        literal_slot_rva: slot_rva,
        xor_key: key,
    })
}

fn opaque_literal(name: &str) -> Option<&str> {
    name.strip_prefix("__opaque_literal__:")
}

fn decode_literal(service: &str, encrypted: &str) -> Result<(String, u8), Box<dyn Error>> {
    let prefix = format!("{service}::");
    let encrypted = encrypted
        .chars()
        .map(|character| {
            u8::try_from(u32::from(character)).map_err(|_| {
                format!(
                    "dispatcher literal contains non-byte character U+{:04X}",
                    u32::from(character)
                )
            })
        })
        .collect::<Result<Vec<_>, _>>()?;
    if encrypted.len() < prefix.len() || !prefix.is_ascii() {
        return Err("dispatcher literal is shorter than its service prefix".into());
    }
    let prefix = prefix.as_bytes();
    let key = encrypted[0] ^ prefix[0];
    if key == 0 {
        return Err("dispatcher literal is plaintext rather than XOR-obfuscated".into());
    }
    if encrypted[..prefix.len()]
        .iter()
        .zip(prefix)
        .any(|(encrypted, plain)| encrypted ^ plain != key)
    {
        return Err("dispatcher literal does not prove one XOR key for the service prefix".into());
    }
    let decoded: Vec<u8> = encrypted[prefix.len()..]
        .iter()
        .map(|byte| byte ^ key)
        .collect();
    let name = String::from_utf8(decoded)?;
    if name.is_empty()
        || !name
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_')
    {
        return Err(format!("decoded dispatcher method name {name:?} is invalid").into());
    }
    Ok((name, key))
}

fn find_dispatcher_rva(dump: &str, service: &str) -> Result<u64, Box<dyn Error>> {
    let class = format!("public class {service}Stub :");
    let mut in_class = false;
    let mut pending_rva = None;
    for line in dump.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with("public class ") {
            in_class = trimmed.starts_with(&class);
            pending_rva = None;
            continue;
        }
        if !in_class {
            continue;
        }
        if let Some(value) = trimmed.strip_prefix("// RVA: 0x") {
            let value = value.split_whitespace().next().ok_or("empty RVA")?;
            pending_rva = Some(u64::from_str_radix(value, 16)?);
        } else if trimmed == "public string GetMethodName(uint methodId) { }" {
            return pending_rva.ok_or_else(|| "GetMethodName has no RVA".into());
        }
    }
    Err(format!("dump has no {service}Stub.GetMethodName(uint)").into())
}

fn parse_literal_map(values: Vec<StringLiteral>) -> Result<BTreeMap<u64, String>, Box<dyn Error>> {
    let mut result = BTreeMap::new();
    for value in values {
        let address = value
            .address
            .strip_prefix("0x")
            .ok_or("string literal address has no 0x prefix")?;
        let address = u64::from_str_radix(address, 16)?;
        if result.insert(address, value.value).is_some() {
            return Err(format!("duplicate string literal address {address:#X}").into());
        }
    }
    Ok(result)
}

fn rebase_literal_map(
    literals: BTreeMap<u64, String>,
    delta: i64,
) -> Result<BTreeMap<u64, String>, Box<dyn Error>> {
    if delta == 0 {
        return Ok(literals);
    }
    let mut rebased = BTreeMap::new();
    for (address, value) in literals {
        let address = i128::from(address)
            .checked_add(i128::from(delta))
            .and_then(|value| u64::try_from(value).ok())
            .ok_or("string-literal RVA rebase overflowed")?;
        if rebased.insert(address, value).is_some() {
            return Err(format!("string-literal RVA rebase collided at 0x{address:X}").into());
        }
    }
    Ok(rebased)
}

impl PeImage {
    fn parse(bytes: Vec<u8>) -> Result<Self, Box<dyn Error>> {
        if bytes.get(0..2) != Some(b"MZ") {
            return Err("GameAssembly is not a PE image".into());
        }
        let pe_offset = read_u32(&bytes, 0x3C)? as usize;
        if bytes.get(pe_offset..pe_offset + 4) != Some(b"PE\0\0") {
            return Err("GameAssembly has no PE signature".into());
        }
        let section_count = read_u16(&bytes, pe_offset + 6)? as usize;
        let optional_size = read_u16(&bytes, pe_offset + 20)? as usize;
        let optional = pe_offset + 24;
        if read_u16(&bytes, optional)? != 0x20B {
            return Err("GameAssembly is not PE32+".into());
        }
        let image_base = read_u64(&bytes, optional + 24)?;
        if image_base == 0 {
            return Err("GameAssembly PE image base is zero".into());
        }
        let section_table = optional + optional_size;
        let mut sections = Vec::with_capacity(section_count);
        for index in 0..section_count {
            let offset = section_table + index * 40;
            sections.push(PeSection {
                virtual_size: read_u32(&bytes, offset + 8)? as u64,
                virtual_address: read_u32(&bytes, offset + 12)? as u64,
                raw_size: read_u32(&bytes, offset + 16)? as u64,
                raw_offset: read_u32(&bytes, offset + 20)? as u64,
            });
        }
        Ok(Self { bytes, sections })
    }

    fn read_rva(&self, rva: u64, length: usize) -> Result<&[u8], Box<dyn Error>> {
        let section = self
            .sections
            .iter()
            .find(|section| {
                let size = section.virtual_size.max(section.raw_size);
                rva >= section.virtual_address && rva < section.virtual_address + size
            })
            .ok_or_else(|| format!("RVA {rva:#X} is outside PE sections"))?;
        let relative = rva - section.virtual_address;
        if relative + length as u64 > section.raw_size {
            return Err(format!("RVA range {rva:#X}+{length:#X} is not file-backed").into());
        }
        let offset = (section.raw_offset + relative) as usize;
        self.bytes
            .get(offset..offset + length)
            .ok_or_else(|| format!("RVA range {rva:#X}+{length:#X} exceeds GameAssembly").into())
    }
}

fn branch_target_rel8(base_rva: u64, offset: usize, bytes: &[u8]) -> Result<u64, Box<dyn Error>> {
    let displacement = *bytes.get(offset + 1).ok_or("truncated rel8 branch")? as i8 as i64;
    (base_rva + offset as u64 + 2)
        .checked_add_signed(displacement)
        .ok_or_else(|| "rel8 branch overflow".into())
}

fn branch_target_rel32(base_rva: u64, offset: usize, bytes: &[u8]) -> Result<u64, Box<dyn Error>> {
    let displacement = read_i32(bytes, offset + 2)? as i64;
    (base_rva + offset as u64 + 6)
        .checked_add_signed(displacement)
        .ok_or_else(|| "rel32 branch overflow".into())
}

fn find_pattern(bytes: &[u8], pattern: &[u8], start: usize) -> Option<usize> {
    bytes
        .get(start..)?
        .windows(pattern.len())
        .position(|window| window == pattern)
        .map(|offset| start + offset)
}

fn read_u16(bytes: &[u8], offset: usize) -> Result<u16, Box<dyn Error>> {
    Ok(u16::from_le_bytes(
        bytes
            .get(offset..offset + 2)
            .ok_or("truncated u16")?
            .try_into()?,
    ))
}

fn read_u32(bytes: &[u8], offset: usize) -> Result<u32, Box<dyn Error>> {
    Ok(u32::from_le_bytes(
        bytes
            .get(offset..offset + 4)
            .ok_or("truncated u32")?
            .try_into()?,
    ))
}

fn read_i32(bytes: &[u8], offset: usize) -> Result<i32, Box<dyn Error>> {
    Ok(i32::from_le_bytes(
        bytes
            .get(offset..offset + 4)
            .ok_or("truncated i32")?
            .try_into()?,
    ))
}

fn read_u64(bytes: &[u8], offset: usize) -> Result<u64, Box<dyn Error>> {
    Ok(u64::from_le_bytes(
        bytes
            .get(offset..offset + 8)
            .ok_or("truncated u64")?
            .try_into()?,
    ))
}

fn validate_identity(
    name: &str,
    bytes: &[u8],
    expected: &ArtifactIdentity,
) -> Result<(), Box<dyn Error>> {
    if bytes.len() as u64 != expected.byte_length {
        return Err(format!(
            "{name} length {} does not match identity {}",
            bytes.len(),
            expected.byte_length
        )
        .into());
    }
    let actual = hex_digest(bytes);
    if actual != expected.sha256 {
        return Err(format!(
            "{name} SHA-256 {actual} does not match identity {}",
            expected.sha256
        )
        .into());
    }
    Ok(())
}

fn artifact_identity(bytes: &[u8], metadata_version: Option<i32>) -> ArtifactIdentity {
    ArtifactIdentity {
        byte_length: bytes.len() as u64,
        sha256: hex_digest(bytes),
        metadata_version,
    }
}

fn hex_digest(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    let mut output = String::with_capacity(digest.len() * 2);
    for byte in digest {
        use std::fmt::Write as _;
        write!(&mut output, "{byte:02x}").expect("writing to a String cannot fail");
    }
    output
}

fn arguments() -> Result<Arguments, Box<dyn Error>> {
    let mut values = BTreeMap::new();
    let mut arguments = env::args().skip(1);
    while let Some(argument) = arguments.next() {
        let key = argument
            .strip_prefix("--")
            .ok_or_else(|| format!("unexpected positional argument {argument}"))?;
        let value = arguments
            .next()
            .ok_or_else(|| format!("missing value for --{key}"))?;
        values.insert(key.to_owned(), value);
    }
    let take_path =
        |values: &mut BTreeMap<String, String>, key: &str| -> Result<PathBuf, Box<dyn Error>> {
            values
                .remove(key)
                .map(PathBuf::from)
                .ok_or_else(|| format!("missing --{key}").into())
        };
    let surface = take_path(&mut values, "surface")?;
    let dump = take_path(&mut values, "dump")?;
    let game_assembly = take_path(&mut values, "game-assembly")?;
    let string_literals = take_path(&mut values, "string-literals")?;
    let string_literal_identity = values.remove("string-literal-identity").map(PathBuf::from);
    let string_literal_rva_delta = values
        .remove("string-literal-rva-delta")
        .map(|value| value.parse::<i64>())
        .transpose()?
        .unwrap_or(0);
    let identity = take_path(&mut values, "identity")?;
    let output = take_path(&mut values, "output")?;
    let service = values.remove("service").ok_or("missing --service")?;
    if !values.is_empty() {
        return Err(format!("unknown arguments: {:?}", values.keys()).into());
    }
    Ok(Arguments {
        surface,
        dump,
        game_assembly,
        string_literals,
        string_literal_identity,
        string_literal_rva_delta,
        identity,
        service,
        output,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decodes_service_prefixed_literal_with_proven_xor_key() {
        let plain = b"WorldNtf::SyncClientUseSkill";
        let encrypted: String = plain.iter().map(|byte| char::from(byte ^ 0x32)).collect();
        let (name, key) = decode_literal("WorldNtf", &encrypted).unwrap();
        assert_eq!(name, "SyncClientUseSkill");
        assert_eq!(key, 0x32);
    }

    #[test]
    fn decodes_byte_valued_non_ascii_literal() {
        let plain = b"WorldNtf::SyncServerSkillStageEnd";
        let encrypted: String = plain.iter().map(|byte| char::from(byte ^ 0xB2)).collect();
        let (name, key) = decode_literal("WorldNtf", &encrypted).unwrap();
        assert_eq!(name, "SyncServerSkillStageEnd");
        assert_eq!(key, 0xB2);
    }

    #[test]
    fn rebases_literal_addresses_without_changing_values() {
        let source = BTreeMap::from([(0x1000, "one".to_owned()), (0x1010, "two".to_owned())]);
        let rebased = rebase_literal_map(source, -0x20).unwrap();
        assert_eq!(rebased.get(&0x0fe0).map(String::as_str), Some("one"));
        assert_eq!(rebased.get(&0x0ff0).map(String::as_str), Some("two"));
    }

    #[test]
    fn rejects_literal_without_one_prefix_key() {
        let error = decode_literal("WorldNtf", "WorldNtf::SyncClientUseSkill")
            .expect_err("plaintext should not satisfy the encrypted prefix policy");
        assert!(error.to_string().contains("plaintext"));
    }

    #[test]
    fn finds_world_dispatcher_rva_only_inside_world_stub() {
        let dump = "public class OtherStub : ZStub\n{\n// RVA: 0x10 Offset: 0x0\npublic string GetMethodName(uint methodId) { }\n}\npublic class WorldNtfStub : ZStub\n{\n// RVA: 0x588E740 Offset: 0x0\npublic string GetMethodName(uint methodId) { }\n}";
        assert_eq!(find_dispatcher_rva(dump, "WorldNtf").unwrap(), 0x588E740);
    }
}
