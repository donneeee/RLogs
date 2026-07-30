use std::cmp::{Ordering, Reverse};
use std::collections::{BTreeMap, BTreeSet, BinaryHeap};

use crate::scoring::ScoringRules;
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

#[derive(Debug, Clone, PartialEq, Eq)]
struct RankedCombination {
    indices: Vec<usize>,
    score: i32,
}

impl Ord for RankedCombination {
    fn cmp(&self, other: &Self) -> Ordering {
        self.score
            .cmp(&other.score)
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
    score: i32,
    upper_bound: i32,
}

impl Ord for BeamState {
    fn cmp(&self, other: &Self) -> Ordering {
        self.upper_bound
            .cmp(&other.upper_bound)
            .then_with(|| self.score.cmp(&other.score))
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
    let (candidates, excluded_module_count) =
        prepare_candidates(rules, request, &target_attributes)?;
    if candidates.len() < request.combination_size {
        return Err(OptimizerError::InvalidRequest(format!(
            "{} candidates remain after filters; {} are required",
            candidates.len(),
            request.combination_size
        )));
    }

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

    let (ranked, evaluated_states) = match used_mode {
        SearchMode::Exact => exact_search(
            rules,
            &candidates,
            request,
            &target_attributes,
            &exclude_attributes,
        ),
        SearchMode::Beam => beam_search(
            rules,
            &candidates,
            request,
            &target_attributes,
            &exclude_attributes,
        ),
        SearchMode::Auto => unreachable!("auto mode is resolved before search"),
    };
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
    candidates.sort_by(|left, right| {
        left.module
            .instance_id
            .cmp(&right.module.instance_id)
            .then_with(|| left.module.config_id.cmp(&right.module.config_id))
    });
    let excluded = request.modules.len().saturating_sub(candidates.len());
    Ok((candidates, excluded))
}

fn exact_search(
    rules: &ScoringRules,
    candidates: &[DenseCandidate],
    request: &OptimizeRequest,
    target_attributes: &BTreeSet<i32>,
    exclude_attributes: &BTreeSet<i32>,
) -> (Vec<RankedCombination>, u64) {
    let mut top = BinaryHeap::<Reverse<RankedCombination>>::new();
    let mut combination = (0..request.combination_size).collect::<Vec<_>>();
    let mut evaluated = 0_u64;
    loop {
        evaluated += 1;
        let (values, total_link_points) = sum_combination(candidates, &combination);
        if meets_requirements(rules, &values, &request.min_attr_requirements) {
            let score = rules.score_dense(
                &values,
                total_link_points,
                target_attributes,
                exclude_attributes,
            );
            retain_ranked(
                &mut top,
                RankedCombination {
                    indices: combination.clone(),
                    score,
                },
                request.max_solutions,
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
) -> (Vec<RankedCombination>, u64) {
    let (suffix_values, suffix_totals) = suffix_upper_bounds(candidates, request.combination_size);
    let mut frontier = vec![BeamState {
        indices: [0; MAX_COMBINATION_SIZE],
        values: [0; MAX_ATTRIBUTE_SLOTS],
        depth: 0,
        total_link_points: 0,
        score: 0,
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
                let score = rules.score_dense(
                    &values[..rules.attributes.len()],
                    total_link_points,
                    target_attributes,
                    exclude_attributes,
                );
                let upper_bound = if remaining_after_pick == 0 {
                    score
                } else {
                    score_upper_bound(
                        rules,
                        &values[..rules.attributes.len()],
                        total_link_points,
                        next_start,
                        remaining_after_pick,
                        &suffix_values,
                        &suffix_totals,
                        target_attributes,
                        exclude_attributes,
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
                        score,
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
                    score: state.score,
                },
                request.max_solutions,
            );
        }
    }
    (finish_ranked(top), evaluated)
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
    let instance_ids = modules
        .iter()
        .map(|module| module.instance_id.clone())
        .collect();
    let breakdown = rules.score_breakdown(&modules, target_attributes, exclude_attributes);
    debug_assert_eq!(
        ranked.score,
        breakdown.threshold_power + breakdown.total_link_power
    );
    ModuleSolution {
        instance_ids,
        modules,
        score: ranked.score,
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

#[allow(clippy::too_many_arguments)]
fn score_upper_bound(
    rules: &ScoringRules,
    values: &[i32],
    total_link_points: i32,
    next_start: usize,
    remaining: usize,
    suffix_values: &[Vec<[i32; 6]>],
    suffix_totals: &[[i32; 6]],
    target_attributes: &BTreeSet<i32>,
    exclude_attributes: &BTreeSet<i32>,
) -> i32 {
    let maximum_values = values
        .iter()
        .enumerate()
        .map(|(slot, value)| value + suffix_values[slot][next_start][remaining])
        .collect::<Vec<_>>();
    rules.score_dense(
        &maximum_values,
        total_link_points + suffix_totals[next_start][remaining],
        target_attributes,
        exclude_attributes,
    )
}

fn suffix_upper_bounds(
    candidates: &[DenseCandidate],
    maximum_picks: usize,
) -> (Vec<Vec<[i32; 6]>>, Vec<[i32; 6]>) {
    let count = candidates.len();
    let slots = candidates[0].values.len();
    let mut values = vec![vec![[0; 6]; count + 1]; slots];
    let mut totals = vec![[0; 6]; count + 1];
    for index in (0..count).rev() {
        totals[index] = totals[index + 1];
        for slot_values in &mut values {
            slot_values[index] = slot_values[index + 1];
        }
        for picks in 1..=maximum_picks {
            totals[index][picks] = totals[index][picks]
                .max(candidates[index].total_link_points + totals[index + 1][picks - 1]);
            for (slot, slot_values) in values.iter_mut().enumerate() {
                slot_values[index][picks] = slot_values[index][picks]
                    .max(candidates[index].values[slot] + slot_values[index + 1][picks - 1]);
            }
        }
    }
    (values, totals)
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
        let (score, breakdown) = score_modules(&rules, &modules, &[1110], &[1111]).unwrap();

        // 1110 reaches 8 (29) and is doubled; 1111 is excluded; 1112 reaches
        // 4 (14); special 2104 reaches 8 (59); special 2105 reaches 4 (29).
        // Total link points are 32, which contributes 186.
        assert_eq!(breakdown.threshold_power, 58 + 14 + 59 + 29);
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
        assert_eq!(beam.solutions[0].score, exact.solutions[0].score);
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
            ],
            search_mode: SearchMode::Exact,
            ..OptimizeRequest::default()
        };
        let response = optimize(&rules, &request).unwrap();
        assert_eq!(
            response.solutions[0].instance_ids,
            [
                "9007199254740993",
                "9007199254740995",
                "9007199254740997",
                "9007199254740999"
            ]
        );
    }
}
