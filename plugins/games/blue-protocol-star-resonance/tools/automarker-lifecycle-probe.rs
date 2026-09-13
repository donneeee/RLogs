//! Exact-build, out-of-process observer for the normal indicator lifecycle.
//!
//! The observer opens one already-running client with query/read rights only.
//! It follows one compile-time allowlisted IL2CPP object chain and samples six
//! reviewed fields. Its default mode cannot emit input. An exact-token armed
//! canary may emit tiny bounded mouse moves and Escape; it never emits a click.
//! A separately armed mode can observe one operator click and packet-derived
//! request/ack evidence through rLogs. It cannot write/invoke game code, debug,
//! inject, suspend, place markers, inspect packets, or scan/dump process memory.

#[cfg(windows)]
#[allow(dead_code)]
#[path = "../../../../apps/desktop/src/automarker_coordinate_planner.rs"]
mod coordinate_planner;

#[cfg(windows)]
mod windows {
    use std::{
        collections::{BTreeMap, BTreeSet},
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

    use serde::{Deserialize, Serialize};
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
                SetWindowsHookExW, UnhookWindowsHookEx, WH_MOUSE_LL, WM_LBUTTONDOWN,
                WM_MBUTTONDOWN, WM_MOUSEMOVE, WM_QUIT, WM_RBUTTONDOWN, WM_XBUTTONDOWN,
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
    const SET_INDICATOR_POS_RVA: usize = 0x53E_86A0;
    const SET_INDICATOR_POS_BYTES: usize = 0x1D0;
    const SET_INDICATOR_POS_SHA256: &str =
        "a02f30ee1ee8ecf606fceb964a3428b83fa3ab2aac9c4290d85d0c1454af17e6";
    const FIRE_PLAY_SKILL_BY_INDICATOR_RVA: usize = 0x52E_09E0;
    const FIRE_PLAY_SKILL_BY_INDICATOR_BYTES: usize = 0xD0;
    const FIRE_PLAY_SKILL_BY_INDICATOR_SHA256: &str =
        "6ae23a6d1f432969dd2d4c9dd4461f80b7ac6ecf5b5382526dbc00a7b6d8b9f6";
    const UNITASK_POST_RVA: usize = 0x670_EB30;
    const UNITASK_POST_BYTES: usize = 0x10;
    const UNITASK_POST_SHA256: &str =
        "01311db2ce75535a228e3edd96331773c56b27e08392ed317af9ae05366fcb49";
    const PLAYER_LOOP_ADD_CONTINUATION_RVA: usize = 0x670_AFE0;
    const PLAYER_LOOP_ADD_CONTINUATION_BYTES: usize = 0x80;
    const PLAYER_LOOP_ADD_CONTINUATION_SHA256: &str =
        "120e184efe897e07e7ac4f2a10307a1cf115544208ec6f95fd0353822234e0a2";
    const CONTINUATION_QUEUE_ENQUEUE_RVA: usize = 0x676_A0B0;
    const CONTINUATION_QUEUE_ENQUEUE_BYTES: usize = 0x570;
    const CONTINUATION_QUEUE_ENQUEUE_SHA256: &str =
        "25888277bd929d97b136cdb9bcc99b57e523e933e644588b70af27b51d9c52fa";
    const CONTINUATION_QUEUE_RUN_CORE_RVA: usize = 0x676_A630;
    const CONTINUATION_QUEUE_RUN_CORE_BYTES: usize = 0x3C0;
    const CONTINUATION_QUEUE_RUN_CORE_SHA256: &str =
        "5dca8245396e0fa60f0f7f74ad34ca76d511e269c3cbfae71fda909440e9a9cd";

    // Exact build 25247556 only. This is the reviewed MethodInfo global used by
    // ZEntityMgr's singleton acquisition. It is not accepted from the CLI.
    const ENTITY_MGR_SINGLETON_METHOD_INFO_RVA: usize = 0x960_BF90;
    const ENTITY_MGR_SINGLETON_TYPE_INFO_RVA: usize = 0x960_BFF8;
    // Reviewed at three independent native callsites for build 25247556.
    const INDICATOR_MGR_SINGLETON_METHOD_INFO_RVA: usize = 0x95EA430;
    const INDICATOR_MGR_SINGLETON_TYPE_INFO_RVA: usize = 0x95EA438;
    // Recovered exact-build metadata plus StageMgr's generated get_Instance
    // MethodInfo identify this independent, read-only singleton root.
    const STAGE_MGR_SINGLETON_METHOD_INFO_RVA: usize = 0x95D_6B50;
    const STAGE_MGR_SINGLETON_TYPE_INFO_RVA: usize = 0x95D_6B68;
    const METHOD_INFO_NAME: usize = 0x18;
    const METHOD_INFO_KLASS: usize = 0x20;
    const IL2CPP_CLASS_NAME: usize = 0x10;
    const IL2CPP_CLASS_NAMESPACE: usize = 0x18;
    const IL2CPP_CLASS_STATIC_FIELDS: usize = 0xB8;
    const IL2CPP_CLASS_GENERIC_CONTEXT: usize = 0xC0;
    const IL2CPP_CLASS_ELEMENT_SIZE: usize = 0x100;
    const IL2CPP_CLASS_RANK: usize = 0x12E;
    const GENERIC_CONTEXT_INFLATED_CLASS: usize = 0x10;
    const STATIC_SINGLETON_INSTANCE: usize = 0;
    const ENTITY_MGR_PLAYER_ENT: usize = 0x18;
    const PLAYER_ENT_PURE_COMPONENTS: usize = 0x20;
    const PLAYER_ENT_SKILL_INPUT_COMP: usize = 0x128;
    const PLAYER_ENT_ATTR_COLLECTION: usize = 0x48;
    const PLAYER_ENT_CHAR_ID: usize = 0xD8;
    const ATTR_COLLECTION_CACHE_SLIM: usize = 0x18;
    const ATTR_CACHE_INDEX_PART: usize = 0;
    const ATTR_CACHE_VALUES: usize = 0x08;
    const ATTR_INDEX_PART_COUNT: usize = 0;
    const ATTR_INDEX_PART_NEXT: usize = 0x08;
    const ATTR_INDEX_PART_KEY_SEGMENTS: usize = 0x10;
    const ATTR_INDEX_PART_VALUE_INDEX_SEGMENTS: usize = 0x110;
    const ATTR_INDEX_PART_START_SEGMENT_MASKS: usize = 0x210;
    const ATTR_KEY_SEGMENT_COUNT: usize = 0x20;
    const ATTR_KEY_SEGMENT_CAPACITY: usize = 8;
    const ATTR_SEGMENT_COUNT: usize = 32;
    const ATTR_INDEX_PART_CAPACITY: usize = 256;
    const MAX_ATTR_INDEX_PARTS: usize = 16;
    const LOCAL_TEAMMATE_LIST_KEY: u32 = 0x8000_0097;
    const VALUE_TUPLE_UINT_OBJECT_ARRAY_TYPE_INFO_RVA: usize = 0x962_E458;
    const OBJECT_ARRAY_TYPE_INFO_RVA: usize = 0x961_3B10;
    const ZLIST_ATTR_LONG_TYPE_INFO_RVA: usize = 0x955_E130;
    const ZLIST_LONG_TYPE_INFO_RVA: usize = 0x95E_A320;
    const LONG_ARRAY_TYPE_INFO_RVA: usize = 0x963_26E8;
    const PLAYER_LOOP_HELPER_TYPE_INFO_RVA: usize = 0x959_1498;
    const UNITY_SYNCHRONIZATION_CONTEXT_TYPE_INFO_RVA: usize = 0x955_9498;
    const CONTINUATION_QUEUE_TYPE_INFO_RVA: usize = 0x959_13E0;
    const CONTINUATION_QUEUE_ARRAY_TYPE_INFO_RVA: usize = 0x959_13E8;
    const SYSTEM_ACTION_ARRAY_TYPE_INFO_RVA: usize = 0x959_97C8;
    const ZATTR_IS_DEFAULT: usize = 0x10;
    const ZATTR_VALUE: usize = 0x18;
    const ZLIST_ITEMS: usize = 0x18;
    const ZLIST_SIZE: usize = 0x20;
    const VALUE_TUPLE_STRIDE: usize = 0x10;
    const VALUE_TUPLE_MASK: usize = 0;
    const VALUE_TUPLE_OBJECT_ARRAY: usize = 0x08;
    const MAX_REVIEWED_TEAM_MEMBERS: usize = 64;
    const SKILL_INPUT_COMP_DATA_MGR: usize = 0x20;
    const SKILL_INPUT_COMP_MGR: usize = 0x28;
    const SKILL_DATA_MGR_CONTROL_DATAS: usize = 0x18;
    const SKILL_DATA_MGR_SLOT_DICT: usize = 0x28;
    const ZDICTIONARY_BUCKETS_LENGTH: usize = 0x14;
    const ZDICTIONARY_ENTRIES: usize = 0x20;
    const ZDICTIONARY_COUNT: usize = 0x38;
    const ZDICTIONARY_VERSION: usize = 0x3C;
    const ZDICTIONARY_FREE_COUNT: usize = 0x44;
    const MANAGED_ARRAY_LENGTH: usize = 0x18;
    const MANAGED_ARRAY_VECTOR: usize = 0x20;
    const ZDICTIONARY_INT_INT_ENTRY_STRIDE: usize = 0x10;
    const ZDICTIONARY_INT_OBJECT_ENTRY_STRIDE: usize = 0x18;
    const ZDICTIONARY_ENTRY_HASH_CODE: usize = 0;
    const ZDICTIONARY_ENTRY_KEY: usize = 0x08;
    const ZDICTIONARY_INT_INT_ENTRY_VALUE: usize = 0x0C;
    const ZDICTIONARY_INT_OBJECT_ENTRY_VALUE: usize = 0x10;
    const SKILL_CONTROL_DATA_SKILL_ID: usize = 0x10;
    const MAX_REVIEWED_DICTIONARY_CAPACITY: usize = 16_384;
    const MAX_REVIEWED_PLAYER_LOOP_TIMINGS: usize = 64;
    const MAX_REVIEWED_SCHEDULER_QUEUE_CAPACITY: usize = 16_384;
    const PLAYER_LOOP_HELPER_MAIN_THREAD_ID: usize = 0;
    const PLAYER_LOOP_HELPER_SYNCHRONIZATION_CONTEXT: usize = 0x10;
    const PLAYER_LOOP_HELPER_YIELDERS: usize = 0x18;
    const CONTINUATION_QUEUE_TIMING: usize = 0x10;
    const CONTINUATION_QUEUE_DEQUEUING: usize = 0x18;
    const CONTINUATION_QUEUE_ACTION_LIST_COUNT: usize = 0x1C;
    const CONTINUATION_QUEUE_ACTION_LIST: usize = 0x20;
    const CONTINUATION_QUEUE_WAITING_LIST_COUNT: usize = 0x28;
    const CONTINUATION_QUEUE_WAITING_LIST: usize = 0x30;
    const UPDATE_PLAYER_LOOP_TIMING: usize = 8;
    const REVIEWED_CODE_READ_CHUNK: usize = 512;
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
    const STAGE_MGR_SWITCH_STATE: usize = 0x18;
    const STAGE_MGR_CURRENT_STAGE: usize = 0x20;
    const STAGE_BASE_STAGE_TYPE: usize = 0x10;
    const SWITCH_STATE_NONE: u8 = 0;
    const STAGE_TYPE_DUNGEON: u8 = 5;
    const MAX_C_STRING_BYTES: usize = 96;
    const MIN_USER_ADDRESS: usize = 0x1_0000;
    const MAX_USER_ADDRESS: usize = 0x0000_7fff_ffff_ffff;
    const ARMED_MODE_TOKEN: &str = "marker1-reversible-calibration-v1";
    const PLANNER_ARMED_MODE_TOKEN: &str = "marker1-single-planner-step-and-restore-v1";
    const CLOSED_LOOP_ARMED_MODE_TOKEN: &str = "marker1-closed-loop-aim-and-rollback-v1";
    const OPERATOR_PLACEMENT_ARMED_MODE_TOKEN: &str =
        "marker1-operator-click-placement-evidence-v1";
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
    const OPERATOR_CLICK_TIMEOUT_MILLIS: u64 = 8_000;
    const PLACEMENT_COORDINATE_TOLERANCE: f32 = 0.075;
    const PROCESS_READ_RIGHTS: u32 = PROCESS_QUERY_INFORMATION | PROCESS_VM_READ;

    static OBSERVED_OWN_MOUSE_MOVES: AtomicU64 = AtomicU64::new(0);
    static OBSERVED_FOREIGN_MOUSE_MOVES: AtomicU64 = AtomicU64::new(0);
    static OBSERVED_HUMAN_LEFT_CLICKS: AtomicU64 = AtomicU64::new(0);
    static OBSERVED_INJECTED_CLICKS: AtomicU64 = AtomicU64::new(0);
    static OBSERVED_OTHER_CLICKS: AtomicU64 = AtomicU64::new(0);

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
        native_dispatch_preflight: Option<NativeDispatchPreflightReceipt>,
    }

    #[derive(Clone, Debug, Serialize)]
    struct NativeDispatchPreflightReceipt {
        preset_id: String,
        scene_id: Option<i64>,
        map_id: Option<u64>,
        activity_family_id: Option<String>,
        marker_1_target: Option<Position>,
        preset_context_failure_reason: Option<&'static str>,
        exact_image_identity: bool,
        root_chain_class_valid: bool,
        root_chain_stable: bool,
        lifecycle_idle: bool,
        preset_context_current: bool,
        reviewed_code: [ReviewedCodeReceipt; 2],
        scheduler_reviewed_code: [ReviewedCodeReceipt; 4],
        dungeon_stage_gate: BoundedGate,
        leader_gate: BoundedGate,
        marker_skill_resolution_gate: BoundedGate,
        main_thread_scheduler_gate: BoundedGate,
        main_thread_bridge_gate: BoundedGate,
        all_resolvable_gates_passed: bool,
        activation_attempted: bool,
        outcome: &'static str,
    }

    #[derive(Clone, Debug, Serialize)]
    struct ReviewedCodeReceipt {
        identity: &'static str,
        rva: &'static str,
        byte_length: usize,
        sha256: String,
        matches_reviewed_image: bool,
    }

    #[derive(Clone, Copy, Debug)]
    struct ReviewedCodeSpec {
        identity: &'static str,
        rva_text: &'static str,
        rva: usize,
        byte_length: usize,
        expected_sha256: &'static str,
    }

    #[derive(Clone, Debug, Serialize)]
    struct BoundedGate {
        proven: bool,
        reason: &'static str,
    }

    #[derive(Clone, Debug)]
    struct NativeDispatchPreflightRequest {
        preset_id: String,
        rlogs_base_url: String,
    }

    #[derive(Clone, Debug)]
    struct NativeDispatchPresetContext {
        scene_id: i64,
        map_id: u64,
        activity_family_id: String,
        marker_1_target: Position,
    }

    #[derive(Clone, Copy, Debug, Eq, PartialEq)]
    struct DungeonStageSample {
        stage_mgr: usize,
        current_stage: usize,
        switch_state: u8,
        stage_type: u8,
    }

    #[derive(Clone, Copy, Debug, Eq, PartialEq)]
    struct MainThreadSchedulerSample {
        player_loop_helper_class: usize,
        static_fields: usize,
        main_thread_id: i32,
        synchronization_context: usize,
        yielders: usize,
        yielders_length: usize,
        update_queue: usize,
        update_timing: i32,
        dequeuing: bool,
        action_list: usize,
        action_list_length: usize,
        action_list_count: usize,
        waiting_list: usize,
        waiting_list_length: usize,
        waiting_list_count: usize,
    }

    #[derive(Clone, Copy, Debug, Eq, PartialEq)]
    struct PartyLeaderSample {
        player_ent: usize,
        attr_collection: usize,
        index_part: usize,
        values: usize,
        attr: usize,
        teammate_list: usize,
        teammate_items: usize,
        teammate_count: usize,
        current_char_id: i64,
        leader_char_id: i64,
    }

    #[derive(Clone, Copy, Debug, Eq, PartialEq)]
    struct MarkerSkillResolutionSample {
        data_mgr: usize,
        slot_dictionary: usize,
        slot_dictionary_entries: usize,
        slot_dictionary_count: usize,
        slot_dictionary_version: i32,
        control_dictionary: usize,
        control_dictionary_entries: usize,
        control_dictionary_count: usize,
        control_dictionary_version: i32,
        resolved_slot_skill_id: i32,
        resolved_control_data: usize,
        resolved_control_skill_id: i32,
    }

    #[derive(Clone, Copy, Debug, Eq, PartialEq)]
    enum MarkerSkillResolutionError {
        RootChain,
        DataManager,
        SlotDictionaryPointer,
        SlotDictionaryClass,
        SlotDictionaryHeader,
        SlotDictionaryShape,
        SlotDictionaryEntryStorage,
        SlotDictionaryEntryCapacity,
        SlotDictionaryEntryArrayClass,
        SlotDictionaryEntryStride,
        ControlDictionary,
        MarkerSlotLookup,
        MarkerSlotMapping,
        ControlDataLookup,
        ControlDataClass,
        ControlDataSkillIdentity,
    }

    impl MarkerSkillResolutionError {
        fn bounded_reason(self) -> &'static str {
            match self {
                Self::RootChain => "unavailable-or-invalid-read-only-marker-skill-root-chain",
                Self::DataManager => "unavailable-or-invalid-marker-skill-data-manager",
                Self::SlotDictionaryPointer => "marker-skill-slot-dictionary-pointer-invalid",
                Self::SlotDictionaryClass => "marker-skill-slot-dictionary-class-invalid",
                Self::SlotDictionaryHeader => "marker-skill-slot-dictionary-header-unavailable",
                Self::SlotDictionaryShape => {
                    "marker-skill-slot-dictionary-counts-or-buckets-invalid"
                }
                Self::SlotDictionaryEntryStorage => {
                    "marker-skill-slot-dictionary-entry-storage-unavailable"
                }
                Self::SlotDictionaryEntryCapacity => {
                    "marker-skill-slot-dictionary-entry-capacity-invalid"
                }
                Self::SlotDictionaryEntryArrayClass => {
                    "marker-skill-slot-dictionary-entry-array-class-invalid"
                }
                Self::SlotDictionaryEntryStride => {
                    "marker-skill-slot-dictionary-entry-stride-invalid"
                }
                Self::ControlDictionary => "unavailable-or-invalid-marker-skill-control-dictionary",
                Self::MarkerSlotLookup => "marker-1-slot-missing-or-duplicate",
                Self::MarkerSlotMapping => "marker-1-slot-mapping-mismatch",
                Self::ControlDataLookup => "marker-1-control-data-missing-or-duplicate",
                Self::ControlDataClass => "marker-1-control-data-class-invalid",
                Self::ControlDataSkillIdentity => "marker-1-control-data-skill-identity-invalid",
            }
        }
    }

    #[derive(Clone, Copy, Debug, Eq, PartialEq)]
    enum ReviewedDictionaryError {
        Class,
        Header,
        Shape,
        EntryStorage,
        EntryCapacity,
        EntryArrayClass,
        EntryStride,
    }

    #[derive(Clone, Copy, Debug, Eq, PartialEq)]
    struct ReviewedDictionary {
        object: usize,
        entries: usize,
        count: usize,
        version: i32,
    }

    #[derive(Clone, Debug, Serialize)]
    struct PlannerStepReceipt {
        target: Position,
        player_origin: Option<Position>,
        live_context_failure_reason: Option<&'static str>,
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
        deployment_id: String,
        client_build: String,
        protocol_pack_digest: String,
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
        OperatorPlacement,
    }

    impl PlannerCanaryMode {
        fn token(self) -> &'static str {
            match self {
                Self::SingleStep => PLANNER_ARMED_MODE_TOKEN,
                Self::ClosedLoop => CLOSED_LOOP_ARMED_MODE_TOKEN,
                Self::OperatorPlacement => OPERATOR_PLACEMENT_ARMED_MODE_TOKEN,
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
        operator_placement: Option<OperatorPlacementReceipt>,
        outcome: &'static str,
    }

    #[derive(Clone, Debug, Serialize)]
    struct OperatorPlacementReceipt {
        human_click_observed: bool,
        programmatic_click_emitted: bool,
        injected_click_observed: bool,
        other_click_observed: bool,
        outbound_marker_1_newer: bool,
        outbound_observed_micros: Option<u64>,
        inbound_marker_1_newer: bool,
        inbound_observed_micros: Option<u64>,
        inbound_target_distance: Option<f32>,
        context_continuous: bool,
        timed_out: bool,
        escape_emitted: bool,
        outcome: &'static str,
    }

    #[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
    struct MouseObservationCounts {
        own: u64,
        foreign: u64,
        human_left_clicks: u64,
        injected_clicks: u64,
        other_clicks: u64,
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
        } else if code >= 0
            && matches!(
                wparam as u32,
                WM_LBUTTONDOWN | WM_RBUTTONDOWN | WM_MBUTTONDOWN | WM_XBUTTONDOWN
            )
            && lparam != 0
        {
            let event = unsafe { &*(lparam as *const MSLLHOOKSTRUCT) };
            if event.flags & LLMHF_INJECTED != 0 {
                OBSERVED_INJECTED_CLICKS.fetch_add(1, Ordering::SeqCst);
            } else if wparam as u32 == WM_LBUTTONDOWN {
                OBSERVED_HUMAN_LEFT_CLICKS.fetch_add(1, Ordering::SeqCst);
            } else {
                OBSERVED_OTHER_CLICKS.fetch_add(1, Ordering::SeqCst);
            }
        }
        unsafe { CallNextHookEx(std::ptr::null_mut(), code, wparam, lparam) }
    }

    impl MouseInterferenceObserver {
        fn start() -> Result<Self, &'static str> {
            OBSERVED_OWN_MOUSE_MOVES.store(0, Ordering::SeqCst);
            OBSERVED_FOREIGN_MOUSE_MOVES.store(0, Ordering::SeqCst);
            OBSERVED_HUMAN_LEFT_CLICKS.store(0, Ordering::SeqCst);
            OBSERVED_INJECTED_CLICKS.store(0, Ordering::SeqCst);
            OBSERVED_OTHER_CLICKS.store(0, Ordering::SeqCst);
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
                human_left_clicks: OBSERVED_HUMAN_LEFT_CLICKS.load(Ordering::SeqCst),
                injected_clicks: OBSERVED_INJECTED_CLICKS.load(Ordering::SeqCst),
                other_clicks: OBSERVED_OTHER_CLICKS.load(Ordering::SeqCst),
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
        let native_dispatch_preflight = options
            .get("native-dispatch-preflight")
            .is_some_and(|value| value == "true");
        let native_dispatch_request = if native_dispatch_preflight {
            Some(NativeDispatchPreflightRequest {
                preset_id: required(&options, "preset-id")?.to_owned(),
                rlogs_base_url: required(&options, "rlogs-base-url")?.to_owned(),
            })
        } else {
            None
        };
        let planner_mode = match armed_mode {
            Some(PLANNER_ARMED_MODE_TOKEN) => Some(PlannerCanaryMode::SingleStep),
            Some(CLOSED_LOOP_ARMED_MODE_TOKEN) => Some(PlannerCanaryMode::ClosedLoop),
            Some(OPERATOR_PLACEMENT_ARMED_MODE_TOKEN) => Some(PlannerCanaryMode::OperatorPlacement),
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
        let (events, counters, canary) = if let Some(request) = native_dispatch_request.as_ref() {
            (
                Vec::new(),
                Counters::default(),
                run_native_dispatch_preflight(&memory, module.base, module.size, request),
            )
        } else if armed {
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
        let operator_placement_attempted = canary
            .closed_loop
            .as_ref()
            .and_then(|closed| closed.operator_placement.as_ref())
            .is_some_and(|placement| placement.human_click_observed);
        let receipt = Receipt {
            schema_version: 9,
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
                placement_attempted: operator_placement_attempted,
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
            native_dispatch_preflight: None,
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
            native_dispatch_preflight: None,
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
                Err(reason) => {
                    failure.live_context_failure_reason = Some(reason);
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

        let input_observer = if planner_request.is_some_and(|request| {
            matches!(
                request.mode,
                PlannerCanaryMode::ClosedLoop | PlannerCanaryMode::OperatorPlacement
            )
        }) {
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
            matches!(
                request.mode,
                PlannerCanaryMode::ClosedLoop | PlannerCanaryMode::OperatorPlacement
            ) && receipt.transitions.len() != CALIBRATION_SEQUENCE.len()
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
                PlannerCanaryMode::OperatorPlacement => {
                    receipt.closed_loop = Some(run_closed_loop_aim(
                        context,
                        input_observer
                            .as_ref()
                            .expect("operator-placement observer started"),
                    ));
                }
            }
        }

        if planner_request.is_some_and(|request| {
            request.mode == PlannerCanaryMode::OperatorPlacement
                && receipt
                    .closed_loop
                    .as_ref()
                    .and_then(|closed| closed.operator_placement.as_ref())
                    .is_some_and(any_click_observed)
        }) {
            receipt.outcome = receipt
                .closed_loop
                .as_ref()
                .map_or("failed-closed", |closed| closed.outcome);
            return receipt;
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
            Err(reason) => {
                result.live_context_failure_reason = Some(reason);
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

    struct OperatorPlacementRunContext<'a, M: Memory> {
        memory: &'a M,
        module_base: usize,
        process_id: u32,
        started: &'a Instant,
        baseline_roots: &'a Roots,
        baseline_context: MarkerContext,
        request: &'a PlannerCanaryRequest,
        live: &'a LivePlayerContext,
        observer: &'a MouseInterferenceObserver,
    }

    fn any_click_observed(receipt: &OperatorPlacementReceipt) -> bool {
        receipt.human_click_observed
            || receipt.injected_click_observed
            || receipt.other_click_observed
    }

    fn should_escape_after_observed_click(
        receipt: &OperatorPlacementReceipt,
        game_foreground: bool,
    ) -> bool {
        any_click_observed(receipt) && game_foreground
    }

    fn finalize_operator_failure(
        mut receipt: OperatorPlacementReceipt,
        observer: &MouseInterferenceObserver,
        process_id: u32,
    ) -> OperatorPlacementReceipt {
        let counts = observer.counts();
        receipt.human_click_observed |= counts.human_left_clicks != 0;
        receipt.injected_click_observed |= counts.injected_clicks != 0;
        receipt.other_click_observed |= counts.other_clicks != 0;
        if should_escape_after_observed_click(&receipt, game_is_foreground(process_id)) {
            receipt.escape_emitted = emit_escape();
        }
        receipt
    }

    fn run_operator_placement_confirmation<M: Memory>(
        context: OperatorPlacementRunContext<'_, M>,
    ) -> OperatorPlacementReceipt {
        let mut receipt = OperatorPlacementReceipt {
            human_click_observed: false,
            programmatic_click_emitted: false,
            injected_click_observed: false,
            other_click_observed: false,
            outbound_marker_1_newer: false,
            outbound_observed_micros: None,
            inbound_marker_1_newer: false,
            inbound_observed_micros: None,
            inbound_target_distance: None,
            context_continuous: true,
            timed_out: false,
            escape_emitted: false,
            outcome: "preflight-failed",
        };
        let initial_counts = context.observer.counts();
        if initial_counts.human_left_clicks != 0
            || initial_counts.injected_clicks != 0
            || initial_counts.other_clicks != 0
            || initial_counts.foreign != 0
        {
            receipt.outcome = "click-or-input-observed-before-ready";
            return finalize_operator_failure(receipt, context.observer, context.process_id);
        }
        let baseline = match read_observed_marker_evidence(&context.request.rlogs_base_url) {
            Ok(value) if observed_evidence_matches_live(&value, context.live) => value,
            _ => {
                receipt.outcome = "outbound-inbound-evidence-unavailable";
                return finalize_operator_failure(receipt, context.observer, context.process_id);
            }
        };
        let marker_context_current =
            stable_marker_1_sample(context.memory, context.module_base, context.started).is_ok_and(
                |sample| {
                    sample.roots == *context.baseline_roots
                        && MarkerContext::from(&sample.state) == context.baseline_context
                },
            );
        if !game_is_foreground(context.process_id) || !marker_context_current {
            receipt.context_continuous = false;
            receipt.outcome = "context-lost-before-human-click";
            return finalize_operator_failure(receipt, context.observer, context.process_id);
        }

        println!(
            "Marker 1 aim is ready. Click the normal game placement control exactly once; do not move the mouse."
        );
        receipt.outcome = "awaiting-human-click";
        let deadline = Instant::now() + Duration::from_millis(OPERATOR_CLICK_TIMEOUT_MILLIS);
        let mut outbound_micros = None;
        while Instant::now() < deadline {
            if !game_is_foreground(context.process_id) {
                receipt.context_continuous = false;
                receipt.outcome = "foreground-lost-during-human-confirmation";
                break;
            }
            let counts = context.observer.counts();
            receipt.injected_click_observed = counts.injected_clicks != 0;
            receipt.other_click_observed = counts.other_clicks != 0;
            if counts.foreign != initial_counts.foreign
                || counts.own != initial_counts.own
                || counts.injected_clicks != 0
                || counts.other_clicks != 0
                || counts.human_left_clicks > 1
            {
                receipt.outcome = "click-or-input-interference";
                break;
            }
            receipt.human_click_observed = counts.human_left_clicks == 1;
            let live = match read_live_player_context(&context.request.rlogs_base_url) {
                Ok(value)
                    if live_context_matches(context.live, &value, &context.request.target) =>
                {
                    value
                }
                _ => {
                    receipt.context_continuous = false;
                    receipt.outcome = "live-context-lost-during-human-confirmation";
                    break;
                }
            };
            if !receipt.human_click_observed
                && !stable_marker_1_sample(context.memory, context.module_base, context.started)
                    .is_ok_and(|sample| {
                        sample.roots == *context.baseline_roots
                            && MarkerContext::from(&sample.state) == context.baseline_context
                    })
            {
                receipt.context_continuous = false;
                receipt.outcome = "marker-context-lost-before-human-click";
                break;
            }
            let evidence = match read_observed_marker_evidence(&context.request.rlogs_base_url) {
                Ok(value)
                    if observed_evidence_matches_live(&value, &live)
                        && same_observed_identity(&baseline, &value) =>
                {
                    value
                }
                _ => {
                    receipt.context_continuous = false;
                    receipt.outcome = "observed-marker-context-changed";
                    break;
                }
            };
            if receipt.human_click_observed && outbound_micros.is_none() {
                outbound_micros = newer_marker_1_outbound(&baseline, &evidence);
                receipt.outbound_marker_1_newer = outbound_micros.is_some();
                receipt.outbound_observed_micros = outbound_micros;
            }
            if let Some(outbound) = outbound_micros
                && let Some((observed, distance)) =
                    newer_marker_1_inbound(&baseline, &evidence, outbound, &context.request.target)
            {
                thread::sleep(Duration::from_millis(150));
                let final_counts = context.observer.counts();
                if final_counts.human_left_clicks == 1
                    && final_counts.injected_clicks == 0
                    && final_counts.other_clicks == 0
                    && final_counts.foreign == initial_counts.foreign
                    && final_counts.own == initial_counts.own
                    && game_is_foreground(context.process_id)
                {
                    receipt.inbound_marker_1_newer = true;
                    receipt.inbound_observed_micros = Some(observed);
                    receipt.inbound_target_distance = Some(distance);
                    receipt.outcome = "passed";
                    return receipt;
                }
                receipt.outcome = "click-or-input-interference-after-ack";
                break;
            }
            thread::sleep(Duration::from_millis(50));
        }
        receipt.timed_out = Instant::now() >= deadline;
        if receipt.timed_out {
            receipt.outcome = if receipt.human_click_observed {
                "timed-out-awaiting-outbound-inbound-evidence"
            } else {
                "timed-out-awaiting-human-click"
            };
        }
        finalize_operator_failure(receipt, context.observer, context.process_id)
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
                        if request.mode == PlannerCanaryMode::OperatorPlacement {
                            let confirmation =
                                run_operator_placement_confirmation(OperatorPlacementRunContext {
                                    memory,
                                    module_base,
                                    process_id,
                                    started,
                                    baseline_roots,
                                    baseline_context,
                                    request,
                                    live: &live,
                                    observer,
                                });
                            result.outcome = confirmation.outcome;
                            result.operator_placement = Some(confirmation);
                        } else {
                            result.outcome = "arrived-awaiting-rollback";
                        }
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
        if let Some(confirmation) = result.operator_placement.as_ref()
            && any_click_observed(confirmation)
        {
            // Once a human click may have committed the marker, replaying the
            // aim deltas could move the gameplay camera rather than a reticle.
            // Never synthesize that unsafe rollback.
            return result;
        }
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
            operator_placement: None,
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
            live_context_failure_reason: None,
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
        after.own == before.own.saturating_add(1)
            && after.foreign == before.foreign
            && after.human_left_clicks == before.human_left_clicks
            && after.injected_clicks == before.injected_clicks
            && after.other_clicks == before.other_clicks
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

    #[derive(Clone, Debug, Deserialize)]
    #[serde(rename_all = "camelCase", deny_unknown_fields)]
    struct ObservedRequestEvidence {
        marker_number: u8,
        observed_micros: u64,
    }

    #[derive(Clone, Debug, Deserialize)]
    #[serde(rename_all = "camelCase", deny_unknown_fields)]
    struct ObservedPointEvidence {
        marker_number: u8,
        x: f32,
        y: f32,
        z: f32,
        observed_micros: u64,
    }

    #[derive(Clone, Debug, Deserialize)]
    #[serde(rename_all = "camelCase", deny_unknown_fields)]
    struct ObservedMarkerEvidence {
        schema_version: u16,
        revision: u64,
        capture_active: bool,
        protocol_supported: bool,
        request_observer_supported: bool,
        verified_request_count: u32,
        last_verified_request_marker_number: Option<u8>,
        last_verified_request_observed_micros: Option<u64>,
        verified_requests: Vec<ObservedRequestEvidence>,
        reason: String,
        session_id: Option<String>,
        deployment_id: Option<String>,
        client_build: Option<String>,
        protocol_pack_digest: Option<String>,
        scene_id: Option<i64>,
        map_id: Option<u64>,
        observed_micros: Option<u64>,
        markers: Vec<ObservedPointEvidence>,
    }

    fn read_observed_marker_evidence(
        base_url: &str,
    ) -> Result<ObservedMarkerEvidence, &'static str> {
        let authority = base_url
            .strip_prefix("http://127.0.0.1:")
            .ok_or("rlogs-base-url-must-be-loopback-http")?;
        if authority.is_empty() || !authority.bytes().all(|byte| byte.is_ascii_digit()) {
            return Err("rlogs-base-url-must-be-loopback-http");
        }
        serde_json::from_value(local_json_get(authority, "/api/automarkers/observed")?)
            .map_err(|_| "invalid-observed-marker-schema")
    }

    fn observed_evidence_matches_live(
        evidence: &ObservedMarkerEvidence,
        live: &LivePlayerContext,
    ) -> bool {
        let request_numbers = evidence
            .verified_requests
            .iter()
            .map(|request| request.marker_number)
            .collect::<BTreeSet<_>>();
        let marker_numbers = evidence
            .markers
            .iter()
            .map(|point| point.marker_number)
            .collect::<BTreeSet<_>>();
        let last_request_represented = evidence
            .last_verified_request_marker_number
            .zip(evidence.last_verified_request_observed_micros)
            .is_none_or(|(number, observed)| {
                evidence.verified_requests.iter().any(|request| {
                    request.marker_number == number && request.observed_micros == observed
                })
            });
        evidence.schema_version == 3
            && evidence.capture_active
            && evidence.protocol_supported
            && evidence.request_observer_supported
            && evidence.session_id.as_deref() == Some(live.identity.session_id.as_str())
            && evidence.deployment_id.as_deref() == Some(live.identity.deployment_id.as_str())
            && evidence.client_build.as_deref() == Some(BUILD)
            && evidence.protocol_pack_digest.as_deref()
                == Some(live.identity.protocol_pack_digest.as_str())
            && evidence.scene_id == Some(live.identity.scene_id)
            && evidence.map_id == Some(live.identity.map_id)
            && matches!(
                evidence.reason.as_str(),
                "observed_markers_available" | "no_fully_positioned_markers_observed"
            )
            && evidence.observed_micros.is_some()
            && evidence.last_verified_request_marker_number.is_some()
                == evidence.last_verified_request_observed_micros.is_some()
            && last_request_represented
            && evidence.verified_requests.len() <= evidence.verified_request_count as usize
            && request_numbers.len() == evidence.verified_requests.len()
            && evidence
                .verified_requests
                .iter()
                .all(|request| (1..=6).contains(&request.marker_number))
            && evidence.markers.iter().all(|point| {
                (1..=6).contains(&point.marker_number)
                    && point.x.is_finite()
                    && point.y.is_finite()
                    && point.z.is_finite()
                    && evidence
                        .observed_micros
                        .is_some_and(|observed| point.observed_micros <= observed)
            })
            && marker_numbers.len() == evidence.markers.len()
            && (evidence.reason != "observed_markers_available" || !evidence.markers.is_empty())
            && (evidence.reason != "no_fully_positioned_markers_observed"
                || evidence.markers.is_empty())
    }

    fn same_observed_identity(
        baseline: &ObservedMarkerEvidence,
        current: &ObservedMarkerEvidence,
    ) -> bool {
        current.session_id == baseline.session_id
            && current.deployment_id == baseline.deployment_id
            && current.client_build == baseline.client_build
            && current.protocol_pack_digest == baseline.protocol_pack_digest
            && current.scene_id == baseline.scene_id
            && current.map_id == baseline.map_id
    }

    fn marker_1_request_micros(evidence: &ObservedMarkerEvidence) -> Option<u64> {
        evidence
            .verified_requests
            .iter()
            .find(|request| request.marker_number == 1)
            .map(|request| request.observed_micros)
    }

    fn marker_1_point(evidence: &ObservedMarkerEvidence) -> Option<&ObservedPointEvidence> {
        evidence
            .markers
            .iter()
            .find(|point| point.marker_number == 1)
    }

    fn newer_marker_1_outbound(
        baseline: &ObservedMarkerEvidence,
        current: &ObservedMarkerEvidence,
    ) -> Option<u64> {
        let observed = marker_1_request_micros(current)?;
        (current.revision > baseline.revision
            && current.verified_request_count > baseline.verified_request_count
            && observed > marker_1_request_micros(baseline).unwrap_or(0))
        .then_some(observed)
    }

    fn newer_marker_1_inbound(
        baseline: &ObservedMarkerEvidence,
        current: &ObservedMarkerEvidence,
        outbound_micros: u64,
        target: &Position,
    ) -> Option<(u64, f32)> {
        let point = marker_1_point(current)?;
        let distance = position_distance(
            &Position {
                x: point.x,
                y: point.y,
                z: point.z,
            },
            target,
        );
        (current.revision > baseline.revision
            && point.observed_micros > outbound_micros
            && point.observed_micros
                > marker_1_point(baseline)
                    .map(|value| value.observed_micros)
                    .unwrap_or(0)
            && distance.is_finite()
            && distance <= PLACEMENT_COORDINATE_TOLERANCE)
            .then_some((point.observed_micros, distance))
    }

    fn capture_session_matches_map(map_session_id: &str, capture_session_id: Option<&str>) -> bool {
        capture_session_id == Some(map_session_id)
    }

    fn run_native_dispatch_preflight(
        memory: &impl Memory,
        module_base: usize,
        module_size: usize,
        request: &NativeDispatchPreflightRequest,
    ) -> CanaryReceipt {
        let set_position_code = reviewed_code_receipt(
            memory,
            module_base,
            module_size,
            ReviewedCodeSpec {
                identity: "Panda.ZGame.EntityAttrExtensions.SetIndicatorPos",
                rva_text: "0x53E86A0",
                rva: SET_INDICATOR_POS_RVA,
                byte_length: SET_INDICATOR_POS_BYTES,
                expected_sha256: SET_INDICATOR_POS_SHA256,
            },
        );
        let fire_indicator_code = reviewed_code_receipt(
            memory,
            module_base,
            module_size,
            ReviewedCodeSpec {
                identity: "Panda.ZGame.ZSkillInputMgr.FirePlaySkillByIndicator",
                rva_text: "0x52E09E0",
                rva: FIRE_PLAY_SKILL_BY_INDICATOR_RVA,
                byte_length: FIRE_PLAY_SKILL_BY_INDICATOR_BYTES,
                expected_sha256: FIRE_PLAY_SKILL_BY_INDICATOR_SHA256,
            },
        );
        let scheduler_reviewed_code = [
            reviewed_code_receipt(
                memory,
                module_base,
                module_size,
                ReviewedCodeSpec {
                    identity: "Cysharp.Threading.Tasks.UniTask.Post",
                    rva_text: "0x670EB30",
                    rva: UNITASK_POST_RVA,
                    byte_length: UNITASK_POST_BYTES,
                    expected_sha256: UNITASK_POST_SHA256,
                },
            ),
            reviewed_code_receipt(
                memory,
                module_base,
                module_size,
                ReviewedCodeSpec {
                    identity: "Cysharp.Threading.Tasks.PlayerLoopHelper.AddContinuation",
                    rva_text: "0x670AFE0",
                    rva: PLAYER_LOOP_ADD_CONTINUATION_RVA,
                    byte_length: PLAYER_LOOP_ADD_CONTINUATION_BYTES,
                    expected_sha256: PLAYER_LOOP_ADD_CONTINUATION_SHA256,
                },
            ),
            reviewed_code_receipt(
                memory,
                module_base,
                module_size,
                ReviewedCodeSpec {
                    identity: "Cysharp.Threading.Tasks.Internal.ContinuationQueue.Enqueue",
                    rva_text: "0x676A0B0",
                    rva: CONTINUATION_QUEUE_ENQUEUE_RVA,
                    byte_length: CONTINUATION_QUEUE_ENQUEUE_BYTES,
                    expected_sha256: CONTINUATION_QUEUE_ENQUEUE_SHA256,
                },
            ),
            reviewed_code_receipt(
                memory,
                module_base,
                module_size,
                ReviewedCodeSpec {
                    identity: "Cysharp.Threading.Tasks.Internal.ContinuationQueue.RunCore",
                    rva_text: "0x676A630",
                    rva: CONTINUATION_QUEUE_RUN_CORE_RVA,
                    byte_length: CONTINUATION_QUEUE_RUN_CORE_BYTES,
                    expected_sha256: CONTINUATION_QUEUE_RUN_CORE_SHA256,
                },
            ),
        ];
        let first = coherent_sample_with_roots(memory, module_base);
        let first_stage = read_dungeon_stage_sample(memory, module_base);
        let first_scheduler = read_main_thread_scheduler_sample(memory, module_base);
        let first_skill = first
            .as_ref()
            .map_err(|_| MarkerSkillResolutionError::RootChain)
            .and_then(|(roots, _)| read_marker_skill_resolution_sample(memory, roots));
        let first_leader = first
            .as_ref()
            .map_err(|error| *error)
            .and_then(|(roots, _)| read_party_leader_sample(memory, module_base, roots));
        thread::sleep(Duration::from_millis(STABILITY_SAMPLE_MILLIS));
        let second = coherent_sample_with_roots(memory, module_base);
        let second_stage = read_dungeon_stage_sample(memory, module_base);
        let second_scheduler = read_main_thread_scheduler_sample(memory, module_base);
        let second_skill = second
            .as_ref()
            .map_err(|_| MarkerSkillResolutionError::RootChain)
            .and_then(|(roots, _)| read_marker_skill_resolution_sample(memory, roots));
        let second_leader = second
            .as_ref()
            .map_err(|error| *error)
            .and_then(|(roots, _)| read_party_leader_sample(memory, module_base, roots));
        let root_chain_class_valid = first.is_ok() && second.is_ok();
        let root_chain_stable = matches!((&first, &second), (Ok((a, _)), Ok((b, _))) if a == b);
        let lifecycle_idle =
            matches!(&second, Ok((_, state)) if !state.is_press && !state.indicator.is_enable);
        let dungeon_stage_gate = dungeon_stage_gate(&first_stage, &second_stage);
        let main_thread_scheduler_gate =
            main_thread_scheduler_gate(&first_scheduler, &second_scheduler);
        let marker_skill_resolution_gate = if !root_chain_stable {
            BoundedGate {
                proven: false,
                reason: "unavailable-or-invalid-read-only-marker-skill-root-chain",
            }
        } else {
            marker_skill_resolution_gate(&first_skill, &second_skill)
        };
        let leader_gate = if !root_chain_stable {
            BoundedGate {
                proven: false,
                reason: "unavailable-or-invalid-read-only-party-leader-chain",
            }
        } else {
            party_leader_gate(&first_leader, &second_leader)
        };

        let (preset, preset_context_failure_reason) = match read_native_dispatch_preset_context(
            &request.rlogs_base_url,
            &request.preset_id,
        ) {
            Ok(value) => (Some(value), None),
            Err(reason) => (None, Some(reason)),
        };
        let preset_context_current = preset.is_some();
        let all_resolvable_gates_passed = root_chain_class_valid
            && root_chain_stable
            && lifecycle_idle
            && preset_context_current
            && dungeon_stage_gate.proven
            && leader_gate.proven
            && marker_skill_resolution_gate.proven
            && set_position_code.matches_reviewed_image
            && fire_indicator_code.matches_reviewed_image
            && scheduler_reviewed_code
                .iter()
                .all(|code| code.matches_reviewed_image)
            && main_thread_scheduler_gate.proven;
        let (scene_id, map_id, activity_family_id, marker_1_target) =
            preset.map_or((None, None, None, None), |value| {
                (
                    Some(value.scene_id),
                    Some(value.map_id),
                    Some(value.activity_family_id),
                    Some(value.marker_1_target),
                )
            });
        CanaryReceipt {
            armed: false,
            mode: "native-dispatch-preflight-v1",
            calibration_pixels: 0,
            marker_1_state_validated: false,
            foreground_validated_before_every_input: false,
            root_context_unchanged: root_chain_stable,
            lifecycle_context_unchanged: root_chain_stable,
            rank_2_input_excitation: false,
            escape_emitted: false,
            baseline: None,
            transitions: Vec::new(),
            final_return_error: None,
            approximately_returned: false,
            cancelled: false,
            outcome: "blocked-unresolved-native-gates",
            preflight: None,
            planner_step: None,
            closed_loop: None,
            native_dispatch_preflight: Some(NativeDispatchPreflightReceipt {
                preset_id: request.preset_id.clone(),
                scene_id,
                map_id,
                activity_family_id,
                marker_1_target,
                preset_context_failure_reason,
                exact_image_identity: set_position_code.matches_reviewed_image
                    && fire_indicator_code.matches_reviewed_image
                    && scheduler_reviewed_code
                        .iter()
                        .all(|code| code.matches_reviewed_image),
                root_chain_class_valid,
                root_chain_stable,
                lifecycle_idle,
                preset_context_current,
                reviewed_code: [set_position_code, fire_indicator_code],
                scheduler_reviewed_code,
                dungeon_stage_gate,
                leader_gate,
                marker_skill_resolution_gate,
                main_thread_scheduler_gate,
                main_thread_bridge_gate: BoundedGate {
                    proven: false,
                    reason: "unresolved-no-sanctioned-one-shot-main-thread-bridge",
                },
                all_resolvable_gates_passed,
                activation_attempted: false,
                outcome: "blocked-unresolved-native-gates",
            }),
        }
    }

    fn read_main_thread_scheduler_sample(
        memory: &impl Memory,
        module_base: usize,
    ) -> Result<MainThreadSchedulerSample, AcquireError> {
        let player_loop_helper_class = pointer_at(
            memory,
            checked_add(module_base, PLAYER_LOOP_HELPER_TYPE_INFO_RVA)?,
            true,
        )?;
        validate_class(
            memory,
            player_loop_helper_class,
            "PlayerLoopHelper",
            "Cysharp.Threading.Tasks",
        )?;
        let static_fields = pointer_at(
            memory,
            checked_add(player_loop_helper_class, IL2CPP_CLASS_STATIC_FIELDS)?,
            true,
        )?;
        let main_thread_id = i32_at(
            memory,
            checked_add(static_fields, PLAYER_LOOP_HELPER_MAIN_THREAD_ID)?,
        )?;
        if main_thread_id <= 0 {
            return Err(AcquireError::Identity);
        }
        let synchronization_context = pointer_at(
            memory,
            checked_add(static_fields, PLAYER_LOOP_HELPER_SYNCHRONIZATION_CONTEXT)?,
            true,
        )?;
        validate_exact_type_info_object(
            memory,
            module_base,
            synchronization_context,
            UNITY_SYNCHRONIZATION_CONTEXT_TYPE_INFO_RVA,
        )?;
        let yielders = pointer_at(
            memory,
            checked_add(static_fields, PLAYER_LOOP_HELPER_YIELDERS)?,
            true,
        )?;
        validate_exact_type_info_object(
            memory,
            module_base,
            yielders,
            CONTINUATION_QUEUE_ARRAY_TYPE_INFO_RVA,
        )?;
        let yielders_class = address_at(memory, yielders)?;
        if byte_at(memory, checked_add(yielders_class, IL2CPP_CLASS_RANK)?)? != 1
            || nonnegative_i32_at(
                memory,
                checked_add(yielders_class, IL2CPP_CLASS_ELEMENT_SIZE)?,
            )? != size_of::<usize>()
        {
            return Err(AcquireError::Identity);
        }
        let yielders_length = usize_at(memory, checked_add(yielders, MANAGED_ARRAY_LENGTH)?)?;
        if yielders_length <= UPDATE_PLAYER_LOOP_TIMING
            || yielders_length > MAX_REVIEWED_PLAYER_LOOP_TIMINGS
        {
            return Err(AcquireError::Identity);
        }
        let update_queue = pointer_at(
            memory,
            checked_add(
                yielders,
                MANAGED_ARRAY_VECTOR + UPDATE_PLAYER_LOOP_TIMING * size_of::<usize>(),
            )?,
            true,
        )?;
        validate_exact_type_info_object(
            memory,
            module_base,
            update_queue,
            CONTINUATION_QUEUE_TYPE_INFO_RVA,
        )?;
        validate_object(
            memory,
            update_queue,
            "ContinuationQueue",
            "Cysharp.Threading.Tasks.Internal",
        )?;
        let update_timing = i32_at(
            memory,
            checked_add(update_queue, CONTINUATION_QUEUE_TIMING)?,
        )?;
        if update_timing != UPDATE_PLAYER_LOOP_TIMING as i32 {
            return Err(AcquireError::Identity);
        }
        let dequeuing = bool_at(
            memory,
            checked_add(update_queue, CONTINUATION_QUEUE_DEQUEUING)?,
        )?;
        let action_list_count = nonnegative_i32_at(
            memory,
            checked_add(update_queue, CONTINUATION_QUEUE_ACTION_LIST_COUNT)?,
        )?;
        let action_list = pointer_at(
            memory,
            checked_add(update_queue, CONTINUATION_QUEUE_ACTION_LIST)?,
            true,
        )?;
        let waiting_list_count = nonnegative_i32_at(
            memory,
            checked_add(update_queue, CONTINUATION_QUEUE_WAITING_LIST_COUNT)?,
        )?;
        let waiting_list = pointer_at(
            memory,
            checked_add(update_queue, CONTINUATION_QUEUE_WAITING_LIST)?,
            true,
        )?;
        validate_exact_type_info_object(
            memory,
            module_base,
            action_list,
            SYSTEM_ACTION_ARRAY_TYPE_INFO_RVA,
        )?;
        validate_exact_type_info_object(
            memory,
            module_base,
            waiting_list,
            SYSTEM_ACTION_ARRAY_TYPE_INFO_RVA,
        )?;
        let action_list_length = usize_at(memory, checked_add(action_list, MANAGED_ARRAY_LENGTH)?)?;
        let waiting_list_length =
            usize_at(memory, checked_add(waiting_list, MANAGED_ARRAY_LENGTH)?)?;
        if action_list_length > MAX_REVIEWED_SCHEDULER_QUEUE_CAPACITY
            || waiting_list_length > MAX_REVIEWED_SCHEDULER_QUEUE_CAPACITY
            || action_list_count > action_list_length
            || waiting_list_count > waiting_list_length
        {
            return Err(AcquireError::Identity);
        }
        Ok(MainThreadSchedulerSample {
            player_loop_helper_class,
            static_fields,
            main_thread_id,
            synchronization_context,
            yielders,
            yielders_length,
            update_queue,
            update_timing,
            dequeuing,
            action_list,
            action_list_length,
            action_list_count,
            waiting_list,
            waiting_list_length,
            waiting_list_count,
        })
    }

    fn main_thread_scheduler_gate(
        first: &Result<MainThreadSchedulerSample, AcquireError>,
        second: &Result<MainThreadSchedulerSample, AcquireError>,
    ) -> BoundedGate {
        match (first, second) {
            (Ok(first), Ok(second)) if first == second => BoundedGate {
                proven: true,
                reason: "proven-read-only-main-thread-scheduler-state",
            },
            (Ok(_), Ok(_)) => BoundedGate {
                proven: false,
                reason: "unstable-read-only-main-thread-scheduler-state",
            },
            _ => BoundedGate {
                proven: false,
                reason: "unavailable-or-invalid-read-only-main-thread-scheduler-state",
            },
        }
    }

    fn read_dungeon_stage_sample(
        memory: &impl Memory,
        module_base: usize,
    ) -> Result<DungeonStageSample, AcquireError> {
        let stage_mgr = acquire_singleton_instance(
            memory,
            module_base,
            STAGE_MGR_SINGLETON_METHOD_INFO_RVA,
            STAGE_MGR_SINGLETON_TYPE_INFO_RVA,
            "StageMgr",
            "Panda",
        )?;
        let current_stage = pointer_at(
            memory,
            checked_add(stage_mgr, STAGE_MGR_CURRENT_STAGE)?,
            true,
        )?;
        validate_object(memory, current_stage, "StageDungeon", "Panda")?;
        Ok(DungeonStageSample {
            stage_mgr,
            current_stage,
            switch_state: byte_at(memory, checked_add(stage_mgr, STAGE_MGR_SWITCH_STATE)?)?,
            stage_type: byte_at(memory, checked_add(current_stage, STAGE_BASE_STAGE_TYPE)?)?,
        })
    }

    fn read_marker_skill_resolution_sample(
        memory: &impl Memory,
        roots: &Roots,
    ) -> Result<MarkerSkillResolutionSample, MarkerSkillResolutionError> {
        let data_mgr = pointer_at(
            memory,
            checked_add(roots.skill_input_comp, SKILL_INPUT_COMP_DATA_MGR)
                .map_err(|_| MarkerSkillResolutionError::DataManager)?,
            true,
        )
        .map_err(|_| MarkerSkillResolutionError::DataManager)?;
        validate_object(memory, data_mgr, "SkillControlDataMgr", "Panda.ZGame")
            .map_err(|_| MarkerSkillResolutionError::DataManager)?;

        let slot_dictionary_object = pointer_at(
            memory,
            checked_add(data_mgr, SKILL_DATA_MGR_SLOT_DICT)
                .map_err(|_| MarkerSkillResolutionError::SlotDictionaryPointer)?,
            true,
        )
        .map_err(|_| MarkerSkillResolutionError::SlotDictionaryPointer)?;
        let slot_dictionary = read_reviewed_dictionary(
            memory,
            slot_dictionary_object,
            ZDICTIONARY_INT_INT_ENTRY_STRIDE,
        )
        .map_err(|error| match error {
            ReviewedDictionaryError::Class => MarkerSkillResolutionError::SlotDictionaryClass,
            ReviewedDictionaryError::Header => MarkerSkillResolutionError::SlotDictionaryHeader,
            ReviewedDictionaryError::Shape => MarkerSkillResolutionError::SlotDictionaryShape,
            ReviewedDictionaryError::EntryStorage => {
                MarkerSkillResolutionError::SlotDictionaryEntryStorage
            }
            ReviewedDictionaryError::EntryCapacity => {
                MarkerSkillResolutionError::SlotDictionaryEntryCapacity
            }
            ReviewedDictionaryError::EntryArrayClass => {
                MarkerSkillResolutionError::SlotDictionaryEntryArrayClass
            }
            ReviewedDictionaryError::EntryStride => {
                MarkerSkillResolutionError::SlotDictionaryEntryStride
            }
        })?;
        let control_dictionary = read_reviewed_dictionary(
            memory,
            pointer_at(
                memory,
                checked_add(data_mgr, SKILL_DATA_MGR_CONTROL_DATAS)
                    .map_err(|_| MarkerSkillResolutionError::ControlDictionary)?,
                true,
            )
            .map_err(|_| MarkerSkillResolutionError::ControlDictionary)?,
            ZDICTIONARY_INT_OBJECT_ENTRY_STRIDE,
        )
        .map_err(|_| MarkerSkillResolutionError::ControlDictionary)?;
        let resolved_slot_skill_id =
            unique_int_dictionary_value(memory, &slot_dictionary, MARKER_1_SLOT_ID)
                .map_err(|_| MarkerSkillResolutionError::MarkerSlotLookup)?;
        if resolved_slot_skill_id != MARKER_1_SKILL_ID {
            return Err(MarkerSkillResolutionError::MarkerSlotMapping);
        }
        let resolved_control_data =
            unique_object_dictionary_value(memory, &control_dictionary, resolved_slot_skill_id)
                .map_err(|_| MarkerSkillResolutionError::ControlDataLookup)?;
        validate_object(
            memory,
            resolved_control_data,
            "SkillControlData",
            "Panda.ZGame",
        )
        .map_err(|_| MarkerSkillResolutionError::ControlDataClass)?;
        let resolved_control_skill_id = i32_at(
            memory,
            checked_add(resolved_control_data, SKILL_CONTROL_DATA_SKILL_ID)
                .map_err(|_| MarkerSkillResolutionError::ControlDataSkillIdentity)?,
        )
        .map_err(|_| MarkerSkillResolutionError::ControlDataSkillIdentity)?;
        if resolved_control_skill_id != MARKER_1_SKILL_ID {
            return Err(MarkerSkillResolutionError::ControlDataSkillIdentity);
        }
        Ok(MarkerSkillResolutionSample {
            data_mgr,
            slot_dictionary: slot_dictionary.object,
            slot_dictionary_entries: slot_dictionary.entries,
            slot_dictionary_count: slot_dictionary.count,
            slot_dictionary_version: slot_dictionary.version,
            control_dictionary: control_dictionary.object,
            control_dictionary_entries: control_dictionary.entries,
            control_dictionary_count: control_dictionary.count,
            control_dictionary_version: control_dictionary.version,
            resolved_slot_skill_id,
            resolved_control_data,
            resolved_control_skill_id,
        })
    }

    fn read_party_leader_sample(
        memory: &impl Memory,
        module_base: usize,
        roots: &Roots,
    ) -> Result<PartyLeaderSample, AcquireError> {
        let player_ent = roots.player_ent;
        let current_char_id = i64_at(memory, checked_add(player_ent, PLAYER_ENT_CHAR_ID)?)?;
        if current_char_id <= 0 {
            return Err(AcquireError::Identity);
        }
        let attr_collection = pointer_at(
            memory,
            checked_add(player_ent, PLAYER_ENT_ATTR_COLLECTION)?,
            true,
        )?;
        validate_object(memory, attr_collection, "ZAttrCollection", "Panda.ZGame")?;
        let cache = checked_add(attr_collection, ATTR_COLLECTION_CACHE_SLIM)?;
        let first_index_part =
            pointer_at(memory, checked_add(cache, ATTR_CACHE_INDEX_PART)?, true)?;
        let values = pointer_at(memory, checked_add(cache, ATTR_CACHE_VALUES)?, true)?;
        validate_exact_type_info_object(
            memory,
            module_base,
            values,
            VALUE_TUPLE_UINT_OBJECT_ARRAY_TYPE_INFO_RVA,
        )?;

        let (index_part, value_index) =
            find_unique_attr_value_index(memory, first_index_part, LOCAL_TEAMMATE_LIST_KEY)?;
        let attr = attr_value_object(memory, module_base, values, value_index)?;
        validate_exact_type_info_object(memory, module_base, attr, ZLIST_ATTR_LONG_TYPE_INFO_RVA)?;
        if bool_at(memory, checked_add(attr, ZATTR_IS_DEFAULT)?)? {
            return Err(AcquireError::Unavailable);
        }
        let teammate_list = pointer_at(memory, checked_add(attr, ZATTR_VALUE)?, true)?;
        validate_exact_type_info_object(
            memory,
            module_base,
            teammate_list,
            ZLIST_LONG_TYPE_INFO_RVA,
        )?;
        let teammate_count = nonnegative_i32_at(memory, checked_add(teammate_list, ZLIST_SIZE)?)?;
        if teammate_count == 0 || teammate_count > MAX_REVIEWED_TEAM_MEMBERS {
            return Err(AcquireError::Identity);
        }
        let teammate_items = pointer_at(memory, checked_add(teammate_list, ZLIST_ITEMS)?, true)?;
        validate_exact_type_info_object(
            memory,
            module_base,
            teammate_items,
            LONG_ARRAY_TYPE_INFO_RVA,
        )?;
        let item_capacity = usize_at(memory, checked_add(teammate_items, MANAGED_ARRAY_LENGTH)?)?;
        if item_capacity < teammate_count || item_capacity > MAX_REVIEWED_DICTIONARY_CAPACITY {
            return Err(AcquireError::Identity);
        }
        let leader_char_id = i64_at(memory, checked_add(teammate_items, MANAGED_ARRAY_VECTOR)?)?;
        if leader_char_id <= 0 {
            return Err(AcquireError::Identity);
        }
        Ok(PartyLeaderSample {
            player_ent,
            attr_collection,
            index_part,
            values,
            attr,
            teammate_list,
            teammate_items,
            teammate_count,
            current_char_id,
            leader_char_id,
        })
    }

    fn find_unique_attr_value_index(
        memory: &impl Memory,
        first_index_part: usize,
        key: u32,
    ) -> Result<(usize, usize), AcquireError> {
        let start_segment = ((key >> 3) as usize) & (ATTR_SEGMENT_COUNT - 1);
        let mut index_part = first_index_part;
        let mut found = None;
        for _ in 0..MAX_ATTR_INDEX_PARTS {
            let entry_count =
                nonnegative_i32_at(memory, checked_add(index_part, ATTR_INDEX_PART_COUNT)?)?;
            if entry_count > ATTR_INDEX_PART_CAPACITY {
                return Err(AcquireError::Identity);
            }
            let mask_offset = start_segment
                .checked_mul(size_of::<u32>())
                .and_then(|value| ATTR_INDEX_PART_START_SEGMENT_MASKS.checked_add(value))
                .ok_or(AcquireError::Read)?;
            let active_segments = u32_at(memory, checked_add(index_part, mask_offset)?)?;
            if entry_count == 0 && active_segments != 0 {
                return Err(AcquireError::Identity);
            }
            for segment in 0..ATTR_SEGMENT_COUNT {
                if active_segments & (1u32 << segment) == 0 {
                    continue;
                }
                let pointer_offset = segment
                    .checked_mul(size_of::<usize>())
                    .ok_or(AcquireError::Read)?;
                let key_segment = pointer_at(
                    memory,
                    checked_add(
                        index_part,
                        ATTR_INDEX_PART_KEY_SEGMENTS
                            .checked_add(pointer_offset)
                            .ok_or(AcquireError::Read)?,
                    )?,
                    true,
                )?;
                let segment_count =
                    nonnegative_i32_at(memory, checked_add(key_segment, ATTR_KEY_SEGMENT_COUNT)?)?;
                if segment_count == 0 || segment_count > ATTR_KEY_SEGMENT_CAPACITY {
                    return Err(AcquireError::Identity);
                }
                for slot in 0..segment_count {
                    let key_offset = slot
                        .checked_mul(size_of::<u32>())
                        .ok_or(AcquireError::Read)?;
                    if u32_at(memory, checked_add(key_segment, key_offset)?)? != key {
                        continue;
                    }
                    let value_indices = pointer_at(
                        memory,
                        checked_add(
                            index_part,
                            ATTR_INDEX_PART_VALUE_INDEX_SEGMENTS
                                .checked_add(pointer_offset)
                                .ok_or(AcquireError::Read)?,
                        )?,
                        true,
                    )?;
                    let value_index = i32_at(
                        memory,
                        checked_add(
                            value_indices,
                            slot.checked_mul(size_of::<i32>())
                                .ok_or(AcquireError::Read)?,
                        )?,
                    )?;
                    if value_index < 0
                        || found.replace((index_part, value_index as usize)).is_some()
                    {
                        return Err(AcquireError::Identity);
                    }
                }
            }
            let next = usize_at(memory, checked_add(index_part, ATTR_INDEX_PART_NEXT)?)?;
            if next == 0 {
                return found.ok_or(AcquireError::Unavailable);
            }
            if !plausible_pointer(next) {
                return Err(AcquireError::Identity);
            }
            index_part = next;
        }
        Err(AcquireError::Identity)
    }

    fn attr_value_object(
        memory: &impl Memory,
        module_base: usize,
        values: usize,
        value_index: usize,
    ) -> Result<usize, AcquireError> {
        let group = value_index / 32;
        let slot = value_index % 32;
        let groups = usize_at(memory, checked_add(values, MANAGED_ARRAY_LENGTH)?)?;
        if group >= groups || groups > MAX_REVIEWED_DICTIONARY_CAPACITY {
            return Err(AcquireError::Identity);
        }
        let tuple_offset = group
            .checked_mul(VALUE_TUPLE_STRIDE)
            .and_then(|value| MANAGED_ARRAY_VECTOR.checked_add(value))
            .ok_or(AcquireError::Read)?;
        let tuple = checked_add(values, tuple_offset)?;
        let mask = u32_at(memory, checked_add(tuple, VALUE_TUPLE_MASK)?)?;
        if mask & (1u32 << slot) == 0 {
            return Err(AcquireError::Identity);
        }
        let objects = pointer_at(memory, checked_add(tuple, VALUE_TUPLE_OBJECT_ARRAY)?, true)?;
        validate_exact_type_info_object(memory, module_base, objects, OBJECT_ARRAY_TYPE_INFO_RVA)?;
        let object_count = usize_at(memory, checked_add(objects, MANAGED_ARRAY_LENGTH)?)?;
        if slot >= object_count || object_count > 32 {
            return Err(AcquireError::Identity);
        }
        pointer_at(
            memory,
            checked_add(
                objects,
                MANAGED_ARRAY_VECTOR
                    .checked_add(
                        slot.checked_mul(size_of::<usize>())
                            .ok_or(AcquireError::Read)?,
                    )
                    .ok_or(AcquireError::Read)?,
            )?,
            true,
        )
    }

    fn validate_exact_type_info_object(
        memory: &impl Memory,
        module_base: usize,
        object: usize,
        type_info_rva: usize,
    ) -> Result<(), AcquireError> {
        let actual_class = pointer_at(memory, object, false)?;
        let expected_class = pointer_at(memory, checked_add(module_base, type_info_rva)?, false)?;
        (actual_class == expected_class)
            .then_some(())
            .ok_or(AcquireError::Identity)
    }

    fn party_leader_gate(
        first: &Result<PartyLeaderSample, AcquireError>,
        second: &Result<PartyLeaderSample, AcquireError>,
    ) -> BoundedGate {
        match (first, second) {
            (Ok(first), Ok(second)) if first != second => BoundedGate {
                proven: false,
                reason: "unstable-read-only-party-leader-state",
            },
            (Ok(sample), Ok(_)) if sample.current_char_id == sample.leader_char_id => BoundedGate {
                proven: true,
                reason: "proven-read-only-current-player-is-party-leader",
            },
            (Ok(_), Ok(_)) => BoundedGate {
                proven: false,
                reason: "current-player-is-not-party-leader",
            },
            _ => BoundedGate {
                proven: false,
                reason: "unavailable-or-invalid-read-only-party-leader-chain",
            },
        }
    }

    fn read_reviewed_dictionary(
        memory: &impl Memory,
        object: usize,
        entry_stride: usize,
    ) -> Result<ReviewedDictionary, ReviewedDictionaryError> {
        validate_object(memory, object, "ZDictionary`2", "ZUtil.Pool.Collections")
            .map_err(|_| ReviewedDictionaryError::Class)?;
        let buckets_length = nonnegative_i32_at(
            memory,
            checked_add(object, ZDICTIONARY_BUCKETS_LENGTH)
                .map_err(|_| ReviewedDictionaryError::Header)?,
        )
        .map_err(|_| ReviewedDictionaryError::Header)?;
        let count = nonnegative_i32_at(
            memory,
            checked_add(object, ZDICTIONARY_COUNT).map_err(|_| ReviewedDictionaryError::Header)?,
        )
        .map_err(|_| ReviewedDictionaryError::Header)?;
        let free_count = nonnegative_i32_at(
            memory,
            checked_add(object, ZDICTIONARY_FREE_COUNT)
                .map_err(|_| ReviewedDictionaryError::Header)?,
        )
        .map_err(|_| ReviewedDictionaryError::Header)?;
        if count == 0
            || buckets_length == 0
            || count > MAX_REVIEWED_DICTIONARY_CAPACITY
            || buckets_length > MAX_REVIEWED_DICTIONARY_CAPACITY
            || free_count > count
        {
            return Err(ReviewedDictionaryError::Shape);
        }
        let entries = pointer_at(
            memory,
            checked_add(object, ZDICTIONARY_ENTRIES)
                .map_err(|_| ReviewedDictionaryError::EntryStorage)?,
            true,
        )
        .map_err(|_| ReviewedDictionaryError::EntryStorage)?;
        let entries_length = usize_at(
            memory,
            checked_add(entries, MANAGED_ARRAY_LENGTH)
                .map_err(|_| ReviewedDictionaryError::EntryStorage)?,
        )
        .map_err(|_| ReviewedDictionaryError::EntryStorage)?;
        if entries_length < count || entries_length > MAX_REVIEWED_DICTIONARY_CAPACITY {
            return Err(ReviewedDictionaryError::EntryCapacity);
        }
        let array_class = pointer_at(memory, entries, false)
            .map_err(|_| ReviewedDictionaryError::EntryArrayClass)?;
        if byte_at(
            memory,
            checked_add(array_class, IL2CPP_CLASS_RANK)
                .map_err(|_| ReviewedDictionaryError::EntryArrayClass)?,
        )
        .map_err(|_| ReviewedDictionaryError::EntryArrayClass)?
            != 1
        {
            return Err(ReviewedDictionaryError::EntryArrayClass);
        }
        if nonnegative_i32_at(
            memory,
            checked_add(array_class, IL2CPP_CLASS_ELEMENT_SIZE)
                .map_err(|_| ReviewedDictionaryError::EntryStride)?,
        )
        .map_err(|_| ReviewedDictionaryError::EntryStride)?
            != entry_stride
        {
            return Err(ReviewedDictionaryError::EntryStride);
        }
        Ok(ReviewedDictionary {
            object,
            entries,
            count,
            version: i32_at(
                memory,
                checked_add(object, ZDICTIONARY_VERSION)
                    .map_err(|_| ReviewedDictionaryError::Header)?,
            )
            .map_err(|_| ReviewedDictionaryError::Header)?,
        })
    }

    fn unique_int_dictionary_value(
        memory: &impl Memory,
        dictionary: &ReviewedDictionary,
        expected_key: i32,
    ) -> Result<i32, AcquireError> {
        let mut found = None;
        for index in 0..dictionary.count {
            let entry = dictionary_entry_address(
                dictionary.entries,
                index,
                ZDICTIONARY_INT_INT_ENTRY_STRIDE,
            )?;
            if i32_at(memory, checked_add(entry, ZDICTIONARY_ENTRY_HASH_CODE)?)? < 0
                || i32_at(memory, checked_add(entry, ZDICTIONARY_ENTRY_KEY)?)? != expected_key
            {
                continue;
            }
            let value = i32_at(memory, checked_add(entry, ZDICTIONARY_INT_INT_ENTRY_VALUE)?)?;
            if found.replace(value).is_some() {
                return Err(AcquireError::Identity);
            }
        }
        found.ok_or(AcquireError::Unavailable)
    }

    fn unique_object_dictionary_value(
        memory: &impl Memory,
        dictionary: &ReviewedDictionary,
        expected_key: i32,
    ) -> Result<usize, AcquireError> {
        let mut found = None;
        for index in 0..dictionary.count {
            let entry = dictionary_entry_address(
                dictionary.entries,
                index,
                ZDICTIONARY_INT_OBJECT_ENTRY_STRIDE,
            )?;
            if i32_at(memory, checked_add(entry, ZDICTIONARY_ENTRY_HASH_CODE)?)? < 0
                || i32_at(memory, checked_add(entry, ZDICTIONARY_ENTRY_KEY)?)? != expected_key
            {
                continue;
            }
            let value = pointer_at(
                memory,
                checked_add(entry, ZDICTIONARY_INT_OBJECT_ENTRY_VALUE)?,
                true,
            )?;
            if found.replace(value).is_some() {
                return Err(AcquireError::Identity);
            }
        }
        found.ok_or(AcquireError::Unavailable)
    }

    fn dictionary_entry_address(
        entries: usize,
        index: usize,
        stride: usize,
    ) -> Result<usize, AcquireError> {
        let offset = index
            .checked_mul(stride)
            .and_then(|value| MANAGED_ARRAY_VECTOR.checked_add(value))
            .ok_or(AcquireError::Read)?;
        checked_add(entries, offset)
    }

    fn marker_skill_resolution_gate(
        first: &Result<MarkerSkillResolutionSample, MarkerSkillResolutionError>,
        second: &Result<MarkerSkillResolutionSample, MarkerSkillResolutionError>,
    ) -> BoundedGate {
        match (first, second) {
            (Ok(first), Ok(second)) if first == second => BoundedGate {
                proven: true,
                reason: "proven-read-only-marker-1-slot-and-skill-resolution",
            },
            (Ok(_), Ok(_)) => BoundedGate {
                proven: false,
                reason: "unstable-read-only-marker-skill-lifecycle",
            },
            (Err(first), Err(second)) if first == second => BoundedGate {
                proven: false,
                reason: first.bounded_reason(),
            },
            (Err(error), Ok(_)) | (Ok(_), Err(error)) => BoundedGate {
                proven: false,
                reason: error.bounded_reason(),
            },
            (Err(_), Err(_)) => BoundedGate {
                proven: false,
                reason: "inconsistent-read-only-marker-skill-failure-stage",
            },
        }
    }

    fn dungeon_stage_gate(
        first: &Result<DungeonStageSample, AcquireError>,
        second: &Result<DungeonStageSample, AcquireError>,
    ) -> BoundedGate {
        match (first, second) {
            (Ok(first), Ok(second)) if first != second => BoundedGate {
                proven: false,
                reason: "unstable-read-only-dungeon-stage-lifecycle",
            },
            (Ok(sample), Ok(_))
                if sample.switch_state == SWITCH_STATE_NONE
                    && sample.stage_type == STAGE_TYPE_DUNGEON =>
            {
                BoundedGate {
                    proven: true,
                    reason: "proven-read-only-current-dungeon-stage",
                }
            }
            (Ok(sample), Ok(_)) if sample.switch_state != SWITCH_STATE_NONE => BoundedGate {
                proven: false,
                reason: "stage-is-loading-or-switching",
            },
            (Ok(_), Ok(_)) => BoundedGate {
                proven: false,
                reason: "current-stage-is-not-standard-dungeon",
            },
            _ => BoundedGate {
                proven: false,
                reason: "unavailable-or-invalid-read-only-dungeon-stage-chain",
            },
        }
    }

    fn reviewed_code_receipt(
        memory: &impl Memory,
        module_base: usize,
        module_size: usize,
        spec: ReviewedCodeSpec,
    ) -> ReviewedCodeReceipt {
        let range_within_module = spec
            .rva
            .checked_add(spec.byte_length)
            .is_some_and(|end| end <= module_size);
        let sha256 = if range_within_module {
            checked_add(module_base, spec.rva)
                .and_then(|address| {
                    let mut hasher = Sha256::new();
                    let mut offset = 0usize;
                    while offset < spec.byte_length {
                        let length = (spec.byte_length - offset).min(REVIEWED_CODE_READ_CHUNK);
                        let bytes = memory.read_exact(checked_add(address, offset)?, length)?;
                        hasher.update(bytes);
                        offset = offset.checked_add(length).ok_or(AcquireError::Read)?;
                    }
                    Ok(format!("{:x}", hasher.finalize()))
                })
                .unwrap_or_default()
        } else {
            String::new()
        };
        let matches_reviewed_image = sha256 == spec.expected_sha256;
        ReviewedCodeReceipt {
            identity: spec.identity,
            rva: spec.rva_text,
            byte_length: spec.byte_length,
            sha256,
            matches_reviewed_image,
        }
    }

    fn read_native_dispatch_preset_context(
        base_url: &str,
        preset_id: &str,
    ) -> Result<NativeDispatchPresetContext, &'static str> {
        if preset_id.len() < 8 || preset_id.len() > 128 || preset_id.chars().any(char::is_control) {
            return Err("invalid-preflight-preset-id");
        }
        let authority = base_url
            .strip_prefix("http://127.0.0.1:")
            .ok_or("rlogs-base-url-must-be-loopback-http")?;
        if authority.is_empty() || !authority.bytes().all(|byte| byte.is_ascii_digit()) {
            return Err("rlogs-base-url-must-be-loopback-http");
        }
        let projection = local_json_get(authority, "/api/automarkers/presets")?;
        parse_native_dispatch_preset_context(&projection, preset_id)
    }

    fn parse_native_dispatch_preset_context(
        projection: &serde_json::Value,
        preset_id: &str,
    ) -> Result<NativeDispatchPresetContext, &'static str> {
        if projection
            .get("schemaVersion")
            .and_then(serde_json::Value::as_u64)
            != Some(4)
        {
            return Err("invalid-automarker-preset-schema");
        }
        let context = projection
            .get("context")
            .ok_or("missing-automarker-context")?;
        if context
            .get("clientBuild")
            .and_then(serde_json::Value::as_str)
            != Some(BUILD)
        {
            return Err("automarker-context-not-current-build");
        }
        let scene_id = context
            .get("sceneId")
            .and_then(serde_json::Value::as_i64)
            .ok_or("missing-scene")?;
        let map_id = context
            .get("mapId")
            .and_then(serde_json::Value::as_u64)
            .ok_or("missing-map")?;
        let activity_family_id = context
            .get("activityFamilyId")
            .and_then(serde_json::Value::as_str)
            .filter(|value| !value.is_empty() && value.len() <= 128)
            .ok_or("missing-family")?;
        let presets = projection
            .get("presets")
            .and_then(serde_json::Value::as_array)
            .ok_or("invalid-automarker-preset-schema")?;
        let mut matches = presets.iter().filter(|preset| {
            preset.get("presetId").and_then(serde_json::Value::as_str) == Some(preset_id)
        });
        let preset = matches.next().ok_or("preflight-preset-not-found")?;
        if matches.next().is_some() {
            return Err("duplicate-preflight-preset-id");
        }
        if preset
            .get("activityFamilyId")
            .and_then(serde_json::Value::as_str)
            != Some(activity_family_id)
        {
            return Err("preflight-preset-family-mismatch");
        }
        let points = preset
            .get("points")
            .and_then(serde_json::Value::as_array)
            .ok_or("invalid-automarker-preset-schema")?;
        let mut marker_1 = points.iter().filter(|point| {
            point
                .get("markerNumber")
                .and_then(serde_json::Value::as_u64)
                == Some(1)
        });
        let point = marker_1.next().ok_or("preflight-preset-missing-marker-1")?;
        if marker_1.next().is_some() {
            return Err("preflight-preset-duplicate-marker-1");
        }
        let coordinate = |name| {
            point
                .get(name)
                .and_then(serde_json::Value::as_f64)
                .filter(|value| value.is_finite())
                .map(|value| value as f32)
                .filter(|value| value.is_finite())
                .ok_or("invalid-preflight-marker-coordinate")
        };
        let marker_1_target = Position {
            x: coordinate("x")?,
            y: coordinate("y")?,
            z: coordinate("z")?,
        };
        if position_norm(&marker_1_target) == 0.0 {
            return Err("invalid-zero-target");
        }
        Ok(NativeDispatchPresetContext {
            scene_id,
            map_id,
            activity_family_id: activity_family_id.to_owned(),
            marker_1_target,
        })
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
        let session_id = snapshot
            .get("session_id")
            .and_then(serde_json::Value::as_str)
            .ok_or("missing-session")?;
        let capture_session_id = presets
            .get("captureSessionId")
            .and_then(serde_json::Value::as_str)
            .ok_or("missing-automarker-capture-session")?;
        let deployment_id = presets
            .get("deploymentId")
            .and_then(serde_json::Value::as_str)
            .filter(|value| !value.is_empty())
            .ok_or("missing-automarker-deployment")?;
        let protocol_pack_digest = presets
            .get("protocolPackDigest")
            .and_then(serde_json::Value::as_str)
            .filter(|value| !value.is_empty())
            .ok_or("missing-automarker-protocol-pack")?;
        if !capture_session_matches_map(session_id, Some(capture_session_id)) {
            return Err("automarker-capture-session-mismatch");
        }
        Ok(LivePlayerContext {
            identity: LiveMapIdentity {
                session_id: session_id.to_owned(),
                deployment_id: deployment_id.to_owned(),
                client_build: BUILD.to_owned(),
                protocol_pack_digest: protocol_pack_digest.to_owned(),
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

    fn u32_at(memory: &impl Memory, address: usize) -> Result<u32, AcquireError> {
        let bytes = memory.read_exact(address, 4)?;
        Ok(u32::from_le_bytes(
            bytes.try_into().map_err(|_| AcquireError::Read)?,
        ))
    }

    fn i64_at(memory: &impl Memory, address: usize) -> Result<i64, AcquireError> {
        let bytes = memory.read_exact(address, 8)?;
        Ok(i64::from_le_bytes(
            bytes.try_into().map_err(|_| AcquireError::Read)?,
        ))
    }

    fn nonnegative_i32_at(memory: &impl Memory, address: usize) -> Result<usize, AcquireError> {
        usize::try_from(i32_at(memory, address)?).map_err(|_| AcquireError::Identity)
    }

    fn usize_at(memory: &impl Memory, address: usize) -> Result<usize, AcquireError> {
        let bytes = memory.read_exact(address, size_of::<usize>())?;
        Ok(usize::from_le_bytes(
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
        size: usize,
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
                    size: entry.modBaseSize as usize,
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
        const ALLOWED: [&str; 13] = [
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
            "native-dispatch-preflight",
            "preset-id",
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
                && value != OPERATOR_PLACEMENT_ARMED_MODE_TOKEN
        }) {
            return Err("unknown armed-mode token".into());
        }
        let native_dispatch_preflight =
            match options.get("native-dispatch-preflight").map(String::as_str) {
                Some("true") => true,
                Some(_) => return Err("--native-dispatch-preflight accepts only true".into()),
                None => false,
            };
        if native_dispatch_preflight && options.contains_key("armed-mode") {
            return Err("native dispatch preflight cannot be combined with an armed mode".into());
        }
        let planner_options = ["target-x", "target-y", "target-z", "rlogs-base-url"];
        if matches!(
            options.get("armed-mode").map(String::as_str),
            Some(
                PLANNER_ARMED_MODE_TOKEN
                    | CLOSED_LOOP_ARMED_MODE_TOKEN
                    | OPERATOR_PLACEMENT_ARMED_MODE_TOKEN
            )
        ) {
            if planner_options
                .iter()
                .any(|key| !options.contains_key(*key))
            {
                return Err("planner mode requires target XYZ and rlogs-base-url".into());
            }
        } else if !native_dispatch_preflight
            && planner_options.iter().any(|key| options.contains_key(*key))
        {
            return Err("planner-only options require the exact planner armed mode".into());
        }
        if native_dispatch_preflight {
            required(options, "preset-id")?;
            required(options, "rlogs-base-url")?;
            if ["target-x", "target-y", "target-z"]
                .iter()
                .any(|key| options.contains_key(*key))
            {
                return Err(
                    "native dispatch preflight resolves its target from the selected preset".into(),
                );
            }
        } else if options.contains_key("preset-id") {
            return Err("--preset-id requires native dispatch preflight".into());
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
            let data_mgr = 0x85_0000;
            let slot_dictionary = 0x86_0000;
            let control_dictionary = 0x87_0000;
            let slot_entries = 0x88_0000;
            let control_entries = 0x89_0000;
            let control_data = 0x8A_0000;
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
            m.ptr(comp + SKILL_INPUT_COMP_DATA_MGR, data_mgr);
            m.ptr(comp + SKILL_INPUT_COMP_MGR, mgr);
            for (i, class) in classes.iter().enumerate() {
                m.ptr(*class + IL2CPP_CLASS_NAME, 0xA1_0000 + i * 0x100);
                m.ptr(*class + IL2CPP_CLASS_NAMESPACE, 0xA2_0000 + i * 0x100);
                m.text(0xA1_0000 + i * 0x100, names[i]);
                m.text(0xA2_0000 + i * 0x100, "Panda.ZGame");
            }
            let data_mgr_class = 0x3B_0000;
            let dictionary_class = 0x3C_0000;
            let slot_array_class = 0x3D_0000;
            let control_array_class = 0x3D_1000;
            let control_data_class = 0x3E_0000;
            m.ptr(data_mgr, data_mgr_class);
            m.ptr(data_mgr_class + IL2CPP_CLASS_NAME, 0xA5_0000);
            m.ptr(data_mgr_class + IL2CPP_CLASS_NAMESPACE, 0xA5_0100);
            m.text(0xA5_0000, "SkillControlDataMgr");
            m.text(0xA5_0100, "Panda.ZGame");
            m.ptr(data_mgr + SKILL_DATA_MGR_SLOT_DICT, slot_dictionary);
            m.ptr(data_mgr + SKILL_DATA_MGR_CONTROL_DATAS, control_dictionary);
            m.ptr(dictionary_class + IL2CPP_CLASS_NAME, 0xA5_0200);
            m.ptr(dictionary_class + IL2CPP_CLASS_NAMESPACE, 0xA5_0300);
            m.text(0xA5_0200, "ZDictionary`2");
            m.text(0xA5_0300, "ZUtil.Pool.Collections");
            for (dictionary, entries, array_class, entry_stride) in [
                (
                    slot_dictionary,
                    slot_entries,
                    slot_array_class,
                    ZDICTIONARY_INT_INT_ENTRY_STRIDE,
                ),
                (
                    control_dictionary,
                    control_entries,
                    control_array_class,
                    ZDICTIONARY_INT_OBJECT_ENTRY_STRIDE,
                ),
            ] {
                m.ptr(dictionary, dictionary_class);
                m.put(dictionary + ZDICTIONARY_BUCKETS_LENGTH, &1i32.to_le_bytes());
                m.ptr(dictionary + ZDICTIONARY_ENTRIES, entries);
                m.put(dictionary + ZDICTIONARY_COUNT, &1i32.to_le_bytes());
                m.put(dictionary + ZDICTIONARY_VERSION, &7i32.to_le_bytes());
                m.put(dictionary + ZDICTIONARY_FREE_COUNT, &0i32.to_le_bytes());
                m.ptr(entries, array_class);
                m.ptr(entries + MANAGED_ARRAY_LENGTH, 1);
                m.put(array_class + IL2CPP_CLASS_RANK, &[1]);
                m.put(
                    array_class + IL2CPP_CLASS_ELEMENT_SIZE,
                    &(entry_stride as i32).to_le_bytes(),
                );
            }
            let slot_entry = slot_entries + MANAGED_ARRAY_VECTOR;
            m.put(
                slot_entry + ZDICTIONARY_ENTRY_HASH_CODE,
                &201i32.to_le_bytes(),
            );
            m.put(
                slot_entry + ZDICTIONARY_ENTRY_KEY,
                &MARKER_1_SLOT_ID.to_le_bytes(),
            );
            m.put(
                slot_entry + ZDICTIONARY_INT_INT_ENTRY_VALUE,
                &MARKER_1_SKILL_ID.to_le_bytes(),
            );
            let control_entry = control_entries + MANAGED_ARRAY_VECTOR;
            m.put(
                control_entry + ZDICTIONARY_ENTRY_HASH_CODE,
                &MARKER_1_SKILL_ID.to_le_bytes(),
            );
            m.put(
                control_entry + ZDICTIONARY_ENTRY_KEY,
                &MARKER_1_SKILL_ID.to_le_bytes(),
            );
            m.ptr(
                control_entry + ZDICTIONARY_INT_OBJECT_ENTRY_VALUE,
                control_data,
            );
            m.ptr(control_data, control_data_class);
            m.ptr(control_data_class + IL2CPP_CLASS_NAME, 0xA5_0400);
            m.ptr(control_data_class + IL2CPP_CLASS_NAMESPACE, 0xA5_0500);
            m.text(0xA5_0400, "SkillControlData");
            m.text(0xA5_0500, "Panda.ZGame");
            m.put(
                control_data + SKILL_CONTROL_DATA_SKILL_ID,
                &MARKER_1_SKILL_ID.to_le_bytes(),
            );
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
            let stage_method = 0x22_0000;
            let stage_declaring_class = 0x38_0000;
            let stage_singleton_class = 0x38_1000;
            let stage_generic_context = 0x38_2000;
            let stage_static_fields = 0x42_0000;
            let stage_mgr = 0x92_0000;
            let stage_mgr_class = 0x39_0000;
            let current_stage = 0x93_0000;
            let current_stage_class = 0x3A_0000;
            m.ptr(base + STAGE_MGR_SINGLETON_METHOD_INFO_RVA, stage_method);
            m.ptr(
                base + STAGE_MGR_SINGLETON_TYPE_INFO_RVA,
                stage_singleton_class,
            );
            m.ptr(stage_method + METHOD_INFO_NAME, 0xA4_0000);
            m.ptr(stage_method + METHOD_INFO_KLASS, stage_declaring_class);
            m.text(0xA4_0000, "get_Instance");
            m.ptr(stage_declaring_class + IL2CPP_CLASS_NAME, 0xA4_0100);
            m.ptr(stage_declaring_class + IL2CPP_CLASS_NAMESPACE, 0xA4_0200);
            m.text(0xA4_0100, "ZSingleton`1");
            m.text(0xA4_0200, "ZUtil");
            m.ptr(
                stage_declaring_class + IL2CPP_CLASS_GENERIC_CONTEXT,
                stage_generic_context,
            );
            m.ptr(
                stage_generic_context + GENERIC_CONTEXT_INFLATED_CLASS,
                stage_singleton_class,
            );
            m.ptr(stage_singleton_class + IL2CPP_CLASS_NAME, 0xA4_0300);
            m.ptr(stage_singleton_class + IL2CPP_CLASS_NAMESPACE, 0xA4_0400);
            m.text(0xA4_0300, "ZSingleton`1");
            m.text(0xA4_0400, "ZUtil");
            m.ptr(
                stage_singleton_class + IL2CPP_CLASS_STATIC_FIELDS,
                stage_static_fields,
            );
            m.ptr(stage_static_fields, stage_mgr);
            m.ptr(stage_mgr, stage_mgr_class);
            m.ptr(stage_mgr_class + IL2CPP_CLASS_NAME, 0xA4_0500);
            m.ptr(stage_mgr_class + IL2CPP_CLASS_NAMESPACE, 0xA4_0600);
            m.text(0xA4_0500, "StageMgr");
            m.text(0xA4_0600, "Panda");
            m.put(stage_mgr + STAGE_MGR_SWITCH_STATE, &[SWITCH_STATE_NONE]);
            m.ptr(stage_mgr + STAGE_MGR_CURRENT_STAGE, current_stage);
            m.ptr(current_stage, current_stage_class);
            m.ptr(current_stage_class + IL2CPP_CLASS_NAME, 0xA4_0700);
            m.ptr(current_stage_class + IL2CPP_CLASS_NAMESPACE, 0xA4_0800);
            m.text(0xA4_0700, "StageDungeon");
            m.text(0xA4_0800, "Panda");
            m.put(current_stage + STAGE_BASE_STAGE_TYPE, &[STAGE_TYPE_DUNGEON]);

            let attr_collection = 0x94_0000;
            let index_part = 0x95_0000;
            let key_segment = 0x96_0000;
            let value_indices = 0x97_0000;
            let values = 0x98_0000;
            let objects = 0x99_0000;
            let attr = 0x9A_0000;
            let teammate_list = 0x9B_0000;
            let teammate_items = 0x9C_0000;
            let attr_collection_class = 0x43_0000;
            let values_class = 0x44_0000;
            let objects_class = 0x45_0000;
            let attr_class = 0x46_0000;
            let list_class = 0x47_0000;
            let long_array_class = 0x48_0000;
            let char_id = 7_654_321i64;
            m.ptr(player + PLAYER_ENT_ATTR_COLLECTION, attr_collection);
            m.put(player + PLAYER_ENT_CHAR_ID, &char_id.to_le_bytes());
            m.ptr(attr_collection, attr_collection_class);
            m.ptr(attr_collection_class + IL2CPP_CLASS_NAME, 0xA6_0000);
            m.ptr(attr_collection_class + IL2CPP_CLASS_NAMESPACE, 0xA6_0100);
            m.text(0xA6_0000, "ZAttrCollection");
            m.text(0xA6_0100, "Panda.ZGame");
            m.ptr(
                attr_collection + ATTR_COLLECTION_CACHE_SLIM + ATTR_CACHE_INDEX_PART,
                index_part,
            );
            m.ptr(
                attr_collection + ATTR_COLLECTION_CACHE_SLIM + ATTR_CACHE_VALUES,
                values,
            );
            m.put(index_part + ATTR_INDEX_PART_COUNT, &1i32.to_le_bytes());
            m.ptr(index_part + ATTR_INDEX_PART_NEXT, 0);
            let start_segment =
                ((LOCAL_TEAMMATE_LIST_KEY >> 3) as usize) & (ATTR_SEGMENT_COUNT - 1);
            let actual_segment = start_segment;
            m.ptr(
                index_part + ATTR_INDEX_PART_KEY_SEGMENTS + actual_segment * size_of::<usize>(),
                key_segment,
            );
            m.ptr(
                index_part
                    + ATTR_INDEX_PART_VALUE_INDEX_SEGMENTS
                    + actual_segment * size_of::<usize>(),
                value_indices,
            );
            m.put(
                index_part + ATTR_INDEX_PART_START_SEGMENT_MASKS + start_segment * size_of::<u32>(),
                &(1u32 << actual_segment).to_le_bytes(),
            );
            m.put(key_segment, &LOCAL_TEAMMATE_LIST_KEY.to_le_bytes());
            m.put(key_segment + ATTR_KEY_SEGMENT_COUNT, &1i32.to_le_bytes());
            m.put(value_indices, &0i32.to_le_bytes());
            m.ptr(values, values_class);
            m.ptr(
                base + VALUE_TUPLE_UINT_OBJECT_ARRAY_TYPE_INFO_RVA,
                values_class,
            );
            m.ptr(values + MANAGED_ARRAY_LENGTH, 1);
            m.put(
                values + MANAGED_ARRAY_VECTOR + VALUE_TUPLE_MASK,
                &1u32.to_le_bytes(),
            );
            m.ptr(
                values + MANAGED_ARRAY_VECTOR + VALUE_TUPLE_OBJECT_ARRAY,
                objects,
            );
            m.ptr(objects, objects_class);
            m.ptr(base + OBJECT_ARRAY_TYPE_INFO_RVA, objects_class);
            m.ptr(objects + MANAGED_ARRAY_LENGTH, 32);
            m.ptr(objects + MANAGED_ARRAY_VECTOR, attr);
            m.ptr(attr, attr_class);
            m.ptr(base + ZLIST_ATTR_LONG_TYPE_INFO_RVA, attr_class);
            m.put(attr + ZATTR_IS_DEFAULT, &[0]);
            m.ptr(attr + ZATTR_VALUE, teammate_list);
            m.ptr(teammate_list, list_class);
            m.ptr(base + ZLIST_LONG_TYPE_INFO_RVA, list_class);
            m.ptr(teammate_list + ZLIST_ITEMS, teammate_items);
            m.put(teammate_list + ZLIST_SIZE, &1i32.to_le_bytes());
            m.ptr(teammate_items, long_array_class);
            m.ptr(base + LONG_ARRAY_TYPE_INFO_RVA, long_array_class);
            m.ptr(teammate_items + MANAGED_ARRAY_LENGTH, 1);
            m.put(
                teammate_items + MANAGED_ARRAY_VECTOR,
                &char_id.to_le_bytes(),
            );

            let player_loop_class = 0x4A_0000;
            let player_loop_static_fields = 0x4B_0000;
            let synchronization_context = 0x4C_0000;
            let synchronization_context_class = 0x4C_1000;
            let yielders = 0x4D_0000;
            let yielders_class = 0x4E_0000;
            let update_queue = 0x4F_0000;
            let update_queue_class = 0x51_0000;
            let action_list = 0x52_0000;
            let waiting_list = 0x53_0000;
            let action_array_class = 0x54_0000;
            m.ptr(base + PLAYER_LOOP_HELPER_TYPE_INFO_RVA, player_loop_class);
            m.ptr(player_loop_class + IL2CPP_CLASS_NAME, 0xA7_0000);
            m.ptr(player_loop_class + IL2CPP_CLASS_NAMESPACE, 0xA7_0100);
            m.text(0xA7_0000, "PlayerLoopHelper");
            m.text(0xA7_0100, "Cysharp.Threading.Tasks");
            m.ptr(
                player_loop_class + IL2CPP_CLASS_STATIC_FIELDS,
                player_loop_static_fields,
            );
            m.put(
                player_loop_static_fields + PLAYER_LOOP_HELPER_MAIN_THREAD_ID,
                &1234i32.to_le_bytes(),
            );
            m.ptr(
                player_loop_static_fields + PLAYER_LOOP_HELPER_SYNCHRONIZATION_CONTEXT,
                synchronization_context,
            );
            m.ptr(synchronization_context, synchronization_context_class);
            m.ptr(
                base + UNITY_SYNCHRONIZATION_CONTEXT_TYPE_INFO_RVA,
                synchronization_context_class,
            );
            m.ptr(
                player_loop_static_fields + PLAYER_LOOP_HELPER_YIELDERS,
                yielders,
            );
            m.ptr(yielders, yielders_class);
            m.ptr(
                base + CONTINUATION_QUEUE_ARRAY_TYPE_INFO_RVA,
                yielders_class,
            );
            m.put(yielders_class + IL2CPP_CLASS_RANK, &[1]);
            m.put(
                yielders_class + IL2CPP_CLASS_ELEMENT_SIZE,
                &(size_of::<usize>() as i32).to_le_bytes(),
            );
            m.ptr(yielders + MANAGED_ARRAY_LENGTH, 16);
            m.ptr(
                yielders + MANAGED_ARRAY_VECTOR + UPDATE_PLAYER_LOOP_TIMING * size_of::<usize>(),
                update_queue,
            );
            m.ptr(update_queue, update_queue_class);
            m.ptr(base + CONTINUATION_QUEUE_TYPE_INFO_RVA, update_queue_class);
            m.ptr(update_queue_class + IL2CPP_CLASS_NAME, 0xA7_0200);
            m.ptr(update_queue_class + IL2CPP_CLASS_NAMESPACE, 0xA7_0300);
            m.text(0xA7_0200, "ContinuationQueue");
            m.text(0xA7_0300, "Cysharp.Threading.Tasks.Internal");
            m.put(
                update_queue + CONTINUATION_QUEUE_TIMING,
                &(UPDATE_PLAYER_LOOP_TIMING as i32).to_le_bytes(),
            );
            m.put(update_queue + CONTINUATION_QUEUE_DEQUEUING, &[0]);
            m.put(
                update_queue + CONTINUATION_QUEUE_ACTION_LIST_COUNT,
                &0i32.to_le_bytes(),
            );
            m.ptr(update_queue + CONTINUATION_QUEUE_ACTION_LIST, action_list);
            m.put(
                update_queue + CONTINUATION_QUEUE_WAITING_LIST_COUNT,
                &0i32.to_le_bytes(),
            );
            m.ptr(update_queue + CONTINUATION_QUEUE_WAITING_LIST, waiting_list);
            m.ptr(base + SYSTEM_ACTION_ARRAY_TYPE_INFO_RVA, action_array_class);
            m.ptr(action_list, action_array_class);
            m.ptr(waiting_list, action_array_class);
            m.ptr(action_list + MANAGED_ARRAY_LENGTH, 16);
            m.ptr(waiting_list + MANAGED_ARRAY_LENGTH, 16);
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
        fn proves_only_a_stable_idle_standard_dungeon_stage() {
            let memory = valid_memory();
            let first = read_dungeon_stage_sample(&memory, 0x10_0000);
            let second = read_dungeon_stage_sample(&memory, 0x10_0000);
            let gate = dungeon_stage_gate(&first, &second);
            assert!(gate.proven);
            assert_eq!(gate.reason, "proven-read-only-current-dungeon-stage");
        }

        #[test]
        fn proves_only_a_stable_bounded_update_scheduler() {
            assert_eq!(SYSTEM_ACTION_ARRAY_TYPE_INFO_RVA, 0x959_97C8);
            assert_eq!(UNITY_SYNCHRONIZATION_CONTEXT_TYPE_INFO_RVA, 0x955_9498);
            assert_eq!(CONTINUATION_QUEUE_TYPE_INFO_RVA, 0x959_13E0);
            assert_eq!(CONTINUATION_QUEUE_ARRAY_TYPE_INFO_RVA, 0x959_13E8);
            let memory = valid_memory();
            let first = read_main_thread_scheduler_sample(&memory, 0x10_0000);
            let second = read_main_thread_scheduler_sample(&memory, 0x10_0000);
            let gate = main_thread_scheduler_gate(&first, &second);
            assert!(gate.proven);
            assert_eq!(gate.reason, "proven-read-only-main-thread-scheduler-state");
            let sample = first.unwrap();
            assert_eq!(sample.main_thread_id, 1234);
            assert_eq!(sample.update_timing, UPDATE_PLAYER_LOOP_TIMING as i32);
            assert_eq!(sample.action_list_count, 0);
            assert_eq!(sample.waiting_list_count, 0);
        }

        #[test]
        fn scheduler_rejects_zero_thread_id_null_context_and_short_yielders() {
            let base = 0x10_0000;
            let mut zero_thread = valid_memory();
            zero_thread.put(
                0x4B_0000 + PLAYER_LOOP_HELPER_MAIN_THREAD_ID,
                &0i32.to_le_bytes(),
            );
            assert!(read_main_thread_scheduler_sample(&zero_thread, base).is_err());

            let mut null_context = valid_memory();
            null_context.ptr(0x4B_0000 + PLAYER_LOOP_HELPER_SYNCHRONIZATION_CONTEXT, 0);
            assert!(read_main_thread_scheduler_sample(&null_context, base).is_err());

            let mut short_yielders = valid_memory();
            short_yielders.ptr(0x4D_0000 + MANAGED_ARRAY_LENGTH, UPDATE_PLAYER_LOOP_TIMING);
            assert!(read_main_thread_scheduler_sample(&short_yielders, base).is_err());
        }

        #[test]
        fn scheduler_rejects_wrong_queue_identity_timing_and_array_type() {
            let base = 0x10_0000;
            let mut wrong_class = valid_memory();
            wrong_class.text(0xA7_0200, "WrongQueue");
            assert!(read_main_thread_scheduler_sample(&wrong_class, base).is_err());

            let mut wrong_timing = valid_memory();
            wrong_timing.put(0x4F_0000 + CONTINUATION_QUEUE_TIMING, &7i32.to_le_bytes());
            assert!(read_main_thread_scheduler_sample(&wrong_timing, base).is_err());

            let mut wrong_array = valid_memory();
            wrong_array.ptr(0x52_0000, 0x55_0000);
            assert!(read_main_thread_scheduler_sample(&wrong_array, base).is_err());

            let mut wrong_sync_context_type = valid_memory();
            wrong_sync_context_type.ptr(0x4C_0000, 0x55_0000);
            assert!(read_main_thread_scheduler_sample(&wrong_sync_context_type, base).is_err());

            let mut wrong_yielders_type = valid_memory();
            wrong_yielders_type.ptr(0x4D_0000, 0x55_0000);
            assert!(read_main_thread_scheduler_sample(&wrong_yielders_type, base).is_err());

            let mut wrong_queue_type = valid_memory();
            wrong_queue_type.ptr(0x4F_0000, 0x55_0000);
            assert!(read_main_thread_scheduler_sample(&wrong_queue_type, base).is_err());
        }

        #[test]
        fn scheduler_rejects_inconsistent_or_unbounded_queue_counts() {
            let base = 0x10_0000;
            let mut excessive_count = valid_memory();
            excessive_count.put(
                0x4F_0000 + CONTINUATION_QUEUE_ACTION_LIST_COUNT,
                &17i32.to_le_bytes(),
            );
            assert!(read_main_thread_scheduler_sample(&excessive_count, base).is_err());

            let mut excessive_capacity = valid_memory();
            excessive_capacity.ptr(
                0x53_0000 + MANAGED_ARRAY_LENGTH,
                MAX_REVIEWED_SCHEDULER_QUEUE_CAPACITY + 1,
            );
            assert!(read_main_thread_scheduler_sample(&excessive_capacity, base).is_err());
        }

        #[test]
        fn scheduler_gate_rejects_a_torn_second_read() {
            let memory = valid_memory();
            let first = read_main_thread_scheduler_sample(&memory, 0x10_0000).unwrap();
            let mut second = first;
            second.waiting_list_count = 1;
            let gate = main_thread_scheduler_gate(&Ok(first), &Ok(second));
            assert!(!gate.proven);
            assert_eq!(
                gate.reason,
                "unstable-read-only-main-thread-scheduler-state"
            );
        }

        #[test]
        fn reviewed_code_hashes_bounded_regions_larger_than_one_memory_read() {
            let module_base = 0x10_0000;
            let rva = 0x1000;
            let bytes: Vec<u8> = (0..700).map(|index| (index % 251) as u8).collect();
            let expected = format!("{:x}", Sha256::digest(&bytes));
            let mut memory = FakeMemory::new();
            memory.put(module_base + rva, &bytes);
            let code = reviewed_code_receipt(
                &memory,
                module_base,
                rva + bytes.len(),
                ReviewedCodeSpec {
                    identity: "chunked-test",
                    rva_text: "0x1000",
                    rva,
                    byte_length: bytes.len(),
                    expected_sha256: Box::leak(expected.clone().into_boxed_str()),
                },
            );
            assert!(code.matches_reviewed_image);
            let mut corrupted = memory;
            corrupted.put(module_base + rva + REVIEWED_CODE_READ_CHUNK + 7, &[0xFF]);
            let changed = reviewed_code_receipt(
                &corrupted,
                module_base,
                rva + bytes.len(),
                ReviewedCodeSpec {
                    identity: "chunked-test",
                    rva_text: "0x1000",
                    rva,
                    byte_length: bytes.len(),
                    expected_sha256: Box::leak(expected.into_boxed_str()),
                },
            );
            assert!(!changed.matches_reviewed_image);
        }

        #[test]
        fn proves_marker_one_slot_and_control_data_without_invoking_game_code() {
            let memory = valid_memory();
            let roots = acquire_roots(&memory, 0x10_0000).unwrap();
            let first = read_marker_skill_resolution_sample(&memory, &roots);
            let second = read_marker_skill_resolution_sample(&memory, &roots);
            let gate = marker_skill_resolution_gate(&first, &second);
            assert!(gate.proven);
            assert_eq!(
                gate.reason,
                "proven-read-only-marker-1-slot-and-skill-resolution"
            );
            let sample = first.unwrap();
            assert_eq!(sample.resolved_slot_skill_id, MARKER_1_SKILL_ID);
            assert_eq!(sample.resolved_control_skill_id, MARKER_1_SKILL_ID);
        }

        #[test]
        fn proves_party_leadership_from_stable_attr_151_first_member() {
            let memory = valid_memory();
            let roots = acquire_roots(&memory, 0x10_0000).unwrap();
            let first = read_party_leader_sample(&memory, 0x10_0000, &roots);
            let second = read_party_leader_sample(&memory, 0x10_0000, &roots);
            let gate = party_leader_gate(&first, &second);
            assert!(gate.proven);
            assert_eq!(
                gate.reason,
                "proven-read-only-current-player-is-party-leader"
            );
            let sample = first.unwrap();
            assert_eq!(sample.current_char_id, 7_654_321);
            assert_eq!(sample.leader_char_id, sample.current_char_id);
            assert_eq!(sample.teammate_count, 1);
        }

        #[test]
        fn party_leader_gate_rejects_a_nonleader_without_exposing_ids() {
            let mut memory = valid_memory();
            memory.put(
                0x9C_0000 + MANAGED_ARRAY_VECTOR,
                &8_765_432i64.to_le_bytes(),
            );
            let roots = acquire_roots(&memory, 0x10_0000).unwrap();
            let first = read_party_leader_sample(&memory, 0x10_0000, &roots);
            let second = read_party_leader_sample(&memory, 0x10_0000, &roots);
            let gate = party_leader_gate(&first, &second);
            assert!(!gate.proven);
            assert_eq!(gate.reason, "current-player-is-not-party-leader");
        }

        #[test]
        fn party_leader_gate_rejects_an_unstable_double_read() {
            let memory = valid_memory();
            let roots = acquire_roots(&memory, 0x10_0000).unwrap();
            let first = read_party_leader_sample(&memory, 0x10_0000, &roots);
            let mut second = first;
            second.as_mut().unwrap().leader_char_id += 1;
            let gate = party_leader_gate(&first, &second);
            assert!(!gate.proven);
            assert_eq!(gate.reason, "unstable-read-only-party-leader-state");
        }

        #[test]
        fn party_leader_lookup_rejects_wrong_exact_attr_type() {
            let mut memory = valid_memory();
            memory.ptr(0x9A_0000, 0x49_0000);
            let roots = acquire_roots(&memory, 0x10_0000).unwrap();
            assert_eq!(
                read_party_leader_sample(&memory, 0x10_0000, &roots),
                Err(AcquireError::Identity)
            );
        }

        #[test]
        fn party_leader_lookup_rejects_missing_or_duplicate_attr_151() {
            let mut missing = valid_memory();
            let start_segment =
                ((LOCAL_TEAMMATE_LIST_KEY >> 3) as usize) & (ATTR_SEGMENT_COUNT - 1);
            missing.put(
                0x95_0000 + ATTR_INDEX_PART_START_SEGMENT_MASKS + start_segment * size_of::<u32>(),
                &0u32.to_le_bytes(),
            );
            let roots = acquire_roots(&missing, 0x10_0000).unwrap();
            assert_eq!(
                read_party_leader_sample(&missing, 0x10_0000, &roots),
                Err(AcquireError::Unavailable)
            );

            let mut duplicate = valid_memory();
            duplicate.put(
                0x96_0000 + size_of::<u32>(),
                &LOCAL_TEAMMATE_LIST_KEY.to_le_bytes(),
            );
            duplicate.put(0x96_0000 + ATTR_KEY_SEGMENT_COUNT, &2i32.to_le_bytes());
            duplicate.put(0x97_0000 + size_of::<i32>(), &0i32.to_le_bytes());
            let roots = acquire_roots(&duplicate, 0x10_0000).unwrap();
            assert_eq!(
                read_party_leader_sample(&duplicate, 0x10_0000, &roots),
                Err(AcquireError::Identity)
            );
        }

        #[test]
        fn party_leader_lookup_rejects_active_segments_with_zero_entry_count() {
            let mut memory = valid_memory();
            memory.put(0x95_0000 + ATTR_INDEX_PART_COUNT, &0i32.to_le_bytes());
            let roots = acquire_roots(&memory, 0x10_0000).unwrap();
            assert_eq!(
                read_party_leader_sample(&memory, 0x10_0000, &roots),
                Err(AcquireError::Identity)
            );
        }

        #[test]
        fn marker_skill_resolution_rejects_wrong_slot_mapping() {
            let mut memory = valid_memory();
            let slot_entry = 0x88_0000 + MANAGED_ARRAY_VECTOR;
            memory.put(
                slot_entry + ZDICTIONARY_INT_INT_ENTRY_VALUE,
                &1102i32.to_le_bytes(),
            );
            let roots = acquire_roots(&memory, 0x10_0000).unwrap();
            assert_eq!(
                read_marker_skill_resolution_sample(&memory, &roots),
                Err(MarkerSkillResolutionError::MarkerSlotMapping)
            );
        }

        #[test]
        fn marker_skill_resolution_rejects_duplicate_active_slot_key() {
            let mut memory = valid_memory();
            memory.put(0x86_0000 + ZDICTIONARY_COUNT, &2i32.to_le_bytes());
            memory.ptr(0x88_0000 + MANAGED_ARRAY_LENGTH, 2);
            let duplicate = 0x88_0000 + MANAGED_ARRAY_VECTOR + ZDICTIONARY_INT_INT_ENTRY_STRIDE;
            memory.put(
                duplicate + ZDICTIONARY_ENTRY_HASH_CODE,
                &201i32.to_le_bytes(),
            );
            memory.put(
                duplicate + ZDICTIONARY_ENTRY_KEY,
                &MARKER_1_SLOT_ID.to_le_bytes(),
            );
            memory.put(
                duplicate + ZDICTIONARY_INT_INT_ENTRY_VALUE,
                &MARKER_1_SKILL_ID.to_le_bytes(),
            );
            let roots = acquire_roots(&memory, 0x10_0000).unwrap();
            assert_eq!(
                read_marker_skill_resolution_sample(&memory, &roots),
                Err(MarkerSkillResolutionError::MarkerSlotLookup)
            );
        }

        #[test]
        fn marker_skill_resolution_rejects_mismatched_control_data_identity() {
            let mut memory = valid_memory();
            memory.put(
                0x8A_0000 + SKILL_CONTROL_DATA_SKILL_ID,
                &1102i32.to_le_bytes(),
            );
            let roots = acquire_roots(&memory, 0x10_0000).unwrap();
            assert_eq!(
                read_marker_skill_resolution_sample(&memory, &roots),
                Err(MarkerSkillResolutionError::ControlDataSkillIdentity)
            );
        }

        #[test]
        fn marker_skill_resolution_bounds_dictionary_capacity() {
            let mut memory = valid_memory();
            memory.put(
                0x86_0000 + ZDICTIONARY_COUNT,
                &(MAX_REVIEWED_DICTIONARY_CAPACITY as i32 + 1).to_le_bytes(),
            );
            let roots = acquire_roots(&memory, 0x10_0000).unwrap();
            assert_eq!(
                read_marker_skill_resolution_sample(&memory, &roots),
                Err(MarkerSkillResolutionError::SlotDictionaryShape)
            );
        }

        #[test]
        fn marker_skill_resolution_rejects_wrong_concrete_entry_stride() {
            let mut memory = valid_memory();
            memory.put(
                0x3D_0000 + IL2CPP_CLASS_ELEMENT_SIZE,
                &(ZDICTIONARY_INT_OBJECT_ENTRY_STRIDE as i32).to_le_bytes(),
            );
            let roots = acquire_roots(&memory, 0x10_0000).unwrap();
            assert_eq!(
                read_marker_skill_resolution_sample(&memory, &roots),
                Err(MarkerSkillResolutionError::SlotDictionaryEntryStride)
            );
        }

        #[test]
        fn marker_skill_resolution_rejects_non_array_entry_storage() {
            let mut memory = valid_memory();
            memory.put(0x3D_0000 + IL2CPP_CLASS_RANK, &[0]);
            let roots = acquire_roots(&memory, 0x10_0000).unwrap();
            assert_eq!(
                read_marker_skill_resolution_sample(&memory, &roots),
                Err(MarkerSkillResolutionError::SlotDictionaryEntryArrayClass)
            );
        }

        #[test]
        fn marker_skill_diagnostics_identify_fixed_chain_stages_without_values() {
            let roots = acquire_roots(&valid_memory(), 0x10_0000).unwrap();
            let cases = [
                (
                    MarkerSkillResolutionError::DataManager,
                    "unavailable-or-invalid-marker-skill-data-manager",
                ),
                (
                    MarkerSkillResolutionError::SlotDictionaryPointer,
                    "marker-skill-slot-dictionary-pointer-invalid",
                ),
                (
                    MarkerSkillResolutionError::SlotDictionaryClass,
                    "marker-skill-slot-dictionary-class-invalid",
                ),
                (
                    MarkerSkillResolutionError::SlotDictionaryHeader,
                    "marker-skill-slot-dictionary-header-unavailable",
                ),
                (
                    MarkerSkillResolutionError::SlotDictionaryShape,
                    "marker-skill-slot-dictionary-counts-or-buckets-invalid",
                ),
                (
                    MarkerSkillResolutionError::SlotDictionaryEntryStorage,
                    "marker-skill-slot-dictionary-entry-storage-unavailable",
                ),
                (
                    MarkerSkillResolutionError::SlotDictionaryEntryCapacity,
                    "marker-skill-slot-dictionary-entry-capacity-invalid",
                ),
                (
                    MarkerSkillResolutionError::SlotDictionaryEntryArrayClass,
                    "marker-skill-slot-dictionary-entry-array-class-invalid",
                ),
                (
                    MarkerSkillResolutionError::SlotDictionaryEntryStride,
                    "marker-skill-slot-dictionary-entry-stride-invalid",
                ),
                (
                    MarkerSkillResolutionError::ControlDictionary,
                    "unavailable-or-invalid-marker-skill-control-dictionary",
                ),
                (
                    MarkerSkillResolutionError::MarkerSlotLookup,
                    "marker-1-slot-missing-or-duplicate",
                ),
                (
                    MarkerSkillResolutionError::MarkerSlotMapping,
                    "marker-1-slot-mapping-mismatch",
                ),
                (
                    MarkerSkillResolutionError::ControlDataLookup,
                    "marker-1-control-data-missing-or-duplicate",
                ),
                (
                    MarkerSkillResolutionError::ControlDataClass,
                    "marker-1-control-data-class-invalid",
                ),
                (
                    MarkerSkillResolutionError::ControlDataSkillIdentity,
                    "marker-1-control-data-skill-identity-invalid",
                ),
            ];
            for (error, expected) in cases {
                let gate = marker_skill_resolution_gate(&Err(error), &Err(error));
                assert!(!gate.proven);
                assert_eq!(gate.reason, expected);
                assert!(gate.reason.bytes().all(|byte| byte.is_ascii_lowercase()
                    || byte.is_ascii_digit()
                    || byte == b'-'));
            }

            let valid = read_marker_skill_resolution_sample(&valid_memory(), &roots).unwrap();
            let gate = marker_skill_resolution_gate(
                &Err(MarkerSkillResolutionError::DataManager),
                &Ok(valid),
            );
            assert_eq!(
                gate.reason,
                "unavailable-or-invalid-marker-skill-data-manager"
            );
            let gate = marker_skill_resolution_gate(
                &Err(MarkerSkillResolutionError::SlotDictionaryClass),
                &Err(MarkerSkillResolutionError::ControlDictionary),
            );
            assert_eq!(
                gate.reason,
                "inconsistent-read-only-marker-skill-failure-stage"
            );
        }

        #[test]
        fn marker_skill_diagnostics_distinguish_live_structure_failures() {
            let mut data_mgr = valid_memory();
            data_mgr.text(0xA5_0000, "WrongSkillDataMgr");
            let roots = acquire_roots(&data_mgr, 0x10_0000).unwrap();
            assert_eq!(
                read_marker_skill_resolution_sample(&data_mgr, &roots),
                Err(MarkerSkillResolutionError::DataManager)
            );

            let mut control_dictionary = valid_memory();
            control_dictionary.put(0x87_0000 + ZDICTIONARY_COUNT, &0i32.to_le_bytes());
            let roots = acquire_roots(&control_dictionary, 0x10_0000).unwrap();
            assert_eq!(
                read_marker_skill_resolution_sample(&control_dictionary, &roots),
                Err(MarkerSkillResolutionError::ControlDictionary)
            );

            let mut control_data = valid_memory();
            control_data.text(0xA5_0400, "WrongControlData");
            let roots = acquire_roots(&control_data, 0x10_0000).unwrap();
            assert_eq!(
                read_marker_skill_resolution_sample(&control_data, &roots),
                Err(MarkerSkillResolutionError::ControlDataClass)
            );
        }

        #[test]
        fn marker_skill_gate_rejects_dictionary_version_change() {
            let memory = valid_memory();
            let roots = acquire_roots(&memory, 0x10_0000).unwrap();
            let first = read_marker_skill_resolution_sample(&memory, &roots).unwrap();
            let mut second = first;
            second.slot_dictionary_version += 1;
            let gate = marker_skill_resolution_gate(&Ok(first), &Ok(second));
            assert!(!gate.proven);
            assert_eq!(gate.reason, "unstable-read-only-marker-skill-lifecycle");
        }

        #[test]
        fn rejects_a_loading_or_switching_stage() {
            let mut memory = valid_memory();
            memory.put(0x92_0000 + STAGE_MGR_SWITCH_STATE, &[1]);
            let first = read_dungeon_stage_sample(&memory, 0x10_0000);
            let second = read_dungeon_stage_sample(&memory, 0x10_0000);
            let gate = dungeon_stage_gate(&first, &second);
            assert!(!gate.proven);
            assert_eq!(gate.reason, "stage-is-loading-or-switching");
        }

        #[test]
        fn rejects_a_non_dungeon_stage_class() {
            let mut memory = valid_memory();
            memory.text(0xA4_0700, "StageCity");
            assert_eq!(
                read_dungeon_stage_sample(&memory, 0x10_0000),
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
            options.insert(
                "armed-mode".into(),
                OPERATOR_PLACEMENT_ARMED_MODE_TOKEN.into(),
            );
            assert!(reject_unknown_options(&options).is_ok());
        }

        #[test]
        fn native_dispatch_preflight_is_read_only_and_preset_bound() {
            let mut options = Map::new();
            options.insert("build".into(), BUILD.into());
            options.insert("native-dispatch-preflight".into(), "true".into());
            options.insert("preset-id".into(), "preset-test".into());
            options.insert("rlogs-base-url".into(), "http://127.0.0.1:1".into());
            assert!(reject_unknown_options(&options).is_ok());

            options.insert("target-x".into(), "1".into());
            assert!(reject_unknown_options(&options).is_err());
            options.remove("target-x");
            options.insert("armed-mode".into(), ARMED_MODE_TOKEN.into());
            assert!(reject_unknown_options(&options).is_err());
            options.remove("armed-mode");
            options.remove("preset-id");
            assert!(reject_unknown_options(&options).is_err());
        }

        #[test]
        fn native_dispatch_preset_context_requires_exact_build_family_and_marker_one() {
            let projection: serde_json::Value = serde_json::from_str(
                r#"{"schemaVersion":4,"context":{"clientBuild":"25247556","sceneId":6525,"mapId":6525,"activityFamilyId":"mech-facility"},"presets":[{"presetId":"preset-test","activityFamilyId":"mech-facility","points":[{"markerNumber":1,"x":248.5,"y":118.0,"z":-53.5}]}]}"#,
            )
            .unwrap();
            let context = parse_native_dispatch_preset_context(&projection, "preset-test").unwrap();
            assert_eq!(context.scene_id, 6525);
            assert_eq!(context.map_id, 6525);
            assert_eq!(context.activity_family_id, "mech-facility");
            assert_eq!(context.marker_1_target.x, 248.5);
        }

        #[test]
        fn reviewed_code_evidence_fails_closed_without_exact_loaded_bytes() {
            let memory = valid_memory();
            let code = reviewed_code_receipt(
                &memory,
                0x10_0000,
                usize::MAX - 0x10_0000,
                ReviewedCodeSpec {
                    identity: "Panda.ZGame.EntityAttrExtensions.SetIndicatorPos",
                    rva_text: "0x53E86A0",
                    rva: SET_INDICATOR_POS_RVA,
                    byte_length: SET_INDICATOR_POS_BYTES,
                    expected_sha256: SET_INDICATOR_POS_SHA256,
                },
            );
            assert!(!code.matches_reviewed_image);
            assert!(code.sha256.is_empty());
        }

        #[test]
        fn reviewed_code_evidence_never_reads_past_the_reported_module_image() {
            let module_base = 0x10_0000;
            let rva = 0x1000;
            let bytes = [1u8, 2, 3, 4];
            let expected = "9f64a747e1b97f131fabb6b447296c9b6f0201e79fb3c5356e6c77e89b6a806a";
            let mut memory = FakeMemory::new();
            memory.put(module_base + rva, &bytes);

            let outside = reviewed_code_receipt(
                &memory,
                module_base,
                rva + bytes.len() - 1,
                ReviewedCodeSpec {
                    identity: "test",
                    rva_text: "0x1000",
                    rva,
                    byte_length: bytes.len(),
                    expected_sha256: expected,
                },
            );
            assert!(outside.sha256.is_empty());
            assert!(!outside.matches_reviewed_image);

            let inside = reviewed_code_receipt(
                &memory,
                module_base,
                rva + bytes.len(),
                ReviewedCodeSpec {
                    identity: "test",
                    rva_text: "0x1000",
                    rva,
                    byte_length: bytes.len(),
                    expected_sha256: expected,
                },
            );
            assert_eq!(inside.sha256, expected);
            assert!(inside.matches_reviewed_image);
        }

        fn observed_evidence(
            revision: u64,
            request_count: u32,
            request_micros: Option<u64>,
            point: Option<(u64, Position)>,
        ) -> ObservedMarkerEvidence {
            ObservedMarkerEvidence {
                schema_version: 3,
                revision,
                capture_active: true,
                protocol_supported: true,
                request_observer_supported: true,
                verified_request_count: request_count,
                last_verified_request_marker_number: request_micros.map(|_| 1),
                last_verified_request_observed_micros: request_micros,
                verified_requests: request_micros
                    .map(|observed_micros| {
                        vec![ObservedRequestEvidence {
                            marker_number: 1,
                            observed_micros,
                        }]
                    })
                    .unwrap_or_default(),
                reason: if point.is_some() {
                    "observed_markers_available"
                } else {
                    "no_fully_positioned_markers_observed"
                }
                .into(),
                session_id: Some("session-test".into()),
                deployment_id: Some("deployment-test".into()),
                client_build: Some(BUILD.into()),
                protocol_pack_digest: Some("digest-test".into()),
                scene_id: Some(1633),
                map_id: Some(1633),
                observed_micros: Some(point.as_ref().map_or(1, |value| value.0)),
                markers: point
                    .map(|(observed_micros, position)| {
                        vec![ObservedPointEvidence {
                            marker_number: 1,
                            x: position.x,
                            y: position.y,
                            z: position.z,
                            observed_micros,
                        }]
                    })
                    .unwrap_or_default(),
            }
        }

        #[test]
        fn placement_evidence_requires_new_outbound_then_new_nearby_inbound() {
            let target = Position {
                x: 10.0,
                y: 20.0,
                z: 30.0,
            };
            let baseline = observed_evidence(5, 2, Some(100), Some((90, target.clone())));
            let outbound = observed_evidence(6, 3, Some(200), Some((90, target.clone())));
            assert_eq!(newer_marker_1_outbound(&baseline, &outbound), Some(200));
            assert!(newer_marker_1_inbound(&baseline, &outbound, 200, &target).is_none());
            let acknowledged = observed_evidence(
                7,
                3,
                Some(200),
                Some((
                    250,
                    Position {
                        x: 10.02,
                        ..target.clone()
                    },
                )),
            );
            let (_, distance) =
                newer_marker_1_inbound(&baseline, &acknowledged, 200, &target).unwrap();
            assert!(distance <= PLACEMENT_COORDINATE_TOLERANCE);
            let too_far = observed_evidence(
                7,
                3,
                Some(200),
                Some((
                    250,
                    Position {
                        x: 10.2,
                        ..target.clone()
                    },
                )),
            );
            assert!(newer_marker_1_inbound(&baseline, &too_far, 200, &target).is_none());
        }

        #[test]
        fn placement_evidence_rejects_stale_or_wrong_order_observations() {
            let target = Position {
                x: 1.0,
                y: 2.0,
                z: 3.0,
            };
            let baseline = observed_evidence(5, 2, Some(100), None);
            let stale_request = observed_evidence(6, 3, Some(100), None);
            assert!(newer_marker_1_outbound(&baseline, &stale_request).is_none());
            let before_request = observed_evidence(7, 3, Some(200), Some((199, target.clone())));
            assert!(newer_marker_1_inbound(&baseline, &before_request, 200, &target).is_none());
            let mut changed_session = observed_evidence(7, 3, Some(200), Some((250, target)));
            changed_session.session_id = Some("other-session".into());
            assert!(!same_observed_identity(&baseline, &changed_session));
        }

        #[test]
        fn placement_evidence_binds_capture_deployment_digest_scene_and_map() {
            assert!(capture_session_matches_map(
                "session-test",
                Some("session-test")
            ));
            assert!(!capture_session_matches_map("session-test", Some("other")));
            assert!(!capture_session_matches_map("session-test", None));
            let live = LivePlayerContext {
                identity: LiveMapIdentity {
                    session_id: "session-test".into(),
                    deployment_id: "deployment-test".into(),
                    client_build: BUILD.into(),
                    protocol_pack_digest: "digest-test".into(),
                    scene_id: 1633,
                    map_id: 1633,
                    local_actor_id: 7,
                    activity_family_id: "dungeon.1633".into(),
                },
                origin: Position {
                    x: 1.0,
                    y: 2.0,
                    z: 3.0,
                },
            };
            let baseline = observed_evidence(5, 2, Some(100), None);
            assert!(observed_evidence_matches_live(&baseline, &live));
            let mut changed = baseline.clone();
            changed.deployment_id = Some("other".into());
            assert!(!observed_evidence_matches_live(&changed, &live));
            let mut changed = baseline.clone();
            changed.protocol_pack_digest = Some("other".into());
            assert!(!observed_evidence_matches_live(&changed, &live));
            let mut changed = baseline.clone();
            changed.scene_id = Some(6515);
            assert!(!observed_evidence_matches_live(&changed, &live));
        }

        #[test]
        fn operator_canary_has_observation_but_no_click_emission_primitive() {
            let source = include_str!("automarker-lifecycle-probe.rs");
            assert!(source.contains(OPERATOR_PLACEMENT_ARMED_MODE_TOKEN));
            for forbidden in [
                ["MOUSEEVENTF_", "LEFTDOWN"].concat(),
                ["MOUSEEVENTF_", "LEFTUP"].concat(),
            ] {
                assert!(!source.contains(&forbidden));
            }
            let receipt = OperatorPlacementReceipt {
                human_click_observed: true,
                programmatic_click_emitted: false,
                injected_click_observed: false,
                other_click_observed: false,
                outbound_marker_1_newer: true,
                outbound_observed_micros: Some(200),
                inbound_marker_1_newer: true,
                inbound_observed_micros: Some(250),
                inbound_target_distance: Some(0.01),
                context_continuous: true,
                timed_out: false,
                escape_emitted: false,
                outcome: "passed",
            };
            let json = serde_json::to_string(&receipt).unwrap();
            assert!(json.contains("\"human_click_observed\":true"));
            assert!(json.contains("\"programmatic_click_emitted\":false"));
            for forbidden in ["session", "deployment", "digest", "account", "pointer"] {
                assert!(!json.contains(forbidden));
            }
            assert!(should_escape_after_observed_click(&receipt, true));
            assert!(!should_escape_after_observed_click(&receipt, false));
            let mut no_click = receipt.clone();
            no_click.human_click_observed = false;
            assert!(!any_click_observed(&no_click));
            assert!(!should_escape_after_observed_click(&no_click, true));
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
        fn planner_receipt_retains_only_bounded_static_live_context_reason() {
            let request = PlannerCanaryRequest {
                mode: PlannerCanaryMode::SingleStep,
                target: Position {
                    x: 1.0,
                    y: 2.0,
                    z: 3.0,
                },
                rlogs_base_url: "http://127.0.0.1:7419".into(),
            };
            let mut receipt = empty_planner_receipt(&request, "live-player-context-unavailable");
            receipt.live_context_failure_reason = Some("mechanics-map-not-fresh");
            let json = serde_json::to_value(receipt).unwrap();
            assert_eq!(
                json.get("live_context_failure_reason")
                    .and_then(serde_json::Value::as_str),
                Some("mechanics-map-not-fresh")
            );
            let encoded = json.to_string();
            assert!(!encoded.contains("127.0.0.1"));
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
            let before = MouseObservationCounts {
                own: 4,
                foreign: 0,
                ..Default::default()
            };
            assert!(input_ownership_verified(
                before,
                MouseObservationCounts {
                    own: 5,
                    foreign: 0,
                    ..Default::default()
                }
            ));
            assert!(!input_ownership_verified(
                before,
                MouseObservationCounts {
                    own: 4,
                    foreign: 0,
                    ..Default::default()
                }
            ));
            assert!(!input_ownership_verified(
                before,
                MouseObservationCounts {
                    own: 6,
                    foreign: 0,
                    ..Default::default()
                }
            ));
            assert!(!input_ownership_verified(
                before,
                MouseObservationCounts {
                    own: 5,
                    foreign: 1,
                    ..Default::default()
                }
            ));
            assert!(!input_ownership_verified(
                before,
                MouseObservationCounts {
                    own: 5,
                    human_left_clicks: 1,
                    ..Default::default()
                }
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
                            r#"{"context":{"clientBuild":"25247556","sceneId":1633,"mapId":1633,"activityFamilyId":"dungeon.1633"},"captureSessionId":"s","deploymentId":"global-steam","protocolPackDigest":"digest"}"#
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
            assert_eq!(value.identity.deployment_id, "global-steam");
            assert_eq!(value.identity.protocol_pack_digest, "digest");
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
                    deployment_id: "deployment".to_owned(),
                    client_build: BUILD.to_owned(),
                    protocol_pack_digest: "digest".to_owned(),
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
            let mut changed_deployment = expected.clone();
            changed_deployment.identity.deployment_id = "other-deployment".into();
            assert!(!live_context_matches(
                &expected,
                &changed_deployment,
                &Position {
                    x: 2.0,
                    y: 2.0,
                    z: 3.0
                }
            ));
            let mut changed_digest = expected.clone();
            changed_digest.identity.protocol_pack_digest = "other-digest".into();
            assert!(!live_context_matches(
                &expected,
                &changed_digest,
                &Position {
                    x: 2.0,
                    y: 2.0,
                    z: 3.0
                }
            ));
        }

        #[test]
        fn loopback_http_rejects_chunked_and_trailing_response_bytes() {
            use std::net::{Shutdown, TcpListener};

            fn serve_once(response: &'static [u8]) -> u16 {
                let listener = TcpListener::bind("127.0.0.1:0").unwrap();
                let port = listener.local_addr().unwrap().port();
                thread::spawn(move || {
                    let (mut stream, _) = listener.accept().unwrap();
                    let mut request = [0u8; 1024];
                    let _ = stream.read(&mut request).unwrap();
                    stream.write_all(response).unwrap();
                    stream.shutdown(Shutdown::Write).unwrap();
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
