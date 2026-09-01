use std::{
    ffi::{CStr, CString, c_char, c_int, c_uchar, c_void},
    os::windows::ffi::OsStrExt,
    path::{Path, PathBuf},
    ptr,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant},
};

use bytes::Bytes;
use windows_sys::Win32::{
    Foundation::{FreeLibrary, HMODULE},
    System::{
        Diagnostics::Debug::{
            GetThreadErrorMode, SEM_FAILCRITICALERRORS, SEM_NOOPENFILEERRORBOX, SetThreadErrorMode,
        },
        LibraryLoader::{
            GetModuleFileNameW, GetModuleHandleW, GetProcAddress, LOAD_LIBRARY_SEARCH_DLL_LOAD_DIR,
            LOAD_LIBRARY_SEARCH_SYSTEM32, LoadLibraryExW,
        },
    },
};

use crate::{
    CaptureError, CaptureSource, CaptureSourceKind, CaptureSourceMetadata, CapturedFrame,
    TimestampNormalization,
};

const PCAP_ERROR_BUFFER_SIZE: usize = 256;
const PCAP_SNAPSHOT_BYTES: c_int = 262_144;
const PCAP_READ_TIMEOUT_MILLIS: c_int = 250;
const MAXIMUM_CAPTURED_FRAME_BYTES: usize = 16 * 1024 * 1024;
const REQUIRED_PACKET_EXPORTS: &[&[u8]] = &[
    b"PacketSetMinToCopy\0",
    b"PacketGetAirPcapHandle\0",
    b"PacketCloseAdapter\0",
    b"PacketGetReadEvent\0",
    b"PacketGetInfo\0",
    b"PacketRequest\0",
    b"PacketGetNetInfoEx\0",
    b"PacketGetAdapterNames\0",
    b"PacketSetHwFilter\0",
    b"PacketReceivePacket\0",
    b"PacketInitPacket\0",
    b"PacketSendPackets\0",
    b"PacketSendPacket\0",
    b"PacketOpenAdapter\0",
    b"PacketGetMonitorMode\0",
    b"PacketSetMonitorMode\0",
    b"PacketIsMonitorModeSupported\0",
    b"PacketIsLoopbackAdapter\0",
    b"PacketGetNetType\0",
    b"PacketSetBuff\0",
    b"PacketGetStatsEx\0",
    b"PacketGetStats\0",
    b"PacketGetTimestampModes\0",
    b"PacketSetTimestampMode\0",
    b"PacketSetLoopbackBehavior\0",
    b"PacketSetBpf\0",
    b"PacketSetReadTimeout\0",
    b"PacketSetMode\0",
    b"PacketGetVersion\0",
];

type PcapOpenLive =
    unsafe extern "C" fn(*const c_char, c_int, c_int, c_int, *mut c_char) -> *mut c_void;
type PcapNextEx =
    unsafe extern "C" fn(*mut c_void, *mut *const PcapPacketHeader, *mut *const c_uchar) -> c_int;
type PcapDataLink = unsafe extern "C" fn(*mut c_void) -> c_int;
type PcapGetError = unsafe extern "C" fn(*mut c_void) -> *mut c_char;
type PcapClose = unsafe extern "C" fn(*mut c_void);
type PcapCompile =
    unsafe extern "C" fn(*mut c_void, *mut BpfProgram, *const c_char, c_int, u32) -> c_int;
type PcapSetFilter = unsafe extern "C" fn(*mut c_void, *mut BpfProgram) -> c_int;
type PcapFreeCode = unsafe extern "C" fn(*mut BpfProgram);

#[repr(C)]
struct PcapTimeval {
    seconds: i32,
    microseconds: i32,
}

#[repr(C)]
struct PcapPacketHeader {
    timestamp: PcapTimeval,
    captured_length: u32,
    original_length: u32,
}

#[repr(C)]
struct BpfProgram {
    instruction_count: u32,
    instructions: *mut c_void,
}

struct NpcapApi {
    module: HMODULE,
    _packet_dependency: PacketDependency,
    open_live: PcapOpenLive,
    next_ex: PcapNextEx,
    data_link: PcapDataLink,
    get_error: PcapGetError,
    close: PcapClose,
    compile: PcapCompile,
    set_filter: PcapSetFilter,
    free_code: PcapFreeCode,
}

// The library handle and immutable function pointers are owned by one capture
// object and used only by its capture worker. Npcap's per-handle API is safe to
// move to that worker as long as the handle is not used concurrently.
unsafe impl Send for NpcapApi {}

impl NpcapApi {
    fn load() -> Result<Self, CaptureError> {
        let paths = installed_wpcap_paths();
        if paths.is_empty() {
            return Err(adapter_error(
                "Npcap's wpcap.dll was not found under the trusted Windows system directory",
            ));
        }
        let mut failures = Vec::with_capacity(paths.len());
        for path in paths {
            match Self::load_from_path(&path) {
                Ok(api) => return Ok(api),
                Err(error) => failures.push(format!("{}: {error}", path.display())),
            }
        }
        Err(adapter_error(format!(
            "Npcap is installed but no compatible wpcap.dll/Packet.dll pair could be loaded. {}. Repair or update Npcap, then refresh capture devices; rLogs does not require dumpcap.exe",
            failures.join("; ")
        )))
    }

    fn load_from_path(path: &Path) -> Result<Self, CaptureError> {
        let packet_dependency = PacketDependency::load_for(path)?;
        let wide_path = path
            .as_os_str()
            .encode_wide()
            .chain(std::iter::once(0))
            .collect::<Vec<_>>();
        let _error_mode = DllLoadErrorModeGuard::suppress_system_dialogs();
        // SAFETY: the path is NUL-terminated and points to an existing DLL in
        // the trusted Windows system tree. The flags resolve Packet.dll from
        // the same candidate directory without searching the working directory.
        let module = unsafe {
            LoadLibraryExW(
                wide_path.as_ptr(),
                ptr::null_mut(),
                LOAD_LIBRARY_SEARCH_DLL_LOAD_DIR | LOAD_LIBRARY_SEARCH_SYSTEM32,
            )
        };
        if module.is_null() {
            let error = std::io::Error::last_os_error();
            let detail = if error.raw_os_error() == Some(127) {
                format!(
                    "Windows error 127: wpcap.dll imports an entry point that its sibling Packet.dll does not export ({error})"
                )
            } else {
                error.to_string()
            };
            return Err(adapter_error(format!(
                "could not load {}: {detail}",
                path.display(),
            )));
        }

        let loaded = (|| {
            Ok(Self {
                module,
                _packet_dependency: packet_dependency,
                // SAFETY: each required export is checked for presence and
                // cast to the signature defined by the public libpcap ABI.
                open_live: unsafe { required_export(module, b"pcap_open_live\0")? },
                next_ex: unsafe { required_export(module, b"pcap_next_ex\0")? },
                data_link: unsafe { required_export(module, b"pcap_datalink\0")? },
                get_error: unsafe { required_export(module, b"pcap_geterr\0")? },
                close: unsafe { required_export(module, b"pcap_close\0")? },
                compile: unsafe { required_export(module, b"pcap_compile\0")? },
                set_filter: unsafe { required_export(module, b"pcap_setfilter\0")? },
                free_code: unsafe { required_export(module, b"pcap_freecode\0")? },
            })
        })();
        if loaded.is_err() {
            // SAFETY: `module` is a live handle returned above and no function
            // pointer escapes when construction fails.
            unsafe { FreeLibrary(module) };
        }
        loaded
    }
}

struct PacketDependency {
    module: HMODULE,
    owned: bool,
}

impl PacketDependency {
    fn load_for(wpcap_path: &Path) -> Result<Self, CaptureError> {
        let packet_name = "Packet.dll\0".encode_utf16().collect::<Vec<_>>();
        // Windows resolves a dependency by base name against modules already in
        // the process before considering the requested DLL's directory. A
        // legacy Packet.dll injected or loaded by another component therefore
        // must be rejected before wpcap.dll is touched.
        let existing = unsafe { GetModuleHandleW(packet_name.as_ptr()) };
        if !existing.is_null() {
            let path = loaded_module_path(existing)
                .unwrap_or_else(|| PathBuf::from("an already-loaded Packet.dll"));
            validate_packet_exports(existing, &path, wpcap_path)?;
            return Ok(Self {
                module: existing,
                owned: false,
            });
        }

        let Some(directory) = wpcap_path.parent() else {
            return Err(adapter_error(format!(
                "Npcap path has no parent directory: {}",
                wpcap_path.display()
            )));
        };
        let packet_path = directory.join("Packet.dll");
        if !packet_path.is_file() {
            return Err(adapter_error(format!(
                "Npcap's sibling Packet.dll was not found beside {}",
                wpcap_path.display()
            )));
        }
        let wide_path = packet_path
            .as_os_str()
            .encode_wide()
            .chain(std::iter::once(0))
            .collect::<Vec<_>>();
        let _error_mode = DllLoadErrorModeGuard::suppress_system_dialogs();
        let module = unsafe {
            LoadLibraryExW(
                wide_path.as_ptr(),
                ptr::null_mut(),
                LOAD_LIBRARY_SEARCH_DLL_LOAD_DIR | LOAD_LIBRARY_SEARCH_SYSTEM32,
            )
        };
        if module.is_null() {
            return Err(adapter_error(format!(
                "could not load {} before wpcap.dll: {}",
                packet_path.display(),
                std::io::Error::last_os_error()
            )));
        }
        let dependency = Self {
            module,
            owned: true,
        };
        validate_packet_exports(module, &packet_path, wpcap_path)?;
        Ok(dependency)
    }
}

impl Drop for PacketDependency {
    fn drop(&mut self) {
        if self.owned {
            unsafe { FreeLibrary(self.module) };
        }
    }
}

fn validate_packet_exports(
    module: HMODULE,
    packet_path: &Path,
    wpcap_path: &Path,
) -> Result<(), CaptureError> {
    let missing = REQUIRED_PACKET_EXPORTS
        .iter()
        .filter_map(|name| {
            let found = unsafe { GetProcAddress(module, name.as_ptr()) }.is_some();
            (!found).then(|| String::from_utf8_lossy(&name[..name.len() - 1]).into_owned())
        })
        .collect::<Vec<_>>();
    if missing.is_empty() {
        return Ok(());
    }
    Err(adapter_error(format!(
        "{} is incompatible with {} because it lacks required exports: {}; close software that preloads a legacy Packet.dll or restart Windows, then refresh capture devices",
        packet_path.display(),
        wpcap_path.display(),
        missing.join(", ")
    )))
}

fn loaded_module_path(module: HMODULE) -> Option<PathBuf> {
    let mut buffer = vec![0u16; 32_768];
    let length = unsafe { GetModuleFileNameW(module, buffer.as_mut_ptr(), buffer.len() as u32) };
    if length == 0 || length as usize >= buffer.len() {
        return None;
    }
    buffer.truncate(length as usize);
    Some(PathBuf::from(String::from_utf16_lossy(&buffer)))
}

struct DllLoadErrorModeGuard {
    previous: u32,
    changed: bool,
}

impl DllLoadErrorModeGuard {
    fn suppress_system_dialogs() -> Self {
        // A broken Npcap installation can contain a wpcap.dll and Packet.dll
        // from different releases. Windows normally presents a blocking
        // "Entry Point Not Found" dialog while resolving that dependency. The
        // capture probe owns the error and must return it to the UI instead.
        let current = unsafe { GetThreadErrorMode() };
        let mut previous = current;
        let changed = unsafe {
            SetThreadErrorMode(
                current | SEM_FAILCRITICALERRORS | SEM_NOOPENFILEERRORBOX,
                &mut previous,
            ) != 0
        };
        Self { previous, changed }
    }
}

impl Drop for DllLoadErrorModeGuard {
    fn drop(&mut self) {
        if self.changed {
            // SAFETY: this restores only the calling thread's prior error mode.
            unsafe {
                SetThreadErrorMode(self.previous, ptr::null_mut());
            }
        }
    }
}

impl Drop for NpcapApi {
    fn drop(&mut self) {
        // SAFETY: the capture handle is closed by `NpcapLiveCapture::drop`
        // before the API object is dropped.
        unsafe { FreeLibrary(self.module) };
    }
}

unsafe fn required_export<T: Copy>(
    module: HMODULE,
    name: &'static [u8],
) -> Result<T, CaptureError> {
    // SAFETY: `name` is a static NUL-terminated ASCII export name and `module`
    // remains loaded for the lifetime of the returned function pointer.
    let Some(export) = (unsafe { GetProcAddress(module, name.as_ptr()) }) else {
        return Err(adapter_error(format!(
            "Npcap is missing required export {}",
            String::from_utf8_lossy(&name[..name.len().saturating_sub(1)])
        )));
    };
    if std::mem::size_of::<T>() != std::mem::size_of_val(&export) {
        return Err(adapter_error("Npcap function pointer size is unsupported"));
    }
    // SAFETY: the caller selects `T` for the exact named libpcap ABI export.
    Ok(unsafe { std::mem::transmute_copy(&export) })
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NpcapLiveConfig {
    pub interface: String,
    pub duration_seconds: u32,
}

impl NpcapLiveConfig {
    pub fn new(interface: impl Into<String>, duration_seconds: u32) -> Result<Self, CaptureError> {
        let config = Self {
            interface: interface.into(),
            duration_seconds,
        };
        config.validate()?;
        Ok(config)
    }

    fn validate(&self) -> Result<(), CaptureError> {
        if self.interface.trim().is_empty() {
            return Err(adapter_error("capture interface must not be empty"));
        }
        if self.interface.contains('\0') {
            return Err(adapter_error("capture interface contains a null byte"));
        }
        if self.duration_seconds > 3_600 {
            return Err(adapter_error(
                "capture duration must be 0 (continuous) or at most 3600 seconds",
            ));
        }
        Ok(())
    }
}

#[derive(Debug, Clone)]
pub struct NpcapLiveStopHandle {
    stop_requested: Arc<AtomicBool>,
}

impl NpcapLiveStopHandle {
    pub fn request_stop(&self) {
        self.stop_requested.store(true, Ordering::Release);
    }
}

pub(crate) struct NpcapLiveCapture {
    api: NpcapApi,
    handle: *mut c_void,
    stop_requested: Arc<AtomicBool>,
    started: Instant,
    maximum_duration: Option<Duration>,
    sequence: u64,
    link_type: crate::CaptureLinkType,
    metadata: CaptureSourceMetadata,
    closed: bool,
}

// The Npcap handle is created before the capture worker is spawned, then all
// reads and closure happen on that one worker. The atomic stop flag is the only
// cross-thread state.
unsafe impl Send for NpcapLiveCapture {}

impl std::fmt::Debug for NpcapLiveCapture {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("NpcapLiveCapture")
            .field("metadata", &self.metadata)
            .field("sequence", &self.sequence)
            .field("closed", &self.closed)
            .finish_non_exhaustive()
    }
}

impl NpcapLiveCapture {
    pub(crate) fn open(config: NpcapLiveConfig) -> Result<Self, CaptureError> {
        config.validate()?;
        let api = NpcapApi::load()?;
        let interface = CString::new(config.interface.as_bytes())
            .map_err(|_| adapter_error("capture interface contains a null byte"))?;
        let mut error_buffer = [0 as c_char; PCAP_ERROR_BUFFER_SIZE];
        // Non-promiscuous mode is sufficient for traffic owned by this PC and
        // avoids requesting broader adapter behavior than rLogs needs.
        // SAFETY: all pointers are valid for this call and the returned handle
        // is exclusively owned by the new capture object.
        let handle = unsafe {
            (api.open_live)(
                interface.as_ptr(),
                PCAP_SNAPSHOT_BYTES,
                0,
                PCAP_READ_TIMEOUT_MILLIS,
                error_buffer.as_mut_ptr(),
            )
        };
        if handle.is_null() {
            return Err(adapter_error(format!(
                "could not open Npcap interface {}: {}",
                config.interface,
                error_buffer_text(&error_buffer)
            )));
        }
        let tcp_filter = CString::new("tcp").expect("static filter has no null byte");
        let mut filter = BpfProgram {
            instruction_count: 0,
            instructions: ptr::null_mut(),
        };
        // SAFETY: the handle is live, the filter output is writable, and the
        // expression remains valid for the duration of compilation.
        if unsafe { (api.compile)(handle, &mut filter, tcp_filter.as_ptr(), 1, u32::MAX) } != 0 {
            // SAFETY: the handle is exclusively owned here.
            unsafe { (api.close)(handle) };
            return Err(adapter_error(
                "Npcap could not compile the required TCP filter",
            ));
        }
        // SAFETY: `filter` was initialized by `pcap_compile`; Npcap copies the
        // program during `pcap_setfilter`, so it is freed immediately after.
        let filter_status = unsafe { (api.set_filter)(handle, &mut filter) };
        unsafe { (api.free_code)(&mut filter) };
        if filter_status != 0 {
            // SAFETY: the handle is exclusively owned here.
            unsafe { (api.close)(handle) };
            return Err(adapter_error(
                "Npcap could not install the required TCP filter",
            ));
        }
        // SAFETY: `handle` was returned by `pcap_open_live` above.
        let data_link = unsafe { (api.data_link)(handle) };
        let link_type = crate::CaptureLinkType::from_pcap_link_type(data_link);
        let stop_requested = Arc::new(AtomicBool::new(false));
        Ok(Self {
            api,
            handle,
            stop_requested,
            started: Instant::now(),
            maximum_duration: (config.duration_seconds > 0)
                .then(|| Duration::from_secs(u64::from(config.duration_seconds))),
            sequence: 0,
            link_type,
            metadata: CaptureSourceMetadata {
                source_id: "npcap-process-owned".into(),
                display_name: "Native process-owned Npcap capture".into(),
                kind: CaptureSourceKind::Live,
                link_types: vec![link_type],
                file_format: None,
            },
            closed: false,
        })
    }

    pub(crate) fn stop_handle(&self) -> NpcapLiveStopHandle {
        NpcapLiveStopHandle {
            stop_requested: Arc::clone(&self.stop_requested),
        }
    }

    fn should_stop(&self) -> bool {
        self.stop_requested.load(Ordering::Acquire)
            || self
                .maximum_duration
                .is_some_and(|duration| self.started.elapsed() >= duration)
    }

    fn close(&mut self) {
        if self.closed || self.handle.is_null() {
            return;
        }
        // SAFETY: this object exclusively owns the live pcap handle.
        unsafe { (self.api.close)(self.handle) };
        self.handle = ptr::null_mut();
        self.closed = true;
    }

    fn pcap_error(&self) -> String {
        // SAFETY: the handle remains open and `pcap_geterr` returns a
        // NUL-terminated string owned by that handle.
        let error = unsafe { (self.api.get_error)(self.handle) };
        if error.is_null() {
            return "unknown Npcap error".into();
        }
        // SAFETY: Npcap guarantees the error pointer is NUL-terminated.
        unsafe { CStr::from_ptr(error) }
            .to_string_lossy()
            .into_owned()
    }
}

impl CaptureSource for NpcapLiveCapture {
    fn metadata(&self) -> &CaptureSourceMetadata {
        &self.metadata
    }

    fn next_frame(&mut self) -> Result<Option<CapturedFrame>, CaptureError> {
        loop {
            if self.should_stop() {
                self.close();
                return Ok(None);
            }
            let mut header = ptr::null();
            let mut bytes = ptr::null();
            // SAFETY: the handle is open and both output pointers are writable
            // for the duration of the call. Packet memory is copied before the
            // next Npcap operation.
            let status = unsafe { (self.api.next_ex)(self.handle, &mut header, &mut bytes) };
            match status {
                0 => continue,
                1 => {
                    if header.is_null() || bytes.is_null() {
                        return Err(adapter_error("Npcap returned an empty packet"));
                    }
                    // SAFETY: a status of one guarantees both pointers refer to
                    // one packet until the next Npcap call.
                    let header = unsafe { &*header };
                    let captured_length = usize::try_from(header.captured_length)
                        .map_err(|_| adapter_error("Npcap packet length is unsupported"))?;
                    if captured_length > MAXIMUM_CAPTURED_FRAME_BYTES {
                        return Err(adapter_error(format!(
                            "Npcap packet exceeds the {} byte safety limit",
                            MAXIMUM_CAPTURED_FRAME_BYTES
                        )));
                    }
                    // SAFETY: Npcap supplies at least `captured_length` bytes.
                    let packet = unsafe { std::slice::from_raw_parts(bytes, captured_length) };
                    self.sequence = self
                        .sequence
                        .checked_add(1)
                        .ok_or_else(|| adapter_error("capture sequence space is exhausted"))?;
                    let source_timestamp_nanos = i64::from(header.timestamp.seconds)
                        .checked_mul(1_000_000_000)
                        .and_then(|value| {
                            value.checked_add(i64::from(header.timestamp.microseconds) * 1_000)
                        });
                    return Ok(Some(CapturedFrame {
                        sequence: self.sequence,
                        observed_micros: self
                            .started
                            .elapsed()
                            .as_micros()
                            .try_into()
                            .unwrap_or(u64::MAX),
                        source_timestamp_nanos,
                        timestamp_normalization: TimestampNormalization::Exact,
                        interface_id: Some(0),
                        link_type: self.link_type,
                        original_length: header.original_length,
                        bytes: Bytes::copy_from_slice(packet),
                    }));
                }
                -2 if self.should_stop() => {
                    self.close();
                    return Ok(None);
                }
                -2 => return Err(adapter_error("Npcap ended an active capture unexpectedly")),
                _ => {
                    return Err(adapter_error(format!(
                        "Npcap read failed: {}",
                        self.pcap_error()
                    )));
                }
            }
        }
    }
}

impl Drop for NpcapLiveCapture {
    fn drop(&mut self) {
        self.close();
    }
}

pub fn npcap_available() -> bool {
    npcap_diagnostic().is_ok()
}

pub fn npcap_diagnostic() -> Result<(), CaptureError> {
    NpcapApi::load().map(drop)
}

pub fn npcap_device_name(adapter_name: &str) -> String {
    let adapter_name = adapter_name.trim();
    if adapter_name.to_ascii_lowercase().contains("\\device\\npf_") {
        adapter_name.to_owned()
    } else {
        format!(r"\Device\NPF_{adapter_name}")
    }
}

fn installed_wpcap_paths() -> Vec<PathBuf> {
    let Some(system_root) = std::env::var_os("SystemRoot").map(PathBuf::from) else {
        return Vec::new();
    };
    [
        system_root.join("System32/Npcap/wpcap.dll"),
        system_root.join("System32/wpcap.dll"),
    ]
    .into_iter()
    .filter(|path| Path::new(path).is_file())
    .collect()
}

fn error_buffer_text(buffer: &[c_char; PCAP_ERROR_BUFFER_SIZE]) -> String {
    let bytes = buffer
        .iter()
        .map(|value| *value as u8)
        .take_while(|value| *value != 0)
        .collect::<Vec<_>>();
    let value = String::from_utf8_lossy(&bytes).trim().to_owned();
    if value.is_empty() {
        "unknown Npcap error".into()
    } else {
        value
    }
}

fn adapter_error(message: impl Into<String>) -> CaptureError {
    CaptureError::Adapter {
        adapter: "npcap".into(),
        message: message.into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn device_names_are_stable_for_windows_adapter_guids() {
        assert_eq!(
            npcap_device_name("{01234567-89AB-CDEF-0123-456789ABCDEF}"),
            r"\Device\NPF_{01234567-89AB-CDEF-0123-456789ABCDEF}"
        );
        assert_eq!(
            npcap_device_name(r"\Device\NPF_{01234567-89AB-CDEF-0123-456789ABCDEF}"),
            r"\Device\NPF_{01234567-89AB-CDEF-0123-456789ABCDEF}"
        );
    }

    #[test]
    fn installed_npcap_exports_the_required_capture_api() {
        if !installed_wpcap_paths().is_empty() {
            NpcapApi::load().expect("installed Npcap must expose the libpcap ABI");
        }
    }

    #[test]
    fn npcap_probe_restores_the_calling_threads_error_mode() {
        let before = unsafe { GetThreadErrorMode() };
        let _ = npcap_diagnostic();
        let after = unsafe { GetThreadErrorMode() };
        assert_eq!(after, before);
    }

    #[test]
    fn incompatible_sibling_packet_dll_returns_error_instead_of_a_system_dialog() {
        let Some(source_wpcap) = installed_wpcap_paths().into_iter().next() else {
            return;
        };
        let Some(system_root) = std::env::var_os("SystemRoot").map(PathBuf::from) else {
            return;
        };
        let unrelated_dll = system_root.join("System32/version.dll");
        if !unrelated_dll.is_file() {
            return;
        }
        let fixture = std::env::temp_dir().join(format!(
            "rlogs-incompatible-npcap-fixture-{}",
            std::process::id()
        ));
        if fixture.is_dir() {
            std::fs::remove_dir_all(&fixture).expect("remove stale Npcap fixture");
        }
        std::fs::create_dir(&fixture).expect("create Npcap fixture");
        let fixture_wpcap = fixture.join("wpcap.dll");
        std::fs::copy(source_wpcap, &fixture_wpcap).expect("copy wpcap fixture");
        std::fs::copy(unrelated_dll, fixture.join("Packet.dll"))
            .expect("copy incompatible Packet fixture");

        let before = unsafe { GetThreadErrorMode() };
        let result = NpcapApi::load_from_path(&fixture_wpcap);
        let after = unsafe { GetThreadErrorMode() };
        assert_eq!(after, before);
        let error = match result {
            Ok(_) => panic!("wpcap unexpectedly loaded against an incompatible Packet.dll"),
            Err(error) => error.to_string(),
        };
        assert!(
            error.contains("lacks required exports") && error.contains("PacketGetMonitorMode"),
            "{error}"
        );

        std::fs::remove_dir_all(fixture).expect("remove Npcap fixture");
    }

    #[test]
    fn preloaded_legacy_packet_dll_is_rejected_before_wpcap_loads() {
        const CHILD_ENV: &str = "RLOGS_PRELOADED_PACKET_TEST_CHILD";
        if std::env::var_os(CHILD_ENV).is_some() {
            run_preloaded_legacy_packet_child();
            return;
        }
        let status = std::process::Command::new(std::env::current_exe().expect("current test exe"))
            .args([
                "--exact",
                "npcap::tests::preloaded_legacy_packet_dll_is_rejected_before_wpcap_loads",
                "--nocapture",
            ])
            .env(CHILD_ENV, "1")
            .status()
            .expect("run isolated preloaded Packet.dll test");
        assert!(status.success(), "isolated Packet.dll test failed");
    }

    fn run_preloaded_legacy_packet_child() {
        let Some(source_wpcap) = installed_wpcap_paths().into_iter().next() else {
            return;
        };
        let Some(system_root) = std::env::var_os("SystemRoot").map(PathBuf::from) else {
            return;
        };
        let unrelated_dll = system_root.join("System32/version.dll");
        if !unrelated_dll.is_file() {
            return;
        }
        let fixture = std::env::temp_dir().join(format!(
            "rlogs-preloaded-packet-fixture-{}",
            std::process::id()
        ));
        std::fs::create_dir(&fixture).expect("create preloaded Packet fixture");
        let packet_path = fixture.join("Packet.dll");
        std::fs::copy(unrelated_dll, &packet_path).expect("copy legacy Packet fixture");
        let wide_path = packet_path
            .as_os_str()
            .encode_wide()
            .chain(std::iter::once(0))
            .collect::<Vec<_>>();
        let module = unsafe {
            LoadLibraryExW(
                wide_path.as_ptr(),
                ptr::null_mut(),
                LOAD_LIBRARY_SEARCH_DLL_LOAD_DIR | LOAD_LIBRARY_SEARCH_SYSTEM32,
            )
        };
        assert!(!module.is_null(), "preload incompatible Packet fixture");

        let error = match NpcapApi::load_from_path(&source_wpcap) {
            Ok(_) => panic!("wpcap loaded against an already-loaded incompatible Packet.dll"),
            Err(error) => error.to_string(),
        };
        assert!(
            error.contains(&packet_path.display().to_string())
                && error.contains("PacketGetMonitorMode"),
            "{error}"
        );

        unsafe { FreeLibrary(module) };
        std::fs::remove_dir_all(fixture).expect("remove preloaded Packet fixture");
    }

    #[test]
    #[ignore = "requires an installed Npcap driver and an active routed Windows adapter"]
    fn installed_npcap_opens_and_cooperatively_stops_an_active_adapter() {
        let adapters = crate::windows_capture_adapters().expect("Windows adapter inventory");
        let recommendation = crate::recommend_windows_capture_adapter(&adapters, &[])
            .expect("active routed adapter");
        let mut capture = NpcapLiveCapture::open(
            NpcapLiveConfig::new(npcap_device_name(&recommendation.adapter_name), 0).unwrap(),
        )
        .expect("open routed adapter directly through Npcap");
        capture.stop_handle().request_stop();
        assert!(capture.next_frame().unwrap().is_none());
    }
}
