//! Explicit, time-bounded, byte-identical WinDivert pass-through canary.
//!
//! This research binary has deliberately separate dry-run and armed modes.
//! Dry-run never loads WinDivert or opens a handle. Armed mode validates the
//! pinned files, elevation, exact process name, an outbound SYN, and exact
//! process-owned four-tuple before opening one narrow NETWORK handle. Every
//! packet and its complete opaque WINDIVERT_ADDRESS are reinjected unchanged.
//! There is no marker substitution, payload mutation, or checksum rewrite path.

use std::{error::Error, fmt};

const ARM_TOKEN: &str = "RLOGS_WINDIVERT_BYTE_IDENTICAL_PASSTHROUGH_V1";
const BOOTSTRAP_TOKEN: &str = "RLOGS_WINDIVERT_DRIVER_BOOTSTRAP_V1";
const MAX_PACKET_BYTES: usize = 65_535;

#[repr(C)]
#[derive(Debug, Default, Clone, PartialEq, Eq)]
struct OpaqueAddress([u64; 10]);
const _: [(); 80] = [(); std::mem::size_of::<OpaqueAddress>()];

const WINDIVERT_LAYER_NETWORK_VALUE: u32 = 0;
const WINDIVERT_LAYER_REFLECT_VALUE: u8 = 4;
const WINDIVERT_EVENT_REFLECT_OPEN_VALUE: u8 = 8;
const WINDIVERT_EVENT_REFLECT_CLOSE_VALUE: u8 = 9;

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

/// Decode only the documented WinDivert 2.2.2 REFLECT fields from the exact
/// 80-byte `WINDIVERT_ADDRESS`. Windows/x64 is little-endian, and the first
/// two bytes of word 1 are the public Layer/Event bitfields.
fn decode_reflect_event(address: &OpaqueAddress) -> Result<ReflectedHandleEvent, &'static str> {
    let header = address.0[1];
    if header as u8 != WINDIVERT_LAYER_REFLECT_VALUE {
        return Err("event did not originate at the REFLECT layer");
    }
    let kind = match (header >> 8) as u8 {
        WINDIVERT_EVENT_REFLECT_OPEN_VALUE => ReflectEventKind::Open,
        WINDIVERT_EVENT_REFLECT_CLOSE_VALUE => ReflectEventKind::Close,
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

#[derive(Debug)]
struct ReflectArbitrator {
    sentinel_process_id: u32,
    sentinel_priority: i16,
    sentinel_flags: u64,
    target_priority: i16,
    open_handles: Vec<ReflectedHandleIdentity>,
    sentinel_seen: bool,
}

impl ReflectArbitrator {
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
                    && event.identity.layer == WINDIVERT_LAYER_NETWORK_VALUE
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
        // Semantic overlap between arbitrary WinDivert filters cannot be
        // decided safely here. Treat every same-priority NETWORK handle as
        // overlapping, which is deliberately stricter than necessary.
        Ok(!self.open_handles.iter().any(|identity| {
            identity.layer == WINDIVERT_LAYER_NETWORK_VALUE
                && identity.priority == self.target_priority
        }))
    }
}

struct ReceivedPacket {
    bytes: Vec<u8>,
    address: OpaqueAddress,
}

trait PassthroughBackend {
    fn receive(&mut self) -> Result<Option<ReceivedPacket>, String>;
    fn send_unchanged(&mut self, bytes: &[u8], address: &OpaqueAddress) -> Result<usize, String>;
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
struct RelayCounts {
    packets_received: u64,
    packets_reinjected: u64,
    bytes_received: u64,
    bytes_reinjected: u64,
}

#[derive(Debug, PartialEq, Eq)]
enum RelayError {
    Receive(String),
    Send(String),
    LengthMismatch { received: usize, sent: usize },
}

impl fmt::Display for RelayError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Receive(error) => write!(formatter, "receive failed: {error}"),
            Self::Send(error) => write!(formatter, "reinject failed: {error}"),
            Self::LengthMismatch { received, sent } => {
                write!(formatter, "reinject length mismatch ({sent} of {received})")
            }
        }
    }
}

impl Error for RelayError {}

/// Drain a backend until it reports no more data. The owned receive buffer and
/// opaque address are passed directly to send; no mutable packet reference is
/// exposed after receive returns.
fn drain_byte_identically<B: PassthroughBackend>(
    backend: &mut B,
) -> Result<RelayCounts, RelayError> {
    let mut counts = RelayCounts::default();
    loop {
        let Some(received) = backend.receive().map_err(RelayError::Receive)? else {
            return Ok(counts);
        };
        counts.packets_received += 1;
        counts.bytes_received += received.bytes.len() as u64;
        let sent = backend
            .send_unchanged(received.bytes.as_slice(), &received.address)
            .map_err(RelayError::Send)?;
        if sent != received.bytes.len() {
            return Err(RelayError::LengthMismatch {
                received: received.bytes.len(),
                sent,
            });
        }
        counts.packets_reinjected += 1;
        counts.bytes_reinjected += sent as u64;
    }
}

#[cfg(not(windows))]
fn main() {
    eprintln!("Automarker WinDivert pass-through canary is available only on Windows");
    std::process::exit(1);
}

#[cfg(windows)]
mod windows {
    use super::*;
    use std::{
        collections::{HashMap, HashSet},
        env,
        ffi::{CString, OsString, c_void},
        fs::{self, OpenOptions},
        io::{BufWriter, Write},
        mem::{size_of, transmute},
        net::{IpAddr, Ipv4Addr},
        os::windows::ffi::{OsStrExt, OsStringExt},
        path::{Path, PathBuf},
        process::Command,
        ptr,
        sync::Arc,
        thread,
        time::Duration,
    };

    use rlogs_capture::{TcpConnection, WindowsProcessSocketOwner};
    use rlogs_game_bpsr::{
        AUTOMARKER_REQUEST_BUILD, AUTOMARKER_WINDIVERT_X64_DLL_SHA256,
        AUTOMARKER_WINDIVERT_X64_DRIVER_SHA256, AutomarkerIpv4Endpoint,
        AutomarkerOwnedTcpConnection, bind_offline_automarker_connection_epoch,
        classify_bpsr_tcp_prefix, classify_observed_automarker_tcp_prefix,
        offline_automarker_windivert_active_filter,
    };
    use serde::Serialize;
    use sha2::{Digest, Sha256};
    use windows_sys::Win32::{
        Foundation::{
            CloseHandle, ERROR_NO_DATA, FreeLibrary, GetLastError, HANDLE, INVALID_HANDLE_VALUE,
        },
        Security::{GetTokenInformation, TOKEN_ELEVATION, TOKEN_QUERY, TokenElevation},
        System::{
            Diagnostics::ToolHelp::{
                CreateToolhelp32Snapshot, PROCESSENTRY32W, Process32FirstW, Process32NextW,
                TH32CS_SNAPPROCESS,
            },
            LibraryLoader::{GetProcAddress, LOAD_LIBRARY_SEARCH_DLL_LOAD_DIR, LoadLibraryExW},
            Threading::{GetCurrentProcess, OpenProcessToken},
        },
    };

    const DLL_NAME: &str = "WinDivert.dll";
    const DRIVER_NAME: &str = "WinDivert64.sys";
    const EXPECTED_DRIVER_SIGNER_THUMBPRINT: &str = "043589F75FCE2795E7F2CC3E526D46784D5DDAB3";
    const WINDIVERT_LAYER_NETWORK: i32 = 0;
    const WINDIVERT_LAYER_REFLECT: i32 = 4;
    const WINDIVERT_FLAG_SNIFF: u64 = 1;
    const WINDIVERT_FLAG_RECV_ONLY: u64 = 4;
    const WINDIVERT_FLAG_NO_INSTALL: u64 = 16;
    const WINDIVERT_SHUTDOWN_RECV: i32 = 0x1;
    const WINDIVERT_PARAM_QUEUE_LENGTH: i32 = 0;
    const WINDIVERT_PARAM_QUEUE_TIME: i32 = 1;
    const WINDIVERT_PARAM_QUEUE_SIZE: i32 = 2;
    const WINDIVERT_PARAM_VERSION_MAJOR: i32 = 3;
    const WINDIVERT_PARAM_VERSION_MINOR: i32 = 4;
    const REFLECT_SENTINEL_PRIORITY: i16 = -1000;
    const BOOTSTRAP_PRIORITY: i16 = -999;
    const REFLECT_BARRIER_TIMEOUT: Duration = Duration::from_secs(5);
    static ARBITRATION_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

    type OpenFn = unsafe extern "system" fn(*const i8, i32, i16, u64) -> HANDLE;
    type RecvFn =
        unsafe extern "system" fn(HANDLE, *mut c_void, u32, *mut u32, *mut OpaqueAddress) -> i32;
    type SendFn = unsafe extern "system" fn(
        HANDLE,
        *const c_void,
        u32,
        *mut u32,
        *const OpaqueAddress,
    ) -> i32;
    type ShutdownFn = unsafe extern "system" fn(HANDLE, i32) -> i32;
    type CloseFn = unsafe extern "system" fn(HANDLE) -> i32;
    type SetParamFn = unsafe extern "system" fn(HANDLE, i32, u64) -> i32;
    type GetParamFn = unsafe extern "system" fn(HANDLE, i32, *mut u64) -> i32;
    type CompileFilterFn =
        unsafe extern "system" fn(*const i8, i32, *mut i8, u32, *mut *const i8, *mut u32) -> i32;

    #[derive(Clone, Copy)]
    struct WinDivertApi {
        open: OpenFn,
        recv: RecvFn,
        send: SendFn,
        shutdown: ShutdownFn,
        close: CloseFn,
        set_param: SetParamFn,
        get_param: GetParamFn,
        compile_filter: CompileFilterFn,
    }

    struct LoadedApi {
        module: windows_sys::Win32::Foundation::HMODULE,
        api: WinDivertApi,
    }

    // The pinned module is retained by Arc until every handle and worker is
    // gone; the exported WinDivert entry points are safe for concurrent calls.
    unsafe impl Send for LoadedApi {}
    unsafe impl Sync for LoadedApi {}

    impl Drop for LoadedApi {
        fn drop(&mut self) {
            unsafe { FreeLibrary(self.module) };
        }
    }

    struct OwnedWinDivertHandle {
        raw: HANDLE,
        loaded: Arc<LoadedApi>,
    }

    unsafe impl Send for OwnedWinDivertHandle {}
    unsafe impl Sync for OwnedWinDivertHandle {}

    impl Drop for OwnedWinDivertHandle {
        fn drop(&mut self) {
            if !self.raw.is_null() && self.raw != INVALID_HANDLE_VALUE {
                unsafe { (self.loaded.api.close)(self.raw) };
            }
        }
    }

    struct OwnedKernelHandle(HANDLE);

    enum ArbitratedNetworkOpen {
        Open(OwnedWinDivertHandle),
        Conflict,
    }

    impl Drop for OwnedKernelHandle {
        fn drop(&mut self) {
            if !self.0.is_null() && self.0 != INVALID_HANDLE_VALUE {
                unsafe { CloseHandle(self.0) };
            }
        }
    }

    struct LiveBackend {
        handle: Arc<OwnedWinDivertHandle>,
        api: WinDivertApi,
    }

    impl PassthroughBackend for LiveBackend {
        fn receive(&mut self) -> Result<Option<ReceivedPacket>, String> {
            let mut bytes = vec![0u8; MAX_PACKET_BYTES];
            let mut address = OpaqueAddress::default();
            let mut length = 0u32;
            let ok = unsafe {
                (self.api.recv)(
                    self.handle.raw,
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
            Ok(Some(ReceivedPacket { bytes, address }))
        }

        fn send_unchanged(
            &mut self,
            bytes: &[u8],
            address: &OpaqueAddress,
        ) -> Result<usize, String> {
            let mut sent = 0u32;
            let ok = unsafe {
                (self.api.send)(
                    self.handle.raw,
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
    }

    #[derive(Serialize)]
    struct GateReceipt {
        explicit_literal_consent: bool,
        elevated: bool,
        pinned_dll_hash: bool,
        pinned_driver_hash: bool,
        driver_signature_and_signer: bool,
        bfe_running: bool,
        exact_process_name: bool,
        exact_ipv4_tuple_discovered: bool,
        syn_observed: bool,
        exact_process_owned_epoch: bool,
        same_priority_reflect_arbitration: bool,
        reflect_arbitration_checks: u8,
        exact_filter_compiled: bool,
        exact_filter_opened: bool,
        version_2_2: bool,
        queue_policy_readback: bool,
    }

    #[derive(Serialize)]
    struct CountsReceipt {
        packets_received: u64,
        packets_reinjected: u64,
        bytes_received: u64,
        bytes_reinjected: u64,
    }

    #[derive(Serialize)]
    struct CanaryReceipt {
        schema_version: u32,
        artifact_kind: &'static str,
        game_build: &'static str,
        mode: &'static str,
        outcome: String,
        gates: GateReceipt,
        counts: CountsReceipt,
        invariants: InvariantReceipt,
    }

    #[derive(Serialize)]
    struct InvariantReceipt {
        single_inflight_synchronous: bool,
        complete_opaque_address_preserved: bool,
        every_received_packet_reinjected: bool,
        packet_lengths_preserved: bool,
        packet_bytes_preserved: bool,
        shutdown_receive_only_then_drained: bool,
        substitution_enabled: bool,
        checksum_rewrite_enabled: bool,
        endpoints_persisted: bool,
        tcp_sequence_or_ack_persisted: bool,
        payloads_persisted: bool,
        protocol_or_account_ids_persisted: bool,
    }

    struct Arguments {
        armed: bool,
        bootstrap: bool,
        process_id: Option<u32>,
        dependency_directory: Option<PathBuf>,
        syn_wait_seconds: u64,
        duration_seconds: u64,
        output: PathBuf,
    }

    pub fn main() {
        if let Err(error) = run() {
            eprintln!("Automarker pass-through canary failed: {error}");
            std::process::exit(1);
        }
    }

    fn run() -> Result<(), Box<dyn Error>> {
        let args = Arguments::parse(env::args_os().skip(1).collect())?;
        if args.output.exists() {
            return Err(format!("refusing to overwrite {}", args.output.display()).into());
        }
        if !args.armed {
            return write_receipt(
                &args.output,
                CanaryReceipt::dry_run("dry-run-no-driver-load-no-handle-open"),
            );
        }

        let process_id = args.process_id.ok_or("armed mode requires --process-id")?;
        let dependency_directory = args
            .dependency_directory
            .as_deref()
            .ok_or("armed mode requires --dependency-directory")?;
        let mut gates = GateReceipt::new_armed();
        gates.elevated = is_elevated()?;
        if !gates.elevated {
            return Err("armed mode requires an elevated Administrator console".into());
        }
        let dll = dependency_directory.join(DLL_NAME);
        let driver = dependency_directory.join(DRIVER_NAME);
        gates.pinned_dll_hash = sha256_is(&dll, AUTOMARKER_WINDIVERT_X64_DLL_SHA256)?;
        gates.pinned_driver_hash = sha256_is(&driver, AUTOMARKER_WINDIVERT_X64_DRIVER_SHA256)?;
        if !gates.pinned_dll_hash || !gates.pinned_driver_hash {
            return Err("WinDivert files do not match the pinned official 2.2.2 x64 hashes".into());
        }
        gates.driver_signature_and_signer = verify_driver_signature(&driver)?;
        if !gates.driver_signature_and_signer {
            return Err(
                "WinDivert driver signature or signer thumbprint is not the pinned value".into(),
            );
        }
        gates.bfe_running = base_filtering_engine_running()?;
        if !gates.bfe_running {
            return Err("Windows Base Filtering Engine is not reported RUNNING".into());
        }
        gates.exact_process_name = exact_process_name(process_id, "BPSR_STEAM.exe")?;
        if !gates.exact_process_name {
            return Err("--process-id is not exactly one live BPSR_STEAM.exe process".into());
        }
        gates.exact_ipv4_tuple_discovered = false;

        // Dynamic loading occurs only after literal consent and all non-driver gates.
        let loaded = Arc::new(unsafe { load_pinned_api(&dll)? });
        // A separately consented bootstrap may install/start the pinned
        // driver, but its false filter at the lowest priority cannot overlap
        // traffic. It stays alive so both arbitration passes and the canary
        // use the same loaded driver instance.
        let bootstrap_handle = if args.bootstrap {
            let handle = open_network_handle(
                &loaded,
                "false",
                BOOTSTRAP_PRIORITY,
                WINDIVERT_FLAG_SNIFF | WINDIVERT_FLAG_RECV_ONLY,
            )?;
            configure_and_verify(&loaded.api, handle.raw, &mut gates)?;
            Some(handle)
        } else {
            None
        };

        // WinDivert documents equal-priority ordering as undefined. A
        // REFLECT snapshot is serialized with a false-filter NETWORK sentinel;
        // seeing the sentinel OPEN is the barrier proving all earlier handle
        // events were consumed. Any priority-0 NETWORK handle is treated as
        // overlapping, regardless of its filter text.
        gates.reflect_arbitration_checks = 1;
        let discovery_handle = match open_network_handle_after_reflect_arbitration(
            &loaded,
            "outbound and ip and tcp",
            1,
            WINDIVERT_FLAG_SNIFF | WINDIVERT_FLAG_RECV_ONLY | WINDIVERT_FLAG_NO_INSTALL,
        )? {
            ArbitratedNetworkOpen::Open(handle) => {
                gates.same_priority_reflect_arbitration = true;
                handle
            }
            ArbitratedNetworkOpen::Conflict => {
                return write_receipt(
                    &args.output,
                    CanaryReceipt::blocked_armed(
                        if args.bootstrap {
                            "explicitly-armed-bootstrap-and-passthrough"
                        } else {
                            "explicitly-armed"
                        },
                        "blocked_same_priority_network_handle",
                        gates,
                    ),
                );
            }
        };
        let owner = WindowsProcessSocketOwner::new(process_id)?;
        let connection = discover_exact_bpsr_syn_epoch(
            &loaded,
            discovery_handle,
            &owner,
            process_id,
            args.syn_wait_seconds,
        )?;
        let filter = offline_automarker_windivert_active_filter(connection);
        gates.syn_observed = true;
        gates.exact_ipv4_tuple_discovered = true;

        let owned = owner.snapshot_all_connections()?;
        let expected = TcpConnection::new(
            rlogs_capture::TcpEndpoint::new(
                IpAddr::V4(connection.local.address),
                connection.local.port,
            ),
            rlogs_capture::TcpEndpoint::new(
                IpAddr::V4(connection.remote.address),
                connection.remote.port,
            ),
        );
        let owned_matches = owned
            .iter()
            .filter(|candidate| **candidate == expected)
            .count();
        let binding = bind_offline_automarker_connection_epoch(
            process_id,
            connection,
            1,
            true,
            &vec![connection; owned_matches],
        )
        .map_err(|error| format!("exact epoch binding failed: {error:?}"))?;
        if binding.connection() != connection {
            return Err("connection epoch binding changed unexpectedly".into());
        }
        gates.exact_process_owned_epoch = true;

        // Discovery can take minutes, so its earlier barrier is not reused.
        // Compile first, then inventory and open the active handle while still
        // holding this process's arbitration lock.
        gates.reflect_arbitration_checks = 2;
        compile_network_filter(&loaded.api, &filter)?;
        gates.exact_filter_compiled = true;
        gates.same_priority_reflect_arbitration = false;
        let active = match open_network_handle_after_reflect_arbitration(
            &loaded,
            &filter,
            0,
            WINDIVERT_FLAG_NO_INSTALL,
        )? {
            ArbitratedNetworkOpen::Open(handle) => {
                gates.same_priority_reflect_arbitration = true;
                handle
            }
            ArbitratedNetworkOpen::Conflict => {
                return write_receipt(
                    &args.output,
                    CanaryReceipt::blocked_armed(
                        if args.bootstrap {
                            "explicitly-armed-bootstrap-and-passthrough"
                        } else {
                            "explicitly-armed"
                        },
                        "blocked_same_priority_network_handle",
                        gates,
                    ),
                );
            }
        };
        gates.exact_filter_opened = true;
        configure_and_verify(&loaded.api, active.raw, &mut gates)?;

        let active = Arc::new(active);
        let stop_handle = Arc::clone(&active);
        let duration = args.duration_seconds;
        let (stop_send, stop_receive) = std::sync::mpsc::channel();
        let timer = thread::spawn(move || {
            let _ = stop_receive.recv_timeout(Duration::from_secs(duration));
            let ok = unsafe {
                (stop_handle.loaded.api.shutdown)(stop_handle.raw, WINDIVERT_SHUTDOWN_RECV)
            };
            if ok == 0 {
                Err(unsafe { GetLastError() })
            } else {
                Ok(())
            }
        });
        println!(
            "Armed byte-identical pass-through is active for {duration} seconds. Do not close this console."
        );
        let mut backend = LiveBackend {
            handle: active,
            api: loaded.api,
        };
        let relay_result = drain_byte_identically(&mut backend);
        let _ = stop_send.send(());
        let shutdown_result = timer
            .join()
            .map_err(|_| "shutdown timer panicked")?
            .map_err(|error| format!("WinDivertShutdown failed with Windows error {error}"));
        shutdown_result?;
        let relay = relay_result?;

        let conserved = relay.packets_received == relay.packets_reinjected
            && relay.bytes_received == relay.bytes_reinjected;
        let receipt = CanaryReceipt {
            schema_version: 1,
            artifact_kind: "sanitized-automarker-windivert-byte-identical-passthrough",
            game_build: AUTOMARKER_REQUEST_BUILD,
            mode: if args.bootstrap {
                "explicitly-armed-bootstrap-and-passthrough"
            } else {
                "explicitly-armed"
            },
            outcome: if conserved {
                "pass"
            } else {
                "fail-conservation"
            }
            .into(),
            gates,
            counts: CountsReceipt::from(relay),
            invariants: InvariantReceipt::armed(conserved),
        };
        drop(bootstrap_handle);
        write_receipt(&args.output, receipt)
    }

    fn discover_exact_bpsr_syn_epoch(
        api_owner: &Arc<LoadedApi>,
        handle: OwnedWinDivertHandle,
        owner: &WindowsProcessSocketOwner,
        process_id: u32,
        wait_seconds: u64,
    ) -> Result<AutomarkerOwnedTcpConnection, Box<dyn Error>> {
        let loaded = Arc::clone(api_owner);
        let api = loaded.api;
        let handle = Arc::new(handle);
        let stop_handle = Arc::clone(&handle);
        let shutdown = api.shutdown;
        let (timer_send, timer_receive) = std::sync::mpsc::channel();
        let timer = thread::spawn(move || {
            if matches!(
                timer_receive.recv_timeout(Duration::from_secs(wait_seconds)),
                Ok(DiscoveryTimerSignal::CandidateFound)
            ) {
                let _ = timer_receive.recv_timeout(Duration::from_millis(250));
            }
            let ok = unsafe { shutdown(stop_handle.raw, WINDIVERT_SHUTDOWN_RECV) };
            if ok == 0 {
                Err(unsafe { GetLastError() })
            } else {
                Ok(())
            }
        });
        println!(
            "Waiting up to {wait_seconds} seconds for a SYN-scoped, process-owned BPSR connection..."
        );
        let mut backend = LiveBackend { handle, api };
        let mut syn_streams = HashMap::<AutomarkerOwnedTcpConnection, DiscoveryPrefix>::new();
        let mut confirmed = HashSet::new();
        let candidate_timer = timer_send.clone();
        let discovery_result = (|| -> Result<(), Box<dyn Error>> {
            while let Some(packet) = backend.receive()? {
                let Some(view) = parse_ipv4_tcp(packet.bytes.as_slice()) else {
                    continue;
                };
                let connection = AutomarkerOwnedTcpConnection {
                    process_id,
                    local: AutomarkerIpv4Endpoint {
                        address: view.source_address,
                        port: view.source_port,
                    },
                    remote: AutomarkerIpv4Endpoint {
                        address: view.destination_address,
                        port: view.destination_port,
                    },
                };
                if view.syn && !view.ack {
                    let start_sequence = view.sequence.wrapping_add(1);
                    syn_streams
                        .entry(connection)
                        .and_modify(|prefix| {
                            if prefix.start_sequence != start_sequence {
                                *prefix = DiscoveryPrefix::new(start_sequence);
                            }
                        })
                        .or_insert_with(|| DiscoveryPrefix::new(start_sequence));
                }
                if let Some(prefix) = syn_streams.get_mut(&connection)
                    && !view.payload.is_empty()
                    && prefix.ingest(view.sequence, view.payload)
                {
                    let regular = classify_bpsr_tcp_prefix(prefix.bytes());
                    let marker = classify_observed_automarker_tcp_prefix(prefix.bytes());
                    if matches!(regular, rlogs_capture::TcpPayloadSignatureResult::Match(_))
                        || matches!(marker, rlogs_capture::TcpPayloadSignatureResult::Match(_))
                    {
                        if owned_connection_count(owner, connection)? == 1
                            && confirmed.insert(connection)
                            && confirmed.len() == 1
                        {
                            let _ = candidate_timer.send(DiscoveryTimerSignal::CandidateFound);
                        }
                    } else if matches!(regular, rlogs_capture::TcpPayloadSignatureResult::Reject)
                        && matches!(marker, rlogs_capture::TcpPayloadSignatureResult::Reject)
                    {
                        prefix.reject();
                    }
                }
            }
            Ok(())
        })();
        let _ = timer_send.send(DiscoveryTimerSignal::StopNow);
        timer
            .join()
            .map_err(|_| "SYN timer panicked")?
            .map_err(|error| format!("WinDivertShutdown failed with Windows error {error}"))?;
        discovery_result?;
        match confirmed.into_iter().collect::<Vec<_>>().as_slice() {
            [connection] => Ok(*connection),
            [] => Err("no SYN-scoped, exact-process-owned BPSR connection was confirmed; start the canary before reconnecting the game".into()),
            _ => Err("more than one SYN-scoped BPSR connection was confirmed for the game process".into()),
        }
    }

    #[derive(Clone, Copy)]
    enum DiscoveryTimerSignal {
        CandidateFound,
        StopNow,
    }

    struct DiscoveryPrefix {
        start_sequence: u32,
        bytes: Vec<u8>,
        pending: HashMap<u32, Vec<u8>>,
        rejected: bool,
    }

    impl DiscoveryPrefix {
        const MAX_BYTES: usize = 65_536;

        fn new(start_sequence: u32) -> Self {
            Self {
                start_sequence,
                bytes: Vec::new(),
                pending: HashMap::new(),
                rejected: false,
            }
        }

        fn bytes(&self) -> &[u8] {
            &self.bytes
        }

        fn reject(&mut self) {
            self.rejected = true;
            self.pending.clear();
        }

        /// Assemble only bytes in the first 64 KiB after the observed SYN.
        /// Exact duplicate/overlapping retransmissions are accepted; gaps are
        /// retained until their predecessor arrives; conflicting or ambiguous
        /// serial ranges permanently reject this candidate.
        fn ingest(&mut self, sequence: u32, payload: &[u8]) -> bool {
            if self.rejected || payload.is_empty() {
                return false;
            }
            if !self.ingest_one(sequence, payload) {
                self.reject();
                return false;
            }
            while let Some((&sequence, _)) = self.pending.iter().find(|(sequence, _)| {
                sequence.wrapping_sub(self.start_sequence) as usize <= self.bytes.len()
            }) {
                let payload = self
                    .pending
                    .remove(&sequence)
                    .expect("pending segment exists");
                if !self.ingest_one(sequence, &payload) {
                    self.reject();
                    return false;
                }
            }
            true
        }

        fn ingest_one(&mut self, sequence: u32, payload: &[u8]) -> bool {
            let delta = sequence.wrapping_sub(self.start_sequence);
            if delta >= 0x8000_0000 {
                return false;
            }
            let offset = delta as usize;
            let Some(end) = offset.checked_add(payload.len()) else {
                return false;
            };
            if end > Self::MAX_BYTES {
                return false;
            }
            if offset > self.bytes.len() {
                match self.pending.get(&sequence) {
                    Some(existing) => return existing == payload,
                    None => {
                        let pending_bytes = self
                            .pending
                            .values()
                            .map(Vec::len)
                            .sum::<usize>()
                            .saturating_add(payload.len());
                        if self.bytes.len().saturating_add(pending_bytes) > Self::MAX_BYTES {
                            return false;
                        }
                        self.pending.insert(sequence, payload.to_vec());
                        return true;
                    }
                }
            }
            let overlap = payload.len().min(self.bytes.len() - offset);
            if self.bytes[offset..offset + overlap] != payload[..overlap] {
                return false;
            }
            if overlap < payload.len() {
                self.bytes.extend_from_slice(&payload[overlap..]);
            }
            true
        }
    }

    #[derive(Clone, Copy)]
    struct Ipv4TcpView<'a> {
        source_address: Ipv4Addr,
        destination_address: Ipv4Addr,
        source_port: u16,
        destination_port: u16,
        sequence: u32,
        syn: bool,
        ack: bool,
        payload: &'a [u8],
    }

    fn parse_ipv4_tcp(packet: &[u8]) -> Option<Ipv4TcpView<'_>> {
        if packet.len() < 40 || packet[0] >> 4 != 4 || packet[9] != 6 {
            return None;
        }
        let ip_header = usize::from(packet[0] & 0x0f) * 4;
        let total = usize::from(u16::from_be_bytes([packet[2], packet[3]]));
        if ip_header < 20 || total != packet.len() || total < ip_header + 20 {
            return None;
        }
        let fragment = u16::from_be_bytes([packet[6], packet[7]]);
        if fragment & 0x3fff != 0 {
            return None;
        }
        let tcp_header = usize::from(packet[ip_header + 12] >> 4) * 4;
        let payload_start = ip_header.checked_add(tcp_header)?;
        if tcp_header < 20 || payload_start > total {
            return None;
        }
        Some(Ipv4TcpView {
            source_address: Ipv4Addr::new(packet[12], packet[13], packet[14], packet[15]),
            destination_address: Ipv4Addr::new(packet[16], packet[17], packet[18], packet[19]),
            source_port: u16::from_be_bytes([packet[ip_header], packet[ip_header + 1]]),
            destination_port: u16::from_be_bytes([packet[ip_header + 2], packet[ip_header + 3]]),
            sequence: u32::from_be_bytes([
                packet[ip_header + 4],
                packet[ip_header + 5],
                packet[ip_header + 6],
                packet[ip_header + 7],
            ]),
            syn: packet[ip_header + 13] & 0x02 != 0,
            ack: packet[ip_header + 13] & 0x10 != 0,
            payload: &packet[payload_start..total],
        })
    }

    fn owned_connection_count(
        owner: &WindowsProcessSocketOwner,
        connection: AutomarkerOwnedTcpConnection,
    ) -> Result<usize, Box<dyn Error>> {
        let expected = TcpConnection::new(
            rlogs_capture::TcpEndpoint::new(
                IpAddr::V4(connection.local.address),
                connection.local.port,
            ),
            rlogs_capture::TcpEndpoint::new(
                IpAddr::V4(connection.remote.address),
                connection.remote.port,
            ),
        );
        Ok(owner
            .snapshot_all_connections()?
            .iter()
            .filter(|candidate| **candidate == expected)
            .count())
    }

    fn open_network_handle_after_reflect_arbitration(
        loaded: &Arc<LoadedApi>,
        filter: &str,
        target_priority: i16,
        flags: u64,
    ) -> Result<ArbitratedNetworkOpen, Box<dyn Error>> {
        let _serialization = ARBITRATION_LOCK
            .lock()
            .map_err(|_| "REFLECT arbitration lock was poisoned")?;
        if !reflect_inventory_is_clear(loaded, target_priority)? {
            return Ok(ArbitratedNetworkOpen::Conflict);
        }
        Ok(ArbitratedNetworkOpen::Open(open_network_handle(
            loaded,
            filter,
            target_priority,
            flags,
        )?))
    }

    fn reflect_inventory_is_clear(
        loaded: &Arc<LoadedApi>,
        target_priority: i16,
    ) -> Result<bool, Box<dyn Error>> {
        let flags = WINDIVERT_FLAG_SNIFF | WINDIVERT_FLAG_RECV_ONLY | WINDIVERT_FLAG_NO_INSTALL;
        let reflect = Arc::new(open_handle_at_layer(
            loaded,
            "true",
            WINDIVERT_LAYER_REFLECT,
            0,
            flags,
        )?);
        if get_param(&loaded.api, reflect.raw, WINDIVERT_PARAM_VERSION_MAJOR)? != 2
            || get_param(&loaded.api, reflect.raw, WINDIVERT_PARAM_VERSION_MINOR)? != 2
        {
            return Err("REFLECT arbitration requires loaded WinDivert driver 2.2".into());
        }

        let stop_handle = Arc::clone(&reflect);
        let shutdown = loaded.api.shutdown;
        let (timer_send, timer_receive) = std::sync::mpsc::channel();
        let timer = thread::spawn(move || {
            let _ = timer_receive.recv_timeout(REFLECT_BARRIER_TIMEOUT);
            let ok = unsafe { shutdown(stop_handle.raw, WINDIVERT_SHUTDOWN_RECV) };
            if ok == 0 {
                Err(unsafe { GetLastError() })
            } else {
                Ok(())
            }
        });

        // Opening this non-matching lowest-priority handle creates the ordered
        // REFLECT event used as an inventory barrier. It cannot divert traffic.
        let sentinel = open_network_handle(loaded, "false", REFLECT_SENTINEL_PRIORITY, flags);
        let arbitration_result = (|| -> Result<bool, Box<dyn Error>> {
            let _sentinel = sentinel?;
            let process_id = std::process::id();
            let mut arbitrator = ReflectArbitrator::new(
                process_id,
                REFLECT_SENTINEL_PRIORITY,
                flags,
                target_priority,
            );
            let mut backend = LiveBackend {
                handle: Arc::clone(&reflect),
                api: loaded.api,
            };
            loop {
                let event = backend
                    .receive()?
                    .ok_or("REFLECT inventory ended before its sentinel barrier")?;
                let decoded = decode_reflect_event(&event.address)
                    .map_err(|error| format!("invalid REFLECT address: {error}"))?;
                arbitrator
                    .observe(decoded)
                    .map_err(|error| format!("ambiguous REFLECT inventory: {error}"))?;
                if arbitrator.sentinel_seen {
                    return arbitrator
                        .affirmative()
                        .map_err(|error| format!("incomplete REFLECT inventory: {error}").into());
                }
            }
        })();
        let _ = timer_send.send(());
        timer
            .join()
            .map_err(|_| "REFLECT arbitration timer panicked")?
            .map_err(|error| {
                format!("WinDivertShutdown for REFLECT failed with Windows error {error}")
            })?;
        arbitration_result
    }

    fn open_network_handle(
        loaded: &Arc<LoadedApi>,
        filter: &str,
        priority: i16,
        flags: u64,
    ) -> Result<OwnedWinDivertHandle, Box<dyn Error>> {
        open_handle_at_layer(loaded, filter, WINDIVERT_LAYER_NETWORK, priority, flags)
    }

    fn open_handle_at_layer(
        loaded: &Arc<LoadedApi>,
        filter: &str,
        layer: i32,
        priority: i16,
        flags: u64,
    ) -> Result<OwnedWinDivertHandle, Box<dyn Error>> {
        let filter = CString::new(filter)?;
        let handle = unsafe { (loaded.api.open)(filter.as_ptr(), layer, priority, flags) };
        if handle == INVALID_HANDLE_VALUE || handle.is_null() {
            return Err(
                format!("WinDivertOpen failed with Windows error {}", unsafe {
                    GetLastError()
                })
                .into(),
            );
        }
        Ok(OwnedWinDivertHandle {
            raw: handle,
            loaded: Arc::clone(loaded),
        })
    }

    fn compile_network_filter(api: &WinDivertApi, filter: &str) -> Result<(), Box<dyn Error>> {
        let filter = CString::new(filter)?;
        let mut error = ptr::null();
        let mut position = 0u32;
        let ok = unsafe {
            (api.compile_filter)(
                filter.as_ptr(),
                WINDIVERT_LAYER_NETWORK,
                ptr::null_mut(),
                0,
                &mut error,
                &mut position,
            )
        };
        if ok == 0 {
            return Err(format!("active WinDivert filter failed compilation at {position}").into());
        }
        Ok(())
    }

    fn configure_and_verify(
        api: &WinDivertApi,
        handle: HANDLE,
        gates: &mut GateReceipt,
    ) -> Result<(), Box<dyn Error>> {
        for (parameter, expected) in [
            (WINDIVERT_PARAM_QUEUE_LENGTH, 4096),
            (WINDIVERT_PARAM_QUEUE_TIME, 2000),
            (WINDIVERT_PARAM_QUEUE_SIZE, 4_194_304),
        ] {
            if unsafe { (api.set_param)(handle, parameter, expected) } == 0 {
                return Err(
                    format!("WinDivertSetParam failed with Windows error {}", unsafe {
                        GetLastError()
                    })
                    .into(),
                );
            }
            if get_param(api, handle, parameter)? != expected {
                return Err("WinDivert queue parameter readback mismatch".into());
            }
        }
        gates.queue_policy_readback = true;
        gates.version_2_2 = get_param(api, handle, WINDIVERT_PARAM_VERSION_MAJOR)? == 2
            && get_param(api, handle, WINDIVERT_PARAM_VERSION_MINOR)? == 2;
        if !gates.version_2_2 {
            return Err("loaded driver does not report WinDivert 2.2".into());
        }
        Ok(())
    }

    fn get_param(
        api: &WinDivertApi,
        handle: HANDLE,
        parameter: i32,
    ) -> Result<u64, Box<dyn Error>> {
        let mut value = 0u64;
        if unsafe { (api.get_param)(handle, parameter, &mut value) } == 0 {
            return Err(
                format!("WinDivertGetParam failed with Windows error {}", unsafe {
                    GetLastError()
                })
                .into(),
            );
        }
        Ok(value)
    }

    unsafe fn load_pinned_api(path: &Path) -> Result<LoadedApi, Box<dyn Error>> {
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
        macro_rules! export {
            ($name:literal, $kind:ty) => {{
                let symbol = unsafe { GetProcAddress(module, concat!($name, "\0").as_ptr()) }
                    .ok_or(concat!("missing WinDivert export ", $name))?;
                unsafe { transmute::<unsafe extern "system" fn() -> isize, $kind>(symbol) }
            }};
        }
        Ok(LoadedApi {
            module,
            api: WinDivertApi {
                open: export!("WinDivertOpen", OpenFn),
                recv: export!("WinDivertRecv", RecvFn),
                send: export!("WinDivertSend", SendFn),
                shutdown: export!("WinDivertShutdown", ShutdownFn),
                close: export!("WinDivertClose", CloseFn),
                set_param: export!("WinDivertSetParam", SetParamFn),
                get_param: export!("WinDivertGetParam", GetParamFn),
                compile_filter: export!("WinDivertHelperCompileFilter", CompileFilterFn),
            },
        })
    }

    fn sha256_is(path: &Path, expected: &str) -> Result<bool, Box<dyn Error>> {
        let digest = Sha256::digest(fs::read(path)?);
        Ok(format!("{digest:x}").eq_ignore_ascii_case(expected))
    }

    fn verify_driver_signature(path: &Path) -> Result<bool, Box<dyn Error>> {
        let script = "$s=Get-AuthenticodeSignature -LiteralPath $env:RLOGS_WINDIVERT_DRIVER_VERIFY_PATH; if($s.Status -eq 'Valid' -and $s.SignerCertificate){[Console]::Write($s.SignerCertificate.Thumbprint)}";
        let output = Command::new("powershell.exe")
            .args(["-NoProfile", "-NonInteractive", "-Command", script])
            .env("RLOGS_WINDIVERT_DRIVER_VERIFY_PATH", path)
            .output()?;
        Ok(output.status.success()
            && String::from_utf8_lossy(&output.stdout)
                .trim()
                .eq_ignore_ascii_case(EXPECTED_DRIVER_SIGNER_THUMBPRINT))
    }

    fn base_filtering_engine_running() -> Result<bool, Box<dyn Error>> {
        let output = Command::new("sc.exe").args(["query", "BFE"]).output()?;
        Ok(output.status.success() && String::from_utf8_lossy(&output.stdout).contains("RUNNING"))
    }

    fn is_elevated() -> Result<bool, Box<dyn Error>> {
        let mut token = ptr::null_mut();
        if unsafe { OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token) } == 0 {
            return Err("OpenProcessToken failed".into());
        }
        let token = OwnedKernelHandle(token);
        let mut elevation = TOKEN_ELEVATION::default();
        let mut returned = 0u32;
        let ok = unsafe {
            GetTokenInformation(
                token.0,
                TokenElevation,
                (&mut elevation as *mut TOKEN_ELEVATION).cast(),
                size_of::<TOKEN_ELEVATION>() as u32,
                &mut returned,
            )
        };
        Ok(ok != 0
            && returned as usize == size_of::<TOKEN_ELEVATION>()
            && elevation.TokenIsElevated != 0)
    }

    fn exact_process_name(process_id: u32, expected: &str) -> Result<bool, Box<dyn Error>> {
        let snapshot = unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) };
        if snapshot == INVALID_HANDLE_VALUE {
            return Err("could not enumerate processes".into());
        }
        let snapshot = OwnedKernelHandle(snapshot);
        let mut entry = PROCESSENTRY32W {
            dwSize: size_of::<PROCESSENTRY32W>() as u32,
            ..Default::default()
        };
        let mut found = Vec::new();
        let mut present = unsafe { Process32FirstW(snapshot.0, &mut entry) } != 0;
        while present {
            let end = entry
                .szExeFile
                .iter()
                .position(|unit| *unit == 0)
                .unwrap_or(entry.szExeFile.len());
            let name = OsString::from_wide(&entry.szExeFile[..end])
                .to_string_lossy()
                .into_owned();
            if name.eq_ignore_ascii_case(expected) {
                found.push(entry.th32ProcessID);
            }
            present = unsafe { Process32NextW(snapshot.0, &mut entry) } != 0;
        }
        Ok(found.as_slice() == [process_id])
    }

    fn write_receipt(path: &Path, receipt: CanaryReceipt) -> Result<(), Box<dyn Error>> {
        let file = OpenOptions::new().write(true).create_new(true).open(path)?;
        let mut writer = BufWriter::new(file);
        serde_json::to_writer_pretty(&mut writer, &receipt)?;
        writer.write_all(b"\n")?;
        writer.flush()?;
        writer.get_ref().sync_all()?;
        println!("Sanitized create-only pass-through receipt written.");
        Ok(())
    }

    impl GateReceipt {
        fn new_armed() -> Self {
            Self {
                explicit_literal_consent: true,
                elevated: false,
                pinned_dll_hash: false,
                pinned_driver_hash: false,
                driver_signature_and_signer: false,
                bfe_running: false,
                exact_process_name: false,
                exact_ipv4_tuple_discovered: false,
                syn_observed: false,
                exact_process_owned_epoch: false,
                same_priority_reflect_arbitration: false,
                reflect_arbitration_checks: 0,
                exact_filter_compiled: false,
                exact_filter_opened: false,
                version_2_2: false,
                queue_policy_readback: false,
            }
        }
    }

    impl CanaryReceipt {
        fn blocked_armed(mode: &'static str, outcome: &str, gates: GateReceipt) -> Self {
            Self {
                schema_version: 1,
                artifact_kind: "sanitized-automarker-windivert-byte-identical-passthrough",
                game_build: AUTOMARKER_REQUEST_BUILD,
                mode,
                outcome: outcome.into(),
                gates,
                counts: CountsReceipt::from(RelayCounts::default()),
                invariants: InvariantReceipt::dry_run(),
            }
        }

        fn dry_run(outcome: &str) -> Self {
            Self {
                schema_version: 1,
                artifact_kind: "sanitized-automarker-windivert-byte-identical-passthrough",
                game_build: AUTOMARKER_REQUEST_BUILD,
                mode: "dry-run",
                outcome: outcome.into(),
                gates: GateReceipt {
                    explicit_literal_consent: false,
                    elevated: false,
                    pinned_dll_hash: false,
                    pinned_driver_hash: false,
                    driver_signature_and_signer: false,
                    bfe_running: false,
                    exact_process_name: false,
                    exact_ipv4_tuple_discovered: false,
                    syn_observed: false,
                    exact_process_owned_epoch: false,
                    same_priority_reflect_arbitration: false,
                    reflect_arbitration_checks: 0,
                    exact_filter_compiled: false,
                    exact_filter_opened: false,
                    version_2_2: false,
                    queue_policy_readback: false,
                },
                counts: CountsReceipt::from(RelayCounts::default()),
                invariants: InvariantReceipt::dry_run(),
            }
        }
    }

    impl From<RelayCounts> for CountsReceipt {
        fn from(value: RelayCounts) -> Self {
            Self {
                packets_received: value.packets_received,
                packets_reinjected: value.packets_reinjected,
                bytes_received: value.bytes_received,
                bytes_reinjected: value.bytes_reinjected,
            }
        }
    }

    impl InvariantReceipt {
        fn dry_run() -> Self {
            Self::common(true, false)
        }
        fn armed(conserved: bool) -> Self {
            Self::common(conserved, true)
        }
        fn common(conserved: bool, shutdown: bool) -> Self {
            Self {
                single_inflight_synchronous: true,
                complete_opaque_address_preserved: true,
                every_received_packet_reinjected: conserved,
                packet_lengths_preserved: conserved,
                packet_bytes_preserved: true,
                shutdown_receive_only_then_drained: shutdown,
                substitution_enabled: false,
                checksum_rewrite_enabled: false,
                endpoints_persisted: false,
                tcp_sequence_or_ack_persisted: false,
                payloads_persisted: false,
                protocol_or_account_ids_persisted: false,
            }
        }
    }

    impl Arguments {
        fn parse(raw: Vec<OsString>) -> Result<Self, Box<dyn Error>> {
            let mut values = raw.into_iter();
            let mut armed = false;
            let mut bootstrap = false;
            let mut dry_run = false;
            let mut process_id = None;
            let mut dependency_directory = None;
            let mut syn_wait_seconds = 120;
            let mut duration_seconds = 20;
            let mut output = None;
            while let Some(argument) = values.next() {
                let argument = argument.to_string_lossy();
                let value = |values: &mut std::vec::IntoIter<OsString>| -> Result<OsString, Box<dyn Error>> {
                    values.next().ok_or_else(|| format!("missing value after {argument}").into())
                };
                match argument.as_ref() {
                    "--arm-byte-identical-passthrough" => {
                        armed = value(&mut values)?.to_string_lossy() == ARM_TOKEN
                    }
                    "--arm-driver-bootstrap" => {
                        bootstrap = value(&mut values)?.to_string_lossy() == BOOTSTRAP_TOKEN
                    }
                    "--dry-run" => dry_run = true,
                    "--process-id" => {
                        process_id = Some(value(&mut values)?.to_string_lossy().parse()?)
                    }
                    "--dependency-directory" => {
                        dependency_directory = Some(PathBuf::from(value(&mut values)?))
                    }
                    "--syn-wait-seconds" => {
                        syn_wait_seconds = value(&mut values)?.to_string_lossy().parse()?
                    }
                    "--duration-seconds" => {
                        duration_seconds = value(&mut values)?.to_string_lossy().parse()?
                    }
                    "--output" => output = Some(PathBuf::from(value(&mut values)?)),
                    _ => return Err(format!("unsupported argument {argument}").into()),
                }
            }
            if armed == dry_run || (bootstrap && !armed) {
                return Err(
                    "choose --dry-run or literal pass-through; bootstrap also requires literal pass-through consent"
                        .into()
                );
            }
            if !(1..=300).contains(&syn_wait_seconds) || !(1..=60).contains(&duration_seconds) {
                return Err(
                    "SYN wait must be 1-300 seconds and active duration 1-60 seconds".into(),
                );
            }
            Ok(Self {
                armed,
                bootstrap,
                process_id,
                dependency_directory,
                syn_wait_seconds,
                duration_seconds,
                output: output.ok_or("--output is required")?,
            })
        }
    }

    #[cfg(test)]
    mod discovery_tests {
        use super::{DiscoveryPrefix, GateReceipt};

        #[test]
        fn prefix_assembly_handles_gap_overlap_and_exact_retransmission() {
            let mut prefix = DiscoveryPrefix::new(100);
            assert!(prefix.ingest(104, b"ef"));
            assert!(prefix.bytes().is_empty());
            assert!(prefix.ingest(100, b"abcd"));
            assert_eq!(prefix.bytes(), b"abcdef");
            assert!(prefix.ingest(102, b"cdefgh"));
            assert_eq!(prefix.bytes(), b"abcdefgh");
            assert!(prefix.ingest(100, b"abcdefgh"));
            assert_eq!(prefix.bytes(), b"abcdefgh");
        }

        #[test]
        fn prefix_assembly_rejects_conflicting_retransmission() {
            let mut prefix = DiscoveryPrefix::new(u32::MAX - 1);
            assert!(prefix.ingest(u32::MAX - 1, b"abcd"));
            assert!(!prefix.ingest(0, b"XX"));
            assert!(!prefix.ingest(2, b"ef"));
        }

        #[test]
        fn armed_mode_starts_with_reflect_gate_closed() {
            assert!(!GateReceipt::new_armed().same_priority_reflect_arbitration);
        }
    }
}

#[cfg(windows)]
fn main() {
    windows::main();
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::VecDeque;

    fn reflect_address(kind: ReflectEventKind, identity: ReflectedHandleIdentity) -> OpaqueAddress {
        let mut address = OpaqueAddress::default();
        let event = match kind {
            ReflectEventKind::Open => WINDIVERT_EVENT_REFLECT_OPEN_VALUE,
            ReflectEventKind::Close => WINDIVERT_EVENT_REFLECT_CLOSE_VALUE,
        };
        address.0[1] = u64::from(WINDIVERT_LAYER_REFLECT_VALUE) | (u64::from(event) << 8);
        address.0[2] = identity.opened_timestamp as u64;
        address.0[3] = u64::from(identity.process_id) | (u64::from(identity.layer) << 32);
        address.0[4] = identity.flags;
        address.0[5] = u64::from(identity.priority as u16);
        address
    }

    #[test]
    fn decodes_the_exact_public_reflect_address_fields() {
        let identity = ReflectedHandleIdentity {
            opened_timestamp: -91,
            process_id: 4242,
            layer: WINDIVERT_LAYER_NETWORK_VALUE,
            flags: 21,
            priority: -1000,
        };
        assert_eq!(
            decode_reflect_event(&reflect_address(ReflectEventKind::Open, identity)),
            Ok(ReflectedHandleEvent {
                kind: ReflectEventKind::Open,
                identity,
            })
        );
        assert!(decode_reflect_event(&OpaqueAddress::default()).is_err());
        let mut unknown_event = reflect_address(ReflectEventKind::Open, identity);
        unknown_event.0[1] = u64::from(WINDIVERT_LAYER_REFLECT_VALUE) | (99u64 << 8);
        assert!(decode_reflect_event(&unknown_event).is_err());
    }

    #[test]
    fn sentinel_barrier_affirms_only_without_same_priority_network_handle() {
        let sentinel = ReflectedHandleIdentity {
            opened_timestamp: 100,
            process_id: 42,
            layer: WINDIVERT_LAYER_NETWORK_VALUE,
            flags: 21,
            priority: -1000,
        };
        let mut clear = ReflectArbitrator::new(42, -1000, 21, 0);
        clear
            .observe(ReflectedHandleEvent {
                kind: ReflectEventKind::Open,
                identity: sentinel,
            })
            .unwrap();
        assert_eq!(clear.affirmative(), Ok(true));

        let mut blocked = ReflectArbitrator::new(42, -1000, 21, 0);
        blocked
            .observe(ReflectedHandleEvent {
                kind: ReflectEventKind::Open,
                identity: ReflectedHandleIdentity {
                    opened_timestamp: 99,
                    process_id: 7,
                    layer: WINDIVERT_LAYER_NETWORK_VALUE,
                    flags: 0,
                    priority: 0,
                },
            })
            .unwrap();
        blocked
            .observe(ReflectedHandleEvent {
                kind: ReflectEventKind::Open,
                identity: sentinel,
            })
            .unwrap();
        assert_eq!(blocked.affirmative(), Ok(false));

        let mut discovery_blocked = ReflectArbitrator::new(42, -1000, 21, 1);
        discovery_blocked
            .observe(ReflectedHandleEvent {
                kind: ReflectEventKind::Open,
                identity: ReflectedHandleIdentity {
                    opened_timestamp: 101,
                    process_id: 9,
                    layer: WINDIVERT_LAYER_NETWORK_VALUE,
                    flags: 5,
                    priority: 1,
                },
            })
            .unwrap();
        discovery_blocked
            .observe(ReflectedHandleEvent {
                kind: ReflectEventKind::Open,
                identity: sentinel,
            })
            .unwrap();
        assert_eq!(discovery_blocked.affirmative(), Ok(false));
    }

    #[test]
    fn close_removes_only_its_exact_open_identity() {
        let other = ReflectedHandleIdentity {
            opened_timestamp: 77,
            process_id: 8,
            layer: WINDIVERT_LAYER_NETWORK_VALUE,
            flags: 0,
            priority: 0,
        };
        let sentinel = ReflectedHandleIdentity {
            opened_timestamp: 78,
            process_id: 42,
            layer: WINDIVERT_LAYER_NETWORK_VALUE,
            flags: 21,
            priority: -1000,
        };
        let mut arbitrator = ReflectArbitrator::new(42, -1000, 21, 0);
        for event in [
            ReflectedHandleEvent {
                kind: ReflectEventKind::Open,
                identity: other,
            },
            ReflectedHandleEvent {
                kind: ReflectEventKind::Close,
                identity: other,
            },
            ReflectedHandleEvent {
                kind: ReflectEventKind::Open,
                identity: sentinel,
            },
        ] {
            arbitrator.observe(event).unwrap();
        }
        assert_eq!(arbitrator.affirmative(), Ok(true));
    }

    #[test]
    fn missing_sentinel_and_ambiguous_lifecycle_fail_closed() {
        let identity = ReflectedHandleIdentity {
            opened_timestamp: 5,
            process_id: 8,
            layer: 2,
            flags: 5,
            priority: 1,
        };
        let mut arbitrator = ReflectArbitrator::new(42, -1000, 21, 0);
        assert!(arbitrator.affirmative().is_err());
        assert!(
            arbitrator
                .observe(ReflectedHandleEvent {
                    kind: ReflectEventKind::Close,
                    identity,
                })
                .is_err()
        );
    }

    #[derive(Default)]
    struct MockBackend {
        received: VecDeque<Result<Option<ReceivedPacket>, String>>,
        sent: Vec<(Vec<u8>, OpaqueAddress)>,
        forced_sent_length: Option<usize>,
        send_error: Option<String>,
    }

    impl PassthroughBackend for MockBackend {
        fn receive(&mut self) -> Result<Option<ReceivedPacket>, String> {
            self.received.pop_front().unwrap_or(Ok(None))
        }
        fn send_unchanged(
            &mut self,
            bytes: &[u8],
            address: &OpaqueAddress,
        ) -> Result<usize, String> {
            if let Some(error) = self.send_error.take() {
                return Err(error);
            }
            self.sent.push((bytes.to_vec(), address.clone()));
            Ok(self.forced_sent_length.unwrap_or(bytes.len()))
        }
    }

    fn packet(bytes: &[u8], marker: u64) -> ReceivedPacket {
        let mut address = OpaqueAddress::default();
        address.0[3] = marker;
        ReceivedPacket {
            bytes: bytes.to_vec(),
            address,
        }
    }

    #[test]
    fn relays_every_packet_and_address_byte_identically_until_drained() {
        let expected_address = packet(&[1, 2, 3, 4], 91).address;
        let mut backend = MockBackend {
            received: VecDeque::from([
                Ok(Some(packet(&[1, 2, 3, 4], 91))),
                Ok(Some(packet(&[5, 6], 92))),
                Ok(None),
            ]),
            ..Default::default()
        };
        let counts = drain_byte_identically(&mut backend).unwrap();
        assert_eq!(
            counts,
            RelayCounts {
                packets_received: 2,
                packets_reinjected: 2,
                bytes_received: 6,
                bytes_reinjected: 6
            }
        );
        assert_eq!(backend.sent[0], (vec![1, 2, 3, 4], expected_address));
        assert_eq!(backend.sent[1], (vec![5, 6], packet(&[], 92).address));
    }

    #[test]
    fn refuses_a_short_send_without_claiming_conservation() {
        let mut backend = MockBackend {
            received: VecDeque::from([Ok(Some(packet(&[7, 8, 9], 1)))]),
            forced_sent_length: Some(2),
            ..Default::default()
        };
        assert_eq!(
            drain_byte_identically(&mut backend),
            Err(RelayError::LengthMismatch {
                received: 3,
                sent: 2
            })
        );
    }

    #[test]
    fn stops_on_send_error_and_never_consumes_a_second_packet() {
        let mut backend = MockBackend {
            received: VecDeque::from([Ok(Some(packet(&[1], 1))), Ok(Some(packet(&[2], 2)))]),
            send_error: Some("fault injection".into()),
            ..Default::default()
        };
        assert_eq!(
            drain_byte_identically(&mut backend),
            Err(RelayError::Send("fault injection".into()))
        );
        assert_eq!(backend.received.len(), 1);
        assert!(backend.sent.is_empty());
    }

    #[test]
    fn propagates_receive_failure_without_sending() {
        let mut backend = MockBackend {
            received: VecDeque::from([Err("receive fault".into())]),
            ..Default::default()
        };
        assert_eq!(
            drain_byte_identically(&mut backend),
            Err(RelayError::Receive("receive fault".into()))
        );
        assert!(backend.sent.is_empty());
    }
}
