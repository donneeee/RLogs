use std::{
    collections::{BTreeMap, BTreeSet},
    env,
    error::Error,
    fs::File,
    io::{BufReader, BufWriter, Write},
    path::{Path, PathBuf},
};

use serde::Serialize;
use serde_json::Value;

const SCHEMA_VERSION: u16 = 14;

#[derive(Debug, Serialize)]
#[serde(deny_unknown_fields)]
struct RecipientScopeLedger {
    schema_version: u16,
    generated_by: &'static str,
    static_game_build: String,
    historical_packet_build: String,
    promotion_state: &'static str,
    policy: Policy,
    inputs: Inputs,
    summary: Summary,
    candidates: Vec<ScopeCandidate>,
}

#[derive(Debug, Serialize)]
#[serde(deny_unknown_fields)]
struct Policy {
    unresolved_evidence_hidden: bool,
    static_description_proves_packet_recipient: bool,
    historical_scope_promotes_current_build: bool,
    mixed_component_scope_collapsed: bool,
    formula_components_gated_independently: bool,
    current_build_packet_lifecycle_required: bool,
    current_component_scope_can_refine_queue: bool,
    current_component_scope_enables_runtime_attribution: bool,
    packet_provider_recipient_overrides_static_scope: bool,
    self_provided_external_effect_transfers_credit: bool,
    provider_personal_damage_transfers_through_target_debuff: bool,
    purpose: &'static str,
}

#[derive(Debug, Serialize)]
#[serde(deny_unknown_fields)]
struct Inputs {
    static_worklist: String,
    magnitude_watchlist: String,
    semantic_audit: String,
    modifier_display: String,
    historical_packet_proof: String,
    historical_packet_inventory: String,
    historical_provider_mechanic_audit: String,
    exhaustive_remaining_provider_audit: String,
    historical_component_packet_proof: String,
    severed_chapter_effect_family_proof: String,
    severed_chapter_provider_audit: String,
    battle_cry_effect_family_proof: String,
    battle_cry_provider_audit: String,
    denvel_effect_family_proof: String,
    denvel_provider_audit: String,
    focused_shot_effect_family_proof: String,
    focused_shot_provider_audit: String,
    stellar_spark_effect_family_proof: String,
    stellar_spark_provider_audit: String,
    current_origin_ledger: String,
}

#[derive(Debug, Default, Serialize)]
#[serde(deny_unknown_fields)]
struct Summary {
    candidates: usize,
    formula_replay_candidates: usize,
    exact_produced_damage_candidates: usize,
    current_component_only_candidates: usize,
    candidates_with_multiple_transfer_labels: usize,
    candidates_observed_in_historical_packet_corpus: usize,
    candidates_with_historical_raw_proxy_source: usize,
    candidates_with_historical_external_player_provider: usize,
    candidates_eligible_for_current_build_promotion: usize,
    candidates_with_exact_current_component_scope: usize,
    declared_unresolved_resolved_by_current_component: usize,
    candidates_with_runtime_related_effects: usize,
    candidates_with_packet_observed_family_edges: usize,
    candidates_with_component_scope_routes: usize,
    candidates_with_mixed_component_scope_routes: usize,
    component_scope_routes: usize,
    transfer_eligibilities: BTreeMap<String, usize>,
    effective_transfer_eligibilities: BTreeMap<String, usize>,
    current_component_scopes: BTreeMap<String, usize>,
    scope_queues: BTreeMap<String, usize>,
    transfer_gate_kinds: BTreeMap<String, usize>,
    component_scope_queues: BTreeMap<String, usize>,
    component_transfer_gate_kinds: BTreeMap<String, usize>,
}

#[derive(Debug, Serialize)]
#[serde(deny_unknown_fields)]
struct ScopeCandidate {
    source_rule_id: String,
    source_id: Option<String>,
    source_name: Option<String>,
    description: Option<String>,
    contribution_mode: String,
    primary_role: Option<String>,
    report_domains: Vec<String>,
    formula_term_ids: Vec<String>,
    formula_zone_ids: Vec<String>,
    transfer_eligibilities: Vec<String>,
    effective_transfer_eligibilities: Vec<String>,
    component_scope_routes: Vec<ComponentScopeRoute>,
    scope_resolution: &'static str,
    scope_queue: &'static str,
    declared_effect_ids: Vec<i64>,
    runtime_related_effect_ids: Vec<i64>,
    effect_ids: Vec<i64>,
    runtime_effect_family_evidence: Vec<RuntimeEffectFamilyEvidence>,
    packet_observed_effect_family_edges: Vec<PacketObservedEffectFamilyEdge>,
    current_component_evidence: Vec<CurrentComponentEvidence>,
    historical_packet_evidence: HistoricalPacketEvidence,
    transfer_gate: TransferGate,
    current_build_promotion_eligible: bool,
    remaining_requirement: &'static str,
}

#[derive(Debug, Serialize)]
#[serde(deny_unknown_fields)]
struct ComponentScopeRoute {
    component_key: String,
    proof_binding: Option<Value>,
    effect_class: Option<String>,
    direction: Option<String>,
    contribution_scope: Option<String>,
    contribution_groups: Vec<String>,
    formula_term_ids: Vec<String>,
    declared_transfer_eligibility: String,
    transfer_eligibility: String,
    scope_queue: &'static str,
    rdps_relevance: &'static str,
    value_resolution: Option<String>,
    required_runtime_evidence: Vec<String>,
    transfer_gate: TransferGate,
    current_build_promotion_eligible: bool,
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(deny_unknown_fields)]
struct RuntimeEffectFamilyEvidence {
    proof_state: String,
    semantic_role: String,
    parent_effect_id: i64,
    child_effect_id: i64,
    historical_origin_observations: u64,
    historical_child_status_events: u64,
    historical_child_opened_windows: u64,
    historical_child_cross_actor_windows: u64,
    current_build_packet_lifecycle_observed: bool,
    formula_replay_allowed: bool,
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(deny_unknown_fields)]
struct PacketObservedEffectFamilyEdge {
    parent_effect_id: i64,
    child_effect_id: i64,
    source_type_id: i64,
    observation_count: u64,
    evidence_authority: &'static str,
}

#[derive(Debug, Serialize)]
#[serde(deny_unknown_fields)]
struct TransferGate {
    kind: &'static str,
    attribution_route: &'static str,
    authority: &'static str,
    runtime_credit_allowed: bool,
    required_current_build_evidence: Vec<&'static str>,
    forbidden_transfers: Vec<&'static str>,
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(deny_unknown_fields)]
struct CurrentComponentEvidence {
    skill_id: u64,
    skill_name: String,
    component_id: String,
    role: String,
    effect_ids: Vec<i64>,
    recipient_scope: String,
    rdps_disposition: String,
    proof_state: String,
}

#[derive(Clone, Debug, Default, Serialize)]
#[serde(deny_unknown_fields)]
struct HistoricalPacketEvidence {
    inventory_effect_rows_present: usize,
    inventory_status_events: u64,
    inventory_origin_observations: u64,
    compact_effect_rows_present: usize,
    compact_selected_status_events: u64,
    provider_audit_rows_present: usize,
    provider_audit_rows_with_status_events: usize,
    authoritative_status_events: u64,
    opened_windows: u64,
    cross_actor_windows: u64,
    raw_proxy_source_windows: u64,
    owner_linked_player_provider_windows: u64,
    non_player_provider_windows: u64,
    resolved_provider_is_recipient_examples: u64,
    resolved_provider_differs_from_recipient_examples: u64,
    source_missing_windows: u64,
    player_recipient_windows: u64,
    monster_recipient_windows: u64,
    provider_is_recipient_observed: bool,
    provider_differs_from_recipient_observed: bool,
    resolved_external_player_provider_observed: bool,
    provider_identity_unresolved_observed: bool,
    provider_resolutions: Vec<String>,
    evidence_authority: &'static str,
}

fn main() {
    if let Err(error) = run(env::args().skip(1).collect()) {
        eprintln!("{error}");
        std::process::exit(1);
    }
}

fn run(arguments: Vec<String>) -> Result<(), Box<dyn Error>> {
    let options = parse_options(&arguments)?;
    let worklist_path = required_path(&options, "worklist")?;
    let watchlist_path = required_path(&options, "watchlist")?;
    let semantic_path = required_path(&options, "semantic-audit")?;
    let display_path_input = required_path(&options, "display")?;
    let packet_proof_path = required_path(&options, "packet-proof")?;
    let packet_inventory_path = required_path(&options, "packet-inventory")?;
    let provider_audit_path = required_path(&options, "provider-audit")?;
    let exhaustive_provider_audit_path = required_path(&options, "exhaustive-provider-audit")?;
    let component_packet_proof_path = required_path(&options, "component-packet-proof")?;
    let severed_chapter_proof_path = required_path(&options, "severed-chapter-proof")?;
    let severed_chapter_audit_path = required_path(&options, "severed-chapter-audit")?;
    let battle_cry_proof_path = required_path(&options, "battle-cry-proof")?;
    let battle_cry_audit_path = required_path(&options, "battle-cry-audit")?;
    let denvel_proof_path = required_path(&options, "denvel-proof")?;
    let denvel_audit_path = required_path(&options, "denvel-audit")?;
    let focused_shot_proof_path = required_path(&options, "focused-shot-proof")?;
    let focused_shot_audit_path = required_path(&options, "focused-shot-audit")?;
    let stellar_spark_proof_path = required_path(&options, "stellar-spark-proof")?;
    let stellar_spark_audit_path = required_path(&options, "stellar-spark-audit")?;
    let origin_ledger_path = required_path(&options, "origin-ledger")?;
    let historical_packet_build = required(&options, "packet-build")?.to_owned();
    let output_path = required_path(&options, "output")?;
    validate_build(&historical_packet_build)?;

    let worklist = read_json(&worklist_path)?;
    let watchlist = read_json(&watchlist_path)?;
    let semantic = read_json(&semantic_path)?;
    let display = read_json(&display_path_input)?;
    let packet_proof = read_json(&packet_proof_path)?;
    let packet_inventory = read_json(&packet_inventory_path)?;
    let provider_audit = read_json(&provider_audit_path)?;
    let exhaustive_provider_audit = read_json(&exhaustive_provider_audit_path)?;
    let component_packet_proof = read_json(&component_packet_proof_path)?;
    let severed_chapter_proof = read_json(&severed_chapter_proof_path)?;
    let severed_chapter_audit = read_json(&severed_chapter_audit_path)?;
    let battle_cry_proof = read_json(&battle_cry_proof_path)?;
    let battle_cry_audit = read_json(&battle_cry_audit_path)?;
    let denvel_proof = read_json(&denvel_proof_path)?;
    let denvel_audit = read_json(&denvel_audit_path)?;
    let focused_shot_proof = read_json(&focused_shot_proof_path)?;
    let focused_shot_audit = read_json(&focused_shot_audit_path)?;
    let stellar_spark_proof = read_json(&stellar_spark_proof_path)?;
    let stellar_spark_audit = read_json(&stellar_spark_audit_path)?;
    let origin_ledger = read_json(&origin_ledger_path)?;
    require_generated_by(&worklist, "rlogs-bpsr-static-rdps-worklist")?;
    require_generated_by(&watchlist, "rlogs-bpsr-static-rdps-worklist")?;
    require_generated_by(&semantic, "rlogs-bpsr-static-rdps-semantic-audit")?;
    if string_at(&display, "generatedBy") != Some("ModifierDisplayTable.gen") {
        return Err("expected ModifierDisplayTable.gen input".into());
    }
    require_generated_by(&packet_proof, "rlogs-bpsr-rdps-status-proof-compact")?;
    if unsigned_at(&packet_inventory, "schema_version") != 1 {
        return Err("historical packet inventory schema must be 1".into());
    }
    require_generated_by(&provider_audit, "rlogs-bpsr-rdps-provider-mechanic-audit")?;
    require_generated_by(
        &exhaustive_provider_audit,
        "rlogs-bpsr-rdps-provider-mechanic-audit",
    )?;
    require_generated_by(
        &severed_chapter_proof,
        "rlogs-bpsr-severed-chapter-effect-family-proof",
    )?;
    require_generated_by(
        &severed_chapter_audit,
        "rlogs-bpsr-rdps-provider-mechanic-audit",
    )?;
    require_generated_by(
        &battle_cry_proof,
        "rlogs-bpsr-battle-cry-effect-family-proof",
    )?;
    require_generated_by(&battle_cry_audit, "rlogs-bpsr-rdps-provider-mechanic-audit")?;
    require_generated_by(&denvel_proof, "rlogs-bpsr-denvel-effect-family-proof")?;
    require_generated_by(&denvel_audit, "rlogs-bpsr-rdps-provider-mechanic-audit")?;
    require_generated_by(
        &focused_shot_proof,
        "rlogs-bpsr-focused-shot-effect-family-proof",
    )?;
    require_generated_by(
        &focused_shot_audit,
        "rlogs-bpsr-rdps-provider-mechanic-audit",
    )?;
    require_generated_by(
        &stellar_spark_proof,
        "rlogs-bpsr-stellar-spark-effect-family-proof",
    )?;
    require_generated_by(
        &stellar_spark_audit,
        "rlogs-bpsr-rdps-provider-mechanic-audit",
    )?;
    if unsigned_at(&origin_ledger, "schema_version") < 14 {
        return Err("current origin ledger schema must be at least 14".into());
    }

    let static_game_build = string_at(&worklist, "game_build")
        .ok_or("static worklist game_build is missing")?
        .to_owned();
    if string_at(&watchlist, "game_build") != Some(static_game_build.as_str())
        || string_at(&semantic, "game_build") != Some(static_game_build.as_str())
        || string_at(&origin_ledger, "game_build") != Some(static_game_build.as_str())
        || string_at(&severed_chapter_proof, "current_game_build")
            != Some(static_game_build.as_str())
        || string_at(&battle_cry_proof, "current_game_build") != Some(static_game_build.as_str())
        || string_at(&denvel_proof, "current_game_build") != Some(static_game_build.as_str())
        || string_at(&focused_shot_proof, "current_game_build") != Some(static_game_build.as_str())
        || string_at(&stellar_spark_proof, "current_game_build") != Some(static_game_build.as_str())
    {
        return Err("worklist, watchlist, and semantic audit builds differ".into());
    }

    let watch_by_rule = array_at(&watchlist, "candidates")?
        .iter()
        .map(|row| {
            let rule = string_at(row, "source_rule_id")
                .ok_or("watchlist candidate source_rule_id is missing")?;
            Ok((rule.to_owned(), row))
        })
        .collect::<Result<BTreeMap<_, _>, Box<dyn Error>>>()?;
    let semantic_by_rule = array_at(&semantic, "findings")?
        .iter()
        .map(|row| {
            let rule = string_at(row, "source_rule_id")
                .ok_or("semantic finding source_rule_id is missing")?;
            Ok((rule.to_owned(), row))
        })
        .collect::<Result<BTreeMap<_, _>, Box<dyn Error>>>()?;
    let compact_by_effect = collect_packet_evidence(&packet_proof)?;
    let inventory_by_effect =
        collect_packet_inventory_occurrence(&packet_inventory, &historical_packet_build)?;
    let packet_observed_effect_families =
        collect_packet_observed_effect_families(&packet_inventory, &historical_packet_build)?;
    let mut provider_audit_by_effect = collect_provider_audit_evidence(&provider_audit)?;
    for (effect_id, evidence) in collect_provider_audit_evidence(&exhaustive_provider_audit)? {
        provider_audit_by_effect.insert(effect_id, evidence);
    }
    for (effect_id, evidence) in collect_provider_audit_evidence(&severed_chapter_audit)? {
        provider_audit_by_effect
            .entry(effect_id)
            .or_insert(evidence);
    }
    for (effect_id, evidence) in collect_provider_audit_evidence(&battle_cry_audit)? {
        provider_audit_by_effect
            .entry(effect_id)
            .or_insert(evidence);
    }
    for (effect_id, evidence) in collect_provider_audit_evidence(&denvel_audit)? {
        provider_audit_by_effect
            .entry(effect_id)
            .or_insert(evidence);
    }
    for (effect_id, evidence) in collect_provider_audit_evidence(&focused_shot_audit)? {
        provider_audit_by_effect
            .entry(effect_id)
            .or_insert(evidence);
    }
    for (effect_id, evidence) in collect_provider_audit_evidence(&stellar_spark_audit)? {
        provider_audit_by_effect
            .entry(effect_id)
            .or_insert(evidence);
    }
    let mut runtime_effect_families =
        collect_runtime_effect_families(&severed_chapter_proof, &historical_packet_build)?;
    merge_runtime_effect_families(
        &mut runtime_effect_families,
        collect_battle_cry_runtime_effect_families(&battle_cry_proof, &historical_packet_build)?,
    );
    merge_runtime_effect_families(
        &mut runtime_effect_families,
        collect_denvel_runtime_effect_families(&denvel_proof, &historical_packet_build)?,
    );
    merge_runtime_effect_families(
        &mut runtime_effect_families,
        collect_focused_shot_runtime_effect_families(
            &focused_shot_proof,
            &historical_packet_build,
        )?,
    );
    merge_runtime_effect_families(
        &mut runtime_effect_families,
        collect_stellar_spark_runtime_effect_families(
            &stellar_spark_proof,
            &historical_packet_build,
        )?,
    );
    let (component_effect_id, component_evidence) =
        collect_component_packet_evidence(&component_packet_proof, &historical_packet_build)?;
    provider_audit_by_effect
        .entry(component_effect_id)
        .or_insert(component_evidence);
    let current_components_by_effect = collect_current_component_evidence(&origin_ledger)?;

    let mut candidates = Vec::new();
    append_candidates(
        &mut candidates,
        array_at(&worklist, "formula_replay_candidates")?,
        &watch_by_rule,
        &semantic_by_rule,
        &display,
        &compact_by_effect,
        &inventory_by_effect,
        &provider_audit_by_effect,
        &current_components_by_effect,
        &runtime_effect_families,
        &packet_observed_effect_families,
    )?;
    append_candidates(
        &mut candidates,
        array_at(&worklist, "exact_produced_damage_candidates")?,
        &watch_by_rule,
        &semantic_by_rule,
        &display,
        &compact_by_effect,
        &inventory_by_effect,
        &provider_audit_by_effect,
        &current_components_by_effect,
        &runtime_effect_families,
        &packet_observed_effect_families,
    )?;
    append_component_only_candidates(
        &mut candidates,
        &current_components_by_effect,
        &compact_by_effect,
        &inventory_by_effect,
        &provider_audit_by_effect,
    );
    candidates.sort_by(|left, right| {
        left.scope_queue
            .cmp(right.scope_queue)
            .then_with(|| left.source_rule_id.cmp(&right.source_rule_id))
    });

    let mut summary = Summary {
        candidates: candidates.len(),
        formula_replay_candidates: candidates
            .iter()
            .filter(|candidate| candidate.contribution_mode == "formula-replay-candidate")
            .count(),
        exact_produced_damage_candidates: candidates
            .iter()
            .filter(|candidate| candidate.contribution_mode == "exact-produced-damage")
            .count(),
        current_component_only_candidates: candidates
            .iter()
            .filter(|candidate| candidate.contribution_mode == "current-component-proof-obligation")
            .count(),
        candidates_with_multiple_transfer_labels: candidates
            .iter()
            .filter(|candidate| candidate.transfer_eligibilities.len() > 1)
            .count(),
        candidates_observed_in_historical_packet_corpus: candidates
            .iter()
            .filter(|candidate| {
                candidate
                    .historical_packet_evidence
                    .authoritative_status_events
                    > 0
            })
            .count(),
        candidates_with_historical_raw_proxy_source: candidates
            .iter()
            .filter(|candidate| {
                candidate
                    .historical_packet_evidence
                    .raw_proxy_source_windows
                    > 0
            })
            .count(),
        candidates_with_historical_external_player_provider: candidates
            .iter()
            .filter(|candidate| {
                candidate
                    .historical_packet_evidence
                    .resolved_external_player_provider_observed
            })
            .count(),
        candidates_eligible_for_current_build_promotion: 0,
        candidates_with_exact_current_component_scope: candidates
            .iter()
            .filter(|candidate| !candidate.current_component_evidence.is_empty())
            .count(),
        declared_unresolved_resolved_by_current_component: candidates
            .iter()
            .filter(|candidate| candidate.scope_resolution == "exact-current-component-scope")
            .count(),
        candidates_with_runtime_related_effects: candidates
            .iter()
            .filter(|candidate| !candidate.runtime_related_effect_ids.is_empty())
            .count(),
        candidates_with_packet_observed_family_edges: candidates
            .iter()
            .filter(|candidate| !candidate.packet_observed_effect_family_edges.is_empty())
            .count(),
        candidates_with_component_scope_routes: candidates
            .iter()
            .filter(|candidate| !candidate.component_scope_routes.is_empty())
            .count(),
        candidates_with_mixed_component_scope_routes: candidates
            .iter()
            .filter(|candidate| candidate.scope_queue == "component-scoped-mixed")
            .count(),
        component_scope_routes: candidates
            .iter()
            .map(|candidate| candidate.component_scope_routes.len())
            .sum(),
        ..Summary::default()
    };
    for candidate in &candidates {
        for eligibility in &candidate.transfer_eligibilities {
            *summary
                .transfer_eligibilities
                .entry(eligibility.clone())
                .or_default() += 1;
        }
        for eligibility in &candidate.effective_transfer_eligibilities {
            *summary
                .effective_transfer_eligibilities
                .entry(eligibility.clone())
                .or_default() += 1;
        }
        for evidence in &candidate.current_component_evidence {
            *summary
                .current_component_scopes
                .entry(evidence.recipient_scope.clone())
                .or_default() += 1;
        }
        *summary
            .scope_queues
            .entry(candidate.scope_queue.to_owned())
            .or_default() += 1;
        *summary
            .transfer_gate_kinds
            .entry(candidate.transfer_gate.kind.to_owned())
            .or_default() += 1;
        for route in &candidate.component_scope_routes {
            *summary
                .component_scope_queues
                .entry(route.scope_queue.to_owned())
                .or_default() += 1;
            *summary
                .component_transfer_gate_kinds
                .entry(route.transfer_gate.kind.to_owned())
                .or_default() += 1;
        }
    }

    let ledger = RecipientScopeLedger {
        schema_version: SCHEMA_VERSION,
        generated_by: "rlogs-bpsr-rdps-recipient-scope-ledger",
        static_game_build,
        historical_packet_build,
        promotion_state: "blocked-pending-current-build-provider-recipient-lifecycle",
        policy: Policy {
            unresolved_evidence_hidden: false,
            static_description_proves_packet_recipient: false,
            historical_scope_promotes_current_build: false,
            mixed_component_scope_collapsed: false,
            formula_components_gated_independently: true,
            current_build_packet_lifecycle_required: true,
            current_component_scope_can_refine_queue: true,
            current_component_scope_enables_runtime_attribution: false,
            packet_provider_recipient_overrides_static_scope: true,
            self_provided_external_effect_transfers_credit: false,
            provider_personal_damage_transfers_through_target_debuff: false,
            purpose: "route every static rDPS candidate by recipient-scope evidence without enabling runtime attribution",
        },
        inputs: Inputs {
            static_worklist: display_path(&worklist_path),
            magnitude_watchlist: display_path(&watchlist_path),
            semantic_audit: display_path(&semantic_path),
            modifier_display: display_path(&display_path_input),
            historical_packet_proof: display_path(&packet_proof_path),
            historical_packet_inventory: display_path(&packet_inventory_path),
            historical_provider_mechanic_audit: display_path(&provider_audit_path),
            exhaustive_remaining_provider_audit: display_path(&exhaustive_provider_audit_path),
            historical_component_packet_proof: display_path(&component_packet_proof_path),
            severed_chapter_effect_family_proof: display_path(&severed_chapter_proof_path),
            severed_chapter_provider_audit: display_path(&severed_chapter_audit_path),
            battle_cry_effect_family_proof: display_path(&battle_cry_proof_path),
            battle_cry_provider_audit: display_path(&battle_cry_audit_path),
            denvel_effect_family_proof: display_path(&denvel_proof_path),
            denvel_provider_audit: display_path(&denvel_audit_path),
            focused_shot_effect_family_proof: display_path(&focused_shot_proof_path),
            focused_shot_provider_audit: display_path(&focused_shot_audit_path),
            stellar_spark_effect_family_proof: display_path(&stellar_spark_proof_path),
            stellar_spark_provider_audit: display_path(&stellar_spark_audit_path),
            current_origin_ledger: display_path(&origin_ledger_path),
        },
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

fn append_candidates<'a>(
    output: &mut Vec<ScopeCandidate>,
    rows: &'a [Value],
    watch_by_rule: &BTreeMap<String, &'a Value>,
    semantic_by_rule: &BTreeMap<String, &'a Value>,
    display: &Value,
    compact_by_effect: &BTreeMap<i64, HistoricalPacketEvidence>,
    inventory_by_effect: &BTreeMap<i64, HistoricalPacketEvidence>,
    provider_audit_by_effect: &BTreeMap<i64, HistoricalPacketEvidence>,
    current_components_by_effect: &BTreeMap<i64, Vec<CurrentComponentEvidence>>,
    runtime_effect_families: &BTreeMap<i64, Vec<RuntimeEffectFamilyEvidence>>,
    packet_observed_effect_families: &BTreeMap<i64, Vec<PacketObservedEffectFamilyEdge>>,
) -> Result<(), Box<dyn Error>> {
    for row in rows {
        let source_rule_id = string_at(row, "source_rule_id")
            .ok_or("worklist candidate source_rule_id is missing")?
            .to_owned();
        let watch = watch_by_rule.get(&source_rule_id).copied();
        let semantic = semantic_by_rule.get(&source_rule_id).copied();
        let declared_effect_ids = match watch {
            Some(watch) => integer_array_at(watch, "effect_ids")?,
            None if string_at(row, "contribution_mode") == Some("exact-produced-damage") => row
                .get("runtime_matcher")
                .map(|matcher| integer_array_at(matcher, "buff_ids"))
                .transpose()?
                .unwrap_or_default(),
            None => {
                return Err(
                    format!("watchlist is missing formula source rule {source_rule_id}").into(),
                );
            }
        };
        let runtime_effect_family_evidence = declared_effect_ids
            .iter()
            .flat_map(|effect_id| {
                runtime_effect_families
                    .get(effect_id)
                    .into_iter()
                    .flatten()
                    .cloned()
            })
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect::<Vec<_>>();
        let packet_observed_effect_family_edges = collect_reachable_packet_family_edges(
            &declared_effect_ids,
            packet_observed_effect_families,
        );
        let runtime_related_effect_ids = runtime_effect_family_evidence
            .iter()
            .map(|evidence| evidence.child_effect_id)
            .chain(
                packet_observed_effect_family_edges
                    .iter()
                    .map(|evidence| evidence.child_effect_id),
            )
            .filter(|effect_id| !declared_effect_ids.contains(effect_id))
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect::<Vec<_>>();
        let effect_ids = declared_effect_ids
            .iter()
            .chain(runtime_related_effect_ids.iter())
            .copied()
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect::<Vec<_>>();
        let transfer_eligibilities = string_array_at(row, "transfer_eligibilities")?;
        let current_component_evidence =
            merge_current_component_evidence(&effect_ids, current_components_by_effect);
        let (effective_transfer_eligibilities, scope_resolution) =
            resolve_transfer_eligibilities(&transfer_eligibilities, &current_component_evidence);
        let (effective_transfer_eligibilities, scope_resolution) =
            resolve_proven_runtime_family_scope(
                &effective_transfer_eligibilities,
                scope_resolution,
                &runtime_effect_family_evidence,
            );
        let historical_packet_evidence = merge_packet_evidence(
            &effect_ids,
            compact_by_effect,
            inventory_by_effect,
            provider_audit_by_effect,
        );
        let source_name = semantic
            .and_then(|value| optional_string_at(value, "source_name"))
            .or_else(|| display_source_name(display, &source_rule_id));
        let contribution_mode = string_at(row, "contribution_mode")
            .ok_or("worklist candidate contribution_mode is missing")?
            .to_owned();
        let component_scope_routes = reconcile_component_scope_routes(
            component_scope_routes(row, &contribution_mode)?,
            &effective_transfer_eligibilities,
            &contribution_mode,
        );
        let aggregate_scope_queue = scope_queue(&effective_transfer_eligibilities);
        let scope_queue = source_scope_queue(&component_scope_routes, aggregate_scope_queue);
        let scope_resolution = if scope_queue == "component-scoped-mixed" {
            "component-scoped-independent-routes"
        } else {
            scope_resolution
        };
        let transfer_gate = transfer_gate(scope_queue, &contribution_mode);
        output.push(ScopeCandidate {
            source_rule_id,
            source_id: optional_string_at(row, "source_id"),
            source_name,
            description: semantic.and_then(|value| optional_string_at(value, "description")),
            contribution_mode,
            primary_role: optional_string_at(row, "primary_role"),
            report_domains: string_array_at(row, "report_domains")?,
            formula_term_ids: string_array_at(row, "formula_term_ids")?,
            formula_zone_ids: string_array_at(row, "formula_zone_ids")?,
            scope_queue,
            transfer_eligibilities,
            effective_transfer_eligibilities,
            component_scope_routes,
            scope_resolution,
            effect_ids,
            declared_effect_ids,
            runtime_related_effect_ids,
            runtime_effect_family_evidence,
            packet_observed_effect_family_edges,
            current_component_evidence,
            historical_packet_evidence,
            transfer_gate,
            current_build_promotion_eligible: false,
            remaining_requirement: "satisfy every typed transfer_gate requirement with matching-build canonical packet evidence and conservation replay",
        });
    }
    Ok(())
}

fn append_component_only_candidates(
    output: &mut Vec<ScopeCandidate>,
    current_components_by_effect: &BTreeMap<i64, Vec<CurrentComponentEvidence>>,
    compact_by_effect: &BTreeMap<i64, HistoricalPacketEvidence>,
    inventory_by_effect: &BTreeMap<i64, HistoricalPacketEvidence>,
    provider_audit_by_effect: &BTreeMap<i64, HistoricalPacketEvidence>,
) {
    let existing_effect_ids = output
        .iter()
        .flat_map(|candidate| candidate.effect_ids.iter().copied())
        .collect::<BTreeSet<_>>();
    for (effect_id, evidence) in current_components_by_effect {
        if existing_effect_ids.contains(effect_id)
            || !evidence.iter().any(is_external_target_component)
        {
            continue;
        }
        let skill_names = evidence
            .iter()
            .filter(|row| is_external_target_component(row))
            .map(|row| row.skill_name.clone())
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect::<Vec<_>>();
        let historical_packet_evidence = merge_packet_evidence(
            &[*effect_id],
            compact_by_effect,
            inventory_by_effect,
            provider_audit_by_effect,
        );
        let transfer_eligibilities = vec!["external-target-state-candidate".to_string()];
        let scope_queue = scope_queue(&transfer_eligibilities);
        output.push(ScopeCandidate {
            source_rule_id: format!("current-component:effect:{effect_id}"),
            source_id: Some(format!("current-component-effect:{effect_id}")),
            source_name: Some(format!(
                "{} target status {effect_id}",
                skill_names.join(" / ")
            )),
            description: Some(
                "Exact current component identity retained outside the generic modifier worklist; formula and matching-build provider ownership remain mandatory."
                    .to_string(),
            ),
            contribution_mode: "current-component-proof-obligation".to_string(),
            primary_role: Some("supportive".to_string()),
            report_domains: vec!["support".to_string()],
            formula_term_ids: Vec::new(),
            formula_zone_ids: Vec::new(),
            transfer_eligibilities: transfer_eligibilities.clone(),
            effective_transfer_eligibilities: transfer_eligibilities,
            component_scope_routes: Vec::new(),
            scope_resolution: "exact-current-component-scope",
            scope_queue,
            declared_effect_ids: vec![*effect_id],
            runtime_related_effect_ids: Vec::new(),
            effect_ids: vec![*effect_id],
            runtime_effect_family_evidence: Vec::new(),
            packet_observed_effect_family_edges: Vec::new(),
            current_component_evidence: evidence.clone(),
            historical_packet_evidence,
            transfer_gate: transfer_gate(scope_queue, "current-component-proof-obligation"),
            current_build_promotion_eligible: false,
            remaining_requirement: "prove matching-build packet provider ownership, exact target mitigation formula and magnitude, then conserve party damage in counterfactual replay",
        });
    }
}

fn is_external_target_component(evidence: &CurrentComponentEvidence) -> bool {
    matches!(
        evidence.role.as_str(),
        "transferable-external-target-mitigation"
            | "conditional-shared-projectile-external-target-mitigation"
    )
}

fn collect_runtime_effect_families(
    proof: &Value,
    historical_packet_build: &str,
) -> Result<BTreeMap<i64, Vec<RuntimeEffectFamilyEvidence>>, Box<dyn Error>> {
    if string_at(proof, "historical_packet_build") != Some(historical_packet_build) {
        return Err("Severed Chapter proof historical packet build differs from the ledger".into());
    }
    let current = proof
        .get("current_static")
        .ok_or("Severed Chapter proof current_static is missing")?;
    let origin = proof
        .get("historical_origin_edge")
        .ok_or("Severed Chapter proof historical_origin_edge is missing")?;
    let child_lifecycle = proof
        .get("historical_lifecycle")
        .and_then(|value| value.get("child"))
        .ok_or("Severed Chapter proof child lifecycle is missing")?;
    let policy = proof
        .get("attribution_policy")
        .ok_or("Severed Chapter proof attribution_policy is missing")?;
    let parent_effect_id = integer_at(current, "parent_effect_id")?;
    let child_effect_id = integer_at(current, "child_effect_id")?;
    if integer_at(origin, "effect_id")? != child_effect_id
        || integer_at(origin, "source_config_id")? != parent_effect_id
        || integer_at(origin, "source_type_id")? != 1
    {
        return Err("Severed Chapter runtime child origin edge is inconsistent".into());
    }
    let evidence = RuntimeEffectFamilyEvidence {
        proof_state: string_at(proof, "proof_state")
            .ok_or("Severed Chapter proof_state is missing")?
            .to_owned(),
        semantic_role: "runtime-child-heroic-melody-branch".to_owned(),
        parent_effect_id,
        child_effect_id,
        historical_origin_observations: unsigned_at(origin, "observation_count"),
        historical_child_status_events: unsigned_at(child_lifecycle, "status_events"),
        historical_child_opened_windows: unsigned_at(child_lifecycle, "opened_windows"),
        historical_child_cross_actor_windows: unsigned_at(child_lifecycle, "cross_actor_windows"),
        current_build_packet_lifecycle_observed: bool_at(
            policy,
            "current_build_packet_lifecycle_observed",
        )?,
        formula_replay_allowed: bool_at(policy, "formula_replay_allowed")?,
    };
    if evidence.historical_origin_observations == 0
        || evidence.historical_child_status_events == 0
        || evidence.historical_child_cross_actor_windows == 0
        || evidence.current_build_packet_lifecycle_observed
        || evidence.formula_replay_allowed
    {
        return Err(
            "Severed Chapter proof does not preserve the required fail-closed state".into(),
        );
    }
    Ok(BTreeMap::from([(parent_effect_id, vec![evidence])]))
}

fn collect_battle_cry_runtime_effect_families(
    proof: &Value,
    historical_packet_build: &str,
) -> Result<BTreeMap<i64, Vec<RuntimeEffectFamilyEvidence>>, Box<dyn Error>> {
    if string_at(proof, "historical_packet_build") != Some(historical_packet_build) {
        return Err("Battle Cry proof historical packet build differs from the ledger".into());
    }
    let current = proof
        .get("current_static")
        .ok_or("Battle Cry proof current_static is missing")?;
    let policy = proof
        .get("attribution_policy")
        .ok_or("Battle Cry proof attribution_policy is missing")?;
    let parent_effect_id = integer_at(current, "parent_effect_id")?;
    let origin_edges = array_at(proof, "historical_origin_edges")?;
    let child_static = array_at(current, "child_effects")?;
    let lifecycle = proof
        .get("historical_lifecycle")
        .ok_or("Battle Cry proof historical_lifecycle is missing")?;
    let current_build_packet_lifecycle_observed =
        bool_at(policy, "current_build_packet_lifecycle_observed")?;
    let external_recipient_child_effect_proven =
        bool_at(policy, "external_recipient_child_effect_proven")?;
    let formula_replay_allowed = bool_at(policy, "formula_replay_allowed")?;
    if current_build_packet_lifecycle_observed
        || external_recipient_child_effect_proven
        || formula_replay_allowed
    {
        return Err("Battle Cry proof does not preserve the required fail-closed state".into());
    }

    let proof_state = string_at(proof, "proof_state")
        .ok_or("Battle Cry proof_state is missing")?
        .to_owned();
    let mut evidence = Vec::new();
    for child in child_static {
        let child_effect_id = integer_at(child, "effect_id")?;
        let semantic_role = string_at(child, "role")
            .ok_or("Battle Cry child role is missing")?
            .to_owned();
        let origin = origin_edges
            .iter()
            .find(|row| signed_at(row, "effect_id") == Some(child_effect_id))
            .ok_or_else(|| format!("Battle Cry child {child_effect_id} origin edge is missing"))?;
        if integer_at(origin, "source_config_id")? != parent_effect_id
            || integer_at(origin, "source_type_id")? != 1
        {
            return Err(
                format!("Battle Cry child {child_effect_id} origin edge is inconsistent").into(),
            );
        }
        let lifecycle_key = match child_effect_id {
            2_205_311 => "owner_child",
            2_205_312 => "countdown_child",
            _ => {
                return Err(format!(
                    "Battle Cry proof contains unexpected child effect {child_effect_id}"
                )
                .into());
            }
        };
        let child_lifecycle = lifecycle
            .get(lifecycle_key)
            .ok_or_else(|| format!("Battle Cry {lifecycle_key} lifecycle is missing"))?;
        let row = RuntimeEffectFamilyEvidence {
            proof_state: proof_state.clone(),
            semantic_role,
            parent_effect_id,
            child_effect_id,
            historical_origin_observations: unsigned_at(origin, "observation_count"),
            historical_child_status_events: unsigned_at(child_lifecycle, "status_events"),
            historical_child_opened_windows: unsigned_at(child_lifecycle, "opened_windows"),
            historical_child_cross_actor_windows: unsigned_at(
                child_lifecycle,
                "cross_actor_windows",
            ),
            current_build_packet_lifecycle_observed,
            formula_replay_allowed,
        };
        if row.historical_origin_observations == 0
            || row.historical_child_status_events == 0
            || row.historical_child_opened_windows == 0
            || row.historical_child_cross_actor_windows != 0
        {
            return Err(format!(
                "Battle Cry child {child_effect_id} evidence no longer matches the retained self-only corpus"
            )
            .into());
        }
        evidence.push(row);
    }
    if evidence.len() != 2 {
        return Err("Battle Cry proof must retain exactly two runtime children".into());
    }
    Ok(BTreeMap::from([(parent_effect_id, evidence)]))
}

fn collect_denvel_runtime_effect_families(
    proof: &Value,
    historical_packet_build: &str,
) -> Result<BTreeMap<i64, Vec<RuntimeEffectFamilyEvidence>>, Box<dyn Error>> {
    if string_at(proof, "historical_packet_build") != Some(historical_packet_build) {
        return Err("Denvel proof historical packet build differs from the ledger".into());
    }
    let current = proof
        .get("current_static")
        .ok_or("Denvel proof current_static is missing")?;
    let owner = current
        .get("active_owner_buff")
        .ok_or("Denvel active owner buff is missing")?;
    let gravity = current
        .get("gravity_counter")
        .ok_or("Denvel gravity counter is missing")?;
    let formula = current
        .get("active_formula")
        .ok_or("Denvel active formula is missing")?;
    let lifecycle = proof
        .get("historical_lifecycle")
        .ok_or("Denvel historical lifecycle is missing")?;
    let policy = proof
        .get("attribution_policy")
        .ok_or("Denvel attribution policy is missing")?;

    let parent_effect_id = integer_at(owner, "effect_id")?;
    let gravity_effect_id = integer_at(gravity, "effect_id")?;
    if parent_effect_id != 2_110_137
        || gravity_effect_id != 2_110_152
        || string_at(owner, "role")
            != Some("casting-player-self-only-active-damage-boost-controller")
        || string_at(gravity, "role")
            != Some("affected-monster-self-sourced-gravity-counter-not-a-damage-modifier")
        || string_at(formula, "recipient_scope") != Some("casting-player-only")
        || !integer_array_at(policy, "transferable_effect_ids")?.is_empty()
        || bool_at(policy, "current_build_packet_lifecycle_observed")?
        || bool_at(policy, "formula_replay_allowed_for_transfer")?
    {
        return Err("Denvel proof does not preserve the exact self-only family contract".into());
    }

    let owner_lifecycle = lifecycle
        .get("owner_buff")
        .ok_or("Denvel owner lifecycle is missing")?;
    let gravity_lifecycle = lifecycle
        .get("gravity_counter")
        .ok_or("Denvel gravity lifecycle is missing")?;
    let owner_windows = unsigned_at(owner_lifecycle, "opened_windows");
    let gravity_windows = unsigned_at(gravity_lifecycle, "opened_windows");
    if unsigned_at(owner_lifecycle, "status_events") == 0
        || owner_windows == 0
        || unsigned_at(owner_lifecycle, "cross_actor_windows") != 0
        || unsigned_at(owner_lifecycle, "player_recipient_windows") != owner_windows
        || unsigned_at(owner_lifecycle, "monster_recipient_windows") != 0
        || unsigned_at(owner_lifecycle, "self_sourced_examples") != owner_windows
        || unsigned_at(owner_lifecycle, "non_self_sourced_examples") != 0
        || unsigned_at(gravity_lifecycle, "status_events") == 0
        || gravity_windows == 0
        || unsigned_at(gravity_lifecycle, "cross_actor_windows") != 0
        || unsigned_at(gravity_lifecycle, "player_recipient_windows") != 0
        || unsigned_at(gravity_lifecycle, "monster_recipient_windows") != gravity_windows
        || unsigned_at(gravity_lifecycle, "self_sourced_examples") != gravity_windows
        || unsigned_at(gravity_lifecycle, "non_self_sourced_examples") != 0
    {
        return Err(
            "Denvel historical lifecycle no longer proves separate self-sourced scopes".into(),
        );
    }

    let proof_state = string_at(proof, "proof_state")
        .ok_or("Denvel proof_state is missing")?
        .to_owned();
    let current_build_packet_lifecycle_observed =
        bool_at(policy, "current_build_packet_lifecycle_observed")?;
    let formula_replay_allowed = bool_at(policy, "formula_replay_allowed_for_transfer")?;
    let evidence = vec![
        RuntimeEffectFamilyEvidence {
            proof_state: proof_state.clone(),
            semantic_role: string_at(owner, "role").unwrap().to_owned(),
            parent_effect_id,
            child_effect_id: parent_effect_id,
            historical_origin_observations: unsigned_at(owner_lifecycle, "self_sourced_examples"),
            historical_child_status_events: unsigned_at(owner_lifecycle, "status_events"),
            historical_child_opened_windows: owner_windows,
            historical_child_cross_actor_windows: 0,
            current_build_packet_lifecycle_observed,
            formula_replay_allowed,
        },
        RuntimeEffectFamilyEvidence {
            proof_state,
            semantic_role: string_at(gravity, "role").unwrap().to_owned(),
            parent_effect_id,
            child_effect_id: gravity_effect_id,
            historical_origin_observations: unsigned_at(gravity_lifecycle, "self_sourced_examples"),
            historical_child_status_events: unsigned_at(gravity_lifecycle, "status_events"),
            historical_child_opened_windows: gravity_windows,
            historical_child_cross_actor_windows: 0,
            current_build_packet_lifecycle_observed,
            formula_replay_allowed,
        },
    ];
    Ok(BTreeMap::from([(parent_effect_id, evidence)]))
}

fn collect_focused_shot_runtime_effect_families(
    proof: &Value,
    historical_packet_build: &str,
) -> Result<BTreeMap<i64, Vec<RuntimeEffectFamilyEvidence>>, Box<dyn Error>> {
    if string_at(proof, "historical_packet_build") != Some(historical_packet_build) {
        return Err("Focused Shot proof historical packet build differs from the ledger".into());
    }
    let current = proof
        .get("current_static")
        .ok_or("Focused Shot proof current_static is missing")?;
    let controller = current
        .get("controller")
        .ok_or("Focused Shot controller is missing")?;
    let stack = current
        .get("stack")
        .ok_or("Focused Shot stack is missing")?;
    let formula = current
        .get("formula")
        .ok_or("Focused Shot formula is missing")?;
    let rejected = current
        .get("rejected_unrelated_focus")
        .ok_or("Focused Shot rejected Focus identity is missing")?;
    let lifecycle = proof
        .get("historical_lifecycle")
        .ok_or("Focused Shot historical lifecycle is missing")?;
    let policy = proof
        .get("attribution_policy")
        .ok_or("Focused Shot attribution policy is missing")?;

    let parent_effect_id = integer_at(controller, "effect_id")?;
    let stack_effect_id = integer_at(stack, "effect_id")?;
    let rejected_effect_id = integer_at(rejected, "effect_id")?;
    if parent_effect_id != 2_203_230
        || stack_effect_id != 2_203_231
        || rejected_effect_id != 55_223
        || unsigned_at(formula, "damage_boost_per_stack_basis_points") != 100
        || unsigned_at(formula, "maximum_stacks") != 4
        || unsigned_at(formula, "duration_seconds") != 3
        || string_at(formula, "qualifying_element") != Some("light")
        || string_at(formula, "recipient_scope") != Some("casting-player-only")
        || integer_array_at(policy, "retained_runtime_effect_ids")?
            != vec![parent_effect_id, stack_effect_id]
        || integer_array_at(policy, "rejected_runtime_effect_ids")? != vec![rejected_effect_id]
        || !integer_array_at(policy, "transferable_effect_ids")?.is_empty()
        || bool_at(policy, "current_build_packet_lifecycle_observed")?
        || bool_at(policy, "formula_replay_allowed_for_transfer")?
    {
        return Err(
            "Focused Shot proof does not preserve the exact self-only stack contract".into(),
        );
    }

    let controller_lifecycle = lifecycle
        .get("controller")
        .ok_or("Focused Shot controller lifecycle is missing")?;
    let stack_lifecycle = lifecycle
        .get("stack")
        .ok_or("Focused Shot stack lifecycle is missing")?;
    let unrelated_lifecycle = lifecycle
        .get("unrelated_focus")
        .ok_or("unrelated Focus lifecycle is missing")?;
    for (label, row) in [
        ("controller", controller_lifecycle),
        ("stack", stack_lifecycle),
        ("unrelated Focus", unrelated_lifecycle),
    ] {
        let windows = unsigned_at(row, "opened_windows");
        if unsigned_at(row, "status_events") == 0
            || windows == 0
            || unsigned_at(row, "cross_actor_windows") != 0
            || unsigned_at(row, "source_missing_windows") != 0
            || unsigned_at(row, "player_recipient_windows") != windows
            || unsigned_at(row, "monster_recipient_windows") != 0
            || unsigned_at(row, "other_recipient_windows") != 0
            || unsigned_at(row, "unresolved_recipient_windows") != 0
        {
            return Err(format!(
                "Focused Shot historical {label} lifecycle no longer proves a separate self-only scope"
            )
            .into());
        }
    }
    if unsigned_at(stack_lifecycle, "maximum_stacks") != 4 {
        return Err("Focused Shot historical stack maximum is no longer four".into());
    }

    let proof_state = string_at(proof, "proof_state")
        .ok_or("Focused Shot proof_state is missing")?
        .to_owned();
    let current_build_packet_lifecycle_observed =
        bool_at(policy, "current_build_packet_lifecycle_observed")?;
    let formula_replay_allowed = bool_at(policy, "formula_replay_allowed_for_transfer")?;
    let evidence = vec![
        RuntimeEffectFamilyEvidence {
            proof_state: proof_state.clone(),
            semantic_role: "owner-only-focused-shot-controller".to_owned(),
            parent_effect_id,
            child_effect_id: parent_effect_id,
            historical_origin_observations: unsigned_at(controller_lifecycle, "opened_windows"),
            historical_child_status_events: unsigned_at(controller_lifecycle, "status_events"),
            historical_child_opened_windows: unsigned_at(controller_lifecycle, "opened_windows"),
            historical_child_cross_actor_windows: 0,
            current_build_packet_lifecycle_observed,
            formula_replay_allowed,
        },
        RuntimeEffectFamilyEvidence {
            proof_state,
            semantic_role: "owner-only-one-percent-light-stack-up-to-four".to_owned(),
            parent_effect_id,
            child_effect_id: stack_effect_id,
            historical_origin_observations: unsigned_at(stack_lifecycle, "opened_windows"),
            historical_child_status_events: unsigned_at(stack_lifecycle, "status_events"),
            historical_child_opened_windows: unsigned_at(stack_lifecycle, "opened_windows"),
            historical_child_cross_actor_windows: 0,
            current_build_packet_lifecycle_observed,
            formula_replay_allowed,
        },
    ];
    Ok(BTreeMap::from([(parent_effect_id, evidence)]))
}

fn collect_stellar_spark_runtime_effect_families(
    proof: &Value,
    historical_packet_build: &str,
) -> Result<BTreeMap<i64, Vec<RuntimeEffectFamilyEvidence>>, Box<dyn Error>> {
    if string_at(proof, "historical_packet_build") != Some(historical_packet_build) {
        return Err("Stellar Spark proof historical packet build differs from the ledger".into());
    }
    let current = proof
        .get("current_static")
        .ok_or("Stellar Spark proof current_static is missing")?;
    let controller = current
        .get("controller")
        .ok_or("Stellar Spark controller is missing")?;
    let stack = current
        .get("stack")
        .ok_or("Stellar Spark stack is missing")?;
    let formula = current
        .get("formula")
        .ok_or("Stellar Spark formula is missing")?;
    let lifecycle = proof
        .get("historical_lifecycle")
        .ok_or("Stellar Spark historical lifecycle is missing")?;
    let policy = proof
        .get("attribution_policy")
        .ok_or("Stellar Spark attribution policy is missing")?;

    let parent_effect_id = integer_at(controller, "effect_id")?;
    let stack_effect_id = integer_at(stack, "effect_id")?;
    if parent_effect_id != 2_208_420
        || stack_effect_id != 2_208_421
        || unsigned_at(formula, "fire_attack_per_stack") != 22
        || unsigned_at(formula, "maximum_stacks") != 10
        || unsigned_at(formula, "duration_seconds") != 10
        || string_at(formula, "qualifying_trigger") != Some("expertise-skill-damage")
        || string_at(formula, "stat") != Some("fireAttack")
        || string_at(formula, "formula_term") != Some("elementalAttack")
        || string_at(formula, "formula_zone") != Some("baseAttackTerm")
        || string_at(formula, "recipient_scope") != Some("casting-player-only")
        || integer_array_at(policy, "retained_runtime_effect_ids")?
            != vec![parent_effect_id, stack_effect_id]
        || !integer_array_at(policy, "transferable_effect_ids")?.is_empty()
        || bool_at(policy, "current_build_packet_lifecycle_observed")?
        || bool_at(policy, "formula_replay_allowed_for_transfer")?
    {
        return Err(
            "Stellar Spark proof does not preserve the exact self-only Fire ATK stack contract"
                .into(),
        );
    }

    let controller_lifecycle = lifecycle
        .get("controller")
        .ok_or("Stellar Spark controller lifecycle is missing")?;
    let stack_lifecycle = lifecycle
        .get("stack")
        .ok_or("Stellar Spark stack lifecycle is missing")?;
    for (label, row) in [
        ("controller", controller_lifecycle),
        ("stack", stack_lifecycle),
    ] {
        let windows = unsigned_at(row, "opened_windows");
        if unsigned_at(row, "status_events") == 0
            || windows == 0
            || unsigned_at(row, "cross_actor_windows") != 0
            || unsigned_at(row, "source_missing_windows") != 0
            || unsigned_at(row, "player_recipient_windows") != windows
            || unsigned_at(row, "monster_recipient_windows") != 0
            || unsigned_at(row, "other_recipient_windows") != 0
            || unsigned_at(row, "unresolved_recipient_windows") != 0
        {
            return Err(format!(
                "Stellar Spark historical {label} lifecycle no longer proves a self-only scope"
            )
            .into());
        }
    }
    if unsigned_at(stack_lifecycle, "maximum_stacks") != 10 {
        return Err("Stellar Spark historical stack maximum is no longer ten".into());
    }

    let proof_state = string_at(proof, "proof_state")
        .ok_or("Stellar Spark proof_state is missing")?
        .to_owned();
    let current_build_packet_lifecycle_observed =
        bool_at(policy, "current_build_packet_lifecycle_observed")?;
    let formula_replay_allowed = bool_at(policy, "formula_replay_allowed_for_transfer")?;
    let evidence = vec![
        RuntimeEffectFamilyEvidence {
            proof_state: proof_state.clone(),
            semantic_role: "owner-only-stellar-spark-controller".to_owned(),
            parent_effect_id,
            child_effect_id: parent_effect_id,
            historical_origin_observations: unsigned_at(controller_lifecycle, "opened_windows"),
            historical_child_status_events: unsigned_at(controller_lifecycle, "status_events"),
            historical_child_opened_windows: unsigned_at(controller_lifecycle, "opened_windows"),
            historical_child_cross_actor_windows: 0,
            current_build_packet_lifecycle_observed,
            formula_replay_allowed,
        },
        RuntimeEffectFamilyEvidence {
            proof_state,
            semantic_role: "owner-only-twenty-two-fire-attack-stack-up-to-ten".to_owned(),
            parent_effect_id,
            child_effect_id: stack_effect_id,
            historical_origin_observations: unsigned_at(stack_lifecycle, "opened_windows"),
            historical_child_status_events: unsigned_at(stack_lifecycle, "status_events"),
            historical_child_opened_windows: unsigned_at(stack_lifecycle, "opened_windows"),
            historical_child_cross_actor_windows: 0,
            current_build_packet_lifecycle_observed,
            formula_replay_allowed,
        },
    ];
    Ok(BTreeMap::from([(parent_effect_id, evidence)]))
}

fn merge_runtime_effect_families(
    destination: &mut BTreeMap<i64, Vec<RuntimeEffectFamilyEvidence>>,
    source: BTreeMap<i64, Vec<RuntimeEffectFamilyEvidence>>,
) {
    for (parent_effect_id, mut evidence) in source {
        destination
            .entry(parent_effect_id)
            .or_default()
            .append(&mut evidence);
    }
}

fn collect_packet_inventory_occurrence(
    inventory: &Value,
    expected_packet_build: &str,
) -> Result<BTreeMap<i64, HistoricalPacketEvidence>, Box<dyn Error>> {
    if string_at(inventory, "game_build") != Some(expected_packet_build) {
        return Err("historical packet inventory build differs from the ledger".into());
    }
    let mut result = BTreeMap::new();
    for row in array_at(inventory, "observed_effects")? {
        let effect_id = integer_at(row, "effect_id")?;
        let status_events = unsigned_at(row, "status_events");
        let origin_observations = unsigned_at(row, "packet_origin_observations");
        result.insert(
            effect_id,
            HistoricalPacketEvidence {
                inventory_effect_rows_present: 1,
                inventory_status_events: status_events,
                inventory_origin_observations: origin_observations,
                authoritative_status_events: status_events,
                evidence_authority: "historical-packet-inventory-occurrence-only",
                ..HistoricalPacketEvidence::default()
            },
        );
    }
    Ok(result)
}

fn collect_packet_observed_effect_families(
    inventory: &Value,
    expected_packet_build: &str,
) -> Result<BTreeMap<i64, Vec<PacketObservedEffectFamilyEdge>>, Box<dyn Error>> {
    if string_at(inventory, "game_build") != Some(expected_packet_build) {
        return Err("historical packet inventory build differs from the ledger".into());
    }
    let mut by_parent = BTreeMap::<i64, BTreeSet<PacketObservedEffectFamilyEdge>>::new();
    for row in array_at(inventory, "display_relations")? {
        let source_type_id = integer_at(row, "source_type_id")?;
        if source_type_id != 1 {
            continue;
        }
        let parent_effect_id = integer_at(row, "source_config_id")?;
        let child_effect_id = integer_at(row, "effect_id")?;
        if parent_effect_id == child_effect_id {
            continue;
        }
        by_parent
            .entry(parent_effect_id)
            .or_default()
            .insert(PacketObservedEffectFamilyEdge {
                parent_effect_id,
                child_effect_id,
                source_type_id,
                observation_count: unsigned_at(row, "observation_count"),
                evidence_authority: "historical-packet-origin-lineage-only",
            });
    }
    Ok(by_parent
        .into_iter()
        .map(|(parent, rows)| (parent, rows.into_iter().collect()))
        .collect())
}

fn collect_reachable_packet_family_edges(
    declared_effect_ids: &[i64],
    by_parent: &BTreeMap<i64, Vec<PacketObservedEffectFamilyEdge>>,
) -> Vec<PacketObservedEffectFamilyEdge> {
    let mut pending = declared_effect_ids.iter().copied().collect::<Vec<_>>();
    let mut visited = declared_effect_ids.iter().copied().collect::<BTreeSet<_>>();
    let mut edges = BTreeSet::new();
    while let Some(parent) = pending.pop() {
        for edge in by_parent.get(&parent).into_iter().flatten() {
            edges.insert(edge.clone());
            if visited.insert(edge.child_effect_id) {
                pending.push(edge.child_effect_id);
            }
        }
    }
    edges.into_iter().collect()
}

fn collect_current_component_evidence(
    origin_ledger: &Value,
) -> Result<BTreeMap<i64, Vec<CurrentComponentEvidence>>, Box<dyn Error>> {
    let mut by_effect = BTreeMap::<i64, BTreeSet<CurrentComponentEvidence>>::new();
    for skill in array_at(origin_ledger, "skills")? {
        let skill_id = unsigned_at(skill, "skill_id");
        let skill_name = string_at(skill, "name")
            .ok_or("current origin skill name is missing")?
            .to_owned();
        for component in array_at(skill, "component_routes")? {
            let effect_ids = integer_array_at(component, "effect_ids")?;
            let evidence = CurrentComponentEvidence {
                skill_id,
                skill_name: skill_name.clone(),
                component_id: string_at(component, "component_id")
                    .ok_or("current component id is missing")?
                    .to_owned(),
                role: string_at(component, "role")
                    .ok_or("current component role is missing")?
                    .to_owned(),
                effect_ids: effect_ids.clone(),
                recipient_scope: string_at(component, "recipient_scope")
                    .ok_or("current component recipient scope is missing")?
                    .to_owned(),
                rdps_disposition: string_at(component, "rdps_disposition")
                    .ok_or("current component rDPS disposition is missing")?
                    .to_owned(),
                proof_state: string_at(component, "proof_state")
                    .ok_or("current component proof state is missing")?
                    .to_owned(),
            };
            for effect_id in effect_ids {
                by_effect
                    .entry(effect_id)
                    .or_default()
                    .insert(evidence.clone());
            }
        }
    }
    Ok(by_effect
        .into_iter()
        .map(|(effect_id, rows)| (effect_id, rows.into_iter().collect()))
        .collect())
}

fn merge_current_component_evidence(
    effect_ids: &[i64],
    by_effect: &BTreeMap<i64, Vec<CurrentComponentEvidence>>,
) -> Vec<CurrentComponentEvidence> {
    effect_ids
        .iter()
        .filter_map(|effect_id| by_effect.get(effect_id))
        .flatten()
        .cloned()
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect()
}

fn resolve_transfer_eligibilities(
    declared: &[String],
    current_components: &[CurrentComponentEvidence],
) -> (Vec<String>, &'static str) {
    let normalized = normalize_declared_transfer_eligibilities(declared);
    let has_unresolved = normalized.iter().any(|value| {
        matches!(
            value.as_str(),
            "recipient-scope-unresolved" | "recipient-scope-unresolved-target-filtered"
        )
    });
    let has_open_owner_context = normalized
        .iter()
        .any(|value| value == "owner-local-formula-context-recipient-scope-open");
    let has_weak_external_recipient = normalized
        .iter()
        .any(|value| value == "external-recipient-candidate");
    if current_components.is_empty() {
        return if normalized == declared {
            (normalized, "declared-static-scope-preserved")
        } else {
            (
                normalized,
                "declared-static-owner-context-kept-recipient-scope-open",
            )
        };
    }

    let is_external = |evidence: &CurrentComponentEvidence| {
        matches!(
            evidence.recipient_scope.as_str(),
            "provider-and-external-teammates-in-area"
                | "provider-and-up-to-ten-allies"
                | "provider-and-up-to-ten-nearby-allies"
                | "each-shielded-recipient-triggers-from-that-friendly-attack"
        ) && evidence.rdps_disposition == "exact-attack-and-mattack-counterfactual-only"
    };
    let is_self_only = |evidence: &CurrentComponentEvidence| {
        matches!(
            evidence.recipient_scope.as_str(),
            "summon-caster-only"
                | "summon-owner-only"
                | "provider-only"
                | "provider-only-while-equipped"
                | "provider-owned-passive-proc"
                | "provider-owned-summon"
                | "source-restricted-enemy-target-state-for-provider-damage-only"
        ) && matches!(
            evidence.rdps_disposition.as_str(),
            "ordinary-owner-damage-never-transferred"
                | "ordinary-owner-damage-never-support-credit"
                | "ordinary-owner-stats-never-transferred"
                | "ordinary-owner-stats-and-procs-never-transferred"
        )
    };
    let all_external = current_components.iter().all(is_external);
    let all_self_only = current_components.iter().all(is_self_only);
    let replacement = if all_external && (has_unresolved || has_open_owner_context) {
        Some("external-recipient-candidate")
    } else if all_self_only
        && (has_unresolved || has_open_owner_context || has_weak_external_recipient)
    {
        Some("self-only-current-component-proof")
    } else {
        None
    };
    let Some(replacement) = replacement else {
        return (
            normalized,
            "declared-static-scope-preserved-current-component-mixed",
        );
    };
    let mut effective = normalized
        .iter()
        .filter(|value| {
            !matches!(
                value.as_str(),
                "recipient-scope-unresolved"
                    | "recipient-scope-unresolved-target-filtered"
                    | "external-recipient-candidate"
                    | "owner-local-formula-context-recipient-scope-open"
            )
        })
        .cloned()
        .collect::<BTreeSet<_>>();
    effective.insert(replacement.to_owned());
    (
        effective.into_iter().collect(),
        "exact-current-component-scope",
    )
}

fn normalize_declared_transfer_eligibilities(declared: &[String]) -> Vec<String> {
    declared
        .iter()
        .map(|value| match value.as_str() {
            "self-only-formula-context" => {
                "owner-local-formula-context-recipient-scope-open".to_owned()
            }
            _ => value.clone(),
        })
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect()
}

fn resolve_proven_runtime_family_scope(
    effective: &[String],
    prior_resolution: &'static str,
    evidence: &[RuntimeEffectFamilyEvidence],
) -> (Vec<String>, &'static str) {
    let has_open_scope = effective.iter().any(|value| {
        matches!(
            value.as_str(),
            "recipient-scope-unresolved"
                | "recipient-scope-unresolved-target-filtered"
                | "owner-local-formula-context-recipient-scope-open"
        )
    });
    if !has_open_scope || evidence.is_empty() {
        return (effective.to_vec(), prior_resolution);
    }

    let proves_self_only = evidence.iter().all(|row| {
        row.semantic_role.starts_with("owner-only-")
            && row.historical_child_status_events > 0
            && row.historical_child_opened_windows > 0
            && row.historical_child_cross_actor_windows == 0
            && !row.formula_replay_allowed
    });
    if !proves_self_only {
        return (
            effective.to_vec(),
            "declared-static-scope-preserved-runtime-family-insufficient",
        );
    }

    let mut resolved = effective
        .iter()
        .filter(|value| {
            !matches!(
                value.as_str(),
                "recipient-scope-unresolved"
                    | "recipient-scope-unresolved-target-filtered"
                    | "owner-local-formula-context-recipient-scope-open"
            )
        })
        .cloned()
        .collect::<BTreeSet<_>>();
    resolved.insert("self-only-runtime-family-proof".to_owned());
    (
        resolved.into_iter().collect(),
        "reviewed-current-static-plus-historical-runtime-family-self-only",
    )
}

fn display_source_name(display: &Value, source_rule_id: &str) -> Option<String> {
    display
        .get("sourcesByRuleId")
        .and_then(|rows| rows.get(source_rule_id))
        .and_then(|row| optional_string_at(row, "sourceName"))
}

fn collect_packet_evidence(
    packet_proof: &Value,
) -> Result<BTreeMap<i64, HistoricalPacketEvidence>, Box<dyn Error>> {
    let mut result = BTreeMap::new();
    for effect in array_at(packet_proof, "effects")? {
        let effect_id = signed_at(effect, "effect_id").ok_or("packet effect_id is missing")?;
        let mut resolutions = BTreeSet::new();
        let mut evidence = HistoricalPacketEvidence {
            compact_effect_rows_present: 1,
            compact_selected_status_events: unsigned_at(effect, "selected_status_events"),
            evidence_authority: "historical-packet-corpus-research-only",
            ..HistoricalPacketEvidence::default()
        };
        for attribute in array_at(effect, "attributes")? {
            for aggregate in array_at(attribute, "aggregates")? {
                match aggregate.get("provider_is_target").and_then(Value::as_bool) {
                    Some(true) => evidence.provider_is_recipient_observed = true,
                    Some(false) => {
                        evidence.provider_differs_from_recipient_observed = true;
                        if string_at(aggregate, "provider_kind") == Some("player") {
                            evidence.resolved_external_player_provider_observed = true;
                        }
                    }
                    None => evidence.provider_identity_unresolved_observed = true,
                }
                if let Some(resolution) = string_at(aggregate, "provider_resolution") {
                    resolutions.insert(resolution.to_owned());
                }
            }
        }
        evidence.provider_resolutions = resolutions.into_iter().collect();
        result.insert(effect_id, evidence);
    }
    Ok(result)
}

fn collect_provider_audit_evidence(
    provider_audit: &Value,
) -> Result<BTreeMap<i64, HistoricalPacketEvidence>, Box<dyn Error>> {
    let mut result = BTreeMap::new();
    for report in array_at(provider_audit, "reports")? {
        for effect in array_at(report, "effects")? {
            let effect_id =
                signed_at(effect, "effect_id").ok_or("provider audit effect_id is missing")?;
            let lifecycle = effect
                .get("lifecycle")
                .ok_or("provider audit lifecycle is missing")?;
            let recipient = effect
                .get("recipient_scope")
                .ok_or("provider audit recipient_scope is missing")?;
            let evidence = result
                .entry(effect_id)
                .or_insert_with(|| HistoricalPacketEvidence {
                    evidence_authority: "historical-packet-corpus-research-only",
                    ..HistoricalPacketEvidence::default()
                });
            evidence.provider_audit_rows_present += 1;
            let status_events = unsigned_at(lifecycle, "status_events");
            evidence.provider_audit_rows_with_status_events += usize::from(status_events > 0);
            evidence.authoritative_status_events += status_events;
            evidence.opened_windows += unsigned_at(lifecycle, "opened_windows");
            evidence.cross_actor_windows += unsigned_at(lifecycle, "cross_actor_windows");
            evidence.source_missing_windows += unsigned_at(lifecycle, "source_missing_windows");
            evidence.player_recipient_windows += unsigned_at(recipient, "player");
            evidence.monster_recipient_windows += unsigned_at(recipient, "monster");
            evidence.provider_identity_unresolved_observed |=
                unsigned_at(lifecycle, "source_missing_windows") > 0;
            let mut resolutions = evidence
                .provider_resolutions
                .iter()
                .cloned()
                .collect::<BTreeSet<_>>();
            for provider in array_at(effect, "providers")? {
                if let Some(resolution) = string_at(provider, "resolution") {
                    resolutions.insert(resolution.to_owned());
                }
                let windows = unsigned_at(provider, "windows");
                match string_at(provider, "resolution") {
                    Some("direct_player")
                    | Some("owner_link_within_run")
                    | Some("direct_source_owner_link_within_run")
                    | Some("paired_owner_attributes_within_run") => {
                        evidence.owner_linked_player_provider_windows += windows;
                    }
                    Some("non_player") => evidence.non_player_provider_windows += windows,
                    _ => evidence.provider_identity_unresolved_observed |= windows > 0,
                }
                let resolved_provider = unsigned_at(provider, "resolved_provider_entity_uuid");
                let has_proxy_source =
                    array_at(provider, "raw_source_entities")?
                        .iter()
                        .any(|source| {
                            source
                                .as_u64()
                                .map(|source| source != resolved_provider)
                                .unwrap_or(false)
                        });
                if has_proxy_source {
                    evidence.raw_proxy_source_windows += windows;
                }
            }
            for example in array_at(effect, "examples")? {
                let provider = unsigned_at(example, "resolved_provider_entity_uuid");
                let recipient = unsigned_at(example, "target_entity_uuid");
                let resolution = string_at(example, "provider_resolution");
                if provider == 0 || recipient == 0 {
                    evidence.provider_identity_unresolved_observed = true;
                    continue;
                }
                if provider == recipient {
                    evidence.resolved_provider_is_recipient_examples += 1;
                    evidence.provider_is_recipient_observed = true;
                } else {
                    evidence.resolved_provider_differs_from_recipient_examples += 1;
                    evidence.provider_differs_from_recipient_observed = true;
                    if resolution == Some("owner_link_within_run") {
                        evidence.resolved_external_player_provider_observed = true;
                    }
                }
            }
            evidence.provider_resolutions = resolutions.into_iter().collect();
        }
    }
    Ok(result)
}

fn collect_component_packet_evidence(
    proof: &Value,
    expected_packet_build: &str,
) -> Result<(i64, HistoricalPacketEvidence), Box<dyn Error>> {
    if unsigned_at(proof, "schema_version") != 1
        || string_at(proof, "historical_packet_build") != Some(expected_packet_build)
        || string_at(proof, "proof_state")
            != Some(
                "current-static-chain-exact-plus-historical-projectile-status-edge-current-packet-provider-live-gated",
            )
    {
        return Err("component packet proof schema, build, or proof state changed".into());
    }
    let current = proof
        .get("current_static")
        .ok_or("component packet proof current_static is missing")?;
    let packet = proof
        .get("historical_packet")
        .ok_or("component packet proof historical_packet is missing")?;
    let limits = proof
        .get("ownership_limits")
        .ok_or("component packet proof ownership_limits is missing")?;
    let effect_id = signed_at(current, "target_status_id")
        .ok_or("component packet proof target_status_id is missing")?;
    let applied = unsigned_at(packet, "applied_count");
    let removed = unsigned_at(packet, "removed_count");
    let source_actor_count = array_at(packet, "source_actor_ids")?.len() as u64;
    let target_actor_count = array_at(packet, "target_actor_ids")?.len() as u64;
    let target_kinds = string_array_at(packet, "target_actor_kinds")?;
    if effect_id == 0
        || applied == 0
        || removed == 0
        || source_actor_count == 0
        || target_actor_count == 0
        || string_at(packet, "source_actor_kind") != Some("projectile")
        || target_kinds.iter().any(|kind| kind == "player")
        || limits
            .get("player_provider_identity_available_in_historical_projection")
            .and_then(Value::as_bool)
            != Some(false)
    {
        return Err("component packet proof does not preserve an unresolved projectile-to-monster lifecycle".into());
    }
    Ok((
        effect_id,
        HistoricalPacketEvidence {
            provider_audit_rows_present: 1,
            provider_audit_rows_with_status_events: 1,
            authoritative_status_events: applied.saturating_add(removed),
            opened_windows: applied,
            cross_actor_windows: applied,
            raw_proxy_source_windows: applied,
            non_player_provider_windows: applied,
            monster_recipient_windows: target_actor_count,
            provider_identity_unresolved_observed: true,
            provider_resolutions: vec!["raw-projectile-owner-unresolved".to_string()],
            evidence_authority: "historical-packet-corpus-research-only",
            ..HistoricalPacketEvidence::default()
        },
    ))
}

fn merge_packet_evidence(
    effect_ids: &[i64],
    compact_by_effect: &BTreeMap<i64, HistoricalPacketEvidence>,
    inventory_by_effect: &BTreeMap<i64, HistoricalPacketEvidence>,
    provider_audit_by_effect: &BTreeMap<i64, HistoricalPacketEvidence>,
) -> HistoricalPacketEvidence {
    let mut merged = HistoricalPacketEvidence {
        evidence_authority: "historical-packet-corpus-research-only",
        ..HistoricalPacketEvidence::default()
    };
    let mut resolutions = BTreeSet::new();
    for effect_id in effect_ids {
        if let Some(inventory) = inventory_by_effect.get(effect_id) {
            merged.inventory_effect_rows_present += inventory.inventory_effect_rows_present;
            merged.inventory_status_events += inventory.inventory_status_events;
            merged.inventory_origin_observations += inventory.inventory_origin_observations;
        }
        if let Some(compact) = compact_by_effect.get(effect_id) {
            merged.compact_effect_rows_present += compact.compact_effect_rows_present;
            merged.compact_selected_status_events += compact.compact_selected_status_events;
            merged.provider_is_recipient_observed |= compact.provider_is_recipient_observed;
            merged.provider_differs_from_recipient_observed |=
                compact.provider_differs_from_recipient_observed;
            merged.resolved_external_player_provider_observed |=
                compact.resolved_external_player_provider_observed;
            merged.provider_identity_unresolved_observed |=
                compact.provider_identity_unresolved_observed;
            resolutions.extend(compact.provider_resolutions.iter().cloned());
        }
        if let Some(audit) = provider_audit_by_effect.get(effect_id) {
            merged.provider_audit_rows_present += audit.provider_audit_rows_present;
            merged.provider_audit_rows_with_status_events +=
                audit.provider_audit_rows_with_status_events;
            merged.authoritative_status_events += audit.authoritative_status_events;
            merged.opened_windows += audit.opened_windows;
            merged.cross_actor_windows += audit.cross_actor_windows;
            merged.raw_proxy_source_windows += audit.raw_proxy_source_windows;
            merged.owner_linked_player_provider_windows +=
                audit.owner_linked_player_provider_windows;
            merged.non_player_provider_windows += audit.non_player_provider_windows;
            merged.resolved_provider_is_recipient_examples +=
                audit.resolved_provider_is_recipient_examples;
            merged.resolved_provider_differs_from_recipient_examples +=
                audit.resolved_provider_differs_from_recipient_examples;
            merged.source_missing_windows += audit.source_missing_windows;
            merged.player_recipient_windows += audit.player_recipient_windows;
            merged.monster_recipient_windows += audit.monster_recipient_windows;
            merged.provider_is_recipient_observed |= audit.provider_is_recipient_observed;
            merged.provider_differs_from_recipient_observed |=
                audit.provider_differs_from_recipient_observed;
            merged.resolved_external_player_provider_observed |=
                audit.resolved_external_player_provider_observed;
            merged.provider_identity_unresolved_observed |=
                audit.provider_identity_unresolved_observed;
            resolutions.extend(audit.provider_resolutions.iter().cloned());
        } else if let Some(inventory) = inventory_by_effect.get(effect_id) {
            merged.authoritative_status_events += inventory.inventory_status_events;
        } else if let Some(compact) = compact_by_effect.get(effect_id) {
            merged.authoritative_status_events += compact.compact_selected_status_events;
        }
    }
    merged.provider_resolutions = resolutions.into_iter().collect();
    merged
}

fn component_scope_routes(
    row: &Value,
    contribution_mode: &str,
) -> Result<Vec<ComponentScopeRoute>, Box<dyn Error>> {
    array_at(row, "relationship_components")?
        .iter()
        .map(|component| {
            let component_key = string_at(component, "componentKey")
                .ok_or("relationship component componentKey is missing")?
                .to_owned();
            let declared_transfer_eligibility = string_at(component, "transferEligibility")
                .unwrap_or("recipient-scope-unresolved")
                .to_owned();
            let transfer_eligibility = normalize_declared_transfer_eligibilities(
                std::slice::from_ref(&declared_transfer_eligibility),
            )
            .into_iter()
            .next()
            .unwrap_or_else(|| "recipient-scope-unresolved".to_owned());
            let scope_queue = scope_queue(std::slice::from_ref(&transfer_eligibility));
            let contribution_groups = optional_string_array_at(component, "contributionGroups")?;
            let formula_term_ids = optional_string_array_at(component, "formulaTermIds")?;
            let direction = optional_string_at(component, "direction");
            let rdps_relevance = component_rdps_relevance(
                &component_key,
                optional_string_at(component, "effectClass").as_deref(),
                direction.as_deref(),
                &contribution_groups,
                &formula_term_ids,
            );
            Ok(ComponentScopeRoute {
                component_key,
                proof_binding: component.get("proofBinding").cloned(),
                effect_class: optional_string_at(component, "effectClass"),
                direction,
                contribution_scope: optional_string_at(component, "contributionScope"),
                contribution_groups,
                formula_term_ids,
                declared_transfer_eligibility,
                transfer_eligibility,
                scope_queue,
                rdps_relevance,
                value_resolution: optional_string_at(component, "valueResolution"),
                required_runtime_evidence: optional_string_array_at(
                    component,
                    "requiredRuntimeEvidence",
                )?,
                transfer_gate: transfer_gate(scope_queue, contribution_mode),
                current_build_promotion_eligible: false,
            })
        })
        .collect()
}

fn reconcile_component_scope_routes(
    mut routes: Vec<ComponentScopeRoute>,
    effective_transfer_eligibilities: &[String],
    contribution_mode: &str,
) -> Vec<ComponentScopeRoute> {
    let exact_self_only = !effective_transfer_eligibilities.is_empty()
        && effective_transfer_eligibilities
            .iter()
            .all(|value| value == "self-only-current-component-proof");
    if !exact_self_only {
        return routes;
    }

    for route in &mut routes {
        if matches!(
            route.transfer_eligibility.as_str(),
            "external-recipient-candidate"
                | "recipient-scope-unresolved"
                | "recipient-scope-unresolved-target-filtered"
                | "owner-local-formula-context-recipient-scope-open"
        ) {
            route.transfer_eligibility = "self-only-current-component-proof".to_owned();
            route.scope_queue = "self-only-formula-context-no-transfer";
            route.transfer_gate = transfer_gate(route.scope_queue, contribution_mode);
            route.current_build_promotion_eligible = false;
        }
    }
    routes
}

fn component_rdps_relevance(
    component_key: &str,
    effect_class: Option<&str>,
    direction: Option<&str>,
    contribution_groups: &[String],
    formula_term_ids: &[String],
) -> &'static str {
    if effect_class == Some("formula-input-dependency") || direction == Some("formula-input") {
        return "mechanic-formula-input-dependency";
    }
    if !formula_term_ids.is_empty()
        || contribution_groups.iter().any(|group| {
            matches!(
                group.as_str(),
                "baseAttack"
                    | "critical"
                    | "damageIncrease"
                    | "elementalDamage"
                    | "luckStat"
                    | "statConversion"
                    | "versatility"
            )
        })
    {
        return "direct-damage-formula-component";
    }
    if matches!(
        component_key,
        "adaptive-primary-stat"
            | "atk"
            | "matk"
            | "critical-rate"
            | "critical-damage"
            | "luck-stat"
            | "mastery-stat"
            | "versatility"
    ) || matches!(
        effect_class,
        Some(
            "offense-stat"
                | "critical-stat"
                | "critical-damage-stat"
                | "luck-stat"
                | "mastery-stat"
                | "versatility-stat"
                | "stat-conversion"
        )
    ) {
        return "formula-upstream-stat-component";
    }
    if direction == Some("timing") || contribution_groups.iter().any(|group| group == "hitTiming") {
        return "indirect-output-cadence-component";
    }
    if direction == Some("damage") {
        return "produced-damage-or-damage-state-component";
    }
    "non-damage-or-unclassified-component"
}

fn source_scope_queue(
    component_scope_routes: &[ComponentScopeRoute],
    fallback: &'static str,
) -> &'static str {
    let queues = component_scope_routes
        .iter()
        .map(|route| route.scope_queue)
        .collect::<BTreeSet<_>>();
    match queues.len() {
        0 => fallback,
        1 => queues.into_iter().next().unwrap_or(fallback),
        _ => "component-scoped-mixed",
    }
}

fn scope_queue(eligibilities: &[String]) -> &'static str {
    if eligibilities
        .iter()
        .any(|value| value == "recipient-scope-unresolved")
    {
        return "unresolved-provider-recipient";
    }
    if eligibilities
        .iter()
        .any(|value| value == "recipient-scope-unresolved-target-filtered")
    {
        return "unresolved-target-filtered-provider-recipient";
    }
    if eligibilities
        .iter()
        .any(|value| value == "external-target-state-candidate")
    {
        return "external-target-state-requires-current-build-proof";
    }
    if eligibilities
        .iter()
        .any(|value| value == "external-recipient-candidate")
    {
        return "external-recipient-requires-current-build-proof";
    }
    let has_open_owner_context = eligibilities.iter().any(|value| {
        matches!(
            value.as_str(),
            "self-only-formula-context" | "owner-local-formula-context-recipient-scope-open"
        )
    });
    if has_open_owner_context
        && eligibilities
            .iter()
            .any(|value| value == "direct-output-owned-by-source")
    {
        return "mixed-source-output-and-open-owner-context";
    }
    if has_open_owner_context {
        return "owner-local-formula-context-requires-recipient-proof";
    }
    if !eligibilities.is_empty()
        && eligibilities.iter().all(|value| {
            matches!(
                value.as_str(),
                "self-only-current-component-proof" | "self-only-runtime-family-proof"
            )
        })
    {
        return "self-only-formula-context-no-transfer";
    }
    if !eligibilities.is_empty()
        && eligibilities
            .iter()
            .all(|value| value == "direct-output-owned-by-source")
    {
        return "direct-output-owned-by-source-no-transfer";
    }
    if !eligibilities.is_empty()
        && eligibilities
            .iter()
            .all(|value| value == "non-outgoing-context")
    {
        return "non-outgoing-context-no-offensive-transfer";
    }
    "mixed-or-unclassified-scope"
}

fn transfer_gate(scope_queue: &str, contribution_mode: &str) -> TransferGate {
    let authority =
        "matching-build canonical packet events plus reviewed exact-build formula evidence";
    match scope_queue {
        "external-recipient-requires-current-build-proof" => TransferGate {
            kind: "external-recipient-counterfactual",
            attribution_route: "provider -> external player recipient effect window -> recipient marginal damage",
            authority,
            runtime_credit_allowed: false,
            required_current_build_evidence: vec![
                "effect apply, refresh, stack, consume, and remove lifecycle",
                "resolved player provider and player recipient identities",
                "provider and recipient must differ for transferred credit",
                "exact affected formula components and encoded magnitudes",
                "recipient damage events inside the exact effect window",
                "baseline and counterfactual replay with party conservation",
            ],
            forbidden_transfers: vec![
                "self-provided effect contribution",
                "damage outside the recipient effect window",
                "credit inferred only from localized description",
                "credit inferred only from historical-build packets",
            ],
        },
        "external-target-state-requires-current-build-proof" => TransferGate {
            kind: "external-target-state-counterfactual",
            attribution_route: "provider -> enemy target state window -> other-player marginal damage to that target",
            authority,
            runtime_credit_allowed: false,
            required_current_build_evidence: vec![
                "effect apply, refresh, stack, consume, and remove lifecycle on the enemy target",
                "resolved player provider and monster target identities",
                "exact mitigation or vulnerability formula component and encoded magnitude",
                "attacker, target, and damage events inside the same target-state window",
                "baseline and counterfactual replay with party conservation",
            ],
            forbidden_transfers: vec![
                "provider's own marginal damage through its target debuff",
                "damage dealt to a different target",
                "damage outside the target-state window",
                "defensive enemy attack reduction treated as offensive rDPS",
                "credit inferred only from localized description",
                "credit inferred only from historical-build packets",
            ],
        },
        "self-only-formula-context-no-transfer" => TransferGate {
            kind: "self-only-nontransfer",
            attribution_route: "owner personal formula context only",
            authority,
            runtime_credit_allowed: false,
            required_current_build_evidence: vec![
                "matching-build owner or summon-owner identity",
                "exact personal formula component and encoded magnitude",
                "matching-build lifecycle when the effect is stateful",
            ],
            forbidden_transfers: vec![
                "credit to a different provider",
                "self buff reclassified as party support",
            ],
        },
        "owner-local-formula-context-requires-recipient-proof" => TransferGate {
            kind: "owner-local-formula-context-scope-hold",
            attribution_route: "none until matching-build packet evidence proves whether the owner-local formula component can affect another recipient",
            authority,
            runtime_credit_allowed: false,
            required_current_build_evidence: vec![
                "matching-build provider and recipient identities",
                "exact component identity, encoded magnitude, and runtime selector",
                "effect or state lifecycle on every observed recipient",
                "same-source comparison between owner and external-recipient outcomes",
            ],
            forbidden_transfers: vec![
                "static owner-local formula wording treated as self-only recipient proof",
                "support credit before a distinct external recipient is observed",
                "self-only closure inferred only from absence in a limited capture",
            ],
        },
        "mixed-source-output-and-open-owner-context" => TransferGate {
            kind: "mixed-source-output-and-open-owner-context-hold",
            attribution_route: "retain source-owned output while independently proving the recipient scope of every owner-local formula component",
            authority,
            runtime_credit_allowed: false,
            required_current_build_evidence: vec![
                "matching-build source ownership and child ancestry",
                "component-specific provider, recipient, magnitude, and lifecycle evidence",
                "deduplicated canonical output plus recipient counterfactual replay",
            ],
            forbidden_transfers: vec![
                "source-owned output treated as transferred support damage",
                "owner-local formula context closed as self-only without packet proof",
                "duplicate parent plus child damage counting",
            ],
        },
        "direct-output-owned-by-source-no-transfer" => TransferGate {
            kind: "source-owned-output-nontransfer",
            attribution_route: "exact produced output -> resolved source owner",
            authority,
            runtime_credit_allowed: false,
            required_current_build_evidence: vec![
                "matching-build event source and summon or child owner resolution",
                "exact produced-damage child ancestry",
                "deduplicated canonical damage event",
            ],
            forbidden_transfers: vec![
                "produced damage credited as transferred support damage",
                "duplicate parent plus child damage counting",
            ],
        },
        "non-outgoing-context-no-offensive-transfer" => TransferGate {
            kind: "non-outgoing-context",
            attribution_route: "exact non-offensive formula dependency -> affected healing, shielding, or defensive output",
            authority,
            runtime_credit_allowed: false,
            required_current_build_evidence: vec![
                "matching-build dependent output event or state transition",
                "exact source or recipient formula-input snapshot selected by the relationship component",
                "exact encoded coefficient and lifecycle when stateful",
            ],
            forbidden_transfers: vec![
                "non-offensive healing, shielding, or mitigation credited as offensive rDPS",
                "recipient formula input substituted with provider state",
            ],
        },
        "mixed-known-nontransfer-output-and-self-context" => TransferGate {
            kind: "mixed-known-nontransfer",
            attribution_route: "source-owned output plus owner personal formula context",
            authority,
            runtime_credit_allowed: false,
            required_current_build_evidence: vec![
                "matching-build source ownership and child ancestry",
                "exact personal formula components and encoded magnitudes",
                "deduplicated canonical damage events",
            ],
            forbidden_transfers: vec![
                "source-owned output transferred to another player",
                "personal formula context reclassified as party support",
                "duplicate parent plus child damage counting",
            ],
        },
        "unresolved-target-filtered-provider-recipient" => TransferGate {
            kind: "unresolved-target-filtered-hold",
            attribution_route: "none until provider, recipient, and target semantics resolve",
            authority,
            runtime_credit_allowed: false,
            required_current_build_evidence: vec![
                "matching-build provider and recipient identities",
                "matching-build target identity and filter semantics",
                "effect lifecycle and exact formula components",
                "baseline and counterfactual replay with party conservation",
            ],
            forbidden_transfers: vec![
                "any rDPS transfer while recipient or target scope is unresolved",
                "credit inferred only from name or localized description",
            ],
        },
        "unresolved-provider-recipient" => TransferGate {
            kind: "unresolved-provider-recipient-hold",
            attribution_route: "none until provider and recipient semantics resolve",
            authority,
            runtime_credit_allowed: false,
            required_current_build_evidence: vec![
                "matching-build provider and recipient identities",
                "effect lifecycle and exact formula components",
                "baseline and counterfactual replay with party conservation",
            ],
            forbidden_transfers: vec![
                "any rDPS transfer while provider or recipient scope is unresolved",
                "credit inferred only from name or localized description",
            ],
        },
        "component-scoped-mixed" => TransferGate {
            kind: "component-scoped-routing-only",
            attribution_route: "none at source level; evaluate every relationship component through its own transfer gate",
            authority,
            runtime_credit_allowed: false,
            required_current_build_evidence: vec![
                "matching-build evidence required by each component transfer gate",
                "component-specific provider, recipient, lifecycle, and formula binding",
                "per-component baseline and counterfactual replay with party conservation",
            ],
            forbidden_transfers: vec![
                "whole-source attribution from a mixed component verdict",
                "one unresolved component blocking or authorizing a different component",
                "credit inferred only from a source-level eligibility label",
            ],
        },
        _ if contribution_mode == "exact-produced-damage" => TransferGate {
            kind: "unclassified-produced-output-hold",
            attribution_route: "none until source ownership and recount ancestry resolve",
            authority,
            runtime_credit_allowed: false,
            required_current_build_evidence: vec![
                "matching-build event source and owner resolution",
                "exact produced-damage child ancestry",
                "deduplicated canonical damage event",
            ],
            forbidden_transfers: vec![
                "any transfer before source ownership resolves",
                "duplicate parent plus child damage counting",
            ],
        },
        _ => TransferGate {
            kind: "mixed-or-unclassified-hold",
            attribution_route: "none until every mixed scope component resolves",
            authority,
            runtime_credit_allowed: false,
            required_current_build_evidence: vec![
                "matching-build provider, recipient, target, and lifecycle evidence",
                "component-by-component exact formula scope",
                "baseline and counterfactual replay with party conservation",
            ],
            forbidden_transfers: vec![
                "any rDPS transfer while mixed component scope remains unresolved",
            ],
        },
    }
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

fn optional_string_array_at(value: &Value, key: &str) -> Result<Vec<String>, Box<dyn Error>> {
    match value.get(key) {
        None | Some(Value::Null) => Ok(Vec::new()),
        Some(_) => string_array_at(value, key),
    }
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

fn signed_at(value: &Value, key: &str) -> Option<i64> {
    value.get(key).and_then(Value::as_i64)
}

fn integer_at(value: &Value, key: &str) -> Result<i64, Box<dyn Error>> {
    signed_at(value, key).ok_or_else(|| format!("{key} is missing or non-integer").into())
}

fn bool_at(value: &Value, key: &str) -> Result<bool, Box<dyn Error>> {
    value
        .get(key)
        .and_then(Value::as_bool)
        .ok_or_else(|| format!("{key} is missing or non-boolean").into())
}

fn unsigned_at(value: &Value, key: &str) -> u64 {
    value.get(key).and_then(Value::as_u64).unwrap_or_default()
}

fn display_path(path: &Path) -> String {
    path.to_string_lossy().replace('\\', "/")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn labels(values: &[&str]) -> Vec<String> {
        values.iter().map(|value| (*value).to_owned()).collect()
    }

    fn component(scope: &str, disposition: &str) -> CurrentComponentEvidence {
        CurrentComponentEvidence {
            skill_id: 3974,
            skill_name: "test".to_owned(),
            component_id: "test-component".to_owned(),
            role: "test-role".to_owned(),
            effect_ids: vec![2_110_143],
            recipient_scope: scope.to_owned(),
            rdps_disposition: disposition.to_owned(),
            proof_state: "test-proof".to_owned(),
        }
    }

    #[test]
    fn provider_only_enemy_target_state_is_not_an_external_target_component() {
        let evidence = CurrentComponentEvidence {
            role: "self-only-modifier".to_owned(),
            ..component(
                "source-restricted-enemy-target-state-for-provider-damage-only",
                "ordinary-owner-damage-never-transferred",
            )
        };

        assert!(!is_external_target_component(&evidence));
    }

    #[test]
    fn reviewed_external_target_role_is_an_external_target_component() {
        let evidence = CurrentComponentEvidence {
            role: "transferable-external-target-mitigation".to_owned(),
            ..component(
                "packet-observed-enemy-targets-hit-by-shared-blade-sweep-projectile",
                "preserve-exact-status-window-block-transfer-until-current-packet-provider-and-armor-formula",
            )
        };

        assert!(is_external_target_component(&evidence));
    }

    fn runtime_family(
        role: &str,
        opened_windows: u64,
        cross_actor_windows: u64,
    ) -> RuntimeEffectFamilyEvidence {
        RuntimeEffectFamilyEvidence {
            proof_state: "reviewed-test-proof".to_owned(),
            semantic_role: role.to_owned(),
            parent_effect_id: 2_203_230,
            child_effect_id: 2_203_231,
            historical_origin_observations: opened_windows,
            historical_child_status_events: opened_windows.max(1),
            historical_child_opened_windows: opened_windows,
            historical_child_cross_actor_windows: cross_actor_windows,
            current_build_packet_lifecycle_observed: false,
            formula_replay_allowed: false,
        }
    }

    fn component_packet_proof(packet_build: &str, target_kind: &str) -> Value {
        serde_json::json!({
            "schema_version": 1,
            "historical_packet_build": packet_build,
            "proof_state": "current-static-chain-exact-plus-historical-projectile-status-edge-current-packet-provider-live-gated",
            "current_static": {
                "target_status_id": 2110092
            },
            "historical_packet": {
                "applied_count": 3,
                "removed_count": 3,
                "source_actor_ids": [252, 254, 255],
                "target_actor_ids": [176],
                "source_actor_kind": "projectile",
                "target_actor_kinds": [target_kind]
            },
            "ownership_limits": {
                "player_provider_identity_available_in_historical_projection": false
            }
        })
    }

    fn denvel_proof(owner_cross_actor_windows: u64) -> Value {
        serde_json::json!({
            "historical_packet_build": "24252055",
            "proof_state": "denvel-test-fail-closed",
            "current_static": {
                "active_owner_buff": {
                    "effect_id": 2110137,
                    "role": "casting-player-self-only-active-damage-boost-controller"
                },
                "gravity_counter": {
                    "effect_id": 2110152,
                    "role": "affected-monster-self-sourced-gravity-counter-not-a-damage-modifier"
                },
                "active_formula": {
                    "recipient_scope": "casting-player-only"
                }
            },
            "historical_lifecycle": {
                "owner_buff": {
                    "status_events": 5,
                    "opened_windows": 3,
                    "cross_actor_windows": owner_cross_actor_windows,
                    "player_recipient_windows": 3,
                    "monster_recipient_windows": 0,
                    "self_sourced_examples": 3,
                    "non_self_sourced_examples": 0
                },
                "gravity_counter": {
                    "status_events": 9,
                    "opened_windows": 3,
                    "cross_actor_windows": 0,
                    "player_recipient_windows": 0,
                    "monster_recipient_windows": 3,
                    "self_sourced_examples": 3,
                    "non_self_sourced_examples": 0
                }
            },
            "attribution_policy": {
                "transferable_effect_ids": [],
                "current_build_packet_lifecycle_observed": false,
                "formula_replay_allowed_for_transfer": false
            }
        })
    }

    fn focused_shot_proof(stack_maximum: u64, stack_cross_actor_windows: u64) -> Value {
        serde_json::json!({
            "historical_packet_build": "24252055",
            "proof_state": "focused-shot-test-fail-closed",
            "current_static": {
                "controller": {"effect_id": 2203230},
                "stack": {"effect_id": 2203231},
                "formula": {
                    "qualifying_element": "light",
                    "damage_boost_per_stack_basis_points": 100,
                    "maximum_stacks": 4,
                    "duration_seconds": 3,
                    "recipient_scope": "casting-player-only"
                },
                "rejected_unrelated_focus": {"effect_id": 55223}
            },
            "historical_lifecycle": {
                "controller": {
                    "status_events": 4,
                    "opened_windows": 4,
                    "cross_actor_windows": 0,
                    "source_missing_windows": 0,
                    "player_recipient_windows": 4,
                    "monster_recipient_windows": 0,
                    "other_recipient_windows": 0,
                    "unresolved_recipient_windows": 0,
                    "maximum_stacks": 1
                },
                "stack": {
                    "status_events": 1416,
                    "opened_windows": 2,
                    "cross_actor_windows": stack_cross_actor_windows,
                    "source_missing_windows": 0,
                    "player_recipient_windows": 2,
                    "monster_recipient_windows": 0,
                    "other_recipient_windows": 0,
                    "unresolved_recipient_windows": 0,
                    "maximum_stacks": stack_maximum
                },
                "unrelated_focus": {
                    "status_events": 16,
                    "opened_windows": 8,
                    "cross_actor_windows": 0,
                    "source_missing_windows": 0,
                    "player_recipient_windows": 8,
                    "monster_recipient_windows": 0,
                    "other_recipient_windows": 0,
                    "unresolved_recipient_windows": 0,
                    "maximum_stacks": 1
                }
            },
            "attribution_policy": {
                "retained_runtime_effect_ids": [2203230, 2203231],
                "rejected_runtime_effect_ids": [55223],
                "transferable_effect_ids": [],
                "current_build_packet_lifecycle_observed": false,
                "formula_replay_allowed_for_transfer": false
            }
        })
    }

    fn stellar_spark_proof(stack_maximum: u64, stack_cross_actor_windows: u64) -> Value {
        serde_json::json!({
            "historical_packet_build": "24252055",
            "proof_state": "stellar-spark-test-fail-closed",
            "current_static": {
                "controller": {"effect_id": 2208420},
                "stack": {"effect_id": 2208421},
                "formula": {
                    "qualifying_trigger": "expertise-skill-damage",
                    "stat": "fireAttack",
                    "formula_term": "elementalAttack",
                    "formula_zone": "baseAttackTerm",
                    "fire_attack_per_stack": 22,
                    "maximum_stacks": 10,
                    "duration_seconds": 10,
                    "recipient_scope": "casting-player-only"
                }
            },
            "historical_lifecycle": {
                "controller": {
                    "status_events": 3,
                    "opened_windows": 3,
                    "cross_actor_windows": 0,
                    "source_missing_windows": 0,
                    "player_recipient_windows": 3,
                    "monster_recipient_windows": 0,
                    "other_recipient_windows": 0,
                    "unresolved_recipient_windows": 0,
                    "maximum_stacks": 1
                },
                "stack": {
                    "status_events": 896,
                    "opened_windows": 2,
                    "cross_actor_windows": stack_cross_actor_windows,
                    "source_missing_windows": 0,
                    "player_recipient_windows": 2,
                    "monster_recipient_windows": 0,
                    "other_recipient_windows": 0,
                    "unresolved_recipient_windows": 0,
                    "maximum_stacks": stack_maximum
                }
            },
            "attribution_policy": {
                "retained_runtime_effect_ids": [2208420, 2208421],
                "transferable_effect_ids": [],
                "current_build_packet_lifecycle_observed": false,
                "formula_replay_allowed_for_transfer": false
            }
        })
    }

    #[test]
    fn unresolved_scope_wins_over_direct_output_label() {
        assert_eq!(
            scope_queue(&labels(&[
                "direct-output-owned-by-source",
                "recipient-scope-unresolved",
            ])),
            "unresolved-provider-recipient"
        );
    }

    #[test]
    fn mixed_source_components_are_gated_independently() {
        let row = serde_json::json!({
            "relationship_components": [
                {
                    "componentKey": "cooldown-or-resource",
                    "contributionGroups": ["hitTiming"],
                    "direction": "timing",
                    "effectClass": "cooldown-or-resource",
                    "requiredRuntimeEvidence": ["cast timeline or cooldown state"],
                    "transferEligibility": "recipient-scope-unresolved",
                    "valueResolution": "ambiguous-multiple-values"
                },
                {
                    "componentKey": "atk",
                    "contributionGroups": ["baseAttack"],
                    "contributionScope": "all",
                    "direction": "stat",
                    "effectClass": "offense-stat",
                    "formulaTermIds": ["primaryAttack"],
                    "requiredRuntimeEvidence": ["attacker stat snapshot at hit time"],
                    "transferEligibility": "external-recipient-candidate",
                    "valueResolution": "owner-party-split"
                }
            ]
        });

        let routes = component_scope_routes(&row, "formula-replay-candidate").unwrap();
        assert_eq!(routes.len(), 2);
        assert_eq!(
            source_scope_queue(&routes, "unresolved-provider-recipient"),
            "component-scoped-mixed"
        );
        assert_eq!(routes[0].scope_queue, "unresolved-provider-recipient");
        assert_eq!(
            routes[0].rdps_relevance,
            "indirect-output-cadence-component"
        );
        assert_eq!(
            routes[1].scope_queue,
            "external-recipient-requires-current-build-proof"
        );
        assert_eq!(routes[1].rdps_relevance, "direct-damage-formula-component");
        assert_eq!(
            routes[1].transfer_gate.kind,
            "external-recipient-counterfactual"
        );
    }

    #[test]
    fn formula_input_dependency_is_not_mislabeled_as_a_granted_damage_modifier() {
        assert_eq!(
            component_rdps_relevance(
                "formula-input:matk",
                Some("formula-input-dependency"),
                Some("formula-input"),
                &labels(&["baseAttack"]),
                &labels(&["primaryAttack"]),
            ),
            "mechanic-formula-input-dependency"
        );
    }

    #[test]
    fn homogeneous_component_routes_preserve_their_specific_queue() {
        let row = serde_json::json!({
            "relationship_components": [
                {
                    "componentKey": "atk",
                    "contributionGroups": ["baseAttack"],
                    "formulaTermIds": ["primaryAttack"],
                    "transferEligibility": "external-recipient-candidate"
                },
                {
                    "componentKey": "matk",
                    "contributionGroups": ["baseAttack"],
                    "formulaTermIds": ["primaryAttack"],
                    "transferEligibility": "external-recipient-candidate"
                }
            ]
        });

        let routes = component_scope_routes(&row, "formula-replay-candidate").unwrap();
        assert_eq!(
            source_scope_queue(&routes, "unresolved-provider-recipient"),
            "external-recipient-requires-current-build-proof"
        );
    }

    #[test]
    fn static_owner_context_keeps_recipient_scope_open() {
        assert_eq!(
            scope_queue(&labels(&["self-only-formula-context"])),
            "owner-local-formula-context-requires-recipient-proof"
        );
        let (effective, resolution) =
            resolve_transfer_eligibilities(&labels(&["self-only-formula-context"]), &[]);
        assert_eq!(
            effective,
            labels(&["owner-local-formula-context-recipient-scope-open"])
        );
        assert_eq!(
            resolution,
            "declared-static-owner-context-kept-recipient-scope-open"
        );
    }

    #[test]
    fn non_outgoing_context_is_retained_without_offensive_transfer() {
        let queue = scope_queue(&labels(&["non-outgoing-context"]));
        assert_eq!(queue, "non-outgoing-context-no-offensive-transfer");
        let gate = transfer_gate(queue, "formula-replay-candidate");
        assert_eq!(gate.kind, "non-outgoing-context");
        assert!(!gate.runtime_credit_allowed);
        assert!(
            gate.forbidden_transfers
                .contains(&"recipient formula input substituted with provider state")
        );
    }

    #[test]
    fn external_target_state_stays_separate_from_recipient_buff() {
        assert_eq!(
            scope_queue(&labels(&["external-target-state-candidate"])),
            "external-target-state-requires-current-build-proof"
        );
    }

    #[test]
    fn external_recipient_gate_requires_distinct_provider_and_recipient() {
        let gate = transfer_gate(
            "external-recipient-requires-current-build-proof",
            "formula-replay-candidate",
        );

        assert_eq!(gate.kind, "external-recipient-counterfactual");
        assert!(!gate.runtime_credit_allowed);
        assert!(
            gate.required_current_build_evidence
                .contains(&"provider and recipient must differ for transferred credit")
        );
        assert!(
            gate.forbidden_transfers
                .contains(&"self-provided effect contribution")
        );
    }

    #[test]
    fn enemy_target_gate_keeps_provider_personal_damage_owned() {
        let gate = transfer_gate(
            "external-target-state-requires-current-build-proof",
            "formula-replay-candidate",
        );

        assert_eq!(gate.kind, "external-target-state-counterfactual");
        assert!(!gate.runtime_credit_allowed);
        assert!(
            gate.forbidden_transfers
                .contains(&"provider's own marginal damage through its target debuff")
        );
        assert!(
            gate.required_current_build_evidence.contains(
                &"attacker, target, and damage events inside the same target-state window"
            )
        );
    }

    #[test]
    fn unresolved_scope_gate_never_allows_runtime_credit() {
        let gate = transfer_gate("unresolved-provider-recipient", "formula-replay-candidate");

        assert_eq!(gate.kind, "unresolved-provider-recipient-hold");
        assert!(!gate.runtime_credit_allowed);
        assert!(
            gate.forbidden_transfers
                .contains(&"any rDPS transfer while provider or recipient scope is unresolved")
        );
    }

    #[test]
    fn mixed_direct_output_and_static_owner_context_keeps_recipient_scope_open() {
        assert_eq!(
            scope_queue(&labels(&[
                "direct-output-owned-by-source",
                "self-only-formula-context",
            ])),
            "mixed-source-output-and-open-owner-context"
        );
    }

    #[test]
    fn current_component_self_only_replaces_unresolved_scope() {
        let (effective, resolution) = resolve_transfer_eligibilities(
            &labels(&["recipient-scope-unresolved"]),
            &[component(
                "summon-owner-only",
                "ordinary-owner-damage-never-transferred",
            )],
        );

        assert_eq!(effective, labels(&["self-only-current-component-proof"]));
        assert_eq!(resolution, "exact-current-component-scope");
        assert_eq!(
            scope_queue(&effective),
            "self-only-formula-context-no-transfer"
        );
    }

    #[test]
    fn exact_self_only_component_replaces_weak_external_recipient_candidate() {
        let (effective, resolution) = resolve_transfer_eligibilities(
            &labels(&[
                "external-recipient-candidate",
                "external-target-state-candidate",
            ]),
            &[component(
                "source-restricted-enemy-target-state-for-provider-damage-only",
                "ordinary-owner-damage-never-transferred",
            )],
        );

        assert_eq!(
            effective,
            labels(&[
                "external-target-state-candidate",
                "self-only-current-component-proof",
            ])
        );
        assert_eq!(resolution, "exact-current-component-scope");
        assert_eq!(
            scope_queue(&effective),
            "external-target-state-requires-current-build-proof"
        );
    }

    #[test]
    fn exact_self_only_scope_reconciles_weak_component_route() {
        let row = serde_json::json!({
            "relationship_components": [{
                "componentKey": "target-vulnerability",
                "transferEligibility": "external-recipient-candidate",
                "requiredRuntimeEvidence": ["retain-this-audit-obligation"]
            }]
        });
        let routes = reconcile_component_scope_routes(
            component_scope_routes(&row, "formula-replay-candidate").unwrap(),
            &labels(&["self-only-current-component-proof"]),
            "formula-replay-candidate",
        );

        assert_eq!(routes.len(), 1);
        assert_eq!(
            routes[0].transfer_eligibility,
            "self-only-current-component-proof"
        );
        assert_eq!(
            routes[0].scope_queue,
            "self-only-formula-context-no-transfer"
        );
        assert_eq!(routes[0].transfer_gate.kind, "self-only-nontransfer");
        assert_eq!(
            routes[0].required_runtime_evidence,
            labels(&["retain-this-audit-obligation"])
        );
    }

    #[test]
    fn mixed_effective_scope_does_not_rewrite_component_route() {
        let row = serde_json::json!({
            "relationship_components": [{
                "componentKey": "target-vulnerability",
                "transferEligibility": "external-recipient-candidate"
            }]
        });
        let routes = reconcile_component_scope_routes(
            component_scope_routes(&row, "formula-replay-candidate").unwrap(),
            &labels(&[
                "external-target-state-candidate",
                "self-only-current-component-proof",
            ]),
            "formula-replay-candidate",
        );

        assert_eq!(
            routes[0].transfer_eligibility,
            "external-recipient-candidate"
        );
        assert_eq!(
            routes[0].scope_queue,
            "external-recipient-requires-current-build-proof"
        );
    }

    #[test]
    fn external_component_does_not_rewrite_an_already_resolved_declared_scope() {
        let declared = labels(&["external-recipient-candidate"]);
        let (effective, resolution) = resolve_transfer_eligibilities(
            &declared,
            &[component(
                "provider-and-external-teammates-in-area",
                "exact-attack-and-mattack-counterfactual-only",
            )],
        );

        assert_eq!(effective, declared);
        assert_eq!(
            resolution,
            "declared-static-scope-preserved-current-component-mixed"
        );
    }

    #[test]
    fn current_component_external_replaces_unresolved_scope() {
        let (effective, resolution) = resolve_transfer_eligibilities(
            &labels(&["recipient-scope-unresolved"]),
            &[component(
                "provider-and-external-teammates-in-area",
                "exact-attack-and-mattack-counterfactual-only",
            )],
        );

        assert_eq!(effective, labels(&["external-recipient-candidate"]));
        assert_eq!(resolution, "exact-current-component-scope");
        assert_eq!(
            scope_queue(&effective),
            "external-recipient-requires-current-build-proof"
        );
    }

    #[test]
    fn mixed_current_component_scope_stays_unresolved() {
        let (effective, resolution) = resolve_transfer_eligibilities(
            &labels(&["recipient-scope-unresolved"]),
            &[
                component(
                    "summon-owner-only",
                    "ordinary-owner-damage-never-transferred",
                ),
                component(
                    "provider-and-external-teammates-in-area",
                    "exact-attack-and-mattack-counterfactual-only",
                ),
            ],
        );

        assert_eq!(effective, labels(&["recipient-scope-unresolved"]));
        assert_eq!(
            resolution,
            "declared-static-scope-preserved-current-component-mixed"
        );
        assert_eq!(scope_queue(&effective), "unresolved-provider-recipient");
    }

    #[test]
    fn reviewed_owner_only_runtime_family_resolves_semantic_scope_without_runtime_promotion() {
        let (effective, resolution) = resolve_proven_runtime_family_scope(
            &labels(&["recipient-scope-unresolved"]),
            "declared-static-scope-preserved",
            &[runtime_family(
                "owner-only-one-percent-light-stack-up-to-four",
                2,
                0,
            )],
        );

        assert_eq!(effective, labels(&["self-only-runtime-family-proof"]));
        assert_eq!(
            resolution,
            "reviewed-current-static-plus-historical-runtime-family-self-only"
        );
        assert_eq!(
            scope_queue(&effective),
            "self-only-formula-context-no-transfer"
        );
    }

    #[test]
    fn component_route_preserves_declared_owner_context_but_keeps_effective_scope_open() {
        let row = serde_json::json!({
            "relationship_components": [{
                "componentKey": "critical-rate",
                "transferEligibility": "self-only-formula-context"
            }]
        });
        let routes = component_scope_routes(&row, "formula-replay-candidate").unwrap();

        assert_eq!(routes.len(), 1);
        assert_eq!(
            routes[0].declared_transfer_eligibility,
            "self-only-formula-context"
        );
        assert_eq!(
            routes[0].transfer_eligibility,
            "owner-local-formula-context-recipient-scope-open"
        );
        assert_eq!(
            routes[0].scope_queue,
            "owner-local-formula-context-requires-recipient-proof"
        );
        assert_eq!(
            routes[0].transfer_gate.kind,
            "owner-local-formula-context-scope-hold"
        );
    }

    #[test]
    fn runtime_family_scope_stays_unresolved_when_role_or_lifecycle_is_not_self_only() {
        for evidence in [
            runtime_family("external-recipient-candidate", 2, 0),
            runtime_family("owner-only-test", 0, 0),
            runtime_family("owner-only-test", 2, 1),
        ] {
            let (effective, _) = resolve_proven_runtime_family_scope(
                &labels(&["recipient-scope-unresolved"]),
                "declared-static-scope-preserved",
                &[evidence],
            );
            assert_eq!(effective, labels(&["recipient-scope-unresolved"]));
        }
    }

    #[test]
    fn provider_audit_counts_replace_overlapping_compact_event_totals() {
        let compact = BTreeMap::from([(
            42,
            HistoricalPacketEvidence {
                compact_effect_rows_present: 1,
                compact_selected_status_events: 11,
                evidence_authority: "historical-packet-corpus-research-only",
                ..HistoricalPacketEvidence::default()
            },
        )]);
        let provider_audit = BTreeMap::from([(
            42,
            HistoricalPacketEvidence {
                provider_audit_rows_present: 2,
                provider_audit_rows_with_status_events: 2,
                authoritative_status_events: 7,
                opened_windows: 3,
                evidence_authority: "historical-packet-corpus-research-only",
                ..HistoricalPacketEvidence::default()
            },
        )]);

        let merged = merge_packet_evidence(&[42], &compact, &BTreeMap::new(), &provider_audit);

        assert_eq!(merged.compact_selected_status_events, 11);
        assert_eq!(merged.authoritative_status_events, 7);
        assert_eq!(merged.opened_windows, 3);
    }

    #[test]
    fn packet_inventory_proves_occurrence_without_inventing_lifecycle() {
        let inventory = BTreeMap::from([(
            42,
            HistoricalPacketEvidence {
                inventory_effect_rows_present: 1,
                inventory_status_events: 9,
                inventory_origin_observations: 2,
                authoritative_status_events: 9,
                evidence_authority: "historical-packet-inventory-occurrence-only",
                ..HistoricalPacketEvidence::default()
            },
        )]);
        let merged = merge_packet_evidence(&[42], &BTreeMap::new(), &inventory, &BTreeMap::new());
        assert_eq!(merged.inventory_status_events, 9);
        assert_eq!(merged.authoritative_status_events, 9);
        assert_eq!(merged.opened_windows, 0);
        assert_eq!(merged.cross_actor_windows, 0);
    }

    #[test]
    fn packet_family_traversal_is_transitive_and_cycle_safe() {
        let edge = |parent, child| PacketObservedEffectFamilyEdge {
            parent_effect_id: parent,
            child_effect_id: child,
            source_type_id: 1,
            observation_count: 1,
            evidence_authority: "historical-packet-origin-lineage-only",
        };
        let by_parent = BTreeMap::from([
            (10, vec![edge(10, 11)]),
            (11, vec![edge(11, 12)]),
            (12, vec![edge(12, 10)]),
        ]);
        let rows = collect_reachable_packet_family_edges(&[10], &by_parent);
        assert_eq!(rows.len(), 3);
        assert_eq!(
            rows.iter()
                .map(|row| row.child_effect_id)
                .collect::<BTreeSet<_>>(),
            BTreeSet::from([10, 11, 12])
        );
    }

    #[test]
    fn summon_proxy_source_does_not_become_an_external_player_provider() {
        let audit = serde_json::json!({
            "reports": [{
                "effects": [{
                    "effect_id": 2110138,
                    "lifecycle": {
                        "status_events": 2,
                        "opened_windows": 1,
                        "cross_actor_windows": 1,
                        "source_missing_windows": 0
                    },
                    "recipient_scope": {"player": 1, "monster": 0},
                    "providers": [{
                        "resolved_provider_entity_uuid": 216009015936_u64,
                        "resolution": "owner_link_within_run",
                        "windows": 1,
                        "raw_source_entities": [229440]
                    }],
                    "examples": [{
                        "raw_source_entity_uuid": 229440,
                        "resolved_provider_entity_uuid": 216009015936_u64,
                        "provider_resolution": "owner_link_within_run",
                        "target_entity_uuid": 216009015936_u64
                    }]
                }]
            }]
        });

        let evidence = collect_provider_audit_evidence(&audit).unwrap();
        let effect = evidence.get(&2_110_138).unwrap();
        assert_eq!(effect.cross_actor_windows, 1);
        assert_eq!(effect.raw_proxy_source_windows, 1);
        assert_eq!(effect.owner_linked_player_provider_windows, 1);
        assert_eq!(effect.resolved_provider_is_recipient_examples, 1);
        assert!(effect.provider_is_recipient_observed);
        assert!(!effect.provider_differs_from_recipient_observed);
        assert!(!effect.resolved_external_player_provider_observed);
    }

    #[test]
    fn resolved_external_player_provider_is_counted_separately() {
        let audit = serde_json::json!({
            "reports": [{
                "effects": [{
                    "effect_id": 42,
                    "lifecycle": {
                        "status_events": 1,
                        "opened_windows": 1,
                        "cross_actor_windows": 1,
                        "source_missing_windows": 0
                    },
                    "recipient_scope": {"player": 1, "monster": 0},
                    "providers": [{
                        "resolved_provider_entity_uuid": 100,
                        "resolution": "owner_link_within_run",
                        "windows": 1,
                        "raw_source_entities": [100]
                    }],
                    "examples": [{
                        "raw_source_entity_uuid": 100,
                        "resolved_provider_entity_uuid": 100,
                        "provider_resolution": "owner_link_within_run",
                        "target_entity_uuid": 200
                    }]
                }]
            }]
        });

        let evidence = collect_provider_audit_evidence(&audit).unwrap();
        let effect = evidence.get(&42).unwrap();
        assert_eq!(effect.raw_proxy_source_windows, 0);
        assert_eq!(effect.resolved_provider_differs_from_recipient_examples, 1);
        assert!(effect.provider_differs_from_recipient_observed);
        assert!(effect.resolved_external_player_provider_observed);
    }

    #[test]
    fn component_packet_proof_preserves_unresolved_projectile_monster_windows() {
        let proof = component_packet_proof("24252055", "monster");
        let (effect_id, evidence) = collect_component_packet_evidence(&proof, "24252055").unwrap();

        assert_eq!(effect_id, 2_110_092);
        assert_eq!(evidence.authoritative_status_events, 6);
        assert_eq!(evidence.opened_windows, 3);
        assert_eq!(evidence.raw_proxy_source_windows, 3);
        assert_eq!(evidence.non_player_provider_windows, 3);
        assert_eq!(evidence.monster_recipient_windows, 1);
        assert!(evidence.provider_identity_unresolved_observed);
        assert!(!evidence.resolved_external_player_provider_observed);
    }

    #[test]
    fn component_packet_proof_rejects_player_recipient() {
        let proof = component_packet_proof("24252055", "player");

        assert!(collect_component_packet_evidence(&proof, "24252055").is_err());
    }

    #[test]
    fn component_packet_proof_rejects_build_drift() {
        let proof = component_packet_proof("24252055", "monster");

        assert!(collect_component_packet_evidence(&proof, "24609362").is_err());
    }

    #[test]
    fn battle_cry_family_retains_both_children_without_promoting_ally_scope() {
        let proof = serde_json::json!({
            "historical_packet_build": "24252055",
            "proof_state": "battle-cry-test-fail-closed",
            "current_static": {
                "parent_effect_id": 2205310,
                "child_effects": [
                    {"effect_id": 2205311, "role": "runtime-child-owner-or-controller-branch"},
                    {"effect_id": 2205312, "role": "runtime-child-countdown-branch"}
                ]
            },
            "historical_origin_edges": [
                {"effect_id": 2205311, "source_type_id": 1, "source_config_id": 2205310, "observation_count": 1},
                {"effect_id": 2205312, "source_type_id": 1, "source_config_id": 2205310, "observation_count": 1}
            ],
            "historical_lifecycle": {
                "owner_child": {"status_events": 2, "opened_windows": 2, "cross_actor_windows": 0},
                "countdown_child": {"status_events": 4, "opened_windows": 2, "cross_actor_windows": 0}
            },
            "attribution_policy": {
                "current_build_packet_lifecycle_observed": false,
                "external_recipient_child_effect_proven": false,
                "formula_replay_allowed": false
            }
        });

        let families = collect_battle_cry_runtime_effect_families(&proof, "24252055").unwrap();
        let children = families.get(&2_205_310).unwrap();
        assert_eq!(children.len(), 2);
        assert!(children.iter().all(|child| !child.formula_replay_allowed));
        assert!(
            children
                .iter()
                .all(|child| child.historical_child_cross_actor_windows == 0)
        );
    }

    #[test]
    fn battle_cry_family_rejects_unreviewed_external_child_promotion() {
        let proof = serde_json::json!({
            "historical_packet_build": "24252055",
            "proof_state": "invalid-battle-cry-promotion",
            "current_static": {
                "parent_effect_id": 2205310,
                "child_effects": [
                    {"effect_id": 2205311, "role": "runtime-child-owner-or-controller-branch"},
                    {"effect_id": 2205312, "role": "runtime-child-countdown-branch"}
                ]
            },
            "historical_origin_edges": [
                {"effect_id": 2205311, "source_type_id": 1, "source_config_id": 2205310, "observation_count": 1},
                {"effect_id": 2205312, "source_type_id": 1, "source_config_id": 2205310, "observation_count": 1}
            ],
            "historical_lifecycle": {
                "owner_child": {"status_events": 2, "opened_windows": 2, "cross_actor_windows": 0},
                "countdown_child": {"status_events": 4, "opened_windows": 2, "cross_actor_windows": 0}
            },
            "attribution_policy": {
                "current_build_packet_lifecycle_observed": false,
                "external_recipient_child_effect_proven": true,
                "formula_replay_allowed": false
            }
        });

        assert!(collect_battle_cry_runtime_effect_families(&proof, "24252055").is_err());
    }

    #[test]
    fn denvel_family_retains_owner_and_gravity_scopes_without_transfer() {
        let families =
            collect_denvel_runtime_effect_families(&denvel_proof(0), "24252055").unwrap();
        let evidence = families.get(&2_110_137).unwrap();

        assert_eq!(evidence.len(), 2);
        assert_eq!(
            evidence
                .iter()
                .map(|row| row.child_effect_id)
                .collect::<BTreeSet<_>>(),
            BTreeSet::from([2_110_137, 2_110_152])
        );
        assert!(evidence.iter().all(|row| !row.formula_replay_allowed));
        assert!(
            evidence
                .iter()
                .all(|row| row.historical_child_cross_actor_windows == 0)
        );
    }

    #[test]
    fn denvel_family_rejects_cross_actor_owner_scope() {
        assert!(collect_denvel_runtime_effect_families(&denvel_proof(1), "24252055").is_err());
    }

    #[test]
    fn focused_shot_family_retains_exact_controller_and_four_stack_child() {
        let families =
            collect_focused_shot_runtime_effect_families(&focused_shot_proof(4, 0), "24252055")
                .unwrap();
        let evidence = families.get(&2_203_230).unwrap();
        assert_eq!(evidence.len(), 2);
        assert_eq!(
            evidence
                .iter()
                .map(|row| row.child_effect_id)
                .collect::<BTreeSet<_>>(),
            BTreeSet::from([2_203_230, 2_203_231])
        );
        assert!(evidence.iter().all(|row| !row.formula_replay_allowed));
    }

    #[test]
    fn focused_shot_family_rejects_wrong_stack_or_cross_actor_scope() {
        assert!(
            collect_focused_shot_runtime_effect_families(&focused_shot_proof(3, 0), "24252055")
                .is_err()
        );
        assert!(
            collect_focused_shot_runtime_effect_families(&focused_shot_proof(4, 1), "24252055")
                .is_err()
        );
    }

    #[test]
    fn stellar_spark_family_retains_exact_controller_and_ten_stack_child() {
        let families =
            collect_stellar_spark_runtime_effect_families(&stellar_spark_proof(10, 0), "24252055")
                .unwrap();
        let evidence = families.get(&2_208_420).unwrap();
        assert_eq!(evidence.len(), 2);
        assert_eq!(
            evidence
                .iter()
                .map(|row| row.child_effect_id)
                .collect::<BTreeSet<_>>(),
            BTreeSet::from([2_208_420, 2_208_421])
        );
        assert!(evidence.iter().all(|row| !row.formula_replay_allowed));
    }

    #[test]
    fn stellar_spark_family_rejects_wrong_stack_or_cross_actor_scope() {
        assert!(
            collect_stellar_spark_runtime_effect_families(&stellar_spark_proof(9, 0), "24252055")
                .is_err()
        );
        assert!(
            collect_stellar_spark_runtime_effect_families(&stellar_spark_proof(10, 1), "24252055")
                .is_err()
        );
    }
}
