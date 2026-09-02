//! Optional, dynamically loaded OpenCL exact and companion-search backend.
//!
//! The backend is deliberately an accelerator only: candidate validation,
//! catalog construction, tie-breaking, and final score breakdowns remain in
//! the shared Rust implementation. NVIDIA and AMD both publish OpenCL GPU
//! drivers; systems without one take the deterministic CPU path.

use std::cmp::Reverse;
use std::collections::{BTreeSet, BinaryHeap};
use std::ptr;
use std::sync::{LazyLock, Mutex, MutexGuard};

use opencl3::command_queue::CommandQueue;
use opencl3::context::Context;
use opencl3::device::{CL_DEVICE_TYPE_GPU, Device, get_all_devices};
use opencl3::kernel::{ExecuteKernel, Kernel};
use opencl3::memory::{Buffer, CL_MEM_READ_ONLY, CL_MEM_READ_WRITE, CL_MEM_WRITE_ONLY};
use opencl3::program::Program;
use opencl3::types::{CL_BLOCKING, cl_int, cl_uchar, cl_uint, cl_ulong};

use crate::scoring::{ScoringRules, attribute_multiplier};
use crate::search::{DenseCandidate, RankedCombination, finish_ranked, retain_ranked};
use crate::types::{GpuSupport, OptimizeRequest, SearchBackend};

const OPENCL_BATCH_SIZE: usize = 1_000_000;
const OPENCL_RADIX_BINS: usize = 256;
const OPENCL_WORKERS_PER_COMPUTE_UNIT: usize = 256;

/// A successfully initialized OpenCL runtime is expensive to build because the
/// driver compiles the scoring kernel. Keep that successful runtime for the
/// life of the app so changing optimizer preferences does not pay the compile
/// cost again. Failures are deliberately not cached: updating a driver or
/// reconnecting an external GPU can make a later attempt succeed.
static OPENCL_RUNTIME: LazyLock<Mutex<Option<OpenClRuntime>>> = LazyLock::new(|| Mutex::new(None));

struct OpenClRuntime {
    context: Context,
    queue: CommandQueue,
    program: Program,
    device_name: String,
    vendor: Option<String>,
    worker_count: usize,
}

impl OpenClRuntime {
    fn build() -> Result<Self, String> {
        let device = preferred_device()?;
        let device_name = device.name().unwrap_or_else(|_| "OpenCL GPU".to_owned());
        let vendor = device.vendor().ok();
        let compute_units = usize::try_from(device.max_compute_units().unwrap_or(4)).unwrap_or(4);
        let context = Context::from_device(&device)
            .map_err(|error| format!("could not create an OpenCL context: {error}"))?;
        let queue = CommandQueue::create_default(&context, 0)
            .map_err(|error| format!("could not create an OpenCL command queue: {error}"))?;
        let program =
            Program::create_and_build_from_source(&context, OPENCL_SOURCE, "-cl-std=CL1.2")
                .map_err(|error| format!("could not build the OpenCL optimizer kernel: {error}"))?;
        Ok(Self {
            context,
            queue,
            program,
            device_name,
            vendor,
            worker_count: compute_units
                .saturating_mul(OPENCL_WORKERS_PER_COMPUTE_UNIT)
                .max(1_024),
        })
    }
}

pub(crate) struct GpuExactResult {
    pub(crate) ranked: Vec<RankedCombination>,
    pub(crate) evaluated_states: u64,
    pub(crate) device_name: String,
}

pub fn gpu_support() -> GpuSupport {
    match initialized_runtime() {
        Ok(runtime_guard) => {
            let runtime = runtime_guard
                .as_ref()
                .expect("OpenCL runtime is initialized by initialized_runtime");
            GpuSupport {
            available: true,
            backend: SearchBackend::OpenCl,
            device_name: Some(runtime.device_name.clone()),
            vendor: runtime.vendor.clone(),
            detail:
                "Cross-vendor OpenCL exact and hybrid search is compiled and ready. CPU remains the automatic fallback."
                    .into(),
            }
        }
        Err(error) => GpuSupport {
            available: false,
            backend: SearchBackend::Cpu,
            device_name: None,
            vendor: None,
            detail: error,
        },
    }
}

fn initialized_runtime() -> Result<MutexGuard<'static, Option<OpenClRuntime>>, String> {
    let mut runtime_guard = OPENCL_RUNTIME
        .lock()
        .map_err(|_| "the OpenCL optimizer runtime lock was poisoned".to_owned())?;
    if runtime_guard.is_none() {
        *runtime_guard = Some(OpenClRuntime::build()?);
    }
    Ok(runtime_guard)
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

    let runtime_guard = initialized_runtime()?;
    let runtime = runtime_guard
        .as_ref()
        .expect("OpenCL runtime is initialized above");
    let context = &runtime.context;
    let queue = &runtime.queue;
    let score_kernel = Kernel::create(&runtime.program, "score_module_combinations")
        .map_err(|error| format!("could not create the OpenCL optimizer kernel: {error}"))?;
    let histogram_kernel = Kernel::create(&runtime.program, "histogram_byte_radix")
        .map_err(|error| format!("could not create the OpenCL histogram kernel: {error}"))?;
    let flag_kernel = Kernel::create(&runtime.program, "flag_scores_by_threshold")
        .map_err(|error| format!("could not create the OpenCL selection kernel: {error}"))?;
    let compact_kernel = Kernel::create(&runtime.program, "compact_selected")
        .map_err(|error| format!("could not create the OpenCL compaction kernel: {error}"))?;

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
            context,
            CL_MEM_READ_ONLY,
            module_matrix.len(),
            ptr::null_mut(),
        )
    }
    .map_err(|error| format!("could not allocate the OpenCL module buffer: {error}"))?;
    let mut attribute_power_buffer = unsafe {
        Buffer::<cl_int>::create(
            context,
            CL_MEM_READ_ONLY,
            attribute_power.len(),
            ptr::null_mut(),
        )
    }
    .map_err(|error| format!("could not allocate the OpenCL score table: {error}"))?;
    let mut minimum_buffer = unsafe {
        Buffer::<cl_int>::create(context, CL_MEM_READ_ONLY, minimums.len(), ptr::null_mut())
    }
    .map_err(|error| format!("could not allocate the OpenCL requirement buffer: {error}"))?;
    let mut link_power_buffer = unsafe {
        Buffer::<cl_int>::create(context, CL_MEM_READ_ONLY, link_power.len(), ptr::null_mut())
    }
    .map_err(|error| format!("could not allocate the OpenCL Link score table: {error}"))?;
    let maximum_batch = usize::try_from(total_combinations)
        .unwrap_or(usize::MAX)
        .min(OPENCL_BATCH_SIZE);
    let score_buffer = unsafe {
        Buffer::<cl_int>::create(context, CL_MEM_READ_WRITE, maximum_batch, ptr::null_mut())
    }
    .map_err(|error| format!("could not allocate the OpenCL result buffer: {error}"))?;
    let rank_buffer = unsafe {
        Buffer::<cl_ulong>::create(context, CL_MEM_READ_WRITE, maximum_batch, ptr::null_mut())
    }
    .map_err(|error| format!("could not allocate the OpenCL rank buffer: {error}"))?;
    let mut histogram_buffer = unsafe {
        Buffer::<cl_uint>::create(
            context,
            CL_MEM_READ_WRITE,
            OPENCL_RADIX_BINS,
            ptr::null_mut(),
        )
    }
    .map_err(|error| format!("could not allocate the OpenCL histogram buffer: {error}"))?;
    let flag_buffer = unsafe {
        Buffer::<cl_uchar>::create(context, CL_MEM_READ_WRITE, maximum_batch, ptr::null_mut())
    }
    .map_err(|error| format!("could not allocate the OpenCL selection flags: {error}"))?;
    let mut selected_count_buffer =
        unsafe { Buffer::<cl_uint>::create(context, CL_MEM_READ_WRITE, 1, ptr::null_mut()) }
            .map_err(|error| format!("could not allocate the OpenCL result counter: {error}"))?;
    let selected_score_buffer = unsafe {
        Buffer::<cl_int>::create(context, CL_MEM_WRITE_ONLY, maximum_batch, ptr::null_mut())
    }
    .map_err(|error| format!("could not allocate the compact OpenCL scores: {error}"))?;
    let selected_rank_buffer = unsafe {
        Buffer::<cl_ulong>::create(context, CL_MEM_WRITE_ONLY, maximum_batch, ptr::null_mut())
    }
    .map_err(|error| format!("could not allocate the compact OpenCL ranks: {error}"))?;

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
        let worker_count = runtime
            .worker_count
            .min(range_len as usize)
            .max(1)
            .div_ceil(64)
            * 64;
        unsafe {
            ExecuteKernel::new(&score_kernel)
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
                .set_arg(&rank_buffer)
                .set_global_work_size(worker_count)
                .set_local_work_size(64)
                .enqueue_nd_range(queue)
        }
        .map_err(|error| format!("the OpenCL optimizer kernel failed: {error}"))?;
        queue
            .finish()
            .map_err(|error| format!("the OpenCL scoring queue did not finish: {error}"))?;

        let mut prefix_mask = 0_u32;
        let mut prefix_value = 0_u32;
        let mut needed = solution_limit.min(range_len as usize) as u32;
        let mut valid_count = 0_u32;
        for byte_index in (0..4_u32).rev() {
            unsafe {
                queue
                    .enqueue_fill_buffer(
                        &mut histogram_buffer,
                        &[0_u32],
                        0,
                        std::mem::size_of::<cl_uint>() * OPENCL_RADIX_BINS,
                        &[],
                    )
                    .map_err(|error| format!("could not clear the OpenCL histogram: {error}"))?;
                ExecuteKernel::new(&histogram_kernel)
                    .set_arg(&score_buffer)
                    .set_arg(&range_len_arg)
                    .set_arg(&prefix_mask)
                    .set_arg(&prefix_value)
                    .set_arg(&byte_index)
                    .set_arg(&histogram_buffer)
                    .set_arg_local_buffer(std::mem::size_of::<cl_uint>() * OPENCL_RADIX_BINS)
                    .set_global_work_size(worker_count)
                    .set_local_work_size(64)
                    .enqueue_nd_range(queue)
                    .map_err(|error| format!("the OpenCL histogram kernel failed: {error}"))?;
            }
            let mut histogram = [0_u32; OPENCL_RADIX_BINS];
            unsafe {
                queue
                    .enqueue_read_buffer(&histogram_buffer, CL_BLOCKING, 0, &mut histogram, &[])
                    .map_err(|error| format!("could not read the OpenCL histogram: {error}"))?;
            }
            let matching = histogram.iter().copied().sum::<u32>();
            if byte_index == 3 {
                valid_count = matching;
                needed = needed.min(valid_count);
            }
            if needed == 0 {
                break;
            }
            let mut accumulated = 0_u32;
            let mut selected_bucket = 0_u32;
            for bucket in (0..OPENCL_RADIX_BINS).rev() {
                accumulated = accumulated.saturating_add(histogram[bucket]);
                if accumulated >= needed {
                    selected_bucket = bucket as u32;
                    break;
                }
            }
            needed = needed.saturating_sub(accumulated - histogram[selected_bucket as usize]);
            let shift = byte_index * 8;
            prefix_mask |= 0xff_u32 << shift;
            prefix_value |= selected_bucket << shift;
        }
        if valid_count == 0 {
            range_start += range_len;
            continue;
        }

        let threshold = prefix_value as cl_int;
        unsafe {
            ExecuteKernel::new(&flag_kernel)
                .set_arg(&score_buffer)
                .set_arg(&range_len_arg)
                .set_arg(&threshold)
                .set_arg(&flag_buffer)
                .set_global_work_size(worker_count)
                .set_local_work_size(64)
                .enqueue_nd_range(queue)
                .map_err(|error| format!("the OpenCL selection kernel failed: {error}"))?;
            queue
                .enqueue_fill_buffer(
                    &mut selected_count_buffer,
                    &[0_u32],
                    0,
                    std::mem::size_of::<cl_uint>(),
                    &[],
                )
                .map_err(|error| format!("could not clear the OpenCL result counter: {error}"))?;
            ExecuteKernel::new(&compact_kernel)
                .set_arg(&score_buffer)
                .set_arg(&rank_buffer)
                .set_arg(&flag_buffer)
                .set_arg(&range_len_arg)
                .set_arg(&selected_score_buffer)
                .set_arg(&selected_rank_buffer)
                .set_arg(&selected_count_buffer)
                .set_global_work_size(worker_count)
                .set_local_work_size(64)
                .enqueue_nd_range(queue)
                .map_err(|error| format!("the OpenCL compaction kernel failed: {error}"))?;
        }
        let mut selected_count = [0_u32];
        unsafe {
            queue
                .enqueue_read_buffer(
                    &selected_count_buffer,
                    CL_BLOCKING,
                    0,
                    &mut selected_count,
                    &[],
                )
                .map_err(|error| format!("could not read the OpenCL result count: {error}"))?;
        }
        let selected_count = selected_count[0] as usize;
        let mut scores = vec![0_i32; selected_count];
        let mut ranks = vec![0_u64; selected_count];
        unsafe {
            queue
                .enqueue_read_buffer(&selected_score_buffer, CL_BLOCKING, 0, &mut scores, &[])
                .map_err(|error| format!("could not read compact OpenCL scores: {error}"))?;
            queue
                .enqueue_read_buffer(&selected_rank_buffer, CL_BLOCKING, 0, &mut ranks, &[])
                .map_err(|error| format!("could not read compact OpenCL ranks: {error}"))?;
        }
        for (ranking_score, rank) in scores.into_iter().zip(ranks) {
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
        device_name: runtime.device_name.clone(),
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
    ulong remaining_rank = rank;
    for (uint position = 0U; position < r; ++position) {
        const uint start = position == 0U ? 0U : output[position - 1U] + 1U;
        const uint remaining_picks = r - position;
        const uint maximum = n - remaining_picks;
        const ulong base = choose_count((ulong)(n - start), (ulong)remaining_picks);
        uint low = start;
        uint high = maximum + 1U;
        while (low + 1U < high) {
            const uint middle = low + ((high - low) >> 1);
            const ulong skipped =
                base - choose_count((ulong)(n - middle), (ulong)remaining_picks);
            if (skipped <= remaining_rank) {
                low = middle;
            } else {
                high = middle;
            }
        }
        const ulong skipped =
            base - choose_count((ulong)(n - low), (ulong)remaining_picks);
        output[position] = low;
        remaining_rank -= skipped;
    }
}

int next_combination(uint n, uint r, uint combination[5]) {
    for (int position = (int)r - 1; position >= 0; --position) {
        const uint maximum = n - (r - (uint)position);
        if (combination[position] < maximum) {
            ++combination[position];
            for (uint suffix = (uint)position + 1U; suffix < r; ++suffix) {
                combination[suffix] = combination[suffix - 1U] + 1U;
            }
            return 1;
        }
    }
    return 0;
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
    global int* scores,
    global ulong* ranks) {
    const ulong worker = (ulong)get_global_id(0);
    const ulong worker_count = (ulong)get_global_size(0);
    const ulong combinations_per_worker =
        (range_len + worker_count - 1UL) / worker_count;
    const ulong segment_start = range_start + worker * combinations_per_worker;
    const ulong range_end = range_start + range_len;
    if (segment_start >= range_end) return;
    const ulong segment_end = min(segment_start + combinations_per_worker, range_end);

    uint combination[5] = {0U, 0U, 0U, 0U, 0U};
    combination_from_rank(module_count, pick_count, segment_start, combination);
    for (ulong rank = segment_start; rank < segment_end; ++rank) {
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
        const ulong output = rank - range_start;
        scores[output] = valid ? score + link_power[link_index] : -1;
        ranks[output] = rank;
        if (rank + 1UL < segment_end) {
            next_combination(module_count, pick_count, combination);
        }
    }
}

kernel void histogram_byte_radix(
    global const int* scores,
    ulong score_count,
    uint prefix_mask,
    uint prefix_value,
    uint byte_index,
    global uint* histogram,
    local uint* local_histogram) {
    const uint local_id = (uint)get_local_id(0);
    const uint local_size = (uint)get_local_size(0);
    for (uint bucket = local_id; bucket < 256U; bucket += local_size) {
        local_histogram[bucket] = 0U;
    }
    barrier(CLK_LOCAL_MEM_FENCE);
    for (ulong index = (ulong)get_global_id(0);
         index < score_count;
         index += (ulong)get_global_size(0)) {
        const int signed_score = scores[index];
        if (signed_score >= 0) {
            const uint score = (uint)signed_score;
            if ((score & prefix_mask) == prefix_value) {
                const uint bucket = (score >> (byte_index * 8U)) & 0xffU;
                atomic_inc((volatile local uint*)&local_histogram[bucket]);
            }
        }
    }
    barrier(CLK_LOCAL_MEM_FENCE);
    for (uint bucket = local_id; bucket < 256U; bucket += local_size) {
        if (local_histogram[bucket] > 0U) {
            atomic_add(
                (volatile global uint*)&histogram[bucket],
                local_histogram[bucket]);
        }
    }
}

kernel void flag_scores_by_threshold(
    global const int* scores,
    ulong score_count,
    int threshold,
    global uchar* flags) {
    for (ulong index = (ulong)get_global_id(0);
         index < score_count;
         index += (ulong)get_global_size(0)) {
        flags[index] = (uchar)(scores[index] >= threshold && scores[index] >= 0);
    }
}

kernel void compact_selected(
    global const int* scores,
    global const ulong* ranks,
    global const uchar* flags,
    ulong score_count,
    global int* selected_scores,
    global ulong* selected_ranks,
    global uint* selected_count) {
    for (ulong index = (ulong)get_global_id(0);
         index < score_count;
         index += (ulong)get_global_size(0)) {
        if (flags[index]) {
            const uint output = atomic_inc((volatile global uint*)selected_count);
            selected_scores[output] = scores[index];
            selected_ranks[output] = ranks[index];
        }
    }
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
