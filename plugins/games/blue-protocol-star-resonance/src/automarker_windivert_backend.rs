//! Non-public WinDivert 2.2.2 handle ownership and REFLECT arbitration.
//!
//! This module is shared by the standalone canary through `#[path]` and is
//! available to an eventual in-crate desktop coordinator. It does not expose a
//! public library API and never opens a handle unless its caller asks it to.

#[repr(C)]
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub(crate) struct WinDivertAddress(pub(crate) [u64; 10]);

const _: [(); 80] = [(); std::mem::size_of::<WinDivertAddress>()];
const LAYER_NETWORK: u32 = 0;
const LAYER_REFLECT: u8 = 4;
const EVENT_REFLECT_OPEN: u8 = 8;
const EVENT_REFLECT_CLOSE: u8 = 9;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ReflectEventKind {
    Open,
    Close,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct ReflectedHandleIdentity {
    opened_timestamp: i64,
    process_id: u32,
    layer: u32,
    flags: u64,
    priority: i16,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct ReflectedHandleEvent {
    kind: ReflectEventKind,
    identity: ReflectedHandleIdentity,
}

fn decode_reflect_event(address: &WinDivertAddress) -> Result<ReflectedHandleEvent, &'static str> {
    let header = address.0[1];
    if header as u8 != LAYER_REFLECT {
        return Err("event did not originate at the REFLECT layer");
    }
    let kind = match (header >> 8) as u8 {
        EVENT_REFLECT_OPEN => ReflectEventKind::Open,
        EVENT_REFLECT_CLOSE => ReflectEventKind::Close,
        _ => return Err("REFLECT event kind was not OPEN or CLOSE"),
    };
    let process_and_layer = address.0[3];
    Ok(ReflectedHandleEvent {
        kind,
        identity: ReflectedHandleIdentity {
            opened_timestamp: address.0[2] as i64,
            process_id: process_and_layer as u32,
            layer: (process_and_layer >> 32) as u32,
            flags: address.0[4],
            priority: address.0[5] as u16 as i16,
        },
    })
}

struct ReflectInventory {
    sentinel_process_id: u32,
    sentinel_priority: i16,
    sentinel_flags: u64,
    target_priority: i16,
    open_handles: Vec<ReflectedHandleIdentity>,
    sentinel_seen: bool,
}

impl ReflectInventory {
    fn new(
        sentinel_process_id: u32,
        sentinel_priority: i16,
        sentinel_flags: u64,
        target_priority: i16,
    ) -> Self {
        Self {
            sentinel_process_id,
            sentinel_priority,
            sentinel_flags,
            target_priority,
            open_handles: Vec::new(),
            sentinel_seen: false,
        }
    }

    fn observe(&mut self, event: ReflectedHandleEvent) -> Result<(), &'static str> {
        match event.kind {
            ReflectEventKind::Open => {
                if self.open_handles.contains(&event.identity) {
                    return Err("duplicate REFLECT OPEN event");
                }
                self.open_handles.push(event.identity);
                if event.identity.process_id == self.sentinel_process_id
                    && event.identity.layer == LAYER_NETWORK
                    && event.identity.priority == self.sentinel_priority
                    && event.identity.flags == self.sentinel_flags
                {
                    self.sentinel_seen = true;
                }
            }
            ReflectEventKind::Close => {
                let Some(index) = self
                    .open_handles
                    .iter()
                    .position(|identity| *identity == event.identity)
                else {
                    return Err("REFLECT CLOSE did not match an observed OPEN");
                };
                self.open_handles.swap_remove(index);
            }
        }
        Ok(())
    }

    fn affirmative(&self) -> Result<bool, &'static str> {
        if !self.sentinel_seen {
            return Err("REFLECT sentinel OPEN was not observed");
        }
        Ok(!self.open_handles.iter().any(|identity| {
            identity.layer == LAYER_NETWORK && identity.priority == self.target_priority
        }))
    }
}

#[cfg(windows)]
mod windows_backend {
    use super::*;
    use std::{
        error::Error,
        ffi::{CString, c_void},
        mem::transmute,
        os::windows::ffi::OsStrExt,
        path::Path,
        ptr,
        sync::{Arc, Mutex},
        thread,
        time::Duration,
    };
    use windows_sys::Win32::{
        Foundation::{
            CloseHandle, ERROR_IO_PENDING, ERROR_NO_DATA, FreeLibrary, GetLastError, HANDLE,
            INVALID_HANDLE_VALUE, WAIT_FAILED, WAIT_OBJECT_0, WAIT_TIMEOUT,
        },
        System::{
            IO::{CancelIoEx, GetOverlappedResult, OVERLAPPED},
            LibraryLoader::{GetProcAddress, LOAD_LIBRARY_SEARCH_DLL_LOAD_DIR, LoadLibraryExW},
            Threading::{CreateEventW, WaitForMultipleObjects},
        },
    };

    pub(crate) const FLAG_SNIFF: u64 = 1;
    pub(crate) const FLAG_RECV_ONLY: u64 = 4;
    pub(crate) const FLAG_NO_INSTALL: u64 = 16;
    pub(crate) const PARAM_QUEUE_LENGTH: i32 = 0;
    pub(crate) const PARAM_QUEUE_TIME: i32 = 1;
    pub(crate) const PARAM_QUEUE_SIZE: i32 = 2;
    pub(crate) const PARAM_VERSION_MAJOR: i32 = 3;
    pub(crate) const PARAM_VERSION_MINOR: i32 = 4;
    const LAYER_NETWORK_ABI: i32 = 0;
    const LAYER_REFLECT_ABI: i32 = 4;
    const SHUTDOWN_RECV: i32 = 1;
    const SENTINEL_PRIORITY: i16 = -1000;
    const BARRIER_TIMEOUT: Duration = Duration::from_secs(5);
    static ARBITRATION_LOCK: Mutex<()> = Mutex::new(());

    type OpenFn = unsafe extern "system" fn(*const i8, i32, i16, u64) -> HANDLE;
    type RecvFn =
        unsafe extern "system" fn(HANDLE, *mut c_void, u32, *mut u32, *mut WinDivertAddress) -> i32;
    type RecvExFn = unsafe extern "system" fn(
        HANDLE,
        *mut c_void,
        u32,
        *mut u32,
        u64,
        *mut WinDivertAddress,
        *mut u32,
        *mut OVERLAPPED,
    ) -> i32;
    type SendFn = unsafe extern "system" fn(
        HANDLE,
        *const c_void,
        u32,
        *mut u32,
        *const WinDivertAddress,
    ) -> i32;
    type ShutdownFn = unsafe extern "system" fn(HANDLE, i32) -> i32;
    type CloseFn = unsafe extern "system" fn(HANDLE) -> i32;
    type SetParamFn = unsafe extern "system" fn(HANDLE, i32, u64) -> i32;
    type GetParamFn = unsafe extern "system" fn(HANDLE, i32, *mut u64) -> i32;
    type CompileFilterFn =
        unsafe extern "system" fn(*const i8, i32, *mut i8, u32, *mut *const i8, *mut u32) -> i32;

    #[derive(Clone, Copy)]
    struct Api {
        open: OpenFn,
        recv: RecvFn,
        #[allow(dead_code)]
        recv_ex: RecvExFn,
        send: SendFn,
        shutdown: ShutdownFn,
        close: CloseFn,
        set_param: SetParamFn,
        get_param: GetParamFn,
        compile_filter: CompileFilterFn,
    }

    struct LoadedApi {
        module: windows_sys::Win32::Foundation::HMODULE,
        api: Api,
    }

    struct LoadingModuleGuard(windows_sys::Win32::Foundation::HMODULE);

    impl LoadingModuleGuard {
        fn into_raw(mut self) -> windows_sys::Win32::Foundation::HMODULE {
            std::mem::replace(&mut self.0, ptr::null_mut())
        }
    }

    impl Drop for LoadingModuleGuard {
        fn drop(&mut self) {
            if !self.0.is_null() {
                unsafe { FreeLibrary(self.0) };
            }
        }
    }

    unsafe impl Send for LoadedApi {}
    unsafe impl Sync for LoadedApi {}

    impl Drop for LoadedApi {
        fn drop(&mut self) {
            unsafe { FreeLibrary(self.module) };
        }
    }

    #[derive(Clone)]
    pub(crate) struct PinnedWinDivertBackend(Arc<LoadedApi>);

    pub(crate) struct WinDivertHandle {
        raw: HANDLE,
        loaded: Arc<LoadedApi>,
    }

    unsafe impl Send for WinDivertHandle {}
    unsafe impl Sync for WinDivertHandle {}

    impl Drop for WinDivertHandle {
        fn drop(&mut self) {
            if !self.raw.is_null() && self.raw != INVALID_HANDLE_VALUE {
                unsafe { (self.loaded.api.close)(self.raw) };
            }
        }
    }

    pub(crate) enum ArbitratedNetworkOpen {
        Open(WinDivertHandle),
        Conflict,
    }

    impl PinnedWinDivertBackend {
        pub(crate) unsafe fn load(path: &Path) -> Result<Self, Box<dyn Error>> {
            let wide = path
                .as_os_str()
                .encode_wide()
                .chain(Some(0))
                .collect::<Vec<_>>();
            let module = unsafe {
                LoadLibraryExW(
                    wide.as_ptr(),
                    ptr::null_mut(),
                    LOAD_LIBRARY_SEARCH_DLL_LOAD_DIR,
                )
            };
            if module.is_null() {
                return Err(
                    format!("LoadLibraryExW failed with Windows error {}", unsafe {
                        GetLastError()
                    })
                    .into(),
                );
            }
            let module = LoadingModuleGuard(module);
            macro_rules! export {
                ($name:literal, $kind:ty) => {{
                    let symbol = unsafe { GetProcAddress(module.0, concat!($name, "\0").as_ptr()) }
                        .ok_or(concat!("missing WinDivert export ", $name))?;
                    unsafe { transmute::<unsafe extern "system" fn() -> isize, $kind>(symbol) }
                }};
            }
            let api = Api {
                open: export!("WinDivertOpen", OpenFn),
                recv: export!("WinDivertRecv", RecvFn),
                recv_ex: export!("WinDivertRecvEx", RecvExFn),
                send: export!("WinDivertSend", SendFn),
                shutdown: export!("WinDivertShutdown", ShutdownFn),
                close: export!("WinDivertClose", CloseFn),
                set_param: export!("WinDivertSetParam", SetParamFn),
                get_param: export!("WinDivertGetParam", GetParamFn),
                compile_filter: export!("WinDivertHelperCompileFilter", CompileFilterFn),
            };
            Ok(Self(Arc::new(LoadedApi {
                module: module.into_raw(),
                api,
            })))
        }

        pub(crate) fn open_network(
            &self,
            filter: &str,
            priority: i16,
            flags: u64,
        ) -> Result<WinDivertHandle, Box<dyn Error>> {
            self.open_at_layer(filter, LAYER_NETWORK_ABI, priority, flags)
        }

        pub(crate) fn open_arbitrated_network(
            &self,
            filter: &str,
            priority: i16,
            flags: u64,
        ) -> Result<ArbitratedNetworkOpen, Box<dyn Error>> {
            let _serialization = ARBITRATION_LOCK
                .lock()
                .map_err(|_| "REFLECT arbitration lock was poisoned")?;
            if !self.reflect_inventory_is_clear(priority)? {
                return Ok(ArbitratedNetworkOpen::Conflict);
            }
            Ok(ArbitratedNetworkOpen::Open(
                self.open_network(filter, priority, flags)?,
            ))
        }

        pub(crate) fn compile_network_filter(&self, filter: &str) -> Result<(), Box<dyn Error>> {
            let filter = CString::new(filter)?;
            let mut error = ptr::null();
            let mut position = 0u32;
            let ok = unsafe {
                (self.0.api.compile_filter)(
                    filter.as_ptr(),
                    LAYER_NETWORK_ABI,
                    ptr::null_mut(),
                    0,
                    &mut error,
                    &mut position,
                )
            };
            if ok == 0 {
                return Err(
                    format!("active WinDivert filter failed compilation at {position}").into(),
                );
            }
            Ok(())
        }

        fn open_at_layer(
            &self,
            filter: &str,
            layer: i32,
            priority: i16,
            flags: u64,
        ) -> Result<WinDivertHandle, Box<dyn Error>> {
            let filter = CString::new(filter)?;
            let handle = unsafe { (self.0.api.open)(filter.as_ptr(), layer, priority, flags) };
            if handle == INVALID_HANDLE_VALUE || handle.is_null() {
                return Err(
                    format!("WinDivertOpen failed with Windows error {}", unsafe {
                        GetLastError()
                    })
                    .into(),
                );
            }
            Ok(WinDivertHandle {
                raw: handle,
                loaded: Arc::clone(&self.0),
            })
        }

        fn reflect_inventory_is_clear(&self, target_priority: i16) -> Result<bool, Box<dyn Error>> {
            let flags = FLAG_SNIFF | FLAG_RECV_ONLY | FLAG_NO_INSTALL;
            let reflect = Arc::new(self.open_at_layer("true", LAYER_REFLECT_ABI, 0, flags)?);
            if reflect.get_param(PARAM_VERSION_MAJOR)? != 2
                || reflect.get_param(PARAM_VERSION_MINOR)? != 2
            {
                return Err("REFLECT arbitration requires loaded WinDivert driver 2.2".into());
            }

            let stop_handle = Arc::clone(&reflect);
            let (timer_send, timer_receive) = std::sync::mpsc::channel();
            let timer = thread::spawn(move || {
                let _ = timer_receive.recv_timeout(BARRIER_TIMEOUT);
                stop_handle.shutdown_receive()
            });
            let sentinel = self.open_network("false", SENTINEL_PRIORITY, flags);
            let result = (|| -> Result<bool, Box<dyn Error>> {
                let _sentinel = sentinel?;
                let mut inventory = ReflectInventory::new(
                    std::process::id(),
                    SENTINEL_PRIORITY,
                    flags,
                    target_priority,
                );
                loop {
                    let (_, address) = reflect
                        .receive(65_535)?
                        .ok_or("REFLECT inventory ended before its sentinel barrier")?;
                    inventory.observe(
                        decode_reflect_event(&address)
                            .map_err(|error| format!("invalid REFLECT address: {error}"))?,
                    )?;
                    if inventory.sentinel_seen {
                        return inventory.affirmative().map_err(|error| error.into());
                    }
                }
            })();
            let _ = timer_send.send(());
            timer
                .join()
                .map_err(|_| "REFLECT arbitration timer panicked")??;
            result
        }
    }

    impl WinDivertHandle {
        /// Starts one overlapped receive while borrowing this handle.
        ///
        /// The returned value owns every pointer passed to WinDivertRecvEx. A
        /// timeout or control wake does not cancel the receive and does not
        /// call WinDivertShutdown; the caller can wait on the same operation
        /// again. Dropping an incomplete operation cancels only that operation
        /// and drains its completion before releasing its storage.
        #[allow(dead_code)]
        pub(crate) fn receive_overlapped(
            &self,
            maximum_bytes: usize,
        ) -> Result<OverlappedReceive<'_>, String> {
            OverlappedReceive::start(self, maximum_bytes)
        }

        pub(crate) fn receive(
            &self,
            maximum_bytes: usize,
        ) -> Result<Option<(Vec<u8>, WinDivertAddress)>, String> {
            let mut bytes = vec![0u8; maximum_bytes];
            let mut address = WinDivertAddress::default();
            let mut length = 0u32;
            let ok = unsafe {
                (self.loaded.api.recv)(
                    self.raw,
                    bytes.as_mut_ptr().cast(),
                    bytes.len() as u32,
                    &mut length,
                    &mut address,
                )
            };
            if ok == 0 {
                let error = unsafe { GetLastError() };
                if error == ERROR_NO_DATA {
                    return Ok(None);
                }
                return Err(format!("WinDivertRecv Windows error {error}"));
            }
            let length = length as usize;
            if length > bytes.len() {
                return Err("WinDivertRecv returned an oversized packet".into());
            }
            bytes.truncate(length);
            Ok(Some((bytes, address)))
        }

        pub(crate) fn send_unchanged(
            &self,
            bytes: &[u8],
            address: &WinDivertAddress,
        ) -> Result<usize, String> {
            let mut sent = 0u32;
            let ok = unsafe {
                (self.loaded.api.send)(
                    self.raw,
                    bytes.as_ptr().cast(),
                    bytes.len() as u32,
                    &mut sent,
                    address,
                )
            };
            if ok == 0 {
                return Err(format!("WinDivertSend Windows error {}", unsafe {
                    GetLastError()
                }));
            }
            Ok(sent as usize)
        }

        pub(crate) fn shutdown_receive(&self) -> Result<(), String> {
            if unsafe { (self.loaded.api.shutdown)(self.raw, SHUTDOWN_RECV) } == 0 {
                return Err(format!(
                    "WinDivertShutdown failed with Windows error {}",
                    unsafe { GetLastError() }
                ));
            }
            Ok(())
        }

        pub(crate) fn set_param(&self, parameter: i32, value: u64) -> Result<(), String> {
            if unsafe { (self.loaded.api.set_param)(self.raw, parameter, value) } == 0 {
                return Err(format!(
                    "WinDivertSetParam failed with Windows error {}",
                    unsafe { GetLastError() }
                ));
            }
            Ok(())
        }

        pub(crate) fn get_param(&self, parameter: i32) -> Result<u64, String> {
            let mut value = 0u64;
            if unsafe { (self.loaded.api.get_param)(self.raw, parameter, &mut value) } == 0 {
                return Err(format!(
                    "WinDivertGetParam failed with Windows error {}",
                    unsafe { GetLastError() }
                ));
            }
            Ok(value)
        }

        /// Attempts to close exactly once. On failure ownership of the live
        /// raw handle is returned to the caller rather than being discarded.
        #[allow(dead_code)]
        pub(crate) fn try_close(mut self) -> Result<(), (Self, String)> {
            if self.raw.is_null() || self.raw == INVALID_HANDLE_VALUE {
                return Ok(());
            }
            if unsafe { (self.loaded.api.close)(self.raw) } == 0 {
                let error = unsafe { GetLastError() };
                return Err((
                    self,
                    format!("WinDivertClose failed with Windows error {error}"),
                ));
            }
            self.raw = INVALID_HANDLE_VALUE;
            Ok(())
        }
    }

    /// A borrowed event which can wake an overlapped receive wait without
    /// cancelling or otherwise changing the WinDivert handle.
    #[derive(Clone, Copy)]
    #[allow(dead_code)]
    pub(crate) struct ReceiveControlEvent(HANDLE);

    #[allow(dead_code)]
    impl ReceiveControlEvent {
        /// The caller must keep `raw` valid for the duration of each wait.
        pub(crate) unsafe fn from_raw(raw: HANDLE) -> Result<Self, &'static str> {
            if raw.is_null() || raw == INVALID_HANDLE_VALUE {
                return Err("receive control event handle was invalid");
            }
            Ok(Self(raw))
        }
    }

    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    #[allow(dead_code)]
    pub(crate) enum OverlappedReceiveWait {
        Completed,
        TimedOut,
        ControlWoken,
    }

    #[allow(dead_code)]
    pub(crate) struct OverlappedReceive<'handle> {
        handle: &'handle WinDivertHandle,
        bytes: Vec<u8>,
        receive_length: Box<u32>,
        address: Box<WinDivertAddress>,
        address_length: Box<u32>,
        overlapped: Box<OVERLAPPED>,
        event: HANDLE,
        state: OverlappedReceiveState,
    }

    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    #[allow(dead_code)]
    enum OverlappedReceiveState {
        Pending,
        Completed(usize),
        Taken,
    }

    #[allow(dead_code)]
    impl OverlappedReceive<'_> {
        fn start(
            handle: &WinDivertHandle,
            maximum_bytes: usize,
        ) -> Result<OverlappedReceive<'_>, String> {
            if maximum_bytes == 0 || maximum_bytes > u32::MAX as usize {
                return Err("WinDivertRecvEx buffer length was out of range".into());
            }
            let event = unsafe { CreateEventW(ptr::null(), 1, 0, ptr::null()) };
            if event.is_null() {
                return Err(format!(
                    "CreateEventW failed with Windows error {}",
                    unsafe { GetLastError() }
                ));
            }
            let mut receive = OverlappedReceive {
                handle,
                bytes: vec![0u8; maximum_bytes],
                receive_length: Box::new(0),
                address: Box::new(WinDivertAddress::default()),
                address_length: Box::new(std::mem::size_of::<WinDivertAddress>() as u32),
                overlapped: Box::new(unsafe { std::mem::zeroed() }),
                event,
                state: OverlappedReceiveState::Pending,
            };
            receive.overlapped.hEvent = event;
            let ok = unsafe {
                (handle.loaded.api.recv_ex)(
                    handle.raw,
                    receive.bytes.as_mut_ptr().cast(),
                    receive.bytes.len() as u32,
                    receive.receive_length.as_mut(),
                    0,
                    receive.address.as_mut(),
                    receive.address_length.as_mut(),
                    receive.overlapped.as_mut(),
                )
            };
            if ok != 0 {
                receive.complete(*receive.receive_length as usize)?;
                return Ok(receive);
            }
            let error = unsafe { GetLastError() };
            if error != ERROR_IO_PENDING {
                receive.state = OverlappedReceiveState::Taken;
                return Err(format!("WinDivertRecvEx Windows error {error}"));
            }
            Ok(receive)
        }

        pub(crate) fn wait(
            &mut self,
            timeout: Duration,
            control: Option<ReceiveControlEvent>,
        ) -> Result<OverlappedReceiveWait, String> {
            match self.state {
                OverlappedReceiveState::Completed(_) => {
                    return Ok(OverlappedReceiveWait::Completed);
                }
                OverlappedReceiveState::Taken => {
                    return Err("overlapped receive result was already taken".into());
                }
                OverlappedReceiveState::Pending => {}
            }
            let handles = [self.event, control.map(|event| event.0).unwrap_or_default()];
            let count = if control.is_some() { 2 } else { 1 };
            let result =
                unsafe { WaitForMultipleObjects(count, handles.as_ptr(), 0, wait_millis(timeout)) };
            if result == WAIT_OBJECT_0 {
                let mut transferred = 0u32;
                if unsafe {
                    GetOverlappedResult(
                        self.handle.raw,
                        self.overlapped.as_mut(),
                        &mut transferred,
                        0,
                    )
                } == 0
                {
                    return Err(format!(
                        "WinDivertRecvEx completion failed with Windows error {}",
                        unsafe { GetLastError() }
                    ));
                }
                self.complete(transferred as usize)?;
                Ok(OverlappedReceiveWait::Completed)
            } else if control.is_some() && result == WAIT_OBJECT_0 + 1 {
                Ok(OverlappedReceiveWait::ControlWoken)
            } else if result == WAIT_TIMEOUT {
                Ok(OverlappedReceiveWait::TimedOut)
            } else if result == WAIT_FAILED {
                Err(format!(
                    "overlapped receive wait failed with Windows error {}",
                    unsafe { GetLastError() }
                ))
            } else {
                Err(format!(
                    "overlapped receive wait returned unexpected status {result}"
                ))
            }
        }

        pub(crate) fn take_packet(
            &mut self,
        ) -> Result<Option<(Vec<u8>, WinDivertAddress)>, String> {
            let OverlappedReceiveState::Completed(length) = self.state else {
                return match self.state {
                    OverlappedReceiveState::Pending => Ok(None),
                    OverlappedReceiveState::Taken => {
                        Err("overlapped receive result was already taken".into())
                    }
                    OverlappedReceiveState::Completed(_) => unreachable!(),
                };
            };
            let mut bytes = std::mem::take(&mut self.bytes);
            bytes.truncate(length);
            let address = std::mem::take(self.address.as_mut());
            self.state = OverlappedReceiveState::Taken;
            Ok(Some((bytes, address)))
        }

        fn complete(&mut self, length: usize) -> Result<(), String> {
            if let Err(error) =
                validate_receive_completion(self.bytes.len(), length, *self.address_length as usize)
            {
                self.state = OverlappedReceiveState::Taken;
                return Err(error.into());
            }
            self.state = OverlappedReceiveState::Completed(length);
            Ok(())
        }
    }

    impl Drop for OverlappedReceive<'_> {
        fn drop(&mut self) {
            if self.state == OverlappedReceiveState::Pending {
                unsafe {
                    // Cancellation is scoped to this OVERLAPPED operation. Do
                    // not use WinDivertShutdown for an ordinary timeout/wake.
                    CancelIoEx(self.handle.raw, self.overlapped.as_mut());
                    let mut transferred = 0u32;
                    GetOverlappedResult(
                        self.handle.raw,
                        self.overlapped.as_mut(),
                        &mut transferred,
                        1,
                    );
                }
            }
            if !self.event.is_null() {
                unsafe { CloseHandle(self.event) };
                self.event = ptr::null_mut();
            }
        }
    }

    #[allow(dead_code)]
    pub(super) fn wait_millis(timeout: Duration) -> u32 {
        u32::try_from(timeout.as_millis()).unwrap_or(u32::MAX - 1)
    }

    pub(super) fn validate_receive_completion(
        packet_capacity: usize,
        packet_length: usize,
        address_length: usize,
    ) -> Result<(), &'static str> {
        if packet_length > packet_capacity {
            return Err("WinDivertRecvEx returned an oversized packet");
        }
        if address_length != std::mem::size_of::<WinDivertAddress>() {
            return Err("WinDivertRecvEx returned an invalid address length");
        }
        Ok(())
    }
}

#[cfg(windows)]
pub(crate) use windows_backend::*;

#[cfg(test)]
mod tests {
    use super::*;

    fn event(kind: ReflectEventKind, identity: ReflectedHandleIdentity) -> ReflectedHandleEvent {
        ReflectedHandleEvent { kind, identity }
    }

    fn address(kind: ReflectEventKind, identity: ReflectedHandleIdentity) -> WinDivertAddress {
        let mut address = WinDivertAddress::default();
        let event = match kind {
            ReflectEventKind::Open => EVENT_REFLECT_OPEN,
            ReflectEventKind::Close => EVENT_REFLECT_CLOSE,
        };
        address.0[1] = u64::from(LAYER_REFLECT) | (u64::from(event) << 8);
        address.0[2] = identity.opened_timestamp as u64;
        address.0[3] = u64::from(identity.process_id) | (u64::from(identity.layer) << 32);
        address.0[4] = identity.flags;
        address.0[5] = u64::from(identity.priority as u16);
        address
    }

    #[test]
    fn exact_reflect_address_fields_decode_and_malformed_events_reject() {
        let identity = ReflectedHandleIdentity {
            opened_timestamp: -91,
            process_id: 4242,
            layer: LAYER_NETWORK,
            flags: 21,
            priority: -1000,
        };
        assert_eq!(
            decode_reflect_event(&address(ReflectEventKind::Open, identity)),
            Ok(event(ReflectEventKind::Open, identity))
        );
        assert!(decode_reflect_event(&WinDivertAddress::default()).is_err());
    }

    #[test]
    fn sentinel_is_an_ordering_barrier_and_same_priority_network_blocks() {
        let sentinel = ReflectedHandleIdentity {
            opened_timestamp: 2,
            process_id: 42,
            layer: LAYER_NETWORK,
            flags: 21,
            priority: -1000,
        };
        let mut clear = ReflectInventory::new(42, -1000, 21, 0);
        clear
            .observe(event(ReflectEventKind::Open, sentinel))
            .unwrap();
        assert_eq!(clear.affirmative(), Ok(true));

        let mut blocked = ReflectInventory::new(42, -1000, 21, 0);
        blocked
            .observe(event(
                ReflectEventKind::Open,
                ReflectedHandleIdentity {
                    opened_timestamp: 1,
                    process_id: 7,
                    layer: LAYER_NETWORK,
                    flags: 0,
                    priority: 0,
                },
            ))
            .unwrap();
        blocked
            .observe(event(ReflectEventKind::Open, sentinel))
            .unwrap();
        assert_eq!(blocked.affirmative(), Ok(false));
    }

    #[test]
    fn lifecycle_ambiguity_and_missing_barrier_fail_closed() {
        let identity = ReflectedHandleIdentity {
            opened_timestamp: 1,
            process_id: 7,
            layer: LAYER_NETWORK,
            flags: 0,
            priority: 0,
        };
        let mut inventory = ReflectInventory::new(42, -1000, 21, 0);
        assert!(inventory.affirmative().is_err());
        assert!(
            inventory
                .observe(event(ReflectEventKind::Close, identity))
                .is_err()
        );
        inventory
            .observe(event(ReflectEventKind::Open, identity))
            .unwrap();
        assert!(
            inventory
                .observe(event(ReflectEventKind::Open, identity))
                .is_err()
        );
    }

    #[test]
    fn exact_close_removes_the_matching_open() {
        let identity = ReflectedHandleIdentity {
            opened_timestamp: 1,
            process_id: 7,
            layer: LAYER_NETWORK,
            flags: 0,
            priority: 0,
        };
        let sentinel = ReflectedHandleIdentity {
            opened_timestamp: 2,
            process_id: 42,
            layer: LAYER_NETWORK,
            flags: 21,
            priority: -1000,
        };
        let mut inventory = ReflectInventory::new(42, -1000, 21, 0);
        inventory
            .observe(event(ReflectEventKind::Open, identity))
            .unwrap();
        inventory
            .observe(event(ReflectEventKind::Close, identity))
            .unwrap();
        inventory
            .observe(event(ReflectEventKind::Open, sentinel))
            .unwrap();
        assert_eq!(inventory.affirmative(), Ok(true));
    }

    #[cfg(windows)]
    #[test]
    fn overlapped_wait_timeout_conversion_is_bounded_and_never_infinite() {
        use std::time::Duration;

        assert_eq!(windows_backend::wait_millis(Duration::ZERO), 0);
        assert_eq!(
            windows_backend::wait_millis(Duration::from_millis(1234)),
            1234
        );
        assert_eq!(
            windows_backend::wait_millis(Duration::from_millis(u64::MAX)),
            u32::MAX - 1
        );
        assert!(
            unsafe { windows_backend::ReceiveControlEvent::from_raw(std::ptr::null_mut()) }
                .is_err()
        );
        assert!(
            unsafe {
                windows_backend::ReceiveControlEvent::from_raw(
                    windows_sys::Win32::Foundation::INVALID_HANDLE_VALUE,
                )
            }
            .is_err()
        );
    }

    #[cfg(windows)]
    #[test]
    fn overlapped_receive_completion_rejects_oversize_and_address_ambiguity() {
        let address_size = std::mem::size_of::<WinDivertAddress>();
        assert_eq!(
            windows_backend::validate_receive_completion(65_535, 1234, address_size),
            Ok(())
        );
        assert!(windows_backend::validate_receive_completion(100, 101, address_size).is_err());
        assert!(windows_backend::validate_receive_completion(100, 100, address_size - 1).is_err());
        assert!(windows_backend::validate_receive_completion(100, 100, address_size + 1).is_err());
    }
}
