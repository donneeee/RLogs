use std::{
    collections::{BTreeMap, BTreeSet},
    env,
    error::Error,
    fs::{self, File},
    io::{BufReader, BufWriter, Read, Write},
    path::{Path, PathBuf},
};

use serde::Serialize;
use serde_json::{Map, Value};
use sha2::{Digest, Sha256};

const SCHEMA_VERSION: u16 = 3;

#[derive(Debug, Serialize)]
#[serde(deny_unknown_fields)]
struct Worklist {
    schema_version: u16,
    generated_by: &'static str,
    game_build: String,
    promotion_state: &'static str,
    policy: Policy,
    inputs: Inputs,
    summary: Summary,
    exact_produced_damage_candidates: Vec<Candidate>,
    formula_replay_candidates: Vec<Candidate>,
}

#[derive(Debug, Serialize)]
#[serde(deny_unknown_fields)]
struct Policy {
    static_metadata_enables_rdps: bool,
    packet_replay_required: bool,
    exact_provider_recipient_required: bool,
    exact_party_conservation_required: bool,
    unresolved_evidence_hidden: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(deny_unknown_fields)]
struct Inputs {
    classification: InputArtifact,
    contribution: InputArtifact,
    recount: InputArtifact,
    value_proof: InputArtifact,
}

#[derive(Debug, Clone, Serialize)]
#[serde(deny_unknown_fields)]
struct InputArtifact {
    file: String,
    bytes: u64,
    sha256: String,
}

#[derive(Debug, Serialize)]
#[serde(deny_unknown_fields)]
struct Summary {
    classification_rules: usize,
    contribution_rules: usize,
    formula_replay_candidates: usize,
    exact_produced_damage_rules: usize,
    overlap_only_rules: usize,
    timing_only_rules: usize,
    defensive_rules: usize,
    candidate_rules_with_value_proof: usize,
    candidate_value_proofs: usize,
    candidate_rules_with_selected_values: usize,
    candidate_rules_with_support_domain: usize,
    candidate_rules_with_supportive_role: usize,
    exact_candidates_with_runtime_matcher: usize,
    formula_candidates_with_runtime_matcher: usize,
    formula_candidates_without_value_proof: usize,
    automatically_enabled_for_rdps: usize,
}

#[derive(Debug, Serialize)]
#[serde(deny_unknown_fields)]
struct Candidate {
    source_rule_id: String,
    source_id: Option<String>,
    primary_role: Option<String>,
    row_model: Option<String>,
    report_domains: Vec<String>,
    contribution_mode: String,
    contribution_tier: Option<String>,
    confidence: Option<String>,
    formula_term_ids: Vec<String>,
    formula_zone_ids: Vec<String>,
    contribution_groups: Vec<String>,
    transfer_eligibilities: Vec<String>,
    predicate_tags: Vec<String>,
    required_runtime_evidence: Vec<String>,
    relationship_components: Vec<Value>,
    runtime_matcher: RuntimeMatcher,
    value_proofs: Vec<ValueProof>,
    static_value_state: &'static str,
    static_blockers: Vec<String>,
    rdps_enablement: &'static str,
}

#[derive(Debug, Serialize)]
#[serde(deny_unknown_fields)]
struct RuntimeMatcher {
    source_kind: Option<String>,
    source_type: Option<String>,
    source_entity_id: Option<i64>,
    runtime_detection: Option<String>,
    buff_ids: Vec<i64>,
    target_damage_ids: Vec<i64>,
    target_recount_ids: Vec<i64>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(deny_unknown_fields)]
struct ValueProof {
    key: String,
    formula_readiness: Option<String>,
    value_proof_status: Option<String>,
    selected_value_count: usize,
    selected_values: Vec<Value>,
    value_selector_count: usize,
    value_selectors: Vec<Value>,
    proof_requirements: Vec<String>,
    value_blockers: Vec<String>,
    blockers: Vec<String>,
}

#[derive(Debug, Serialize)]
#[serde(deny_unknown_fields)]
struct MagnitudeProofWatchlist {
    schema_version: u16,
    generated_by: &'static str,
    deployment_id: &'static str,
    game_build: String,
    source_inputs: Inputs,
    buff_table: InputArtifact,
    promotion_state: &'static str,
    policy: MagnitudeProofPolicy,
    after_window_micros: u64,
    example_limit: usize,
    selected_effect_ids: Vec<i64>,
    reported_effect_ids: Vec<i64>,
    selected_attribute_ids: Vec<i32>,
    non_attributable_context_attribute_ids: Vec<i32>,
    stateful_attribute_ids: Vec<i32>,
    candidates: Vec<MagnitudeProofCandidate>,
}

#[derive(Debug, Serialize)]
#[serde(deny_unknown_fields)]
struct MagnitudeProofPolicy {
    game_tables_prove_identity_and_semantics_only: bool,
    static_values_are_validation_expectations_not_runtime_authority: bool,
    packet_lifecycle_proves_occurrence_provider_recipient_and_magnitude: bool,
    application_and_removal_must_be_reversible: bool,
    multiple_independent_instances_required: bool,
    exact_party_damage_conservation_required_before_runtime_credit: bool,
    selected_attributes_are_formula_context_not_credit_authority: bool,
    self_only_attributes_never_create_external_credit: bool,
    unresolved_evidence_is_hidden: bool,
    runtime_enabled: bool,
}

#[derive(Debug, Serialize)]
#[serde(deny_unknown_fields)]
struct MagnitudeProofCandidate {
    source_rule_id: String,
    source_id: Option<String>,
    declared_effect_references: Vec<i64>,
    effect_ids: Vec<i64>,
    rejected_effect_references: Vec<RejectedEffectReference>,
    formula_terms: Vec<String>,
    transfer_eligibilities: Vec<String>,
    selected_attribute_ids: Vec<i32>,
    required_runtime_evidence: Vec<String>,
    static_value_state: String,
    static_value_proofs: Vec<ValueProof>,
    static_blockers: Vec<String>,
    lifecycle_effects: Vec<MagnitudeProofLifecycleEffect>,
}

#[derive(Debug, Serialize)]
#[serde(deny_unknown_fields)]
struct RejectedEffectReference {
    effect_id: i64,
    reason: &'static str,
}

#[derive(Debug, Serialize)]
#[serde(deny_unknown_fields)]
struct MagnitudeProofLifecycleEffect {
    effect_id: i64,
    name: Option<String>,
    icon: Option<String>,
    repeat_add_rule: Vec<i64>,
    declared_max_stacks: Option<i64>,
    proof_model: &'static str,
    destroy_param: Value,
}

fn main() {
    if let Err(error) = run(env::args().skip(1).collect()) {
        eprintln!("{error}");
        std::process::exit(1);
    }
}

fn run(arguments: Vec<String>) -> Result<(), Box<dyn Error>> {
    let options = parse_options(&arguments)?;
    let classification_path = required_path(&options, "classification")?;
    let contribution_path = required_path(&options, "contribution")?;
    let recount_path = required_path(&options, "recount")?;
    let value_proof_path = required_path(&options, "value-proof")?;
    let game_build = required(&options, "build")?.to_owned();
    let output_path = required_path(&options, "output")?;
    let watchlist_output_path = optional_path(&options, "watchlist-output");
    let buff_table_path = optional_path(&options, "buff-table");
    validate_build(&game_build)?;

    let classification = read_json(&classification_path)?;
    let contribution = read_json(&contribution_path)?;
    let recount = read_json(&recount_path)?;
    let value_proof = read_json(&value_proof_path)?;
    require_generator(&classification, "ModifierClassificationTable.gen")?;
    require_generator(&contribution, "ModifierContributionTable.gen")?;
    require_generator(&recount, "ModifierRecountTable.gen")?;
    require_schema(&value_proof, 1)?;

    let classifications = object_at(&classification, "sourcesByRuleId")?;
    let contributions = object_at(&contribution, "sourcesByRuleId")?;
    let recounts = object_at(&recount, "sourcesById")?;
    let proofs = object_at(&value_proof, "entriesByKey")?;
    let proof_index = index_value_proofs(proofs);

    let mut mode_counts = BTreeMap::<String, usize>::new();
    let mut exact_candidates = Vec::new();
    let mut formula_candidates = Vec::new();
    let mut candidates_with_value_proof = 0usize;
    let mut candidate_value_proofs = 0usize;
    let mut candidates_with_selected_values = 0usize;
    let mut candidates_with_support_domain = 0usize;
    let mut candidates_with_supportive_role = 0usize;

    for (rule_id, contribution_row) in contributions {
        let contribution_mode = string_at(contribution_row, "contributionMode")
            .unwrap_or("unresolved")
            .to_owned();
        *mode_counts.entry(contribution_mode.clone()).or_default() += 1;
        if contribution_mode != "formula-replay-candidate"
            && contribution_mode != "exact-produced-damage"
        {
            continue;
        }
        let is_formula_candidate = contribution_mode == "formula-replay-candidate";

        let classification_row = classifications.get(rule_id);
        let report_domains =
            string_array(classification_row.and_then(|row| row.get("reportDomains")));
        let primary_role = classification_row
            .and_then(|row| string_at(row, "primaryRole"))
            .map(str::to_owned);
        if is_formula_candidate && report_domains.iter().any(|domain| domain == "support") {
            candidates_with_support_domain += 1;
        }
        if is_formula_candidate && primary_role.as_deref() == Some("supportive") {
            candidates_with_supportive_role += 1;
        }

        let relationship_components = value_array(contribution_row.get("relationshipComponents"));
        let mut value_proofs = proof_index
            .get(rule_id)
            .into_iter()
            .flat_map(|keys| keys.iter())
            .filter_map(|key| proofs.get(key).map(|row| build_value_proof(key, row)))
            .collect::<Vec<_>>();
        value_proofs.extend(exact_relationship_component_proofs(
            rule_id,
            &relationship_components,
        ));
        if is_formula_candidate && !value_proofs.is_empty() {
            candidates_with_value_proof += 1;
            candidate_value_proofs += value_proofs.len();
        }
        if is_formula_candidate
            && value_proofs
                .iter()
                .any(|proof| proof.selected_value_count > 0)
        {
            candidates_with_selected_values += 1;
        }

        let recount_row = recounts.get(rule_id);
        let runtime_matcher = build_runtime_matcher(recount_row);
        let (static_value_state, static_blockers) = if is_formula_candidate {
            static_value_state(&value_proofs)
        } else {
            ("not-required-for-packet-exact-produced-damage", Vec::new())
        };
        let transfer_eligibilities = string_array(contribution_row.get("transferEligibilities"));
        let rdps_enablement = rdps_enablement_for(&transfer_eligibilities);
        let candidate = Candidate {
            source_rule_id: rule_id.clone(),
            source_id: string_at(contribution_row, "sourceId").map(str::to_owned),
            primary_role,
            row_model: classification_row
                .and_then(|row| string_at(row, "rowModel"))
                .map(str::to_owned),
            report_domains,
            contribution_mode,
            contribution_tier: string_at(contribution_row, "contributionTier").map(str::to_owned),
            confidence: string_at(contribution_row, "confidence").map(str::to_owned),
            formula_term_ids: string_array(contribution_row.get("formulaTermIds")),
            formula_zone_ids: string_array(contribution_row.get("formulaZoneIds")),
            contribution_groups: string_array(contribution_row.get("contributionGroups")),
            transfer_eligibilities,
            predicate_tags: string_array(contribution_row.get("predicateTags")),
            required_runtime_evidence: string_array(
                contribution_row.get("requiredRuntimeEvidence"),
            ),
            relationship_components,
            runtime_matcher,
            value_proofs,
            static_value_state,
            static_blockers,
            rdps_enablement,
        };
        if is_formula_candidate {
            formula_candidates.push(candidate);
        } else {
            exact_candidates.push(candidate);
        }
    }

    exact_candidates.sort_by(|left, right| left.source_rule_id.cmp(&right.source_rule_id));
    formula_candidates.sort_by(|left, right| left.source_rule_id.cmp(&right.source_rule_id));
    let exact_candidates_with_runtime_matcher = exact_candidates
        .iter()
        .filter(|candidate| candidate.runtime_matcher.has_identity())
        .count();
    let formula_candidates_with_runtime_matcher = formula_candidates
        .iter()
        .filter(|candidate| candidate.runtime_matcher.has_identity())
        .count();
    let formula_candidates_without_value_proof = formula_candidates
        .iter()
        .filter(|candidate| candidate.value_proofs.is_empty())
        .count();
    let worklist = Worklist {
        schema_version: SCHEMA_VERSION,
        generated_by: "rlogs-bpsr-static-rdps-worklist",
        game_build,
        promotion_state: "candidate-only-not-runtime-authority",
        policy: Policy {
            static_metadata_enables_rdps: false,
            packet_replay_required: true,
            exact_provider_recipient_required: true,
            exact_party_conservation_required: true,
            unresolved_evidence_hidden: false,
        },
        inputs: Inputs {
            classification: input_artifact(&classification_path)?,
            contribution: input_artifact(&contribution_path)?,
            recount: input_artifact(&recount_path)?,
            value_proof: input_artifact(&value_proof_path)?,
        },
        summary: Summary {
            classification_rules: classifications.len(),
            contribution_rules: contributions.len(),
            formula_replay_candidates: formula_candidates.len(),
            exact_produced_damage_rules: mode_count(&mode_counts, "exact-produced-damage"),
            overlap_only_rules: mode_count(&mode_counts, "overlap-only"),
            timing_only_rules: mode_count(&mode_counts, "timing-only"),
            defensive_rules: mode_count(&mode_counts, "defensive"),
            candidate_rules_with_value_proof: candidates_with_value_proof,
            candidate_value_proofs,
            candidate_rules_with_selected_values: candidates_with_selected_values,
            candidate_rules_with_support_domain: candidates_with_support_domain,
            candidate_rules_with_supportive_role: candidates_with_supportive_role,
            exact_candidates_with_runtime_matcher,
            formula_candidates_with_runtime_matcher,
            formula_candidates_without_value_proof,
            automatically_enabled_for_rdps: 0,
        },
        exact_produced_damage_candidates: exact_candidates,
        formula_replay_candidates: formula_candidates,
    };

    if let Some(path) = watchlist_output_path {
        let buff_table_path = buff_table_path
            .as_ref()
            .ok_or("--buff-table is required when --watchlist-output is present")?;
        let buff_table = read_json(buff_table_path)?;
        let watchlist = magnitude_proof_watchlist(&worklist, buff_table_path, &buff_table)?;
        write_pretty_json(&path, &watchlist)?;
    }

    write_pretty_json(&output_path, &worklist)?;
    Ok(())
}

fn write_pretty_json(path: &Path, value: &impl Serialize) -> Result<(), Box<dyn Error>> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let mut writer = BufWriter::new(File::create(path)?);
    serde_json::to_writer_pretty(&mut writer, value)?;
    writer.write_all(b"\n")?;
    writer.flush()?;
    Ok(())
}

fn rdps_enablement_for(transfer_eligibilities: &[String]) -> &'static str {
    let has_external_candidate = transfer_eligibilities.iter().any(|value| {
        matches!(
            value.as_str(),
            "external-recipient-candidate" | "external-target-state-candidate"
        )
    });
    if has_external_candidate {
        return "blocked-pending-current-build-provider-recipient-replay";
    }
    if !transfer_eligibilities.is_empty()
        && transfer_eligibilities
            .iter()
            .all(|value| value == "self-only-formula-context")
    {
        return "not-transferable-self-only-formula-context";
    }
    if !transfer_eligibilities.is_empty()
        && transfer_eligibilities
            .iter()
            .all(|value| value == "direct-output-owned-by-source")
    {
        return "not-transferable-direct-output-owned-by-source";
    }
    "blocked-pending-recipient-scope-and-current-build-packet-replay"
}

impl RuntimeMatcher {
    fn has_identity(&self) -> bool {
        self.source_entity_id.is_some()
            || !self.buff_ids.is_empty()
            || !self.target_damage_ids.is_empty()
            || !self.target_recount_ids.is_empty()
    }
}

fn magnitude_proof_watchlist(
    worklist: &Worklist,
    buff_table_path: &Path,
    buff_table: &Value,
) -> Result<MagnitudeProofWatchlist, Box<dyn Error>> {
    let buff_rows = buff_table
        .as_object()
        .ok_or("BuffTable must be an object keyed by effect ID")?;
    let packet_proof_candidates = worklist
        .formula_replay_candidates
        .iter()
        .collect::<Vec<_>>();
    let mut effect_ids = BTreeSet::new();
    let mut attribute_ids = BTreeSet::new();
    let mut candidates = Vec::with_capacity(packet_proof_candidates.len());
    for candidate in packet_proof_candidates {
        if candidate.runtime_matcher.buff_ids.is_empty() {
            return Err(format!(
                "{} lacks a packet-visible effect ID for magnitude proof",
                candidate.source_rule_id
            )
            .into());
        }
        let mut candidate_attribute_ids = BTreeSet::new();
        for term in &candidate.formula_term_ids {
            add_formula_term_attributes(term, &mut candidate_attribute_ids)?;
        }
        if candidate
            .required_runtime_evidence
            .iter()
            .any(|evidence| evidence.contains("cast timeline"))
        {
            // Packet-final HastePct. Raw Haste rating is not substituted.
            candidate_attribute_ids.insert(11_930);
        }
        attribute_ids.extend(candidate_attribute_ids.iter().copied());
        let mut verified_effect_ids = Vec::new();
        let mut rejected_effect_references = Vec::new();
        let mut lifecycle_effects = Vec::new();
        for effect_id in &candidate.runtime_matcher.buff_ids {
            if buff_rows.contains_key(&effect_id.to_string()) {
                verified_effect_ids.push(*effect_id);
                lifecycle_effects.push(magnitude_proof_lifecycle_effect(buff_rows, *effect_id)?);
            } else {
                rejected_effect_references.push(RejectedEffectReference {
                    effect_id: *effect_id,
                    reason: "generated numeric reference is not an exact current-build BuffTable row and is retained as non-status evidence",
                });
            }
        }
        if verified_effect_ids.is_empty() {
            return Err(format!(
                "{} has no exact current-build BuffTable effect after reference validation",
                candidate.source_rule_id
            )
            .into());
        }
        effect_ids.extend(verified_effect_ids.iter().copied());
        candidates.push(MagnitudeProofCandidate {
            source_rule_id: candidate.source_rule_id.clone(),
            source_id: candidate.source_id.clone(),
            declared_effect_references: candidate.runtime_matcher.buff_ids.clone(),
            effect_ids: verified_effect_ids,
            rejected_effect_references,
            formula_terms: candidate.formula_term_ids.clone(),
            transfer_eligibilities: candidate.transfer_eligibilities.clone(),
            selected_attribute_ids: candidate_attribute_ids.into_iter().collect(),
            required_runtime_evidence: candidate.required_runtime_evidence.clone(),
            static_value_state: candidate.static_value_state.to_owned(),
            static_value_proofs: candidate.value_proofs.clone(),
            static_blockers: candidate.static_blockers.clone(),
            lifecycle_effects,
        });
    }
    let effect_ids = effect_ids.into_iter().collect::<Vec<_>>();
    Ok(MagnitudeProofWatchlist {
        schema_version: 3,
        generated_by: "rlogs-bpsr-static-rdps-worklist",
        deployment_id: "global",
        game_build: worklist.game_build.clone(),
        source_inputs: worklist.inputs.clone(),
        buff_table: input_artifact(buff_table_path)?,
        promotion_state: "packet-proof-required-runtime-disabled",
        policy: MagnitudeProofPolicy {
            game_tables_prove_identity_and_semantics_only: true,
            static_values_are_validation_expectations_not_runtime_authority: true,
            packet_lifecycle_proves_occurrence_provider_recipient_and_magnitude: true,
            application_and_removal_must_be_reversible: true,
            multiple_independent_instances_required: true,
            exact_party_damage_conservation_required_before_runtime_credit: true,
            selected_attributes_are_formula_context_not_credit_authority: true,
            self_only_attributes_never_create_external_credit: true,
            unresolved_evidence_is_hidden: false,
            runtime_enabled: false,
        },
        after_window_micros: 250_000,
        example_limit: 8,
        selected_effect_ids: effect_ids.clone(),
        reported_effect_ids: effect_ids,
        selected_attribute_ids: attribute_ids.into_iter().collect(),
        non_attributable_context_attribute_ids: self_only_damage_context_attribute_ids(),
        stateful_attribute_ids: vec![11_310, 20_010],
        candidates,
    })
}

fn magnitude_proof_lifecycle_effect(
    buff_rows: &Map<String, Value>,
    effect_id: i64,
) -> Result<MagnitudeProofLifecycleEffect, Box<dyn Error>> {
    let key = effect_id.to_string();
    let row = buff_rows
        .get(&key)
        .and_then(Value::as_object)
        .ok_or_else(|| format!("BuffTable lacks exact effect row {effect_id}"))?;
    if row.get("Id").and_then(Value::as_i64) != Some(effect_id) {
        return Err(format!("BuffTable row {effect_id} has a mismatched Id").into());
    }
    let repeat_add_rule = integer_array(row.get("RepeatAddRule"));
    // A zero second RepeatAddRule lane means the table does not declare a
    // positive stack cap. Preserve the raw rule for auditability, but do not
    // serialize zero as a maximum: the proof reader intentionally accepts
    // only positive declared caps and treats their absence as binary uptime.
    let declared_max_stacks = repeat_add_rule.get(1).copied().filter(|value| *value > 0);
    let proof_model = if declared_max_stacks.is_some_and(|value| value > 1) {
        "exact-stack-delta"
    } else {
        "exact-binary-presence"
    };
    Ok(MagnitudeProofLifecycleEffect {
        effect_id,
        name: row.get("Name").and_then(Value::as_str).map(str::to_owned),
        icon: row.get("Icon").and_then(Value::as_str).map(str::to_owned),
        repeat_add_rule,
        declared_max_stacks,
        proof_model,
        destroy_param: row.get("DestroyParam").cloned().unwrap_or(Value::Null),
    })
}

fn add_formula_term_attributes(
    term: &str,
    attributes: &mut BTreeSet<i32>,
) -> Result<(), Box<dyn Error>> {
    match term {
        "adaptivePrimaryStat" => {
            attributes.extend(11_010..=11_015);
            attributes.extend(11_020..=11_025);
            attributes.extend(11_030..=11_035);
        }
        "primaryAttack" => {
            attributes.extend(11_330..=11_335);
            attributes.extend(11_340..=11_345);
        }
        "sourceHpBasis" | "recipientHpBasis" => {
            // CurrentHP is packet-observed as exact EAttrType 11310. Max HP
            // uses the complete current-client six-member 11320-11325 family.
            // The formula term determines whether these attributes are read
            // from the source or the effect recipient; keeping distinct terms
            // prevents recipient-scaled heals/shields from being attributed to
            // the provider's HP. Missing HP and threshold state are derived
            // from those retained values and must not be substituted with a
            // description scalar.
            attributes.insert(11_310);
            attributes.extend(11_320..=11_325);
        }
        "sourceArmorBasis" => {
            // Current-build EAttrType and FightAttrTable agree on the complete
            // six-member physical Armor/Defense (11350-11355) and magic
            // defense (11360-11365) families. The contribution component's
            // reviewed stat label selects the applicable family; retaining
            // both here keeps the packet proof surface complete without
            // treating one defense family as the other.
            attributes.extend(11_350..=11_355);
            attributes.extend(11_360..=11_365);
        }
        "elementalAttack" => {
            // Exact current-client EAttrType families for generic elemental
            // attack plus Fire, Water, Wood, Electricity, Wind, Rock, Light,
            // and Dark attack. The contribution component's reviewed `stat`
            // narrows the applicable family; this watch surface deliberately
            // does not fall back to the unrelated primary-attack attributes.
            attributes.extend(11_500..=11_505);
            for base in (11_510..=11_580).step_by(10) {
                attributes.extend(base..=base + 5);
            }
        }
        "luckStatPct" => attributes.extend(11_130..=11_135),
        "luckyChancePct" => attributes.extend(11_780..=11_785),
        "luckyDamagePct" => attributes.extend(12_530..=12_535),
        "targetArmorMitigation" => {
            attributes.extend(11_350..=11_355);
            attributes.extend(11_360..=11_365);
        }
        "resistance" => {
            for base in (13_200..=13_280).step_by(10) {
                attributes.extend(base..=base + 5);
            }
            for base in (13_310..=13_390).step_by(10) {
                attributes.extend(base..=base + 5);
            }
        }
        "versatilityDamagePct" => {
            attributes.extend(11_950..=11_955);
            add_outgoing_damage_attributes(attributes);
        }
        "critMultiplier" => {
            attributes.extend(11_710..=11_715);
            attributes.extend(12_510..=12_515);
        }
        "elementalDamagePct" => {
            for base in (13_100..=13_180).step_by(10) {
                attributes.extend(base..=base + 5);
            }
        }
        "finalDamagePct" => add_outgoing_damage_attributes(attributes),
        "genericDamagePct" => {
            add_outgoing_damage_attributes(attributes);
        }
        "masteryStat" => {
            attributes.extend(11_140..=11_145);
            attributes.extend(11_940..=11_945);
        }
        "seasonDamagePct" => attributes.extend(12_690..=12_695),
        // `hitTiming` is the current extractor contract name. Keep the older
        // `actionTiming` spelling readable for already-recorded research
        // artifacts, but generate current-build watchlists from `hitTiming`.
        "hitTiming" | "actionTiming" => {
            attributes.extend(11_930..=11_935);
        }
        unsupported => {
            return Err(format!(
                "formula term {unsupported} has no reviewed packet attribute watch surface"
            )
            .into());
        }
    }
    Ok(())
}

fn add_outgoing_damage_attributes(attributes: &mut BTreeSet<i32>) {
    // Exact current-client EAttrType families that can participate in outgoing
    // damage. These are formula-state inputs only: packet status lifecycles and
    // provider/recipient identity remain the authority for external rDPS credit.
    for range in [
        11_830..=11_835,
        11_840..=11_845,
        11_860..=11_865,
        11_870..=11_875,
        11_880..=11_885,
        11_900..=11_905,
        12_550..=12_555,
        12_570..=12_575,
        12_590..=12_595,
        12_610..=12_615,
        12_630..=12_635,
        12_650..=12_655,
        12_670..=12_675,
        12_690..=12_695,
        12_710..=12_715,
        12_730..=12_735,
        12_750..=12_755,
        12_790..=12_795,
        12_800..=12_805,
    ] {
        attributes.extend(range);
    }
    for base in (13_100..=13_180).step_by(10) {
        attributes.extend(base..=base + 5);
    }
}

fn self_only_damage_context_attribute_ids() -> Vec<i32> {
    // EAttrType::AttrDpsOwnEffectStr and its packet variants describe the
    // recipient's own-effect strength. They are necessary replay context but
    // must never be interpreted as an externally supplied modifier.
    (11_860..=11_865).collect()
}

fn build_runtime_matcher(row: Option<&Value>) -> RuntimeMatcher {
    RuntimeMatcher {
        source_kind: row
            .and_then(|value| string_at(value, "sourceKind"))
            .map(str::to_owned),
        source_type: row
            .and_then(|value| string_at(value, "sourceType"))
            .map(str::to_owned),
        source_entity_id: row
            .and_then(|value| value.get("sourceEntityId"))
            .and_then(Value::as_i64),
        runtime_detection: row
            .and_then(|value| string_at(value, "runtimeDetection"))
            .map(str::to_owned),
        buff_ids: integer_array(row.and_then(|value| value.get("buffIds"))),
        target_damage_ids: integer_array(row.and_then(|value| value.get("targetDamageIds"))),
        target_recount_ids: integer_array(row.and_then(|value| value.get("targetRecountIds"))),
    }
}

fn static_value_state(proofs: &[ValueProof]) -> (&'static str, Vec<String>) {
    if proofs.is_empty() {
        return (
            "missing-value-proof",
            vec!["no generated value proof is linked to this source rule".to_owned()],
        );
    }
    let selected = proofs
        .iter()
        .map(|proof| proof.selected_value_count)
        .sum::<usize>();
    let selectors = proofs
        .iter()
        .map(|proof| proof.value_selector_count)
        .sum::<usize>();
    let mut blockers = proofs
        .iter()
        .flat_map(|proof| {
            proof
                .blockers
                .iter()
                .chain(proof.value_blockers.iter())
                .cloned()
        })
        .collect::<BTreeSet<_>>();
    if selected == 0 {
        if selectors > 0 {
            blockers.insert("runtime selector evidence must choose one candidate value".to_owned());
            ("runtime-selector-present", blockers.into_iter().collect())
        } else {
            blockers.insert("no exact formula component value is selected".to_owned());
            ("needs-value-selection", blockers.into_iter().collect())
        }
    } else if blockers.is_empty() {
        ("selected-values-present", Vec::new())
    } else {
        (
            "selected-values-with-blockers",
            blockers.into_iter().collect(),
        )
    }
}

fn index_value_proofs(proofs: &Map<String, Value>) -> BTreeMap<String, BTreeSet<String>> {
    let mut index = BTreeMap::<String, BTreeSet<String>>::new();
    for (key, proof) in proofs {
        for rule_id in string_array(proof.get("sourceRuleIds")) {
            index.entry(rule_id).or_default().insert(key.clone());
        }
    }
    index
}

fn build_value_proof(key: &str, row: &Value) -> ValueProof {
    let selected_values = value_array(row.get("selectedValues"));
    let value_selectors = value_array(row.get("valueSelectors"));
    ValueProof {
        key: key.to_owned(),
        formula_readiness: string_at(row, "formulaReadiness").map(str::to_owned),
        value_proof_status: string_at(row, "valueProofStatus").map(str::to_owned),
        selected_value_count: selected_values.len(),
        selected_values,
        value_selector_count: value_selectors.len(),
        value_selectors,
        proof_requirements: string_array(row.get("proofRequirements")),
        value_blockers: string_array(row.get("valueBlockers")),
        blockers: string_array(row.get("blockers")),
    }
}

fn exact_relationship_component_proofs(
    source_rule_id: &str,
    components: &[Value],
) -> Vec<ValueProof> {
    components
        .iter()
        .enumerate()
        .filter_map(|(index, component)| {
            if string_at(component, "valueResolution") != Some("single")
                || component
                    .get("proofBinding")
                    .and_then(Value::as_object)
                    .is_none()
                || string_at(component, "valueTextSource").is_none()
            {
                return None;
            }

            let component_key = string_at(component, "componentKey")?;
            let mut selected_values = value_array(component.get("values"));
            if selected_values.len() != 1
                || !selected_values
                    .iter()
                    .all(exact_component_value_is_complete)
            {
                return None;
            }
            for value in &mut selected_values {
                if let Some(object) = value.as_object_mut() {
                    object.insert(
                        "componentKey".to_owned(),
                        Value::String(component_key.to_owned()),
                    );
                    object.insert(
                        "sourceRuleId".to_owned(),
                        Value::String(source_rule_id.to_owned()),
                    );
                }
            }

            Some(ValueProof {
                key: format!("relationship-component:{source_rule_id}:{index}"),
                formula_readiness: Some("formula-replay-required".to_owned()),
                value_proof_status: Some("exact-bound-relationship-component".to_owned()),
                selected_value_count: selected_values.len(),
                selected_values,
                value_selector_count: 0,
                value_selectors: Vec::new(),
                proof_requirements: string_array(component.get("requiredRuntimeEvidence")),
                value_blockers: Vec::new(),
                blockers: Vec::new(),
            })
        })
        .collect()
}

fn exact_component_value_is_complete(value: &Value) -> bool {
    string_at(value, "rawText").is_some()
        && string_at(value, "scope").is_some()
        && string_at(value, "unit").is_some()
        && value.get("value").and_then(Value::as_f64).is_some()
}

fn mode_count(counts: &BTreeMap<String, usize>, key: &str) -> usize {
    counts.get(key).copied().unwrap_or_default()
}

fn require_generator(value: &Value, expected: &str) -> Result<(), Box<dyn Error>> {
    if string_at(value, "generatedBy") != Some(expected) {
        return Err(format!("input was not generated by {expected}").into());
    }
    Ok(())
}

fn require_schema(value: &Value, expected: u64) -> Result<(), Box<dyn Error>> {
    if value.get("schemaVersion").and_then(Value::as_u64) != Some(expected) {
        return Err(format!("unsupported input schema; expected {expected}").into());
    }
    Ok(())
}

fn object_at<'a>(value: &'a Value, key: &str) -> Result<&'a Map<String, Value>, Box<dyn Error>> {
    value
        .get(key)
        .and_then(Value::as_object)
        .ok_or_else(|| format!("missing object {key}").into())
}

fn string_at<'a>(value: &'a Value, key: &str) -> Option<&'a str> {
    value.get(key).and_then(Value::as_str)
}

fn string_array(value: Option<&Value>) -> Vec<String> {
    value
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .map(str::to_owned)
        .collect()
}

fn integer_array(value: Option<&Value>) -> Vec<i64> {
    value
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(Value::as_i64)
        .collect()
}

fn value_array(value: Option<&Value>) -> Vec<Value> {
    value.and_then(Value::as_array).cloned().unwrap_or_default()
}

fn file_name(path: &Path) -> String {
    path.file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("external-artifact")
        .to_owned()
}

fn input_artifact(path: &Path) -> Result<InputArtifact, Box<dyn Error>> {
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
        file: file_name(path),
        bytes,
        sha256: format!("{:x}", hasher.finalize()),
    })
}

fn read_json(path: &Path) -> Result<Value, Box<dyn Error>> {
    Ok(serde_json::from_reader(BufReader::new(File::open(path)?))?)
}

fn parse_options(arguments: &[String]) -> Result<BTreeMap<String, String>, Box<dyn Error>> {
    let mut options = BTreeMap::new();
    let mut index = 0usize;
    while index < arguments.len() {
        let key = arguments[index]
            .strip_prefix("--")
            .ok_or_else(usage)?
            .to_owned();
        let value = arguments.get(index + 1).ok_or_else(usage)?.to_owned();
        if options.insert(key, value).is_some() {
            return Err("duplicate option".into());
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
        .ok_or_else(|| usage().into())
}

fn required_path(options: &BTreeMap<String, String>, key: &str) -> Result<PathBuf, Box<dyn Error>> {
    Ok(PathBuf::from(required(options, key)?))
}

fn optional_path(options: &BTreeMap<String, String>, key: &str) -> Option<PathBuf> {
    options.get(key).map(PathBuf::from)
}

fn validate_build(build: &str) -> Result<(), Box<dyn Error>> {
    if build.is_empty() || !build.bytes().all(|byte| byte.is_ascii_digit()) {
        return Err("build must contain ASCII digits only".into());
    }
    Ok(())
}

fn usage() -> &'static str {
    "usage: rlogs-bpsr-static-rdps-worklist --classification <ModifierClassificationRuntime.json> --contribution <ModifierContributionRuntime.json> --recount <ModifierRecountTable.json> --value-proof <ModifierValueProofRuntime.json> --build <client-build> --output <worklist.json> [--watchlist-output <build-locked-watchlist.json> --buff-table <current-build-BuffTable.json>]"
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn value_proofs_index_every_rule() {
        let proofs = serde_json::json!({
            "entry": {"sourceRuleIds": ["a", "b"]}
        });
        let index = index_value_proofs(proofs.as_object().unwrap());
        assert!(index["a"].contains("entry"));
        assert!(index["b"].contains("entry"));
    }

    #[test]
    fn missing_value_proof_remains_an_explicit_blocker() {
        let (state, blockers) = static_value_state(&[]);
        assert_eq!(state, "missing-value-proof");
        assert_eq!(blockers.len(), 1);
    }

    #[test]
    fn runtime_matcher_accepts_packet_visible_ids() {
        let row = serde_json::json!({
            "sourceEntityId": 42,
            "buffIds": [7],
            "targetDamageIds": [8],
            "targetRecountIds": [9]
        });
        let matcher = build_runtime_matcher(Some(&row));
        assert!(matcher.has_identity());
        assert_eq!(matcher.buff_ids, vec![7]);
    }

    #[test]
    fn proof_watch_surfaces_are_explicit_and_fail_closed() {
        let mut attributes = BTreeSet::new();
        add_formula_term_attributes("adaptivePrimaryStat", &mut attributes).unwrap();
        add_formula_term_attributes("primaryAttack", &mut attributes).unwrap();
        add_formula_term_attributes("sourceHpBasis", &mut attributes).unwrap();
        add_formula_term_attributes("recipientHpBasis", &mut attributes).unwrap();
        add_formula_term_attributes("sourceArmorBasis", &mut attributes).unwrap();
        add_formula_term_attributes("elementalAttack", &mut attributes).unwrap();
        add_formula_term_attributes("critMultiplier", &mut attributes).unwrap();
        add_formula_term_attributes("elementalDamagePct", &mut attributes).unwrap();
        add_formula_term_attributes("finalDamagePct", &mut attributes).unwrap();
        add_formula_term_attributes("luckStatPct", &mut attributes).unwrap();
        add_formula_term_attributes("luckyChancePct", &mut attributes).unwrap();
        add_formula_term_attributes("luckyDamagePct", &mut attributes).unwrap();
        add_formula_term_attributes("masteryStat", &mut attributes).unwrap();
        add_formula_term_attributes("resistance", &mut attributes).unwrap();
        add_formula_term_attributes("seasonDamagePct", &mut attributes).unwrap();
        add_formula_term_attributes("hitTiming", &mut attributes).unwrap();
        add_formula_term_attributes("actionTiming", &mut attributes).unwrap();
        assert!(attributes.contains(&11_010));
        assert!(attributes.contains(&11_035));
        assert!(attributes.contains(&11_130));
        assert!(attributes.contains(&11_135));
        assert!(attributes.contains(&11_330));
        assert!(attributes.contains(&11_345));
        assert!(attributes.contains(&11_310));
        assert!(attributes.contains(&11_320));
        assert!(attributes.contains(&11_325));
        assert!(attributes.contains(&11_350));
        assert!(attributes.contains(&11_355));
        assert!(attributes.contains(&11_360));
        assert!(attributes.contains(&11_365));
        assert!(attributes.contains(&11_500));
        assert!(attributes.contains(&11_505));
        assert!(attributes.contains(&11_510));
        assert!(attributes.contains(&11_515));
        assert!(attributes.contains(&11_580));
        assert!(attributes.contains(&11_585));
        assert!(attributes.contains(&11_710));
        assert!(attributes.contains(&11_715));
        assert!(attributes.contains(&11_785));
        assert!(attributes.contains(&11_830));
        assert!(attributes.contains(&11_860));
        assert!(attributes.contains(&11_900));
        assert!(attributes.contains(&11_935));
        assert!(attributes.contains(&11_945));
        assert!(attributes.contains(&12_515));
        assert!(attributes.contains(&12_535));
        assert!(attributes.contains(&12_695));
        assert!(attributes.contains(&12_795));
        assert!(attributes.contains(&12_805));
        assert!(attributes.contains(&13_185));
        assert!(attributes.contains(&13_285));
        assert!(attributes.contains(&13_395));
        assert!(add_formula_term_attributes("unreviewedFutureTerm", &mut attributes).is_err());
    }

    #[test]
    fn generic_damage_context_is_complete_without_conflating_lucky_damage() {
        let mut attributes = BTreeSet::new();
        add_formula_term_attributes("genericDamagePct", &mut attributes).unwrap();

        for expected in [
            11_830, 11_840, 11_860, 11_870, 11_880, 11_900, 12_550, 12_590, 12_610, 12_650, 12_790,
            12_800, 13_100, 13_110, 13_120, 13_130, 13_140, 13_150, 13_160, 13_170, 13_180,
        ] {
            assert!(
                attributes.contains(&expected),
                "missing attribute {expected}"
            );
        }
        assert!(!attributes.contains(&12_530));
        assert_eq!(
            self_only_damage_context_attribute_ids(),
            vec![11_860, 11_861, 11_862, 11_863, 11_864, 11_865]
        );
    }

    #[test]
    fn buff_table_repeat_rule_selects_exact_stack_proof_without_guessing() {
        let rows = serde_json::json!({
            "2110077": {
                "Id": 2110077,
                "Name": "Intimidation",
                "Icon": "ui/atlas/hud/buff/buff_blue_atk",
                "RepeatAddRule": [2, 10],
                "DestroyParam": [[0.0, 10.0]]
            }
        });
        let lifecycle =
            magnitude_proof_lifecycle_effect(rows.as_object().unwrap(), 2_110_077).unwrap();
        assert_eq!(lifecycle.declared_max_stacks, Some(10));
        assert_eq!(lifecycle.proof_model, "exact-stack-delta");
        assert_eq!(lifecycle.repeat_add_rule, [2, 10]);
    }

    #[test]
    fn zero_repeat_rule_cap_is_binary_presence_without_a_declared_maximum() {
        let rows = serde_json::json!({
            "2205210": {
                "Id": 2205210,
                "Name": "Stackless effect",
                "Icon": "",
                "RepeatAddRule": [0, 0],
                "DestroyParam": []
            }
        });
        let lifecycle =
            magnitude_proof_lifecycle_effect(rows.as_object().unwrap(), 2_205_210).unwrap();
        assert_eq!(lifecycle.declared_max_stacks, None);
        assert_eq!(lifecycle.proof_model, "exact-binary-presence");
        assert_eq!(lifecycle.repeat_add_rule, [0, 0]);
    }

    #[test]
    fn runtime_value_ladder_is_preserved_for_replay() {
        let proof = ValueProof {
            key: "factor:1".to_owned(),
            formula_readiness: Some("formula-replay-required".to_owned()),
            value_proof_status: Some("needs-value-ladder-selector".to_owned()),
            selected_value_count: 0,
            selected_values: Vec::new(),
            value_selector_count: 1,
            value_selectors: vec![serde_json::json!({"kind": "runtime-value-ladder"})],
            proof_requirements: Vec::new(),
            value_blockers: vec!["runtime grade required".to_owned()],
            blockers: Vec::new(),
        };
        let (state, blockers) = static_value_state(&[proof]);
        assert_eq!(state, "runtime-selector-present");
        assert!(
            blockers
                .iter()
                .any(|value| value == "runtime grade required")
        );
    }

    #[test]
    fn exact_bound_relationship_component_supplies_value_proof() {
        let components = vec![serde_json::json!({
            "componentKey": "spring-breeze-max-hp",
            "valueResolution": "single",
            "valueTextSource": "SeasonEffectDescriptions exact BuffId join",
            "proofBinding": {"buffId": 2404260, "sourceTable": "CTB:4192598123"},
            "requiredRuntimeEvidence": ["active Buff 2404260"],
            "values": [{
                "decimalValue": 0.025,
                "rawText": "+2.5%",
                "scope": "effect-recipient",
                "unit": "percent",
                "value": 2.5
            }]
        })];

        let proofs = exact_relationship_component_proofs("mrs:test", &components);
        assert_eq!(proofs.len(), 1);
        assert_eq!(proofs[0].selected_value_count, 1);
        assert_eq!(
            string_at(&proofs[0].selected_values[0], "sourceRuleId"),
            Some("mrs:test")
        );
        assert_eq!(static_value_state(&proofs).0, "selected-values-present");
    }

    #[test]
    fn relationship_component_without_exact_binding_stays_unproven() {
        let components = vec![serde_json::json!({
            "componentKey": "unbound-value",
            "valueResolution": "single",
            "values": [{
                "rawText": "+2.5%",
                "scope": "owner",
                "unit": "percent",
                "value": 2.5
            }]
        })];

        assert!(exact_relationship_component_proofs("mrs:test", &components).is_empty());
    }
}
