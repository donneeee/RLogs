use std::cmp::{Ordering, Reverse};
use std::collections::{BTreeMap, BTreeSet, BinaryHeap};

use crate::scoring::{ScoringRules, attribute_multiplier};
use crate::types::{
    ModuleCandidate, ModuleSolution, OptimizeRequest, OptimizeResponse, OptimizerError,
    ScoreBreakdown, SearchMode, SearchSummary,
};

const MAX_EXACT_COMBINATIONS: u64 = 10_000_000;
const MAX_BEAM_WIDTH: usize = 32_768;
const MAX_INPUT_MODULES: usize = 4_096;
const MAX_ATTRIBUTE_SLOTS: usize = 32;
const MAX_COMBINATION_SIZE: usize = 5;

#[derive(Debug, Clone)]
struct DenseCandidate {
    module: ModuleCandidate,
    values: Vec<i32>,
    total_link_points: i32,
}

#[derive(Debug, Clone, Copy)]
struct CandidateSortMetrics {
    original_index: usize,
    primary_slot: usize,
    primary_power: i32,
    maximum_special_power: i32,
    constraint_contribution: i32,
    total_link_points: i32,
}

struct DenseScorer<'a> {
    rules: &'a ScoringRules,
    attribute_power: Vec<Vec<i32>>,
}

impl<'a> DenseScorer<'a> {
    fn new(
        rules: &'a ScoringRules,
        target_attributes: &BTreeSet<i32>,
        exclude_attributes: &BTreeSet<i32>,
    ) -> Self {
        let attribute_power = rules
            .attribute_ids()
            .map(|attribute_id| {
                let multiplier =
                    attribute_multiplier(attribute_id, target_attributes, exclude_attributes);
                let maximum_value = rules
                    .attributes
                    .get(&attribute_id)
                    .and_then(|levels| levels.last())
                    .map_or(0, |(threshold, _)| *threshold)
                    .max(0);
                (0..=maximum_value)
                    .map(|value| rules.attribute_power(attribute_id, value).1 * multiplier)
                    .collect()
            })
            .collect();
        Self {
            rules,
            attribute_power,
        }
    }

    fn score(&self, values: &[i32], total_link_points: i32) -> i32 {
        self.attribute_power
            .iter()
            .zip(values)
            .map(|(power, value)| power[score_table_index(power, *value)])
            .sum::<i32>()
            + self.rules.total_link_power(total_link_points)
    }

    fn score_with_addition(&self, values: &[i32], addition: &[i32], total_link_points: i32) -> i32 {
        self.attribute_power
            .iter()
            .zip(values)
            .zip(addition)
            .map(|((power, value), addition)| power[score_table_index(power, value + addition)])
            .sum::<i32>()
            + self.rules.total_link_power(total_link_points)
    }

    fn attribute_score(&self, slot: usize, value: i32) -> i32 {
        let power = &self.attribute_power[slot];
        power[score_table_index(power, value)]
    }
}

fn score_table_index(power: &[i32], value: i32) -> usize {
    usize::try_from(value.max(0))
        .unwrap_or(usize::MAX)
        .min(power.len().saturating_sub(1))
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct RankedCombination {
    indices: Vec<usize>,
    ranking_score: i32,
}

impl Ord for RankedCombination {
    fn cmp(&self, other: &Self) -> Ordering {
        self.ranking_score
            .cmp(&other.ranking_score)
            .then_with(|| other.indices.cmp(&self.indices))
    }
}

impl PartialOrd for RankedCombination {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct BeamState {
    indices: [u16; MAX_COMBINATION_SIZE],
    values: [i32; MAX_ATTRIBUTE_SLOTS],
    depth: u8,
    total_link_points: i32,
    ranking_score: i32,
    upper_bound: i32,
}

impl Ord for BeamState {
    fn cmp(&self, other: &Self) -> Ordering {
        self.upper_bound
            .cmp(&other.upper_bound)
            .then_with(|| self.ranking_score.cmp(&other.ranking_score))
            .then_with(|| {
                other.indices[..usize::from(other.depth)]
                    .cmp(&self.indices[..usize::from(self.depth)])
            })
    }
}

impl PartialOrd for BeamState {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

pub fn optimize(
    rules: &ScoringRules,
    request: &OptimizeRequest,
) -> Result<OptimizeResponse, OptimizerError> {
    validate_request(rules, request)?;
    let target_attributes = request.target_attributes.iter().copied().collect();
    let exclude_attributes = request.exclude_attributes.iter().copied().collect();
    let current_setup =
        build_current_setup(rules, request, &target_attributes, &exclude_attributes)?;
    let (candidates, excluded_module_count) =
        prepare_candidates(rules, request, &target_attributes)?;
    if candidates.len() < request.combination_size {
        return Err(OptimizerError::InvalidRequest(format!(
            "{} candidates remain after filters; {} are required",
            candidates.len(),
            request.combination_size
        )));
    }
    let current_combination = current_combination_indices(rules, &candidates, request);
    let solution_limit = request.max_solutions + usize::from(current_combination.is_some());

    let total_combinations = combination_count(candidates.len(), request.combination_size);
    let used_mode = match request.search_mode {
        SearchMode::Auto if total_combinations <= request.exact_combination_limit => {
            SearchMode::Exact
        }
        SearchMode::Auto => SearchMode::Beam,
        SearchMode::Exact => {
            if total_combinations > request.exact_combination_limit {
                return Err(OptimizerError::InvalidRequest(format!(
                    "exact search needs {total_combinations} combinations, above the configured limit {}; use auto/beam or raise exact_combination_limit up to {MAX_EXACT_COMBINATIONS}",
                    request.exact_combination_limit
                )));
            }
            SearchMode::Exact
        }
        SearchMode::Beam => SearchMode::Beam,
    };

    let (mut ranked, evaluated_states) = match used_mode {
        SearchMode::Exact => exact_search(
            rules,
            &candidates,
            request,
            &target_attributes,
            &exclude_attributes,
            solution_limit,
        ),
        SearchMode::Beam => beam_search(
            rules,
            &candidates,
            request,
            &target_attributes,
            &exclude_attributes,
            solution_limit,
        ),
        SearchMode::Auto => unreachable!("auto mode is resolved before search"),
    };
    if let Some(current_indices) = current_combination {
        ranked.retain(|combination| combination.indices != current_indices);
    }
    ranked.truncate(request.max_solutions);
    let solutions = ranked
        .into_iter()
        .map(|ranked| {
            build_solution(
                rules,
                &candidates,
                ranked,
                &target_attributes,
                &exclude_attributes,
            )
        })
        .collect();

    Ok(OptimizeResponse {
        scoring_revision: rules.scoring_revision().into(),
        catalog_revision: rules.catalog_revision().into(),
        current_setup,
        solutions,
        search: SearchSummary {
            requested_mode: request.search_mode,
            used_mode,
            exact: used_mode == SearchMode::Exact,
            input_module_count: request.modules.len(),
            candidate_module_count: candidates.len(),
            excluded_module_count,
            total_combinations,
            evaluated_states,
            combination_size: request.combination_size,
            beam_width: (used_mode == SearchMode::Beam).then_some(request.beam_width),
        },
    })
}

fn current_combination_indices(
    rules: &ScoringRules,
    candidates: &[DenseCandidate],
    request: &OptimizeRequest,
) -> Option<Vec<usize>> {
    if request.current_instance_ids.len() != request.combination_size {
        return None;
    }
    let indices_by_id = candidates
        .iter()
        .enumerate()
        .map(|(index, candidate)| (candidate.module.instance_id.as_str(), index))
        .collect::<BTreeMap<_, _>>();
    let mut indices = request
        .current_instance_ids
        .iter()
        .map(|instance_id| indices_by_id.get(instance_id.as_str()).copied())
        .collect::<Option<Vec<_>>>()?;
    indices.sort_unstable();
    let (values, _) = sum_combination(candidates, &indices);
    if !meets_requirements(rules, &values, &request.min_attr_requirements) {
        return None;
    }
    Some(indices)
}

pub fn score_modules(
    rules: &ScoringRules,
    modules: &[ModuleCandidate],
    target_attributes: &[i32],
    exclude_attributes: &[i32],
) -> Result<(i32, ScoreBreakdown), OptimizerError> {
    for module in modules {
        validate_module(module)?;
    }
    let targets = target_attributes.iter().copied().collect();
    let exclusions = exclude_attributes.iter().copied().collect();
    let breakdown = rules.score_breakdown(modules, &targets, &exclusions);
    let score = breakdown.threshold_power + breakdown.total_link_power;
    Ok((score, breakdown))
}

fn validate_request(rules: &ScoringRules, request: &OptimizeRequest) -> Result<(), OptimizerError> {
    if request.modules.len() > MAX_INPUT_MODULES {
        return Err(OptimizerError::InvalidRequest(format!(
            "module input exceeds the {MAX_INPUT_MODULES} item limit"
        )));
    }
    if rules.attributes.len() > MAX_ATTRIBUTE_SLOTS {
        return Err(OptimizerError::InvalidCatalog(format!(
            "catalog has {} module attributes; optimizer capacity is {MAX_ATTRIBUTE_SLOTS}",
            rules.attributes.len()
        )));
    }
    if !matches!(request.combination_size, 4 | 5) {
        return Err(OptimizerError::InvalidRequest(
            "combination_size must be 4 or 5".into(),
        ));
    }
    if !(1..=60).contains(&request.max_solutions) {
        return Err(OptimizerError::InvalidRequest(
            "max_solutions must be between 1 and 60".into(),
        ));
    }
    if !(1..=MAX_EXACT_COMBINATIONS).contains(&request.exact_combination_limit) {
        return Err(OptimizerError::InvalidRequest(format!(
            "exact_combination_limit must be between 1 and {MAX_EXACT_COMBINATIONS}"
        )));
    }
    if !(64..=MAX_BEAM_WIDTH).contains(&request.beam_width) {
        return Err(OptimizerError::InvalidRequest(format!(
            "beam_width must be between 64 and {MAX_BEAM_WIDTH}"
        )));
    }
    if !(1..=8).contains(&request.minimum_parts) {
        return Err(OptimizerError::InvalidRequest(
            "minimum_parts must be between 1 and 8".into(),
        ));
    }
    let mut seen = BTreeSet::new();
    for module in &request.modules {
        validate_module(module)?;
        if !seen.insert(&module.instance_id) {
            return Err(OptimizerError::InvalidRequest(format!(
                "duplicate module instance_id {}",
                module.instance_id
            )));
        }
    }
    for attribute_id in request
        .target_attributes
        .iter()
        .chain(&request.exclude_attributes)
        .chain(request.min_attr_requirements.keys())
    {
        if !rules.contains_attribute(*attribute_id) {
            return Err(OptimizerError::InvalidRequest(format!(
                "attribute {attribute_id} is not in catalog {}",
                rules.catalog_revision()
            )));
        }
    }
    if request
        .min_attr_requirements
        .values()
        .any(|value| *value < 0)
    {
        return Err(OptimizerError::InvalidRequest(
            "minimum attribute requirements cannot be negative".into(),
        ));
    }
    Ok(())
}

fn validate_module(module: &ModuleCandidate) -> Result<(), OptimizerError> {
    if module.instance_id.trim().is_empty() {
        return Err(OptimizerError::InvalidRequest(
            "module instance_id cannot be empty".into(),
        ));
    }
    for part in &module.parts {
        let Some(value) = part.initial_link_points else {
            return Err(OptimizerError::InvalidRequest(format!(
                "module {} part {} has no initial_link_points value",
                module.instance_id, part.part_id
            )));
        };
        if value < 0 {
            return Err(OptimizerError::InvalidRequest(format!(
                "module {} part {} has a negative link value",
                module.instance_id, part.part_id
            )));
        }
    }
    Ok(())
}

fn prepare_candidates(
    rules: &ScoringRules,
    request: &OptimizeRequest,
    target_attributes: &BTreeSet<i32>,
) -> Result<(Vec<DenseCandidate>, usize), OptimizerError> {
    let attribute_ids = rules.attribute_ids().collect::<Vec<_>>();
    let attribute_slots = attribute_ids
        .iter()
        .enumerate()
        .map(|(index, attribute_id)| (*attribute_id, index))
        .collect::<BTreeMap<_, _>>();
    let mut candidates = Vec::new();
    for module in &request.modules {
        if module.parts.len() < request.minimum_parts {
            continue;
        }
        let mut values = vec![0_i32; attribute_ids.len()];
        let mut total_link_points = 0_i32;
        let mut has_target = target_attributes.is_empty();
        for part in &module.parts {
            let value = part.initial_link_points.unwrap_or_default();
            total_link_points = total_link_points.checked_add(value).ok_or_else(|| {
                OptimizerError::InvalidRequest(format!(
                    "module {} link total overflows i32",
                    module.instance_id
                ))
            })?;
            if let Some(slot) = attribute_slots.get(&part.part_id) {
                values[*slot] = values[*slot].checked_add(value).ok_or_else(|| {
                    OptimizerError::InvalidRequest(format!(
                        "module {} attribute {} overflows i32",
                        module.instance_id, part.part_id
                    ))
                })?;
            }
            has_target |= target_attributes.contains(&part.part_id);
        }
        if request.require_target_match && !has_target {
            continue;
        }
        if request
            .minimum_module_total
            .is_some_and(|minimum| total_link_points < minimum)
        {
            continue;
        }
        candidates.push(DenseCandidate {
            module: module.clone(),
            values,
            total_link_points,
        });
    }
    let excluded = request.modules.len().saturating_sub(candidates.len());
    Ok((candidates, excluded))
}

fn exact_search(
    rules: &ScoringRules,
    candidates: &[DenseCandidate],
    request: &OptimizeRequest,
    target_attributes: &BTreeSet<i32>,
    exclude_attributes: &BTreeSet<i32>,
    solution_limit: usize,
) -> (Vec<RankedCombination>, u64) {
    let mut top = BinaryHeap::<Reverse<RankedCombination>>::new();
    let mut combination = (0..request.combination_size).collect::<Vec<_>>();
    let mut evaluated = 0_u64;
    loop {
        evaluated += 1;
        let (values, total_link_points) = sum_combination(candidates, &combination);
        if meets_requirements(rules, &values, &request.min_attr_requirements) {
            let ranking_score = rules.ranking_score_dense(
                &values,
                total_link_points,
                target_attributes,
                exclude_attributes,
            );
            retain_ranked(
                &mut top,
                RankedCombination {
                    indices: combination.clone(),
                    ranking_score,
                },
                solution_limit,
            );
        }
        if !next_combination(&mut combination, candidates.len()) {
            break;
        }
    }
    (finish_ranked(top), evaluated)
}

fn beam_search(
    rules: &ScoringRules,
    candidates: &[DenseCandidate],
    request: &OptimizeRequest,
    target_attributes: &BTreeSet<i32>,
    exclude_attributes: &BTreeSet<i32>,
    solution_limit: usize,
) -> (Vec<RankedCombination>, u64) {
    let scorer = DenseScorer::new(rules, target_attributes, exclude_attributes);
    let mut top = BinaryHeap::<Reverse<RankedCombination>>::new();
    let mut seen = BTreeSet::new();
    let mut evaluated = 0_u64;
    for ordering in candidate_orderings(rules, candidates, request, &scorer) {
        let ordered = ordering
            .iter()
            .map(|index| candidates[*index].clone())
            .collect::<Vec<_>>();
        let (ranked, strategy_evaluated) =
            beam_search_ordered(rules, &ordered, request, &scorer, solution_limit);
        evaluated = evaluated.saturating_add(strategy_evaluated);
        for mut combination in ranked {
            for index in &mut combination.indices {
                *index = ordering[*index];
            }
            combination.indices.sort_unstable();
            if seen.insert(combination.indices.clone()) {
                retain_ranked(&mut top, combination, solution_limit);
            }
        }
    }
    (finish_ranked(top), evaluated)
}

fn beam_search_ordered(
    rules: &ScoringRules,
    candidates: &[DenseCandidate],
    request: &OptimizeRequest,
    scorer: &DenseScorer<'_>,
    solution_limit: usize,
) -> (Vec<RankedCombination>, u64) {
    let suffix_values = suffix_value_upper_bounds(candidates, request.combination_size);
    let mut frontier = vec![BeamState {
        indices: [0; MAX_COMBINATION_SIZE],
        values: [0; MAX_ATTRIBUTE_SLOTS],
        depth: 0,
        total_link_points: 0,
        ranking_score: 0,
        upper_bound: i32::MAX,
    }];
    let mut evaluated = 0_u64;

    for depth in 0..request.combination_size {
        let remaining_after_pick = request.combination_size - depth - 1;
        let mut next = BinaryHeap::<Reverse<BeamState>>::new();
        for state in frontier {
            let start = if state.depth == 0 {
                0
            } else {
                usize::from(state.indices[usize::from(state.depth) - 1]) + 1
            };
            let Some(maximum_index) = candidates.len().checked_sub(remaining_after_pick + 1) else {
                continue;
            };
            if start > maximum_index {
                continue;
            }
            for candidate_index in start..=maximum_index {
                evaluated += 1;
                let candidate = &candidates[candidate_index];
                let mut values = state.values;
                for (value, addition) in values.iter_mut().zip(&candidate.values) {
                    *value += *addition;
                }
                let total_link_points = state.total_link_points + candidate.total_link_points;
                let next_start = candidate_index + 1;
                if !can_meet_requirements(
                    rules,
                    &values[..rules.attributes.len()],
                    next_start,
                    remaining_after_pick,
                    &suffix_values,
                    &request.min_attr_requirements,
                ) {
                    continue;
                }
                let ranking_score =
                    scorer.score(&values[..rules.attributes.len()], total_link_points);
                let upper_bound = if remaining_after_pick == 0 {
                    ranking_score
                } else {
                    greedy_completion_score(
                        candidates,
                        scorer,
                        &values[..rules.attributes.len()],
                        total_link_points,
                        next_start,
                        remaining_after_pick,
                    )
                };
                let mut indices = state.indices;
                indices[usize::from(state.depth)] =
                    u16::try_from(candidate_index).expect("module input is capped below u16");
                retain_beam(
                    &mut next,
                    BeamState {
                        indices,
                        values,
                        depth: state.depth + 1,
                        total_link_points,
                        ranking_score,
                        upper_bound,
                    },
                    request.beam_width,
                );
            }
        }
        frontier = next.into_iter().map(|entry| entry.0).collect();
        frontier.sort_by(|left, right| right.cmp(left));
    }

    let mut top = BinaryHeap::<Reverse<RankedCombination>>::new();
    for state in frontier {
        if meets_requirements(
            rules,
            &state.values[..rules.attributes.len()],
            &request.min_attr_requirements,
        ) {
            retain_ranked(
                &mut top,
                RankedCombination {
                    indices: state.indices[..usize::from(state.depth)]
                        .iter()
                        .map(|index| usize::from(*index))
                        .collect(),
                    ranking_score: state.ranking_score,
                },
                solution_limit,
            );
        }
    }
    (finish_ranked(top), evaluated)
}

fn candidate_orderings(
    rules: &ScoringRules,
    candidates: &[DenseCandidate],
    request: &OptimizeRequest,
    scorer: &DenseScorer<'_>,
) -> Vec<Vec<usize>> {
    let attribute_ids = rules.attribute_ids().collect::<Vec<_>>();
    let highest_attribute_power = attribute_ids
        .iter()
        .map(|attribute_id| rules.attribute_power(*attribute_id, i32::MAX).1)
        .max()
        .unwrap_or_default();
    let metrics = candidates
        .iter()
        .enumerate()
        .map(|(original_index, candidate)| {
            let mut primary_slot = attribute_ids.len();
            let mut primary_power = 0;
            let mut primary_value = 0;
            let mut maximum_special_power = 0;
            let mut constraint_contribution = 0;
            for (slot, (attribute_id, value)) in
                attribute_ids.iter().zip(&candidate.values).enumerate()
            {
                if request.min_attr_requirements.contains_key(attribute_id) {
                    constraint_contribution += *value;
                }
                let power = scorer.attribute_score(slot, *value);
                if rules.attribute_power(*attribute_id, i32::MAX).1 == highest_attribute_power {
                    maximum_special_power = maximum_special_power.max(power);
                }
                if power > primary_power
                    || (power == primary_power && *value > primary_value)
                    || (power == primary_power && *value == primary_value && slot < primary_slot)
                {
                    primary_slot = slot;
                    primary_power = power;
                    primary_value = *value;
                }
            }
            CandidateSortMetrics {
                original_index,
                primary_slot,
                primary_power,
                maximum_special_power,
                constraint_contribution,
                total_link_points: candidate.total_link_points,
            }
        })
        .collect::<Vec<_>>();
    let strategy_count = if request.min_attr_requirements.is_empty() {
        3
    } else {
        4
    };
    (0..strategy_count)
        .map(|strategy| {
            let mut ordered = metrics.clone();
            ordered.sort_by(|left, right| {
                let ordering = match strategy {
                    1 => right
                        .maximum_special_power
                        .cmp(&left.maximum_special_power)
                        .then_with(|| left.primary_slot.cmp(&right.primary_slot))
                        .then_with(|| right.total_link_points.cmp(&left.total_link_points)),
                    2 => right
                        .total_link_points
                        .cmp(&left.total_link_points)
                        .then_with(|| right.primary_power.cmp(&left.primary_power)),
                    3 => right
                        .constraint_contribution
                        .cmp(&left.constraint_contribution)
                        .then_with(|| right.primary_power.cmp(&left.primary_power))
                        .then_with(|| right.total_link_points.cmp(&left.total_link_points)),
                    _ => left
                        .primary_slot
                        .cmp(&right.primary_slot)
                        .then_with(|| right.primary_power.cmp(&left.primary_power))
                        .then_with(|| right.total_link_points.cmp(&left.total_link_points)),
                };
                ordering.then_with(|| left.original_index.cmp(&right.original_index))
            });
            ordered
                .into_iter()
                .map(|entry| entry.original_index)
                .collect()
        })
        .collect()
}

fn greedy_completion_score(
    candidates: &[DenseCandidate],
    scorer: &DenseScorer<'_>,
    values: &[i32],
    total_link_points: i32,
    next_start: usize,
    remaining: usize,
) -> i32 {
    let mut values = values.to_vec();
    let mut total_link_points = total_link_points;
    let mut score = scorer.score(&values, total_link_points);
    let mut search_start = next_start;
    for pick in 0..remaining {
        let scan_window = 64_usize.max((remaining - pick) * 32);
        let scan_end = candidates
            .len()
            .min(search_start.saturating_add(scan_window));
        let mut best = None;
        for (candidate_index, candidate) in candidates
            .iter()
            .enumerate()
            .take(scan_end)
            .skip(search_start)
        {
            let trial_total = total_link_points + candidate.total_link_points;
            let trial_score = scorer.score_with_addition(&values, &candidate.values, trial_total);
            if best
                .as_ref()
                .is_none_or(|(_, best_score)| trial_score > *best_score)
            {
                best = Some((candidate_index, trial_score));
            }
        }
        let Some((candidate_index, next_score)) = best else {
            break;
        };
        let candidate = &candidates[candidate_index];
        for (value, addition) in values.iter_mut().zip(&candidate.values) {
            *value += *addition;
        }
        total_link_points += candidate.total_link_points;
        score = next_score;
        search_start = candidate_index + 1;
    }
    score
}

fn build_solution(
    rules: &ScoringRules,
    candidates: &[DenseCandidate],
    ranked: RankedCombination,
    target_attributes: &BTreeSet<i32>,
    exclude_attributes: &BTreeSet<i32>,
) -> ModuleSolution {
    let modules = ranked
        .indices
        .iter()
        .map(|index| candidates[*index].module.clone())
        .collect::<Vec<_>>();
    let solution = module_solution(rules, modules, target_attributes, exclude_attributes);
    debug_assert_eq!(ranked.ranking_score, solution.ranking_score);
    solution
}

fn build_current_setup(
    rules: &ScoringRules,
    request: &OptimizeRequest,
    target_attributes: &BTreeSet<i32>,
    exclude_attributes: &BTreeSet<i32>,
) -> Result<Option<ModuleSolution>, OptimizerError> {
    if request.current_instance_ids.is_empty() {
        return Ok(None);
    }
    if request.current_instance_ids.len() > MAX_COMBINATION_SIZE {
        return Err(OptimizerError::InvalidRequest(format!(
            "current setup contains {} modules; at most {MAX_COMBINATION_SIZE} are supported",
            request.current_instance_ids.len()
        )));
    }
    let modules_by_id = request
        .modules
        .iter()
        .map(|module| (module.instance_id.as_str(), module))
        .collect::<BTreeMap<_, _>>();
    let mut seen = BTreeSet::new();
    let mut modules = Vec::with_capacity(request.current_instance_ids.len());
    for instance_id in &request.current_instance_ids {
        if !seen.insert(instance_id.as_str()) {
            return Err(OptimizerError::InvalidRequest(format!(
                "current setup contains duplicate module instance_id {instance_id}"
            )));
        }
        let module = modules_by_id.get(instance_id.as_str()).ok_or_else(|| {
            OptimizerError::InvalidRequest(format!(
                "current module instance_id {instance_id} is not in the inventory"
            ))
        })?;
        modules.push((*module).clone());
    }
    Ok(Some(module_solution(
        rules,
        modules,
        target_attributes,
        exclude_attributes,
    )))
}

fn module_solution(
    rules: &ScoringRules,
    modules: Vec<ModuleCandidate>,
    target_attributes: &BTreeSet<i32>,
    exclude_attributes: &BTreeSet<i32>,
) -> ModuleSolution {
    let instance_ids = modules
        .iter()
        .map(|module| module.instance_id.clone())
        .collect();
    let breakdown = rules.score_breakdown(&modules, target_attributes, exclude_attributes);
    ModuleSolution {
        instance_ids,
        modules,
        score: breakdown.threshold_power + breakdown.total_link_power,
        ranking_score: breakdown.ranking_threshold_power + breakdown.total_link_power,
        breakdown,
    }
}

fn retain_ranked(
    heap: &mut BinaryHeap<Reverse<RankedCombination>>,
    candidate: RankedCombination,
    limit: usize,
) {
    if heap.len() < limit {
        heap.push(Reverse(candidate));
    } else if heap.peek().is_some_and(|worst| candidate > worst.0) {
        heap.pop();
        heap.push(Reverse(candidate));
    }
}

fn finish_ranked(heap: BinaryHeap<Reverse<RankedCombination>>) -> Vec<RankedCombination> {
    let mut ranked = heap.into_iter().map(|entry| entry.0).collect::<Vec<_>>();
    ranked.sort_by(|left, right| right.cmp(left));
    ranked
}

fn retain_beam(heap: &mut BinaryHeap<Reverse<BeamState>>, candidate: BeamState, limit: usize) {
    if heap.len() < limit {
        heap.push(Reverse(candidate));
    } else if heap.peek().is_some_and(|worst| candidate > worst.0) {
        heap.pop();
        heap.push(Reverse(candidate));
    }
}

fn sum_combination(candidates: &[DenseCandidate], indices: &[usize]) -> (Vec<i32>, i32) {
    let mut values = vec![0; candidates[0].values.len()];
    let mut total_link_points = 0;
    for index in indices {
        let candidate = &candidates[*index];
        for (value, addition) in values.iter_mut().zip(&candidate.values) {
            *value += *addition;
        }
        total_link_points += candidate.total_link_points;
    }
    (values, total_link_points)
}

fn meets_requirements(
    rules: &ScoringRules,
    values: &[i32],
    requirements: &BTreeMap<i32, i32>,
) -> bool {
    rules
        .attribute_ids()
        .zip(values)
        .all(|(attribute_id, actual)| {
            requirements
                .get(&attribute_id)
                .is_none_or(|required| actual >= required)
        })
}

fn can_meet_requirements(
    rules: &ScoringRules,
    values: &[i32],
    next_start: usize,
    remaining: usize,
    suffix_values: &[Vec<[i32; 6]>],
    requirements: &BTreeMap<i32, i32>,
) -> bool {
    rules
        .attribute_ids()
        .zip(values)
        .enumerate()
        .all(|(slot, (attribute_id, actual))| {
            requirements.get(&attribute_id).is_none_or(|required| {
                actual + suffix_values[slot][next_start][remaining] >= *required
            })
        })
}

fn suffix_value_upper_bounds(
    candidates: &[DenseCandidate],
    maximum_picks: usize,
) -> Vec<Vec<[i32; 6]>> {
    let count = candidates.len();
    let slots = candidates[0].values.len();
    let mut values = vec![vec![[0; 6]; count + 1]; slots];
    for index in (0..count).rev() {
        for slot_values in &mut values {
            slot_values[index] = slot_values[index + 1];
        }
        for picks in 1..=maximum_picks {
            for (slot, slot_values) in values.iter_mut().enumerate() {
                slot_values[index][picks] = slot_values[index][picks]
                    .max(candidates[index].values[slot] + slot_values[index + 1][picks - 1]);
            }
        }
    }
    values
}

fn next_combination(combination: &mut [usize], item_count: usize) -> bool {
    for position in (0..combination.len()).rev() {
        let limit = item_count - combination.len() + position;
        if combination[position] < limit {
            combination[position] += 1;
            for next in position + 1..combination.len() {
                combination[next] = combination[next - 1] + 1;
            }
            return true;
        }
    }
    false
}

fn combination_count(item_count: usize, pick_count: usize) -> u64 {
    if pick_count > item_count {
        return 0;
    }
    let pick_count = pick_count.min(item_count - pick_count);
    let mut result = 1_u128;
    for index in 0..pick_count {
        result = result * (item_count - index) as u128 / (index + 1) as u128;
    }
    result.min(u64::MAX as u128) as u64
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::ModulePartInput;

    fn module(instance_id: &str, parts: &[(i32, i32)]) -> ModuleCandidate {
        ModuleCandidate {
            instance_id: instance_id.into(),
            config_id: 5_500_101,
            quality: Some(5),
            parts: parts
                .iter()
                .map(|(part_id, value)| ModulePartInput {
                    part_id: *part_id,
                    initial_link_points: Some(*value),
                })
                .collect(),
        }
    }

    #[test]
    fn cn_threshold_scoring_is_preserved() {
        let rules = ScoringRules::cn_0_2_0_fixture();
        let modules = vec![
            module("9007199254740993", &[(1110, 4), (2104, 1)]),
            module("9007199254740994", &[(1110, 4), (2104, 3)]),
            module("9007199254740995", &[(1111, 8), (2104, 4)]),
            module("9007199254740996", &[(1112, 4), (2105, 4)]),
        ];
        let (score, breakdown) = score_modules(&rules, &modules, &[1110], &[]).unwrap();

        // Actual power remains unweighted: 1110 reaches 8 (29), 1111 reaches
        // 8 (29), 1112 reaches 4 (14), special 2104 reaches 8 (59), and
        // special 2105 reaches 4 (29). The separate ranking threshold doubles
        // only the preferred 1110 contribution. Total link points are 32,
        // which contributes 186.
        assert_eq!(breakdown.threshold_power, 29 + 29 + 14 + 59 + 29);
        assert_eq!(breakdown.ranking_threshold_power, 58 + 29 + 14 + 59 + 29);
        assert_eq!(breakdown.total_link_power, 186);
        assert_eq!(score, 346);
    }

    #[test]
    fn exact_search_finds_a_stable_top_combination() {
        let rules = ScoringRules::cn_0_2_0_fixture();
        let mut request = OptimizeRequest {
            modules: vec![
                module("a", &[(1110, 4), (1111, 1)]),
                module("b", &[(1110, 4), (1112, 1)]),
                module("c", &[(1110, 4), (1113, 1)]),
                module("d", &[(1110, 4), (1114, 1)]),
                module("e", &[(1111, 4), (1112, 4)]),
                module("f", &[(1113, 4), (1114, 4)]),
            ],
            target_attributes: vec![1110],
            search_mode: SearchMode::Exact,
            max_solutions: 3,
            require_target_match: false,
            ..OptimizeRequest::default()
        };
        request.min_attr_requirements.insert(1110, 16);
        let response = optimize(&rules, &request).unwrap();
        assert!(response.search.exact);
        assert_eq!(response.search.total_combinations, 15);
        assert_eq!(response.solutions[0].instance_ids, ["a", "b", "c", "d"]);
        assert!(response.solutions.iter().all(|solution| {
            solution
                .breakdown
                .attributes
                .iter()
                .find(|attribute| attribute.attribute_id == 1110)
                .is_some_and(|attribute| attribute.total >= 16)
        }));
    }

    #[test]
    fn auto_uses_bounded_beam_search_for_large_inputs() {
        let rules = ScoringRules::cn_0_2_0_fixture();
        let request = OptimizeRequest {
            modules: (0..80)
                .map(|index| {
                    module(
                        &format!("{index:03}"),
                        &[(1110 + index % 5, 1 + index % 10), (2104, 1)],
                    )
                })
                .collect(),
            exact_combination_limit: 1_000,
            beam_width: 256,
            max_solutions: 5,
            ..OptimizeRequest::default()
        };
        let response = optimize(&rules, &request).unwrap();
        assert_eq!(response.search.used_mode, SearchMode::Beam);
        assert!(!response.search.exact);
        assert_eq!(response.solutions.len(), 5);
        assert!(response.search.evaluated_states < response.search.total_combinations);
    }

    #[test]
    fn default_beam_matches_exact_best_score_on_a_medium_fixture() {
        let rules = ScoringRules::cn_0_2_0_fixture();
        let modules = (0..36)
            .map(|index| {
                module(
                    &format!("{index:03}"),
                    &[
                        (1110 + index % 5, 1 + index % 10),
                        (1407 + index % 4, 1 + (index * 3) % 10),
                        (2104 + index % 2, 1 + (index * 7) % 10),
                    ],
                )
            })
            .collect::<Vec<_>>();
        let exact = optimize(
            &rules,
            &OptimizeRequest {
                modules: modules.clone(),
                target_attributes: vec![1110, 1111],
                search_mode: SearchMode::Exact,
                exact_combination_limit: 100_000,
                require_target_match: false,
                ..OptimizeRequest::default()
            },
        )
        .unwrap();
        let beam = optimize(
            &rules,
            &OptimizeRequest {
                modules,
                target_attributes: vec![1110, 1111],
                search_mode: SearchMode::Beam,
                require_target_match: false,
                ..OptimizeRequest::default()
            },
        )
        .unwrap();
        assert_eq!(
            beam.solutions[0].ranking_score,
            exact.solutions[0].ranking_score
        );
        assert_eq!(
            beam.solutions[0].instance_ids,
            exact.solutions[0].instance_ids
        );
    }

    #[test]
    fn clustered_beam_recovers_the_sanitized_profile_best_set() {
        let rules = ScoringRules::cn_0_2_0_fixture();
        let modules = vec![
            module("10129", &[(1408, 3), (1410, 10), (1110, 4)]),
            module("10154", &[(1110, 8), (1111, 9), (1112, 1)]),
            module("10189", &[(2104, 10), (1112, 2), (1410, 2)]),
            module("10870", &[(1111, 10), (1112, 9), (1409, 3)]),
            module("11353", &[(2105, 3), (1114, 9), (1410, 3)]),
            module("11608", &[(1114, 10), (1110, 5), (1410, 3)]),
            module("11803", &[(2404, 9), (1409, 5), (1407, 1)]),
            module("13048", &[(2405, 9), (1110, 5), (1410, 5)]),
            module("13958", &[(1114, 6), (1410, 8), (1110, 3)]),
            module("14874", &[(2104, 10), (1110, 2), (1111, 3)]),
            module("14917", &[(1409, 9), (1110, 5), (1410, 3)]),
            module("14949", &[(2104, 10), (1408, 3), (1410, 4)]),
            module("16613", &[(1112, 10), (1110, 6), (1409, 1)]),
            module("18394", &[(1410, 9), (1110, 3), (1407, 5)]),
            module("5458", &[(2105, 3), (1114, 10), (1407, 3)]),
            module("7586", &[(2405, 6), (1110, 8), (1409, 3)]),
            module("7588", &[(1111, 5), (1410, 10), (1407, 2)]),
            module("7594", &[(1111, 8), (1112, 9), (1110, 2)]),
            module("8153", &[(2105, 9), (1113, 6), (1111, 3)]),
            module("8154", &[(1114, 10), (1111, 4), (1110, 5)]),
            module("11742", &[(1111, 7), (1110, 8), (1408, 3)]),
            module("15749", &[(2404, 3), (1410, 10), (1114, 3)]),
            module("18322", &[(2404, 8), (1410, 5), (1113, 3)]),
            module("18324", &[(1110, 3), (1410, 10), (1112, 4)]),
            module("4977", &[(2105, 8), (1110, 7), (1114, 2)]),
            module("5431", &[(1409, 10), (1114, 3), (1110, 3)]),
            module("5460", &[(2405, 5), (1409, 8), (1114, 3)]),
            module("15844", &[(1409, 9), (1410, 6), (1407, 3)]),
            module("16142", &[(2405, 6), (1409, 8), (1308, 2)]),
            module("18549", &[(1410, 5), (1409, 10), (1307, 3)]),
        ];
        let exact = optimize(
            &rules,
            &OptimizeRequest {
                modules: modules.clone(),
                combination_size: 5,
                max_solutions: 20,
                search_mode: SearchMode::Exact,
                exact_combination_limit: 200_000,
                require_target_match: false,
                ..OptimizeRequest::default()
            },
        )
        .unwrap();
        let beam = optimize(
            &rules,
            &OptimizeRequest {
                modules,
                combination_size: 5,
                max_solutions: 20,
                search_mode: SearchMode::Beam,
                beam_width: 64,
                require_target_match: false,
                ..OptimizeRequest::default()
            },
        )
        .unwrap();

        assert_eq!(exact.solutions[0].score, 1676);
        assert_eq!(beam.solutions[0].score, exact.solutions[0].score);
        let mut best_ids = beam.solutions[0].instance_ids.clone();
        best_ids.sort();
        assert_eq!(best_ids, ["10154", "10870", "14874", "14949", "16613"]);
    }

    #[test]
    fn current_setup_is_scored_separately_without_changing_search_candidates() {
        let rules = ScoringRules::cn_0_2_0_fixture();
        let request = OptimizeRequest {
            modules: vec![
                module("a", &[(1110, 4), (1111, 1)]),
                module("b", &[(1110, 4), (1112, 1)]),
                module("c", &[(1110, 4), (1113, 1)]),
                module("d", &[(1110, 4), (1114, 1)]),
                module("e", &[(1111, 4), (1112, 4)]),
                module("f", &[(1113, 4), (1114, 4)]),
            ],
            current_instance_ids: vec!["e".into(), "b".into(), "a".into(), "d".into()],
            target_attributes: vec![1110],
            search_mode: SearchMode::Exact,
            max_solutions: 3,
            require_target_match: false,
            ..OptimizeRequest::default()
        };
        let response = optimize(&rules, &request).unwrap();
        let current = response.current_setup.unwrap();

        assert_eq!(current.instance_ids, ["e", "b", "a", "d"]);
        assert_eq!(response.search.input_module_count, 6);
        assert_eq!(response.search.candidate_module_count, 6);
        assert_eq!(response.search.total_combinations, 15);
        assert_eq!(
            current.score,
            current.breakdown.threshold_power + current.breakdown.total_link_power
        );
        assert_eq!(
            current.ranking_score,
            current.breakdown.ranking_threshold_power + current.breakdown.total_link_power
        );
        assert!(current.ranking_score > current.score);
        assert_eq!(response.solutions.len(), 3);
        let mut current_ids = current.instance_ids.clone();
        current_ids.sort();
        assert!(response.solutions.iter().all(|solution| {
            let mut solution_ids = solution.instance_ids.clone();
            solution_ids.sort();
            solution_ids != current_ids
        }));
    }

    #[test]
    fn string_instance_ids_remain_lossless() {
        let rules = ScoringRules::cn_0_2_0_fixture();
        let request = OptimizeRequest {
            modules: vec![
                module("9007199254740993", &[(1110, 4), (1111, 4)]),
                module("9007199254740995", &[(1110, 4), (1111, 4)]),
                module("9007199254740997", &[(1110, 4), (1111, 4)]),
                module("9007199254740999", &[(1110, 4), (1111, 4)]),
                module("9007199254741001", &[(1110, 4), (1111, 4)]),
            ],
            current_instance_ids: vec![
                "9007199254740999".into(),
                "9007199254740993".into(),
                "9007199254740997".into(),
                "9007199254740995".into(),
            ],
            search_mode: SearchMode::Exact,
            ..OptimizeRequest::default()
        };
        let response = optimize(&rules, &request).unwrap();
        assert_eq!(response.solutions.len(), 4);
        assert!(response.solutions.iter().all(|solution| {
            solution
                .instance_ids
                .iter()
                .all(|instance_id| instance_id.parse::<u64>().unwrap() > 9_007_199_254_740_991)
        }));
        assert_eq!(
            response.current_setup.unwrap().instance_ids,
            [
                "9007199254740999",
                "9007199254740993",
                "9007199254740997",
                "9007199254740995"
            ]
        );
    }
}
