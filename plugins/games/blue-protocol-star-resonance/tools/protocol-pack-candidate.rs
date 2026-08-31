use std::{
    collections::{BTreeMap, BTreeSet},
    env,
    error::Error,
    fs::{self, File},
    io::{BufReader, BufWriter, Write},
    path::{Path, PathBuf},
};

use rlogs_game_bpsr::{
    DecoderKind, FragmentKind, MappingConfidence, MappingProvenance, PacketDirection,
    ProtocolFeature, ProtocolPack, ProtocolPackDefinition, ProtocolPackRoute,
    ProtocolPackRouteDisposition, RouteKey,
};
use serde::Deserialize;

#[derive(Debug)]
struct Arguments {
    baseline_pack: PathBuf,
    rpc_surface: PathBuf,
    route_proof: PathBuf,
    use_slot_proof: PathBuf,
    identity: PathBuf,
    output: PathBuf,
}

#[derive(Debug, Deserialize)]
struct BuildIdentity {
    game_build: String,
    deployment: String,
    channel: String,
}

#[derive(Debug, Deserialize)]
struct RpcSurface {
    build_id: String,
    services: Vec<RpcService>,
}

#[derive(Debug, Deserialize)]
struct RpcService {
    name: String,
    service_id: u64,
    id_state: String,
    methods: Vec<RpcMethod>,
}

#[derive(Debug, Deserialize)]
struct RpcMethod {
    name: String,
    wire_method_id: Option<u32>,
    wire_method_id_state: String,
}

#[derive(Debug, Deserialize)]
struct RouteProof {
    game_build: String,
    server_dispatcher: ServerDispatcher,
}

#[derive(Debug, Deserialize)]
struct ServerDispatcher {
    routes: Vec<ProvenRoute>,
}

#[derive(Debug, Deserialize)]
struct ProvenRoute {
    name: String,
    method_id_decimal: u32,
    proof_state: String,
}

#[derive(Debug, Deserialize)]
struct UseSlotProof {
    game_build: String,
    route: UseSlotRoute,
    promotion_state: UseSlotPromotionState,
}

#[derive(Debug, Deserialize)]
struct UseSlotRoute {
    service: String,
    method: String,
    service_id_decimal: u64,
    method_id_decimal: u32,
    proof_state: String,
}

#[derive(Debug, Deserialize)]
struct UseSlotPromotionState {
    complete_static_route_exact: bool,
    matching_build_packet_replay_exact: bool,
    runtime_route_enabled: bool,
}

fn main() {
    if let Err(error) = run() {
        eprintln!("protocol-pack candidate generation failed: {error}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), Box<dyn Error>> {
    let args = arguments()?;
    if args.output.exists() {
        return Err(format!("refusing to overwrite {}", args.output.display()).into());
    }
    let baseline: ProtocolPackDefinition = read_json(&args.baseline_pack)?;
    let surface: RpcSurface = read_json(&args.rpc_surface)?;
    let proof: RouteProof = read_json(&args.route_proof)?;
    let use_slot_proof: UseSlotProof = read_json(&args.use_slot_proof)?;
    let identity: BuildIdentity = read_json(&args.identity)?;
    if surface.build_id != identity.game_build
        || proof.game_build != identity.game_build
        || use_slot_proof.game_build != identity.game_build
    {
        return Err("candidate input build identities disagree".into());
    }

    let world = surface
        .services
        .iter()
        .find(|service| service.name == "WorldNtf")
        .ok_or("current RPC surface has no WorldNtf service")?;
    if world.id_state != "exact_native_factory_return" {
        return Err("WorldNtf service ID is not exact native evidence".into());
    }
    let surface_routes = exact_surface_routes(world)?;
    let proof_routes = exact_proof_routes(&proof)?;
    if surface_routes != proof_routes {
        return Err("RPC surface and native WorldNtf route proof disagree".into());
    }

    let (mut routes, carried_decoders) = carry_forward_world_routes(
        &baseline,
        &surface_routes,
        world.service_id,
        current_provenance(&args),
        args.baseline_pack.to_string_lossy().into_owned(),
    );
    let use_slot = exact_use_slot_candidate(&use_slot_proof, &args)?;
    let use_slot_runtime_enabled = matches!(
        use_slot.disposition,
        ProtocolPackRouteDisposition::Allowed {
            decoder: DecoderKind::WorldUseSlotV1,
            ..
        }
    );
    routes.push(use_slot);
    routes.sort_by_key(|route| route.route);

    let definition = ProtocolPackDefinition {
        schema_version: baseline.schema_version,
        pack_id: format!("global-steam-{}-static-candidate-v3", identity.game_build),
        target: rlogs_game_bpsr::ProtocolPackTarget {
            deployment_id: identity.deployment,
            region_id: None,
            channel: identity.channel,
            build_id: identity.game_build,
            executable_version: None,
        },
        acquisition: Default::default(),
        provenance: current_provenance(&args),
        routes,
    };
    ProtocolPack::build(definition.clone())?;
    if let Some(parent) = args.output.parent() {
        fs::create_dir_all(parent)?;
    }
    let mut output = BufWriter::new(File::create(&args.output)?);
    serde_json::to_writer_pretty(&mut output, &definition)?;
    output.write_all(b"\n")?;
    output.flush()?;
    println!(
        "wrote {} conservative WorldNtf routes plus {} World.UseSlot with {} packet-proven decoders carried forward to {}",
        definition.routes.len() - 1,
        if use_slot_runtime_enabled {
            "packet-proven"
        } else {
            "opaque static"
        },
        carried_decoders,
        args.output.display()
    );
    Ok(())
}

/// Static native dispatch ordering is not the packet wire method identity.
///
/// Keep packet-observed baseline route keys and decoder contracts intact, then
/// add only non-conflicting current-build static routes as opaque evidence. A
/// new static name at an occupied wire key must not displace a decoder that was
/// proven from packets; an actual wire move is promoted only after matching-
/// build packet replay establishes it.
fn carry_forward_world_routes(
    baseline: &ProtocolPackDefinition,
    surface_routes: &BTreeMap<String, u32>,
    service_id: u64,
    current_provenance: Vec<MappingProvenance>,
    baseline_reference: String,
) -> (Vec<ProtocolPackRoute>, usize) {
    let carry_provenance = || {
        let mut provenance = current_provenance.clone();
        provenance.push(MappingProvenance {
            source: "wire-compatible-baseline-route-carry-forward".to_owned(),
            reference: baseline_reference.clone(),
        });
        provenance
    };
    let mut routes = BTreeMap::<RouteKey, ProtocolPackRoute>::new();
    let mut carried_decoders = 0usize;

    for baseline_route in baseline
        .routes
        .iter()
        .filter(|route| route.service_name == "WorldNtf")
    {
        let key = RouteKey::new(
            PacketDirection::ServerToClient,
            FragmentKind::Notify,
            service_id,
            baseline_route.route.method_id,
        );
        let mut route = baseline_route.clone();
        route.route = key;
        route.confidence = MappingConfidence::Candidate;
        route.provenance = carry_provenance();
        if matches!(
            route.disposition,
            ProtocolPackRouteDisposition::Allowed { .. }
        ) {
            carried_decoders += 1;
        }
        routes.insert(key, route);
    }

    for (name, method_id) in surface_routes {
        let key = RouteKey::new(
            PacketDirection::ServerToClient,
            FragmentKind::Notify,
            service_id,
            *method_id,
        );
        routes.entry(key).or_insert_with(|| ProtocolPackRoute {
            route: key,
            service_name: "WorldNtf".to_owned(),
            method_name: name.clone(),
            message_name: None,
            confidence: MappingConfidence::Candidate,
            provenance: current_provenance.clone(),
            features: Vec::new(),
            disposition: ProtocolPackRouteDisposition::Opaque,
        });
    }

    (routes.into_values().collect(), carried_decoders)
}

fn exact_use_slot_candidate(
    proof: &UseSlotProof,
    args: &Arguments,
) -> Result<ProtocolPackRoute, Box<dyn Error>> {
    if proof.route.service != "World" || proof.route.method != "UseSlot" {
        return Err("skill-action proof is not for World.UseSlot".into());
    }
    if proof.route.service_id_decimal == 0 || proof.route.method_id_decimal == 0 {
        return Err("World.UseSlot static route contains a zero identifier".into());
    }
    if proof.route.proof_state != "exact_current_build_native_validated_service_name_hash"
        && proof.route.proof_state != "exact_native_proxy_uuid_constant_return_leaf"
    {
        return Err("World.UseSlot service ID is not exact current-build evidence".into());
    }
    if !proof.promotion_state.complete_static_route_exact {
        return Err("World.UseSlot static route is incomplete".into());
    }
    if proof.promotion_state.runtime_route_enabled
        && !proof.promotion_state.matching_build_packet_replay_exact
    {
        return Err("World.UseSlot runtime authorization requires matching-build replay".into());
    }

    let decoder = DecoderKind::WorldUseSlotV1;
    let disposition = if proof.promotion_state.matching_build_packet_replay_exact
        && proof.promotion_state.runtime_route_enabled
    {
        ProtocolPackRouteDisposition::Allowed {
            domain: decoder.domain(),
            decoder,
        }
    } else {
        // Static route identity is useful evidence, but it is not permission to
        // run a copied decoder. Preserve the exact numeric route and payload as
        // opaque until matching-build replay and explicit runtime authorization
        // are both present in the proof input.
        ProtocolPackRouteDisposition::Opaque
    };
    Ok(ProtocolPackRoute {
        route: RouteKey::new(
            PacketDirection::ClientToServer,
            FragmentKind::Call,
            proof.route.service_id_decimal,
            proof.route.method_id_decimal,
        ),
        service_name: proof.route.service.clone(),
        method_name: proof.route.method.clone(),
        message_name: Some("Zproto.World.Types.UseSlot".to_owned()),
        confidence: MappingConfidence::Candidate,
        provenance: vec![MappingProvenance {
            source: "exact-current-build-use-slot-static-proof".to_owned(),
            reference: args.use_slot_proof.to_string_lossy().into_owned(),
        }],
        features: vec![ProtocolFeature::Skill],
        disposition,
    })
}

fn exact_surface_routes(service: &RpcService) -> Result<BTreeMap<String, u32>, Box<dyn Error>> {
    let mut routes = BTreeMap::new();
    for method in &service.methods {
        if method.wire_method_id_state != "exact_native_build_bound_route_proof" {
            continue;
        }
        let method_id = method
            .wire_method_id
            .ok_or("exact RPC surface route has no method ID")?;
        if routes.insert(method.name.clone(), method_id).is_some() {
            return Err(format!("duplicate current RPC method {}", method.name).into());
        }
    }
    Ok(routes)
}

fn exact_proof_routes(proof: &RouteProof) -> Result<BTreeMap<String, u32>, Box<dyn Error>> {
    let mut routes = BTreeMap::new();
    let mut ids = BTreeSet::new();
    for route in &proof.server_dispatcher.routes {
        if route.proof_state != "exact_native_dispatch_and_literal_plaintext" {
            return Err(format!("route {} is not exact native evidence", route.name).into());
        }
        if !ids.insert(route.method_id_decimal) {
            return Err(format!(
                "duplicate current WorldNtf method ID {}",
                route.method_id_decimal
            )
            .into());
        }
        if routes
            .insert(route.name.clone(), route.method_id_decimal)
            .is_some()
        {
            return Err(format!("duplicate current WorldNtf method {}", route.name).into());
        }
    }
    Ok(routes)
}

fn current_provenance(args: &Arguments) -> Vec<MappingProvenance> {
    vec![
        MappingProvenance {
            source: "exact-current-build-rpc-surface".to_owned(),
            reference: args.rpc_surface.to_string_lossy().into_owned(),
        },
        MappingProvenance {
            source: "exact-current-build-native-dispatch-proof".to_owned(),
            reference: args.route_proof.to_string_lossy().into_owned(),
        },
        MappingProvenance {
            source: "decoder-contract-migration-candidate".to_owned(),
            reference: args.baseline_pack.to_string_lossy().into_owned(),
        },
    ]
}

fn read_json<T: for<'de> Deserialize<'de>>(path: &Path) -> Result<T, Box<dyn Error>> {
    Ok(serde_json::from_reader(BufReader::new(File::open(path)?))?)
}

fn arguments() -> Result<Arguments, Box<dyn Error>> {
    let mut values = BTreeMap::new();
    let mut args = env::args().skip(1);
    while let Some(flag) = args.next() {
        let value = args
            .next()
            .ok_or_else(|| format!("missing value for {flag}"))?;
        values.insert(flag, PathBuf::from(value));
    }
    let required = |flag: &str| -> Result<PathBuf, Box<dyn Error>> {
        values
            .get(flag)
            .cloned()
            .ok_or_else(|| format!("missing {flag}").into())
    };
    Ok(Arguments {
        baseline_pack: required("--baseline-pack")?,
        rpc_surface: required("--rpc-surface")?,
        route_proof: required("--route-proof")?,
        use_slot_proof: required("--use-slot-proof")?,
        identity: required("--identity")?,
        output: required("--output")?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn use_slot_proof(replay: bool, runtime: bool) -> UseSlotProof {
        UseSlotProof {
            game_build: "1".to_owned(),
            route: UseSlotRoute {
                service: "World".to_owned(),
                method: "UseSlot".to_owned(),
                service_id_decimal: 103_198_054,
                method_id_decimal: 0x3D002,
                proof_state: "exact_current_build_native_validated_service_name_hash".to_owned(),
            },
            promotion_state: UseSlotPromotionState {
                complete_static_route_exact: true,
                matching_build_packet_replay_exact: replay,
                runtime_route_enabled: runtime,
            },
        }
    }

    fn candidate_arguments() -> Arguments {
        Arguments {
            baseline_pack: "baseline.json".into(),
            rpc_surface: "surface.json".into(),
            route_proof: "routes.json".into(),
            use_slot_proof: "use-slot.json".into(),
            identity: "identity.json".into(),
            output: "candidate.json".into(),
        }
    }

    #[test]
    fn proof_routes_reject_duplicate_method_ids() {
        let proof = RouteProof {
            game_build: "1".to_owned(),
            server_dispatcher: ServerDispatcher {
                routes: vec![
                    ProvenRoute {
                        name: "A".to_owned(),
                        method_id_decimal: 7,
                        proof_state: "exact_native_dispatch_and_literal_plaintext".to_owned(),
                    },
                    ProvenRoute {
                        name: "B".to_owned(),
                        method_id_decimal: 7,
                        proof_state: "exact_native_dispatch_and_literal_plaintext".to_owned(),
                    },
                ],
            },
        };
        assert!(exact_proof_routes(&proof).is_err());
    }

    #[test]
    fn static_dispatch_ids_cannot_displace_packet_proven_wire_routes() {
        let baseline: ProtocolPackDefinition = serde_json::from_str(
            r#"{
                "schema_version": 1,
                "pack_id": "baseline",
                "target": {"deployment_id":"global","region_id":null,"channel":"steam","build_id":"1","executable_version":null},
                "provenance": [],
                "routes": [{
                    "route":{"direction":"server_to_client","fragment":{"kind":"notify"},"service_id":1664308034,"method_id":45},
                    "service_name":"WorldNtf",
                    "method_name":"SyncNearDeltaInfo",
                    "message_name":"SyncNearDeltaInfo",
                    "confidence":"verified",
                    "provenance":[],
                    "features":["damage"],
                    "disposition":"allowed",
                    "domain":"combat",
                    "decoder":"sync_near_delta_v1"
                }]
            }"#,
        )
        .unwrap();
        let surface = BTreeMap::from([
            ("SyncNearDeltaInfo".to_owned(), 12),
            ("SyncNearEntities".to_owned(), 45),
        ]);

        let (routes, carried) = carry_forward_world_routes(
            &baseline,
            &surface,
            1664308034,
            Vec::new(),
            "baseline".into(),
        );

        assert_eq!(carried, 1);
        let wire_45 = routes
            .iter()
            .find(|route| route.route.method_id == 45)
            .unwrap();
        assert_eq!(wire_45.method_name, "SyncNearDeltaInfo");
        assert!(matches!(
            wire_45.disposition,
            ProtocolPackRouteDisposition::Allowed {
                decoder: DecoderKind::SyncNearDeltaV1,
                ..
            }
        ));
        let static_12 = routes
            .iter()
            .find(|route| route.route.method_id == 12)
            .unwrap();
        assert_eq!(static_12.method_name, "SyncNearDeltaInfo");
        assert_eq!(static_12.disposition, ProtocolPackRouteDisposition::Opaque);
    }

    #[test]
    fn static_use_slot_identity_is_opaque_without_matching_build_replay() {
        let route = exact_use_slot_candidate(&use_slot_proof(false, false), &candidate_arguments())
            .unwrap();
        assert_eq!(route.route.service_id, 103_198_054);
        assert_eq!(route.route.method_id, 0x3D002);
        assert_eq!(route.disposition, ProtocolPackRouteDisposition::Opaque);
    }

    #[test]
    fn use_slot_runtime_authority_requires_matching_build_replay() {
        let error = exact_use_slot_candidate(&use_slot_proof(false, true), &candidate_arguments())
            .unwrap_err();
        assert!(
            error
                .to_string()
                .contains("runtime authorization requires matching-build replay")
        );
    }

    #[test]
    fn replay_and_runtime_authority_enable_the_exact_use_slot_decoder() {
        let route =
            exact_use_slot_candidate(&use_slot_proof(true, true), &candidate_arguments()).unwrap();
        assert!(matches!(
            route.disposition,
            ProtocolPackRouteDisposition::Allowed {
                decoder: DecoderKind::WorldUseSlotV1,
                ..
            }
        ));
    }
}
