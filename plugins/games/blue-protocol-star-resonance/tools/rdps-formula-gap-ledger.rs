use std::{
    collections::{BTreeMap, BTreeSet},
    env,
    error::Error,
    fs::File,
    io::{BufReader, BufWriter, Write},
    path::{Path, PathBuf},
};

use serde::Serialize;
use serde_json::{Value, json};

const SCHEMA_VERSION: u16 = 13;
const FORMULA_GAP_CATEGORY: &str = "formula-magnitude-unresolved";

#[derive(Debug, Serialize)]
#[serde(deny_unknown_fields)]
struct FormulaGapLedger {
    schema_version: u16,
    generated_by: &'static str,
    static_game_build: String,
    discovery_game_build: String,
    promotion_state: &'static str,
    policy: Policy,
    inputs: Inputs,
    corpus: CorpusSummary,
    summary: Summary,
    candidates: Vec<CandidateGap>,
}

#[derive(Debug, Serialize)]
#[serde(deny_unknown_fields)]
struct Policy {
    unresolved_evidence_hidden: bool,
    historical_absence_proves_mechanic_absence: bool,
    historical_coefficients_promote_current_build: bool,
    formula_values_inferred_from_descriptions: bool,
    exact_current_build_packet_replay_required: bool,
    purpose: &'static str,
}

#[derive(Debug, Serialize)]
#[serde(deny_unknown_fields)]
struct Inputs {
    semantic_audit: String,
    magnitude_watchlist: String,
    current_origin_ledger: String,
    current_source_index: String,
    historical_packet_proof: String,
    retained_historical_proofs: Option<String>,
}

#[derive(Clone, Debug, Default, Serialize)]
#[serde(deny_unknown_fields)]
struct CorpusSummary {
    sessions: usize,
    run_ordinals_observed: u64,
    actor_events: u64,
    attribute_events: u64,
    all_status_events: u64,
    selected_status_events: u64,
    decoded_selected_attribute_values: u64,
    undecodable_selected_attribute_values: u64,
}

#[derive(Debug, Default, Serialize)]
#[serde(deny_unknown_fields)]
struct Summary {
    formula_gap_candidates: usize,
    distinct_effect_ids: usize,
    packet_observed_effect_ids: usize,
    packet_unobserved_effect_ids: usize,
    packet_observed_status_events: u64,
    candidates_with_packet_observations: usize,
    candidates_without_packet_observations: usize,
    candidates_with_historical_coefficient_proof: usize,
    candidates_with_retained_historical_proof: usize,
    effects_with_current_active_modifier_parameter_evidence: usize,
    effects_with_exact_current_component_owner: usize,
    effects_with_exact_current_relationship_owner: usize,
    effects_with_only_current_candidate_owner: usize,
    effects_with_only_current_semantic_owner_candidate: usize,
    effects_without_current_owner_candidate: usize,
    candidates_eligible_for_current_build_promotion: usize,
    outcomes: BTreeMap<String, usize>,
}

#[derive(Debug, Serialize)]
#[serde(deny_unknown_fields)]
struct CandidateGap {
    source_rule_id: String,
    source_id: Option<String>,
    source_name: Option<String>,
    formula_term_ids: Vec<String>,
    transfer_eligibilities: Vec<String>,
    effective_transfer_eligibilities: Vec<String>,
    transfer_scope_resolution: &'static str,
    effect_ids: Vec<i64>,
    declared_effect_references: Vec<i64>,
    rejected_effect_references: Vec<i64>,
    selected_attribute_ids: Vec<i64>,
    lifecycle_effects: Vec<LifecycleEffect>,
    required_runtime_evidence: Vec<String>,
    static_blockers: Vec<String>,
    current_owner_evidence: Vec<CurrentOwnerEvidence>,
    historical_packet_observations: Vec<EffectObservation>,
    retained_historical_proofs: Vec<RetainedHistoricalProof>,
    outcome: &'static str,
    current_build_promotion_eligible: bool,
    remaining_requirement: &'static str,
}

#[derive(Clone, Debug, Serialize)]
#[serde(deny_unknown_fields)]
struct CurrentOwnerEvidence {
    effect_id: i64,
    owner_evidence_state: String,
    exact_component_skill_ids: Vec<i64>,
    exact_relationship_skill_ids: Vec<i64>,
    strong_owner_skill_ids: Vec<i64>,
    broad_owner_skill_ids: Vec<i64>,
    semantic_owner_skill_ids: Vec<i64>,
    semantic_recipient_scopes: Vec<String>,
    semantic_rdps_dispositions: Vec<String>,
    exact_component_recipient_scopes: Vec<String>,
    exact_component_rdps_dispositions: Vec<String>,
    current_active_modifier_parameter_evidence: Vec<Value>,
}

#[derive(Debug, Serialize)]
#[serde(deny_unknown_fields)]
struct LifecycleEffect {
    effect_id: i64,
    name: Option<String>,
    icon: Option<String>,
    repeat_add_rule: Vec<i64>,
    declared_max_stacks: i64,
    proof_model: String,
    destroy_param: Value,
}

#[derive(Clone, Debug, Serialize)]
#[serde(deny_unknown_fields)]
struct RetainedHistoricalProof {
    effect_id: i64,
    historical_packet_build_id: String,
    historical_proof: String,
    carry_forward_state: String,
    current_build_runtime_enabled: bool,
}

#[derive(Clone, Debug, Default, Serialize)]
#[serde(deny_unknown_fields)]
struct EffectObservation {
    effect_id: i64,
    status_events: u64,
    mechanic_state_changes: u64,
    selected_attributes_examined: usize,
    attributes_with_complete_pairs: usize,
    complete_attribute_pairs: u64,
    isolated_attribute_pairs: u64,
    competing_attribute_pairs: u64,
    same_wire_attribute_delta_observations: u64,
    binary_presence_equation_occurrences: u64,
    reversible_static_coefficient_proofs: usize,
    matched_lifecycle_coefficient_proofs: usize,
    historical_runtime_eligible_proofs: usize,
}

fn main() {
    if let Err(error) = run(env::args().skip(1).collect()) {
        eprintln!("{error}");
        std::process::exit(1);
    }
}

fn run(arguments: Vec<String>) -> Result<(), Box<dyn Error>> {
    let options = parse_options(&arguments)?;
    let semantic_path = required_path(&options, "semantic-audit")?;
    let watchlist_path = required_path(&options, "watchlist")?;
    let current_origin_path = required_path(&options, "origin-ledger")?;
    let current_source_index_path = required_path(&options, "source-index")?;
    let packet_proof_path = required_path(&options, "packet-proof")?;
    let retained_proofs_path = options.get("retained-proofs").map(PathBuf::from);
    let discovery_game_build = required(&options, "discovery-build")?.to_owned();
    let output_path = required_path(&options, "output")?;
    let gap_watchlist_output_path = options.get("gap-watchlist-output").map(PathBuf::from);
    validate_build(&discovery_game_build)?;

    let semantic = read_json(&semantic_path)?;
    let watchlist = read_json(&watchlist_path)?;
    let current_origin = read_json(&current_origin_path)?;
    let current_source_index = read_json(&current_source_index_path)?;
    let packet_proof = read_json(&packet_proof_path)?;
    let retained_proofs = retained_proofs_path.as_deref().map(read_json).transpose()?;
    require_generated_by(&semantic, "rlogs-bpsr-static-rdps-semantic-audit")?;
    let watchlist_generator =
        string_at(&watchlist, "generated_by").ok_or("watchlist generated_by is missing")?;
    if !matches!(
        watchlist_generator,
        "rlogs-bpsr-static-rdps-worklist" | "rlogs-bpsr-rdps-formula-gap-ledger"
    ) {
        return Err("expected static worklist or formula-gap watchlist input".into());
    }
    require_generated_by(&packet_proof, "rlogs-bpsr-rdps-status-proof-compact")?;

    let static_game_build = string_at(&semantic, "game_build")
        .ok_or("semantic audit game_build is missing")?
        .to_owned();
    if string_at(&watchlist, "game_build") != Some(static_game_build.as_str()) {
        return Err("semantic audit and watchlist game builds differ".into());
    }
    if string_at(&current_origin, "game_build") != Some(static_game_build.as_str()) {
        return Err("semantic audit and current origin ledger game builds differ".into());
    }
    if unsigned_at(&current_origin, "schema_version") < 14 {
        return Err("current origin ledger schema 14 or newer is required".into());
    }
    if let Some(retained) = &retained_proofs {
        if string_at(retained, "build_id") != Some(static_game_build.as_str()) {
            return Err("retained proof manifest and static audit game builds differ".into());
        }
        if retained
            .get("policy")
            .and_then(|policy| policy.get("runtime_promotion_allowed"))
            .and_then(Value::as_bool)
            != Some(false)
        {
            return Err("retained proof manifest must explicitly disable runtime promotion".into());
        }
    }

    let watchlist_by_rule = array_at(&watchlist, "candidates")?
        .iter()
        .map(|candidate| {
            let id = string_at(candidate, "source_rule_id")
                .ok_or("watchlist candidate source_rule_id is missing")?;
            Ok((id.to_owned(), candidate))
        })
        .collect::<Result<BTreeMap<_, _>, Box<dyn Error>>>()?;
    let semantic_by_rule = array_at(&semantic, "findings")?
        .iter()
        .filter_map(|finding| {
            string_at(finding, "source_rule_id").map(|rule_id| (rule_id.to_owned(), finding))
        })
        .collect::<BTreeMap<_, _>>();
    let effect_observations = collect_effect_observations(&packet_proof)?;
    let (mut current_owners_by_effect, current_owners_by_source_effect) =
        collect_source_index_owner_evidence_sets(&current_source_index)?;
    for (effect_id, evidence) in collect_current_owner_evidence(&current_origin)? {
        current_owners_by_effect.insert(effect_id, evidence);
    }
    let retained_proofs_by_effect = retained_proofs
        .as_ref()
        .map(collect_retained_proofs)
        .transpose()?
        .unwrap_or_default();
    let mut candidates = Vec::new();
    let mut distinct_effect_ids = BTreeSet::new();

    for (source_rule_id, watch) in &watchlist_by_rule {
        let finding = semantic_by_rule
            .get(source_rule_id)
            .copied()
            .filter(|finding| has_issue(finding, FORMULA_GAP_CATEGORY));
        let effect_ids = integer_array_at(watch, "effect_ids")?;
        let source_id = finding
            .and_then(|finding| optional_string_at(finding, "source_id"))
            .or_else(|| optional_string_at(watch, "source_id"));
        distinct_effect_ids.extend(effect_ids.iter().copied());
        let observations = effect_ids
            .iter()
            .map(|effect_id| {
                effect_observations
                    .get(effect_id)
                    .cloned()
                    .unwrap_or_else(|| EffectObservation {
                        effect_id: *effect_id,
                        ..EffectObservation::default()
                    })
            })
            .collect::<Vec<_>>();
        let retained = effect_ids
            .iter()
            .filter_map(|effect_id| retained_proofs_by_effect.get(effect_id).cloned())
            .collect::<Vec<_>>();
        let current_owner_evidence = effect_ids
            .iter()
            .map(|effect_id| {
                source_id
                    .as_ref()
                    .and_then(|source_id| {
                        current_owners_by_source_effect.get(&(source_id.clone(), *effect_id))
                    })
                    .or_else(|| current_owners_by_effect.get(effect_id))
                    .cloned()
                    .unwrap_or_else(|| unresolved_current_owner_evidence(*effect_id))
            })
            .collect::<Vec<_>>();
        let transfer_eligibilities = match finding {
            Some(finding) => string_array_at(finding, "transfer_eligibilities")?,
            None => string_array_at(watch, "transfer_eligibilities")?,
        };
        let (effective_transfer_eligibilities, transfer_scope_resolution) =
            resolve_transfer_scope(&transfer_eligibilities, &current_owner_evidence);
        let outcome = classify_candidate(&observations, !retained.is_empty());

        let lifecycle_effects = lifecycle_effects_at(watch)?;
        let source_name = finding
            .and_then(|finding| optional_string_at(finding, "source_name"))
            .or_else(|| {
                lifecycle_effects
                    .first()
                    .and_then(|effect| effect.name.clone())
            });
        candidates.push(CandidateGap {
            source_rule_id: source_rule_id.clone(),
            source_id,
            source_name,
            formula_term_ids: match finding {
                Some(finding) => string_array_at(finding, "formula_term_ids")?,
                None => string_array_at(watch, "formula_terms")?,
            },
            transfer_eligibilities,
            effective_transfer_eligibilities,
            transfer_scope_resolution,
            effect_ids,
            declared_effect_references: integer_array_at(watch, "declared_effect_references")?,
            rejected_effect_references: integer_array_at(watch, "rejected_effect_references")?,
            selected_attribute_ids: integer_array_at(watch, "selected_attribute_ids")?,
            lifecycle_effects,
            required_runtime_evidence: string_array_at(watch, "required_runtime_evidence")?,
            static_blockers: string_array_at(watch, "static_blockers")?,
            current_owner_evidence,
            historical_packet_observations: observations,
            retained_historical_proofs: retained,
            outcome,
            current_build_promotion_eligible: false,
            remaining_requirement: "exact current-build provider/recipient lifecycle, formula inputs, observed output, counterfactual damage, and conservation replay",
        });
    }
    candidates.sort_by(|left, right| left.source_rule_id.cmp(&right.source_rule_id));

    let corpus = collect_corpus_summary(&packet_proof)?;
    let mut summary = Summary {
        formula_gap_candidates: candidates.len(),
        distinct_effect_ids: distinct_effect_ids.len(),
        packet_observed_effect_ids: distinct_effect_ids
            .iter()
            .filter(|effect_id| {
                effect_observations
                    .get(effect_id)
                    .is_some_and(|effect| effect.status_events > 0)
            })
            .count(),
        packet_unobserved_effect_ids: distinct_effect_ids
            .iter()
            .filter(|effect_id| {
                !effect_observations
                    .get(effect_id)
                    .is_some_and(|effect| effect.status_events > 0)
            })
            .count(),
        packet_observed_status_events: distinct_effect_ids
            .iter()
            .filter_map(|effect_id| effect_observations.get(effect_id))
            .map(|effect| effect.status_events)
            .sum(),
        candidates_with_packet_observations: candidates
            .iter()
            .filter(|candidate| {
                candidate
                    .historical_packet_observations
                    .iter()
                    .any(|effect| effect.status_events > 0)
            })
            .count(),
        candidates_without_packet_observations: candidates
            .iter()
            .filter(|candidate| {
                candidate
                    .historical_packet_observations
                    .iter()
                    .all(|effect| effect.status_events == 0)
            })
            .count(),
        candidates_with_historical_coefficient_proof: candidates
            .iter()
            .filter(|candidate| {
                candidate
                    .historical_packet_observations
                    .iter()
                    .any(|effect| {
                        effect.reversible_static_coefficient_proofs > 0
                            || effect.matched_lifecycle_coefficient_proofs > 0
                    })
            })
            .count(),
        candidates_with_retained_historical_proof: candidates
            .iter()
            .filter(|candidate| !candidate.retained_historical_proofs.is_empty())
            .count(),
        effects_with_current_active_modifier_parameter_evidence: candidates
            .iter()
            .flat_map(|candidate| &candidate.current_owner_evidence)
            .filter(|owner| !owner.current_active_modifier_parameter_evidence.is_empty())
            .count(),
        effects_with_exact_current_component_owner: candidates
            .iter()
            .flat_map(|candidate| &candidate.current_owner_evidence)
            .filter(|owner| !owner.exact_component_skill_ids.is_empty())
            .count(),
        effects_with_exact_current_relationship_owner: candidates
            .iter()
            .flat_map(|candidate| &candidate.current_owner_evidence)
            .filter(|owner| {
                owner.exact_component_skill_ids.is_empty()
                    && !owner.exact_relationship_skill_ids.is_empty()
            })
            .count(),
        effects_with_only_current_candidate_owner: candidates
            .iter()
            .flat_map(|candidate| &candidate.current_owner_evidence)
            .filter(|owner| {
                owner.exact_component_skill_ids.is_empty()
                    && owner.exact_relationship_skill_ids.is_empty()
                    && (!owner.strong_owner_skill_ids.is_empty()
                        || !owner.broad_owner_skill_ids.is_empty())
            })
            .count(),
        effects_with_only_current_semantic_owner_candidate: candidates
            .iter()
            .flat_map(|candidate| &candidate.current_owner_evidence)
            .filter(|owner| {
                owner.exact_component_skill_ids.is_empty()
                    && owner.exact_relationship_skill_ids.is_empty()
                    && owner.strong_owner_skill_ids.is_empty()
                    && owner.broad_owner_skill_ids.is_empty()
                    && !owner.semantic_owner_skill_ids.is_empty()
            })
            .count(),
        effects_without_current_owner_candidate: candidates
            .iter()
            .flat_map(|candidate| &candidate.current_owner_evidence)
            .filter(|owner| {
                owner.exact_component_skill_ids.is_empty()
                    && owner.exact_relationship_skill_ids.is_empty()
                    && owner.strong_owner_skill_ids.is_empty()
                    && owner.broad_owner_skill_ids.is_empty()
                    && owner.semantic_owner_skill_ids.is_empty()
                    && owner.exact_component_recipient_scopes.is_empty()
            })
            .count(),
        candidates_eligible_for_current_build_promotion: 0,
        outcomes: BTreeMap::new(),
    };
    for candidate in &candidates {
        *summary
            .outcomes
            .entry(candidate.outcome.to_owned())
            .or_default() += 1;
    }

    if let Some(path) = gap_watchlist_output_path {
        write_gap_watchlist(&watchlist, &candidates, &path)?;
    }

    let ledger = FormulaGapLedger {
        schema_version: SCHEMA_VERSION,
        generated_by: "rlogs-bpsr-rdps-formula-gap-ledger",
        static_game_build,
        discovery_game_build,
        promotion_state: "blocked-pending-current-build-packet-proof",
        policy: Policy {
            unresolved_evidence_hidden: false,
            historical_absence_proves_mechanic_absence: false,
            historical_coefficients_promote_current_build: false,
            formula_values_inferred_from_descriptions: false,
            exact_current_build_packet_replay_required: true,
            purpose: "reproducible gap accounting only; this ledger never enables rDPS",
        },
        inputs: Inputs {
            semantic_audit: display_path(&semantic_path),
            magnitude_watchlist: display_path(&watchlist_path),
            current_origin_ledger: display_path(&current_origin_path),
            current_source_index: display_path(&current_source_index_path),
            historical_packet_proof: display_path(&packet_proof_path),
            retained_historical_proofs: retained_proofs_path.as_deref().map(display_path),
        },
        corpus,
        summary,
        candidates,
    };

    if let Some(parent) = output_path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let output = File::create(&output_path)?;
    let mut writer = BufWriter::new(output);
    serde_json::to_writer_pretty(&mut writer, &ledger)?;
    writer.write_all(b"\n")?;
    Ok(())
}

fn write_gap_watchlist(
    source_watchlist: &Value,
    candidates: &[CandidateGap],
    output_path: &Path,
) -> Result<(), Box<dyn Error>> {
    let reduced = build_gap_watchlist(source_watchlist, candidates)?;
    if let Some(parent) = output_path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let output = File::create(output_path)?;
    let mut writer = BufWriter::new(output);
    serde_json::to_writer_pretty(&mut writer, &reduced)?;
    writer.write_all(b"\n")?;
    Ok(())
}

fn build_gap_watchlist(
    source_watchlist: &Value,
    candidates: &[CandidateGap],
) -> Result<Value, Box<dyn Error>> {
    let ledger_gap_rule_ids = candidates
        .iter()
        .map(|candidate| candidate.source_rule_id.clone())
        .collect::<BTreeSet<_>>();
    if ledger_gap_rule_ids.is_empty() {
        return Err("formula-gap watchlist cannot be empty".into());
    }

    let source_candidates = array_at(source_watchlist, "candidates")?;
    let missing_value_rule_ids = source_candidates
        .iter()
        .filter(|candidate| {
            string_at(candidate, "static_value_state") == Some("missing-value-proof")
        })
        .filter_map(|candidate| string_at(candidate, "source_rule_id").map(str::to_owned))
        .collect::<BTreeSet<_>>();
    let selected_rule_ids = ledger_gap_rule_ids
        .union(&missing_value_rule_ids)
        .cloned()
        .collect::<BTreeSet<_>>();
    let selected_candidates = source_candidates
        .iter()
        .filter(|candidate| {
            string_at(candidate, "source_rule_id")
                .is_some_and(|rule_id| selected_rule_ids.contains(rule_id))
        })
        .cloned()
        .collect::<Vec<_>>();
    if selected_candidates.len() != selected_rule_ids.len() {
        let found_rule_ids = selected_candidates
            .iter()
            .filter_map(|candidate| string_at(candidate, "source_rule_id"))
            .collect::<BTreeSet<_>>();
        let missing = selected_rule_ids
            .iter()
            .map(String::as_str)
            .collect::<BTreeSet<_>>()
            .difference(&found_rule_ids)
            .copied()
            .collect::<Vec<_>>();
        return Err(format!(
            "formula-gap watchlist is missing source rules: {}",
            missing.join(", ")
        )
        .into());
    }

    let selected_effect_ids = selected_candidates
        .iter()
        .flat_map(|candidate| {
            candidate
                .get("effect_ids")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
                .filter_map(Value::as_i64)
        })
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect::<Vec<_>>();
    let mut selected_attribute_ids = selected_candidates
        .iter()
        .flat_map(|candidate| {
            candidate
                .get("selected_attribute_ids")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
                .filter_map(Value::as_i64)
        })
        .collect::<BTreeSet<_>>();
    for attribute_id in
        integer_array_at(source_watchlist, "non_attributable_context_attribute_ids")?
    {
        selected_attribute_ids.insert(attribute_id);
    }

    let selected_attribute_ids = selected_attribute_ids.into_iter().collect::<Vec<_>>();
    let mut reduced = source_watchlist.clone();
    let object = reduced
        .as_object_mut()
        .ok_or("magnitude watchlist must be an object")?;
    object.insert(
        "generated_by".to_owned(),
        json!("rlogs-bpsr-rdps-formula-gap-ledger"),
    );
    object.insert(
        "promotion_state".to_owned(),
        json!(
            "all-missing-formula-value-proofs-require-current-build-packet-proof-runtime-disabled"
        ),
    );
    object.insert(
        "formula_gap_rule_ids".to_owned(),
        json!(ledger_gap_rule_ids),
    );
    object.insert(
        "missing_value_proof_rule_ids".to_owned(),
        json!(missing_value_rule_ids),
    );
    object.insert(
        "selection_summary".to_owned(),
        json!({
            "ledger_formula_gap_rules": candidates.len(),
            "missing_value_proof_rules": object["missing_value_proof_rule_ids"]
                .as_array()
                .map(Vec::len)
                .unwrap_or_default(),
            "selected_unresolved_formula_rules": selected_candidates.len()
        }),
    );
    object.insert(
        "selected_effect_ids".to_owned(),
        json!(selected_effect_ids.clone()),
    );
    object.insert("reported_effect_ids".to_owned(), json!(selected_effect_ids));
    object.insert(
        "selected_attribute_ids".to_owned(),
        json!(selected_attribute_ids),
    );
    object.insert("candidates".to_owned(), json!(selected_candidates));

    Ok(reduced)
}

fn collect_corpus_summary(packet_proof: &Value) -> Result<CorpusSummary, Box<dyn Error>> {
    let sessions = array_at(packet_proof, "sessions")?;
    let mut summary = CorpusSummary {
        sessions: sessions.len(),
        ..CorpusSummary::default()
    };
    for session in sessions {
        summary.run_ordinals_observed += unsigned_at(session, "run_ordinals_observed");
        summary.actor_events += unsigned_at(session, "actor_events");
        summary.attribute_events += unsigned_at(session, "attribute_events");
        summary.all_status_events += unsigned_at(session, "all_status_events");
        summary.selected_status_events += unsigned_at(session, "selected_status_events");
        summary.decoded_selected_attribute_values +=
            unsigned_at(session, "decoded_selected_attribute_values");
        summary.undecodable_selected_attribute_values +=
            unsigned_at(session, "undecodable_selected_attribute_values");
    }
    Ok(summary)
}

fn collect_effect_observations(
    packet_proof: &Value,
) -> Result<BTreeMap<i64, EffectObservation>, Box<dyn Error>> {
    let static_proofs = proof_counts(packet_proof, "reversible_static_coefficient_proofs")?;
    let lifecycle_proofs = proof_counts(packet_proof, "matched_lifecycle_coefficient_proofs")?;
    let equation_counts = equation_counts(packet_proof)?;
    let mut observations = BTreeMap::new();

    for effect in array_at(packet_proof, "effects")? {
        let effect_id =
            signed_at(effect, "effect_id").ok_or("packet proof effect_id is missing")?;
        let attributes = array_at(effect, "attributes")?;
        let mut observation = EffectObservation {
            effect_id,
            status_events: unsigned_at(effect, "selected_status_events"),
            mechanic_state_changes: unsigned_at(effect, "selected_mechanic_state_changes"),
            selected_attributes_examined: attributes.len(),
            binary_presence_equation_occurrences: equation_counts
                .get(&effect_id)
                .copied()
                .unwrap_or_default(),
            ..EffectObservation::default()
        };
        for attribute in attributes {
            let complete = unsigned_at(attribute, "complete_before_and_after");
            if complete > 0 {
                observation.attributes_with_complete_pairs += 1;
            }
            observation.complete_attribute_pairs += complete;
            observation.isolated_attribute_pairs += unsigned_at(attribute, "isolated_transitions");
            observation.competing_attribute_pairs +=
                unsigned_at(attribute, "transitions_with_competing_target_statuses");
            for aggregate in array_at(attribute, "aggregates")? {
                if bool_at(aggregate, "same_wire_attribute_update") {
                    observation.same_wire_attribute_delta_observations +=
                        unsigned_at(aggregate, "count");
                }
            }
        }
        if let Some((count, eligible)) = static_proofs.get(&effect_id) {
            observation.reversible_static_coefficient_proofs = *count;
            observation.historical_runtime_eligible_proofs += *eligible;
        }
        if let Some((count, eligible)) = lifecycle_proofs.get(&effect_id) {
            observation.matched_lifecycle_coefficient_proofs = *count;
            observation.historical_runtime_eligible_proofs += *eligible;
        }
        observations.insert(effect_id, observation);
    }
    Ok(observations)
}

fn collect_retained_proofs(
    manifest: &Value,
) -> Result<BTreeMap<i64, RetainedHistoricalProof>, Box<dyn Error>> {
    let mut proofs = BTreeMap::new();
    for proof in array_at(manifest, "proofs")? {
        let effect_id = signed_at(proof, "effect_id")
            .ok_or("retained historical proof effect_id is missing")?;
        let retained = RetainedHistoricalProof {
            effect_id,
            historical_packet_build_id: string_at(proof, "historical_packet_build_id")
                .ok_or("retained historical proof packet build is missing")?
                .to_owned(),
            historical_proof: string_at(proof, "historical_proof")
                .ok_or("retained historical proof path is missing")?
                .to_owned(),
            carry_forward_state: string_at(proof, "carry_forward_state")
                .ok_or("retained historical proof carry-forward state is missing")?
                .to_owned(),
            current_build_runtime_enabled: bool_at(proof, "current_build_runtime_enabled"),
        };
        if retained.current_build_runtime_enabled {
            return Err(format!(
                "retained historical proof {effect_id} must not enable current-build runtime"
            )
            .into());
        }
        if proofs.insert(effect_id, retained).is_some() {
            return Err(
                format!("duplicate retained historical proof for effect {effect_id}").into(),
            );
        }
    }
    Ok(proofs)
}

fn collect_current_owner_evidence(
    ledger: &Value,
) -> Result<BTreeMap<i64, CurrentOwnerEvidence>, Box<dyn Error>> {
    let mut owners = BTreeMap::new();
    for effect in array_at(ledger, "legacy_formula_gap_effects")? {
        let effect_id = signed_at(effect, "effect_id")
            .ok_or("current origin formula-gap effect_id is missing")?;
        let owner = CurrentOwnerEvidence {
            effect_id,
            owner_evidence_state: string_at(effect, "owner_evidence_state")
                .ok_or("current origin owner_evidence_state is missing")?
                .to_owned(),
            exact_component_skill_ids: integer_array_at(
                effect,
                "current_exact_component_skill_ids",
            )?,
            exact_relationship_skill_ids: integer_array_at(
                effect,
                "current_exact_relationship_skill_ids",
            )?,
            strong_owner_skill_ids: integer_array_at(effect, "current_strong_owner_skill_ids")?,
            broad_owner_skill_ids: integer_array_at(effect, "current_broad_owner_skill_ids")?,
            semantic_owner_skill_ids: integer_array_at(effect, "current_semantic_owner_skill_ids")?,
            semantic_recipient_scopes: string_array_at(
                effect,
                "current_semantic_recipient_scopes",
            )?,
            semantic_rdps_dispositions: string_array_at(
                effect,
                "current_semantic_rdps_dispositions",
            )?,
            exact_component_recipient_scopes: string_array_at(
                effect,
                "current_exact_component_recipient_scopes",
            )?,
            exact_component_rdps_dispositions: string_array_at(
                effect,
                "current_exact_component_rdps_dispositions",
            )?,
            current_active_modifier_parameter_evidence: array_at(
                effect,
                "current_active_modifier_parameter_evidence",
            )?
            .to_vec(),
        };
        if owners.insert(effect_id, owner).is_some() {
            return Err(format!("duplicate current origin formula-gap effect {effect_id}").into());
        }
    }
    Ok(owners)
}

#[cfg(test)]
fn collect_source_index_owner_evidence(
    source_index: &Value,
) -> Result<BTreeMap<i64, CurrentOwnerEvidence>, Box<dyn Error>> {
    Ok(collect_source_index_owner_evidence_sets(source_index)?.0)
}

type SourceEffectOwnerEvidence = BTreeMap<(String, i64), CurrentOwnerEvidence>;

fn collect_source_index_owner_evidence_sets(
    source_index: &Value,
) -> Result<
    (
        BTreeMap<i64, CurrentOwnerEvidence>,
        SourceEffectOwnerEvidence,
    ),
    Box<dyn Error>,
> {
    if unsigned_at(source_index, "schemaVersion") < 1 {
        return Err("current source index schema 1 or newer is required".into());
    }
    let by_buff_id = source_index
        .get("byBuffId")
        .and_then(Value::as_object)
        .ok_or("current source index byBuffId is missing")?;
    let mut owners = BTreeMap::new();
    let mut owners_by_source_effect = BTreeMap::new();
    for (raw_effect_id, raw_sources) in by_buff_id {
        let effect_id = raw_effect_id
            .parse::<i64>()
            .map_err(|_| format!("invalid source-index buff id {raw_effect_id}"))?;
        let sources = raw_sources
            .as_array()
            .ok_or_else(|| format!("source-index buff {effect_id} is not an array"))?;
        for source in sources {
            let source_id = string_at(source, "sourceId").map(str::to_owned);
            let mut related_effect_ids = BTreeSet::from([effect_id]);
            for key in ["buffIds", "runtimeAliasBuffIds"] {
                for related_effect_id in optional_integer_array_at(source, key)? {
                    related_effect_ids.insert(related_effect_id);
                }
            }

            let mut exact_relationship_skill_ids = BTreeSet::new();
            let mut exact_component_recipient_scopes = BTreeSet::new();
            let mut exact_component_rdps_dispositions = BTreeSet::new();
            let mut components = Vec::new();
            for edge in source
                .get("uidEdges")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
            {
                let uid_kind = string_at(edge, "uidKind").unwrap_or_default();
                let role = string_at(edge, "role").unwrap_or_default();
                if uid_kind.contains("skill")
                    && matches!(role, "owner" | "controller" | "source-output")
                    && let Some(skill_id) = signed_at(edge, "uid")
                {
                    exact_relationship_skill_ids.insert(skill_id);
                }
                if uid_kind == "buff"
                    && matches!(
                        string_at(edge, "edgeKind").unwrap_or_default(),
                        "runtime-buff-alias"
                            | "runtime-buff"
                            | "observed-buff"
                            | "linked-buff"
                            | "description-buff"
                    )
                    && let Some(related_effect_id) = signed_at(edge, "uid")
                {
                    related_effect_ids.insert(related_effect_id);
                }
            }
            for component in source
                .get("attributionModel")
                .and_then(|model| model.get("components"))
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
            {
                if let Some(scope) = string_at(component, "contributionScope") {
                    exact_component_recipient_scopes.insert(scope.to_owned());
                }
                if let Some(eligibility) = string_at(component, "transferEligibility") {
                    exact_component_rdps_dispositions
                        .insert(normalize_source_index_disposition(eligibility).to_owned());
                }
                components.push(component.clone());
            }

            for related_effect_id in related_effect_ids {
                merge_current_owner_evidence(
                    owners
                        .entry(related_effect_id)
                        .or_insert_with(|| unresolved_current_owner_evidence(related_effect_id)),
                    exact_relationship_skill_ids.iter().copied(),
                    exact_component_recipient_scopes.iter().cloned(),
                    exact_component_rdps_dispositions.iter().cloned(),
                    components.iter().cloned(),
                );
                if let Some(source_id) = &source_id {
                    let source_owner = owners_by_source_effect
                        .entry((source_id.clone(), related_effect_id))
                        .or_insert_with(|| unresolved_current_owner_evidence(related_effect_id));
                    merge_current_owner_evidence(
                        source_owner,
                        exact_relationship_skill_ids.iter().copied(),
                        exact_component_recipient_scopes.iter().cloned(),
                        exact_component_rdps_dispositions.iter().cloned(),
                        components.iter().cloned(),
                    );
                    source_owner.owner_evidence_state =
                        if !source_owner.exact_component_recipient_scopes.is_empty() {
                            "exact-source-index-component-route-current-runtime-reproof-required"
                        } else if !source_owner.exact_relationship_skill_ids.is_empty() {
                            "exact-source-index-skill-route-current-runtime-reproof-required"
                        } else {
                            "exact-source-index-source-route-current-runtime-reproof-required"
                        }
                        .to_owned();
                }
            }
        }
    }
    Ok((owners, owners_by_source_effect))
}

fn merge_current_owner_evidence(
    owner: &mut CurrentOwnerEvidence,
    relationship_skill_ids: impl IntoIterator<Item = i64>,
    component_recipient_scopes: impl IntoIterator<Item = String>,
    component_rdps_dispositions: impl IntoIterator<Item = String>,
    components: impl IntoIterator<Item = Value>,
) {
    let mut merged_relationship_skill_ids = owner
        .exact_relationship_skill_ids
        .iter()
        .copied()
        .collect::<BTreeSet<_>>();
    merged_relationship_skill_ids.extend(relationship_skill_ids);
    owner.exact_relationship_skill_ids = merged_relationship_skill_ids.into_iter().collect();

    let mut merged_scopes = owner
        .exact_component_recipient_scopes
        .iter()
        .cloned()
        .collect::<BTreeSet<_>>();
    merged_scopes.extend(component_recipient_scopes);
    owner.exact_component_recipient_scopes = merged_scopes.into_iter().collect();

    let mut merged_dispositions = owner
        .exact_component_rdps_dispositions
        .iter()
        .cloned()
        .collect::<BTreeSet<_>>();
    merged_dispositions.extend(component_rdps_dispositions);
    owner.exact_component_rdps_dispositions = merged_dispositions.into_iter().collect();

    let mut component_keys = owner
        .current_active_modifier_parameter_evidence
        .iter()
        .map(canonical_json_key)
        .collect::<BTreeSet<_>>();
    for component in components {
        if component_keys.insert(canonical_json_key(&component)) {
            owner
                .current_active_modifier_parameter_evidence
                .push(component);
        }
    }

    owner.owner_evidence_state = if !owner.exact_component_recipient_scopes.is_empty() {
        "shared-source-index-component-route-current-runtime-reproof-required"
    } else if !owner.exact_relationship_skill_ids.is_empty() {
        "shared-source-index-skill-route-current-runtime-reproof-required"
    } else {
        "shared-source-index-source-route-current-runtime-reproof-required"
    }
    .to_owned();
}

fn canonical_json_key(value: &Value) -> String {
    serde_json::to_string(value).unwrap_or_else(|_| format!("{value:?}"))
}

fn normalize_source_index_disposition(value: &str) -> &str {
    match value {
        "self-only-formula-context" => "self-only-formula-context-never-support-credit",
        "ordinary-owner-damage" => "ordinary-owner-damage-never-transferred",
        "ordinary-owner-stats" => "ordinary-owner-stats-never-transferred",
        other => other,
    }
}

fn unresolved_current_owner_evidence(effect_id: i64) -> CurrentOwnerEvidence {
    CurrentOwnerEvidence {
        effect_id,
        owner_evidence_state: "not-present-in-current-shared-source-index".to_owned(),
        exact_component_skill_ids: Vec::new(),
        exact_relationship_skill_ids: Vec::new(),
        strong_owner_skill_ids: Vec::new(),
        broad_owner_skill_ids: Vec::new(),
        semantic_owner_skill_ids: Vec::new(),
        semantic_recipient_scopes: Vec::new(),
        semantic_rdps_dispositions: Vec::new(),
        exact_component_recipient_scopes: Vec::new(),
        exact_component_rdps_dispositions: Vec::new(),
        current_active_modifier_parameter_evidence: Vec::new(),
    }
}

fn resolve_transfer_scope(
    declared: &[String],
    current_owner_evidence: &[CurrentOwnerEvidence],
) -> (Vec<String>, &'static str) {
    let exact_component_scope_for_every_effect = !current_owner_evidence.is_empty()
        && current_owner_evidence.iter().all(|evidence| {
            !evidence.exact_component_recipient_scopes.is_empty()
                && !evidence.exact_component_rdps_dispositions.is_empty()
        });
    let every_scope_is_owner_only = exact_component_scope_for_every_effect
        && current_owner_evidence.iter().all(|evidence| {
            evidence
                .exact_component_recipient_scopes
                .iter()
                .all(|scope| exact_component_scope_is_owner_only(scope))
                && evidence
                    .exact_component_rdps_dispositions
                    .iter()
                    .all(|disposition| {
                        disposition.contains("never-transferred")
                            || disposition.contains("never-support-credit")
                    })
        });
    let every_scope_is_external_recipient = exact_component_scope_for_every_effect
        && current_owner_evidence.iter().all(|evidence| {
            evidence
                .exact_component_recipient_scopes
                .iter()
                .all(|scope| exact_component_scope_is_external_recipient(scope))
                && evidence
                    .exact_component_rdps_dispositions
                    .iter()
                    .all(|disposition| {
                        disposition == "exact-attack-and-mattack-counterfactual-only"
                    })
        });

    if every_scope_is_owner_only {
        (
            vec!["self-only-current-component-proof".to_owned()],
            "current-build-exact-component-route-proves-owner-only",
        )
    } else if every_scope_is_external_recipient {
        (
            vec!["external-recipient-candidate".to_owned()],
            "current-build-exact-component-route-proves-external-recipient-candidate",
        )
    } else {
        (
            declared.to_vec(),
            "declared-static-scope-preserved-current-packet-proof-required",
        )
    }
}

fn exact_component_scope_is_external_recipient(scope: &str) -> bool {
    matches!(
        scope,
        "provider-and-external-teammates-in-area"
            | "provider-and-up-to-ten-allies"
            | "provider-and-up-to-ten-nearby-allies"
            | "each-shielded-recipient-triggers-from-that-friendly-attack"
    )
}

fn exact_component_scope_is_owner_only(scope: &str) -> bool {
    matches!(
        scope,
        "summon-caster-only"
            | "summon-owner-only"
            | "provider-only"
            | "provider-only-while-equipped"
            | "provider-owned-summon"
            | "provider-owned-passive-proc"
    )
}

fn proof_counts(
    packet_proof: &Value,
    key: &str,
) -> Result<BTreeMap<i64, (usize, usize)>, Box<dyn Error>> {
    let mut counts = BTreeMap::new();
    for proof in array_at(packet_proof, key)? {
        let effect_id = proof
            .get("fingerprint")
            .and_then(|value| signed_at(value, "effect_id"))
            .ok_or_else(|| format!("{key} fingerprint.effect_id is missing"))?;
        let entry = counts.entry(effect_id).or_insert((0usize, 0usize));
        entry.0 += 1;
        if bool_at(proof, "runtime_eligible_for_rdps") {
            entry.1 += 1;
        }
    }
    Ok(counts)
}

fn equation_counts(packet_proof: &Value) -> Result<BTreeMap<i64, u64>, Box<dyn Error>> {
    let mut counts = BTreeMap::new();
    for system in array_at(packet_proof, "wire_additive_equation_systems")? {
        for equation in array_at(system, "equations")? {
            let occurrences = unsigned_at(equation, "count");
            for term in array_at(equation, "terms")? {
                if let Some(effect_id) = signed_at(term, "effect_id") {
                    *counts.entry(effect_id).or_default() += occurrences;
                }
            }
        }
    }
    Ok(counts)
}

fn classify_candidate(
    observations: &[EffectObservation],
    has_retained_historical_proof: bool,
) -> &'static str {
    if observations.is_empty() {
        return "blocked-missing-effect-identity";
    }
    if has_retained_historical_proof {
        return "retained-historical-coefficient-proof-requires-current-build-reproof";
    }
    if observations.iter().all(|effect| effect.status_events == 0) {
        return "historical-corpus-no-effect-observation";
    }
    if observations.iter().any(|effect| {
        effect.reversible_static_coefficient_proofs > 0
            || effect.matched_lifecycle_coefficient_proofs > 0
    }) {
        return "historical-coefficient-evidence-requires-current-build-reproof";
    }
    if observations.iter().any(|effect| {
        effect.same_wire_attribute_delta_observations > 0
            || effect.binary_presence_equation_occurrences > 0
            || effect.isolated_attribute_pairs > 0
    }) {
        return "historical-observation-without-reversible-coefficient-proof";
    }
    "historical-observation-without-exact-attribute-delta"
}

fn has_issue(finding: &Value, category: &str) -> bool {
    finding
        .get("issues")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .any(|issue| string_at(issue, "category") == Some(category))
}

fn parse_options(arguments: &[String]) -> Result<BTreeMap<String, String>, Box<dyn Error>> {
    let mut options = BTreeMap::new();
    let mut index = 0usize;
    while index < arguments.len() {
        let key = arguments[index]
            .strip_prefix("--")
            .ok_or_else(|| format!("unexpected positional argument {}", arguments[index]))?;
        let value = arguments
            .get(index + 1)
            .ok_or_else(|| format!("--{key} requires a value"))?;
        if options.insert(key.to_owned(), value.to_owned()).is_some() {
            return Err(format!("--{key} was supplied more than once").into());
        }
        index += 2;
    }
    Ok(options)
}

fn required<'a>(
    options: &'a BTreeMap<String, String>,
    key: &str,
) -> Result<&'a str, Box<dyn Error>> {
    options
        .get(key)
        .map(String::as_str)
        .ok_or_else(|| format!("missing required --{key}").into())
}

fn required_path(options: &BTreeMap<String, String>, key: &str) -> Result<PathBuf, Box<dyn Error>> {
    Ok(PathBuf::from(required(options, key)?))
}

fn validate_build(build: &str) -> Result<(), Box<dyn Error>> {
    if build.is_empty() || !build.bytes().all(|byte| byte.is_ascii_digit()) {
        return Err("build identifiers must contain only ASCII digits".into());
    }
    Ok(())
}

fn read_json(path: &Path) -> Result<Value, Box<dyn Error>> {
    Ok(serde_json::from_reader(BufReader::new(File::open(path)?))?)
}

fn require_generated_by(value: &Value, expected: &str) -> Result<(), Box<dyn Error>> {
    if string_at(value, "generated_by") != Some(expected) {
        return Err(format!("expected {expected} input").into());
    }
    Ok(())
}

fn array_at<'a>(value: &'a Value, key: &str) -> Result<&'a [Value], Box<dyn Error>> {
    value
        .get(key)
        .and_then(Value::as_array)
        .map(Vec::as_slice)
        .ok_or_else(|| format!("{key} array is missing").into())
}

fn string_at<'a>(value: &'a Value, key: &str) -> Option<&'a str> {
    value.get(key).and_then(Value::as_str)
}

fn optional_string_at(value: &Value, key: &str) -> Option<String> {
    string_at(value, key).map(str::to_owned)
}

fn string_array_at(value: &Value, key: &str) -> Result<Vec<String>, Box<dyn Error>> {
    array_at(value, key)?
        .iter()
        .map(|entry| {
            entry
                .as_str()
                .map(str::to_owned)
                .ok_or_else(|| format!("{key} contains a non-string value").into())
        })
        .collect()
}

fn integer_array_at(value: &Value, key: &str) -> Result<Vec<i64>, Box<dyn Error>> {
    array_at(value, key)?
        .iter()
        .map(|entry| {
            entry
                .as_i64()
                .ok_or_else(|| format!("{key} contains a non-integer value").into())
        })
        .collect()
}

fn optional_integer_array_at(value: &Value, key: &str) -> Result<Vec<i64>, Box<dyn Error>> {
    match value.get(key) {
        None => Ok(Vec::new()),
        Some(raw) => raw
            .as_array()
            .ok_or_else(|| -> Box<dyn Error> { format!("{key} is not an array").into() })?
            .iter()
            .map(|entry| {
                entry
                    .as_i64()
                    .ok_or_else(|| format!("{key} contains a non-integer value").into())
            })
            .collect(),
    }
}

fn lifecycle_effects_at(value: &Value) -> Result<Vec<LifecycleEffect>, Box<dyn Error>> {
    array_at(value, "lifecycle_effects")?
        .iter()
        .map(|effect| {
            Ok(LifecycleEffect {
                effect_id: signed_at(effect, "effect_id")
                    .ok_or("lifecycle effect_id is missing")?,
                name: optional_string_at(effect, "name"),
                icon: optional_string_at(effect, "icon"),
                repeat_add_rule: integer_array_at(effect, "repeat_add_rule")?,
                declared_max_stacks: signed_at(effect, "declared_max_stacks")
                    .ok_or("lifecycle declared_max_stacks is missing")?,
                proof_model: string_at(effect, "proof_model")
                    .ok_or("lifecycle proof_model is missing")?
                    .to_owned(),
                destroy_param: effect
                    .get("destroy_param")
                    .cloned()
                    .ok_or("lifecycle destroy_param is missing")?,
            })
        })
        .collect()
}

fn signed_at(value: &Value, key: &str) -> Option<i64> {
    value.get(key).and_then(Value::as_i64)
}

fn unsigned_at(value: &Value, key: &str) -> u64 {
    value.get(key).and_then(Value::as_u64).unwrap_or_default()
}

fn bool_at(value: &Value, key: &str) -> bool {
    value.get(key).and_then(Value::as_bool).unwrap_or(false)
}

fn display_path(path: &Path) -> String {
    path.to_string_lossy().replace('\\', "/")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn gap(source_rule_id: &str, effect_id: i64, attribute_id: i64) -> CandidateGap {
        CandidateGap {
            source_rule_id: source_rule_id.to_owned(),
            source_id: None,
            source_name: None,
            formula_term_ids: vec!["primaryAttack".to_owned()],
            transfer_eligibilities: vec!["recipient-scope-unresolved".to_owned()],
            effective_transfer_eligibilities: vec!["recipient-scope-unresolved".to_owned()],
            transfer_scope_resolution: "declared-static-scope-preserved-current-packet-proof-required",
            effect_ids: vec![effect_id],
            declared_effect_references: vec![effect_id],
            rejected_effect_references: vec![],
            selected_attribute_ids: vec![attribute_id],
            lifecycle_effects: vec![],
            required_runtime_evidence: vec![],
            static_blockers: vec!["missing value proof".to_owned()],
            current_owner_evidence: vec![CurrentOwnerEvidence {
                effect_id,
                owner_evidence_state: "no-current-owner-candidate".to_owned(),
                exact_component_skill_ids: vec![],
                exact_relationship_skill_ids: vec![],
                strong_owner_skill_ids: vec![],
                broad_owner_skill_ids: vec![],
                semantic_owner_skill_ids: vec![],
                semantic_recipient_scopes: vec![],
                semantic_rdps_dispositions: vec![],
                exact_component_recipient_scopes: vec![],
                exact_component_rdps_dispositions: vec![],
                current_active_modifier_parameter_evidence: vec![],
            }],
            historical_packet_observations: vec![],
            retained_historical_proofs: vec![],
            outcome: "historical-corpus-no-effect-observation",
            current_build_promotion_eligible: false,
            remaining_requirement: "current-build replay",
        }
    }

    fn observation(effect_id: i64, status_events: u64) -> EffectObservation {
        EffectObservation {
            effect_id,
            status_events,
            ..EffectObservation::default()
        }
    }

    #[test]
    fn source_index_maps_runtime_child_effects_to_the_same_exact_source() {
        let source_index = serde_json::json!({
            "schemaVersion": 1,
            "byBuffId": {
                "2208420": [{
                    "buffIds": [2208420, 2208421],
                    "runtimeAliasBuffIds": [2208420, 2208421],
                    "uidEdges": [
                        {
                            "edgeKind": "runtime-buff-alias",
                            "uidKind": "buff",
                            "uid": "2208421",
                            "role": "stack"
                        },
                        {
                            "edgeKind": "owner-skill",
                            "uidKind": "skill",
                            "uid": 220842,
                            "role": "owner"
                        }
                    ],
                    "attributionModel": {
                        "components": [{
                            "componentKey": "stellar-spark-fire-attack",
                            "contributionScope": "owner",
                            "transferEligibility": "self-only-formula-context"
                        }]
                    }
                }]
            }
        });

        let owners = collect_source_index_owner_evidence(&source_index).unwrap();
        for effect_id in [2_208_420, 2_208_421] {
            let owner = owners.get(&effect_id).unwrap();
            assert_eq!(owner.exact_relationship_skill_ids, vec![220_842]);
            assert_eq!(
                owner.exact_component_recipient_scopes,
                vec!["owner".to_owned()]
            );
            assert_eq!(
                owner.exact_component_rdps_dispositions,
                vec!["self-only-formula-context-never-support-credit".to_owned()]
            );
            assert_eq!(owner.current_active_modifier_parameter_evidence.len(), 1);
        }
    }

    #[test]
    fn source_index_merges_multiple_exact_sources_for_one_effect() {
        let source_index = serde_json::json!({
            "schemaVersion": 1,
            "byBuffId": {
                "42": [{
                    "buffIds": [42],
                    "uidEdges": [{
                        "edgeKind": "owner-skill",
                        "uidKind": "skill",
                        "uid": 100,
                        "role": "owner"
                    }]
                }],
                "43": [{
                    "buffIds": [43, 42],
                    "uidEdges": [{
                        "edgeKind": "owner-skill",
                        "uidKind": "skill",
                        "uid": 200,
                        "role": "owner"
                    }]
                }]
            }
        });

        let owners = collect_source_index_owner_evidence(&source_index).unwrap();
        assert_eq!(
            owners.get(&42).unwrap().exact_relationship_skill_ids,
            vec![100, 200]
        );
    }

    #[test]
    fn source_index_keeps_formula_components_scoped_to_the_exact_source() {
        let source_index = serde_json::json!({
            "schemaVersion": 1,
            "byBuffId": {
                "42": [{
                    "sourceId": "equipment-set:101:2:variant:464",
                    "buffIds": [42],
                    "attributionModel": {
                        "components": [{
                            "componentKey": "equipment-set-attribute:4001:464:42:owner-stat",
                            "contributionScope": "owner",
                            "transferEligibility": "self-only-formula-context"
                        }]
                    }
                }, {
                    "sourceId": "equipment-set:102:2:variant:1786",
                    "buffIds": [42],
                    "attributionModel": {
                        "components": [{
                            "componentKey": "equipment-set-attribute:4002:1786:42:owner-stat",
                            "contributionScope": "owner",
                            "transferEligibility": "self-only-formula-context"
                        }]
                    }
                }]
            }
        });

        let (owners, owners_by_source_effect) =
            collect_source_index_owner_evidence_sets(&source_index).unwrap();
        assert_eq!(
            owners
                .get(&42)
                .unwrap()
                .current_active_modifier_parameter_evidence
                .len(),
            2
        );

        let first = owners_by_source_effect
            .get(&("equipment-set:101:2:variant:464".to_owned(), 42))
            .unwrap();
        assert_eq!(first.current_active_modifier_parameter_evidence.len(), 1);
        assert_eq!(
            string_at(
                &first.current_active_modifier_parameter_evidence[0],
                "componentKey"
            ),
            Some("equipment-set-attribute:4001:464:42:owner-stat")
        );

        let second = owners_by_source_effect
            .get(&("equipment-set:102:2:variant:1786".to_owned(), 42))
            .unwrap();
        assert_eq!(second.current_active_modifier_parameter_evidence.len(), 1);
        assert_eq!(
            string_at(
                &second.current_active_modifier_parameter_evidence[0],
                "componentKey"
            ),
            Some("equipment-set-attribute:4002:1786:42:owner-stat")
        );
    }

    #[test]
    fn exact_current_owner_only_component_overrides_stale_unresolved_scope() {
        let evidence = CurrentOwnerEvidence {
            effect_id: 2_110_110,
            owner_evidence_state: "exact-component-route-current-runtime-reproof-required"
                .to_owned(),
            exact_component_skill_ids: vec![3_937],
            exact_relationship_skill_ids: vec![],
            strong_owner_skill_ids: vec![],
            broad_owner_skill_ids: vec![],
            semantic_owner_skill_ids: vec![],
            semantic_recipient_scopes: vec![],
            semantic_rdps_dispositions: vec![],
            exact_component_recipient_scopes: vec!["summon-caster-only".to_owned()],
            exact_component_rdps_dispositions: vec![
                "ordinary-owner-damage-never-transferred".to_owned(),
            ],
            current_active_modifier_parameter_evidence: vec![],
        };
        let (effective, resolution) =
            resolve_transfer_scope(&["recipient-scope-unresolved".to_owned()], &[evidence]);
        assert_eq!(effective, vec!["self-only-current-component-proof"]);
        assert_eq!(
            resolution,
            "current-build-exact-component-route-proves-owner-only"
        );
    }

    #[test]
    fn summon_owner_only_is_exact_self_only_scope() {
        let evidence = CurrentOwnerEvidence {
            effect_id: 2_110_138,
            owner_evidence_state: "exact-component-route-current-runtime-reproof-required"
                .to_owned(),
            exact_component_skill_ids: vec![3_969],
            exact_relationship_skill_ids: vec![],
            strong_owner_skill_ids: vec![],
            broad_owner_skill_ids: vec![],
            semantic_owner_skill_ids: vec![],
            semantic_recipient_scopes: vec![],
            semantic_rdps_dispositions: vec![],
            exact_component_recipient_scopes: vec!["summon-owner-only".to_owned()],
            exact_component_rdps_dispositions: vec![
                "ordinary-owner-damage-never-transferred".to_owned(),
            ],
            current_active_modifier_parameter_evidence: vec![],
        };
        let (effective, resolution) =
            resolve_transfer_scope(&["recipient-scope-unresolved".to_owned()], &[evidence]);
        assert_eq!(effective, vec!["self-only-current-component-proof"]);
        assert_eq!(
            resolution,
            "current-build-exact-component-route-proves-owner-only"
        );
    }

    #[test]
    fn exact_external_component_routes_to_external_recipient_proof_queue() {
        let evidence = CurrentOwnerEvidence {
            effect_id: 2_110_143,
            owner_evidence_state: "exact-component-route-current-runtime-reproof-required"
                .to_owned(),
            exact_component_skill_ids: vec![3_974],
            exact_relationship_skill_ids: vec![],
            strong_owner_skill_ids: vec![],
            broad_owner_skill_ids: vec![],
            semantic_owner_skill_ids: vec![],
            semantic_recipient_scopes: vec![],
            semantic_rdps_dispositions: vec![],
            exact_component_recipient_scopes: vec![
                "provider-and-external-teammates-in-area".to_owned(),
            ],
            exact_component_rdps_dispositions: vec![
                "exact-attack-and-mattack-counterfactual-only".to_owned(),
            ],
            current_active_modifier_parameter_evidence: vec![],
        };
        let (effective, resolution) =
            resolve_transfer_scope(&["recipient-scope-unresolved".to_owned()], &[evidence]);
        assert_eq!(effective, vec!["external-recipient-candidate"]);
        assert_eq!(
            resolution,
            "current-build-exact-component-route-proves-external-recipient-candidate"
        );
    }

    #[test]
    fn mixed_exact_component_scopes_preserve_declared_scope() {
        let owner_only = CurrentOwnerEvidence {
            effect_id: 1,
            owner_evidence_state: "exact".to_owned(),
            exact_component_skill_ids: vec![1],
            exact_relationship_skill_ids: vec![],
            strong_owner_skill_ids: vec![],
            broad_owner_skill_ids: vec![],
            semantic_owner_skill_ids: vec![],
            semantic_recipient_scopes: vec![],
            semantic_rdps_dispositions: vec![],
            exact_component_recipient_scopes: vec!["summon-owner-only".to_owned()],
            exact_component_rdps_dispositions: vec![
                "ordinary-owner-damage-never-transferred".to_owned(),
            ],
            current_active_modifier_parameter_evidence: vec![],
        };
        let external = CurrentOwnerEvidence {
            effect_id: 2,
            owner_evidence_state: "exact".to_owned(),
            exact_component_skill_ids: vec![2],
            exact_relationship_skill_ids: vec![],
            strong_owner_skill_ids: vec![],
            broad_owner_skill_ids: vec![],
            semantic_owner_skill_ids: vec![],
            semantic_recipient_scopes: vec![],
            semantic_rdps_dispositions: vec![],
            exact_component_recipient_scopes: vec![
                "provider-and-external-teammates-in-area".to_owned(),
            ],
            exact_component_rdps_dispositions: vec![
                "exact-attack-and-mattack-counterfactual-only".to_owned(),
            ],
            current_active_modifier_parameter_evidence: vec![],
        };
        let declared = vec!["recipient-scope-unresolved".to_owned()];
        let (effective, resolution) = resolve_transfer_scope(&declared, &[owner_only, external]);
        assert_eq!(effective, declared);
        assert_eq!(
            resolution,
            "declared-static-scope-preserved-current-packet-proof-required"
        );
    }

    #[test]
    fn incomplete_component_evidence_preserves_declared_scope() {
        let evidence = CurrentOwnerEvidence {
            effect_id: 42,
            owner_evidence_state: "candidate".to_owned(),
            exact_component_skill_ids: vec![],
            exact_relationship_skill_ids: vec![],
            strong_owner_skill_ids: vec![],
            broad_owner_skill_ids: vec![],
            semantic_owner_skill_ids: vec![],
            semantic_recipient_scopes: vec![],
            semantic_rdps_dispositions: vec![],
            exact_component_recipient_scopes: vec![],
            exact_component_rdps_dispositions: vec![],
            current_active_modifier_parameter_evidence: vec![],
        };
        let declared = vec!["recipient-scope-unresolved".to_owned()];
        let (effective, resolution) = resolve_transfer_scope(&declared, &[evidence]);
        assert_eq!(effective, declared);
        assert_eq!(
            resolution,
            "declared-static-scope-preserved-current-packet-proof-required"
        );
    }

    #[test]
    fn absence_remains_a_historical_coverage_gap() {
        assert_eq!(
            classify_candidate(&[observation(42, 0)], false),
            "historical-corpus-no-effect-observation"
        );
    }

    #[test]
    fn observation_without_wire_delta_does_not_become_a_coefficient() {
        assert_eq!(
            classify_candidate(&[observation(42, 3)], false),
            "historical-observation-without-exact-attribute-delta"
        );
    }

    #[test]
    fn historical_proof_still_requires_current_build_reproof() {
        let mut value = observation(42, 4);
        value.reversible_static_coefficient_proofs = 1;
        value.historical_runtime_eligible_proofs = 1;
        assert_eq!(
            classify_candidate(&[value], false),
            "historical-coefficient-evidence-requires-current-build-reproof"
        );
    }

    #[test]
    fn separately_retained_proof_is_visible_but_not_promoted() {
        assert_eq!(
            classify_candidate(&[observation(42, 0)], true),
            "retained-historical-coefficient-proof-requires-current-build-reproof"
        );
    }

    #[test]
    fn current_owner_inventory_preserves_exact_and_candidate_strengths() {
        let ledger = json!({
            "legacy_formula_gap_effects": [
                {
                    "effect_id": 2110138,
                    "owner_evidence_state": "exact-component-route-current-runtime-reproof-required",
                    "current_exact_component_skill_ids": [3969],
                    "current_exact_relationship_skill_ids": [],
                    "current_strong_owner_skill_ids": [],
                    "current_broad_owner_skill_ids": [],
                    "current_semantic_owner_skill_ids": [],
                    "current_semantic_recipient_scopes": [],
                    "current_semantic_rdps_dispositions": [],
                    "current_exact_component_recipient_scopes": ["summon-caster-only"],
                    "current_exact_component_rdps_dispositions": ["ordinary-owner-damage-never-transferred"],
                    "current_active_modifier_parameter_evidence": [{
                        "skill_effect_id": 396901,
                        "runtime_authority": false
                    }]
                },
                {
                    "effect_id": 2110102,
                    "owner_evidence_state": "broad-design-prefix-candidate-not-formula-authority",
                    "current_exact_component_skill_ids": [],
                    "current_exact_relationship_skill_ids": [],
                    "current_strong_owner_skill_ids": [],
                    "current_broad_owner_skill_ids": [3926],
                    "current_semantic_owner_skill_ids": [],
                    "current_semantic_recipient_scopes": [],
                    "current_semantic_rdps_dispositions": [],
                    "current_exact_component_recipient_scopes": [],
                    "current_exact_component_rdps_dispositions": [],
                    "current_active_modifier_parameter_evidence": []
                },
                {
                    "effect_id": 2110126,
                    "owner_evidence_state": "unique-semantic-duration-candidate-not-numeric-owner-edge",
                    "current_exact_component_skill_ids": [],
                    "current_exact_relationship_skill_ids": [],
                    "current_strong_owner_skill_ids": [],
                    "current_broad_owner_skill_ids": [],
                    "current_semantic_owner_skill_ids": [3958],
                    "current_semantic_recipient_scopes": ["summon-caster-only-per-current-localized-description"],
                    "current_semantic_rdps_dispositions": ["ordinary-owner-stats-never-transferred"],
                    "current_exact_component_recipient_scopes": [],
                    "current_exact_component_rdps_dispositions": [],
                    "current_active_modifier_parameter_evidence": []
                }
            ]
        });

        let owners = collect_current_owner_evidence(&ledger).unwrap();
        assert_eq!(owners[&2_110_138].exact_component_skill_ids, vec![3_969]);
        assert_eq!(
            owners[&2_110_138].current_active_modifier_parameter_evidence,
            vec![json!({
                "skill_effect_id": 396901,
                "runtime_authority": false
            })]
        );
        assert_eq!(owners[&2_110_102].broad_owner_skill_ids, vec![3_926]);
        assert_eq!(owners[&2_110_126].semantic_owner_skill_ids, vec![3_958]);
        assert_eq!(
            owners[&2_110_126].semantic_recipient_scopes,
            vec!["summon-caster-only-per-current-localized-description"]
        );
        assert_eq!(
            owners[&2_110_126].semantic_rdps_dispositions,
            vec!["ordinary-owner-stats-never-transferred"]
        );
    }

    #[test]
    fn gap_watchlist_keeps_gap_rules_and_all_missing_value_proofs() {
        let source = json!({
            "schema_version": 3,
            "generated_by": "rlogs-bpsr-static-rdps-worklist",
            "promotion_state": "packet-proof-required-runtime-disabled",
            "selected_effect_ids": [42, 99],
            "reported_effect_ids": [42, 99],
            "selected_attribute_ids": [100, 200, 11310, 20010],
            "non_attributable_context_attribute_ids": [11310, 20010],
            "stateful_attribute_ids": [11310, 20010],
            "candidates": [
                {
                    "source_rule_id": "keep",
                    "static_value_state": "missing-value-proof",
                    "effect_ids": [42],
                    "selected_attribute_ids": [100]
                },
                {
                    "source_rule_id": "also-missing",
                    "static_value_state": "missing-value-proof",
                    "effect_ids": [77],
                    "selected_attribute_ids": [150]
                },
                {
                    "source_rule_id": "drop",
                    "static_value_state": "selected-values-present",
                    "effect_ids": [99],
                    "selected_attribute_ids": [200]
                }
            ]
        });

        let reduced = build_gap_watchlist(&source, &[gap("keep", 42, 100)]).unwrap();

        assert_eq!(reduced["selected_effect_ids"], json!([42, 77]));
        assert_eq!(reduced["reported_effect_ids"], json!([42, 77]));
        assert_eq!(
            reduced["selected_attribute_ids"],
            json!([100, 150, 11310, 20010])
        );
        assert_eq!(reduced["candidates"].as_array().unwrap().len(), 2);
        assert_eq!(
            reduced["selection_summary"],
            json!({
                "ledger_formula_gap_rules": 1,
                "missing_value_proof_rules": 2,
                "selected_unresolved_formula_rules": 2
            })
        );
        assert_eq!(
            reduced["generated_by"],
            json!("rlogs-bpsr-rdps-formula-gap-ledger")
        );
    }

    #[test]
    fn gap_watchlist_refuses_to_drop_a_missing_rule() {
        let source = json!({
            "non_attributable_context_attribute_ids": [11310],
            "candidates": [{"source_rule_id": "different"}]
        });

        let error = build_gap_watchlist(&source, &[gap("missing", 42, 100)]).unwrap_err();
        assert!(error.to_string().contains("missing source rules: missing"));
    }
}
