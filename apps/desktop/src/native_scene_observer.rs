//! Exact-build, out-of-process scene identity observer.
//!
//! This module is deliberately presentation-only. It can open the selected
//! game process with query/read rights, but has no handle or API capable of
//! writing memory, invoking game code, or producing canonical events.

use std::{
    ffi::{OsString, c_void},
    fs::File,
    io::Read,
    mem::size_of,
    os::windows::ffi::OsStringExt,
    path::Path,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    thread::{self, JoinHandle},
    time::Duration,
};

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
        Memory::{MEM_COMMIT, MEMORY_BASIC_INFORMATION, PAGE_GUARD, PAGE_NOACCESS, VirtualQueryEx},
        Threading::{OpenProcess, PROCESS_QUERY_INFORMATION, PROCESS_VM_READ},
    },
};

pub(crate) const REVIEWED_BUILD: &str = "25247556";
const PROCESS_NAME: &str = "BPSR_STEAM.exe";
const EXECUTABLE_BYTES: u64 = 808_496;
const EXECUTABLE_SHA256: &str = "90537dbd0e4c9d4b3ed2af06aa8bf7ffe7219fc94bb2f6963673f7b340288588";
const ASSEMBLY_BYTES: u64 = 218_074_672;
const ASSEMBLY_SHA256: &str = "4a079aec0a3e51a8023aa86bbd152e12068907b65b9aafb355d20bb6d6c41fe3";

const SCENE_MGR_METHOD_INFO_RVA: usize = 0x95D_6650;
const SCENE_MGR_TYPE_INFO_RVA: usize = 0x95D_6658;
const STAGE_MGR_METHOD_INFO_RVA: usize = 0x95D_6B50;
const STAGE_MGR_TYPE_INFO_RVA: usize = 0x95D_6B68;
const ENTITY_MGR_METHOD_INFO_RVA: usize = 0x960_BF90;
const ENTITY_MGR_TYPE_INFO_RVA: usize = 0x960_BFF8;
const CHAR_SERIALIZE_TYPE_INFO_RVA: usize = 0x956_55C8;
const SCENE_DATA_TYPE_INFO_RVA: usize = 0x961_0248;

const METHOD_INFO_NAME: usize = 0x18;
const METHOD_INFO_KLASS: usize = 0x20;
const IL2CPP_CLASS_NAME: usize = 0x10;
const IL2CPP_CLASS_NAMESPACE: usize = 0x18;
const IL2CPP_CLASS_STATIC_FIELDS: usize = 0xB8;
const IL2CPP_CLASS_GENERIC_CONTEXT: usize = 0xC0;
const GENERIC_CONTEXT_INFLATED_CLASS: usize = 0x10;

const SCENE_MGR_SCENE_ID: usize = 0x10;
const SCENE_MGR_TO_SCENE_ID: usize = 0x14;
const SCENE_MGR_SCENE_SUB_TYPE: usize = 0x18;
const STAGE_MGR_SWITCH_STATE: usize = 0x18;
const STAGE_MGR_CURRENT_STAGE: usize = 0x20;
const STAGE_BASE_STAGE_TYPE: usize = 0x10;
const ENTITY_MGR_PLAYER_ENT: usize = 0x18;
const PLAYER_ENT_PURE_COMPONENTS: usize = 0x20;
const PLAYER_STORAGE_CHAR_DATA: usize = 0x158;
const CHAR_SERIALIZE_SCENE_DATA: usize = 0x20;
const SCENE_DATA_MAP_ID: usize = 0x10;

const DUNGEON_STAGE_TYPE: u8 = 5;
const DUNGEON_SCENE_SUB_TYPE: i32 = 5;
const MAX_C_STRING_BYTES: usize = 96;
const MIN_USER_ADDRESS: usize = 0x1_0000;
const MAX_USER_ADDRESS: usize = 0x0000_7FFF_FFFF_FFFF;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct NativeSceneIdentity {
    pub scene_id: i32,
    pub map_id: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum NativeSceneUpdate {
    Stable(NativeSceneIdentity),
    Unavailable,
}

pub(crate) struct NativeSceneObserver {
    stop: Arc<AtomicBool>,
    worker: Option<JoinHandle<()>>,
}

impl NativeSceneObserver {
    pub(crate) fn spawn(
        selected_process_id: Option<u32>,
        client_build: &str,
        mut publish: impl FnMut(NativeSceneUpdate) + Send + 'static,
    ) -> Option<Self> {
        if client_build != REVIEWED_BUILD {
            return None;
        }
        // Establish every identity and read-only access gate before claiming
        // that a native presentation source is active. A missing/mismatched
        // client must leave packet presentation untouched.
        let (memory, module_base) = open_reviewed_process(selected_process_id)?;
        let stop = Arc::new(AtomicBool::new(false));
        let worker_stop = Arc::clone(&stop);
        let worker = thread::Builder::new()
            .name("rlogs-native-scene-observer".into())
            .spawn(move || {
                publish(NativeSceneUpdate::Unavailable);
                let mut last = NativeSceneUpdate::Unavailable;
                while !worker_stop.load(Ordering::Acquire) {
                    let first = read_scene_sample(&memory, module_base);
                    if wait_or_stopped(&worker_stop, Duration::from_millis(50)) {
                        break;
                    }
                    let second = read_scene_sample(&memory, module_base);
                    let next = stable_identity(first, second)
                        .map(NativeSceneUpdate::Stable)
                        .unwrap_or(NativeSceneUpdate::Unavailable);
                    if next != last {
                        publish(next);
                        last = next;
                    }
                    if wait_or_stopped(&worker_stop, Duration::from_millis(150)) {
                        break;
                    }
                }
                if last != NativeSceneUpdate::Unavailable {
                    publish(NativeSceneUpdate::Unavailable);
                }
            })
            .ok()?;
        Some(Self {
            stop,
            worker: Some(worker),
        })
    }
}

impl Drop for NativeSceneObserver {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

fn wait_or_stopped(stop: &AtomicBool, duration: Duration) -> bool {
    let steps = (duration.as_millis() / 10).max(1);
    for _ in 0..steps {
        if stop.load(Ordering::Acquire) {
            return true;
        }
        thread::sleep(Duration::from_millis(10));
    }
    stop.load(Ordering::Acquire)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct SceneSample {
    scene_mgr: usize,
    stage_mgr: usize,
    current_stage: usize,
    entity_mgr: usize,
    player_ent: usize,
    pure_components: usize,
    char_serialize: usize,
    scene_data: usize,
    scene_id: i32,
    map_id: u32,
    to_scene_id: i32,
    scene_sub_type: i32,
    switch_state: u8,
    stage_type: u8,
}

fn stable_identity(
    first: Option<SceneSample>,
    second: Option<SceneSample>,
) -> Option<NativeSceneIdentity> {
    let sample = first.filter(|first| Some(*first) == second)?;
    (sample.switch_state == 0
        && sample.stage_type == DUNGEON_STAGE_TYPE
        && sample.scene_sub_type == DUNGEON_SCENE_SUB_TYPE
        && sample.to_scene_id == 0
        && sample.scene_id > 0
        && u32::try_from(sample.scene_id).ok() == Some(sample.map_id))
    .then_some(NativeSceneIdentity {
        scene_id: sample.scene_id,
        map_id: sample.map_id,
    })
}

fn read_scene_sample(memory: &ProcessMemory, module_base: usize) -> Option<SceneSample> {
    let scene_mgr = acquire_singleton(
        memory,
        module_base,
        SCENE_MGR_METHOD_INFO_RVA,
        SCENE_MGR_TYPE_INFO_RVA,
        "SceneMgr",
        "Panda",
    )?;
    let stage_mgr = acquire_singleton(
        memory,
        module_base,
        STAGE_MGR_METHOD_INFO_RVA,
        STAGE_MGR_TYPE_INFO_RVA,
        "StageMgr",
        "Panda",
    )?;
    let current_stage = memory.pointer(stage_mgr.checked_add(STAGE_MGR_CURRENT_STAGE)?, true)?;
    memory.validate_object(current_stage, "StageDungeon", "Panda")?;
    let entity_mgr = acquire_singleton(
        memory,
        module_base,
        ENTITY_MGR_METHOD_INFO_RVA,
        ENTITY_MGR_TYPE_INFO_RVA,
        "ZEntityMgr",
        "Panda.ZGame",
    )?;
    let player_ent = memory.pointer(entity_mgr.checked_add(ENTITY_MGR_PLAYER_ENT)?, true)?;
    memory.validate_object(player_ent, "PlayerEnt", "Panda.ZGame")?;
    let pure_components =
        memory.pointer(player_ent.checked_add(PLAYER_ENT_PURE_COMPONENTS)?, true)?;
    memory.validate_object(pure_components, "PlayerEnt__Storage", "Panda.ZGame")?;
    let char_serialize =
        memory.pointer(pure_components.checked_add(PLAYER_STORAGE_CHAR_DATA)?, true)?;
    memory.validate_exact_type(
        char_serialize,
        module_base.checked_add(CHAR_SERIALIZE_TYPE_INFO_RVA)?,
        "CharSerialize",
        "Zproto",
    )?;
    let scene_data =
        memory.pointer(char_serialize.checked_add(CHAR_SERIALIZE_SCENE_DATA)?, true)?;
    memory.validate_exact_type(
        scene_data,
        module_base.checked_add(SCENE_DATA_TYPE_INFO_RVA)?,
        "SceneData",
        "Zproto",
    )?;
    Some(SceneSample {
        scene_mgr,
        stage_mgr,
        current_stage,
        entity_mgr,
        player_ent,
        pure_components,
        char_serialize,
        scene_data,
        scene_id: memory.i32(scene_mgr.checked_add(SCENE_MGR_SCENE_ID)?)?,
        map_id: memory.u32(scene_data.checked_add(SCENE_DATA_MAP_ID)?)?,
        to_scene_id: memory.i32(scene_mgr.checked_add(SCENE_MGR_TO_SCENE_ID)?)?,
        scene_sub_type: memory.i32(scene_mgr.checked_add(SCENE_MGR_SCENE_SUB_TYPE)?)?,
        switch_state: memory.u8(stage_mgr.checked_add(STAGE_MGR_SWITCH_STATE)?)?,
        stage_type: memory.u8(current_stage.checked_add(STAGE_BASE_STAGE_TYPE)?)?,
    })
}

fn acquire_singleton(
    memory: &ProcessMemory,
    base: usize,
    method_rva: usize,
    type_rva: usize,
    name: &str,
    namespace: &str,
) -> Option<usize> {
    let method = memory.pointer(base.checked_add(method_rva)?, false)?;
    let method_name = memory.address(method.checked_add(METHOD_INFO_NAME)?)?;
    (memory.c_string(method_name)? == "get_Instance").then_some(())?;
    let declaring = memory.pointer(method.checked_add(METHOD_INFO_KLASS)?, false)?;
    memory.validate_class(declaring, "ZSingleton`1", "ZUtil")?;
    let context = memory.pointer(declaring.checked_add(IL2CPP_CLASS_GENERIC_CONTEXT)?, false)?;
    let inflated = memory.pointer(context.checked_add(GENERIC_CONTEXT_INFLATED_CLASS)?, false)?;
    (inflated == memory.pointer(base.checked_add(type_rva)?, false)?).then_some(())?;
    memory.validate_class(inflated, "ZSingleton`1", "ZUtil")?;
    let static_fields = memory.pointer(inflated.checked_add(IL2CPP_CLASS_STATIC_FIELDS)?, false)?;
    let instance = memory.pointer(static_fields, true)?;
    memory.validate_object(instance, name, namespace)?;
    Some(instance)
}

struct ProcessMemory(HANDLE);

unsafe impl Send for ProcessMemory {}

impl Drop for ProcessMemory {
    fn drop(&mut self) {
        if !self.0.is_null() {
            unsafe { CloseHandle(self.0) };
        }
    }
}

impl ProcessMemory {
    fn read(&self, address: usize, length: usize) -> Option<Vec<u8>> {
        if !plausible_address(address) || length == 0 || length > 128 {
            return None;
        }
        let end = address.checked_add(length)?;
        let mut region = MEMORY_BASIC_INFORMATION::default();
        if unsafe {
            VirtualQueryEx(
                self.0,
                address as *const c_void,
                &mut region,
                size_of::<MEMORY_BASIC_INFORMATION>(),
            )
        } == 0
            || region.State != MEM_COMMIT
            || region.Protect & (PAGE_NOACCESS | PAGE_GUARD) != 0
            || address < region.BaseAddress as usize
            || end > (region.BaseAddress as usize).saturating_add(region.RegionSize)
        {
            return None;
        }
        let mut bytes = vec![0; length];
        let mut read = 0;
        if unsafe {
            ReadProcessMemory(
                self.0,
                address as *const c_void,
                bytes.as_mut_ptr().cast(),
                length,
                &mut read,
            )
        } == 0
            || read != length
        {
            return None;
        }
        Some(bytes)
    }

    fn pointer(&self, address: usize, null_unavailable: bool) -> Option<usize> {
        let value = usize::from_le_bytes(self.read(address, size_of::<usize>())?.try_into().ok()?);
        ((!null_unavailable || value != 0) && plausible_pointer(value)).then_some(value)
    }

    fn address(&self, address: usize) -> Option<usize> {
        let value = usize::from_le_bytes(self.read(address, size_of::<usize>())?.try_into().ok()?);
        plausible_address(value).then_some(value)
    }

    fn i32(&self, address: usize) -> Option<i32> {
        Some(i32::from_le_bytes(self.read(address, 4)?.try_into().ok()?))
    }

    fn u32(&self, address: usize) -> Option<u32> {
        Some(u32::from_le_bytes(self.read(address, 4)?.try_into().ok()?))
    }

    fn u8(&self, address: usize) -> Option<u8> {
        self.read(address, 1)?.first().copied()
    }

    fn c_string(&self, address: usize) -> Option<String> {
        let bytes = self.read(address, MAX_C_STRING_BYTES)?;
        let end = bytes.iter().position(|byte| *byte == 0)?;
        std::str::from_utf8(&bytes[..end]).ok().map(str::to_owned)
    }

    fn validate_class(&self, class: usize, name: &str, namespace: &str) -> Option<()> {
        let actual_name = self.c_string(self.address(class.checked_add(IL2CPP_CLASS_NAME)?)?)?;
        let actual_namespace =
            self.c_string(self.address(class.checked_add(IL2CPP_CLASS_NAMESPACE)?)?)?;
        (actual_name == name && actual_namespace == namespace).then_some(())
    }

    fn validate_object(&self, object: usize, name: &str, namespace: &str) -> Option<()> {
        self.validate_class(self.pointer(object, false)?, name, namespace)
    }

    fn validate_exact_type(
        &self,
        object: usize,
        type_slot: usize,
        name: &str,
        namespace: &str,
    ) -> Option<()> {
        let class = self.pointer(object, false)?;
        (class == self.pointer(type_slot, false)?).then_some(())?;
        self.validate_class(class, name, namespace)
    }
}

fn plausible_address(value: usize) -> bool {
    (MIN_USER_ADDRESS..=MAX_USER_ADDRESS).contains(&value)
}

fn plausible_pointer(value: usize) -> bool {
    value % size_of::<usize>() == 0 && plausible_address(value)
}

struct OwnedHandle(HANDLE);

impl Drop for OwnedHandle {
    fn drop(&mut self) {
        if self.0 != INVALID_HANDLE_VALUE && !self.0.is_null() {
            unsafe { CloseHandle(self.0) };
        }
    }
}

#[derive(Clone)]
struct ModuleIdentity {
    base: usize,
    path: OsString,
}

fn open_reviewed_process(selected: Option<u32>) -> Option<(ProcessMemory, usize)> {
    let process_id = selected.or_else(unique_process_id)?;
    let executable = find_module(process_id, PROCESS_NAME)?;
    let assembly = find_module(process_id, "GameAssembly.dll")?;
    validate_file(
        Path::new(&executable.path),
        EXECUTABLE_BYTES,
        EXECUTABLE_SHA256,
    )?;
    validate_file(Path::new(&assembly.path), ASSEMBLY_BYTES, ASSEMBLY_SHA256)?;
    let handle = unsafe { OpenProcess(PROCESS_QUERY_INFORMATION | PROCESS_VM_READ, 0, process_id) };
    (!handle.is_null()).then_some((ProcessMemory(handle), assembly.base))
}

fn unique_process_id() -> Option<u32> {
    let snapshot = OwnedHandle(unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) });
    if snapshot.0 == INVALID_HANDLE_VALUE {
        return None;
    }
    let mut entry = PROCESSENTRY32W {
        dwSize: size_of::<PROCESSENTRY32W>() as u32,
        ..Default::default()
    };
    let mut found = None;
    let mut present = unsafe { Process32FirstW(snapshot.0, &mut entry) } != 0;
    while present {
        if wide_string(&entry.szExeFile).eq_ignore_ascii_case(PROCESS_NAME)
            && found.replace(entry.th32ProcessID).is_some()
        {
            return None;
        }
        present = unsafe { Process32NextW(snapshot.0, &mut entry) } != 0;
    }
    found
}

fn find_module(process_id: u32, expected: &str) -> Option<ModuleIdentity> {
    let snapshot = OwnedHandle(unsafe {
        CreateToolhelp32Snapshot(TH32CS_SNAPMODULE | TH32CS_SNAPMODULE32, process_id)
    });
    if snapshot.0 == INVALID_HANDLE_VALUE {
        return None;
    }
    let mut entry = MODULEENTRY32W {
        dwSize: size_of::<MODULEENTRY32W>() as u32,
        ..Default::default()
    };
    let mut present = unsafe { Module32FirstW(snapshot.0, &mut entry) } != 0;
    while present {
        if wide_string(&entry.szModule).eq_ignore_ascii_case(expected) {
            return Some(ModuleIdentity {
                base: entry.modBaseAddr as usize,
                path: wide_os_string(&entry.szExePath),
            });
        }
        present = unsafe { Module32NextW(snapshot.0, &mut entry) } != 0;
    }
    None
}

fn wide_os_string(value: &[u16]) -> OsString {
    let length = value
        .iter()
        .position(|unit| *unit == 0)
        .unwrap_or(value.len());
    OsString::from_wide(&value[..length])
}

fn wide_string(value: &[u16]) -> String {
    wide_os_string(value).to_string_lossy().into_owned()
}

fn validate_file(path: &Path, expected_len: u64, expected_digest: &str) -> Option<()> {
    let mut file = File::open(path).ok()?;
    (file.metadata().ok()?.len() == expected_len).then_some(())?;
    let mut digest = Sha256::new();
    let mut buffer = [0_u8; 1024 * 1024];
    loop {
        let read = file.read(&mut buffer).ok()?;
        if read == 0 {
            break;
        }
        digest.update(&buffer[..read]);
    }
    (format!("{:x}", digest.finalize()) == expected_digest).then_some(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample(scene_id: i32, map_id: u32) -> SceneSample {
        SceneSample {
            scene_mgr: 1,
            stage_mgr: 2,
            current_stage: 3,
            entity_mgr: 4,
            player_ent: 5,
            pure_components: 6,
            char_serialize: 7,
            scene_data: 8,
            scene_id,
            map_id,
            to_scene_id: 0,
            scene_sub_type: DUNGEON_SCENE_SUB_TYPE,
            switch_state: 0,
            stage_type: DUNGEON_STAGE_TYPE,
        }
    }

    #[test]
    fn stable_matching_dungeon_identity_is_accepted() {
        let reef = sample(6_561, 6_561);
        assert_eq!(
            stable_identity(Some(reef), Some(reef)),
            Some(NativeSceneIdentity {
                scene_id: 6_561,
                map_id: 6_561
            })
        );
    }

    #[test]
    fn torn_transition_and_mismatched_map_fail_closed() {
        let reef = sample(6_561, 6_561);
        let mut transition = reef;
        transition.to_scene_id = 6_562;
        assert_eq!(stable_identity(Some(reef), Some(transition)), None);
        assert_eq!(stable_identity(Some(transition), Some(transition)), None);
        let mismatch = sample(6_561, 6_525);
        assert_eq!(stable_identity(Some(mismatch), Some(mismatch)), None);
        assert_eq!(stable_identity(None, None), None);
    }

    #[test]
    fn non_dungeon_and_unstable_roots_fail_closed() {
        let reef = sample(6_561, 6_561);
        let mut city = reef;
        city.stage_type = 3;
        city.scene_sub_type = 3;
        assert_eq!(stable_identity(Some(city), Some(city)), None);
        let mut moved = reef;
        moved.scene_data += 8;
        assert_eq!(stable_identity(Some(reef), Some(moved)), None);
    }
}
