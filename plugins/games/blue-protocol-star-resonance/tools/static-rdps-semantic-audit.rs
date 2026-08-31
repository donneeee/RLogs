use std::{
    collections::{BTreeMap, BTreeSet},
    env,
    error::Error,
    fs::{self, File},
    io::{BufReader, BufWriter, Write},
    path::{Path, PathBuf},
};

use serde::Serialize;
use serde_json::Value;

const SCHEMA_VERSION: u16 = 2;

#[derive(Debug, Serialize)]
#[serde(deny_unknown_fields)]
struct SemanticAudit {
    schema_version: u16,
    generated_by: &'static str,
    game_build: String,
    promotion_state: &'static str,
    policy: Policy,
    inputs: Inputs,
    summary: Summary,
    findings: Vec<Finding>,
}

#[derive(Debug, Serialize)]
#[serde(deny_unknown_fields)]
struct Policy {
    unresolved_evidence_hidden: bool,
    audit_mutates_source_catalogs: bool,
    audit_enables_rdps: bool,
    semantic_correction_required_before_replay: bool,
    matching_build_packet_replay_required: bool,
}

#[derive(Debug, Serialize)]
#[serde(deny_unknown_fields)]
struct Inputs {
    worklist: String,
    effect_sources: String,
}

#[derive(Debug, Serialize)]
#[serde(deny_unknown_fields)]
struct Summary {
    candidates_audited: usize,
    candidates_with_effect_source: usize,
    candidates_without_effect_source: usize,
    candidates_with_findings: usize,
    promotion_blocked_candidates: usize,
    findings_by_category: BTreeMap<String, usize>,
}

#[derive(Debug, Serialize)]
#[serde(deny_unknown_fields)]
struct Finding {
    source_rule_id: String,
    source_id: Option<String>,
    source_name: Option<String>,
    contribution_mode: String,
    static_value_state: String,
    formula_term_ids: Vec<String>,
    transfer_eligibilities: Vec<String>,
    target_damage_ids: Vec<i64>,
    selected_value_raw_texts: Vec<String>,
    value_selector_kinds: Vec<String>,
    description: Option<String>,
    existing_components: Vec<ComponentSummary>,
    issues: Vec<Issue>,
    promotion_blocked: bool,
}

#[derive(Debug, Serialize)]
#[serde(deny_unknown_fields)]
struct ComponentSummary {
    component_key: String,
    effect_class: Option<String>,
    direction: Option<String>,
    has_activation_predicate: bool,
    formula_term_ids: Vec<String>,
    predicate_tags: Vec<String>,
    required_runtime_evidence: Vec<String>,
    value_texts: Vec<String>,
    target_damage_ids: Vec<i64>,
    target_recount_ids: Vec<i64>,
    target_skill_names: Vec<String>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
struct Issue {
    category: &'static str,
    severity: &'static str,
    evidence: String,
    required_model: &'static str,
    promotion_blocker: bool,
}

#[derive(Debug)]
struct CandidateContext<'a> {
    description: &'a str,
    contribution_mode: &'a str,
    static_value_state: &'a str,
    formula_term_ids: &'a [String],
    transfer_eligibilities: &'a [String],
    target_damage_ids: &'a [i64],
    selected_value_raw_texts: &'a [String],
    value_selector_kinds: &'a [String],
    components: &'a [ComponentSummary],
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
    let effect_sources_path = required_path(&options, "effect-sources")?;
    let game_build = required(&options, "build")?.to_owned();
    let output_path = required_path(&options, "output")?;
    validate_build(&game_build)?;

    let worklist = read_json(&worklist_path)?;
    let effect_sources = read_json(&effect_sources_path)?;
    require_generated_by(&worklist, "generated_by", "rlogs-bpsr-static-rdps-worklist")?;
    require_generated_by(
        effect_sources
            .get("summary")
            .ok_or("EffectSources summary is missing")?,
        "source",
        "EffectSources.gen",
    )?;
    if string_at(&worklist, "game_build") != Some(game_build.as_str()) {
        return Err("worklist build does not match --build".into());
    }

    let sources = object_at(&effect_sources, "effectSourcesById")?;
    let candidates = worklist
        .get("exact_produced_damage_candidates")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .chain(
            worklist
                .get("formula_replay_candidates")
                .and_then(Value::as_array)
                .into_iter()
                .flatten(),
        );

    let mut findings = Vec::new();
    let mut candidates_audited = 0usize;
    let mut candidates_with_effect_source = 0usize;
    let mut candidates_without_effect_source = 0usize;
    let mut category_counts = BTreeMap::<String, usize>::new();

    for candidate in candidates {
        candidates_audited += 1;
        let source_rule_id = string_at(candidate, "source_rule_id")
            .ok_or("candidate source_rule_id is missing")?
            .to_owned();
        let source_id = string_at(candidate, "source_id").map(str::to_owned);
        let effect_source = source_id.as_deref().and_then(|id| sources.get(id));
        if effect_source.is_some() {
            candidates_with_effect_source += 1;
        } else {
            candidates_without_effect_source += 1;
        }

        let description = effect_source.and_then(english_description);
        let components = component_summaries(effect_source);
        let formula_term_ids = string_array(candidate.get("formula_term_ids"));
        let transfer_eligibilities = string_array(candidate.get("transfer_eligibilities"));
        let target_damage_ids = integer_array(
            candidate
                .get("runtime_matcher")
                .and_then(|matcher| matcher.get("target_damage_ids")),
        );
        let selected_value_raw_texts =
            nested_strings(candidate, "value_proofs", "selected_values", "rawText");
        let value_selector_kinds =
            nested_strings(candidate, "value_proofs", "value_selectors", "kind");
        let contribution_mode = string_at(candidate, "contribution_mode").unwrap_or("unresolved");
        let static_value_state = string_at(candidate, "static_value_state").unwrap_or("unresolved");
        let mut issues = audit_candidate(&CandidateContext {
            description: description.as_deref().unwrap_or(""),
            contribution_mode,
            static_value_state,
            formula_term_ids: &formula_term_ids,
            transfer_eligibilities: &transfer_eligibilities,
            target_damage_ids: &target_damage_ids,
            selected_value_raw_texts: &selected_value_raw_texts,
            value_selector_kinds: &value_selector_kinds,
            components: &components,
        });
        if effect_source.is_none() {
            issues.push(Issue {
                category: "missing-effect-source-join",
                severity: "error",
                evidence:
                    "The worklist source_id does not join to EffectSources.effectSourcesById."
                        .to_owned(),
                required_model: "source-identity-join",
                promotion_blocker: true,
            });
        }
        issues.sort_by(|left, right| left.category.cmp(right.category));
        issues.dedup_by(|left, right| left.category == right.category);
        if issues.is_empty() {
            continue;
        }
        for issue in &issues {
            *category_counts
                .entry(issue.category.to_owned())
                .or_default() += 1;
        }
        let promotion_blocked = issues.iter().any(|issue| issue.promotion_blocker);
        findings.push(Finding {
            source_rule_id,
            source_id,
            source_name: effect_source
                .and_then(|source| string_at(source, "sourceName"))
                .map(str::to_owned),
            contribution_mode: contribution_mode.to_owned(),
            static_value_state: static_value_state.to_owned(),
            formula_term_ids,
            transfer_eligibilities,
            target_damage_ids,
            selected_value_raw_texts,
            value_selector_kinds,
            description,
            existing_components: components,
            issues,
            promotion_blocked,
        });
    }

    findings.sort_by(|left, right| left.source_rule_id.cmp(&right.source_rule_id));
    let promotion_blocked_candidates = findings
        .iter()
        .filter(|finding| finding.promotion_blocked)
        .count();
    let audit = SemanticAudit {
        schema_version: SCHEMA_VERSION,
        generated_by: "rlogs-bpsr-static-rdps-semantic-audit",
        game_build,
        promotion_state: "audit-only-no-runtime-authority",
        policy: Policy {
            unresolved_evidence_hidden: false,
            audit_mutates_source_catalogs: false,
            audit_enables_rdps: false,
            semantic_correction_required_before_replay: true,
            matching_build_packet_replay_required: true,
        },
        inputs: Inputs {
            worklist: file_name(&worklist_path),
            effect_sources: file_name(&effect_sources_path),
        },
        summary: Summary {
            candidates_audited,
            candidates_with_effect_source,
            candidates_without_effect_source,
            candidates_with_findings: findings.len(),
            promotion_blocked_candidates,
            findings_by_category: category_counts,
        },
        findings,
    };

    if let Some(parent) = output_path.parent() {
        fs::create_dir_all(parent)?;
    }
    let mut writer = BufWriter::new(File::create(output_path)?);
    serde_json::to_writer_pretty(&mut writer, &audit)?;
    writer.write_all(b"\n")?;
    writer.flush()?;
    Ok(())
}

fn audit_candidate(context: &CandidateContext<'_>) -> Vec<Issue> {
    let text = context.description.to_ascii_lowercase();
    let component_keys = context
        .components
        .iter()
        .map(|component| component.component_key.as_str())
        .collect::<BTreeSet<_>>();
    let directions = context
        .components
        .iter()
        .filter_map(|component| component.direction.as_deref())
        .collect::<BTreeSet<_>>();
    let has_formula_terms = !context.formula_term_ids.is_empty();
    let has_produced_damage = component_keys.contains("produced-damage")
        || context.contribution_mode == "exact-produced-damage";
    let healing_text = contains_any(
        &text,
        &[
            " heal",
            "heals",
            "healing",
            "restore hp",
            "restores hp",
            "restored hp",
            "restoring hp",
            "recover hp",
            "recovers hp",
            "recovered hp",
            "recovering hp",
        ],
    );
    let (healing_attack_coefficient, damage_attack_coefficient) =
        attack_coefficient_contexts(&text);
    let attack_formula = context
        .formula_term_ids
        .iter()
        .any(|term| matches!(term.as_str(), "primaryAttack" | "baseAttack"));

    let mut issues = Vec::new();
    if healing_text && healing_attack_coefficient && attack_formula && !damage_attack_coefficient {
        issues.push(Issue {
            category: "healing-coefficient-as-offense-stat",
            severity: "error",
            evidence: "The description uses ATK as a healing coefficient, but the generated rule modifies an attack formula term.".to_owned(),
            required_model: "healing-coefficient",
            promotion_blocker: true,
        });
    }

    let trigger_damage_text = contains_any(&text, &["when ", "whenever ", "after "])
        && contains_any(
            &text,
            &[
                "deal dmg",
                "deals dmg",
                "deal damage",
                "deals damage",
                "inflicts damage",
                "causes damage",
            ],
        );
    let non_damage_outcome = healing_text
        || contains_any(
            &text,
            &[
                " grants ",
                " grant ",
                "gains ",
                "gain ",
                "restores energy",
                "recovers energy",
            ],
        );
    if has_produced_damage
        && trigger_damage_text
        && non_damage_outcome
        && !has_explicit_produced_damage_output(&text)
    {
        issues.push(Issue {
            category: "trigger-text-as-produced-damage",
            severity: "error",
            evidence: "Damage appears in the trigger clause, while the described outcome is healing/resource/status application; no new damage output is established by that sentence.".to_owned(),
            required_model: "trigger-outcome-clause-separation",
            promotion_blocker: true,
        });
    }

    let stack_cap_values = selector_values_after_markers(
        &text,
        &[
            "cap +",
            "cap by ",
            "maximum stacks +",
            "max stacks +",
            "stack limit +",
            "stacks up to ",
        ],
    );
    let selected_values = context
        .selected_value_raw_texts
        .iter()
        .flat_map(|value| numeric_tokens(value))
        .collect::<BTreeSet<_>>();
    let stack_cap_is_component_isolated = context.components.iter().any(|component| {
        component.direction.as_deref() == Some("timing")
            && component
                .value_texts
                .iter()
                .flat_map(|value| numeric_tokens(value))
                .any(|value| stack_cap_values.contains(&value))
            && component.formula_term_ids.is_empty()
    });
    if has_formula_terms
        && !stack_cap_values.is_empty()
        && !selected_values.is_disjoint(&stack_cap_values)
        && !stack_cap_is_component_isolated
        && context
            .value_selector_kinds
            .iter()
            .all(|kind| !kind.contains("stack"))
    {
        issues.push(Issue {
            category: "stack-cap-as-direct-formula-value",
            severity: "error",
            evidence: "A stack-cap or maximum-stack value was promoted as a direct damage/stat formula value.".to_owned(),
            required_model: "stack-cap-selector",
            promotion_blocker: true,
        });
    }

    if has_formula_terms
        && contains_any(
            &text,
            &[" reaches ", "at least ", "when strength", "when str"],
        )
        && contains_any(&text, &["increases to", "changes to", "becomes "])
        && context
            .value_selector_kinds
            .iter()
            .all(|kind| !kind.contains("threshold"))
    {
        issues.push(Issue {
            category: "threshold-selector-as-direct-formula-value",
            severity: "error",
            evidence: "The description selects a replacement value after a threshold; it is not one unconditional modifier amount.".to_owned(),
            required_model: "threshold-value-selector",
            promotion_blocker: true,
        });
    }

    let exact_skill_components: Vec<_> = context
        .components
        .iter()
        .filter(|component| {
            component
                .predicate_tags
                .iter()
                .any(|tag| tag == "target.exact-skill-id-required")
        })
        .collect();
    if has_formula_terms
        && exact_skill_components.iter().any(|component| {
            component.target_damage_ids.is_empty() && component.target_recount_ids.is_empty()
        })
    {
        issues.push(Issue {
            category: "skill-specific-multiplier-needs-target-skill",
            severity: "error",
            evidence: "The value applies to a named skill/action, but the generated candidate uses a generic formula term without a proved target-skill predicate.".to_owned(),
            required_model: "conditional-skill-multiplier",
            promotion_blocker: true,
        });
    }

    if has_produced_damage && context.target_damage_ids.is_empty() {
        issues.push(Issue {
            category: "produced-damage-without-packet-row",
            severity: "error",
            evidence: "Produced damage is claimed, but the runtime matcher has no exact target damage row ID.".to_owned(),
            required_model: "produced-damage-row-bridge",
            promotion_blocker: true,
        });
    }

    let timing_text = contains_any(
        &text,
        &[
            "cooldown",
            "casting speed",
            "attack speed",
            "gain energy",
            "gains energy",
            "restore energy",
            "resource",
        ],
    );
    let has_damage_or_stat_direction = directions.iter().any(|direction| {
        matches!(
            *direction,
            "damage-dealt" | "stat" | "target-mitigation" | "damage-taken"
        )
    });
    if timing_text && has_formula_terms && !has_damage_or_stat_direction {
        issues.push(Issue {
            category: "cooldown-or-resource-as-damage-formula",
            severity: "error",
            evidence: "A cooldown/resource mechanic has damage formula terms without an independently modeled damage/stat component.".to_owned(),
            required_model: "timing-or-resource-only",
            promotion_blocker: true,
        });
    }
    let compound_components = context
        .components
        .iter()
        .filter(|component| {
            matches!(
                component.direction.as_deref(),
                Some("timing")
                    | Some("damage-dealt")
                    | Some("stat")
                    | Some("target-mitigation")
                    | Some("damage-taken")
            )
        })
        .collect::<Vec<_>>();
    let compound_component_keys = compound_components
        .iter()
        .map(|component| component.component_key.as_str())
        .collect::<BTreeSet<_>>();
    let compound_components_are_separated = compound_components.len() >= 2
        && compound_component_keys.len() == compound_components.len()
        && compound_components.iter().all(|component| {
            component.component_key != "unresolved"
                && (component.has_activation_predicate || !component.predicate_tags.is_empty())
        });
    if directions.contains("timing")
        && has_damage_or_stat_direction
        && !compound_components_are_separated
    {
        issues.push(Issue {
            category: "mixed-timing-and-damage-components",
            severity: "warning",
            evidence: "Timing and damage/stat behavior share one generated source without distinct component keys and runtime predicate tags for every participating mechanic. Missing magnitudes remain independently visible in the formula-magnitude audit.".to_owned(),
            required_model: "component-separated-compound-effect",
            promotion_blocker: true,
        });
    }

    if has_formula_terms
        && contains_any(
            &text,
            &[
                "chance of obtaining",
                "chance to obtain",
                "obtaining crit entries",
                "obtain affixes",
                "starting affix",
                "initial affix",
            ],
        )
    {
        issues.push(Issue {
            category: "acquisition-rule-as-live-combat-modifier",
            severity: "error",
            evidence: "The rule changes loot/affix acquisition rather than a live combat state."
                .to_owned(),
            required_model: "non-combat-configuration",
            promotion_blocker: true,
        });
    }

    if context.contribution_mode == "formula-replay-candidate"
        && matches!(
            context.static_value_state,
            "needs-value-selection" | "missing-value-proof"
        )
    {
        let self_only = !context.transfer_eligibilities.is_empty()
            && context
                .transfer_eligibilities
                .iter()
                .all(|value| value == "self-only-formula-context");
        issues.push(Issue {
            category: if self_only {
                "formula-context-magnitude-unresolved"
            } else {
                "formula-magnitude-unresolved"
            },
            severity: "error",
            evidence: if self_only {
                "No exact formula value or runtime selector is selected for this self-only damage context. It remains necessary for recipient damage reconstruction but cannot create external rDPS credit."
                    .to_owned()
            } else {
                "No exact formula value or runtime selector is currently selected for this candidate."
                    .to_owned()
            },
            required_model: "exact-value-or-runtime-selector",
            promotion_blocker: true,
        });
    }

    if context.contribution_mode == "formula-replay-candidate"
        && (context.transfer_eligibilities.is_empty()
            || context
                .transfer_eligibilities
                .iter()
                .any(|value| value.starts_with("recipient-scope-unresolved")))
    {
        issues.push(Issue {
            category: "formula-recipient-scope-unresolved",
            severity: "error",
            evidence: "The formula term is preserved, but static evidence does not prove whether it is self-only or transferable. Packet provider/recipient lifecycle must resolve that scope before rDPS credit."
                .to_owned(),
            required_model: "packet-provider-recipient-scope",
            promotion_blocker: true,
        });
    }

    issues
}

fn attack_coefficient_contexts(text: &str) -> (bool, bool) {
    let coefficient_markers = ["% atk", "% of atk", "% attack", "% of attack"];
    let healing_markers = [
        " heal",
        "heals",
        "healing",
        "restore hp",
        "restores hp",
        "restoring hp",
        "recover hp",
        "recovers hp",
        "recovering hp",
    ];
    let damage_markers = [
        "deal dmg",
        "deals dmg",
        "deal magic dmg",
        "deals magic dmg",
        "deal attack dmg",
        "deals attack dmg",
        "dealing dmg",
        "dealing magic dmg",
        "dealing attack dmg",
        "attack dmg",
        "takes dmg",
        "deal damage",
        "deals damage",
        "dealing damage",
        "takes damage",
    ];
    let mut healing = false;
    let mut damage = false;

    for marker in coefficient_markers {
        let mut start = 0;
        while let Some(relative) = text[start..].find(marker) {
            let position = start + relative;
            let prefix_start = position.saturating_sub(220);
            let prefix = &text[prefix_start..position];
            let last_healing = healing_markers
                .iter()
                .filter_map(|word| prefix.rfind(word))
                .max();
            let last_damage = damage_markers
                .iter()
                .filter_map(|word| prefix.rfind(word))
                .max();
            match (last_healing, last_damage) {
                (Some(heal), Some(dmg)) if heal > dmg => healing = true,
                (Some(_), Some(_)) => damage = true,
                (Some(_), None) => healing = true,
                (None, Some(_)) => damage = true,
                (None, None) => {}
            }
            start = position + marker.len();
        }
    }

    (healing, damage)
}

fn component_summaries(source: Option<&Value>) -> Vec<ComponentSummary> {
    source
        .and_then(|value| value.get("effectComponents"))
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .map(|component| ComponentSummary {
            component_key: string_at(component, "componentKey")
                .unwrap_or("unresolved")
                .to_owned(),
            effect_class: string_at(component, "effectClass").map(str::to_owned),
            direction: string_at(component, "direction").map(str::to_owned),
            has_activation_predicate: component
                .get("activationPredicate")
                .is_some_and(|value| !value.is_null()),
            formula_term_ids: string_array(component.get("formulaTermIds")),
            predicate_tags: string_array(component.get("predicateTags")),
            required_runtime_evidence: string_array(component.get("requiredRuntimeEvidence")),
            value_texts: string_array(component.get("valueTexts")),
            target_damage_ids: integer_array(component.get("targetDamageIds")),
            target_recount_ids: integer_array(component.get("targetRecountIds")),
            target_skill_names: string_array(component.get("targetSkillNames")),
        })
        .collect()
}

fn nested_strings(value: &Value, outer: &str, inner: &str, key: &str) -> Vec<String> {
    value
        .get(outer)
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .flat_map(|proof| {
            proof
                .get(inner)
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
        })
        .filter_map(|entry| string_at(entry, key))
        .map(str::to_owned)
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect()
}

fn numeric_tokens(value: &str) -> BTreeSet<String> {
    let mut values = BTreeSet::new();
    let mut token = String::new();
    for character in value.chars() {
        if character.is_ascii_digit() || (character == '.' && !token.is_empty()) {
            token.push(character);
        } else if !token.is_empty() {
            values.insert(token.trim_end_matches('.').to_owned());
            token.clear();
        }
    }
    if !token.is_empty() {
        values.insert(token.trim_end_matches('.').to_owned());
    }
    values.remove("");
    values
}

fn first_numeric_token(value: &str) -> Option<String> {
    let mut token = String::new();
    for character in value.chars() {
        if character.is_ascii_digit() || (character == '.' && !token.is_empty()) {
            token.push(character);
        } else if !token.is_empty() {
            return Some(token.trim_end_matches('.').to_owned());
        }
    }
    (!token.is_empty()).then(|| token.trim_end_matches('.').to_owned())
}

fn selector_values_after_markers(text: &str, markers: &[&str]) -> BTreeSet<String> {
    let mut values = BTreeSet::new();
    for marker in markers {
        let mut remainder = text;
        while let Some(index) = remainder.find(marker) {
            let after = &remainder[index + marker.len()..];
            if let Some(value) = first_numeric_token(after) {
                values.insert(value);
            }
            remainder = after;
        }
    }
    values
}

fn has_explicit_produced_damage_output(text: &str) -> bool {
    contains_any(
        text,
        &[
            "dmg equal to",
            "damage equal to",
            "loses hp equal to",
            "loss of hp equal to",
            "deals attack dmg",
            "deals magic dmg",
            "dealing attack dmg",
            "dealing magic dmg",
            "inflicts attack dmg",
            "inflicts magic dmg",
        ],
    )
}

fn english_description(source: &Value) -> Option<String> {
    source
        .get("cleanDescriptions")
        .and_then(|descriptions| descriptions.get("en"))
        .and_then(Value::as_str)
        .or_else(|| {
            source
                .get("descriptions")
                .and_then(|descriptions| descriptions.get("en"))
                .and_then(Value::as_str)
        })
        .map(str::to_owned)
}

fn contains_any(text: &str, needles: &[&str]) -> bool {
    needles.iter().any(|needle| text.contains(needle))
}

fn require_generated_by(value: &Value, key: &str, expected: &str) -> Result<(), Box<dyn Error>> {
    if string_at(value, key) != Some(expected) {
        return Err(format!("input was not generated by {expected}").into());
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

fn file_name(path: &Path) -> String {
    path.file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("external-artifact")
        .to_owned()
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

fn validate_build(build: &str) -> Result<(), Box<dyn Error>> {
    if build.is_empty() || !build.bytes().all(|byte| byte.is_ascii_digit()) {
        return Err("build must contain ASCII digits only".into());
    }
    Ok(())
}

fn usage() -> &'static str {
    "usage: rlogs-bpsr-static-rdps-semantic-audit --worklist <static-rdps-worklist.json> --effect-sources <EffectSources.json> --build <client-build> --output <audit.json>"
}

#[cfg(test)]
mod tests {
    use super::*;

    fn strings(values: &[&str]) -> Vec<String> {
        values.iter().map(|value| (*value).to_owned()).collect()
    }

    fn components(values: &[(&str, &str)]) -> Vec<ComponentSummary> {
        values
            .iter()
            .map(|(key, direction)| ComponentSummary {
                component_key: (*key).to_owned(),
                effect_class: None,
                direction: Some((*direction).to_owned()),
                has_activation_predicate: false,
                formula_term_ids: Vec::new(),
                predicate_tags: Vec::new(),
                required_runtime_evidence: Vec::new(),
                value_texts: Vec::new(),
                target_damage_ids: Vec::new(),
                target_recount_ids: Vec::new(),
                target_skill_names: Vec::new(),
            })
            .collect()
    }

    fn categories(issues: &[Issue]) -> BTreeSet<&str> {
        issues.iter().map(|issue| issue.category).collect()
    }

    #[test]
    fn healing_attack_coefficient_is_not_an_attack_modifier_or_damage_proc() {
        let terms = strings(&["primaryAttack"]);
        let components = components(&[
            ("primary-attack", "stat"),
            ("produced-damage", "damage-dealt"),
        ]);
        let issues = audit_candidate(&CandidateContext {
            description: "When Expertise deals damage, additionally heal targets. Restores HP equal to 70% ATK.",
            contribution_mode: "formula-replay-candidate",
            static_value_state: "needs-value-selection",
            formula_term_ids: &terms,
            transfer_eligibilities: &[],
            target_damage_ids: &[],
            selected_value_raw_texts: &[],
            value_selector_kinds: &[],
            components: &components,
        });
        let categories = categories(&issues);
        assert!(categories.contains("healing-coefficient-as-offense-stat"));
        assert!(categories.contains("trigger-text-as-produced-damage"));
    }

    #[test]
    fn gerund_healing_attack_coefficient_is_not_an_attack_modifier() {
        let terms = strings(&["baseAttack"]);
        let components = components(&[("primary-attack", "stat")]);
        let issues = audit_candidate(&CandidateContext {
            description: "Natural Bloom activates, restoring HP equal to 200% ATK to nearby allies.",
            contribution_mode: "formula-replay-candidate",
            static_value_state: "needs-value-selection",
            formula_term_ids: &terms,
            transfer_eligibilities: &[],
            target_damage_ids: &[],
            selected_value_raw_texts: &[],
            value_selector_kinds: &[],
            components: &components,
        });
        assert!(categories(&issues).contains("healing-coefficient-as-offense-stat"));
    }

    #[test]
    fn threshold_replacement_is_a_selector() {
        let terms = strings(&["primaryAttack"]);
        let issues = audit_candidate(&CandidateContext {
            description: "When Strength reaches 500 points, the additional ATK from Sharp increases to 6%.",
            contribution_mode: "formula-replay-candidate",
            static_value_state: "needs-value-selection",
            formula_term_ids: &terms,
            transfer_eligibilities: &[],
            target_damage_ids: &[],
            selected_value_raw_texts: &[],
            value_selector_kinds: &[],
            components: &[],
        });
        assert!(categories(&issues).contains("threshold-selector-as-direct-formula-value"));
    }

    #[test]
    fn named_skill_bonus_requires_a_target_skill_predicate() {
        let terms = strings(&["critMultiplier"]);
        let mut exact_component = components(&[("critical-damage", "stat")]);
        exact_component[0].predicate_tags = strings(&["target.exact-skill-id-required"]);
        let issues = audit_candidate(&CandidateContext {
            description: "Chasing Step: DMG of Instant Edge +30% for 10s.",
            contribution_mode: "formula-replay-candidate",
            static_value_state: "needs-value-selection",
            formula_term_ids: &terms,
            transfer_eligibilities: &[],
            target_damage_ids: &[],
            selected_value_raw_texts: &[],
            value_selector_kinds: &[],
            components: &exact_component,
        });
        assert!(categories(&issues).contains("skill-specific-multiplier-needs-target-skill"));
    }

    #[test]
    fn exact_skill_component_with_current_build_targets_is_proved() {
        let terms = strings(&["critMultiplier"]);
        let mut exact_component = components(&[("critical-damage", "stat")]);
        exact_component[0].predicate_tags = strings(&["target.exact-skill-id-required"]);
        exact_component[0].target_damage_ids = vec![114350101];
        exact_component[0].target_recount_ids = vec![15];
        let issues = audit_candidate(&CandidateContext {
            description: "Crit DMG of Drake Cannon +8%.",
            contribution_mode: "formula-replay-candidate",
            static_value_state: "needs-value-selection",
            formula_term_ids: &terms,
            transfer_eligibilities: &[],
            target_damage_ids: &[],
            selected_value_raw_texts: &[],
            value_selector_kinds: &[],
            components: &exact_component,
        });
        assert!(!categories(&issues).contains("skill-specific-multiplier-needs-target-skill"));
    }

    #[test]
    fn exact_damage_rows_without_recount_parent_are_proved() {
        let terms = strings(&["critMultiplier"]);
        let mut exact_component = components(&[("critical-damage", "stat")]);
        exact_component[0].predicate_tags = strings(&["target.exact-skill-id-required"]);
        exact_component[0].target_damage_ids = vec![299755003];
        let issues = audit_candidate(&CandidateContext {
            description: "Pulse Beam Crit DMG +40%.",
            contribution_mode: "formula-replay-candidate",
            static_value_state: "selected-values-present",
            formula_term_ids: &terms,
            transfer_eligibilities: &[],
            target_damage_ids: &[],
            selected_value_raw_texts: &[],
            value_selector_kinds: &[],
            components: &exact_component,
        });
        assert!(!categories(&issues).contains("skill-specific-multiplier-needs-target-skill"));
    }

    #[test]
    fn acquisition_rules_are_not_runtime_crit_modifiers() {
        let terms = strings(&["critMultiplier"]);
        let issues = audit_candidate(&CandidateContext {
            description: "Increases the chance of obtaining Crit entries",
            contribution_mode: "formula-replay-candidate",
            static_value_state: "needs-value-selection",
            formula_term_ids: &terms,
            transfer_eligibilities: &[],
            target_damage_ids: &[],
            selected_value_raw_texts: &[],
            value_selector_kinds: &[],
            components: &[],
        });
        assert!(categories(&issues).contains("acquisition-rule-as-live-combat-modifier"));
    }

    #[test]
    fn compound_timing_and_damage_remains_blocked_for_component_separation() {
        let terms = strings(&["targetArmorMitigation"]);
        let components = components(&[
            ("cooldown-or-resource", "timing"),
            ("armor-scaling-damage", "damage-dealt"),
        ]);
        let issues = audit_candidate(&CandidateContext {
            description: "Cooldown reduced by 0.5s and deals 330% Armor damage.",
            contribution_mode: "formula-replay-candidate",
            static_value_state: "selected-values-with-blockers",
            formula_term_ids: &terms,
            transfer_eligibilities: &[],
            target_damage_ids: &[42],
            selected_value_raw_texts: &[],
            value_selector_kinds: &[],
            components: &components,
        });
        assert!(categories(&issues).contains("mixed-timing-and-damage-components"));
    }

    #[test]
    fn compound_timing_and_damage_is_not_blocked_when_components_are_separated() {
        let terms = strings(&["targetArmorMitigation"]);
        let mut components = components(&[
            ("cooldown-or-resource", "timing"),
            ("armor-scaling-damage", "damage-dealt"),
        ]);
        for component in &mut components {
            component.predicate_tags = strings(&["runtime-trigger"]);
            component.value_texts = strings(&["exact component value"]);
        }
        let issues = audit_candidate(&CandidateContext {
            description: "Cooldown reduced by 0.5s and deals 330% Armor damage.",
            contribution_mode: "formula-replay-candidate",
            static_value_state: "selected-values-with-blockers",
            formula_term_ids: &terms,
            transfer_eligibilities: &[],
            target_damage_ids: &[42],
            selected_value_raw_texts: &[],
            value_selector_kinds: &[],
            components: &components,
        });
        assert!(!categories(&issues).contains("mixed-timing-and-damage-components"));
    }

    #[test]
    fn categorical_timing_component_does_not_duplicate_formula_magnitude_blocker() {
        let terms = strings(&["primaryAttack"]);
        let mut components = components(&[
            ("cooldown-or-resource", "timing"),
            ("produced-damage", "damage-dealt"),
        ]);
        for component in &mut components {
            component.predicate_tags = strings(&["runtime-trigger"]);
        }
        components[1].value_texts = strings(&["100% ATK"]);
        let issues = audit_candidate(&CandidateContext {
            description: "Deals damage equal to ATK. Dealing damage grants energy.",
            contribution_mode: "formula-replay-candidate",
            static_value_state: "selected-values-with-blockers",
            formula_term_ids: &terms,
            transfer_eligibilities: &[],
            target_damage_ids: &[42],
            selected_value_raw_texts: &[],
            value_selector_kinds: &[],
            components: &components,
        });
        assert!(!categories(&issues).contains("mixed-timing-and-damage-components"));
    }

    #[test]
    fn stack_cap_is_not_a_formula_value_when_runtime_stack_selector_exists() {
        let terms = strings(&["genericDamagePct"]);
        let values = strings(&["20%"]);
        let selectors = strings(&["runtime-stack"]);
        let issues = audit_candidate(&CandidateContext {
            description: "Damage dealt +20% for 12s and stacks up to 5 times.",
            contribution_mode: "formula-replay-candidate",
            static_value_state: "selected-values-present",
            formula_term_ids: &terms,
            transfer_eligibilities: &[],
            target_damage_ids: &[],
            selected_value_raw_texts: &values,
            value_selector_kinds: &selectors,
            components: &[],
        });
        assert!(!categories(&issues).contains("stack-cap-as-direct-formula-value"));
    }

    #[test]
    fn selected_stack_cap_without_selector_is_rejected() {
        let terms = strings(&["genericDamagePct"]);
        let values = strings(&["+5"]);
        let issues = audit_candidate(&CandidateContext {
            description: "Maximum stacks +5.",
            contribution_mode: "formula-replay-candidate",
            static_value_state: "selected-values-present",
            formula_term_ids: &terms,
            transfer_eligibilities: &[],
            target_damage_ids: &[],
            selected_value_raw_texts: &values,
            value_selector_kinds: &[],
            components: &[],
        });
        assert!(categories(&issues).contains("stack-cap-as-direct-formula-value"));
    }

    #[test]
    fn direct_proc_sentence_is_not_confused_with_its_trigger_clause() {
        let components = components(&[("produced-damage", "damage-dealt")]);
        let issues = audit_candidate(&CandidateContext {
            description: "When Frost Lance deals DMG, summon Frost Comet. Frost Comet deals Magic DMG equal to 200% ATK.",
            contribution_mode: "exact-produced-damage",
            static_value_state: "not-required-for-packet-exact-produced-damage",
            formula_term_ids: &[],
            transfer_eligibilities: &["direct-output-owned-by-source".to_owned()],
            target_damage_ids: &[112590101],
            selected_value_raw_texts: &[],
            value_selector_kinds: &[],
            components: &components,
        });
        assert!(!categories(&issues).contains("trigger-text-as-produced-damage"));
    }

    #[test]
    fn self_only_missing_magnitude_is_formula_context_not_transferable_rdps() {
        let terms = strings(&["luckyChancePct"]);
        let transfer = strings(&["self-only-formula-context"]);
        let issues = audit_candidate(&CandidateContext {
            description: "Greatly increases own Luck and Lucky Strike chance.",
            contribution_mode: "formula-replay-candidate",
            static_value_state: "missing-value-proof",
            formula_term_ids: &terms,
            transfer_eligibilities: &transfer,
            target_damage_ids: &[],
            selected_value_raw_texts: &[],
            value_selector_kinds: &[],
            components: &[],
        });
        let categories = categories(&issues);
        assert!(categories.contains("formula-context-magnitude-unresolved"));
        assert!(!categories.contains("formula-magnitude-unresolved"));
        assert!(!categories.contains("formula-recipient-scope-unresolved"));
    }
}
