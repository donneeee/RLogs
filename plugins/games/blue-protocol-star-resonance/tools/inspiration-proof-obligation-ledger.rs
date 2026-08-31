use std::{
    collections::BTreeSet,
    env,
    error::Error,
    fs::{self, File},
    io::{BufWriter, Write},
    path::{Path, PathBuf},
};

use serde::Serialize;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

const SCHEMA_VERSION: u16 = 4;
const INSPIRATION_EFFECT_ID: i64 = 2_202_041;
const INSPIRATION_SOURCE_CONFIG_ID: i64 = 2_202_040;
const FULL_BLOOM_EFFECT_ID: i64 = 2_404_271;
const FULL_BLOOM_SOURCE_CONFIG_ID: i64 = 2_404_270;
const FALCONRY_CLASS_ID: i64 = 11;
const FALCONRY_SPECIALIZATION_ID: i64 = 117;
const LIGHT_DAMAGE_ATTRIBUTE_ID: i64 = 13_170;
const LIGHT_DAMAGE_PROPERTY_ID: i64 = 7;
const MASTERY_TO_LIGHT_NUMERATOR: i64 = 60;
const MASTERY_TO_LIGHT_DENOMINATOR: i64 = 100;

#[derive(Debug)]
struct Arguments {
    packet_build: String,
    runtime: PathBuf,
    closure: PathBuf,
    mastery: PathBuf,
    external_proofs: Vec<PathBuf>,
    status_surface: PathBuf,
    output: PathBuf,
    overwrite: bool,
}

#[derive(Debug, Serialize)]
struct SourceFile {
    path: String,
    bytes: u64,
    sha256: String,
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
struct ProviderVectorPartition {
    attack_raw_delta: i64,
    external_damage_raw_delta: i64,
    property_damage_attribute_id: i64,
    property_damage_raw_delta: i64,
    required_damage_property: i64,
    provider_full_bloom: bool,
    status_effect_id: i64,
    status_source_config_id: i64,
}

fn main() {
    if let Err(error) = run(env::args().skip(1).collect()) {
        eprintln!("{error}");
        std::process::exit(1);
    }
}

fn run(values: Vec<String>) -> Result<(), Box<dyn Error>> {
    let arguments = parse_arguments(values)?;
    if arguments.output.exists() && !arguments.overwrite {
        return Err(format!(
            "output already exists: {} (pass --overwrite to replace it)",
            arguments.output.display()
        )
        .into());
    }

    let (runtime, runtime_source) = read_json_with_source(&arguments.runtime)?;
    let (closure, closure_source) = read_json_with_source(&arguments.closure)?;
    let (mastery, mastery_source) = read_json_with_source(&arguments.mastery)?;
    let external_inputs = arguments
        .external_proofs
        .iter()
        .map(|path| read_json_with_source(path))
        .collect::<Result<Vec<_>, _>>()?;
    let (status, status_source) = read_json_with_source(&arguments.status_surface)?;

    for (external, _) in &external_inputs {
        validate_inputs(&arguments, &runtime, &closure, &mastery, external, &status)?;
    }
    let vector_partitions = validate_external_vector_partitions(&runtime, &external_inputs)?;

    let inspiration = object_at(&runtime, "inspiration")?;
    let packet_vectors = array_at(inspiration, "packet_proven_vectors")?.clone();
    let first_external = &external_inputs
        .first()
        .ok_or("at least one component-aware external proof is required")?
        .0;
    let formula = object_at(first_external, "formula")?;
    let diagnostic = object_at(first_external, "status_uncontrolled_diagnostic_formula")?;
    let status_summary = object_at(&status, "summary")?;
    let mastery_summary = object_at(&mastery, "summary")?;
    let snapshot = object_at(&closure, "snapshot_boundary_proof")?;

    let mut external_active_events = None;
    let mut exact_attack_events = 0_u64;
    let mut exact_composite_events = 0_u64;
    let mut missing_composite_events = 0_u64;
    let mut property_matched_events = 0_u64;
    let mut property_rejected_events = 0_u64;
    let mut exact_property_composite_events = 0_u64;
    let mut missing_property_composite_events = 0_u64;
    let mut matched_vector_events = 0_u64;
    let mut coverage_gap_buckets = 0_usize;
    let mut exact_observed_damage = 0_i128;
    let mut exact_composite_observed_damage = 0_i128;
    let mut exact_property_composite_observed_damage = 0_i128;
    let mut configured_vectors = Vec::new();
    for ((external, _), partition) in external_inputs.iter().zip(&vector_partitions) {
        let single = object_at(external, "single_event_damage_attr_counterfactual")?;
        let active = u64_at(single, "external_active_damage_events")?;
        if external_active_events.is_some_and(|expected| expected != active) {
            return Err("component-aware proofs do not cover the same external event set".into());
        }
        external_active_events = Some(active);
        exact_attack_events = exact_attack_events.saturating_add(u64_at(
            single,
            "events_with_exact_conserved_attack_stage_share",
        )?);
        exact_composite_events = exact_composite_events.saturating_add(u64_at(
            single,
            "events_with_exact_conserved_attack_external_composite_share",
        )?);
        missing_composite_events = missing_composite_events.saturating_add(u64_at(
            single,
            "events_without_exact_conserved_attack_external_composite_share",
        )?);
        property_matched_events = property_matched_events
            .saturating_add(u64_at(single, "events_matching_required_damage_property")?);
        property_rejected_events = property_rejected_events.saturating_add(u64_at(
            single,
            "events_rejected_by_required_damage_property",
        )?);
        exact_property_composite_events = exact_property_composite_events.saturating_add(u64_at(
            single,
            "events_with_exact_conserved_attack_external_property_composite_share",
        )?);
        missing_property_composite_events =
            missing_property_composite_events.saturating_add(u64_at(
                single,
                "events_without_exact_conserved_attack_external_property_composite_share",
            )?);
        matched_vector_events = matched_vector_events
            .saturating_add(u64_at(single, "events_matching_required_provider_status")?);
        coverage_gap_buckets = coverage_gap_buckets
            .saturating_add(array_at(single, "exact_conserved_share_coverage_gaps")?.len());
        exact_observed_damage = exact_observed_damage.saturating_add(parse_i128_value(
            single,
            "exact_conserved_share_observed_damage",
        )?);
        exact_composite_observed_damage =
            exact_composite_observed_damage.saturating_add(parse_i128_value(
                single,
                "exact_conserved_attack_external_composite_observed_damage",
            )?);
        exact_property_composite_observed_damage = exact_property_composite_observed_damage
            .saturating_add(parse_i128_value(
                single,
                "exact_conserved_attack_external_property_composite_observed_damage",
            )?);
        configured_vectors.push(json!({
            "attack_raw_delta": partition.attack_raw_delta,
            "external_damage_raw_delta": partition.external_damage_raw_delta,
            "property_damage_attribute_id": partition.property_damage_attribute_id,
            "property_damage_raw_delta": partition.property_damage_raw_delta,
            "required_damage_property": partition.required_damage_property,
            "provider_full_bloom": partition.provider_full_bloom,
            "status_gate": {
                "effect_id": partition.status_effect_id,
                "source_config_id": partition.status_source_config_id,
                "expected_active": partition.provider_full_bloom
            }
        }));
    }
    let external_active_events = external_active_events.unwrap_or_default();
    if matched_vector_events > external_active_events {
        return Err("provider-vector partitions overlap".into());
    }
    let unclassified_vector_events = external_active_events - matched_vector_events;
    let strict_pairs = u64_at(formula, "pairs")?;
    let diagnostic_pairs = u64_at(diagnostic, "pairs")?;
    let diagnostic_exact = u64_at(diagnostic, "exact_ratio_matches")?;
    let mismatch_effects = u64_at(status_summary, "distinct_effects")?;
    let mismatch_mentions = u64_at(status_summary, "candidate_pair_mentions")?;
    let mismatch_with_attributes =
        u64_at(status_summary, "effects_with_observed_attribute_evidence")?;
    let mastery_consumers = u64_at(mastery_summary, "paired_consumers")?;

    let output = json!({
        "schema_version": SCHEMA_VERSION,
        "generated_by": "rlogs-bpsr-inspiration-proof-obligation-ledger",
        "game": "blue-protocol-star-resonance",
        "packet_build": arguments.packet_build,
        "effect_id": INSPIRATION_EFFECT_ID,
        "source_config_id": INSPIRATION_SOURCE_CONFIG_ID,
        "promotion_state": "blocked-by-explicit-proof-obligations",
        "runtime_transfer_enabled": false,
        "policy": {
            "unresolved_evidence_hidden": false,
            "diagnostic_status_ignores_are_formula_authority": false,
            "localized_descriptions_are_formula_authority": false,
            "current_tables_reinterpret_historical_packets": false,
            "partial_vector_transfer_allowed": false,
            "promotion_requires_complete_event_level_conservation": true,
            "purpose": "one versioned, reproducible boundary between packet-proven Inspiration components and the remaining formula obligations"
        },
        "inputs": {
            "runtime": runtime_source,
            "packet_attribute_closure": closure_source,
            "exact_build_mastery_consumers": mastery_source,
            "component_aware_external_proofs": external_inputs
                .iter()
                .map(|(_, source)| source)
                .collect::<Vec<_>>(),
            "status_mismatch_semantic_surface": status_source
        },
        "packet_proven_vectors": packet_vectors,
        "proof_components": [
            {
                "component": "primary-stat-to-attack",
                "evidence_state": "exact-conserved-rational-share-for-resolved-standard-attack-events",
                "covered_events": exact_attack_events,
                "eligible_events": external_active_events,
                "covered_observed_damage": exact_observed_damage.to_string(),
                "coverage_gap_buckets": coverage_gap_buckets,
                "runtime_component_enabled": false,
                "remaining_obligation": "compose this base-stage share with every other provider-caused Inspiration stage once, without double-counting cross terms"
            },
            {
                "component": "critical-chance",
                "evidence_state": "packet-transition-and-chance-occurrence-counterfactual-proven",
                "runtime_component_enabled": false,
                "remaining_obligation": "compose chance ownership with the same event's base, damage-factor, and other chance ownership under exact conservation"
            },
            {
                "component": "lucky-strike-chance",
                "evidence_state": "packet-transition-and-chance-occurrence-counterfactual-proven",
                "runtime_component_enabled": false,
                "remaining_obligation": "compose chance ownership with the same event's base, damage-factor, and other chance ownership under exact conservation"
            },
            {
                "component": "versatility-to-external-damage",
                "evidence_state": "packet-transition-conversion-and-cross-term-safe-composite-accounting-proven-factor-placement-unproven",
                "configured_packet_proven_vectors": configured_vectors,
                "partitioned_events": matched_vector_events,
                "unclassified_events": unclassified_vector_events,
                "composite_events": exact_composite_events,
                "composite_failures": missing_composite_events,
                "composite_observed_damage": exact_composite_observed_damage.to_string(),
                "runtime_component_enabled": false,
                "remaining_obligation": "prove the External Damage integer stage and snapshot against packet outcomes; do not count raw Versatility and derived External Damage twice"
            },
            {
                "component": "class-and-spec-specific-mastery",
                "evidence_state": "exact-build-consumer-inventory-plus-falconry-mastery-to-light-transition-and-property-classified-three-stage-candidate-conservation",
                "exact_build_consumer_count": mastery_consumers,
                "falconry_class_id": FALCONRY_CLASS_ID,
                "falconry_specialization_id": FALCONRY_SPECIALIZATION_ID,
                "falconry_light_damage_attribute_id": LIGHT_DAMAGE_ATTRIBUTE_ID,
                "falconry_light_damage_property_id": LIGHT_DAMAGE_PROPERTY_ID,
                "mastery_to_light_integer_relation": {
                    "numerator": MASTERY_TO_LIGHT_NUMERATOR,
                    "denominator": MASTERY_TO_LIGHT_DENOMINATOR,
                    "rounding": "floor-for-nonnegative-packet-deltas"
                },
                "attack_external_composite_events": exact_composite_events,
                "light_property_matching_events": property_matched_events,
                "general_or_other_property_events_retained": property_rejected_events,
                "attack_external_light_composite_events": exact_property_composite_events,
                "attack_external_light_composite_failures": missing_property_composite_events,
                "attack_external_light_composite_observed_damage": exact_property_composite_observed_damage.to_string(),
                "snapshot_gap_events": u64_at(snapshot, "gap_events")?,
                "state_controlled_observations": u64_at(snapshot, "two_sided_state_controlled_observations")?,
                "runtime_component_enabled": false,
                "remaining_obligation": "prove Light-property stage placement/order and calculation snapshot against packet outcomes, then prove applicability and integer formula separately for every other packet-observed class/spec/ability consumer; Mastery is not a universal damage factor"
            },
            {
                "component": "haste-action-opportunity",
                "evidence_state": "packet-attribute-transition-proven-opportunity-counterfactual-unproven",
                "runtime_component_enabled": false,
                "remaining_obligation": "prove additional actions in a conserved time-window model without assigning hypothetical unobserved hits"
            }
        ],
        "pairing_diagnostics": {
            "strict_pairs": strict_pairs,
            "relaxed_diagnostic_pairs": diagnostic_pairs,
            "relaxed_exact_ratio_matches": diagnostic_exact,
            "co_active_mismatch_effects_retained": mismatch_effects,
            "candidate_pair_mentions_retained": mismatch_mentions,
            "mismatch_effects_with_packet_attribute_evidence": mismatch_with_attributes,
            "interpretation": "relaxing status controls did not produce a formula witness; no mismatch effect is silently ignored"
        },
        "promotion_gate": {
            "complete_vector_counterfactual_proven": false,
            "complete_vector_conservation_proven": false,
            "falconry_mastery_to_light_packet_relation_proven": true,
            "falconry_attack_external_light_candidate_conservation_complete": missing_property_composite_events == 0,
            "falconry_light_stage_placement_and_snapshot_proven": false,
            "all_class_specific_mastery_consumers_proven": false,
            "haste_opportunity_model_proven": false,
            "runtime_transfer_allowed": false
        }
    });

    if let Some(parent) = arguments.output.parent() {
        fs::create_dir_all(parent)?;
    }
    let mut writer = BufWriter::new(File::create(&arguments.output)?);
    serde_json::to_writer_pretty(&mut writer, &output)?;
    writer.write_all(b"\n")?;
    writer.flush()?;
    println!(
        "wrote Inspiration proof ledger with {exact_attack_events}/{external_active_events} exact Attack-stage events and {exact_composite_events} cross-term-safe composite candidates to {}",
        arguments.output.display()
    );
    Ok(())
}

fn validate_external_vector_partitions(
    runtime: &Value,
    external_inputs: &[(Value, SourceFile)],
) -> Result<Vec<ProviderVectorPartition>, Box<dyn Error>> {
    let runtime_vectors = array_at(object_at(runtime, "inspiration")?, "packet_proven_vectors")?;
    let mut expected = BTreeSet::new();
    let mut expected_modes = BTreeSet::new();
    for vector in runtime_vectors {
        let partition = ProviderVectorPartition {
            attack_raw_delta: integer_at(vector, "secondary_raw_add_delta")
                .ok_or("runtime Inspiration vector secondary_raw_add_delta missing")?,
            external_damage_raw_delta: integer_at(vector, "external_damage_delta")
                .ok_or("runtime Inspiration vector external_damage_delta missing")?,
            property_damage_attribute_id: LIGHT_DAMAGE_ATTRIBUTE_ID,
            property_damage_raw_delta: mastery_to_light_delta(
                integer_at(vector, "secondary_raw_add_delta")
                    .ok_or("runtime Inspiration vector secondary_raw_add_delta missing")?,
            )?,
            required_damage_property: LIGHT_DAMAGE_PROPERTY_ID,
            provider_full_bloom: vector
                .get("provider_full_bloom")
                .and_then(Value::as_bool)
                .ok_or("runtime Inspiration vector provider_full_bloom missing")?,
            status_effect_id: FULL_BLOOM_EFFECT_ID,
            status_source_config_id: FULL_BLOOM_SOURCE_CONFIG_ID,
        };
        if !expected.insert(partition) {
            return Err(
                "runtime Inspiration packet vectors contain a duplicate provider mode".into(),
            );
        }
        if !expected_modes.insert(partition.provider_full_bloom) {
            return Err(
                "binary Full Bloom status cannot uniquely partition multiple packet vectors in the same provider mode"
                    .into(),
            );
        }
    }

    let mut observed = BTreeSet::new();
    let mut partitions = Vec::with_capacity(external_inputs.len());
    for (external, _) in external_inputs {
        let status_gate = object_at(external, "required_provider_status")?;
        let partition = ProviderVectorPartition {
            attack_raw_delta: object_at(external, "attack_provider_delta")?
                .get("raw_delta")
                .and_then(Value::as_i64)
                .ok_or("component-aware proof attack delta missing")?,
            external_damage_raw_delta: external
                .get("provider_external_damage_raw_delta")
                .and_then(Value::as_i64)
                .ok_or("component-aware proof External Damage delta missing")?,
            property_damage_attribute_id: external
                .get("provider_property_damage_attribute_id")
                .and_then(Value::as_i64)
                .ok_or("component-aware proof property Damage attribute missing")?,
            property_damage_raw_delta: external
                .get("provider_property_damage_raw_delta")
                .and_then(Value::as_i64)
                .ok_or("component-aware proof property Damage delta missing")?,
            required_damage_property: external
                .get("required_damage_property")
                .and_then(Value::as_i64)
                .ok_or("component-aware proof required damage property missing")?,
            provider_full_bloom: status_gate
                .get("expected_active")
                .and_then(Value::as_bool)
                .unwrap_or(true),
            status_effect_id: status_gate
                .get("effect_id")
                .and_then(Value::as_i64)
                .ok_or("component-aware proof status gate effect_id missing")?,
            status_source_config_id: status_gate
                .get("source_config_id")
                .and_then(Value::as_i64)
                .ok_or("component-aware proof status gate source_config_id missing")?,
        };
        if partition.status_effect_id != FULL_BLOOM_EFFECT_ID
            || partition.status_source_config_id != FULL_BLOOM_SOURCE_CONFIG_ID
        {
            return Err(
                "component-aware proof does not use the exact Full Bloom status identity".into(),
            );
        }
        if !expected.contains(&partition) {
            return Err(format!(
                "component-aware proof vector Attack={}, External={}, PropertyAttribute={}, PropertyDelta={}, RequiredProperty={}, FullBloom={} does not match the exact runtime vector partition",
                partition.attack_raw_delta,
                partition.external_damage_raw_delta,
                partition.property_damage_attribute_id,
                partition.property_damage_raw_delta,
                partition.required_damage_property,
                partition.provider_full_bloom
            )
            .into());
        }
        if !observed.insert(partition) {
            return Err(
                "component-aware proofs contain a duplicate provider-vector partition".into(),
            );
        }
        let single = object_at(external, "single_event_damage_attr_counterfactual")?;
        let active = u64_at(single, "external_active_damage_events")?;
        let matched = u64_at(single, "events_matching_required_provider_status")?;
        let rejected = u64_at(single, "events_rejected_by_required_provider_status")?;
        if matched.saturating_add(rejected) != active {
            return Err(
                "provider-vector status gate does not account for every External-active event"
                    .into(),
            );
        }
        let exact_composite = u64_at(
            single,
            "events_with_exact_conserved_attack_external_composite_share",
        )?;
        let property_matched = u64_at(single, "events_matching_required_damage_property")?;
        let property_rejected = u64_at(single, "events_rejected_by_required_damage_property")?;
        if property_matched.saturating_add(property_rejected) != exact_composite {
            return Err(
                "damage-property gate does not account for every exact Attack+External composite event"
                    .into(),
            );
        }
        let exact_property_composite = u64_at(
            single,
            "events_with_exact_conserved_attack_external_property_composite_share",
        )?;
        let missing_property_composite = u64_at(
            single,
            "events_without_exact_conserved_attack_external_property_composite_share",
        )?;
        if exact_property_composite.saturating_add(missing_property_composite) != property_matched {
            return Err(
                "property composite accounting does not cover every matching damage-property event"
                    .into(),
            );
        }
        if missing_property_composite != 0 {
            return Err(
                "property composite proof retains matching Light events without an exact conserved share"
                    .into(),
            );
        }
        validate_falconry_recipients(external)?;
        partitions.push(partition);
    }
    if observed != expected {
        return Err(
            "component-aware proofs do not cover every exact runtime provider-vector partition once"
                .into(),
        );
    }
    Ok(partitions)
}

fn mastery_to_light_delta(mastery_raw_delta: i64) -> Result<i64, Box<dyn Error>> {
    if mastery_raw_delta < 0 {
        return Err("packet-proven Inspiration Mastery delta must be nonnegative".into());
    }
    Ok(mastery_raw_delta
        .saturating_mul(MASTERY_TO_LIGHT_NUMERATOR)
        .div_euclid(MASTERY_TO_LIGHT_DENOMINATOR))
}

fn validate_falconry_recipients(external: &Value) -> Result<(), Box<dyn Error>> {
    let sessions = external
        .get("sessions")
        .and_then(Value::as_array)
        .ok_or("component-aware proof sessions missing")?;
    let mut recipients = 0_u64;
    for session in sessions {
        let specializations = session
            .get("externally_affected_actor_specializations")
            .and_then(Value::as_array)
            .ok_or("component-aware proof recipient specialization evidence missing")?;
        for specialization in specializations {
            recipients = recipients.saturating_add(1);
            let class_id = integer_at(specialization, "resolved_class_id")
                .ok_or("externally affected actor class is unresolved")?;
            let specialization_id = integer_at(specialization, "resolved_specialization_id")
                .ok_or("externally affected actor specialization is unresolved")?;
            if class_id != FALCONRY_CLASS_ID || specialization_id != FALCONRY_SPECIALIZATION_ID {
                return Err(format!(
                    "Light-property proof includes non-Falconry recipient class={class_id} specialization={specialization_id}"
                )
                .into());
            }
        }
    }
    if recipients == 0 {
        return Err("Light-property proof has no resolved Falconry recipients".into());
    }
    Ok(())
}

fn validate_inputs(
    arguments: &Arguments,
    runtime: &Value,
    closure: &Value,
    mastery: &Value,
    external: &Value,
    status: &Value,
) -> Result<(), Box<dyn Error>> {
    let runtime_build = string_at(runtime, "game_build").ok_or("runtime game_build missing")?;
    if runtime_build != arguments.packet_build {
        return Err(format!(
            "runtime build {runtime_build} does not match packet build {}",
            arguments.packet_build
        )
        .into());
    }
    let closure_build = string_at(closure, "game_build")
        .ok_or("packet attribute closure game_build missing")?
        .strip_prefix("global-steam-")
        .unwrap_or_else(|| string_at(closure, "game_build").unwrap());
    if closure_build != arguments.packet_build {
        return Err("packet attribute closure build mismatch".into());
    }
    for (label, input) in [("mastery", mastery), ("status surface", status)] {
        if string_at(input, "game_build") != Some(arguments.packet_build.as_str()) {
            return Err(format!("{label} build mismatch").into());
        }
    }
    require_generated_by(mastery, "rlogs-bpsr-mastery-consumer-scan")?;
    require_generated_by(external, "rlogs-bpsr-external-attack-damage-proof")?;
    require_generated_by(status, "rlogs-bpsr-status-mismatch-semantic-surface")?;
    if integer_at(closure, "effect_id") != Some(INSPIRATION_EFFECT_ID)
        || integer_at(closure, "source_config_id") != Some(INSPIRATION_SOURCE_CONFIG_ID)
        || integer_at(external, "selected_effect_id") != Some(INSPIRATION_EFFECT_ID)
        || integer_at(external, "selected_source_config_id") != Some(INSPIRATION_SOURCE_CONFIG_ID)
    {
        return Err("Inspiration effect or source-config identity mismatch".into());
    }
    if object_at(runtime, "inspiration")?
        .get("runtime_transfer_enabled")
        .and_then(Value::as_bool)
        != Some(false)
    {
        return Err("input runtime must keep Inspiration transfer disabled".into());
    }
    let attack_delta = object_at(external, "attack_provider_delta")?
        .get("raw_delta")
        .and_then(Value::as_i64)
        .ok_or("component-aware proof attack delta missing")?;
    let external_delta = external
        .get("provider_external_damage_raw_delta")
        .and_then(Value::as_i64)
        .ok_or("component-aware proof External Damage delta missing")?;
    let vector_matches = array_at(object_at(runtime, "inspiration")?, "packet_proven_vectors")?
        .iter()
        .any(|vector| {
            integer_at(vector, "secondary_raw_add_delta") == Some(attack_delta)
                && integer_at(vector, "external_damage_delta") == Some(external_delta)
        });
    if !vector_matches {
        return Err(format!(
            "component-aware proof deltas Attack={attack_delta}, External={external_delta} do not match one packet-proven Inspiration vector"
        )
        .into());
    }
    Ok(())
}

fn parse_arguments(values: Vec<String>) -> Result<Arguments, Box<dyn Error>> {
    let mut packet_build = None;
    let mut runtime = None;
    let mut closure = None;
    let mut mastery = None;
    let mut external_proofs = Vec::new();
    let mut status_surface = None;
    let mut output = None;
    let mut overwrite = false;
    let mut index = 0;
    while index < values.len() {
        let key = values[index].as_str();
        if key == "--overwrite" {
            overwrite = true;
            index += 1;
            continue;
        }
        let value = values
            .get(index + 1)
            .ok_or_else(|| format!("missing value for {key}"))?;
        match key {
            "--packet-build" => packet_build = Some(value.clone()),
            "--runtime" => runtime = Some(PathBuf::from(value)),
            "--closure" => closure = Some(PathBuf::from(value)),
            "--mastery" => mastery = Some(PathBuf::from(value)),
            "--external-proof" => external_proofs.push(PathBuf::from(value)),
            "--status-surface" => status_surface = Some(PathBuf::from(value)),
            "--output" => output = Some(PathBuf::from(value)),
            _ => return Err(format!("unknown argument: {key}").into()),
        }
        index += 2;
    }
    let packet_build = packet_build.ok_or("--packet-build is required")?;
    if packet_build.is_empty() || !packet_build.bytes().all(|byte| byte.is_ascii_digit()) {
        return Err("--packet-build must contain only digits".into());
    }
    Ok(Arguments {
        packet_build,
        runtime: runtime.ok_or("--runtime is required")?,
        closure: closure.ok_or("--closure is required")?,
        mastery: mastery.ok_or("--mastery is required")?,
        external_proofs: if external_proofs.is_empty() {
            return Err("--external-proof is required at least once".into());
        } else {
            external_proofs
        },
        status_surface: status_surface.ok_or("--status-surface is required")?,
        output: output.ok_or("--output is required")?,
        overwrite,
    })
}

fn read_json_with_source(path: &Path) -> Result<(Value, SourceFile), Box<dyn Error>> {
    let bytes = fs::read(path)?;
    let value = serde_json::from_slice(&bytes)?;
    let sha256 = format!("sha256:{:x}", Sha256::digest(&bytes));
    Ok((
        value,
        SourceFile {
            path: path.display().to_string(),
            bytes: bytes.len() as u64,
            sha256,
        },
    ))
}

fn require_generated_by(value: &Value, expected: &str) -> Result<(), Box<dyn Error>> {
    if string_at(value, "generated_by") != Some(expected) {
        return Err(format!("expected generated_by={expected}").into());
    }
    Ok(())
}

fn object_at<'a>(
    value: &'a Value,
    key: &str,
) -> Result<&'a serde_json::Map<String, Value>, Box<dyn Error>> {
    value
        .get(key)
        .and_then(Value::as_object)
        .ok_or_else(|| format!("missing object: {key}").into())
}

fn array_at<'a>(
    value: &'a serde_json::Map<String, Value>,
    key: &str,
) -> Result<&'a Vec<Value>, Box<dyn Error>> {
    value
        .get(key)
        .and_then(Value::as_array)
        .ok_or_else(|| format!("missing array: {key}").into())
}

fn string_at<'a>(value: &'a Value, key: &str) -> Option<&'a str> {
    value.get(key).and_then(Value::as_str)
}

fn integer_at(value: &Value, key: &str) -> Option<i64> {
    value.get(key).and_then(Value::as_i64)
}

fn u64_at(value: &serde_json::Map<String, Value>, key: &str) -> Result<u64, Box<dyn Error>> {
    value
        .get(key)
        .and_then(Value::as_u64)
        .ok_or_else(|| format!("missing unsigned integer: {key}").into())
}

fn string_or_integer_at(
    value: &serde_json::Map<String, Value>,
    key: &str,
) -> Result<String, Box<dyn Error>> {
    let candidate = value
        .get(key)
        .ok_or_else(|| format!("missing value: {key}"))?;
    if let Some(text) = candidate.as_str() {
        return Ok(text.to_owned());
    }
    if let Some(integer) = candidate.as_i64() {
        return Ok(integer.to_string());
    }
    if let Some(integer) = candidate.as_u64() {
        return Ok(integer.to_string());
    }
    Err(format!("{key} is neither a string nor an integer").into())
}

fn parse_i128_value(
    value: &serde_json::Map<String, Value>,
    key: &str,
) -> Result<i128, Box<dyn Error>> {
    string_or_integer_at(value, key)?
        .parse::<i128>()
        .map_err(|error| format!("invalid integer value for {key}: {error}").into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parser_requires_digit_build_and_every_input() {
        let error = parse_arguments(vec!["--packet-build".into(), "old".into()])
            .expect_err("non-numeric builds must fail");
        assert!(error.to_string().contains("only digits"));
    }

    #[test]
    fn string_or_integer_preserves_large_decimal_text() {
        let value = serde_json::from_value::<Value>(json!({"damage": "3326630368"})).unwrap();
        assert_eq!(
            string_or_integer_at(value.as_object().unwrap(), "damage").unwrap(),
            "3326630368"
        );
    }

    #[test]
    fn packet_proven_mastery_delta_converts_to_light_with_integer_floor() {
        assert_eq!(mastery_to_light_delta(300).unwrap(), 180);
        assert_eq!(mastery_to_light_delta(360).unwrap(), 216);
        assert_eq!(mastery_to_light_delta(1).unwrap(), 0);
        assert!(mastery_to_light_delta(-1).is_err());
    }
}
