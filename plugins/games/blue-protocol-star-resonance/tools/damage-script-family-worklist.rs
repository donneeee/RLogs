use std::{
    collections::BTreeMap,
    env,
    fs::File,
    io::{BufReader, BufWriter, Read, Write},
    path::{Path, PathBuf},
};

use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};

const MISSING_SCRIPT: &str = "<missing>";

#[derive(Debug, Deserialize)]
struct CandidateCatalog {
    schema_version: u16,
    game_build: String,
    summary: CandidateSummary,
    coverage_gaps: Vec<CoverageGap>,
}

#[derive(Debug, Deserialize)]
struct CandidateSummary {
    nonstandard_or_missing_script_keys: usize,
}

#[derive(Debug, Deserialize)]
struct CoverageGap {
    #[serde(default)]
    gap_class: Option<String>,
    lookup_key: String,
    ability_id: i64,
    hit_event_id: i32,
    reason: String,
    candidates: Vec<DamageStageCandidate>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct DamageStageCandidate {
    damage_attr_id: i64,
    linked_ability_id: Option<i64>,
    hit_event_suffix_candidate: Option<i64>,
    row_level: Option<i64>,
    name: Option<String>,
    type_enum: Option<i64>,
    damage_type: Option<i64>,
    damage_script: Option<String>,
    coefficient_basis_points_by_stage: Vec<i64>,
    fixed_parameter_by_level: Vec<i64>,
    pve_loop_time: Option<i64>,
    pve_stunned_damage: Vec<i64>,
    pve_extinction_damage: Option<i64>,
    part_damage_radio: Vec<i64>,
    abnormal_damage: Value,
    damage_property: Option<i64>,
    part_damage_type: Option<i64>,
    damage_weight: Value,
    tags: Vec<i64>,
    behit_light_is_open: Option<bool>,
    is_profession: Option<bool>,
}

#[derive(Debug, Deserialize)]
struct RouteProof {
    schema_version: u16,
    game_build: String,
    keys: Vec<RouteKey>,
}

#[derive(Debug, Deserialize)]
struct RouteKey {
    lookup_key: String,
    candidates: Vec<RouteCandidate>,
    resolution_state: String,
}

#[derive(Debug, Deserialize)]
struct RouteCandidate {
    damage_attr_id: i64,
    routes: Vec<StaticRoute>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct StaticRoute {
    damage_source: String,
    damage_source_id: i32,
    construction: String,
    owner_table: String,
    owner_id: i64,
    intermediary_id: Option<i64>,
}

#[derive(Debug, Serialize)]
struct Worklist {
    schema_version: u16,
    game_build: String,
    generated_by: &'static str,
    promotion_state: &'static str,
    catalog_input: InputArtifact,
    route_proof_input: InputArtifact,
    policy: WorklistPolicy,
    summary: WorklistSummary,
    families: Vec<ScriptFamily>,
}

#[derive(Debug, Serialize)]
struct InputArtifact {
    file: String,
    bytes: u64,
    sha256: String,
}

#[derive(Debug, Serialize)]
struct WorklistPolicy {
    runtime_authority: bool,
    candidate_retention: &'static str,
    grouping_semantics: &'static str,
    missing_script_semantics: &'static str,
    packet_requirement: &'static str,
    formula_requirement: &'static str,
    attribution_requirement: &'static str,
}

#[derive(Debug, Serialize)]
struct WorklistSummary {
    script_families: usize,
    formula_signatures: usize,
    lookup_keys: usize,
    candidate_rows: usize,
    candidates_with_static_route: usize,
    candidates_without_static_route: usize,
    missing_script_lookup_keys: usize,
    missing_script_candidate_rows: usize,
}

#[derive(Debug, Serialize)]
struct ScriptFamily {
    damage_script: String,
    proof_state: &'static str,
    proof_requirements: Vec<&'static str>,
    summary: ScriptFamilySummary,
    distributions: FamilyDistributions,
    formula_signatures: Vec<FormulaSignatureGroup>,
}

#[derive(Debug, Serialize)]
struct ScriptFamilySummary {
    lookup_keys: usize,
    candidate_rows: usize,
    formula_signatures: usize,
    candidates_with_static_route: usize,
    candidates_without_static_route: usize,
}

#[derive(Debug, Default, Serialize)]
struct FamilyDistributions {
    damage_type: BTreeMap<String, usize>,
    damage_property: BTreeMap<String, usize>,
    part_damage_type: BTreeMap<String, usize>,
    row_level: BTreeMap<String, usize>,
    route_damage_source: BTreeMap<String, usize>,
}

#[derive(Debug, Serialize)]
struct FormulaSignatureGroup {
    signature_id: String,
    signature: FormulaSignature,
    lookup_keys: usize,
    candidate_rows: usize,
    work_items: Vec<WorkItem>,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize)]
struct FormulaSignature {
    damage_script: String,
    type_enum: Option<i64>,
    damage_type: Option<i64>,
    coefficient_basis_points_by_stage: Vec<i64>,
    fixed_parameter_by_level: Vec<i64>,
    pve_loop_time: Option<i64>,
    pve_stunned_damage: Vec<i64>,
    pve_extinction_damage: Option<i64>,
    part_damage_radio: Vec<i64>,
    abnormal_damage_json: String,
    damage_property: Option<i64>,
    part_damage_type: Option<i64>,
    damage_weight_json: String,
    row_level: Option<i64>,
    tags: Vec<i64>,
    behit_light_is_open: Option<bool>,
    is_profession: Option<bool>,
}

#[derive(Debug, Serialize)]
struct WorkItem {
    lookup_key: String,
    ability_id: i64,
    hit_event_id: i32,
    gap_reason: String,
    damage_attr: DamageStageCandidate,
    static_routes: Vec<StaticRoute>,
    route_resolution_state: String,
}

#[derive(Debug, Default)]
struct FamilyBuilder {
    work_items: BTreeMap<FormulaSignature, Vec<WorkItem>>,
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let arguments = env::args().skip(1).collect::<Vec<_>>();
    if arguments.len() != 8 {
        return Err(usage().into());
    }
    let catalog_path = PathBuf::from(option(&arguments, "--catalog")?);
    let route_path = PathBuf::from(option(&arguments, "--route-proof")?);
    let output_path = PathBuf::from(option(&arguments, "--output")?);
    let game_build = option(&arguments, "--build")?.to_owned();
    if game_build.is_empty() || !game_build.bytes().all(|byte| byte.is_ascii_digit()) {
        return Err("--build requires a numeric client build".into());
    }

    let catalog: CandidateCatalog =
        serde_json::from_reader(BufReader::new(File::open(&catalog_path)?))?;
    let route_proof: RouteProof =
        serde_json::from_reader(BufReader::new(File::open(&route_path)?))?;
    let worklist = build_worklist(
        catalog,
        route_proof,
        game_build,
        input_artifact(&catalog_path)?,
        input_artifact(&route_path)?,
    )?;

    let mut writer = BufWriter::new(File::create(output_path)?);
    serde_json::to_writer(&mut writer, &worklist)?;
    writer.write_all(b"\n")?;
    writer.flush()?;
    eprintln!(
        "wrote {} nonstandard/missing-script candidates across {} keys, {} exact script families, and {} formula signatures; retained {} candidates without a static source route",
        worklist.summary.candidate_rows,
        worklist.summary.lookup_keys,
        worklist.summary.script_families,
        worklist.summary.formula_signatures,
        worklist.summary.candidates_without_static_route,
    );
    Ok(())
}

fn build_worklist(
    catalog: CandidateCatalog,
    route_proof: RouteProof,
    game_build: String,
    catalog_input: InputArtifact,
    route_proof_input: InputArtifact,
) -> Result<Worklist, String> {
    if !(5..=9).contains(&catalog.schema_version) {
        return Err(format!(
            "damage-stage catalog schema {} is unsupported; expected 5 through 9",
            catalog.schema_version
        ));
    }
    if route_proof.schema_version < 5 {
        return Err(format!(
            "route-proof schema {} is unsupported; expected 5 or newer",
            route_proof.schema_version
        ));
    }
    if catalog.game_build != game_build || route_proof.game_build != game_build {
        return Err(format!(
            "input build mismatch: requested {game_build}, catalog {}, route proof {}",
            catalog.game_build, route_proof.game_build
        ));
    }
    let nonstandard_gaps = catalog
        .coverage_gaps
        .into_iter()
        .filter(|gap| {
            gap.gap_class
                .as_deref()
                .unwrap_or("nonstandard-or-missing-script")
                == "nonstandard-or-missing-script"
        })
        .collect::<Vec<_>>();
    if nonstandard_gaps.len() != catalog.summary.nonstandard_or_missing_script_keys {
        return Err(format!(
            "catalog retained {} gaps but summary declares {}",
            nonstandard_gaps.len(),
            catalog.summary.nonstandard_or_missing_script_keys
        ));
    }

    let route_keys = route_proof
        .keys
        .into_iter()
        .map(|key| (key.lookup_key.clone(), key))
        .collect::<BTreeMap<_, _>>();
    let mut builders = BTreeMap::<String, FamilyBuilder>::new();
    let mut lookup_keys = 0_usize;
    let mut candidate_rows = 0_usize;
    let mut candidates_with_static_route = 0_usize;
    let mut missing_script_lookup_keys = 0_usize;
    let mut missing_script_candidate_rows = 0_usize;

    for gap in nonstandard_gaps {
        let route_key = route_keys
            .get(&gap.lookup_key)
            .ok_or_else(|| format!("route proof is missing lookup {}", gap.lookup_key))?;
        lookup_keys += 1;
        let key_has_missing_script = gap
            .candidates
            .iter()
            .any(|candidate| candidate.damage_script.is_none());
        missing_script_lookup_keys += usize::from(key_has_missing_script);
        for candidate in gap.candidates {
            candidate_rows += 1;
            let script = candidate
                .damage_script
                .clone()
                .unwrap_or_else(|| MISSING_SCRIPT.to_owned());
            missing_script_candidate_rows += usize::from(script == MISSING_SCRIPT);
            let route_candidate = route_key
                .candidates
                .iter()
                .find(|routed| routed.damage_attr_id == candidate.damage_attr_id)
                .ok_or_else(|| {
                    format!(
                        "route proof lookup {} is missing damage row {}",
                        gap.lookup_key, candidate.damage_attr_id
                    )
                })?;
            candidates_with_static_route += usize::from(!route_candidate.routes.is_empty());
            let signature = FormulaSignature {
                damage_script: script.clone(),
                type_enum: candidate.type_enum,
                damage_type: candidate.damage_type,
                coefficient_basis_points_by_stage: candidate
                    .coefficient_basis_points_by_stage
                    .clone(),
                fixed_parameter_by_level: candidate.fixed_parameter_by_level.clone(),
                pve_loop_time: candidate.pve_loop_time,
                pve_stunned_damage: candidate.pve_stunned_damage.clone(),
                pve_extinction_damage: candidate.pve_extinction_damage,
                part_damage_radio: candidate.part_damage_radio.clone(),
                abnormal_damage_json: serde_json::to_string(&candidate.abnormal_damage)
                    .map_err(|error| format!("cannot encode AbnormalDamage: {error}"))?,
                damage_property: candidate.damage_property,
                part_damage_type: candidate.part_damage_type,
                damage_weight_json: serde_json::to_string(&candidate.damage_weight)
                    .map_err(|error| format!("cannot encode DamageWeight: {error}"))?,
                row_level: candidate.row_level,
                tags: candidate.tags.clone(),
                behit_light_is_open: candidate.behit_light_is_open,
                is_profession: candidate.is_profession,
            };
            builders
                .entry(script)
                .or_default()
                .work_items
                .entry(signature)
                .or_default()
                .push(WorkItem {
                    lookup_key: gap.lookup_key.clone(),
                    ability_id: gap.ability_id,
                    hit_event_id: gap.hit_event_id,
                    gap_reason: gap.reason.clone(),
                    damage_attr: candidate,
                    static_routes: route_candidate.routes.clone(),
                    route_resolution_state: route_key.resolution_state.clone(),
                });
        }
    }

    let mut formula_signature_count = 0_usize;
    let families = builders
        .into_iter()
        .map(|(script, builder)| {
            let mut distributions = FamilyDistributions::default();
            let mut family_keys = BTreeMap::<String, ()>::new();
            let mut family_candidate_rows = 0_usize;
            let mut family_with_route = 0_usize;
            let formula_signatures = builder
                .work_items
                .into_iter()
                .map(|(signature, mut work_items)| {
                    work_items.sort_by(|left, right| {
                        (
                            left.ability_id,
                            left.hit_event_id,
                            left.damage_attr.damage_attr_id,
                        )
                            .cmp(&(
                                right.ability_id,
                                right.hit_event_id,
                                right.damage_attr.damage_attr_id,
                            ))
                    });
                    for item in &work_items {
                        family_keys.insert(item.lookup_key.clone(), ());
                        family_candidate_rows += 1;
                        family_with_route += usize::from(!item.static_routes.is_empty());
                        increment(&mut distributions.damage_type, item.damage_attr.damage_type);
                        increment(
                            &mut distributions.damage_property,
                            item.damage_attr.damage_property,
                        );
                        increment(
                            &mut distributions.part_damage_type,
                            item.damage_attr.part_damage_type,
                        );
                        increment(&mut distributions.row_level, item.damage_attr.row_level);
                        if item.static_routes.is_empty() {
                            *distributions
                                .route_damage_source
                                .entry("<unresolved>".to_owned())
                                .or_default() += 1;
                        } else {
                            for route in &item.static_routes {
                                *distributions
                                    .route_damage_source
                                    .entry(format!(
                                        "{}:{}",
                                        route.damage_source_id, route.damage_source
                                    ))
                                    .or_default() += 1;
                            }
                        }
                    }
                    let lookup_key_count = work_items
                        .iter()
                        .map(|item| item.lookup_key.as_str())
                        .collect::<std::collections::BTreeSet<_>>()
                        .len();
                    FormulaSignatureGroup {
                        signature_id: signature_id(&signature),
                        signature,
                        lookup_keys: lookup_key_count,
                        candidate_rows: work_items.len(),
                        work_items,
                    }
                })
                .collect::<Vec<_>>();
            formula_signature_count += formula_signatures.len();
            ScriptFamily {
                damage_script: script.clone(),
                proof_state: "research-only-not-runtime-authority",
                proof_requirements: proof_requirements(&script),
                summary: ScriptFamilySummary {
                    lookup_keys: family_keys.len(),
                    candidate_rows: family_candidate_rows,
                    formula_signatures: formula_signatures.len(),
                    candidates_with_static_route: family_with_route,
                    candidates_without_static_route: family_candidate_rows - family_with_route,
                },
                distributions,
                formula_signatures,
            }
        })
        .collect::<Vec<_>>();

    let candidates_without_static_route = candidate_rows - candidates_with_static_route;
    Ok(Worklist {
        schema_version: 2,
        game_build,
        generated_by: "rlogs-bpsr-damage-script-family-worklist",
        promotion_state: "research-only-every-family-requires-same-build-proof",
        catalog_input,
        route_proof_input,
        policy: WorklistPolicy {
            runtime_authority: false,
            candidate_retention: "every catalog coverage-gap key and candidate row is retained; no unresolved event is hidden or discarded",
            grouping_semantics: "families group exact DamageScript identity and exact formula-relevant table fields only; grouping does not assert formula equivalence",
            missing_script_semantics: "an absent DamageScript remains its own explicit family and is never interpreted as Attack or MAttack",
            packet_requirement: "same-build packet ability, semantic hit, damage source, damage type, source entity, target entity, owner stage, and owner level must select the observed candidate",
            formula_requirement: "each script family requires isolated packet-state replay proving inputs, units, stage and level selection, rounding, mitigation ordering, and output conservation",
            attribution_requirement: "a formula may become rDPS authority only after provider, recipient, self-only scope, lifecycle, stacking, and marginal-damage behavior are packet proven",
        },
        summary: WorklistSummary {
            script_families: families.len(),
            formula_signatures: formula_signature_count,
            lookup_keys,
            candidate_rows,
            candidates_with_static_route,
            candidates_without_static_route,
            missing_script_lookup_keys,
            missing_script_candidate_rows,
        },
        families,
    })
}

fn proof_requirements(script: &str) -> Vec<&'static str> {
    let mut requirements = vec![
        "same-build-packet-occurrence",
        "exact-damage-source-route",
        "formula-input-and-fixed-point-unit-isolation",
        "stage-level-and-rounding-isolation",
        "state-and-target-dependency-ledger",
        "canonical-replay-conservation",
        "provider-recipient-and-self-only-scope-proof-before-rdps",
    ];
    if script == MISSING_SCRIPT {
        requirements.insert(
            0,
            "current-build-table-relationship-or-server-route-recovery",
        );
    }
    requirements
}

fn increment(distribution: &mut BTreeMap<String, usize>, value: Option<i64>) {
    *distribution
        .entry(value.map_or_else(|| "<missing>".to_owned(), |value| value.to_string()))
        .or_default() += 1;
}

fn signature_id(signature: &FormulaSignature) -> String {
    let encoded = serde_json::to_vec(signature).expect("formula signatures are serializable");
    let digest = format!("{:x}", Sha256::digest(encoded));
    format!("formula-{}", &digest[..16])
}

fn option<'a>(arguments: &'a [String], name: &str) -> Result<&'a str, String> {
    arguments
        .iter()
        .position(|argument| argument == name)
        .and_then(|index| arguments.get(index + 1))
        .map(String::as_str)
        .ok_or_else(usage)
}

fn usage() -> String {
    "usage: rlogs-bpsr-damage-script-family-worklist --catalog <damage-stage-candidate.json> --route-proof <damage-source-route-proof.json> --build <numeric-client-build> --output <worklist.json>".to_owned()
}

fn input_artifact(path: &Path) -> Result<InputArtifact, Box<dyn std::error::Error>> {
    let mut reader = BufReader::new(File::open(path)?);
    let mut hasher = Sha256::new();
    let mut bytes = 0_u64;
    let mut buffer = [0_u8; 64 * 1024];
    loop {
        let count = reader.read(&mut buffer)?;
        if count == 0 {
            break;
        }
        hasher.update(&buffer[..count]);
        bytes = bytes.saturating_add(count as u64);
    }
    Ok(InputArtifact {
        file: path
            .file_name()
            .and_then(|value| value.to_str())
            .unwrap_or("external-artifact")
            .to_owned(),
        bytes,
        sha256: format!("{:x}", hasher.finalize()),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn artifact(file: &str) -> InputArtifact {
        InputArtifact {
            file: file.to_owned(),
            bytes: 1,
            sha256: "00".repeat(32),
        }
    }

    fn candidate(script: Option<&str>, damage_attr_id: i64) -> DamageStageCandidate {
        DamageStageCandidate {
            damage_attr_id,
            linked_ability_id: Some(10),
            hit_event_suffix_candidate: Some(1),
            row_level: Some(0),
            name: None,
            type_enum: Some(10),
            damage_type: Some(3),
            damage_script: script.map(str::to_owned),
            coefficient_basis_points_by_stage: vec![10_000],
            fixed_parameter_by_level: vec![0],
            pve_loop_time: Some(0),
            pve_stunned_damage: vec![],
            pve_extinction_damage: Some(0),
            part_damage_radio: vec![],
            abnormal_damage: Value::Array(vec![]),
            damage_property: Some(0),
            part_damage_type: Some(0),
            damage_weight: Value::Array(vec![]),
            tags: vec![8],
            behit_light_is_open: Some(false),
            is_profession: Some(false),
        }
    }

    #[test]
    fn retains_every_gap_candidate_and_missing_script_family() {
        let catalog = CandidateCatalog {
            schema_version: 5,
            game_build: "24568685".to_owned(),
            summary: CandidateSummary {
                nonstandard_or_missing_script_keys: 1,
            },
            coverage_gaps: vec![CoverageGap {
                gap_class: None,
                lookup_key: "10:1".to_owned(),
                ability_id: 10,
                hit_event_id: 1,
                reason: "gap".to_owned(),
                candidates: vec![candidate(Some("HealByHp"), 2101), candidate(None, 2102)],
            }],
        };
        let route_proof = RouteProof {
            schema_version: 5,
            game_build: "24568685".to_owned(),
            keys: vec![RouteKey {
                lookup_key: "10:1".to_owned(),
                candidates: vec![
                    RouteCandidate {
                        damage_attr_id: 2101,
                        routes: vec![],
                    },
                    RouteCandidate {
                        damage_attr_id: 2102,
                        routes: vec![],
                    },
                ],
                resolution_state: "unresolved".to_owned(),
            }],
        };
        let worklist = build_worklist(
            catalog,
            route_proof,
            "24568685".to_owned(),
            artifact("catalog.json"),
            artifact("routes.json"),
        )
        .unwrap();
        assert_eq!(worklist.summary.lookup_keys, 1);
        assert_eq!(worklist.summary.candidate_rows, 2);
        assert_eq!(worklist.summary.script_families, 2);
        assert_eq!(worklist.summary.missing_script_candidate_rows, 1);
    }

    #[test]
    fn rejects_wrong_build_before_generating_evidence() {
        let catalog = CandidateCatalog {
            schema_version: 5,
            game_build: "old".to_owned(),
            summary: CandidateSummary {
                nonstandard_or_missing_script_keys: 0,
            },
            coverage_gaps: vec![],
        };
        let route_proof = RouteProof {
            schema_version: 5,
            game_build: "24568685".to_owned(),
            keys: vec![],
        };
        assert!(
            build_worklist(
                catalog,
                route_proof,
                "24568685".to_owned(),
                artifact("catalog.json"),
                artifact("routes.json"),
            )
            .is_err()
        );
    }
}
