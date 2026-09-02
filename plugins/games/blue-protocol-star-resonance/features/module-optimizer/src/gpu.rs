//! Optional, dynamically loaded OpenCL exact-search backend.
//!
//! The backend is deliberately an accelerator only: candidate validation,
//! catalog construction, tie-breaking, and final score breakdowns remain in
//! the shared Rust implementation. NVIDIA and AMD both publish OpenCL GPU
//! drivers; systems without one take the deterministic CPU path.

use std::cmp::Reverse;
use std::collections::{BTreeSet, BinaryHeap};
use std::ptr;

use opencl3::command_queue::CommandQueue;
use opencl3::context::Context;
use opencl3::device::{CL_DEVICE_TYPE_GPU, Device, get_all_devices};
use opencl3::kernel::{ExecuteKernel, Kernel};
use opencl3::memory::{Buffer, CL_MEM_READ_ONLY, CL_MEM_WRITE_ONLY};
use opencl3::program::Program;
use opencl3::types::{CL_BLOCKING, cl_int, cl_uint, cl_ulong};

use crate::scoring::{ScoringRules, attribute_multiplier};
use crate::search::{DenseCandidate, RankedCombination, finish_ranked, retain_ranked};
use crate::types::{GpuSupport, OptimizeRequest, SearchBackend};

const OPENCL_BATCH_SIZE: usize = 1_000_000;

pub(crate) struct GpuExactResult {
    pub(crate) ranked: Vec<RankedCombination>,
    pub(crate) evaluated_states: u64,
    pub(crate) device_name: String,
}

pub fn gpu_support() -> GpuSupport {
    match preferred_device() {
        Ok(device) => GpuSupport {
            available: true,
            backend: SearchBackend::OpenCl,
            device_name: device.name().ok(),
            vendor: device.vendor().ok(),
            detail:
                "Cross-vendor OpenCL exact search is available. CPU remains the automatic fallback."
                    .into(),
        },
        Err(error) => GpuSupport {
            available: false,
            backend: SearchBackend::Cpu,
            device_name: None,
            vendor: None,
            detail: error,
        },
    }
}

pub(crate) fn exact_search_opencl(
    rules: &ScoringRules,
    candidates: &[DenseCandidate],
    request: &OptimizeRequest,
    target_attributes: &BTreeSet<i32>,
    exclude_attributes: &BTreeSet<i32>,
    solution_limit: usize,
) -> Result<GpuExactResult, String> {
    let total_combinations = choose(candidates.len(), request.combination_size);
    if total_combinations == 0 {
        return Err("the GPU exact search received no combinations".into());
    }

    let device = preferred_device()?;
    let device_name = device.name().unwrap_or_else(|_| "OpenCL GPU".to_owned());
    let context = Context::from_device(&device)
        .map_err(|error| format!("could not create an OpenCL context: {error}"))?;
    let queue = CommandQueue::create_default(&context, 0)
        .map_err(|error| format!("could not create an OpenCL command queue: {error}"))?;
    let program =
        Program::create_and_build_from_source(&context, OPENCL_SOURCE, "-cl-std=CL1.2")
            .map_err(|error| format!("could not build the OpenCL optimizer kernel: {error}"))?;
    let kernel = Kernel::create(&program, "score_module_combinations")
        .map_err(|error| format!("could not create the OpenCL optimizer kernel: {error}"))?;

    let attribute_ids = rules.attribute_ids().collect::<Vec<_>>();
    let attribute_count = attribute_ids.len();
    let row_stride = attribute_count + 1;
    let maximum_threshold = rules
        .attributes
        .values()
        .flat_map(|levels| levels.iter().map(|(threshold, _)| *threshold))
        .max()
        .unwrap_or_default()
        .max(0) as usize;
    let power_stride = maximum_threshold + 1;

    let mut module_matrix = Vec::with_capacity(candidates.len() * row_stride);
    for candidate in candidates {
        module_matrix.extend_from_slice(&candidate.values);
        module_matrix.push(candidate.total_link_points);
    }
    let mut attribute_power = Vec::with_capacity(attribute_count * power_stride);
    for attribute_id in &attribute_ids {
        let multiplier = attribute_multiplier(*attribute_id, target_attributes, exclude_attributes);
        for value in 0..power_stride {
            attribute_power.push(rules.attribute_power(*attribute_id, value as i32).1 * multiplier);
        }
    }
    let minimums = attribute_ids
        .iter()
        .map(|attribute_id| {
            request
                .min_attr_requirements
                .get(attribute_id)
                .copied()
                .unwrap_or_default()
        })
        .collect::<Vec<_>>();
    let link_power = rules.link_power.clone();

    let mut module_buffer = unsafe {
        Buffer::<cl_int>::create(
            &context,
            CL_MEM_READ_ONLY,
            module_matrix.len(),
            ptr::null_mut(),
        )
    }
    .map_err(|error| format!("could not allocate the OpenCL module buffer: {error}"))?;
    let mut attribute_power_buffer = unsafe {
        Buffer::<cl_int>::create(
            &context,
            CL_MEM_READ_ONLY,
            attribute_power.len(),
            ptr::null_mut(),
        )
    }
    .map_err(|error| format!("could not allocate the OpenCL score table: {error}"))?;
    let mut minimum_buffer = unsafe {
        Buffer::<cl_int>::create(&context, CL_MEM_READ_ONLY, minimums.len(), ptr::null_mut())
    }
    .map_err(|error| format!("could not allocate the OpenCL requirement buffer: {error}"))?;
    let mut link_power_buffer = unsafe {
        Buffer::<cl_int>::create(
            &context,
            CL_MEM_READ_ONLY,
            link_power.len(),
            ptr::null_mut(),
        )
    }
    .map_err(|error| format!("could not allocate the OpenCL Link score table: {error}"))?;
    let maximum_batch = usize::try_from(total_combinations)
        .unwrap_or(usize::MAX)
        .min(OPENCL_BATCH_SIZE);
    let score_buffer = unsafe {
        Buffer::<cl_int>::create(&context, CL_MEM_WRITE_ONLY, maximum_batch, ptr::null_mut())
    }
    .map_err(|error| format!("could not allocate the OpenCL result buffer: {error}"))?;

    unsafe {
        queue
            .enqueue_write_buffer(&mut module_buffer, CL_BLOCKING, 0, &module_matrix, &[])
            .map_err(|error| format!("could not upload modules to the GPU: {error}"))?;
        queue
            .enqueue_write_buffer(
                &mut attribute_power_buffer,
                CL_BLOCKING,
                0,
                &attribute_power,
                &[],
            )
            .map_err(|error| format!("could not upload score tables to the GPU: {error}"))?;
        queue
            .enqueue_write_buffer(&mut minimum_buffer, CL_BLOCKING, 0, &minimums, &[])
            .map_err(|error| format!("could not upload requirements to the GPU: {error}"))?;
        queue
            .enqueue_write_buffer(&mut link_power_buffer, CL_BLOCKING, 0, &link_power, &[])
            .map_err(|error| format!("could not upload Link scores to the GPU: {error}"))?;
    }

    let module_count = candidates.len() as cl_uint;
    let pick_count = request.combination_size as cl_uint;
    let row_stride = row_stride as cl_uint;
    let attribute_count = attribute_count as cl_uint;
    let power_stride = power_stride as cl_uint;
    let link_power_len = link_power.len() as cl_uint;
    let mut top = BinaryHeap::<Reverse<RankedCombination>>::new();
    let mut range_start = 0_u64;
    while range_start < total_combinations {
        let range_len = (total_combinations - range_start).min(OPENCL_BATCH_SIZE as u64);
        let range_start_arg = range_start as cl_ulong;
        let range_len_arg = range_len as cl_ulong;
        let kernel_event = unsafe {
            ExecuteKernel::new(&kernel)
                .set_arg(&module_buffer)
                .set_arg(&module_count)
                .set_arg(&row_stride)
                .set_arg(&attribute_count)
                .set_arg(&pick_count)
                .set_arg(&attribute_power_buffer)
                .set_arg(&power_stride)
                .set_arg(&minimum_buffer)
                .set_arg(&link_power_buffer)
                .set_arg(&link_power_len)
                .set_arg(&range_start_arg)
                .set_arg(&range_len_arg)
                .set_arg(&score_buffer)
                .set_global_work_size(range_len as usize)
                .enqueue_nd_range(&queue)
        }
        .map_err(|error| format!("the OpenCL optimizer kernel failed: {error}"))?;
        let mut scores = vec![i32::MIN; range_len as usize];
        unsafe {
            queue
                .enqueue_read_buffer(
                    &score_buffer,
                    CL_BLOCKING,
                    0,
                    &mut scores,
                    &[kernel_event.get()],
                )
                .map_err(|error| format!("could not read OpenCL optimizer results: {error}"))?;
        }
        for (offset, ranking_score) in scores.into_iter().enumerate() {
            if ranking_score == i32::MIN {
                continue;
            }
            let rank = range_start + offset as u64;
            retain_ranked(
                &mut top,
                RankedCombination {
                    indices: combination_from_rank(
                        candidates.len(),
                        request.combination_size,
                        rank,
                    ),
                    ranking_score,
                },
                solution_limit,
            );
        }
        range_start += range_len;
    }

    Ok(GpuExactResult {
        ranked: finish_ranked(top),
        evaluated_states: total_combinations,
        device_name,
    })
}

fn preferred_device() -> Result<Device, String> {
    let devices = get_all_devices(CL_DEVICE_TYPE_GPU)
        .map_err(|error| format!("OpenCL GPU discovery failed: {error}"))?;
    devices
        .into_iter()
        .map(Device::new)
        .max_by_key(|device| {
            let compute_units = u64::from(device.max_compute_units().unwrap_or_default());
            let clock = u64::from(device.max_clock_frequency().unwrap_or_default());
            let memory = device.global_mem_size().unwrap_or_default();
            (compute_units.saturating_mul(clock), memory)
        })
        .ok_or_else(|| {
            "No OpenCL GPU was found. Install the current NVIDIA or AMD graphics driver, or leave GPU acceleration off."
                .into()
        })
}

fn combination_from_rank(item_count: usize, pick_count: usize, mut rank: u64) -> Vec<usize> {
    let mut combination = Vec::with_capacity(pick_count);
    let mut start = 0_usize;
    for position in 0..pick_count {
        let remaining = pick_count - position - 1;
        let maximum = item_count - remaining - 1;
        for candidate in start..=maximum {
            let suffixes = choose(item_count - candidate - 1, remaining);
            if rank < suffixes {
                combination.push(candidate);
                start = candidate + 1;
                break;
            }
            rank -= suffixes;
        }
    }
    combination
}

fn choose(item_count: usize, pick_count: usize) -> u64 {
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

const OPENCL_SOURCE: &str = r#"
ulong choose_count(ulong n, ulong r) {
    if (r > n) return 0UL;
    if (r == 0UL || r == n) return 1UL;
    if (r > n - r) r = n - r;
    ulong result = 1UL;
    for (ulong i = 0UL; i < r; ++i) {
        result = (result * (n - i)) / (i + 1UL);
    }
    return result;
}

void combination_from_rank(uint n, uint r, ulong rank, uint output[5]) {
    uint start = 0U;
    for (uint position = 0U; position < r; ++position) {
        const uint remaining = r - position - 1U;
        const uint maximum = n - remaining - 1U;
        for (uint candidate = start; candidate <= maximum; ++candidate) {
            const ulong suffixes = choose_count((ulong)(n - candidate - 1U), (ulong)remaining);
            if (rank < suffixes) {
                output[position] = candidate;
                start = candidate + 1U;
                break;
            }
            rank -= suffixes;
        }
    }
}

kernel void score_module_combinations(
    global const int* modules,
    uint module_count,
    uint row_stride,
    uint attribute_count,
    uint pick_count,
    global const int* attribute_power,
    uint power_stride,
    global const int* minimums,
    global const int* link_power,
    uint link_power_len,
    ulong range_start,
    ulong range_len,
    global int* scores) {
    const ulong offset = (ulong)get_global_id(0);
    if (offset >= range_len) return;

    uint combination[5] = {0U, 0U, 0U, 0U, 0U};
    combination_from_rank(module_count, pick_count, range_start + offset, combination);
    int score = 0;
    int total_link = 0;
    int valid = 1;
    for (uint attribute = 0U; attribute < attribute_count; ++attribute) {
        int value = 0;
        for (uint pick = 0U; pick < pick_count; ++pick) {
            value += modules[combination[pick] * row_stride + attribute];
        }
        valid &= value >= minimums[attribute];
        const uint score_value = (uint)min(max(value, 0), (int)power_stride - 1);
        score += attribute_power[attribute * power_stride + score_value];
    }
    for (uint pick = 0U; pick < pick_count; ++pick) {
        total_link += modules[combination[pick] * row_stride + attribute_count];
    }
    const uint link_index = (uint)min(max(total_link, 0), (int)link_power_len - 1);
    scores[offset] = valid ? score + link_power[link_index] : (-2147483647 - 1);
}
"#;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn host_combination_unranking_is_lexicographic_and_complete() {
        let combinations = (0..choose(6, 4))
            .map(|rank| combination_from_rank(6, 4, rank))
            .collect::<Vec<_>>();
        assert_eq!(combinations.len(), 15);
        assert_eq!(combinations[0], [0, 1, 2, 3]);
        assert_eq!(combinations[1], [0, 1, 2, 4]);
        assert_eq!(combinations[14], [2, 3, 4, 5]);
        let unique = combinations.into_iter().collect::<BTreeSet<_>>();
        assert_eq!(unique.len(), 15);
    }
}
