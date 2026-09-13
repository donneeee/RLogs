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

#[derive(Debug, Default, Clone, PartialEq, Eq)]
struct OpaqueAddress([u64; 10]);

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
    const WINDIVERT_FLAG_SNIFF: u64 = 1;
    const WINDIVERT_FLAG_RECV_ONLY: u64 = 4;
    const WINDIVERT_FLAG_NO_INSTALL: u64 = 16;
    const WINDIVERT_SHUTDOWN_RECV: i32 = 0x1;
    const WINDIVERT_PARAM_QUEUE_LENGTH: i32 = 0;
    const WINDIVERT_PARAM_QUEUE_TIME: i32 = 1;
    const WINDIVERT_PARAM_QUEUE_SIZE: i32 = 2;
    const WINDIVERT_PARAM_VERSION_MAJOR: i32 = 3;
    const WINDIVERT_PARAM_VERSION_MINOR: i32 = 4;

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
    type SetParamFn = unsafe extern "system" fn(HANDLE, i32, u64) -> i32;
    type GetParamFn = unsafe extern "system" fn(HANDLE, i32, *mut u64) -> i32;

    #[derive(Clone, Copy)]
    struct WinDivertApi {
        open: OpenFn,
        recv: RecvFn,
        send: SendFn,
        shutdown: ShutdownFn,
        set_param: SetParamFn,
        get_param: GetParamFn,
    }

    struct LoadedApi {
        module: windows_sys::Win32::Foundation::HMODULE,
        api: WinDivertApi,
    }

    impl Drop for LoadedApi {
        fn drop(&mut self) {
            unsafe { FreeLibrary(self.module) };
        }
    }

    struct OwnedHandle(HANDLE);

    unsafe impl Send for OwnedHandle {}
    unsafe impl Sync for OwnedHandle {}

    impl Drop for OwnedHandle {
        fn drop(&mut self) {
            if !self.0.is_null() && self.0 != INVALID_HANDLE_VALUE {
                unsafe { CloseHandle(self.0) };
            }
        }
    }

    struct LiveBackend {
        handle: Arc<OwnedHandle>,
        api: WinDivertApi,
    }

    impl PassthroughBackend for LiveBackend {
        fn receive(&mut self) -> Result<Option<ReceivedPacket>, String> {
            let mut bytes = vec![0u8; MAX_PACKET_BYTES];
            let mut address = OpaqueAddress::default();
            let mut length = 0u32;
            let ok = unsafe {
                (self.api.recv)(
                    self.handle.0,
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
                    self.handle.0,
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
        let loaded = unsafe { load_pinned_api(&dll)? };
        // In the separately armed bootstrap-and-pass-through mode, keep this
        // non-matching handle alive for the entire canary. This is the sole
        // call site allowed to omit NO_INSTALL. It can install/start the pinned
        // driver, but its false filter cannot capture, block, or send traffic.
        let bootstrap_handle = if args.bootstrap {
            let handle = open_handle(
                &loaded.api,
                "false",
                0,
                WINDIVERT_FLAG_SNIFF | WINDIVERT_FLAG_RECV_ONLY,
            )?;
            configure_and_verify(&loaded.api, handle.0, &mut gates)?;
            Some(handle)
        } else {
            None
        };
        let owner = WindowsProcessSocketOwner::new(process_id)?;
        let connection =
            discover_exact_bpsr_syn_epoch(&loaded.api, &owner, process_id, args.syn_wait_seconds)?;
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

        let active = open_handle(&loaded.api, &filter, 0, WINDIVERT_FLAG_NO_INSTALL)?;
        gates.exact_filter_opened = true;
        configure_and_verify(&loaded.api, active.0, &mut gates)?;

        let active = Arc::new(active);
        let stop_handle = Arc::clone(&active);
        let shutdown = loaded.api.shutdown;
        let duration = args.duration_seconds;
        let timer = thread::spawn(move || {
            thread::sleep(Duration::from_secs(duration));
            let ok = unsafe { shutdown(stop_handle.0, WINDIVERT_SHUTDOWN_RECV) };
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
        let relay = drain_byte_identically(&mut backend)?;
        timer
            .join()
            .map_err(|_| "shutdown timer panicked")?
            .map_err(|error| format!("WinDivertShutdown failed with Windows error {error}"))?;

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
        api: &WinDivertApi,
        owner: &WindowsProcessSocketOwner,
        process_id: u32,
        wait_seconds: u64,
    ) -> Result<AutomarkerOwnedTcpConnection, Box<dyn Error>> {
        let handle = Arc::new(open_handle(
            api,
            "outbound and ip and tcp",
            1,
            WINDIVERT_FLAG_SNIFF | WINDIVERT_FLAG_RECV_ONLY | WINDIVERT_FLAG_NO_INSTALL,
        )?);
        let stop_handle = Arc::clone(&handle);
        let shutdown = api.shutdown;
        let (cancel_send, cancel_receive) = std::sync::mpsc::channel();
        let timer = thread::spawn(move || {
            if cancel_receive
                .recv_timeout(Duration::from_secs(wait_seconds))
                .is_err()
            {
                unsafe { shutdown(stop_handle.0, WINDIVERT_SHUTDOWN_RECV) };
            }
        });
        println!(
            "Waiting up to {wait_seconds} seconds for a SYN-scoped, process-owned BPSR connection..."
        );
        let mut backend = LiveBackend { handle, api: *api };
        let mut syn_owned = HashSet::new();
        let mut prefixes = HashMap::<AutomarkerOwnedTcpConnection, Vec<u8>>::new();
        let mut confirmed = HashSet::new();
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
            if view.syn && !view.ack && owned_connection_count(owner, connection)? == 1 {
                syn_owned.insert(connection);
            }
            if syn_owned.contains(&connection) && !view.payload.is_empty() {
                let prefix = prefixes.entry(connection).or_default();
                if prefix.len().saturating_add(view.payload.len()) <= 65_536 {
                    prefix.extend_from_slice(view.payload);
                    let regular = classify_bpsr_tcp_prefix(prefix);
                    let marker = classify_observed_automarker_tcp_prefix(prefix);
                    if matches!(regular, rlogs_capture::TcpPayloadSignatureResult::Match(_))
                        || matches!(marker, rlogs_capture::TcpPayloadSignatureResult::Match(_))
                    {
                        confirmed.insert(connection);
                        if confirmed.len() == 1 {
                            break;
                        }
                    } else if matches!(regular, rlogs_capture::TcpPayloadSignatureResult::Reject)
                        && matches!(marker, rlogs_capture::TcpPayloadSignatureResult::Reject)
                    {
                        prefixes.remove(&connection);
                    }
                }
            }
        }
        let _ = cancel_send.send(());
        timer.join().map_err(|_| "SYN timer panicked")?;
        match confirmed.into_iter().collect::<Vec<_>>().as_slice() {
            [connection] => Ok(*connection),
            [] => Err("no SYN-scoped, exact-process-owned BPSR connection was confirmed; start the canary before reconnecting the game".into()),
            _ => Err("more than one SYN-scoped BPSR connection was confirmed for the game process".into()),
        }
    }

    #[derive(Clone, Copy)]
    struct Ipv4TcpView<'a> {
        source_address: Ipv4Addr,
        destination_address: Ipv4Addr,
        source_port: u16,
        destination_port: u16,
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
        if ip_header < 20 || total > packet.len() || total < ip_header + 20 {
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

    fn open_handle(
        api: &WinDivertApi,
        filter: &str,
        priority: i16,
        flags: u64,
    ) -> Result<OwnedHandle, Box<dyn Error>> {
        let filter = CString::new(filter)?;
        let handle =
            unsafe { (api.open)(filter.as_ptr(), WINDIVERT_LAYER_NETWORK, priority, flags) };
        if handle == INVALID_HANDLE_VALUE || handle.is_null() {
            return Err(
                format!("WinDivertOpen failed with Windows error {}", unsafe {
                    GetLastError()
                })
                .into(),
            );
        }
        Ok(OwnedHandle(handle))
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
                set_param: export!("WinDivertSetParam", SetParamFn),
                get_param: export!("WinDivertGetParam", GetParamFn),
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
        let token = OwnedHandle(token);
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
        let snapshot = OwnedHandle(snapshot);
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
                exact_filter_opened: false,
                version_2_2: false,
                queue_policy_readback: false,
            }
        }
    }

    impl CanaryReceipt {
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
}

#[cfg(windows)]
fn main() {
    windows::main();
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::VecDeque;

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
