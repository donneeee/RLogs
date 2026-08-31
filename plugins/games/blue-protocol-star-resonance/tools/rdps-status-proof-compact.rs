use std::{
    env,
    ffi::OsString,
    fs::File,
    io::{BufReader, BufWriter},
    path::PathBuf,
};

use serde::{Deserialize, Serialize};
use serde_json::Value;

const SCHEMA_VERSION: u16 = 1;

#[derive(Debug, Deserialize)]
struct AuditInput {
    schema_version: u16,
    selected_effect_ids: Vec<i64>,
    reported_effect_ids: Vec<i64>,
    selected_attribute_ids: Vec<i32>,
    sessions: Vec<SessionSummary>,
    effects: Vec<EffectInput>,
    wire_additive_equation_systems: Vec<WireAttributeInput>,
    reversible_static_coefficient_proofs: Vec<StaticProof>,
    matched_lifecycle_coefficient_proofs: Vec<LifecycleProof>,
}

#[derive(Debug, Deserialize, Serialize)]
struct SessionSummary {
    rlog: String,
    session_id: String,
    run_ordinals_observed: u32,
    actor_events: u64,
    attribute_events: u64,
    decoded_selected_attribute_values: u64,
    undecodable_selected_attribute_values: u64,
    all_status_events: u64,
    selected_status_events: u64,
}

#[derive(Debug, Deserialize)]
struct EffectInput {
    effect_id: i64,
    selected_status_events: u64,
    selected_mechanic_state_changes: u64,
    attributes: Vec<AttributeInput>,
    percent_family_formulas: Vec<PercentFormulaInput>,
}

#[derive(Debug, Deserialize)]
struct AttributeInput {
    attribute_id: i32,
    transitions_examined: u64,
    complete_before_and_after: u64,
    missing_before: u64,
    missing_after_within_window: u64,
    isolated_transitions: u64,
    transitions_with_competing_target_statuses: u64,
    aggregates: Vec<TransitionAggregate>,
}

#[derive(Debug, Deserialize, Serialize)]
struct TransitionAggregate {
    state: String,
    raw_delta_units: i64,
    isolated: bool,
    provider_resolution: String,
    provider_kind: Option<String>,
    provider_class_id: Option<i32>,
    provider_specialization_id: Option<i32>,
    provider_is_target: Option<bool>,
    same_wire_attribute_update: bool,
    count: u64,
}

#[derive(Debug, Deserialize, Serialize)]
struct PercentFormulaInput {
    family: String,
    final_attribute_id: i32,
    intermediate_attribute_id: i32,
    base_attribute_id: i32,
    raw_extra_add_attribute_id: i32,
    raw_percent_attribute_id: i32,
    raw_extra_percent_attribute_id: i32,
    scale: i64,
    transitions_examined: u64,
    transitions_with_exact_wire_inputs: u64,
    intermediate_exact_delta_matches: u64,
    intermediate_residual_mismatches: u64,
    nearest_intermediate_exact_delta_matches: u64,
    nearest_intermediate_residual_mismatches: u64,
    final_transitions_with_known_extra_percent: u64,
    final_exact_delta_matches: u64,
    final_residual_mismatches: u64,
    final_transitions_with_unknown_extra_percent: u64,
    transitions_with_changed_base: u64,
}

#[derive(Debug, Deserialize)]
struct WireAttributeInput {
    attribute_id: i32,
    wire_messages_with_attribute_update: u64,
    binary_presence_equations: u64,
    equations_containing_reported_effect: u64,
    excluded_nonbinary_mechanic_equations: u64,
    unique_equations: usize,
    conflicting_term_sets: usize,
    equations: Vec<WireEquation>,
}

#[derive(Debug, Deserialize, Serialize)]
struct WireEquation {
    terms: Vec<WireTerm>,
    raw_attribute_delta: i64,
    count: u64,
    independent_run_contexts: usize,
    target_entity_count: usize,
    source_entity_count: usize,
    cross_actor_occurrences: u64,
    self_source_occurrences: u64,
    missing_source_occurrences: u64,
}

#[derive(Debug, Deserialize, Serialize)]
struct WireTerm {
    effect_id: i64,
    origin: Option<Value>,
    level: Option<i32>,
    part_id: Option<i32>,
    stacks: Option<u32>,
    count: Option<i32>,
    signed_presence_delta: i8,
}

#[derive(Debug, Deserialize, Serialize)]
struct StaticProof {
    attribute_id: i32,
    fingerprint: Fingerprint,
    status: String,
    proven_coefficient_units: Option<i64>,
    normalized_coefficient_counts: Value,
    apply_occurrences: u64,
    remove_occurrences: u64,
    independent_run_contexts: usize,
    target_entity_count: usize,
    source_entity_count: usize,
    cross_actor_occurrences: u64,
    self_source_occurrences: u64,
    missing_source_occurrences: u64,
    source_equations: u64,
    runtime_eligible_for_rdps: bool,
    blocker: Option<String>,
}

#[derive(Debug, Deserialize, Serialize)]
struct LifecycleProof {
    attribute_id: i32,
    fingerprint: Fingerprint,
    status: String,
    proven_coefficient_units: Option<i64>,
    exact_coefficient_counts: Value,
    exact_pair_count: u64,
    contradictory_pair_count: u64,
    ambiguous_instance_count: u64,
    application_only_instance_count: u64,
    removal_only_instance_count: u64,
    independent_run_contexts: usize,
    target_entity_count: usize,
    source_entity_count: usize,
    cross_actor_exact_pairs: u64,
    runtime_eligible_for_rdps: bool,
    blocker: Option<String>,
}

#[derive(Debug, Deserialize, Serialize)]
struct Fingerprint {
    effect_id: i64,
    origin: Option<Value>,
    level: Option<i32>,
    part_id: Option<i32>,
    stacks: Option<u32>,
    count: Option<i32>,
}

#[derive(Debug, Serialize)]
struct CompactAudit {
    schema_version: u16,
    source_schema_version: u16,
    generated_by: &'static str,
    policy: CompactPolicy,
    selected_effect_count: usize,
    reported_effect_count: usize,
    selected_attribute_count: usize,
    sessions: Vec<SessionSummary>,
    effects: Vec<EffectOutput>,
    wire_additive_equation_systems: Vec<WireAttributeOutput>,
    reversible_static_coefficient_proofs: Vec<StaticProof>,
    matched_lifecycle_coefficient_proofs: Vec<LifecycleProof>,
}

#[derive(Debug, Serialize)]
struct CompactPolicy {
    runtime_use: &'static str,
    source_examples_retained_in_full_ledger: bool,
    unresolved_evidence_is_hidden: bool,
    zero_evidence_rows_omitted: bool,
}

#[derive(Debug, Serialize)]
struct EffectOutput {
    effect_id: i64,
    selected_status_events: u64,
    selected_mechanic_state_changes: u64,
    attributes: Vec<AttributeOutput>,
    percent_family_formulas: Vec<PercentFormulaInput>,
}

#[derive(Debug, Serialize)]
struct AttributeOutput {
    attribute_id: i32,
    transitions_examined: u64,
    complete_before_and_after: u64,
    missing_before: u64,
    missing_after_within_window: u64,
    isolated_transitions: u64,
    transitions_with_competing_target_statuses: u64,
    aggregates: Vec<TransitionAggregate>,
}

#[derive(Debug, Serialize)]
struct WireAttributeOutput {
    attribute_id: i32,
    wire_messages_with_attribute_update: u64,
    binary_presence_equations: u64,
    equations_containing_reported_effect: u64,
    excluded_nonbinary_mechanic_equations: u64,
    unique_equations: usize,
    conflicting_term_sets: usize,
    equations: Vec<WireEquation>,
}

fn main() -> Result<(), String> {
    let mut args = env::args_os().skip(1);
    let input_path = required_path(&mut args, "input proof JSON")?;
    let output_path = required_path(&mut args, "output compact JSON")?;
    if args.next().is_some() {
        return Err(
            "usage: rlogs-bpsr-rdps-status-proof-compact <input.json> <output.json>".into(),
        );
    }

    let input = File::open(&input_path)
        .map_err(|error| format!("failed to open {}: {error}", input_path.display()))?;
    let audit: AuditInput = serde_json::from_reader(BufReader::new(input))
        .map_err(|error| format!("failed to decode {}: {error}", input_path.display()))?;

    let effects = audit
        .effects
        .into_iter()
        .map(compact_effect)
        .filter(|effect| {
            effect.selected_status_events > 0
                || effect.selected_mechanic_state_changes > 0
                || !effect.attributes.is_empty()
                || !effect.percent_family_formulas.is_empty()
        })
        .collect();
    let wire_additive_equation_systems = audit
        .wire_additive_equation_systems
        .into_iter()
        .filter(|report| {
            report.wire_messages_with_attribute_update > 0
                || report.binary_presence_equations > 0
                || !report.equations.is_empty()
        })
        .map(|report| WireAttributeOutput {
            attribute_id: report.attribute_id,
            wire_messages_with_attribute_update: report.wire_messages_with_attribute_update,
            binary_presence_equations: report.binary_presence_equations,
            equations_containing_reported_effect: report.equations_containing_reported_effect,
            excluded_nonbinary_mechanic_equations: report.excluded_nonbinary_mechanic_equations,
            unique_equations: report.unique_equations,
            conflicting_term_sets: report.conflicting_term_sets,
            equations: report.equations,
        })
        .collect();

    let compact = CompactAudit {
        schema_version: SCHEMA_VERSION,
        source_schema_version: audit.schema_version,
        generated_by: "rlogs-bpsr-rdps-status-proof-compact",
        policy: CompactPolicy {
            runtime_use: "offline_review_only_not_loaded_by_live_parser",
            source_examples_retained_in_full_ledger: true,
            unresolved_evidence_is_hidden: false,
            zero_evidence_rows_omitted: true,
        },
        selected_effect_count: audit.selected_effect_ids.len(),
        reported_effect_count: audit.reported_effect_ids.len(),
        selected_attribute_count: audit.selected_attribute_ids.len(),
        sessions: audit.sessions,
        effects,
        wire_additive_equation_systems,
        reversible_static_coefficient_proofs: audit.reversible_static_coefficient_proofs,
        matched_lifecycle_coefficient_proofs: audit.matched_lifecycle_coefficient_proofs,
    };

    let output = File::create(&output_path)
        .map_err(|error| format!("failed to create {}: {error}", output_path.display()))?;
    serde_json::to_writer_pretty(BufWriter::new(output), &compact)
        .map_err(|error| format!("failed to write {}: {error}", output_path.display()))?;
    Ok(())
}

fn compact_effect(effect: EffectInput) -> EffectOutput {
    let attributes = effect
        .attributes
        .into_iter()
        .filter(|attribute| {
            attribute.transitions_examined > 0
                || attribute.complete_before_and_after > 0
                || attribute.missing_before > 0
                || attribute.missing_after_within_window > 0
                || attribute.isolated_transitions > 0
                || attribute.transitions_with_competing_target_statuses > 0
                || !attribute.aggregates.is_empty()
        })
        .map(|attribute| AttributeOutput {
            attribute_id: attribute.attribute_id,
            transitions_examined: attribute.transitions_examined,
            complete_before_and_after: attribute.complete_before_and_after,
            missing_before: attribute.missing_before,
            missing_after_within_window: attribute.missing_after_within_window,
            isolated_transitions: attribute.isolated_transitions,
            transitions_with_competing_target_statuses: attribute
                .transitions_with_competing_target_statuses,
            aggregates: attribute.aggregates,
        })
        .collect();
    let percent_family_formulas = effect
        .percent_family_formulas
        .into_iter()
        .filter(|formula| formula.transitions_examined > 0)
        .collect();
    EffectOutput {
        effect_id: effect.effect_id,
        selected_status_events: effect.selected_status_events,
        selected_mechanic_state_changes: effect.selected_mechanic_state_changes,
        attributes,
        percent_family_formulas,
    }
}

fn required_path(
    args: &mut impl Iterator<Item = OsString>,
    label: &str,
) -> Result<PathBuf, String> {
    args.next()
        .map(PathBuf::from)
        .ok_or_else(|| format!("missing {label}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn drops_only_zero_evidence_attribute_rows() {
        let effect = compact_effect(EffectInput {
            effect_id: 7,
            selected_status_events: 1,
            selected_mechanic_state_changes: 0,
            attributes: vec![
                AttributeInput {
                    attribute_id: 10,
                    transitions_examined: 0,
                    complete_before_and_after: 0,
                    missing_before: 0,
                    missing_after_within_window: 0,
                    isolated_transitions: 0,
                    transitions_with_competing_target_statuses: 0,
                    aggregates: Vec::new(),
                },
                AttributeInput {
                    attribute_id: 11,
                    transitions_examined: 1,
                    complete_before_and_after: 1,
                    missing_before: 0,
                    missing_after_within_window: 0,
                    isolated_transitions: 1,
                    transitions_with_competing_target_statuses: 0,
                    aggregates: Vec::new(),
                },
            ],
            percent_family_formulas: Vec::new(),
        });
        assert_eq!(
            effect
                .attributes
                .iter()
                .map(|attribute| attribute.attribute_id)
                .collect::<Vec<_>>(),
            vec![11]
        );
    }
}
