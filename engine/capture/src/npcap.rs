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
    System::LibraryLoader::{
        GetProcAddress, LOAD_LIBRARY_SEARCH_DLL_LOAD_DIR, LOAD_LIBRARY_SEARCH_SYSTEM32,
        LoadLibraryExW,
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
        let path = installed_wpcap_path().ok_or_else(|| {
            adapter_error(
                "Npcap's wpcap.dll was not found under the trusted Windows system directory",
            )
        })?;
        let wide_path = path
            .as_os_str()
            .encode_wide()
            .chain(std::iter::once(0))
            .collect::<Vec<_>>();
        // SAFETY: the path is NUL-terminated and points to an existing DLL in
        // the trusted Windows system tree. The flags resolve Packet.dll from
        // the same Npcap directory without searching the working directory.
        let module = unsafe {
            LoadLibraryExW(
                wide_path.as_ptr(),
                ptr::null_mut(),
                LOAD_LIBRARY_SEARCH_DLL_LOAD_DIR | LOAD_LIBRARY_SEARCH_SYSTEM32,
            )
        };
        if module.is_null() {
            return Err(adapter_error(format!(
                "could not load {}: {}",
                path.display(),
                std::io::Error::last_os_error()
            )));
        }

        let loaded = (|| {
            Ok(Self {
                module,
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
    NpcapApi::load().is_ok()
}

pub fn npcap_device_name(adapter_name: &str) -> String {
    let adapter_name = adapter_name.trim();
    if adapter_name.to_ascii_lowercase().contains("\\device\\npf_") {
        adapter_name.to_owned()
    } else {
        format!(r"\Device\NPF_{adapter_name}")
    }
}

fn installed_wpcap_path() -> Option<PathBuf> {
    let system_root = PathBuf::from(std::env::var_os("SystemRoot")?);
    [
        system_root.join("System32/Npcap/wpcap.dll"),
        system_root.join("System32/wpcap.dll"),
    ]
    .into_iter()
    .find(|path| Path::new(path).is_file())
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
        if installed_wpcap_path().is_some() {
            NpcapApi::load().expect("installed Npcap must expose the libpcap ABI");
        }
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
