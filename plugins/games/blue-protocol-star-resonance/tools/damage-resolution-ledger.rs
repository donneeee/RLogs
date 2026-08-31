use std::{
    collections::BTreeMap,
    env,
    fs::{self, File},
    io::{BufReader, BufWriter, Read, Write},
    path::{Path, PathBuf},
};

use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};

const SCHEMA_VERSION: u16 = 2;

#[derive(Debug, Deserialize)]
struct RouteProof {
    schema_version: u16,
    game_build: String,
    keys: Vec<RouteKey>,
}

#[derive(Debug, Deserialize)]
struct RouteKey {
    lookup_key: String,
    ability_id: i64,
    hit_event_id: i32,
    candidates: Vec<RouteCandidate>,
    resolution_state: String,
}

#[derive(Debug, Deserialize)]
struct RouteCandidate {
    damage_attr_id: i64,
    routes: Vec<StaticRoute>,
    recount_owners: Vec<RecountOwner>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
struct StaticRoute {
    damage_source: String,
    damage_source_id: i32,
    construction: String,
    owner_table: String,
    owner_id: i64,
    intermediary_id: Option<i64>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
struct RecountOwner {
    recount_id: i64,
    recount_name: String,
}

#[derive(Debug, Deserialize)]
struct StageCatalog {
    schema_version: u16,
    game_build: String,
    promotion_state: String,
    rules: Vec<StageRule>,
    coverage_gaps: Vec<StageGap>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
struct StageRule {
    ability_id: i64,
    hit_event_id: i32,
    damage_source: Option<i32>,
    damage_attr_id: i64,
    equivalent_damage_attr_ids: Vec<i64>,
    excluded_nonstandard_damage_attr_ids: Vec<i64>,
    type_enum: Option<i64>,
    damage_type: Option<i64>,
    damage_script: String,
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
struct StageGap {
    gap_class: String,
    lookup_key: String,
    reason: String,
    candidates: Vec<StageCandidate>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
struct StageCandidate {
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

type GapFormulaIndex = BTreeMap<(String, i64), (String, StageCandidate)>;

#[derive(Debug, Deserialize)]
struct FamilyWorklist {
    schema_version: u16,
    game_build: String,
    families: Vec<ScriptFamily>,
}

#[derive(Debug, Deserialize)]
struct ScriptFamily {
    damage_script: String,
    formula_signatures: Vec<FormulaSignature>,
}

#[derive(Debug, Deserialize)]
struct FormulaSignature {
    signature_id: String,
    work_items: Vec<FamilyWorkItem>,
}

#[derive(Debug, Deserialize)]
struct FamilyWorkItem {
    lookup_key: String,
    damage_attr: FamilyDamageAttr,
}

#[derive(Debug, Deserialize)]
struct FamilyDamageAttr {
    damage_attr_id: i64,
}

#[derive(Debug, Deserialize)]
struct ReferenceScan {
    schema_version: u16,
    build_id: String,
    targets: Vec<ReferenceTarget>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
struct ReferenceTarget {
    value: i64,
    roles: Vec<String>,
    lookup_keys: Vec<String>,
    reference_count: usize,
    referenced_by_tables: Vec<String>,
}

#[derive(Debug, Serialize)]
struct ResolutionLedger {
    schema_version: u16,
    game_build: String,
    generated_by: &'static str,
    promotion_state: &'static str,
    inputs: Vec<InputArtifact>,
    policy: LedgerPolicy,
    summary: LedgerSummary,
    entries: Vec<LedgerEntry>,
}

#[derive(Debug, Serialize)]
struct InputArtifact {
    role: &'static str,
    file: String,
    bytes: u64,
    sha256: String,
}

#[derive(Debug, Serialize)]
struct LedgerPolicy {
    packet_source_and_recount_parent_are_separate: bool,
    recount_parent_is_formula_authority: bool,
    decoded_scalar_reference_is_formula_authority: bool,
    unresolved_evidence_hidden: bool,
    static_formula_is_runtime_authority: bool,
    runtime_promotion_rule: &'static str,
}

#[derive(Debug, Default, Serialize)]
struct LedgerSummary {
    lookup_keys: usize,
    candidate_rows: usize,
    candidates_with_static_source_route: usize,
    candidates_without_static_source_route: usize,
    candidates_with_named_recount_parent: usize,
    candidates_with_other_recount_parent_only: usize,
    candidates_without_recount_parent: usize,
    standard_static_formula_candidates: usize,
    nonstandard_or_missing_formula_candidates: usize,
    runtime_replay_ready_candidates: usize,
    candidates_blocked_on_source_only: usize,
    candidates_blocked_on_formula_only: usize,
    candidates_blocked_on_source_and_formula: usize,
    candidates_with_reference_leads: usize,
}

#[derive(Debug, Serialize)]
struct LedgerEntry {
    lookup_key: String,
    ability_id: i64,
    hit_event_id: i32,
    damage_attr_id: i64,
    source: SourceEvidence,
    recount: RecountEvidence,
    formula: FormulaEvidence,
    decoded_reference_leads: Vec<ReferenceTarget>,
    readiness: &'static str,
}

#[derive(Debug, Serialize)]
struct SourceEvidence {
    state: &'static str,
    route_key_resolution_state: String,
    routes: Vec<StaticRoute>,
}

#[derive(Debug, Serialize)]
struct RecountEvidence {
    state: &'static str,
    owners: Vec<RecountOwner>,
}

#[derive(Debug, Serialize)]
#[serde(tag = "state", rename_all = "kebab-case")]
enum FormulaEvidence {
    StandardStaticCandidate {
        promotion_state: String,
        rule: StageRule,
    },
    NonstandardOrMissing {
        gap_reason: String,
        family: Option<String>,
        formula_signature_id: Option<String>,
        candidate: StageCandidate,
    },
}

#[derive(Debug, Clone)]
struct FamilyIdentity {
    family: String,
    signature_id: String,
}

fn main() {
    if let Err(error) = run() {
        eprintln!("damage resolution ledger failed: {error}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), Box<dyn std::error::Error>> {
    let arguments = env::args().skip(1).collect::<Vec<_>>();
    if arguments.len() != 12 {
        return Err(usage().into());
    }
    let route_path = PathBuf::from(option(&arguments, "--route-proof")?);
    let stage_path = PathBuf::from(option(&arguments, "--stage-catalog")?);
    let family_path = PathBuf::from(option(&arguments, "--family-worklist")?);
    let references_path = PathBuf::from(option(&arguments, "--reference-scan")?);
    let output_path = PathBuf::from(option(&arguments, "--output")?);
    let game_build = option(&arguments, "--build")?.to_owned();
    if game_build.is_empty() || !game_build.bytes().all(|byte| byte.is_ascii_digit()) {
        return Err("--build requires a numeric client build".into());
    }

    let route: RouteProof = read_json(&route_path)?;
    let stage: StageCatalog = read_json(&stage_path)?;
    let family: FamilyWorklist = read_json(&family_path)?;
    let references: ReferenceScan = read_json(&references_path)?;
    validate_inputs(&route, &stage, &family, &references, &game_build)?;

    let standard = standard_formula_index(&stage)?;
    let gaps = gap_formula_index(&stage)?;
    let family_index = family_index(&family)?;
    let reference_index = references
        .targets
        .into_iter()
        .map(|target| (target.value, target))
        .collect::<BTreeMap<_, _>>();

    let mut summary = LedgerSummary {
        lookup_keys: route.keys.len(),
        ..LedgerSummary::default()
    };
    let mut entries = Vec::new();
    for key in route.keys {
        for candidate in key.candidates {
            summary.candidate_rows += 1;
            let source_resolved = !candidate.routes.is_empty();
            if source_resolved {
                summary.candidates_with_static_source_route += 1;
            } else {
                summary.candidates_without_static_source_route += 1;
            }
            let recount_state = recount_state(&candidate.recount_owners);
            match recount_state {
                "named-parent" => summary.candidates_with_named_recount_parent += 1,
                "other-parent-only" => summary.candidates_with_other_recount_parent_only += 1,
                "no-parent" => summary.candidates_without_recount_parent += 1,
                _ => unreachable!("recount state is closed"),
            }

            let identity = (key.lookup_key.clone(), candidate.damage_attr_id);
            let standard_rule = standard.get(&identity).cloned();
            let gap = gaps.get(&identity).cloned();
            let (formula_resolved, formula) = match (standard_rule, gap) {
                (Some(rule), None) => {
                    summary.standard_static_formula_candidates += 1;
                    (
                        true,
                        FormulaEvidence::StandardStaticCandidate {
                            promotion_state: stage.promotion_state.clone(),
                            rule,
                        },
                    )
                }
                (None, Some((gap_reason, gap_candidate))) => {
                    summary.nonstandard_or_missing_formula_candidates += 1;
                    let family = family_index.get(&identity);
                    (
                        false,
                        FormulaEvidence::NonstandardOrMissing {
                            gap_reason,
                            family: family.map(|value| value.family.clone()),
                            formula_signature_id: family.map(|value| value.signature_id.clone()),
                            candidate: gap_candidate,
                        },
                    )
                }
                (Some(_), Some(_)) => {
                    return Err(format!(
                        "{} damage {} is simultaneously standard and a retained formula gap",
                        key.lookup_key, candidate.damage_attr_id
                    )
                    .into());
                }
                (None, None) => {
                    return Err(format!(
                        "{} damage {} is absent from both formula indexes",
                        key.lookup_key, candidate.damage_attr_id
                    )
                    .into());
                }
            };
            let readiness = readiness(source_resolved, formula_resolved);
            match readiness {
                "runtime-replay-ready" => summary.runtime_replay_ready_candidates += 1,
                "blocked-source-route" => summary.candidates_blocked_on_source_only += 1,
                "blocked-formula" => summary.candidates_blocked_on_formula_only += 1,
                "blocked-source-and-formula" => {
                    summary.candidates_blocked_on_source_and_formula += 1
                }
                _ => unreachable!("readiness state is closed"),
            }
            let decoded_reference_leads = [key.ability_id, candidate.damage_attr_id]
                .into_iter()
                .filter_map(|value| reference_index.get(&value).cloned())
                .collect::<Vec<_>>();
            summary.candidates_with_reference_leads +=
                usize::from(!decoded_reference_leads.is_empty());
            entries.push(LedgerEntry {
                lookup_key: key.lookup_key.clone(),
                ability_id: key.ability_id,
                hit_event_id: key.hit_event_id,
                damage_attr_id: candidate.damage_attr_id,
                source: SourceEvidence {
                    state: if source_resolved {
                        "static-route-requires-packet-source"
                    } else {
                        "unresolved"
                    },
                    route_key_resolution_state: key.resolution_state.clone(),
                    routes: candidate.routes,
                },
                recount: RecountEvidence {
                    state: recount_state,
                    owners: candidate.recount_owners,
                },
                formula,
                decoded_reference_leads,
                readiness,
            });
        }
    }
    entries.sort_by(|left, right| {
        left.ability_id
            .cmp(&right.ability_id)
            .then_with(|| left.hit_event_id.cmp(&right.hit_event_id))
            .then_with(|| left.damage_attr_id.cmp(&right.damage_attr_id))
    });

    if summary.candidate_rows
        != summary.standard_static_formula_candidates
            + summary.nonstandard_or_missing_formula_candidates
    {
        return Err("formula classification does not conserve candidate rows".into());
    }
    if summary.candidate_rows
        != summary.runtime_replay_ready_candidates
            + summary.candidates_blocked_on_source_only
            + summary.candidates_blocked_on_formula_only
            + summary.candidates_blocked_on_source_and_formula
    {
        return Err("readiness classification does not conserve candidate rows".into());
    }

    let ledger = ResolutionLedger {
        schema_version: SCHEMA_VERSION,
        game_build,
        generated_by: "rlogs-bpsr-damage-resolution-ledger",
        promotion_state: "research-only-current-build-runtime-replay-required",
        inputs: vec![
            input_artifact("damage-source-route-proof", &route_path)?,
            input_artifact("damage-stage-runtime-catalog", &stage_path)?,
            input_artifact("damage-script-family-worklist", &family_path)?,
            input_artifact("decoded-table-reference-scan", &references_path)?,
        ],
        policy: LedgerPolicy {
            packet_source_and_recount_parent_are_separate: true,
            recount_parent_is_formula_authority: false,
            decoded_scalar_reference_is_formula_authority: false,
            unresolved_evidence_hidden: false,
            static_formula_is_runtime_authority: false,
            runtime_promotion_rule: "requires an exact current-build packet source route, same-build packet occurrence, isolated server formula semantics, and conservation replay before rDPS use",
        },
        summary,
        entries,
    };
    if let Some(parent) = output_path.parent() {
        fs::create_dir_all(parent)?;
    }
    let mut writer = BufWriter::new(File::create(&output_path)?);
    serde_json::to_writer(&mut writer, &ledger)?;
    writer.write_all(b"\n")?;
    writer.flush()?;
    eprintln!(
        "wrote {} conserved candidates: {} replay-ready, {} source-only blocked, {} formula-only blocked, {} blocked on both",
        ledger.summary.candidate_rows,
        ledger.summary.runtime_replay_ready_candidates,
        ledger.summary.candidates_blocked_on_source_only,
        ledger.summary.candidates_blocked_on_formula_only,
        ledger.summary.candidates_blocked_on_source_and_formula,
    );
    Ok(())
}

fn validate_inputs(
    route: &RouteProof,
    stage: &StageCatalog,
    family: &FamilyWorklist,
    references: &ReferenceScan,
    build: &str,
) -> Result<(), String> {
    if route.schema_version < 8 {
        return Err(format!(
            "route proof schema {} is unsupported; expected 8 or newer",
            route.schema_version
        ));
    }
    if stage.schema_version < 8 {
        return Err(format!(
            "stage catalog schema {} is unsupported; expected 8 or newer",
            stage.schema_version
        ));
    }
    if family.schema_version < 1 {
        return Err("family worklist schema must be at least 1".to_owned());
    }
    if references.schema_version < 2 {
        return Err(format!(
            "reference scan schema {} is unsupported; expected 2 or newer",
            references.schema_version
        ));
    }
    for (role, actual) in [
        ("route proof", route.game_build.as_str()),
        ("stage catalog", stage.game_build.as_str()),
        ("family worklist", family.game_build.as_str()),
        ("reference scan", references.build_id.as_str()),
    ] {
        if actual != build {
            return Err(format!(
                "{role} build {actual} does not match requested build {build}"
            ));
        }
    }
    Ok(())
}

fn standard_formula_index(
    stage: &StageCatalog,
) -> Result<BTreeMap<(String, i64), StageRule>, String> {
    let mut index = BTreeMap::new();
    for rule in &stage.rules {
        let lookup_key = format!("{}:{}", rule.ability_id, rule.hit_event_id);
        for damage_attr_id in &rule.equivalent_damage_attr_ids {
            if index
                .insert((lookup_key.clone(), *damage_attr_id), rule.clone())
                .is_some()
            {
                return Err(format!(
                    "duplicate standard formula for {lookup_key} damage {damage_attr_id}"
                ));
            }
        }
        if !rule
            .equivalent_damage_attr_ids
            .contains(&rule.damage_attr_id)
        {
            return Err(format!(
                "standard rule {lookup_key} omits selected damage {} from equivalents",
                rule.damage_attr_id
            ));
        }
    }
    Ok(index)
}

fn gap_formula_index(stage: &StageCatalog) -> Result<GapFormulaIndex, String> {
    let mut index = BTreeMap::new();
    for gap in &stage.coverage_gaps {
        if gap.gap_class != "nonstandard-or-missing-script" {
            continue;
        }
        for candidate in &gap.candidates {
            if index
                .insert(
                    (gap.lookup_key.clone(), candidate.damage_attr_id),
                    (gap.reason.clone(), candidate.clone()),
                )
                .is_some()
            {
                return Err(format!(
                    "duplicate formula gap for {} damage {}",
                    gap.lookup_key, candidate.damage_attr_id
                ));
            }
        }
    }
    Ok(index)
}

fn family_index(
    worklist: &FamilyWorklist,
) -> Result<BTreeMap<(String, i64), FamilyIdentity>, String> {
    let mut index = BTreeMap::new();
    for family in &worklist.families {
        for signature in &family.formula_signatures {
            for item in &signature.work_items {
                let key = (item.lookup_key.clone(), item.damage_attr.damage_attr_id);
                if index
                    .insert(
                        key.clone(),
                        FamilyIdentity {
                            family: family.damage_script.clone(),
                            signature_id: signature.signature_id.clone(),
                        },
                    )
                    .is_some()
                {
                    return Err(format!(
                        "duplicate family identity for {} damage {}",
                        key.0, key.1
                    ));
                }
            }
        }
    }
    Ok(index)
}

fn recount_state(owners: &[RecountOwner]) -> &'static str {
    if owners.is_empty() {
        "no-parent"
    } else if owners.iter().any(|owner| owner.recount_name != "Other") {
        "named-parent"
    } else {
        "other-parent-only"
    }
}

fn readiness(source_resolved: bool, formula_resolved: bool) -> &'static str {
    match (source_resolved, formula_resolved) {
        (true, true) => "runtime-replay-ready",
        (false, true) => "blocked-source-route",
        (true, false) => "blocked-formula",
        (false, false) => "blocked-source-and-formula",
    }
}

fn read_json<T: for<'de> Deserialize<'de>>(path: &Path) -> Result<T, Box<dyn std::error::Error>> {
    Ok(serde_json::from_reader(BufReader::new(File::open(path)?))?)
}

fn input_artifact(
    role: &'static str,
    path: &Path,
) -> Result<InputArtifact, Box<dyn std::error::Error>> {
    let mut file = File::open(path)?;
    let bytes = file.metadata()?.len();
    let mut hasher = Sha256::new();
    let mut buffer = [0_u8; 64 * 1024];
    loop {
        let read = file.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    Ok(InputArtifact {
        role,
        file: path
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or_default()
            .to_owned(),
        bytes,
        sha256: format!("{:x}", hasher.finalize()),
    })
}

fn option<'a>(arguments: &'a [String], name: &str) -> Result<&'a str, String> {
    let position = arguments
        .iter()
        .position(|argument| argument == name)
        .ok_or_else(usage)?;
    arguments
        .get(position + 1)
        .map(String::as_str)
        .ok_or_else(usage)
}

fn usage() -> String {
    "usage: rlogs-bpsr-damage-resolution-ledger --route-proof <damage-source-route-proof.json> --stage-catalog <damage-stage-runtime-catalog.json> --family-worklist <damage-script-family-worklist.json> --reference-scan <decoded-table-reference-scan.json> --build <numeric-client-build> --output <damage-resolution-ledger.json>".to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn readiness_keeps_source_and_formula_blocks_separate() {
        assert_eq!(readiness(true, true), "runtime-replay-ready");
        assert_eq!(readiness(false, true), "blocked-source-route");
        assert_eq!(readiness(true, false), "blocked-formula");
        assert_eq!(readiness(false, false), "blocked-source-and-formula");
    }

    #[test]
    fn recount_other_is_not_promoted_to_named_ownership() {
        assert_eq!(recount_state(&[]), "no-parent");
        assert_eq!(
            recount_state(&[RecountOwner {
                recount_id: 349,
                recount_name: "Other".to_owned(),
            }]),
            "other-parent-only"
        );
        assert_eq!(
            recount_state(&[RecountOwner {
                recount_id: 106,
                recount_name: "Explosive Arrow".to_owned(),
            }]),
            "named-parent"
        );
    }
}
