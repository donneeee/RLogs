//! Exact-build, out-of-process observer for the normal indicator lifecycle.
//!
//! The observer opens one already-running client with query/read rights only.
//! It follows one compile-time allowlisted IL2CPP object chain and samples six
//! reviewed fields. It cannot write, invoke game code, debug, inject, suspend,
//! place markers, inspect packets, or scan/dump process memory.

#[cfg(windows)]
mod windows {
    use std::{
        collections::BTreeMap,
        env,
        error::Error,
        ffi::{OsString, c_void},
        fs::{self, File},
        io::Read,
        mem::size_of,
        os::windows::ffi::OsStringExt,
        path::{Path, PathBuf},
        thread,
        time::{Duration, Instant, SystemTime, UNIX_EPOCH},
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
                OpenProcess, PROCESS_NAME_WIN32, PROCESS_QUERY_INFORMATION, PROCESS_VM_READ,
                QueryFullProcessImageNameW,
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
    const SKILL_SLOT_ID: usize = 0x18;
    const SKILL_LAST_USED_SLOT_ID: usize = 0x1C;
    const SKILL_LAST_PRESS_SLOT_ID: usize = 0x20;
    const SKILL_IS_PRESS: usize = 0x38;
    const MAX_C_STRING_BYTES: usize = 96;
    const MIN_USER_ADDRESS: usize = 0x1_0000;
    const MAX_USER_ADDRESS: usize = 0x0000_7fff_ffff_ffff;

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
        validated_classes: [&'static str; 6],
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
        packets_observed_or_modified: bool,
        place_enabled: bool,
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

    pub fn main() -> Result<(), Box<dyn Error>> {
        let options = parse_options(env::args().skip(1))?;
        reject_unknown_options(&options)?;
        let output = PathBuf::from(required(&options, "output")?);
        if output.exists() {
            return Err("refusing to overwrite the receipt target".into());
        }
        let duration_millis = numeric_option(&options, "duration-ms", 15_000, 100, 60_000)?;
        let interval_millis = numeric_option(&options, "interval-ms", 10, 5, 1_000)?;
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

        let handle =
            unsafe { OpenProcess(PROCESS_VM_READ | PROCESS_QUERY_INFORMATION, 0, process_id) };
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

        let (events, counters) = observe(
            &ProcessMemory(handle.0),
            module.base,
            Duration::from_millis(duration_millis),
            Duration::from_millis(interval_millis),
        );
        let receipt = Receipt {
            schema_version: 1,
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
                packets_observed_or_modified: false,
                place_enabled: false,
            },
        };
        if let Some(parent) = output.parent() {
            fs::create_dir_all(parent)?;
        }
        let encoded = serde_json::to_vec_pretty(&receipt)?;
        fs::write(&output, [&encoded[..], b"\n"].concat())?;
        println!(
            "sanitized lifecycle receipt written: {} transitions, Place disabled",
            receipt.summary.emitted_transitions
        );
        Ok(())
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
        let before = acquire_roots(memory, module_base)?;
        let first_state = read_state(memory, &before)?;
        let second_state = read_state(memory, &before)?;
        let after = acquire_roots(memory, module_base)?;
        if before != after || first_state != second_state {
            return Err(AcquireError::Torn);
        }
        Ok(first_state)
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
        })
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
        Ok(LifecycleState {
            slot_id,
            last_used_slot_id,
            last_press_slot_id,
            is_press,
            indicator_pos: position,
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
        const ALLOWED: [&str; 6] = [
            "build",
            "process-executable",
            "game-assembly",
            "steam-manifest",
            "output",
            "duration-ms",
        ];
        for key in options.keys() {
            if key != "interval-ms" && !ALLOWED.contains(&key.as_str()) {
                return Err("unknown option".into());
            }
        }
        if required(options, "build")? != BUILD {
            return Err(format!("this probe supports exact build {BUILD} only").into());
        }
        Ok(())
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
                packets_observed_or_modified: false,
                place_enabled: false,
            };
            let json = serde_json::to_string(&policy).unwrap();
            assert!(!json.contains("pid"));
            assert!(!json.contains("address_hex"));
            assert!(!json.contains("path\""));
            assert!(json.contains("\"place_enabled\":false"));
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
