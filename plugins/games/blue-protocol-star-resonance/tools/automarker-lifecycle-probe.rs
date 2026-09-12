//! Exact-build, out-of-process observer for the normal indicator lifecycle.
//!
//! The observer opens one already-running client with query/read rights only.
//! It follows one compile-time allowlisted IL2CPP object chain and samples six
//! reviewed fields. Its default mode cannot emit input. An exact-token armed
//! canary may emit four tiny symmetric orthogonal mouse moves and Escape; it
//! never clicks or confirms a marker. It cannot write/invoke game code, debug,
//! inject, suspend, place markers, inspect packets, or scan/dump process memory.

#[cfg(windows)]
#[allow(dead_code)]
#[path = "../../../../apps/desktop/src/automarker_coordinate_planner.rs"]
mod coordinate_planner;

#[cfg(windows)]
mod windows {
    use std::{
        collections::BTreeMap,
        env,
        error::Error,
        ffi::{OsString, c_void},
        fs::{self, File},
        io::{Read, Write},
        mem::size_of,
        net::TcpStream,
        os::windows::ffi::OsStringExt,
        path::{Path, PathBuf},
        sync::{
            atomic::{AtomicU64, Ordering},
            mpsc,
        },
        thread,
        time::{Duration, Instant, SystemTime, UNIX_EPOCH},
    };

    use crate::coordinate_planner::{
        CertifiedMouseDelta, CoordinatePlannerConfig, CoordinatePlannerSession, PlannerDecision,
        SettledIndicatorObservation,
    };

    use serde::Serialize;
    use sha2::{Digest, Sha256};
    use windows_sys::Win32::{
        Foundation::{CloseHandle, HANDLE, INVALID_HANDLE_VALUE},
        System::{
            Diagnostics::{
                Debug::ReadProcessMemory,
                ToolHelp::{
                    CreateToolhelp32Snapshot, MODULEENTRY32W, Module32FirstW, Module32NextW,
                    PROCESSENTRY32W, Process32FirstW, Process32NextW, TH32CS_SNAPMODULE,
                    TH32CS_SNAPMODULE32, TH32CS_SNAPPROCESS,
                },
            },
            Memory::{
                MEM_COMMIT, MEMORY_BASIC_INFORMATION, PAGE_GUARD, PAGE_NOACCESS, VirtualQueryEx,
            },
            Threading::{
                GetCurrentThreadId, OpenProcess, PROCESS_NAME_WIN32, PROCESS_QUERY_INFORMATION,
                PROCESS_VM_READ, QueryFullProcessImageNameW,
            },
        },
        UI::{
            Input::KeyboardAndMouse::{
                INPUT, INPUT_0, INPUT_KEYBOARD, INPUT_MOUSE, KEYBDINPUT, KEYEVENTF_KEYUP,
                MOUSEEVENTF_MOVE, MOUSEINPUT, SendInput, VIRTUAL_KEY, VK_ESCAPE,
            },
            WindowsAndMessaging::{
                CallNextHookEx, GetForegroundWindow, GetMessageW, GetWindowThreadProcessId,
                LLMHF_INJECTED, MSG, MSLLHOOKSTRUCT, PM_NOREMOVE, PeekMessageW, PostThreadMessageW,
                SetWindowsHookExW, UnhookWindowsHookEx, WH_MOUSE_LL, WM_MOUSEMOVE, WM_QUIT,
            },
        },
    };

    const BUILD: &str = "25247556";
    const APP_ID: &str = "3681810";
    const PROCESS_NAME: &str = "BPSR_STEAM";
    const EXECUTABLE_BYTES: u64 = 808_496;
    const EXECUTABLE_SHA256: &str =
        "90537dbd0e4c9d4b3ed2af06aa8bf7ffe7219fc94bb2f6963673f7b340288588";
    const ASSEMBLY_BYTES: u64 = 218_074_672;
    const ASSEMBLY_SHA256: &str =
        "4a079aec0a3e51a8023aa86bbd152e12068907b65b9aafb355d20bb6d6c41fe3";

    // Exact build 25247556 only. This is the reviewed MethodInfo global used by
    // ZEntityMgr's singleton acquisition. It is not accepted from the CLI.
    const ENTITY_MGR_SINGLETON_METHOD_INFO_RVA: usize = 0x960_BF90;
    const ENTITY_MGR_SINGLETON_TYPE_INFO_RVA: usize = 0x960_BFF8;
    // Reviewed at three independent native callsites for build 25247556.
    const INDICATOR_MGR_SINGLETON_METHOD_INFO_RVA: usize = 0x95EA430;
    const INDICATOR_MGR_SINGLETON_TYPE_INFO_RVA: usize = 0x95EA438;
    const METHOD_INFO_NAME: usize = 0x18;
    const METHOD_INFO_KLASS: usize = 0x20;
    const IL2CPP_CLASS_NAME: usize = 0x10;
    const IL2CPP_CLASS_NAMESPACE: usize = 0x18;
    const IL2CPP_CLASS_STATIC_FIELDS: usize = 0xB8;
    const IL2CPP_CLASS_GENERIC_CONTEXT: usize = 0xC0;
    const GENERIC_CONTEXT_INFLATED_CLASS: usize = 0x10;
    const STATIC_SINGLETON_INSTANCE: usize = 0;
    const ENTITY_MGR_PLAYER_ENT: usize = 0x18;
    const PLAYER_ENT_PURE_COMPONENTS: usize = 0x20;
    const PLAYER_ENT_SKILL_INPUT_COMP: usize = 0x128;
    const SKILL_INPUT_COMP_MGR: usize = 0x28;
    const INDICATOR_POS: usize = 0x9C4;
    const INDICATOR_IS_ENABLE: usize = 0x10;
    const INDICATOR_IS_PC_UP_RELEASE: usize = 0x11;
    const INDICATOR_IS_CAN_RELEASE: usize = 0x12;
    const INDICATOR_DATA_TYPE: usize = 0x48;
    const INDICATOR_DATA_SKILL_ID: usize = 0x4C;
    const INDICATOR_DATA_SLOT_ID: usize = 0x50;
    const INDICATOR_DATA_PARAM_1: usize = 0x54;
    const INDICATOR_DATA_PARAM_2: usize = 0x58;
    const INDICATOR_DATA_MAX_DISTANCE: usize = 0x5C;
    const INDICATOR_MGR_POS: usize = 0x80;
    const INDICATOR_IS_PC_MODE: usize = 0xD8;
    const INDICATOR_CURRENT_VELOCITY: usize = 0xDC;
    const INDICATOR_SKILL_ID: usize = 0xE8;
    const INDICATOR_SLOT_ID: usize = 0xEC;
    const INDICATOR_ENTER_STATE: usize = 0x110;
    const INDICATOR_CAMERA_OPEN_ID: usize = 0x114;
    const SKILL_SLOT_ID: usize = 0x18;
    const SKILL_LAST_USED_SLOT_ID: usize = 0x1C;
    const SKILL_LAST_PRESS_SLOT_ID: usize = 0x20;
    const SKILL_IS_PRESS: usize = 0x38;
    const MAX_C_STRING_BYTES: usize = 96;
    const MIN_USER_ADDRESS: usize = 0x1_0000;
    const MAX_USER_ADDRESS: usize = 0x0000_7fff_ffff_ffff;
    const ARMED_MODE_TOKEN: &str = "marker1-reversible-calibration-v1";
    const PLANNER_ARMED_MODE_TOKEN: &str = "marker1-single-planner-step-and-restore-v1";
    const CLOSED_LOOP_ARMED_MODE_TOKEN: &str = "marker1-closed-loop-aim-and-rollback-v1";
    const INPUT_OBSERVER_TAG: usize = 0x524C_4F47_5341_4D31;
    const CLOSED_LOOP_MAX_MOVES: usize = 4;
    const CLOSED_LOOP_MAX_CUMULATIVE_PIXELS: f64 = 16.0;
    const MARKER_1_SKILL_ID: i32 = 1101;
    const MARKER_1_SLOT_ID: i32 = 201;
    const MARKER_PARAM: f32 = 1.0;
    const MARKER_MAX_DISTANCE: f32 = 18.0;
    const CALIBRATION_PIXELS: i32 = 6;
    const CALIBRATION_SEQUENCE: [[i32; 2]; 4] = [
        [CALIBRATION_PIXELS, 0],
        [-CALIBRATION_PIXELS, 0],
        [0, CALIBRATION_PIXELS],
        [0, -CALIBRATION_PIXELS],
    ];
    const SETTLE_MILLIS: u64 = 350;
    const STABILITY_SAMPLE_MILLIS: u64 = 50;
    const MAX_SETTLED_POSITION_DELTA: f32 = 0.002;
    const MAX_SETTLED_VELOCITY: f32 = 0.02;
    const PROCESS_READ_RIGHTS: u32 = PROCESS_QUERY_INFORMATION | PROCESS_VM_READ;

    static OBSERVED_OWN_MOUSE_MOVES: AtomicU64 = AtomicU64::new(0);
    static OBSERVED_FOREIGN_MOUSE_MOVES: AtomicU64 = AtomicU64::new(0);

    #[derive(Clone, Debug, Eq, PartialEq)]
    struct Roots {
        method_info: usize,
        declaring_class: usize,
        generic_context: usize,
        singleton_class: usize,
        static_fields: usize,
        entity_mgr: usize,
        player_ent: usize,
        pure_components: usize,
        skill_input_comp: usize,
        skill_input_mgr: usize,
        indicator_mgr: usize,
    }

    #[derive(Clone, Debug, PartialEq, Serialize)]
    struct Position {
        x: f32,
        y: f32,
        z: f32,
    }

    #[derive(Clone, Debug, PartialEq, Serialize)]
    struct LifecycleState {
        slot_id: i32,
        last_used_slot_id: i32,
        last_press_slot_id: i32,
        is_press: bool,
        indicator_pos: Position,
        indicator: IndicatorState,
    }

    #[derive(Clone, Debug, PartialEq, Serialize)]
    struct IndicatorState {
        is_enable: bool,
        is_pc_up_release: bool,
        is_can_release: bool,
        indicator_type: u8,
        data_skill_id: i32,
        data_slot_id: i32,
        param_1: f32,
        param_2: f32,
        max_distance: f32,
        position: Position,
        current_velocity: Position,
        is_pc_mode: bool,
        skill_id: i32,
        slot_id: i32,
        enter_state: i32,
        camera_open_id: i32,
    }

    #[derive(Clone, Debug, Serialize)]
    struct CanaryReceipt {
        armed: bool,
        mode: &'static str,
        calibration_pixels: i32,
        marker_1_state_validated: bool,
        foreground_validated_before_every_input: bool,
        root_context_unchanged: bool,
        lifecycle_context_unchanged: bool,
        rank_2_input_excitation: bool,
        escape_emitted: bool,
        baseline: Option<SettledObservation>,
        transitions: Vec<CalibrationTransition>,
        final_return_error: Option<f32>,
        approximately_returned: bool,
        cancelled: bool,
        outcome: &'static str,
        preflight: Option<PreflightDiagnostic>,
        planner_step: Option<PlannerStepReceipt>,
        closed_loop: Option<ClosedLoopReceipt>,
    }

    #[derive(Clone, Debug, Serialize)]
    struct PlannerStepReceipt {
        target: Position,
        player_origin: Option<Position>,
        target_player_distance: Option<f32>,
        proposed_integer_mouse_delta: Option<[i32; 2]>,
        predicted_distance: Option<f32>,
        observed_distance: Option<f32>,
        actual_to_predicted_improvement_ratio: Option<f32>,
        strict_distance_reduction: bool,
        rollback_attempted: bool,
        rollback_not_safe: bool,
        rollback_cancel_emitted: bool,
        inverse_emitted: bool,
        inverse_return_error: Option<f32>,
        outcome: &'static str,
    }

    #[derive(Clone, Debug, PartialEq, Eq)]
    struct LiveMapIdentity {
        session_id: String,
        client_build: String,
        scene_id: i64,
        map_id: u64,
        local_actor_id: u64,
        activity_family_id: String,
    }

    #[derive(Clone, Debug)]
    struct LivePlayerContext {
        identity: LiveMapIdentity,
        origin: Position,
    }

    #[derive(Clone, Debug)]
    struct PlannerCanaryRequest {
        mode: PlannerCanaryMode,
        target: Position,
        rlogs_base_url: String,
    }

    #[derive(Clone, Copy, Debug, Eq, PartialEq)]
    enum PlannerCanaryMode {
        SingleStep,
        ClosedLoop,
    }

    impl PlannerCanaryMode {
        fn token(self) -> &'static str {
            match self {
                Self::SingleStep => PLANNER_ARMED_MODE_TOKEN,
                Self::ClosedLoop => CLOSED_LOOP_ARMED_MODE_TOKEN,
            }
        }
    }

    #[derive(Clone, Debug, Serialize)]
    struct ClosedLoopStepReceipt {
        command_id: u32,
        emitted_integer_mouse_delta: [i32; 2],
        distance_before: f32,
        predicted_distance: f32,
        observed_distance: Option<f32>,
        input_ownership_verified: bool,
    }

    #[derive(Clone, Debug, Serialize)]
    struct ClosedLoopReceipt {
        target: Position,
        player_origin: Option<Position>,
        target_player_distance: Option<f32>,
        arrived: bool,
        arrival_distance: Option<f32>,
        steps: Vec<ClosedLoopStepReceipt>,
        emitted_move_count: usize,
        cumulative_motion_pixels: f64,
        input_observer_started: bool,
        foreign_mouse_moves_observed: u64,
        rollback_attempted: bool,
        rollback_inverse_count: usize,
        rollback_not_safe: bool,
        rollback_cancel_emitted: bool,
        rollback_return_error: Option<f32>,
        outcome: &'static str,
    }

    #[derive(Clone, Copy, Debug, Eq, PartialEq)]
    struct MouseObservationCounts {
        own: u64,
        foreign: u64,
    }

    struct MouseInterferenceObserver {
        thread_id: u32,
        join: Option<thread::JoinHandle<()>>,
    }

    struct PlannerStepRunContext<'a, M: Memory> {
        memory: &'a M,
        module_base: usize,
        process_id: u32,
        started: &'a Instant,
        baseline_roots: &'a Roots,
        baseline_context: MarkerContext,
        calibration: &'a CanaryReceipt,
        request: &'a PlannerCanaryRequest,
        initial_live: &'a LivePlayerContext,
    }

    #[derive(Clone, Debug, Serialize)]
    struct PreflightDiagnostic {
        acquisition_status: &'static str,
        class_context_validated: bool,
        coherent_samples_acquired: bool,
        root_context_unchanged: Option<bool>,
        observed: Option<SafeObservedScalars>,
        gates: Option<PreflightFieldGates>,
        stability: Option<PreflightStability>,
        passed: bool,
    }

    #[derive(Clone, Debug, Serialize)]
    struct SafeObservedScalars {
        input_slot_id: i32,
        input_last_used_slot_id: i32,
        input_last_press_slot_id: i32,
        input_is_press: bool,
        is_enable: bool,
        is_pc_up_release: bool,
        is_can_release: bool,
        indicator_type: u8,
        data_skill_id: i32,
        data_slot_id: i32,
        param_1: f32,
        param_2: f32,
        max_distance: f32,
        current_velocity: Position,
        is_pc_mode: bool,
        manager_skill_id: i32,
        manager_slot_id: i32,
        enter_state: i32,
        camera_open_id: i32,
    }

    #[derive(Clone, Debug, Serialize)]
    struct PreflightFieldGates {
        is_enable: bool,
        is_pc_up_release_false: bool,
        is_can_release: bool,
        indicator_type_point: bool,
        data_skill_id_marker_1: bool,
        data_slot_id_marker_1: bool,
        param_1_expected: bool,
        param_2_expected: bool,
        max_distance_expected: bool,
        is_pc_mode: bool,
        manager_skill_id_marker_1: bool,
        manager_slot_id_marker_1: bool,
    }

    impl PreflightFieldGates {
        fn from_state(state: &LifecycleState) -> Self {
            let indicator = &state.indicator;
            Self {
                is_enable: indicator.is_enable,
                is_pc_up_release_false: !indicator.is_pc_up_release,
                is_can_release: indicator.is_can_release,
                indicator_type_point: indicator.indicator_type == 1,
                data_skill_id_marker_1: indicator.data_skill_id == MARKER_1_SKILL_ID,
                data_slot_id_marker_1: indicator.data_slot_id == MARKER_1_SLOT_ID,
                // buildIndicatorData consumes [1, 18] as Type and MaxDistance;
                // absent array elements 2 and 3 default Param1/Param2 to 1.
                param_1_expected: approx(indicator.param_1, MARKER_PARAM, 0.01),
                param_2_expected: approx(indicator.param_2, MARKER_PARAM, 0.01),
                max_distance_expected: approx(indicator.max_distance, MARKER_MAX_DISTANCE, 0.01),
                is_pc_mode: indicator.is_pc_mode,
                manager_skill_id_marker_1: indicator.skill_id == MARKER_1_SKILL_ID,
                manager_slot_id_marker_1: indicator.slot_id == MARKER_1_SLOT_ID,
            }
        }

        fn all_passed(&self) -> bool {
            self.is_enable
                && self.is_pc_up_release_false
                && self.is_can_release
                && self.indicator_type_point
                && self.data_skill_id_marker_1
                && self.data_slot_id_marker_1
                && self.param_1_expected
                && self.param_2_expected
                && self.max_distance_expected
                && self.is_pc_mode
                && self.manager_skill_id_marker_1
                && self.manager_slot_id_marker_1
        }
    }

    #[derive(Clone, Debug, Serialize)]
    struct PreflightStability {
        sample_gap_millis: u64,
        position_delta: f32,
        maximum_position_delta: f32,
        velocity_norm: f32,
        maximum_velocity_norm: f32,
        position_stable: bool,
        velocity_settled: bool,
    }

    #[derive(Clone, Debug, Serialize)]
    struct SettledObservation {
        elapsed_micros: u128,
        position: Position,
        current_velocity: Position,
        stability_sample_gap_millis: u64,
        stability_position_delta: f32,
        velocity_norm: f32,
        settled: bool,
    }

    #[derive(Clone, Debug, Serialize)]
    struct CalibrationTransition {
        sequence_index: usize,
        emitted_integer_mouse_delta: [i32; 2],
        input_elapsed_micros: u128,
        subsequent_observation: SettledObservation,
        displacement: f32,
    }

    #[derive(Clone, Copy, Debug, Eq, PartialEq)]
    enum CalibrationAction {
        Move([i32; 2]),
        Escape,
        AbortForeground,
        AbortContext,
    }

    #[derive(Clone, Copy, Debug, Eq, PartialEq)]
    enum PlannerRollbackAction {
        Inverse([i32; 2]),
        Escape,
        CannotActWithoutForeground,
    }

    #[derive(Debug, Serialize)]
    struct LifecycleEvent {
        elapsed_micros: u128,
        state: LifecycleState,
    }

    #[derive(Debug, Serialize)]
    struct FileIdentity {
        byte_length: u64,
        sha256: String,
    }

    #[derive(Debug, Serialize)]
    struct Receipt {
        schema_version: u16,
        generated_by: &'static str,
        game: &'static str,
        deployment: &'static str,
        channel: &'static str,
        game_build: &'static str,
        distribution_app_id: &'static str,
        observed_unix_millis: u128,
        duration_millis: u64,
        interval_millis: u64,
        identities: Identities,
        acquisition: Acquisition,
        events: Vec<LifecycleEvent>,
        summary: Summary,
        policy: Policy,
        canary: CanaryReceipt,
    }

    #[derive(Debug, Serialize)]
    struct Identities {
        process_executable: FileIdentity,
        game_assembly: FileIdentity,
        steam_manifest: FileIdentity,
    }

    #[derive(Debug, Serialize)]
    struct Acquisition {
        root_kind: &'static str,
        class_identity_validation_required: bool,
        validated_classes: [&'static str; 7],
        roots_double_read_per_sample: bool,
        lifecycle_state_double_read_per_sample: bool,
    }

    #[derive(Debug, Serialize)]
    struct Summary {
        poll_attempts: u64,
        accepted_samples: u64,
        emitted_transitions: usize,
        unavailable_samples: u64,
        rejected_identity_samples: u64,
        torn_samples: u64,
        placement_attempted: bool,
        programmatic_activation_proven: bool,
    }

    #[derive(Debug, Serialize)]
    struct Policy {
        exact_build_and_hashes_required: bool,
        process_rights: [&'static str; 2],
        allowlisted_pointer_chain_only: bool,
        allowlisted_fields_only: bool,
        heap_or_process_scan_performed: bool,
        process_identifiers_emitted: bool,
        raw_addresses_emitted: bool,
        filesystem_paths_emitted: bool,
        debugger_attached: bool,
        threads_suspended: bool,
        code_injected_or_invoked: bool,
        process_memory_written: bool,
        remote_process_write_rights_requested: bool,
        remote_memory_allocated: bool,
        remote_thread_created: bool,
        dll_injected: bool,
        internal_game_function_invoked: bool,
        packets_observed_or_modified: bool,
        packet_synthesis_performed: bool,
        ordinary_foreground_input_only: bool,
        place_enabled: bool,
        mouse_click_emitted: bool,
        reversible_mouse_move_enabled: bool,
        escape_cancel_enabled: bool,
    }

    #[derive(Default)]
    struct Counters {
        poll_attempts: u64,
        accepted_samples: u64,
        unavailable_samples: u64,
        rejected_identity_samples: u64,
        torn_samples: u64,
    }

    #[derive(Debug, Clone, Copy, Eq, PartialEq)]
    enum AcquireError {
        Unavailable,
        Identity,
        Read,
        Torn,
    }

    trait Memory {
        fn read_exact(&self, address: usize, length: usize) -> Result<Vec<u8>, AcquireError>;
    }

    struct ProcessMemory(HANDLE);

    impl Memory for ProcessMemory {
        fn read_exact(&self, address: usize, length: usize) -> Result<Vec<u8>, AcquireError> {
            if !plausible_address(address) || length == 0 || length > 512 {
                return Err(AcquireError::Read);
            }
            let end = address.checked_add(length).ok_or(AcquireError::Read)?;
            let mut region = MEMORY_BASIC_INFORMATION::default();
            if unsafe {
                VirtualQueryEx(
                    self.0,
                    address as *const c_void,
                    &mut region,
                    size_of::<MEMORY_BASIC_INFORMATION>(),
                )
            } == 0
                || !region_allows_read(&region, address, end)
            {
                return Err(AcquireError::Read);
            }
            let mut bytes = vec![0u8; length];
            let mut read = 0usize;
            let succeeded = unsafe {
                ReadProcessMemory(
                    self.0,
                    address as *const c_void,
                    bytes.as_mut_ptr().cast(),
                    length,
                    &mut read,
                )
            };
            if succeeded == 0 || read != length {
                return Err(AcquireError::Read);
            }
            Ok(bytes)
        }
    }

    struct OwnedHandle(HANDLE);

    impl Drop for OwnedHandle {
        fn drop(&mut self) {
            if !self.0.is_null() && self.0 != INVALID_HANDLE_VALUE {
                unsafe { CloseHandle(self.0) };
            }
        }
    }

    unsafe extern "system" fn low_level_mouse_observer(
        code: i32,
        wparam: usize,
        lparam: isize,
    ) -> isize {
        if code >= 0 && wparam as u32 == WM_MOUSEMOVE && lparam != 0 {
            let event = unsafe { &*(lparam as *const MSLLHOOKSTRUCT) };
            if event.flags & LLMHF_INJECTED != 0 && event.dwExtraInfo == INPUT_OBSERVER_TAG {
                OBSERVED_OWN_MOUSE_MOVES.fetch_add(1, Ordering::SeqCst);
            } else {
                OBSERVED_FOREIGN_MOUSE_MOVES.fetch_add(1, Ordering::SeqCst);
            }
        }
        unsafe { CallNextHookEx(std::ptr::null_mut(), code, wparam, lparam) }
    }

    impl MouseInterferenceObserver {
        fn start() -> Result<Self, &'static str> {
            OBSERVED_OWN_MOUSE_MOVES.store(0, Ordering::SeqCst);
            OBSERVED_FOREIGN_MOUSE_MOVES.store(0, Ordering::SeqCst);
            let (ready_tx, ready_rx) = mpsc::sync_channel(1);
            let join = thread::spawn(move || {
                let thread_id = unsafe { GetCurrentThreadId() };
                let hook = unsafe {
                    SetWindowsHookExW(
                        WH_MOUSE_LL,
                        Some(low_level_mouse_observer),
                        std::ptr::null_mut(),
                        0,
                    )
                };
                if hook.is_null() {
                    let _ = ready_tx.send(None);
                    return;
                }
                let mut message = MSG::default();
                unsafe { PeekMessageW(&mut message, std::ptr::null_mut(), 0, 0, PM_NOREMOVE) };
                if ready_tx.send(Some(thread_id)).is_err() {
                    unsafe { UnhookWindowsHookEx(hook) };
                    return;
                }
                while unsafe { GetMessageW(&mut message, std::ptr::null_mut(), 0, 0) } > 0 {}
                unsafe { UnhookWindowsHookEx(hook) };
            });
            let thread_id = ready_rx
                .recv_timeout(Duration::from_secs(2))
                .map_err(|_| "mouse-interference-observer-start-timeout")?
                .ok_or("mouse-interference-observer-unavailable")?;
            Ok(Self {
                thread_id,
                join: Some(join),
            })
        }

        fn counts(&self) -> MouseObservationCounts {
            MouseObservationCounts {
                own: OBSERVED_OWN_MOUSE_MOVES.load(Ordering::SeqCst),
                foreign: OBSERVED_FOREIGN_MOUSE_MOVES.load(Ordering::SeqCst),
            }
        }
    }

    impl Drop for MouseInterferenceObserver {
        fn drop(&mut self) {
            let stopped = unsafe { PostThreadMessageW(self.thread_id, WM_QUIT, 0, 0) } != 0;
            if let Some(join) = self.join.take()
                && stopped
            {
                let _ = join.join();
            }
        }
    }

    pub fn main() -> Result<(), Box<dyn Error>> {
        let options = parse_options(env::args().skip(1))?;
        reject_unknown_options(&options)?;
        let output = PathBuf::from(required(&options, "output")?);
        if output.exists() {
            return Err("refusing to overwrite the receipt target".into());
        }
        let duration_millis = numeric_option(&options, "duration-ms", 15_000, 100, 60_000)?;
        let interval_millis = numeric_option(&options, "interval-ms", 10, 5, 1_000)?;
        let armed_mode = options.get("armed-mode").map(String::as_str);
        let armed = armed_mode.is_some();
        let planner_mode = match armed_mode {
            Some(PLANNER_ARMED_MODE_TOKEN) => Some(PlannerCanaryMode::SingleStep),
            Some(CLOSED_LOOP_ARMED_MODE_TOKEN) => Some(PlannerCanaryMode::ClosedLoop),
            _ => None,
        };
        let planner_request = if let Some(mode) = planner_mode {
            Some(PlannerCanaryRequest {
                mode,
                target: Position {
                    x: float_option(&options, "target-x")?,
                    y: float_option(&options, "target-y")?,
                    z: float_option(&options, "target-z")?,
                },
                rlogs_base_url: required(&options, "rlogs-base-url")?.to_owned(),
            })
        } else {
            None
        };
        if interval_millis > duration_millis {
            return Err("--interval-ms must not exceed --duration-ms".into());
        }

        let process_executable_path = PathBuf::from(required(&options, "process-executable")?);
        let assembly_path = PathBuf::from(required(&options, "game-assembly")?);
        let manifest_path = PathBuf::from(required(&options, "steam-manifest")?);
        let process_executable = exact_identity(
            &process_executable_path,
            EXECUTABLE_BYTES,
            EXECUTABLE_SHA256,
            "process executable",
        )?;
        let game_assembly = exact_identity(
            &assembly_path,
            ASSEMBLY_BYTES,
            ASSEMBLY_SHA256,
            "GameAssembly",
        )?;
        let manifest_bytes = fs::read(&manifest_path)?;
        require_manifest_identity(std::str::from_utf8(&manifest_bytes)?)?;
        let steam_manifest = identity_from_bytes(&manifest_bytes);

        let process_id = unique_process_id(PROCESS_NAME)?;
        let query_handle = unsafe { OpenProcess(PROCESS_QUERY_INFORMATION, 0, process_id) };
        if query_handle.is_null() {
            return Err("could not query the selected process".into());
        }
        let query_handle = OwnedHandle(query_handle);
        require_process_image(query_handle.0, &process_executable_path)?;
        let module = find_module(process_id, "GameAssembly.dll")?;
        if !same_path(&module.path, &assembly_path)? {
            return Err("running GameAssembly does not match the reviewed file".into());
        }

        let handle = unsafe { OpenProcess(PROCESS_READ_RIGHTS, 0, process_id) };
        if handle.is_null() {
            return Err("could not open the selected process for read/query access".into());
        }
        let handle = OwnedHandle(handle);
        require_process_image(handle.0, &process_executable_path)?;
        // Revalidate immutable inputs immediately before observation.
        exact_identity(
            &process_executable_path,
            EXECUTABLE_BYTES,
            EXECUTABLE_SHA256,
            "process executable",
        )?;
        exact_identity(
            &assembly_path,
            ASSEMBLY_BYTES,
            ASSEMBLY_SHA256,
            "GameAssembly",
        )?;
        require_manifest_identity(std::str::from_utf8(&fs::read(&manifest_path)?)?)?;

        let memory = ProcessMemory(handle.0);
        let (events, counters, canary) = if armed {
            let canary = run_reversible_calibration_canary(
                &memory,
                module.base,
                process_id,
                planner_request.as_ref(),
            );
            (Vec::new(), Counters::default(), canary)
        } else {
            let (events, counters) = observe(
                &memory,
                module.base,
                Duration::from_millis(duration_millis),
                Duration::from_millis(interval_millis),
            );
            (events, counters, read_only_canary_receipt())
        };
        let receipt = Receipt {
            schema_version: 6,
            generated_by: "rlogs-bpsr-automarker-lifecycle-probe",
            game: "blue-protocol-star-resonance",
            deployment: "global",
            channel: "steam",
            game_build: BUILD,
            distribution_app_id: APP_ID,
            observed_unix_millis: SystemTime::now().duration_since(UNIX_EPOCH)?.as_millis(),
            duration_millis,
            interval_millis,
            identities: Identities {
                process_executable,
                game_assembly,
                steam_manifest,
            },
            acquisition: Acquisition {
                root_kind: "reviewed-singleton-method-info",
                class_identity_validation_required: true,
                validated_classes: [
                    "ZUtil.ZSingleton`1",
                    "Panda.ZGame.ZEntityMgr",
                    "Panda.ZGame.PlayerEnt",
                    "Panda.ZGame.PlayerEnt__Storage",
                    "Panda.ZGame.PlayerSkillInputComp",
                    "Panda.ZGame.ZSkillInputMgr",
                    "Panda.ZGame.ZIndicatorMgr",
                ],
                roots_double_read_per_sample: true,
                lifecycle_state_double_read_per_sample: true,
            },
            summary: Summary {
                poll_attempts: counters.poll_attempts,
                accepted_samples: counters.accepted_samples,
                emitted_transitions: events.len(),
                unavailable_samples: counters.unavailable_samples,
                rejected_identity_samples: counters.rejected_identity_samples,
                torn_samples: counters.torn_samples,
                placement_attempted: false,
                programmatic_activation_proven: false,
            },
            events,
            policy: Policy {
                exact_build_and_hashes_required: true,
                process_rights: ["PROCESS_QUERY_INFORMATION", "PROCESS_VM_READ"],
                allowlisted_pointer_chain_only: true,
                allowlisted_fields_only: true,
                heap_or_process_scan_performed: false,
                process_identifiers_emitted: false,
                raw_addresses_emitted: false,
                filesystem_paths_emitted: false,
                debugger_attached: false,
                threads_suspended: false,
                code_injected_or_invoked: false,
                process_memory_written: false,
                remote_process_write_rights_requested: false,
                remote_memory_allocated: false,
                remote_thread_created: false,
                dll_injected: false,
                internal_game_function_invoked: false,
                packets_observed_or_modified: false,
                packet_synthesis_performed: false,
                ordinary_foreground_input_only: true,
                place_enabled: false,
                mouse_click_emitted: false,
                reversible_mouse_move_enabled: armed,
                escape_cancel_enabled: armed,
            },
            canary,
        };
        if let Some(parent) = output.parent() {
            fs::create_dir_all(parent)?;
        }
        let encoded = serde_json::to_vec_pretty(&receipt)?;
        fs::write(&output, [&encoded[..], b"\n"].concat())?;
        println!(
            "sanitized lifecycle receipt written: {} transitions, Place disabled, canary {}",
            receipt.summary.emitted_transitions, receipt.canary.outcome,
        );
        Ok(())
    }

    fn read_only_canary_receipt() -> CanaryReceipt {
        CanaryReceipt {
            armed: false,
            mode: "read-only",
            calibration_pixels: 0,
            marker_1_state_validated: false,
            foreground_validated_before_every_input: false,
            root_context_unchanged: false,
            lifecycle_context_unchanged: false,
            rank_2_input_excitation: false,
            escape_emitted: false,
            baseline: None,
            transitions: Vec::new(),
            final_return_error: None,
            approximately_returned: false,
            cancelled: false,
            outcome: "not-armed-read-only",
            preflight: None,
            planner_step: None,
            closed_loop: None,
        }
    }

    fn run_reversible_calibration_canary(
        memory: &impl Memory,
        module_base: usize,
        process_id: u32,
        planner_request: Option<&PlannerCanaryRequest>,
    ) -> CanaryReceipt {
        let started = Instant::now();
        let mut receipt = CanaryReceipt {
            armed: true,
            mode: planner_request.map_or(ARMED_MODE_TOKEN, |request| request.mode.token()),
            calibration_pixels: CALIBRATION_PIXELS,
            marker_1_state_validated: false,
            foreground_validated_before_every_input: true,
            root_context_unchanged: true,
            lifecycle_context_unchanged: true,
            rank_2_input_excitation: calibration_sequence_is_rank_2(),
            escape_emitted: false,
            baseline: None,
            transitions: Vec::new(),
            final_return_error: None,
            approximately_returned: false,
            cancelled: false,
            outcome: "preflight-rejected-marker-state",
            preflight: None,
            planner_step: None,
            closed_loop: None,
        };
        let (preflight, baseline) = diagnose_marker_1_preflight(memory, module_base, &started);
        receipt.preflight = Some(preflight);
        let baseline = match baseline {
            Some(sample) => sample,
            None => return receipt,
        };
        receipt.marker_1_state_validated = true;
        let baseline_position = baseline.observation.position.clone();
        let baseline_roots = baseline.roots.clone();
        let baseline_context = MarkerContext::from(&baseline.state);
        receipt.baseline = Some(baseline.observation);
        receipt.outcome = "failed-closed";

        let planner_live = if let Some(request) = planner_request {
            let mut failure = empty_planner_receipt(request, "live-player-context-unavailable");
            let live = match read_live_player_context(&request.rlogs_base_url) {
                Ok(value) => value,
                Err(_) => {
                    receipt.planner_step = Some(failure);
                    return cancel_preflight_receipt(receipt, memory, module_base, process_id);
                }
            };
            failure.player_origin = Some(live.origin.clone());
            let distance = position_distance(&request.target, &live.origin);
            failure.target_player_distance = Some(distance);
            if position_norm(&request.target) == 0.0 || distance > MARKER_MAX_DISTANCE {
                failure.outcome = if position_norm(&request.target) == 0.0 {
                    "invalid-zero-target"
                } else {
                    "target-outside-live-player-range"
                };
                receipt.planner_step = Some(failure);
                return cancel_preflight_receipt(receipt, memory, module_base, process_id);
            }
            Some(live)
        } else {
            None
        };

        let input_observer = if planner_request
            .is_some_and(|request| request.mode == PlannerCanaryMode::ClosedLoop)
        {
            match MouseInterferenceObserver::start() {
                Ok(observer) => Some(observer),
                Err(_) => {
                    receipt.closed_loop = planner_request.map(|request| {
                        empty_closed_loop_receipt(request, "input-observer-unavailable")
                    });
                    return cancel_preflight_receipt(receipt, memory, module_base, process_id);
                }
            }
        } else {
            None
        };
        let mut calibration_moves_emitted = Vec::with_capacity(CALIBRATION_SEQUENCE.len());

        for (sequence_index, delta) in CALIBRATION_SEQUENCE.into_iter().enumerate() {
            let pre_input = match stable_marker_1_sample(memory, module_base, &started) {
                Ok(sample) => sample,
                Err(_) => {
                    receipt.root_context_unchanged = false;
                    receipt.lifecycle_context_unchanged = false;
                    receipt.outcome = "aborted-context";
                    break;
                }
            };
            let roots_match = pre_input.roots == baseline_roots;
            let context_matches = MarkerContext::from(&pre_input.state) == baseline_context;
            let live_context_matches = planner_request.is_none()
                || planner_request
                    .zip(planner_live.as_ref())
                    .is_some_and(|(request, expected)| {
                        read_live_player_context(&request.rlogs_base_url).is_ok_and(|current| {
                            live_context_matches(expected, &current, &request.target)
                        })
                    });
            let action = next_calibration_action(
                sequence_index,
                game_is_foreground(process_id),
                roots_match && context_matches && live_context_matches,
            );
            match action {
                CalibrationAction::AbortForeground => {
                    receipt.foreground_validated_before_every_input = false;
                    receipt.outcome = "aborted-foreground";
                    break;
                }
                CalibrationAction::AbortContext => {
                    receipt.root_context_unchanged &= roots_match;
                    receipt.lifecycle_context_unchanged &= context_matches;
                    receipt.outcome = "aborted-context";
                    break;
                }
                CalibrationAction::Move(planned) if planned == delta => {}
                _ => {
                    receipt.outcome = "aborted-sequence";
                    break;
                }
            }
            let input_elapsed_micros = started.elapsed().as_micros();
            let ownership_before = input_observer
                .as_ref()
                .map(MouseInterferenceObserver::counts);
            if !emit_relative_mouse(delta) {
                receipt.outcome = "aborted-input";
                break;
            }
            calibration_moves_emitted.push(delta);
            thread::sleep(Duration::from_millis(SETTLE_MILLIS));
            if let (Some(before), Some(observer)) = (ownership_before, input_observer.as_ref()) {
                let after = observer.counts();
                if !input_ownership_verified(before, after) {
                    receipt.outcome = "aborted-input-interference";
                    break;
                }
            }
            let observed = match stable_marker_1_sample(memory, module_base, &started) {
                Ok(sample) => sample,
                Err(_) => {
                    receipt.root_context_unchanged = false;
                    receipt.lifecycle_context_unchanged = false;
                    receipt.outcome = "aborted-context-after-input";
                    break;
                }
            };
            let roots_match = observed.roots == baseline_roots;
            let context_matches = MarkerContext::from(&observed.state) == baseline_context;
            receipt.root_context_unchanged &= roots_match;
            receipt.lifecycle_context_unchanged &= context_matches;
            if !roots_match || !context_matches {
                receipt.outcome = "aborted-context-after-input";
                break;
            }
            let prior_position = receipt
                .transitions
                .last()
                .map(|transition| &transition.subsequent_observation.position)
                .unwrap_or(&baseline_position);
            let displacement = position_distance(prior_position, &observed.observation.position);
            receipt.transitions.push(CalibrationTransition {
                sequence_index,
                emitted_integer_mouse_delta: delta,
                input_elapsed_micros,
                subsequent_observation: observed.observation,
                displacement,
            });
        }

        if receipt.transitions.len() == CALIBRATION_SEQUENCE.len() {
            if let Some(last) = receipt.transitions.last() {
                let error =
                    position_distance(&baseline_position, &last.subsequent_observation.position);
                let maximum_displacement = receipt
                    .transitions
                    .iter()
                    .map(|transition| transition.displacement)
                    .fold(0.0f32, f32::max);
                receipt.final_return_error = Some(error);
                receipt.approximately_returned = error <= (maximum_displacement * 0.35).max(0.15);
            }
        }

        let closed_loop_calibration_failed = planner_request.is_some_and(|request| {
            request.mode == PlannerCanaryMode::ClosedLoop
                && receipt.transitions.len() != CALIBRATION_SEQUENCE.len()
        });
        if closed_loop_calibration_failed {
            let request = planner_request.expect("closed-loop request");
            receipt.closed_loop = Some(rollback_incomplete_calibration(
                PlannerStepRunContext {
                    memory,
                    module_base,
                    process_id,
                    started: &started,
                    baseline_roots: &baseline_roots,
                    baseline_context,
                    calibration: &receipt,
                    request,
                    initial_live: planner_live
                        .as_ref()
                        .expect("planner preflight established"),
                },
                input_observer
                    .as_ref()
                    .expect("closed-loop observer started"),
                &calibration_moves_emitted,
            ));
        }

        if let Some(request) = planner_request.filter(|_| !closed_loop_calibration_failed) {
            let context = PlannerStepRunContext {
                memory,
                module_base,
                process_id,
                started: &started,
                baseline_roots: &baseline_roots,
                baseline_context,
                calibration: &receipt,
                request,
                initial_live: planner_live
                    .as_ref()
                    .expect("planner preflight established"),
            };
            match request.mode {
                PlannerCanaryMode::SingleStep => {
                    receipt.planner_step = Some(run_single_planner_step(context));
                }
                PlannerCanaryMode::ClosedLoop => {
                    receipt.closed_loop = Some(run_closed_loop_aim(
                        context,
                        input_observer
                            .as_ref()
                            .expect("closed-loop observer started"),
                    ));
                }
            }
        }

        let (final_roots_match, final_lifecycle_match) =
            match stable_marker_1_sample(memory, module_base, &started) {
                Ok(sample) => (
                    sample.roots == baseline_roots,
                    MarkerContext::from(&sample.state) == baseline_context,
                ),
                Err(_) => (false, false),
            };
        receipt.root_context_unchanged &= final_roots_match;
        receipt.lifecycle_context_unchanged &= final_lifecycle_match;
        let escape_action = next_calibration_action(
            CALIBRATION_SEQUENCE.len(),
            game_is_foreground(process_id),
            final_roots_match && final_lifecycle_match,
        );
        if escape_action == CalibrationAction::Escape {
            receipt.escape_emitted = emit_escape();
            if receipt.escape_emitted {
                thread::sleep(Duration::from_millis(150));
                receipt.cancelled = coherent_sample(memory, module_base)
                    .map(|state| !state.indicator.is_enable)
                    .unwrap_or(false);
            }
        } else if escape_action == CalibrationAction::AbortForeground {
            receipt.foreground_validated_before_every_input = false;
        }

        let planner_passed = planner_request.is_none()
            || receipt
                .planner_step
                .as_ref()
                .is_some_and(|step| step.outcome == "passed")
            || receipt
                .closed_loop
                .as_ref()
                .is_some_and(|step| step.outcome == "passed");
        if receipt.transitions.len() == CALIBRATION_SEQUENCE.len()
            && receipt
                .transitions
                .iter()
                .all(|step| step.displacement >= 0.001)
            && receipt.foreground_validated_before_every_input
            && receipt.root_context_unchanged
            && receipt.lifecycle_context_unchanged
            && receipt.rank_2_input_excitation
            && receipt.escape_emitted
            && receipt.approximately_returned
            && receipt.cancelled
            && planner_passed
        {
            receipt.outcome = "passed";
        }
        receipt
    }

    fn run_single_planner_step<M: Memory>(
        context: PlannerStepRunContext<'_, M>,
    ) -> PlannerStepReceipt {
        let PlannerStepRunContext {
            memory,
            module_base,
            process_id,
            started,
            baseline_roots,
            baseline_context,
            calibration,
            request,
            initial_live,
        } = context;
        let mut result = empty_planner_receipt(request, "preflight-failed");
        let live = match read_live_player_context(&request.rlogs_base_url) {
            Ok(value) => value,
            Err(_) => {
                result.outcome = "live-player-context-unavailable";
                return result;
            }
        };
        if !live_context_matches(initial_live, &live, &request.target) {
            result.outcome = "live-context-changed-after-calibration";
            return result;
        }
        result.player_origin = Some(live.origin.clone());
        let player_distance = position_distance(&request.target, &live.origin);
        result.target_player_distance = Some(player_distance);
        if player_distance > MARKER_MAX_DISTANCE {
            result.outcome = "target-outside-live-player-range";
            return result;
        }
        let Some(baseline) = calibration.baseline.as_ref() else {
            return result;
        };
        let mut observations = vec![planner_observation(&baseline.position, None)];
        for transition in &calibration.transitions {
            observations.push(planner_observation(
                &transition.subsequent_observation.position,
                Some(CertifiedMouseDelta {
                    planner_command_id: None,
                    delta: transition.emitted_integer_mouse_delta,
                    exclusive_input_ownership: true,
                }),
            ));
        }
        let mut planner = match CoordinatePlannerSession::new(
            CoordinatePlannerConfig {
                maximum_mouse_step: 4.0,
                maximum_mouse_axis_step: 4,
                ..CoordinatePlannerConfig::default()
            },
            1,
            1,
            1,
        ) {
            Ok(value) => value,
            Err(_) => {
                result.outcome = "planner-rejected-calibration";
                return result;
            }
        };
        let target = [
            f64::from(request.target.x),
            f64::from(request.target.y),
            f64::from(request.target.z),
        ];
        let origin = [
            f64::from(live.origin.x),
            f64::from(live.origin.y),
            f64::from(live.origin.z),
        ];
        let proposal = match planner.plan(&observations, target, origin, 18.0) {
            Ok(PlannerDecision::Move(value)) => value,
            Ok(PlannerDecision::Arrived { .. }) => {
                result.outcome = "target-already-within-arrival-tolerance";
                return result;
            }
            Err(_) => {
                result.outcome = "planner-rejected-target";
                return result;
            }
        };
        let delta = proposal.relative_mouse_delta;
        if delta == [0, 0]
            || delta.into_iter().any(|axis| axis.abs() > 4)
            || (f64::from(delta[0]).powi(2) + f64::from(delta[1]).powi(2)).sqrt() > 4.0
        {
            result.outcome = "planner-step-out-of-bounds";
            return result;
        }
        result.proposed_integer_mouse_delta = Some(delta);
        result.predicted_distance = Some(proposal.predicted_distance as f32);
        let pre_input = match stable_marker_1_sample(memory, module_base, started) {
            Ok(value) => value,
            Err(_) => {
                result.outcome = "context-lost-before-planner-step";
                return result;
            }
        };
        let before = pre_input.observation.position.clone();
        let live_before = read_live_player_context(&request.rlogs_base_url);
        if !game_is_foreground(process_id)
            || pre_input.roots != *baseline_roots
            || MarkerContext::from(&pre_input.state) != baseline_context
            || !live_before
                .as_ref()
                .is_ok_and(|value| live_context_matches(&live, value, &request.target))
        {
            result.outcome = "context-lost-before-planner-step";
            return result;
        }
        if !emit_relative_mouse(delta) {
            result.outcome = "planner-step-input-failed";
            return result;
        }
        result.rollback_attempted = true;
        thread::sleep(Duration::from_millis(SETTLE_MILLIS));
        let observed = stable_marker_1_sample(memory, module_base, started);
        let live_after = read_live_player_context(&request.rlogs_base_url);
        if observed.as_ref().is_err()
            || observed.as_ref().is_ok_and(|value| {
                value.roots != *baseline_roots
                    || MarkerContext::from(&value.state) != baseline_context
            })
            || !live_after
                .as_ref()
                .is_ok_and(|value| live_context_matches(&live, value, &request.target))
        {
            result.outcome = "context-lost-after-planner-step";
        }
        let before_distance = position_distance(&before, &request.target);
        let observed_distance = observed
            .as_ref()
            .ok()
            .map(|value| position_distance(&value.observation.position, &request.target));
        result.observed_distance = observed_distance;
        let predicted_improvement = before_distance - proposal.predicted_distance as f32;
        if let Some(observed_distance) = observed_distance {
            let actual_improvement = before_distance - observed_distance;
            result.strict_distance_reduction = actual_improvement > 0.0;
            let ratio = actual_improvement / predicted_improvement.max(f32::EPSILON);
            result.actual_to_predicted_improvement_ratio = Some(ratio);
        }

        let inverse = [-delta[0], -delta[1]];
        let pre_inverse = stable_marker_1_sample(memory, module_base, started);
        let rollback_context_matches = pre_inverse.as_ref().is_ok_and(|value| {
            value.roots == *baseline_roots && MarkerContext::from(&value.state) == baseline_context
        });
        match next_planner_rollback_action(
            inverse,
            game_is_foreground(process_id),
            rollback_context_matches,
        ) {
            PlannerRollbackAction::Inverse(exact_inverse) => {
                result.inverse_emitted = emit_relative_mouse(exact_inverse);
                if !result.inverse_emitted {
                    result.outcome = "rollback-input-failed";
                }
            }
            PlannerRollbackAction::Escape => {
                result.rollback_not_safe = true;
                result.rollback_cancel_emitted = emit_escape();
                result.outcome = "rollback_not_safe";
            }
            PlannerRollbackAction::CannotActWithoutForeground => {
                result.rollback_not_safe = true;
                result.outcome = "rollback_not_safe";
            }
        }
        if result.inverse_emitted {
            thread::sleep(Duration::from_millis(SETTLE_MILLIS));
            let live_returned = read_live_player_context(&request.rlogs_base_url);
            if let Ok(returned) = stable_marker_1_sample(memory, module_base, started) {
                if returned.roots == *baseline_roots
                    && MarkerContext::from(&returned.state) == baseline_context
                    && live_returned
                        .as_ref()
                        .is_ok_and(|value| live_context_matches(&live, value, &request.target))
                {
                    result.inverse_return_error =
                        Some(position_distance(&before, &returned.observation.position));
                }
            }
        }
        if result.outcome != "context-lost-after-planner-step"
            && observed_distance.is_some_and(|distance| {
                planner_step_passes(
                    before_distance,
                    proposal.predicted_distance as f32,
                    distance,
                    result.inverse_return_error,
                )
            })
            && result.inverse_emitted
        {
            result.outcome = "passed";
        } else if result.outcome == "preflight-failed" {
            result.outcome = "planner-step-validation-failed";
        }
        result
    }

    fn rollback_incomplete_calibration<M: Memory>(
        context: PlannerStepRunContext<'_, M>,
        observer: &MouseInterferenceObserver,
        emitted: &[[i32; 2]],
    ) -> ClosedLoopReceipt {
        let mut result = empty_closed_loop_receipt(context.request, "calibration-failed");
        result.input_observer_started = true;
        result.rollback_attempted = !emitted.is_empty();
        for inverse in reverse_rollback_deltas(emitted) {
            let sample =
                stable_marker_1_sample(context.memory, context.module_base, context.started);
            let context_matches = sample.as_ref().is_ok_and(|value| {
                value.roots == *context.baseline_roots
                    && MarkerContext::from(&value.state) == context.baseline_context
            });
            match next_planner_rollback_action(
                inverse,
                game_is_foreground(context.process_id),
                context_matches,
            ) {
                PlannerRollbackAction::Inverse(exact_inverse) => {
                    let before = observer.counts();
                    if !emit_relative_mouse(exact_inverse) {
                        result.rollback_not_safe = true;
                        result.outcome = "rollback_not_safe";
                        break;
                    }
                    thread::sleep(Duration::from_millis(SETTLE_MILLIS));
                    let after = observer.counts();
                    result.foreign_mouse_moves_observed = after.foreign;
                    if !input_ownership_verified(before, after) {
                        result.rollback_not_safe = true;
                        result.outcome = "rollback_not_safe";
                        break;
                    }
                    result.rollback_inverse_count += 1;
                }
                PlannerRollbackAction::Escape => {
                    result.rollback_not_safe = true;
                    result.rollback_cancel_emitted = emit_escape();
                    result.outcome = "rollback_not_safe";
                    break;
                }
                PlannerRollbackAction::CannotActWithoutForeground => {
                    result.rollback_not_safe = true;
                    result.outcome = "rollback_not_safe";
                    break;
                }
            }
        }
        result
    }

    fn run_closed_loop_aim<M: Memory>(
        context: PlannerStepRunContext<'_, M>,
        observer: &MouseInterferenceObserver,
    ) -> ClosedLoopReceipt {
        let PlannerStepRunContext {
            memory,
            module_base,
            process_id,
            started,
            baseline_roots,
            baseline_context,
            calibration,
            request,
            initial_live,
        } = context;
        let mut result = empty_closed_loop_receipt(request, "preflight-failed");
        result.input_observer_started = true;
        let initial_counts = observer.counts();
        if calibration.transitions.len() != CALIBRATION_SEQUENCE.len()
            || initial_counts.own != CALIBRATION_SEQUENCE.len() as u64
            || initial_counts.foreign != 0
        {
            result.foreign_mouse_moves_observed = initial_counts.foreign;
            result.outcome = "calibration-input-ownership-unverified";
            return result;
        }
        let live = match read_live_player_context(&request.rlogs_base_url) {
            Ok(value) if live_context_matches(initial_live, &value, &request.target) => value,
            _ => {
                result.outcome = "live-context-changed-after-calibration";
                return result;
            }
        };
        result.player_origin = Some(live.origin.clone());
        result.target_player_distance = Some(position_distance(&request.target, &live.origin));

        let baseline = calibration
            .baseline
            .as_ref()
            .expect("complete calibration has a baseline");
        let mut observations = vec![planner_observation(&baseline.position, None)];
        for transition in &calibration.transitions {
            observations.push(planner_observation(
                &transition.subsequent_observation.position,
                Some(CertifiedMouseDelta {
                    planner_command_id: None,
                    delta: transition.emitted_integer_mouse_delta,
                    exclusive_input_ownership: true,
                }),
            ));
        }
        let mut planner = match CoordinatePlannerSession::new(
            CoordinatePlannerConfig {
                maximum_mouse_step: 4.0,
                maximum_mouse_axis_step: 4,
                maximum_commands: CLOSED_LOOP_MAX_MOVES as u32,
                maximum_cumulative_motion: CLOSED_LOOP_MAX_CUMULATIVE_PIXELS,
                arrival_tolerance: 0.075,
                ..CoordinatePlannerConfig::default()
            },
            1,
            1,
            1,
        ) {
            Ok(value) => value,
            Err(_) => {
                result.outcome = "planner-rejected-calibration";
                return result;
            }
        };
        let target = [
            f64::from(request.target.x),
            f64::from(request.target.y),
            f64::from(request.target.z),
        ];
        let origin = [
            f64::from(live.origin.x),
            f64::from(live.origin.y),
            f64::from(live.origin.z),
        ];
        let mut movement_ledger = Vec::with_capacity(CLOSED_LOOP_MAX_MOVES);
        let mut rollback_reference = None;

        loop {
            let pre_input = match stable_marker_1_sample(memory, module_base, started) {
                Ok(value)
                    if value.roots == *baseline_roots
                        && MarkerContext::from(&value.state) == baseline_context =>
                {
                    value
                }
                _ => {
                    result.outcome = "context-lost-before-closed-loop-step";
                    break;
                }
            };
            if !game_is_foreground(process_id) {
                result.outcome = "foreground-lost-before-closed-loop-step";
                break;
            }
            let counts_before = observer.counts();
            if counts_before.foreign != 0 {
                result.foreign_mouse_moves_observed = counts_before.foreign;
                result.outcome = "input-interference-before-closed-loop-step";
                break;
            }
            let current_live = match read_live_player_context(&request.rlogs_base_url) {
                Ok(value) if live_context_matches(&live, &value, &request.target) => value,
                _ => {
                    result.outcome = "live-context-lost-before-closed-loop-step";
                    break;
                }
            };
            if position_distance(&pre_input.observation.position, &request.target)
                > MARKER_MAX_DISTANCE * 2.0
                || position_distance(&request.target, &current_live.origin) > MARKER_MAX_DISTANCE
            {
                result.outcome = "closed-loop-range-gate-failed";
                break;
            }
            if movement_ledger.is_empty() {
                let last = observations.last_mut().expect("calibration observation");
                last.position = [
                    f64::from(pre_input.observation.position.x),
                    f64::from(pre_input.observation.position.y),
                    f64::from(pre_input.observation.position.z),
                ];
                rollback_reference = Some(pre_input.observation.position.clone());
            } else if position_distance(
                &pre_input.observation.position,
                &Position {
                    x: observations.last().unwrap().position[0] as f32,
                    y: observations.last().unwrap().position[1] as f32,
                    z: observations.last().unwrap().position[2] as f32,
                },
            ) > MAX_SETTLED_POSITION_DELTA
            {
                result.outcome = "indicator-moved-without-certified-input";
                break;
            }
            let proposal =
                match planner.plan(&observations, target, origin, MARKER_MAX_DISTANCE.into()) {
                    Ok(PlannerDecision::Arrived { distance }) => {
                        result.arrived = true;
                        result.arrival_distance = Some(distance as f32);
                        result.outcome = "arrived-awaiting-rollback";
                        break;
                    }
                    Ok(PlannerDecision::Move(value))
                        if movement_ledger.len() < CLOSED_LOOP_MAX_MOVES =>
                    {
                        value
                    }
                    Ok(PlannerDecision::Move(_)) => {
                        result.outcome = "closed-loop-command-budget-exhausted";
                        break;
                    }
                    Err(_) => {
                        result.outcome = "closed-loop-planner-rejected";
                        break;
                    }
                };
            let delta = proposal.relative_mouse_delta;
            if delta == [0, 0]
                || delta.into_iter().any(|axis| axis.abs() > 4)
                || (f64::from(delta[0]).powi(2) + f64::from(delta[1]).powi(2)).sqrt() > 4.0
            {
                result.outcome = "closed-loop-step-out-of-bounds";
                break;
            }
            let distance_before =
                position_distance(&pre_input.observation.position, &request.target);
            if !emit_relative_mouse(delta) {
                result.outcome = "closed-loop-input-failed";
                break;
            }
            movement_ledger.push(delta);
            thread::sleep(Duration::from_millis(SETTLE_MILLIS));
            let counts_after = observer.counts();
            let ownership_verified = input_ownership_verified(counts_before, counts_after);
            result.foreign_mouse_moves_observed = counts_after.foreign;
            let observed = stable_marker_1_sample(memory, module_base, started);
            let observed_distance = observed
                .as_ref()
                .ok()
                .map(|value| position_distance(&value.observation.position, &request.target));
            result.steps.push(ClosedLoopStepReceipt {
                command_id: proposal.command_id,
                emitted_integer_mouse_delta: delta,
                distance_before,
                predicted_distance: proposal.predicted_distance as f32,
                observed_distance,
                input_ownership_verified: ownership_verified,
            });
            if !ownership_verified {
                result.outcome = "closed-loop-input-interference";
                break;
            }
            let observed = match observed {
                Ok(value)
                    if value.roots == *baseline_roots
                        && MarkerContext::from(&value.state) == baseline_context =>
                {
                    value
                }
                _ => {
                    result.outcome = "context-lost-after-closed-loop-step";
                    break;
                }
            };
            observations.push(planner_observation(
                &observed.observation.position,
                Some(CertifiedMouseDelta {
                    planner_command_id: Some(proposal.command_id),
                    delta,
                    exclusive_input_ownership: true,
                }),
            ));
        }

        result.emitted_move_count = movement_ledger.len();
        result.cumulative_motion_pixels = movement_ledger
            .iter()
            .map(|delta| (f64::from(delta[0]).powi(2) + f64::from(delta[1]).powi(2)).sqrt())
            .sum();
        result.rollback_attempted = !movement_ledger.is_empty();
        for inverse in reverse_rollback_deltas(&movement_ledger) {
            let rollback_sample = stable_marker_1_sample(memory, module_base, started);
            let context_matches = rollback_sample.as_ref().is_ok_and(|value| {
                value.roots == *baseline_roots
                    && MarkerContext::from(&value.state) == baseline_context
            });
            match next_planner_rollback_action(
                inverse,
                game_is_foreground(process_id),
                context_matches,
            ) {
                PlannerRollbackAction::Inverse(exact_inverse) => {
                    let before = observer.counts();
                    if !emit_relative_mouse(exact_inverse) {
                        result.rollback_not_safe = true;
                        result.outcome = "rollback_not_safe";
                        break;
                    }
                    thread::sleep(Duration::from_millis(SETTLE_MILLIS));
                    let after = observer.counts();
                    if !input_ownership_verified(before, after) {
                        result.foreign_mouse_moves_observed = after.foreign;
                        result.rollback_not_safe = true;
                        result.outcome = "rollback_not_safe";
                        break;
                    }
                    result.rollback_inverse_count += 1;
                }
                PlannerRollbackAction::Escape => {
                    result.rollback_not_safe = true;
                    result.rollback_cancel_emitted = emit_escape();
                    result.outcome = "rollback_not_safe";
                    break;
                }
                PlannerRollbackAction::CannotActWithoutForeground => {
                    result.rollback_not_safe = true;
                    result.outcome = "rollback_not_safe";
                    break;
                }
            }
        }
        if result.rollback_inverse_count == movement_ledger.len()
            && let (Some(reference), Ok(returned)) = (
                rollback_reference,
                stable_marker_1_sample(memory, module_base, started),
            )
            && returned.roots == *baseline_roots
            && MarkerContext::from(&returned.state) == baseline_context
        {
            result.rollback_return_error = Some(position_distance(
                &reference,
                &returned.observation.position,
            ));
        }
        if result.arrived
            && !result.rollback_not_safe
            && result.rollback_inverse_count == movement_ledger.len()
            && result
                .rollback_return_error
                .is_some_and(|error| error.is_finite() && error <= 0.01)
            && result.foreign_mouse_moves_observed == 0
        {
            result.outcome = "passed";
        }
        result
    }

    fn empty_closed_loop_receipt(
        request: &PlannerCanaryRequest,
        outcome: &'static str,
    ) -> ClosedLoopReceipt {
        ClosedLoopReceipt {
            target: request.target.clone(),
            player_origin: None,
            target_player_distance: None,
            arrived: false,
            arrival_distance: None,
            steps: Vec::new(),
            emitted_move_count: 0,
            cumulative_motion_pixels: 0.0,
            input_observer_started: false,
            foreign_mouse_moves_observed: 0,
            rollback_attempted: false,
            rollback_inverse_count: 0,
            rollback_not_safe: false,
            rollback_cancel_emitted: false,
            rollback_return_error: None,
            outcome,
        }
    }

    fn empty_planner_receipt(
        request: &PlannerCanaryRequest,
        outcome: &'static str,
    ) -> PlannerStepReceipt {
        PlannerStepReceipt {
            target: request.target.clone(),
            player_origin: None,
            target_player_distance: None,
            proposed_integer_mouse_delta: None,
            predicted_distance: None,
            observed_distance: None,
            actual_to_predicted_improvement_ratio: None,
            strict_distance_reduction: false,
            rollback_attempted: false,
            rollback_not_safe: false,
            rollback_cancel_emitted: false,
            inverse_emitted: false,
            inverse_return_error: None,
            outcome,
        }
    }

    fn cancel_preflight_receipt(
        mut receipt: CanaryReceipt,
        memory: &impl Memory,
        module_base: usize,
        process_id: u32,
    ) -> CanaryReceipt {
        if game_is_foreground(process_id) {
            receipt.escape_emitted = emit_escape();
            if receipt.escape_emitted {
                thread::sleep(Duration::from_millis(150));
                receipt.cancelled = coherent_sample(memory, module_base)
                    .map(|state| !state.indicator.is_enable)
                    .unwrap_or(false);
            }
        } else {
            receipt.foreground_validated_before_every_input = false;
        }
        receipt
    }

    fn planner_step_passes(
        before_distance: f32,
        predicted_distance: f32,
        observed_distance: f32,
        inverse_return_error: Option<f32>,
    ) -> bool {
        let predicted_improvement = before_distance - predicted_distance;
        let actual_improvement = before_distance - observed_distance;
        before_distance.is_finite()
            && predicted_distance.is_finite()
            && observed_distance.is_finite()
            && predicted_improvement > 0.0
            && actual_improvement > 0.0
            && actual_improvement / predicted_improvement >= 0.20
            && inverse_return_error.is_some_and(|error| error.is_finite() && error <= 0.002)
    }

    fn live_context_matches(
        expected: &LivePlayerContext,
        current: &LivePlayerContext,
        target: &Position,
    ) -> bool {
        current.identity == expected.identity
            && position_distance(target, &current.origin).is_finite()
            && position_distance(target, &current.origin) <= MARKER_MAX_DISTANCE
    }

    fn planner_observation(
        position: &Position,
        applied_mouse_delta: Option<CertifiedMouseDelta>,
    ) -> SettledIndicatorObservation {
        SettledIndicatorObservation {
            position: [
                f64::from(position.x),
                f64::from(position.y),
                f64::from(position.z),
            ],
            applied_mouse_delta,
            lifecycle_generation: 1,
            root_generation: 1,
            context_identity: 1,
            settled: true,
            foreground: true,
            context_continuous: true,
        }
    }

    #[derive(Clone)]
    struct StableMarkerSample {
        roots: Roots,
        state: LifecycleState,
        observation: SettledObservation,
    }

    #[derive(Clone, Copy, Debug, PartialEq)]
    struct MarkerContext {
        slot_id: i32,
        last_used_slot_id: i32,
        last_press_slot_id: i32,
        is_press: bool,
        is_enable: bool,
        is_pc_up_release: bool,
        is_can_release: bool,
        indicator_type: u8,
        data_skill_id: i32,
        data_slot_id: i32,
        param_1: f32,
        param_2: f32,
        max_distance: f32,
        is_pc_mode: bool,
        skill_id: i32,
        indicator_slot_id: i32,
        enter_state: i32,
        camera_open_id: i32,
    }

    impl From<&LifecycleState> for MarkerContext {
        fn from(state: &LifecycleState) -> Self {
            Self {
                slot_id: state.slot_id,
                last_used_slot_id: state.last_used_slot_id,
                last_press_slot_id: state.last_press_slot_id,
                is_press: state.is_press,
                is_enable: state.indicator.is_enable,
                is_pc_up_release: state.indicator.is_pc_up_release,
                is_can_release: state.indicator.is_can_release,
                indicator_type: state.indicator.indicator_type,
                data_skill_id: state.indicator.data_skill_id,
                data_slot_id: state.indicator.data_slot_id,
                param_1: state.indicator.param_1,
                param_2: state.indicator.param_2,
                max_distance: state.indicator.max_distance,
                is_pc_mode: state.indicator.is_pc_mode,
                skill_id: state.indicator.skill_id,
                indicator_slot_id: state.indicator.slot_id,
                enter_state: state.indicator.enter_state,
                camera_open_id: state.indicator.camera_open_id,
            }
        }
    }

    impl From<&LifecycleState> for SafeObservedScalars {
        fn from(state: &LifecycleState) -> Self {
            Self {
                input_slot_id: state.slot_id,
                input_last_used_slot_id: state.last_used_slot_id,
                input_last_press_slot_id: state.last_press_slot_id,
                input_is_press: state.is_press,
                is_enable: state.indicator.is_enable,
                is_pc_up_release: state.indicator.is_pc_up_release,
                is_can_release: state.indicator.is_can_release,
                indicator_type: state.indicator.indicator_type,
                data_skill_id: state.indicator.data_skill_id,
                data_slot_id: state.indicator.data_slot_id,
                param_1: state.indicator.param_1,
                param_2: state.indicator.param_2,
                max_distance: state.indicator.max_distance,
                current_velocity: state.indicator.current_velocity.clone(),
                is_pc_mode: state.indicator.is_pc_mode,
                manager_skill_id: state.indicator.skill_id,
                manager_slot_id: state.indicator.slot_id,
                enter_state: state.indicator.enter_state,
                camera_open_id: state.indicator.camera_open_id,
            }
        }
    }

    fn diagnose_marker_1_preflight(
        memory: &impl Memory,
        module_base: usize,
        started: &Instant,
    ) -> (PreflightDiagnostic, Option<StableMarkerSample>) {
        let rejected = |status, class_context_validated| PreflightDiagnostic {
            acquisition_status: status,
            class_context_validated,
            coherent_samples_acquired: false,
            root_context_unchanged: None,
            observed: None,
            gates: None,
            stability: None,
            passed: false,
        };
        let initial_roots = match acquire_roots(memory, module_base) {
            Ok(roots) => roots,
            Err(error) => return (rejected(acquire_error_label(error), false), None),
        };
        let (first_roots, first) = match coherent_sample_with_roots(memory, module_base) {
            Ok(sample) => sample,
            Err(error) => return (rejected(acquire_error_label(error), true), None),
        };
        thread::sleep(Duration::from_millis(STABILITY_SAMPLE_MILLIS));
        let (second_roots, second) = match coherent_sample_with_roots(memory, module_base) {
            Ok(sample) => sample,
            Err(error) => return (rejected(acquire_error_label(error), true), None),
        };
        let roots_unchanged = initial_roots == first_roots && first_roots == second_roots;
        let gates = PreflightFieldGates::from_state(&second);
        let position_delta =
            position_distance(&first.indicator.position, &second.indicator.position);
        let velocity_norm = position_norm(&second.indicator.current_velocity);
        let stability = PreflightStability {
            sample_gap_millis: STABILITY_SAMPLE_MILLIS,
            position_delta,
            maximum_position_delta: MAX_SETTLED_POSITION_DELTA,
            velocity_norm,
            maximum_velocity_norm: MAX_SETTLED_VELOCITY,
            position_stable: position_delta <= MAX_SETTLED_POSITION_DELTA,
            velocity_settled: velocity_norm <= MAX_SETTLED_VELOCITY,
        };
        let passed = roots_unchanged
            && gates.all_passed()
            && stability.position_stable
            && stability.velocity_settled;
        let observation = SettledObservation {
            elapsed_micros: started.elapsed().as_micros(),
            position: second.indicator.position.clone(),
            current_velocity: second.indicator.current_velocity.clone(),
            stability_sample_gap_millis: STABILITY_SAMPLE_MILLIS,
            stability_position_delta: position_delta,
            velocity_norm,
            settled: passed,
        };
        let diagnostic = PreflightDiagnostic {
            acquisition_status: "coherent",
            class_context_validated: true,
            coherent_samples_acquired: true,
            root_context_unchanged: Some(roots_unchanged),
            observed: Some(SafeObservedScalars::from(&second)),
            gates: Some(gates),
            stability: Some(stability),
            passed,
        };
        let sample = passed.then_some(StableMarkerSample {
            roots: second_roots,
            state: second,
            observation,
        });
        (diagnostic, sample)
    }

    fn acquire_error_label(error: AcquireError) -> &'static str {
        match error {
            AcquireError::Unavailable => "unavailable",
            AcquireError::Identity => "identity-rejected",
            AcquireError::Read => "read-failed",
            AcquireError::Torn => "torn",
        }
    }

    fn stable_marker_1_sample(
        memory: &impl Memory,
        module_base: usize,
        started: &Instant,
    ) -> Result<StableMarkerSample, Box<dyn Error>> {
        diagnose_marker_1_preflight(memory, module_base, started)
            .1
            .ok_or_else(|| "Marker 1 preflight or settle gate failed; no input emitted".into())
    }

    fn next_calibration_action(
        completed_transitions: usize,
        foreground: bool,
        context_matches: bool,
    ) -> CalibrationAction {
        if !foreground {
            return CalibrationAction::AbortForeground;
        }
        if !context_matches {
            return CalibrationAction::AbortContext;
        }
        CALIBRATION_SEQUENCE
            .get(completed_transitions)
            .copied()
            .map_or(CalibrationAction::Escape, CalibrationAction::Move)
    }

    fn next_planner_rollback_action(
        inverse: [i32; 2],
        foreground: bool,
        context_matches: bool,
    ) -> PlannerRollbackAction {
        if !foreground {
            return PlannerRollbackAction::CannotActWithoutForeground;
        }
        if !context_matches {
            return PlannerRollbackAction::Escape;
        }
        PlannerRollbackAction::Inverse(inverse)
    }

    fn input_ownership_verified(
        before: MouseObservationCounts,
        after: MouseObservationCounts,
    ) -> bool {
        after.own == before.own.saturating_add(1) && after.foreign == before.foreign
    }

    fn reverse_rollback_deltas(movement_ledger: &[[i32; 2]]) -> Vec<[i32; 2]> {
        movement_ledger
            .iter()
            .rev()
            .map(|delta| [-delta[0], -delta[1]])
            .collect()
    }

    fn calibration_sequence_is_rank_2() -> bool {
        let xx: i64 = CALIBRATION_SEQUENCE
            .iter()
            .map(|delta| i64::from(delta[0]).pow(2))
            .sum();
        let yy: i64 = CALIBRATION_SEQUENCE
            .iter()
            .map(|delta| i64::from(delta[1]).pow(2))
            .sum();
        let xy: i64 = CALIBRATION_SEQUENCE
            .iter()
            .map(|delta| i64::from(delta[0]) * i64::from(delta[1]))
            .sum();
        xx * yy - xy * xy > 0
    }

    fn approx(left: f32, right: f32, epsilon: f32) -> bool {
        left.is_finite() && right.is_finite() && (left - right).abs() <= epsilon
    }

    fn position_distance(left: &Position, right: &Position) -> f32 {
        ((left.x - right.x).powi(2) + (left.y - right.y).powi(2) + (left.z - right.z).powi(2))
            .sqrt()
    }

    fn position_norm(position: &Position) -> f32 {
        (position.x.powi(2) + position.y.powi(2) + position.z.powi(2)).sqrt()
    }

    fn read_live_player_context(base_url: &str) -> Result<LivePlayerContext, &'static str> {
        let authority = base_url
            .strip_prefix("http://127.0.0.1:")
            .ok_or("rlogs-base-url-must-be-loopback-http")?;
        if authority.is_empty() || !authority.bytes().all(|byte| byte.is_ascii_digit()) {
            return Err("rlogs-base-url-must-be-loopback-http");
        }
        let initial = local_json_get(authority, "/api/runtime/live/mechanics-map")?;
        let initial_revision = initial
            .get("revision")
            .and_then(serde_json::Value::as_u64)
            .ok_or("missing-map-revision")?;
        let initial_observed = initial
            .get("snapshot")
            .and_then(|value| value.get("last_observed_micros"))
            .and_then(serde_json::Value::as_u64)
            .ok_or("missing-map-observation-clock")?;
        let wait_body = format!("{{\"after_revision\":{initial_revision},\"timeout_millis\":750}}");
        let map = local_json_request(
            authority,
            "POST",
            "/api/runtime/live/mechanics-map/wait",
            Some(wait_body.as_bytes()),
        )?;
        if map.get("revision").and_then(serde_json::Value::as_u64) <= Some(initial_revision)
            || map
                .get("snapshot")
                .and_then(|value| value.get("last_observed_micros"))
                .and_then(serde_json::Value::as_u64)
                <= Some(initial_observed)
        {
            return Err("mechanics-map-not-fresh");
        }
        let presets = local_json_get(authority, "/api/automarkers/presets")?;
        let snapshot = map.get("snapshot").ok_or("missing-map-snapshot")?;
        if snapshot
            .get("client_build")
            .and_then(serde_json::Value::as_str)
            != Some(BUILD)
            || snapshot
                .get("local_position_observed")
                .and_then(serde_json::Value::as_bool)
                != Some(true)
        {
            return Err("map-context-not-current-build-positioned");
        }
        let local_actor = snapshot
            .get("local_actor_id")
            .and_then(serde_json::Value::as_u64)
            .ok_or("missing-local-actor")?;
        let entity = snapshot
            .get("entities")
            .and_then(serde_json::Value::as_array)
            .and_then(|entities| {
                entities.iter().find(|entity| {
                    entity.get("actor_id").and_then(serde_json::Value::as_u64) == Some(local_actor)
                })
            })
            .ok_or("missing-local-entity")?;
        if entity.get("stale").and_then(serde_json::Value::as_bool) != Some(false) {
            return Err("local-entity-stale");
        }
        let number = |key| {
            entity
                .get(key)
                .and_then(serde_json::Value::as_f64)
                .filter(|value| value.is_finite())
                .map(|value| value as f32)
                .ok_or("invalid-local-position")
        };
        let context = presets.get("context").ok_or("missing-automarker-context")?;
        let scene_id = snapshot
            .get("scene_id")
            .and_then(serde_json::Value::as_i64)
            .ok_or("missing-scene")?;
        let map_id = snapshot
            .get("map_id")
            .and_then(serde_json::Value::as_u64)
            .ok_or("missing-map")?;
        if context
            .get("clientBuild")
            .and_then(serde_json::Value::as_str)
            != Some(BUILD)
            || context.get("sceneId").and_then(serde_json::Value::as_i64) != Some(scene_id)
            || context.get("mapId").and_then(serde_json::Value::as_u64) != Some(map_id)
        {
            return Err("automarker-map-context-mismatch");
        }
        Ok(LivePlayerContext {
            identity: LiveMapIdentity {
                session_id: snapshot
                    .get("session_id")
                    .and_then(serde_json::Value::as_str)
                    .ok_or("missing-session")?
                    .to_owned(),
                client_build: BUILD.to_owned(),
                scene_id,
                map_id,
                local_actor_id: local_actor,
                activity_family_id: context
                    .get("activityFamilyId")
                    .and_then(serde_json::Value::as_str)
                    .ok_or("missing-family")?
                    .to_owned(),
            },
            origin: Position {
                x: number("x")?,
                y: number("y")?,
                z: number("z")?,
            },
        })
    }

    fn local_json_get(authority: &str, path: &str) -> Result<serde_json::Value, &'static str> {
        local_json_request(authority, "GET", path, None)
    }

    fn local_json_request(
        authority: &str,
        method: &str,
        path: &str,
        body: Option<&[u8]>,
    ) -> Result<serde_json::Value, &'static str> {
        let mut stream = TcpStream::connect(format!("127.0.0.1:{authority}"))
            .map_err(|_| "rlogs-host-unavailable")?;
        stream
            .set_read_timeout(Some(Duration::from_secs(2)))
            .map_err(|_| "rlogs-host-unavailable")?;
        let body = body.unwrap_or_default();
        write!(stream, "{method} {path} HTTP/1.1\r\nHost: 127.0.0.1:{authority}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n", body.len())
            .map_err(|_| "rlogs-host-unavailable")?;
        stream
            .write_all(body)
            .map_err(|_| "rlogs-host-unavailable")?;
        let mut bytes = Vec::new();
        stream
            .read_to_end(&mut bytes)
            .map_err(|_| "rlogs-host-unavailable")?;
        let split = bytes
            .windows(4)
            .position(|window| window == b"\r\n\r\n")
            .ok_or("invalid-rlogs-response")?;
        let header = std::str::from_utf8(&bytes[..split]).map_err(|_| "invalid-rlogs-response")?;
        if !header.starts_with("HTTP/1.1 200 ") {
            return Err("rlogs-host-rejected-request");
        }
        if header.to_ascii_lowercase().contains("transfer-encoding:") {
            return Err("unsupported-rlogs-transfer-encoding");
        }
        let content_length = header
            .lines()
            .find_map(|line| {
                line.to_ascii_lowercase()
                    .strip_prefix("content-length:")
                    .map(str::trim)
                    .map(str::to_owned)
            })
            .ok_or("missing-rlogs-content-length")?
            .parse::<usize>()
            .map_err(|_| "invalid-rlogs-content-length")?;
        let response_body = &bytes[split + 4..];
        if response_body.len() != content_length {
            return Err("rlogs-content-length-mismatch");
        }
        serde_json::from_slice(response_body).map_err(|_| "invalid-rlogs-json")
    }

    fn game_is_foreground(process_id: u32) -> bool {
        let window = unsafe { GetForegroundWindow() };
        if window.is_null() {
            return false;
        }
        let mut foreground_process_id = 0u32;
        unsafe { GetWindowThreadProcessId(window, &mut foreground_process_id) };
        foreground_process_id == process_id
    }

    fn emit_relative_mouse(delta: [i32; 2]) -> bool {
        let input = INPUT {
            r#type: INPUT_MOUSE,
            Anonymous: INPUT_0 {
                mi: MOUSEINPUT {
                    dx: delta[0],
                    dy: delta[1],
                    mouseData: 0,
                    dwFlags: MOUSEEVENTF_MOVE,
                    time: 0,
                    dwExtraInfo: INPUT_OBSERVER_TAG,
                },
            },
        };
        let inputs = [input];
        (unsafe {
            SendInput(
                inputs.len() as u32,
                inputs.as_ptr(),
                size_of::<INPUT>() as i32,
            )
        }) == inputs.len() as u32
    }

    fn emit_escape() -> bool {
        let down = INPUT {
            r#type: INPUT_KEYBOARD,
            Anonymous: INPUT_0 {
                ki: KEYBDINPUT {
                    wVk: VK_ESCAPE as VIRTUAL_KEY,
                    wScan: 0,
                    dwFlags: 0,
                    time: 0,
                    dwExtraInfo: 0,
                },
            },
        };
        let up = INPUT {
            r#type: INPUT_KEYBOARD,
            Anonymous: INPUT_0 {
                ki: KEYBDINPUT {
                    wVk: VK_ESCAPE as VIRTUAL_KEY,
                    wScan: 0,
                    dwFlags: KEYEVENTF_KEYUP,
                    time: 0,
                    dwExtraInfo: 0,
                },
            },
        };
        let inputs = [down, up];
        (unsafe {
            SendInput(
                inputs.len() as u32,
                inputs.as_ptr(),
                size_of::<INPUT>() as i32,
            )
        }) == inputs.len() as u32
    }

    fn observe(
        memory: &impl Memory,
        module_base: usize,
        duration: Duration,
        interval: Duration,
    ) -> (Vec<LifecycleEvent>, Counters) {
        let start = Instant::now();
        let mut counters = Counters::default();
        let mut events = Vec::new();
        let mut previous = None;
        loop {
            counters.poll_attempts += 1;
            match coherent_sample(memory, module_base) {
                Ok(state) => {
                    counters.accepted_samples += 1;
                    if previous.as_ref() != Some(&state) {
                        events.push(LifecycleEvent {
                            elapsed_micros: start.elapsed().as_micros(),
                            state: state.clone(),
                        });
                        previous = Some(state);
                    }
                }
                Err(AcquireError::Unavailable) => counters.unavailable_samples += 1,
                Err(AcquireError::Identity) => counters.rejected_identity_samples += 1,
                Err(AcquireError::Read) => counters.unavailable_samples += 1,
                Err(AcquireError::Torn) => counters.torn_samples += 1,
            }
            if start.elapsed() >= duration {
                break;
            }
            thread::sleep(interval);
        }
        (events, counters)
    }

    fn coherent_sample(
        memory: &impl Memory,
        module_base: usize,
    ) -> Result<LifecycleState, AcquireError> {
        coherent_sample_with_roots(memory, module_base).map(|(_, state)| state)
    }

    fn coherent_sample_with_roots(
        memory: &impl Memory,
        module_base: usize,
    ) -> Result<(Roots, LifecycleState), AcquireError> {
        let before = acquire_roots(memory, module_base)?;
        let first_state = read_state(memory, &before)?;
        let second_state = read_state(memory, &before)?;
        let after = acquire_roots(memory, module_base)?;
        if before != after || first_state != second_state {
            return Err(AcquireError::Torn);
        }
        Ok((before, first_state))
    }

    fn acquire_roots(memory: &impl Memory, module_base: usize) -> Result<Roots, AcquireError> {
        let slot = checked_add(module_base, ENTITY_MGR_SINGLETON_METHOD_INFO_RVA)?;
        let type_info_slot = checked_add(module_base, ENTITY_MGR_SINGLETON_TYPE_INFO_RVA)?;
        let method_info = pointer_at(memory, slot, false)?;
        let method_name = address_at(memory, checked_add(method_info, METHOD_INFO_NAME)?)?;
        if c_string(memory, method_name)? != "get_Instance" {
            return Err(AcquireError::Identity);
        }
        let declaring_class =
            pointer_at(memory, checked_add(method_info, METHOD_INFO_KLASS)?, false)?;
        validate_singleton_class(memory, declaring_class)?;
        let generic_context = pointer_at(
            memory,
            checked_add(declaring_class, IL2CPP_CLASS_GENERIC_CONTEXT)?,
            false,
        )?;
        let singleton_class = pointer_at(
            memory,
            checked_add(generic_context, GENERIC_CONTEXT_INFLATED_CLASS)?,
            false,
        )?;
        let type_info_class = pointer_at(memory, type_info_slot, false)?;
        if singleton_class != type_info_class {
            return Err(AcquireError::Identity);
        }
        validate_singleton_class(memory, singleton_class)?;
        let static_fields = pointer_at(
            memory,
            checked_add(singleton_class, IL2CPP_CLASS_STATIC_FIELDS)?,
            false,
        )?;
        let entity_mgr = pointer_at(
            memory,
            checked_add(static_fields, STATIC_SINGLETON_INSTANCE)?,
            true,
        )?;
        validate_object(memory, entity_mgr, "ZEntityMgr", "Panda.ZGame")?;
        let player_ent = pointer_at(
            memory,
            checked_add(entity_mgr, ENTITY_MGR_PLAYER_ENT)?,
            true,
        )?;
        validate_object(memory, player_ent, "PlayerEnt", "Panda.ZGame")?;
        let pure_components = pointer_at(
            memory,
            checked_add(player_ent, PLAYER_ENT_PURE_COMPONENTS)?,
            true,
        )?;
        validate_object(memory, pure_components, "PlayerEnt__Storage", "Panda.ZGame")?;
        let skill_input_comp = pointer_at(
            memory,
            checked_add(player_ent, PLAYER_ENT_SKILL_INPUT_COMP)?,
            true,
        )?;
        validate_object(
            memory,
            skill_input_comp,
            "PlayerSkillInputComp",
            "Panda.ZGame",
        )?;
        let skill_input_mgr = pointer_at(
            memory,
            checked_add(skill_input_comp, SKILL_INPUT_COMP_MGR)?,
            true,
        )?;
        validate_object(memory, skill_input_mgr, "ZSkillInputMgr", "Panda.ZGame")?;
        let indicator_mgr = acquire_singleton_instance(
            memory,
            module_base,
            INDICATOR_MGR_SINGLETON_METHOD_INFO_RVA,
            INDICATOR_MGR_SINGLETON_TYPE_INFO_RVA,
            "ZIndicatorMgr",
            "Panda.ZGame",
        )?;
        Ok(Roots {
            method_info,
            declaring_class,
            generic_context,
            singleton_class,
            static_fields,
            entity_mgr,
            player_ent,
            pure_components,
            skill_input_comp,
            skill_input_mgr,
            indicator_mgr,
        })
    }

    fn acquire_singleton_instance(
        memory: &impl Memory,
        module_base: usize,
        method_info_rva: usize,
        type_info_rva: usize,
        instance_name: &str,
        instance_namespace: &str,
    ) -> Result<usize, AcquireError> {
        let method_info = pointer_at(memory, checked_add(module_base, method_info_rva)?, false)?;
        let method_name = address_at(memory, checked_add(method_info, METHOD_INFO_NAME)?)?;
        if c_string(memory, method_name)? != "get_Instance" {
            return Err(AcquireError::Identity);
        }
        let declaring_class =
            pointer_at(memory, checked_add(method_info, METHOD_INFO_KLASS)?, false)?;
        validate_singleton_class(memory, declaring_class)?;
        let generic_context = pointer_at(
            memory,
            checked_add(declaring_class, IL2CPP_CLASS_GENERIC_CONTEXT)?,
            false,
        )?;
        let singleton_class = pointer_at(
            memory,
            checked_add(generic_context, GENERIC_CONTEXT_INFLATED_CLASS)?,
            false,
        )?;
        if singleton_class != pointer_at(memory, checked_add(module_base, type_info_rva)?, false)? {
            return Err(AcquireError::Identity);
        }
        validate_singleton_class(memory, singleton_class)?;
        let static_fields = pointer_at(
            memory,
            checked_add(singleton_class, IL2CPP_CLASS_STATIC_FIELDS)?,
            false,
        )?;
        let instance = pointer_at(memory, static_fields, true)?;
        validate_object(memory, instance, instance_name, instance_namespace)?;
        Ok(instance)
    }

    fn validate_singleton_class(memory: &impl Memory, class: usize) -> Result<(), AcquireError> {
        validate_class(memory, class, "ZSingleton`1", "ZUtil")
    }

    fn read_state(memory: &impl Memory, roots: &Roots) -> Result<LifecycleState, AcquireError> {
        let skill = roots.skill_input_mgr;
        let slot_id = i32_at(memory, checked_add(skill, SKILL_SLOT_ID)?)?;
        let last_used_slot_id = i32_at(memory, checked_add(skill, SKILL_LAST_USED_SLOT_ID)?)?;
        let last_press_slot_id = i32_at(memory, checked_add(skill, SKILL_LAST_PRESS_SLOT_ID)?)?;
        let is_press = match byte_at(memory, checked_add(skill, SKILL_IS_PRESS)?)? {
            0 => false,
            1 => true,
            _ => return Err(AcquireError::Identity),
        };
        let bytes = memory.read_exact(checked_add(roots.pure_components, INDICATOR_POS)?, 12)?;
        let position = Position {
            x: f32::from_le_bytes(bytes[0..4].try_into().map_err(|_| AcquireError::Read)?),
            y: f32::from_le_bytes(bytes[4..8].try_into().map_err(|_| AcquireError::Read)?),
            z: f32::from_le_bytes(bytes[8..12].try_into().map_err(|_| AcquireError::Read)?),
        };
        if !position.x.is_finite() || !position.y.is_finite() || !position.z.is_finite() {
            return Err(AcquireError::Identity);
        }
        let indicator = roots.indicator_mgr;
        let indicator_state = IndicatorState {
            is_enable: bool_at(memory, checked_add(indicator, INDICATOR_IS_ENABLE)?)?,
            is_pc_up_release: bool_at(memory, checked_add(indicator, INDICATOR_IS_PC_UP_RELEASE)?)?,
            is_can_release: bool_at(memory, checked_add(indicator, INDICATOR_IS_CAN_RELEASE)?)?,
            indicator_type: byte_at(memory, checked_add(indicator, INDICATOR_DATA_TYPE)?)?,
            data_skill_id: i32_at(memory, checked_add(indicator, INDICATOR_DATA_SKILL_ID)?)?,
            data_slot_id: i32_at(memory, checked_add(indicator, INDICATOR_DATA_SLOT_ID)?)?,
            param_1: f32_at(memory, checked_add(indicator, INDICATOR_DATA_PARAM_1)?)?,
            param_2: f32_at(memory, checked_add(indicator, INDICATOR_DATA_PARAM_2)?)?,
            max_distance: f32_at(memory, checked_add(indicator, INDICATOR_DATA_MAX_DISTANCE)?)?,
            position: position_at(memory, checked_add(indicator, INDICATOR_MGR_POS)?)?,
            current_velocity: position_at(
                memory,
                checked_add(indicator, INDICATOR_CURRENT_VELOCITY)?,
            )?,
            is_pc_mode: bool_at(memory, checked_add(indicator, INDICATOR_IS_PC_MODE)?)?,
            skill_id: i32_at(memory, checked_add(indicator, INDICATOR_SKILL_ID)?)?,
            slot_id: i32_at(memory, checked_add(indicator, INDICATOR_SLOT_ID)?)?,
            enter_state: i32_at(memory, checked_add(indicator, INDICATOR_ENTER_STATE)?)?,
            camera_open_id: i32_at(memory, checked_add(indicator, INDICATOR_CAMERA_OPEN_ID)?)?,
        };
        Ok(LifecycleState {
            slot_id,
            last_used_slot_id,
            last_press_slot_id,
            is_press,
            indicator_pos: position,
            indicator: indicator_state,
        })
    }

    fn validate_object(
        memory: &impl Memory,
        object: usize,
        name: &str,
        namespace: &str,
    ) -> Result<(), AcquireError> {
        let class = pointer_at(memory, object, false)?;
        validate_class(memory, class, name, namespace)
    }

    fn validate_class(
        memory: &impl Memory,
        class: usize,
        expected_name: &str,
        expected_namespace: &str,
    ) -> Result<(), AcquireError> {
        let name = address_at(memory, checked_add(class, IL2CPP_CLASS_NAME)?)?;
        let namespace = address_at(memory, checked_add(class, IL2CPP_CLASS_NAMESPACE)?)?;
        if c_string(memory, name)? != expected_name
            || c_string(memory, namespace)? != expected_namespace
        {
            return Err(AcquireError::Identity);
        }
        Ok(())
    }

    fn c_string(memory: &impl Memory, address: usize) -> Result<String, AcquireError> {
        let bytes = memory.read_exact(address, MAX_C_STRING_BYTES)?;
        let end = bytes
            .iter()
            .position(|byte| *byte == 0)
            .ok_or(AcquireError::Identity)?;
        if end == 0 {
            return Ok(String::new());
        }
        std::str::from_utf8(&bytes[..end])
            .map(str::to_owned)
            .map_err(|_| AcquireError::Identity)
    }

    fn pointer_at(
        memory: &impl Memory,
        address: usize,
        null_is_unavailable: bool,
    ) -> Result<usize, AcquireError> {
        let bytes = memory.read_exact(address, size_of::<usize>())?;
        let value = usize::from_le_bytes(bytes.try_into().map_err(|_| AcquireError::Read)?);
        if value == 0 && null_is_unavailable {
            return Err(AcquireError::Unavailable);
        }
        if !plausible_pointer(value) {
            return Err(AcquireError::Identity);
        }
        Ok(value)
    }

    fn address_at(memory: &impl Memory, address: usize) -> Result<usize, AcquireError> {
        let bytes = memory.read_exact(address, size_of::<usize>())?;
        let value = usize::from_le_bytes(bytes.try_into().map_err(|_| AcquireError::Read)?);
        plausible_address(value)
            .then_some(value)
            .ok_or(AcquireError::Identity)
    }

    fn i32_at(memory: &impl Memory, address: usize) -> Result<i32, AcquireError> {
        let bytes = memory.read_exact(address, 4)?;
        Ok(i32::from_le_bytes(
            bytes.try_into().map_err(|_| AcquireError::Read)?,
        ))
    }

    fn byte_at(memory: &impl Memory, address: usize) -> Result<u8, AcquireError> {
        Ok(memory.read_exact(address, 1)?[0])
    }

    fn bool_at(memory: &impl Memory, address: usize) -> Result<bool, AcquireError> {
        match byte_at(memory, address)? {
            0 => Ok(false),
            1 => Ok(true),
            _ => Err(AcquireError::Identity),
        }
    }

    fn f32_at(memory: &impl Memory, address: usize) -> Result<f32, AcquireError> {
        let bytes = memory.read_exact(address, 4)?;
        let value = f32::from_le_bytes(bytes.try_into().map_err(|_| AcquireError::Read)?);
        value
            .is_finite()
            .then_some(value)
            .ok_or(AcquireError::Identity)
    }

    fn position_at(memory: &impl Memory, address: usize) -> Result<Position, AcquireError> {
        Ok(Position {
            x: f32_at(memory, address)?,
            y: f32_at(memory, checked_add(address, 4)?)?,
            z: f32_at(memory, checked_add(address, 8)?)?,
        })
    }

    fn checked_add(left: usize, right: usize) -> Result<usize, AcquireError> {
        let value = left.checked_add(right).ok_or(AcquireError::Read)?;
        plausible_address(value)
            .then_some(value)
            .ok_or(AcquireError::Read)
    }

    fn plausible_address(value: usize) -> bool {
        (MIN_USER_ADDRESS..=MAX_USER_ADDRESS).contains(&value)
    }

    fn plausible_pointer(value: usize) -> bool {
        value % size_of::<usize>() == 0 && plausible_address(value)
    }

    fn region_allows_read(region: &MEMORY_BASIC_INFORMATION, address: usize, end: usize) -> bool {
        let base = region.BaseAddress as usize;
        region.State == MEM_COMMIT
            && region.Protect & (PAGE_NOACCESS | PAGE_GUARD) == 0
            && address >= base
            && end <= base.saturating_add(region.RegionSize)
    }

    struct ModuleIdentity {
        base: usize,
        path: PathBuf,
    }

    fn find_module(process_id: u32, expected: &str) -> Result<ModuleIdentity, Box<dyn Error>> {
        let snapshot = unsafe {
            CreateToolhelp32Snapshot(TH32CS_SNAPMODULE | TH32CS_SNAPMODULE32, process_id)
        };
        if snapshot == INVALID_HANDLE_VALUE {
            return Err("could not enumerate selected-process modules".into());
        }
        let snapshot = OwnedHandle(snapshot);
        let mut entry = MODULEENTRY32W {
            dwSize: size_of::<MODULEENTRY32W>() as u32,
            ..Default::default()
        };
        let mut present = unsafe { Module32FirstW(snapshot.0, &mut entry) } != 0;
        while present {
            if names_match(expected, &wide_string(&entry.szModule)) {
                return Ok(ModuleIdentity {
                    base: entry.modBaseAddr as usize,
                    path: PathBuf::from(wide_string(&entry.szExePath)),
                });
            }
            present = unsafe { Module32NextW(snapshot.0, &mut entry) } != 0;
        }
        Err("selected process has no reviewed GameAssembly module".into())
    }

    fn unique_process_id(expected: &str) -> Result<u32, Box<dyn Error>> {
        let snapshot = unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) };
        if snapshot == INVALID_HANDLE_VALUE {
            return Err("could not enumerate processes".into());
        }
        let snapshot = OwnedHandle(snapshot);
        let mut entry = PROCESSENTRY32W {
            dwSize: size_of::<PROCESSENTRY32W>() as u32,
            ..Default::default()
        };
        let mut matches = Vec::new();
        let mut present = unsafe { Process32FirstW(snapshot.0, &mut entry) } != 0;
        while present {
            if names_match(expected, &wide_string(&entry.szExeFile)) {
                matches.push(entry.th32ProcessID);
            }
            present = unsafe { Process32NextW(snapshot.0, &mut entry) } != 0;
        }
        match matches.as_slice() {
            [process_id] => Ok(*process_id),
            [] => Err("no exact-name game process is running".into()),
            _ => Err("more than one exact-name game process is running".into()),
        }
    }

    fn require_process_image(handle: HANDLE, expected: &Path) -> Result<(), Box<dyn Error>> {
        let mut buffer = vec![0u16; 32_768];
        let mut length = buffer.len() as u32;
        if unsafe {
            QueryFullProcessImageNameW(handle, PROCESS_NAME_WIN32, buffer.as_mut_ptr(), &mut length)
        } == 0
        {
            return Err("could not resolve selected-process image identity".into());
        }
        buffer.truncate(length as usize);
        let actual = PathBuf::from(OsString::from_wide(&buffer));
        if !same_path(&actual, expected)? {
            return Err("selected process image does not match the reviewed executable".into());
        }
        Ok(())
    }

    fn wide_string(value: &[u16]) -> String {
        let length = value
            .iter()
            .position(|unit| *unit == 0)
            .unwrap_or(value.len());
        OsString::from_wide(&value[..length])
            .to_string_lossy()
            .into_owned()
    }

    fn names_match(expected: &str, actual: &str) -> bool {
        fn normalized(value: &str) -> String {
            let lower = value.to_ascii_lowercase();
            lower.strip_suffix(".exe").unwrap_or(&lower).to_owned()
        }
        normalized(expected) == normalized(actual)
    }

    fn same_path(left: &Path, right: &Path) -> Result<bool, Box<dyn Error>> {
        Ok(fs::canonicalize(left)?
            .to_string_lossy()
            .eq_ignore_ascii_case(&fs::canonicalize(right)?.to_string_lossy()))
    }

    fn exact_identity(
        path: &Path,
        expected_bytes: u64,
        expected_sha256: &str,
        label: &str,
    ) -> Result<FileIdentity, Box<dyn Error>> {
        let identity = file_identity(path)?;
        if identity.byte_length != expected_bytes || identity.sha256 != expected_sha256 {
            return Err(format!("{label} does not match exact build {BUILD}").into());
        }
        Ok(identity)
    }

    fn file_identity(path: &Path) -> Result<FileIdentity, Box<dyn Error>> {
        let mut source = File::open(path)?;
        let byte_length = source.metadata()?.len();
        let mut digest = Sha256::new();
        let mut buffer = vec![0u8; 1024 * 1024];
        loop {
            let read = source.read(&mut buffer)?;
            if read == 0 {
                break;
            }
            digest.update(&buffer[..read]);
        }
        Ok(FileIdentity {
            byte_length,
            sha256: format!("{:x}", digest.finalize()),
        })
    }

    fn identity_from_bytes(bytes: &[u8]) -> FileIdentity {
        FileIdentity {
            byte_length: bytes.len() as u64,
            sha256: format!("{:x}", Sha256::digest(bytes)),
        }
    }

    fn require_manifest_identity(manifest: &str) -> Result<(), Box<dyn Error>> {
        if acf_value(manifest, "buildid").as_deref() != Some(BUILD)
            || acf_value(manifest, "appid").as_deref() != Some(APP_ID)
        {
            return Err("Steam manifest does not match the exact app/build".into());
        }
        Ok(())
    }

    fn acf_value(contents: &str, key: &str) -> Option<String> {
        contents.lines().find_map(|line| {
            let mut quoted = line.split('"').skip(1).step_by(2);
            let actual_key = quoted.next()?;
            let value = quoted.next()?;
            actual_key
                .eq_ignore_ascii_case(key)
                .then(|| value.to_owned())
        })
    }

    fn parse_options(
        mut arguments: impl Iterator<Item = String>,
    ) -> Result<BTreeMap<String, String>, Box<dyn Error>> {
        let mut options = BTreeMap::new();
        while let Some(argument) = arguments.next() {
            let key = argument
                .strip_prefix("--")
                .ok_or("positional arguments are not accepted")?;
            let value = arguments.next().ok_or("an option is missing its value")?;
            if options.insert(key.to_owned(), value).is_some() {
                return Err("duplicate option".into());
            }
        }
        Ok(options)
    }

    fn reject_unknown_options(options: &BTreeMap<String, String>) -> Result<(), Box<dyn Error>> {
        const ALLOWED: [&str; 11] = [
            "build",
            "process-executable",
            "game-assembly",
            "steam-manifest",
            "output",
            "duration-ms",
            "armed-mode",
            "target-x",
            "target-y",
            "target-z",
            "rlogs-base-url",
        ];
        for key in options.keys() {
            if key != "interval-ms" && !ALLOWED.contains(&key.as_str()) {
                return Err("unknown option".into());
            }
        }
        if required(options, "build")? != BUILD {
            return Err(format!("this probe supports exact build {BUILD} only").into());
        }
        if options.get("armed-mode").is_some_and(|value| {
            value != ARMED_MODE_TOKEN
                && value != PLANNER_ARMED_MODE_TOKEN
                && value != CLOSED_LOOP_ARMED_MODE_TOKEN
        }) {
            return Err("unknown armed-mode token".into());
        }
        let planner_options = ["target-x", "target-y", "target-z", "rlogs-base-url"];
        if matches!(
            options.get("armed-mode").map(String::as_str),
            Some(PLANNER_ARMED_MODE_TOKEN | CLOSED_LOOP_ARMED_MODE_TOKEN)
        ) {
            if planner_options
                .iter()
                .any(|key| !options.contains_key(*key))
            {
                return Err("planner mode requires target XYZ and rlogs-base-url".into());
            }
        } else if planner_options.iter().any(|key| options.contains_key(*key)) {
            return Err("planner-only options require the exact planner armed mode".into());
        }
        Ok(())
    }

    fn float_option(options: &BTreeMap<String, String>, key: &str) -> Result<f32, Box<dyn Error>> {
        let value: f32 = required(options, key)?.parse()?;
        if !value.is_finite() {
            return Err(format!("--{key} must be finite").into());
        }
        Ok(value)
    }

    fn required<'a>(
        options: &'a BTreeMap<String, String>,
        key: &str,
    ) -> Result<&'a str, Box<dyn Error>> {
        options
            .get(key)
            .map(String::as_str)
            .ok_or_else(|| "missing required option".into())
    }

    fn numeric_option(
        options: &BTreeMap<String, String>,
        key: &str,
        default: u64,
        minimum: u64,
        maximum: u64,
    ) -> Result<u64, Box<dyn Error>> {
        let value = options
            .get(key)
            .map_or(Ok(default), |value| value.parse())?;
        if !(minimum..=maximum).contains(&value) {
            return Err(format!("--{key} must be in {minimum}..={maximum}").into());
        }
        Ok(value)
    }

    #[cfg(test)]
    mod tests {
        use super::*;
        use std::{cell::Cell, collections::BTreeMap as Map};

        struct FakeMemory {
            bytes: Map<usize, u8>,
            mutate_root_after: Cell<Option<usize>>,
            root_reads: Cell<usize>,
            mutate_state: Cell<bool>,
            state_reads: Cell<usize>,
        }

        impl FakeMemory {
            fn new() -> Self {
                Self {
                    bytes: Map::new(),
                    mutate_root_after: Cell::new(None),
                    root_reads: Cell::new(0),
                    mutate_state: Cell::new(false),
                    state_reads: Cell::new(0),
                }
            }
            fn put(&mut self, address: usize, bytes: &[u8]) {
                for (index, byte) in bytes.iter().enumerate() {
                    self.bytes.insert(address + index, *byte);
                }
            }
            fn ptr(&mut self, address: usize, value: usize) {
                self.put(address, &value.to_le_bytes());
            }
            fn text(&mut self, address: usize, value: &str) {
                let mut bytes = vec![0u8; MAX_C_STRING_BYTES];
                bytes[..value.len()].copy_from_slice(value.as_bytes());
                self.put(address, &bytes);
            }
        }

        impl Memory for FakeMemory {
            fn read_exact(&self, address: usize, length: usize) -> Result<Vec<u8>, AcquireError> {
                let root_slot = 0x10_0000 + ENTITY_MGR_SINGLETON_METHOD_INFO_RVA;
                if address == root_slot {
                    let count = self.root_reads.get() + 1;
                    self.root_reads.set(count);
                    if self
                        .mutate_root_after
                        .get()
                        .is_some_and(|limit| count > limit)
                    {
                        return Ok(0x22_0000usize.to_le_bytes().to_vec());
                    }
                }
                if address == 0x90_0000 + SKILL_SLOT_ID && self.mutate_state.get() {
                    let count = self.state_reads.get() + 1;
                    self.state_reads.set(count);
                    if count > 1 {
                        return Ok(8i32.to_le_bytes().to_vec());
                    }
                }
                (0..length)
                    .map(|i| {
                        self.bytes
                            .get(&(address + i))
                            .copied()
                            .ok_or(AcquireError::Read)
                    })
                    .collect()
            }
        }

        fn valid_memory() -> FakeMemory {
            let mut m = FakeMemory::new();
            let base = 0x10_0000;
            let method = 0x20_0000;
            let declaring_class = 0x30_0000;
            let singleton_class = 0x30_1000;
            let generic_context = 0x30_2000;
            let static_fields = 0x40_0000;
            let entity_mgr = 0x50_0000;
            let player = 0x60_0000;
            let storage = 0x70_0000;
            let comp = 0x80_0000;
            let mgr = 0x90_0000;
            let indicator = 0x91_0000;
            let classes = [0x31_0000, 0x32_0000, 0x33_0000, 0x34_0000];
            let names = [
                "ZEntityMgr",
                "PlayerEnt",
                "PlayerEnt__Storage",
                "PlayerSkillInputComp",
            ];
            m.ptr(base + ENTITY_MGR_SINGLETON_METHOD_INFO_RVA, method);
            m.ptr(base + ENTITY_MGR_SINGLETON_TYPE_INFO_RVA, singleton_class);
            m.ptr(method + METHOD_INFO_NAME, 0xA0_0203);
            m.ptr(method + METHOD_INFO_KLASS, declaring_class);
            m.ptr(declaring_class + IL2CPP_CLASS_NAME, 0xA0_0003);
            m.ptr(declaring_class + IL2CPP_CLASS_NAMESPACE, 0xA0_0105);
            m.ptr(
                declaring_class + IL2CPP_CLASS_GENERIC_CONTEXT,
                generic_context,
            );
            m.ptr(
                generic_context + GENERIC_CONTEXT_INFLATED_CLASS,
                singleton_class,
            );
            m.ptr(singleton_class + IL2CPP_CLASS_NAME, 0xA0_0303);
            m.ptr(singleton_class + IL2CPP_CLASS_NAMESPACE, 0xA0_0405);
            m.ptr(singleton_class + IL2CPP_CLASS_STATIC_FIELDS, static_fields);
            m.text(0xA0_0003, "ZSingleton`1");
            m.text(0xA0_0105, "ZUtil");
            m.text(0xA0_0203, "get_Instance");
            m.text(0xA0_0303, "ZSingleton`1");
            m.text(0xA0_0405, "ZUtil");
            m.ptr(static_fields, entity_mgr);
            m.ptr(entity_mgr, classes[0]);
            m.ptr(entity_mgr + ENTITY_MGR_PLAYER_ENT, player);
            m.ptr(player, classes[1]);
            m.ptr(player + PLAYER_ENT_PURE_COMPONENTS, storage);
            m.ptr(player + PLAYER_ENT_SKILL_INPUT_COMP, comp);
            m.ptr(storage, classes[2]);
            m.ptr(comp, classes[3]);
            m.ptr(comp + SKILL_INPUT_COMP_MGR, mgr);
            for (i, class) in classes.iter().enumerate() {
                m.ptr(*class + IL2CPP_CLASS_NAME, 0xA1_0000 + i * 0x100);
                m.ptr(*class + IL2CPP_CLASS_NAMESPACE, 0xA2_0000 + i * 0x100);
                m.text(0xA1_0000 + i * 0x100, names[i]);
                m.text(0xA2_0000 + i * 0x100, "Panda.ZGame");
            }
            let mgr_class = 0x35_0000;
            m.ptr(mgr, mgr_class);
            m.ptr(mgr_class + IL2CPP_CLASS_NAME, 0xA1_0400);
            m.ptr(mgr_class + IL2CPP_CLASS_NAMESPACE, 0xA2_0400);
            m.text(0xA1_0400, "ZSkillInputMgr");
            m.text(0xA2_0400, "Panda.ZGame");
            m.put(mgr + SKILL_SLOT_ID, &7i32.to_le_bytes());
            m.put(mgr + SKILL_LAST_USED_SLOT_ID, &6i32.to_le_bytes());
            m.put(mgr + SKILL_LAST_PRESS_SLOT_ID, &7i32.to_le_bytes());
            m.put(mgr + SKILL_IS_PRESS, &[1]);
            let mut xyz = Vec::new();
            for v in [1.25f32, -2.5, 3.75] {
                xyz.extend_from_slice(&v.to_le_bytes());
            }
            m.put(storage + INDICATOR_POS, &xyz);
            let indicator_method = 0x21_0000;
            let indicator_declaring_class = 0x36_0000;
            let indicator_singleton_class = 0x36_1000;
            let indicator_generic_context = 0x36_2000;
            let indicator_static_fields = 0x41_0000;
            let indicator_class = 0x37_0000;
            m.ptr(
                base + INDICATOR_MGR_SINGLETON_METHOD_INFO_RVA,
                indicator_method,
            );
            m.ptr(
                base + INDICATOR_MGR_SINGLETON_TYPE_INFO_RVA,
                indicator_singleton_class,
            );
            m.ptr(indicator_method + METHOD_INFO_NAME, 0xA3_0000);
            m.ptr(
                indicator_method + METHOD_INFO_KLASS,
                indicator_declaring_class,
            );
            m.text(0xA3_0000, "get_Instance");
            m.ptr(indicator_declaring_class + IL2CPP_CLASS_NAME, 0xA3_0100);
            m.ptr(
                indicator_declaring_class + IL2CPP_CLASS_NAMESPACE,
                0xA3_0200,
            );
            m.text(0xA3_0100, "ZSingleton`1");
            m.text(0xA3_0200, "ZUtil");
            m.ptr(
                indicator_declaring_class + IL2CPP_CLASS_GENERIC_CONTEXT,
                indicator_generic_context,
            );
            m.ptr(
                indicator_generic_context + GENERIC_CONTEXT_INFLATED_CLASS,
                indicator_singleton_class,
            );
            m.ptr(indicator_singleton_class + IL2CPP_CLASS_NAME, 0xA3_0300);
            m.ptr(
                indicator_singleton_class + IL2CPP_CLASS_NAMESPACE,
                0xA3_0400,
            );
            m.text(0xA3_0300, "ZSingleton`1");
            m.text(0xA3_0400, "ZUtil");
            m.ptr(
                indicator_singleton_class + IL2CPP_CLASS_STATIC_FIELDS,
                indicator_static_fields,
            );
            m.ptr(indicator_static_fields, indicator);
            m.ptr(indicator, indicator_class);
            m.ptr(indicator_class + IL2CPP_CLASS_NAME, 0xA3_0500);
            m.ptr(indicator_class + IL2CPP_CLASS_NAMESPACE, 0xA3_0600);
            m.text(0xA3_0500, "ZIndicatorMgr");
            m.text(0xA3_0600, "Panda.ZGame");
            m.put(indicator + INDICATOR_IS_ENABLE, &[1]);
            m.put(indicator + INDICATOR_IS_PC_UP_RELEASE, &[0]);
            m.put(indicator + INDICATOR_IS_CAN_RELEASE, &[1]);
            m.put(indicator + INDICATOR_DATA_TYPE, &[1]);
            m.put(
                indicator + INDICATOR_DATA_SKILL_ID,
                &MARKER_1_SKILL_ID.to_le_bytes(),
            );
            m.put(
                indicator + INDICATOR_DATA_SLOT_ID,
                &MARKER_1_SLOT_ID.to_le_bytes(),
            );
            m.put(
                indicator + INDICATOR_DATA_PARAM_1,
                &MARKER_PARAM.to_le_bytes(),
            );
            m.put(
                indicator + INDICATOR_DATA_PARAM_2,
                &MARKER_PARAM.to_le_bytes(),
            );
            m.put(
                indicator + INDICATOR_DATA_MAX_DISTANCE,
                &MARKER_MAX_DISTANCE.to_le_bytes(),
            );
            m.put(indicator + INDICATOR_MGR_POS, &xyz);
            m.put(indicator + INDICATOR_IS_PC_MODE, &[1]);
            m.put(indicator + INDICATOR_CURRENT_VELOCITY, &[0u8; 12]);
            m.put(
                indicator + INDICATOR_SKILL_ID,
                &MARKER_1_SKILL_ID.to_le_bytes(),
            );
            m.put(
                indicator + INDICATOR_SLOT_ID,
                &MARKER_1_SLOT_ID.to_le_bytes(),
            );
            m.put(indicator + INDICATOR_ENTER_STATE, &3i32.to_le_bytes());
            m.put(indicator + INDICATOR_CAMERA_OPEN_ID, &7i32.to_le_bytes());
            m
        }

        #[test]
        fn accepts_only_the_reviewed_identity_chain_and_fields() {
            let state = coherent_sample(&valid_memory(), 0x10_0000).unwrap();
            assert_eq!(state.slot_id, 7);
            assert!(state.is_press);
            assert_eq!(
                state.indicator_pos,
                Position {
                    x: 1.25,
                    y: -2.5,
                    z: 3.75
                }
            );
            assert!(PreflightFieldGates::from_state(&state).all_passed());
        }

        #[test]
        fn rejects_a_wrong_class_identity() {
            let mut memory = valid_memory();
            memory.text(0xA1_0300, "WrongComponent");
            assert_eq!(
                coherent_sample(&memory, 0x10_0000),
                Err(AcquireError::Identity)
            );
        }

        #[test]
        fn rejects_type_info_that_disagrees_with_the_inflated_generic_class() {
            let mut memory = valid_memory();
            memory.ptr(0x10_0000 + ENTITY_MGR_SINGLETON_TYPE_INFO_RVA, 0x30_0000);
            assert_eq!(
                coherent_sample(&memory, 0x10_0000),
                Err(AcquireError::Identity)
            );
        }

        #[test]
        fn double_read_rejects_a_root_change() {
            let memory = valid_memory();
            memory.mutate_root_after.set(Some(1));
            assert!(coherent_sample(&memory, 0x10_0000).is_err());
        }

        #[test]
        fn double_read_rejects_a_mixed_lifecycle_state() {
            let memory = valid_memory();
            memory.mutate_state.set(true);
            assert_eq!(coherent_sample(&memory, 0x10_0000), Err(AcquireError::Torn));
        }

        #[test]
        fn manifest_gate_is_exact() {
            assert!(
                require_manifest_identity("\"appid\" \"3681810\"\n\"buildid\" \"25247556\"")
                    .is_ok()
            );
            assert!(
                require_manifest_identity("\"appid\" \"3681810\"\n\"buildid\" \"other\"").is_err()
            );
        }

        #[test]
        fn armed_mode_requires_the_exact_literal_token() {
            let mut options = Map::new();
            options.insert("build".into(), BUILD.into());
            options.insert("armed-mode".into(), "yes".into());
            assert!(reject_unknown_options(&options).is_err());
            options.insert("armed-mode".into(), ARMED_MODE_TOKEN.into());
            assert!(reject_unknown_options(&options).is_ok());
            options.insert("armed-mode".into(), PLANNER_ARMED_MODE_TOKEN.into());
            options.insert("target-x".into(), "1".into());
            options.insert("target-y".into(), "2".into());
            options.insert("target-z".into(), "3".into());
            options.insert("rlogs-base-url".into(), "http://127.0.0.1:1".into());
            assert!(reject_unknown_options(&options).is_ok());
            options.insert("armed-mode".into(), CLOSED_LOOP_ARMED_MODE_TOKEN.into());
            assert!(reject_unknown_options(&options).is_ok());
        }

        #[test]
        fn planner_step_requires_strict_improvement_ratio_and_two_mm_return() {
            assert!(planner_step_passes(1.0, 0.5, 0.8, Some(0.002)));
            assert!(!planner_step_passes(1.0, 0.5, 1.0, Some(0.0)));
            assert!(!planner_step_passes(1.0, 0.5, 0.91, Some(0.0)));
            assert!(!planner_step_passes(1.0, 0.5, 0.8, Some(0.0021)));
            assert!(!planner_step_passes(1.0, 1.1, 0.8, Some(0.0)));
        }

        #[test]
        fn post_move_observation_failure_does_not_bypass_exact_inverse_selection() {
            let post_move_observation_succeeded = false;
            assert!(!post_move_observation_succeeded);
            assert_eq!(
                next_planner_rollback_action([-3, 1], true, true),
                PlannerRollbackAction::Inverse([-3, 1])
            );
            assert_eq!(
                next_planner_rollback_action([-3, 1], true, false),
                PlannerRollbackAction::Escape
            );
            assert_eq!(
                next_planner_rollback_action([-3, 1], false, true),
                PlannerRollbackAction::CannotActWithoutForeground
            );
        }

        #[test]
        fn closed_loop_rollback_is_exact_reverse_order_and_bounded() {
            let ledger = [[4, 0], [0, -4], [2, 1], [-1, 2]];
            assert_eq!(
                reverse_rollback_deltas(&ledger),
                vec![[1, -2], [-2, -1], [0, 4], [-4, 0]]
            );
            assert_eq!(ledger.len(), CLOSED_LOOP_MAX_MOVES);
            let cumulative = ledger
                .iter()
                .map(|delta| (f64::from(delta[0]).powi(2) + f64::from(delta[1]).powi(2)).sqrt())
                .sum::<f64>();
            assert!(cumulative <= CLOSED_LOOP_MAX_CUMULATIVE_PIXELS);
            assert!(
                ledger
                    .iter()
                    .all(|delta| delta.iter().all(|axis| axis.abs() <= 4))
            );
        }

        #[test]
        fn input_ownership_requires_one_tagged_move_and_no_foreign_move() {
            let before = MouseObservationCounts { own: 4, foreign: 0 };
            assert!(input_ownership_verified(
                before,
                MouseObservationCounts { own: 5, foreign: 0 }
            ));
            assert!(!input_ownership_verified(
                before,
                MouseObservationCounts { own: 4, foreign: 0 }
            ));
            assert!(!input_ownership_verified(
                before,
                MouseObservationCounts { own: 6, foreign: 0 }
            ));
            assert!(!input_ownership_verified(
                before,
                MouseObservationCounts { own: 5, foreign: 1 }
            ));
        }

        #[test]
        fn loopback_map_contract_requires_current_non_stale_local_position_and_family() {
            use std::net::TcpListener;
            let listener = TcpListener::bind("127.0.0.1:0").unwrap();
            let port = listener.local_addr().unwrap().port();
            let server = thread::spawn(move || {
                for request_index in 0..3 {
                    let (mut stream, _) = listener.accept().unwrap();
                    let mut request = Vec::new();
                    loop {
                        let mut chunk = [0u8; 1024];
                        let read = stream.read(&mut chunk).unwrap();
                        request.extend_from_slice(&chunk[..read]);
                        let complete = request
                            .windows(4)
                            .position(|window| window == b"\r\n\r\n")
                            .is_some_and(|header_end| {
                                let header = String::from_utf8_lossy(&request[..header_end]);
                                let content_length = header.lines().find_map(|line| {
                                    line.to_ascii_lowercase()
                                        .strip_prefix("content-length:")
                                        .and_then(|value| value.trim().parse::<usize>().ok())
                                });
                                content_length
                                    .is_some_and(|length| request.len() >= header_end + 4 + length)
                            });
                        if complete || read == 0 {
                            break;
                        }
                    }
                    let body = match request_index {
                        0 => {
                            r#"{"revision":1,"snapshot":{"last_observed_micros":1,"client_build":"25247556","session_id":"s","scene_id":1633,"map_id":1633,"local_position_observed":true,"local_actor_id":7,"entities":[{"actor_id":7,"x":1.0,"y":2.0,"z":3.0,"stale":false}]}}"#
                        }
                        1 => {
                            r#"{"revision":2,"snapshot":{"last_observed_micros":2,"client_build":"25247556","session_id":"s","scene_id":1633,"map_id":1633,"local_position_observed":true,"local_actor_id":7,"entities":[{"actor_id":7,"x":1.0,"y":2.0,"z":3.0,"stale":false}]}}"#
                        }
                        _ => {
                            r#"{"context":{"clientBuild":"25247556","sceneId":1633,"mapId":1633,"activityFamilyId":"dungeon.1633"}}"#
                        }
                    };
                    write!(
                        stream,
                        "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                        body.len(),
                        body
                    )
                    .unwrap();
                }
            });
            let value = read_live_player_context(&format!("http://127.0.0.1:{port}")).unwrap();
            server.join().unwrap();
            assert_eq!(value.identity.client_build, BUILD);
            assert_eq!(value.identity.activity_family_id, "dungeon.1633");
            assert_eq!(value.identity.local_actor_id, 7);
            assert_eq!(
                value.origin,
                Position {
                    x: 1.0,
                    y: 2.0,
                    z: 3.0
                }
            );
            assert!(read_live_player_context("http://localhost:1").is_err());
        }

        #[test]
        fn live_context_rejects_local_actor_rotation() {
            let expected = LivePlayerContext {
                identity: LiveMapIdentity {
                    session_id: "session".to_owned(),
                    client_build: BUILD.to_owned(),
                    scene_id: 1633,
                    map_id: 1633,
                    local_actor_id: 7,
                    activity_family_id: "dungeon.1633".to_owned(),
                },
                origin: Position {
                    x: 1.0,
                    y: 2.0,
                    z: 3.0,
                },
            };
            let mut rotated = expected.clone();
            rotated.identity.local_actor_id = 8;
            assert!(!live_context_matches(
                &expected,
                &rotated,
                &Position {
                    x: 2.0,
                    y: 2.0,
                    z: 3.0,
                }
            ));
        }

        #[test]
        fn loopback_http_rejects_chunked_and_trailing_response_bytes() {
            use std::net::TcpListener;

            fn serve_once(response: &'static [u8]) -> u16 {
                let listener = TcpListener::bind("127.0.0.1:0").unwrap();
                let port = listener.local_addr().unwrap().port();
                thread::spawn(move || {
                    let (mut stream, _) = listener.accept().unwrap();
                    let mut request = [0u8; 1024];
                    let _ = stream.read(&mut request).unwrap();
                    stream.write_all(response).unwrap();
                });
                port
            }

            let chunked_port = serve_once(
                b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\nConnection: close\r\n\r\n2\r\n{}\r\n0\r\n\r\n",
            );
            assert_eq!(
                local_json_get(&chunked_port.to_string(), "/test"),
                Err("unsupported-rlogs-transfer-encoding")
            );

            let trailing_port =
                serve_once(b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\nConnection: close\r\n\r\n{}x");
            assert_eq!(
                local_json_get(&trailing_port.to_string(), "/test"),
                Err("rlogs-content-length-mismatch")
            );
        }

        #[test]
        fn calibration_sequence_is_symmetric_ordered_and_rank_two() {
            assert_eq!(CALIBRATION_SEQUENCE, [[6, 0], [-6, 0], [0, 6], [0, -6]]);
            assert!(calibration_sequence_is_rank_2());
            assert_eq!(
                CALIBRATION_SEQUENCE
                    .into_iter()
                    .fold([0, 0], |sum, delta| [sum[0] + delta[0], sum[1] + delta[1]]),
                [0, 0]
            );
        }

        #[test]
        fn calibration_actions_fail_closed_on_focus_or_context_loss() {
            assert_eq!(
                next_calibration_action(0, true, true),
                CalibrationAction::Move([6, 0])
            );
            assert_eq!(
                next_calibration_action(1, true, true),
                CalibrationAction::Move([-6, 0])
            );
            assert_eq!(
                next_calibration_action(2, true, true),
                CalibrationAction::Move([0, 6])
            );
            assert_eq!(
                next_calibration_action(3, true, true),
                CalibrationAction::Move([0, -6])
            );
            assert_eq!(
                next_calibration_action(4, true, true),
                CalibrationAction::Escape
            );
            assert_eq!(
                next_calibration_action(1, false, true),
                CalibrationAction::AbortForeground
            );
            assert_eq!(
                next_calibration_action(1, true, false),
                CalibrationAction::AbortContext
            );

            let baseline = coherent_sample(&valid_memory(), 0x10_0000).unwrap();
            let mut changed = baseline.clone();
            changed.indicator.camera_open_id += 1;
            assert_ne!(
                MarkerContext::from(&baseline),
                MarkerContext::from(&changed)
            );
        }

        #[test]
        fn calibration_receipt_pairs_exact_integer_delta_with_settled_observation() {
            let observation = SettledObservation {
                elapsed_micros: 200,
                position: Position {
                    x: 1.0,
                    y: 2.0,
                    z: 3.0,
                },
                current_velocity: Position {
                    x: 0.0,
                    y: 0.0,
                    z: 0.0,
                },
                stability_sample_gap_millis: STABILITY_SAMPLE_MILLIS,
                stability_position_delta: 0.0,
                velocity_norm: 0.0,
                settled: true,
            };
            let transition = CalibrationTransition {
                sequence_index: 2,
                emitted_integer_mouse_delta: [0, 6],
                input_elapsed_micros: 100,
                subsequent_observation: observation,
                displacement: 1.0,
            };
            let json = serde_json::to_string(&transition).unwrap();
            assert!(json.contains("\"emitted_integer_mouse_delta\":[0,6]"));
            assert!(json.contains("\"input_elapsed_micros\":100"));
            assert!(json.contains("\"elapsed_micros\":200"));
            assert!(json.contains("\"settled\":true"));
            assert!(!json.contains("pid"));
            assert!(!json.contains("address"));
            assert!(!json.contains("path"));
            let read_only_json = serde_json::to_string(&read_only_canary_receipt()).unwrap();
            assert!(!read_only_json.contains("pid"));
            assert!(!read_only_json.contains("address"));
            assert!(!read_only_json.contains("path"));
        }

        #[test]
        fn calibration_action_surface_has_no_click_or_confirmation_variant() {
            for completed in 0..CALIBRATION_SEQUENCE.len() {
                assert!(matches!(
                    next_calibration_action(completed, true, true),
                    CalibrationAction::Move(_)
                ));
            }
            assert_eq!(
                next_calibration_action(CALIBRATION_SEQUENCE.len(), true, true),
                CalibrationAction::Escape
            );
            let policy = read_only_canary_receipt();
            assert!(!policy.armed);
            assert!(policy.transitions.is_empty());
        }

        #[test]
        fn marker_gate_rejects_any_non_marker_one_state() {
            let mut state = coherent_sample(&valid_memory(), 0x10_0000).unwrap();
            state.indicator.slot_id = 202;
            assert!(!PreflightFieldGates::from_state(&state).all_passed());
            state.indicator.slot_id = MARKER_1_SLOT_ID;
            state.indicator.is_can_release = false;
            assert!(!PreflightFieldGates::from_state(&state).all_passed());
        }

        #[test]
        fn preflight_diagnostic_reports_each_safe_gate_without_input_authority() {
            let memory = valid_memory();
            let (diagnostic, sample) =
                diagnose_marker_1_preflight(&memory, 0x10_0000, &Instant::now());
            assert!(sample.is_some());
            assert!(diagnostic.class_context_validated);
            assert!(diagnostic.coherent_samples_acquired);
            assert_eq!(diagnostic.root_context_unchanged, Some(true));
            assert!(diagnostic.passed);
            let observed = diagnostic.observed.as_ref().unwrap();
            assert_eq!(observed.indicator_type, 1);
            assert_eq!(observed.data_skill_id, MARKER_1_SKILL_ID);
            assert_eq!(observed.data_slot_id, MARKER_1_SLOT_ID);
            assert_eq!(observed.max_distance, MARKER_MAX_DISTANCE);
            assert_eq!(
                observed.current_velocity,
                Position {
                    x: 0.0,
                    y: 0.0,
                    z: 0.0
                }
            );
            assert!(diagnostic.gates.as_ref().unwrap().all_passed());
            assert!(diagnostic.stability.as_ref().unwrap().velocity_settled);
        }

        #[test]
        fn preflight_mismatch_is_diagnostic_and_returns_no_sample() {
            let mut memory = valid_memory();
            memory.put(0x91_0000 + INDICATOR_DATA_TYPE, &[0]);
            memory.put(
                0x91_0000 + INDICATOR_DATA_PARAM_2,
                &MARKER_MAX_DISTANCE.to_le_bytes(),
            );
            let (diagnostic, sample) =
                diagnose_marker_1_preflight(&memory, 0x10_0000, &Instant::now());
            assert!(sample.is_none());
            assert!(!diagnostic.passed);
            assert_eq!(diagnostic.observed.as_ref().unwrap().indicator_type, 0);
            assert_eq!(
                diagnostic.observed.as_ref().unwrap().param_2,
                MARKER_MAX_DISTANCE
            );
            assert!(!diagnostic.gates.as_ref().unwrap().indicator_type_point);
            assert!(!diagnostic.gates.as_ref().unwrap().param_2_expected);
        }

        #[test]
        fn preflight_velocity_rejection_is_sanitized() {
            let mut memory = valid_memory();
            memory.put(
                0x91_0000 + INDICATOR_CURRENT_VELOCITY,
                &0.5f32.to_le_bytes(),
            );
            let (diagnostic, sample) =
                diagnose_marker_1_preflight(&memory, 0x10_0000, &Instant::now());
            assert!(sample.is_none());
            assert!(!diagnostic.stability.as_ref().unwrap().velocity_settled);
            let json = serde_json::to_string(&diagnostic).unwrap();
            for forbidden in ["pid", "pointer", "address", "path", "account", "session"] {
                assert!(!json.contains(forbidden));
            }
        }

        #[test]
        fn preflight_reports_class_context_rejection_without_scalar_leakage() {
            let mut memory = valid_memory();
            memory.text(0xA3_0500, "WrongIndicatorManager");
            let (diagnostic, sample) =
                diagnose_marker_1_preflight(&memory, 0x10_0000, &Instant::now());
            assert!(sample.is_none());
            assert!(!diagnostic.class_context_validated);
            assert_eq!(diagnostic.acquisition_status, "identity-rejected");
            assert!(diagnostic.observed.is_none());
            assert!(diagnostic.gates.is_none());
        }

        #[test]
        fn position_distance_uses_three_dimensions() {
            let origin = Position {
                x: 0.0,
                y: 0.0,
                z: 0.0,
            };
            let moved = Position {
                x: 0.03,
                y: 0.04,
                z: 0.0,
            };
            assert!(approx(position_distance(&origin, &moved), 0.05, 0.0001));
        }

        #[test]
        fn receipt_policy_serialization_contains_no_pointer_pid_or_path_fields() {
            let policy = Policy {
                exact_build_and_hashes_required: true,
                process_rights: ["PROCESS_QUERY_INFORMATION", "PROCESS_VM_READ"],
                allowlisted_pointer_chain_only: true,
                allowlisted_fields_only: true,
                heap_or_process_scan_performed: false,
                process_identifiers_emitted: false,
                raw_addresses_emitted: false,
                filesystem_paths_emitted: false,
                debugger_attached: false,
                threads_suspended: false,
                code_injected_or_invoked: false,
                process_memory_written: false,
                remote_process_write_rights_requested: false,
                remote_memory_allocated: false,
                remote_thread_created: false,
                dll_injected: false,
                internal_game_function_invoked: false,
                packets_observed_or_modified: false,
                packet_synthesis_performed: false,
                ordinary_foreground_input_only: true,
                place_enabled: false,
                mouse_click_emitted: false,
                reversible_mouse_move_enabled: false,
                escape_cancel_enabled: false,
            };
            let json = serde_json::to_string(&policy).unwrap();
            assert!(!json.contains("pid"));
            assert!(!json.contains("address_hex"));
            assert!(!json.contains("path\""));
            assert!(json.contains("\"place_enabled\":false"));
            assert!(json.contains("\"remote_process_write_rights_requested\":false"));
            assert!(json.contains("\"internal_game_function_invoked\":false"));
            assert!(json.contains("\"packet_synthesis_performed\":false"));
            assert!(json.contains("\"ordinary_foreground_input_only\":true"));
        }

        #[test]
        fn source_has_no_remote_write_injection_or_packet_synthesis_primitives() {
            let source = include_str!("automarker-lifecycle-probe.rs");
            let forbidden = [
                ["PROCESS_", "VM_WRITE"].concat(),
                ["PROCESS_", "ALL_ACCESS"].concat(),
                ["Write", "ProcessMemory"].concat(),
                ["Virtual", "AllocEx"].concat(),
                ["Virtual", "ProtectEx"].concat(),
                ["Create", "RemoteThread"].concat(),
                ["Load", "LibraryW"].concat(),
                ["WSA", "Send"].concat(),
                ["send", "to("].concat(),
            ];
            for primitive in forbidden {
                assert!(
                    !source.contains(&primitive),
                    "prohibited primitive introduced into read-only observer"
                );
            }
            assert_eq!(
                PROCESS_READ_RIGHTS,
                PROCESS_QUERY_INFORMATION | PROCESS_VM_READ
            );
        }

        #[test]
        fn option_surface_has_no_runtime_address_or_activation_override() {
            let allowed = [
                "build",
                "process-executable",
                "game-assembly",
                "steam-manifest",
                "output",
                "duration-ms",
                "interval-ms",
                "armed-mode",
            ];
            assert!(
                !allowed
                    .iter()
                    .any(|key| ["root-rva", "address", "place", "write", "invoke"].contains(key))
            );
        }

        #[test]
        fn pointer_values_are_aligned_but_scalar_read_addresses_need_not_be() {
            assert!(plausible_pointer(0x10_0000));
            assert!(!plausible_pointer(0x10_0004));
            assert!(plausible_address(0x10_0004));
            assert!(plausible_address(0x10_0003));
        }

        #[test]
        fn region_gate_requires_committed_unguarded_complete_range() {
            let mut region = MEMORY_BASIC_INFORMATION {
                BaseAddress: 0x10_0000usize as *mut c_void,
                RegionSize: 0x1000,
                State: MEM_COMMIT,
                Protect: 0x04,
                ..Default::default()
            };
            assert!(region_allows_read(&region, 0x10_001c, 0x10_0020));
            assert!(!region_allows_read(&region, 0x10_0ffc, 0x10_1004));
            region.Protect = PAGE_GUARD;
            assert!(!region_allows_read(&region, 0x10_001c, 0x10_0020));
        }
    }
}

#[cfg(windows)]
fn main() -> Result<(), Box<dyn std::error::Error>> {
    windows::main()
}

#[cfg(not(windows))]
fn main() {
    eprintln!("rlogs-bpsr-automarker-lifecycle-probe requires Windows");
    std::process::exit(1);
}
