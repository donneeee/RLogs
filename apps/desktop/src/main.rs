use std::fs::{File, OpenOptions};
use std::io::{BufReader, BufWriter, Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::path::{Component, Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use rlogs_capture::OfflineCapture;
#[cfg(windows)]
use rlogs_capture::{
    DumpcapLiveConfig, LiveCaptureStopHandle, OwnedProcessCaptureConfig,
    WindowsOwnedDumpcapCapture, record_owned_capture_to_files,
};
use rlogs_core::ResearchConnectionFile;
use rlogs_events::{RegionEvidence, RegionEvidenceKind, RegionIdentity};
use rlogs_game_bpsr::{
    GameBuild, NetworkEndpoint, OfflineRecordingConfig, OfflineRecordingLimits,
    OfflineRecordingReport, ProtocolPack, ProtocolRuntimeConfig, RegionResolverError,
    ResolvedRegion, ServerRealmCatalog, record_offline_capture,
};
use rlogs_log_format::RlogLimits;
use rlogs_plugin_combat_meter::CombatTimelinePlugin;
use rlogs_plugin_runtime::{PluginRunLimits, PluginRunReport, replay_rlog};
use serde::{Deserialize, Serialize};
#[cfg(windows)]
use windows_sys::Win32::{
    Foundation::{CloseHandle, INVALID_HANDLE_VALUE},
    System::Diagnostics::ToolHelp::{
        CreateToolhelp32Snapshot, PROCESSENTRY32W, Process32FirstW, Process32NextW,
        TH32CS_SNAPPROCESS,
    },
};

const DEFAULT_BIND: &str = "127.0.0.1:7419";
const MAX_REQUEST_HEADER_BYTES: usize = 64 * 1024;
const MAX_REQUEST_BODY_BYTES: usize = 1024 * 1024;

fn main() {
    if let Err(error) = run() {
        eprintln!("rLogs local host failed: {error}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), Box<dyn std::error::Error>> {
    let options = Options::parse(std::env::args().skip(1))?;
    let install_root = std::fs::canonicalize(&options.install_root)?;
    let ui_root =
        std::fs::canonicalize(install_root.join("apps/desktop/ui/dist")).map_err(|error| {
            format!(
                "desktop UI build was not found; run `npm run build` in apps/desktop/ui: {error}"
            )
        })?;
    let bind: SocketAddr = options.bind.parse()?;
    if !bind.ip().is_loopback() {
        return Err("the local control host may bind only to a loopback address".into());
    }
    let listener = TcpListener::bind(bind)?;
    let controller = Arc::new(RuntimeController::new(install_root));

    println!("rLogs local controls: http://{bind}");
    println!("Press Ctrl+C to stop the local host.");
    for stream in listener.incoming() {
        match stream {
            Ok(stream) => {
                if let Err(error) = handle_connection(stream, &ui_root, &controller) {
                    eprintln!("local HTTP request failed: {error}");
                }
            }
            Err(error) => eprintln!("local HTTP accept failed: {error}"),
        }
    }
    Ok(())
}

#[derive(Debug)]
struct Options {
    install_root: PathBuf,
    bind: String,
}

impl Options {
    fn parse(arguments: impl IntoIterator<Item = String>) -> Result<Self, String> {
        let mut install_root = None;
        let mut bind = None;
        let mut arguments = arguments.into_iter();
        while let Some(argument) = arguments.next() {
            match argument.as_str() {
                "--install-root" => {
                    if install_root.is_some() {
                        return Err("--install-root may be supplied only once".into());
                    }
                    install_root = Some(PathBuf::from(
                        arguments.next().ok_or("--install-root requires a value")?,
                    ));
                }
                "--bind" => {
                    if bind.is_some() {
                        return Err("--bind may be supplied only once".into());
                    }
                    bind = Some(arguments.next().ok_or("--bind requires a value")?);
                }
                _ => return Err(Self::usage()),
            }
        }
        Ok(Self {
            install_root: install_root.unwrap_or_else(|| PathBuf::from(".")),
            bind: bind.unwrap_or_else(|| DEFAULT_BIND.into()),
        })
    }

    fn usage() -> String {
        "usage: rlogs-desktop-host [--install-root <rlogs-root>] [--bind <127.0.0.1:port>]".into()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
enum RuntimePhase {
    Idle,
    Processing,
    Complete,
    Failed,
}

#[derive(Debug, Clone, Serialize)]
struct RuntimeSnapshot {
    schema_version: u16,
    phase: RuntimePhase,
    active_session_id: Option<String>,
    detail: String,
    started_unix_millis: Option<u64>,
    completed_unix_millis: Option<u64>,
    live_capture_can_stop: bool,
    last_result: Option<SessionResult>,
}

impl Default for RuntimeSnapshot {
    fn default() -> Self {
        Self {
            schema_version: 1,
            phase: RuntimePhase::Idle,
            active_session_id: None,
            detail: "Ready for a safe replay or offline capture.".into(),
            started_unix_millis: None,
            completed_unix_millis: None,
            live_capture_can_stop: false,
            last_result: None,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
struct SessionResult {
    session_id: String,
    source_kind: String,
    output_rlog: String,
    coverage_report: Option<String>,
    frame_count: Option<u64>,
    framed_record_count: Option<u64>,
    canonical_event_count: u64,
    known_route_count: Option<u64>,
    unknown_route_count: Option<u64>,
    data_gap_count: Option<u64>,
    private_capture: Option<String>,
    connection_evidence: Option<String>,
    combat_plugin: PluginRunReport,
}

#[derive(Debug, Clone, Deserialize)]
struct OfflineSessionRequest {
    session_id: String,
    capture_path: String,
    connections_path: String,
    #[serde(default)]
    pack_path: Option<String>,
    #[serde(default)]
    output_directory: Option<String>,
    #[serde(default)]
    region_id: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
struct LiveSessionRequest {
    session_id: String,
    process_id: u32,
    interface: String,
    dumpcap_path: String,
    duration_seconds: u32,
    #[serde(default)]
    private_output_directory: Option<String>,
    #[serde(default)]
    log_output_directory: Option<String>,
    #[serde(default)]
    pack_path: Option<String>,
    #[serde(default)]
    region_id: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
struct RuntimeEnvironment {
    platform: &'static str,
    game_processes: Vec<GameProcessView>,
    dumpcap_path: Option<String>,
    capture_interfaces: Vec<CaptureInterfaceView>,
}

#[derive(Debug, Clone, Serialize)]
struct GameProcessView {
    process_id: u32,
    executable_name: String,
}

#[derive(Debug, Clone, Serialize)]
struct CaptureInterfaceView {
    value: String,
    label: String,
}

#[derive(Debug)]
struct RuntimeController {
    install_root: PathBuf,
    state: Arc<Mutex<RuntimeSnapshot>>,
    #[cfg(windows)]
    live_stop: Arc<Mutex<Option<LiveCaptureStopHandle>>>,
}

impl RuntimeController {
    fn new(install_root: PathBuf) -> Self {
        Self {
            install_root,
            state: Arc::new(Mutex::new(RuntimeSnapshot::default())),
            #[cfg(windows)]
            live_stop: Arc::new(Mutex::new(None)),
        }
    }

    fn snapshot(&self) -> RuntimeSnapshot {
        self.state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone()
    }

    fn run_reference_replay(&self) -> Result<SessionResult, String> {
        {
            let state = self
                .state
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            if state.phase == RuntimePhase::Processing {
                return Err("another session is already processing".into());
            }
        }
        let input = self
            .install_root
            .join("tests/fixtures/replay/reference-combat.rlog");
        let result = replay_combat_log(&input).map_err(|error| error.to_string())?;
        let session = SessionResult {
            session_id: "fixture-reference-combat".into(),
            source_kind: "sanitized_reference_rlog".into(),
            output_rlog: display_path(&input),
            coverage_report: None,
            frame_count: None,
            framed_record_count: None,
            canonical_event_count: result.rlog.event_count,
            known_route_count: None,
            unknown_route_count: None,
            data_gap_count: None,
            private_capture: None,
            connection_evidence: None,
            combat_plugin: result,
        };
        let mut state = self
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        state.phase = RuntimePhase::Complete;
        state.active_session_id = None;
        state.detail = "Sanitized canonical replay reached the combat plug-in.".into();
        state.started_unix_millis = None;
        state.completed_unix_millis = Some(unix_millis());
        state.last_result = Some(session.clone());
        Ok(session)
    }

    fn start_offline(&self, request: OfflineSessionRequest) -> Result<(), String> {
        validate_identifier("session_id", &request.session_id)?;
        if let Some(region_id) = &request.region_id {
            validate_identifier("region_id", region_id)?;
        }
        {
            let mut state = self
                .state
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            if state.phase == RuntimePhase::Processing {
                return Err("another session is already processing".into());
            }
            state.phase = RuntimePhase::Processing;
            state.active_session_id = Some(request.session_id.clone());
            state.detail =
                "Reconstructing TCP, decoding BPSR, and writing a sealed canonical log.".into();
            state.started_unix_millis = Some(unix_millis());
            state.completed_unix_millis = None;
        }

        let install_root = self.install_root.clone();
        let state = Arc::clone(&self.state);
        thread::Builder::new()
            .name(format!("rlogs-offline-{}", request.session_id))
            .spawn(move || {
                let result = process_offline_session(&install_root, &request);
                let mut state = state
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner);
                state.active_session_id = None;
                state.completed_unix_millis = Some(unix_millis());
                match result {
                    Ok(result) => {
                        state.phase = RuntimePhase::Complete;
                        state.detail = format!(
                            "Sealed {} canonical events and delivered them to the combat plug-in.",
                            result.canonical_event_count
                        );
                        state.last_result = Some(result);
                    }
                    Err(error) => {
                        state.phase = RuntimePhase::Failed;
                        state.detail = error;
                    }
                }
            })
            .map_err(|error| format!("could not start session worker: {error}"))?;
        Ok(())
    }

    #[cfg(windows)]
    fn start_live(&self, request: LiveSessionRequest) -> Result<(), String> {
        validate_identifier("session_id", &request.session_id)?;
        if let Some(region_id) = &request.region_id {
            validate_identifier("region_id", region_id)?;
        }
        if request.process_id == 0 {
            return Err("process_id must be greater than zero".into());
        }
        let interface = request.interface.trim();
        if interface.is_empty() {
            return Err("capture interface cannot be empty".into());
        }
        {
            let state = self
                .state
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            if state.phase == RuntimePhase::Processing {
                return Err("another session is already processing".into());
            }
        }

        let capture = WindowsOwnedDumpcapCapture::spawn(
            request.process_id,
            DumpcapLiveConfig::new(&request.dumpcap_path, interface, request.duration_seconds)
                .map_err(|error| error.to_string())?,
            OwnedProcessCaptureConfig::default(),
        )
        .map_err(|error| error.to_string())?;
        let stop_handle = capture.stop_handle();
        {
            let mut live_stop = self
                .live_stop
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            *live_stop = Some(stop_handle);
        }
        {
            let mut state = self
                .state
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            state.phase = RuntimePhase::Processing;
            state.active_session_id = Some(request.session_id.clone());
            state.detail = "Capturing exact TCP flows owned by the selected BPSR process.".into();
            state.started_unix_millis = Some(unix_millis());
            state.completed_unix_millis = None;
            state.live_capture_can_stop = true;
        }

        let install_root = self.install_root.clone();
        let state = Arc::clone(&self.state);
        let live_stop = Arc::clone(&self.live_stop);
        let session_id = request.session_id.clone();
        let worker = thread::Builder::new()
            .name(format!("rlogs-live-{session_id}"))
            .spawn(move || {
                let private_directory = request
                    .private_output_directory
                    .as_ref()
                    .filter(|path| !path.trim().is_empty())
                    .map(PathBuf::from)
                    .unwrap_or_else(|| install_root.join("private-research/live-captures"));
                let capture_result =
                    record_owned_capture_to_files(capture, &private_directory, &session_id)
                        .map_err(|error| format!("live capture failed: {error}"));
                {
                    let mut stop = live_stop
                        .lock()
                        .unwrap_or_else(std::sync::PoisonError::into_inner);
                    *stop = None;
                }
                let result = capture_result.and_then(|recording| {
                    {
                        let mut state = state
                            .lock()
                            .unwrap_or_else(std::sync::PoisonError::into_inner);
                        state.detail = format!(
                            "Captured {} owned frames; decoding and sealing the canonical log.",
                            recording.metrics.emitted_frames
                        );
                        state.live_capture_can_stop = false;
                    }
                    let mut result = process_offline_session(
                        &install_root,
                        &OfflineSessionRequest {
                            session_id: session_id.clone(),
                            capture_path: recording.capture_path.display().to_string(),
                            connections_path: recording.connections_path.display().to_string(),
                            pack_path: request.pack_path.clone(),
                            output_directory: request.log_output_directory.clone(),
                            region_id: request.region_id.clone(),
                        },
                    )?;
                    result.source_kind = "live_process_owned_capture".into();
                    result.private_capture = Some(display_path(&recording.capture_path));
                    result.connection_evidence = Some(display_path(&recording.connections_path));
                    Ok(result)
                });

                let mut state = state
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner);
                state.active_session_id = None;
                state.completed_unix_millis = Some(unix_millis());
                state.live_capture_can_stop = false;
                match result {
                    Ok(result) => {
                        state.phase = RuntimePhase::Complete;
                        state.detail = format!(
                            "Captured, sealed, and delivered {} canonical events.",
                            result.canonical_event_count
                        );
                        state.last_result = Some(result);
                    }
                    Err(error) => {
                        state.phase = RuntimePhase::Failed;
                        state.detail = error;
                    }
                }
            });
        if let Err(error) = worker {
            let mut stop = self
                .live_stop
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            *stop = None;
            let mut state = self
                .state
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            state.phase = RuntimePhase::Failed;
            state.active_session_id = None;
            state.live_capture_can_stop = false;
            state.detail = format!("could not start live session worker: {error}");
            return Err(state.detail.clone());
        }
        Ok(())
    }

    #[cfg(not(windows))]
    fn start_live(&self, _: LiveSessionRequest) -> Result<(), String> {
        Err("live process-owned capture is not connected on this platform yet".into())
    }

    #[cfg(windows)]
    fn stop_live(&self) -> Result<(), String> {
        let stop = self
            .live_stop
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone()
            .ok_or("no stoppable live capture is active")?;
        {
            let mut state = self
                .state
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            state.detail = "Stop requested; draining owned frames and finalizing artifacts.".into();
            state.live_capture_can_stop = false;
        }
        stop.request_stop().map_err(|error| error.to_string())
    }

    #[cfg(not(windows))]
    fn stop_live(&self) -> Result<(), String> {
        Err("live process-owned capture is not connected on this platform yet".into())
    }
}

fn process_offline_session(
    install_root: &Path,
    request: &OfflineSessionRequest,
) -> Result<SessionResult, String> {
    let capture_path = existing_file(&request.capture_path, "capture")?;
    let connections_path = existing_file(&request.connections_path, "connection evidence")?;
    let pack_path = match &request.pack_path {
        Some(path) if !path.trim().is_empty() => existing_file(path, "protocol pack")?,
        _ => std::fs::canonicalize(install_root.join(
            "plugins/games/blue-protocol-star-resonance/protocol-packs/global/steam-24252055/pack.json",
        ))
        .map_err(|error| format!("default BPSR protocol pack is unavailable: {error}"))?,
    };
    let output_directory = match &request.output_directory {
        Some(path) if !path.trim().is_empty() => PathBuf::from(path),
        _ => install_root.join("runtime-data/logs"),
    };
    std::fs::create_dir_all(&output_directory)
        .map_err(|error| format!("could not create log output directory: {error}"))?;
    let output_directory = std::fs::canonicalize(&output_directory)
        .map_err(|error| format!("could not resolve log output directory: {error}"))?;
    let output_path = output_directory.join(format!("{}.rlog", request.session_id));
    let coverage_path = output_directory.join(format!("{}.coverage.json", request.session_id));
    let output_partial = partial_path(&output_path)?;
    let coverage_partial = partial_path(&coverage_path)?;
    ensure_outputs_available(&[
        &output_path,
        &coverage_path,
        &output_partial,
        &coverage_partial,
    ])?;

    let pack = ProtocolPack::from_json(
        &std::fs::read(&pack_path)
            .map_err(|error| format!("could not read protocol pack: {error}"))?,
    )
    .map_err(|error| format!("protocol pack is invalid: {error}"))?;
    let connection_file: ResearchConnectionFile = serde_json::from_slice(
        &std::fs::read(&connections_path)
            .map_err(|error| format!("could not read connection evidence: {error}"))?,
    )
    .map_err(|error| format!("connection evidence is invalid: {error}"))?;
    let resolved = resolve_server_realm(&pack_path, &connection_file)?;
    let target = &pack.definition().target;
    let mut region = resolved
        .as_ref()
        .map(|resolved| resolved.identity.clone())
        .unwrap_or_else(|| RegionIdentity {
            deployment_id: target.deployment_id.clone(),
            region_id: target
                .region_id
                .clone()
                .unwrap_or_else(|| target.deployment_id.clone()),
            realm_id: None,
            world_id: None,
        });
    if region.deployment_id != target.deployment_id {
        return Err("server catalog resolved another deployment".into());
    }
    if let Some(region_id) = &request.region_id {
        if region.region_id != "unknown" && &region.region_id != region_id {
            return Err(format!(
                "explicit region {region_id} conflicts with resolved region {}",
                region.region_id
            ));
        }
        region.region_id.clone_from(region_id);
    }
    let mut evidence = vec![RegionEvidence {
        kind: RegionEvidenceKind::ReplayManifest,
        reference: format!("offline-region:{}", region.region_id),
    }];
    if let Some(resolved) = resolved {
        evidence.extend(resolved.evidence);
    }
    let connections = connection_file
        .validate()
        .map_err(|error| format!("connection evidence failed validation: {error}"))?;
    let build = GameBuild {
        deployment_id: target.deployment_id.clone(),
        region_id: Some(region.region_id.clone()),
        channel: target.channel.clone(),
        build_id: target.build_id.clone(),
        executable_version: target.executable_version.clone(),
    };
    if !pack.matches(&build) {
        return Err("protocol pack does not match its resolved deployment/build".into());
    }

    let recording = (|| {
        let source = OfflineCapture::open(&capture_path)
            .map_err(|error| format!("capture could not be opened: {error}"))?;
        let file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&output_partial)
            .map_err(|error| format!("could not create partial rlog: {error}"))?;
        let result = record_offline_capture(
            source,
            connections,
            &pack,
            OfflineRecordingConfig {
                session_id: request.session_id.clone(),
                producer: format!("rlogs-desktop-host/{}", env!("CARGO_PKG_VERSION")),
                build,
                region,
                region_evidence: evidence,
                limits: OfflineRecordingLimits::default(),
                decoder: ProtocolRuntimeConfig::default(),
            },
            BufWriter::new(file),
        )
        .map_err(|error| format!("BPSR recording failed: {error}"))?;
        finalize_recording_files(
            result.output,
            &result.report,
            &output_partial,
            &coverage_partial,
            &output_path,
            &coverage_path,
        )?;
        Ok::<OfflineRecordingReport, String>(result.report)
    })();

    let report = match recording {
        Ok(report) => report,
        Err(error) => {
            remove_owned_partial(&output_partial);
            remove_owned_partial(&coverage_partial);
            return Err(error);
        }
    };
    let combat_plugin = replay_combat_log(&output_path)
        .map_err(|error| format!("combat plug-in failed: {error}"))?;
    Ok(SessionResult {
        session_id: request.session_id.clone(),
        source_kind: "offline_pcap".into(),
        output_rlog: display_path(&output_path),
        coverage_report: Some(display_path(&coverage_path)),
        frame_count: Some(report.frame_count),
        framed_record_count: Some(report.record_count),
        canonical_event_count: report.rlog.event_count,
        known_route_count: Some(report.capture.known_route_count),
        unknown_route_count: Some(report.capture.unknown_route_count),
        data_gap_count: Some(report.capture.gap_count),
        private_capture: Some(display_path(&capture_path)),
        connection_evidence: Some(display_path(&connections_path)),
        combat_plugin,
    })
}

fn finalize_recording_files(
    mut rlog: BufWriter<File>,
    report: &OfflineRecordingReport,
    output_partial: &Path,
    coverage_partial: &Path,
    output: &Path,
    coverage: &Path,
) -> Result<(), String> {
    rlog.flush()
        .map_err(|error| format!("could not flush rlog: {error}"))?;
    rlog.get_ref()
        .sync_all()
        .map_err(|error| format!("could not sync rlog: {error}"))?;
    drop(rlog);

    let coverage_file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(coverage_partial)
        .map_err(|error| format!("could not create partial coverage report: {error}"))?;
    let mut coverage_writer = BufWriter::new(coverage_file);
    serde_json::to_writer_pretty(&mut coverage_writer, report)
        .map_err(|error| format!("could not serialize coverage report: {error}"))?;
    coverage_writer
        .write_all(b"\n")
        .and_then(|_| coverage_writer.flush())
        .and_then(|_| coverage_writer.get_ref().sync_all())
        .map_err(|error| format!("could not finalize coverage report: {error}"))?;
    drop(coverage_writer);

    std::fs::rename(coverage_partial, coverage)
        .map_err(|error| format!("could not publish coverage report: {error}"))?;
    std::fs::rename(output_partial, output)
        .map_err(|error| format!("could not publish sealed rlog: {error}"))?;
    Ok(())
}

fn replay_combat_log(path: &Path) -> Result<PluginRunReport, String> {
    let file =
        File::open(path).map_err(|error| format!("could not open {}: {error}", path.display()))?;
    replay_rlog(
        BufReader::new(file),
        CombatTimelinePlugin::new(),
        RlogLimits::default(),
        PluginRunLimits::default(),
    )
    .map_err(|error| error.to_string())
}

fn resolve_server_realm(
    pack_path: &Path,
    connections: &ResearchConnectionFile,
) -> Result<Option<ResolvedRegion>, String> {
    let Some(build_folder) = pack_path.parent() else {
        return Ok(None);
    };
    let Some(deployment_folder) = build_folder.parent() else {
        return Ok(None);
    };
    let catalog_path = deployment_folder.join("server-realms.json");
    if !catalog_path.is_file() {
        return Ok(None);
    }
    let catalog = ServerRealmCatalog::from_json(
        &std::fs::read(&catalog_path)
            .map_err(|error| format!("could not read server realm catalog: {error}"))?,
    )
    .map_err(|error| format!("server realm catalog is invalid: {error}"))?;
    let mut resolved: Option<ResolvedRegion> = None;
    for connection in &connections.connections {
        let endpoint = NetworkEndpoint {
            address: connection.server.address.to_string(),
            port: connection.server.port,
        };
        let candidate = match catalog.resolve(&endpoint) {
            Ok(candidate) => candidate,
            Err(RegionResolverError::NoMatch { .. }) => continue,
            Err(error) => return Err(format!("server resolution failed: {error}")),
        };
        if let Some(current) = &mut resolved {
            if current.identity != candidate.identity {
                return Err("capture connections resolve to conflicting server realms".into());
            }
            for evidence in candidate.evidence {
                if !current.evidence.contains(&evidence) {
                    current.evidence.push(evidence);
                }
            }
        } else {
            resolved = Some(candidate);
        }
    }
    Ok(resolved)
}

fn existing_file(value: &str, label: &str) -> Result<PathBuf, String> {
    let path = PathBuf::from(value);
    let path = std::fs::canonicalize(&path)
        .map_err(|error| format!("{label} path could not be resolved: {error}"))?;
    if !path.is_file() {
        return Err(format!("{label} path is not a file"));
    }
    Ok(path)
}

fn validate_identifier(field: &str, value: &str) -> Result<(), String> {
    if value.is_empty()
        || value.len() > 128
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-'))
    {
        return Err(format!(
            "{field} must use 1-128 ASCII letters, digits, '.', '_', or '-'"
        ));
    }
    Ok(())
}

fn partial_path(output: &Path) -> Result<PathBuf, String> {
    let name = output
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or("output must have a valid UTF-8 filename")?;
    Ok(output.with_file_name(format!(".{name}.partial")))
}

fn ensure_outputs_available(paths: &[&Path]) -> Result<(), String> {
    for path in paths {
        if path.exists() {
            return Err(format!(
                "refusing to overwrite existing runtime output {}",
                path.display()
            ));
        }
    }
    Ok(())
}

fn remove_owned_partial(path: &Path) {
    if path.is_file() {
        let _ = std::fs::remove_file(path);
    }
}

fn unix_millis() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .try_into()
        .unwrap_or(u64::MAX)
}

fn display_path(path: &Path) -> String {
    let value = path.display().to_string();
    value
        .strip_prefix(r"\\?\")
        .unwrap_or(value.as_str())
        .to_owned()
}

fn runtime_environment() -> RuntimeEnvironment {
    let dumpcap = default_dumpcap_path().filter(|path| path.is_file());
    RuntimeEnvironment {
        platform: std::env::consts::OS,
        game_processes: discover_game_processes().unwrap_or_default(),
        dumpcap_path: dumpcap.as_deref().map(display_path),
        capture_interfaces: dumpcap
            .as_deref()
            .and_then(|path| discover_capture_interfaces(path).ok())
            .unwrap_or_default(),
    }
}

fn default_dumpcap_path() -> Option<PathBuf> {
    #[cfg(windows)]
    {
        Some(PathBuf::from(r"C:\Program Files\Wireshark\dumpcap.exe"))
    }
    #[cfg(not(windows))]
    {
        for path in ["/usr/bin/dumpcap", "/usr/local/bin/dumpcap"] {
            let path = PathBuf::from(path);
            if path.is_file() {
                return Some(path);
            }
        }
        None
    }
}

fn discover_capture_interfaces(path: &Path) -> Result<Vec<CaptureInterfaceView>, String> {
    let output = std::process::Command::new(path)
        .arg("-D")
        .output()
        .map_err(|error| format!("could not list dumpcap interfaces: {error}"))?;
    if !output.status.success() {
        return Err(format!(
            "dumpcap interface discovery failed ({})",
            output.status
        ));
    }
    let stdout = String::from_utf8_lossy(&output.stdout);
    Ok(stdout
        .lines()
        .filter_map(|line| {
            let line = line.trim();
            let (number, _) = line.split_once('.')?;
            if number.is_empty() || !number.bytes().all(|byte| byte.is_ascii_digit()) {
                return None;
            }
            Some(CaptureInterfaceView {
                value: number.into(),
                label: line.into(),
            })
        })
        .collect())
}

#[cfg(windows)]
fn discover_game_processes() -> Result<Vec<GameProcessView>, String> {
    let manifest = rlogs_game_bpsr::bundled_manifest()
        .map_err(|error| format!("BPSR manifest is invalid: {error}"))?;
    let names = manifest
        .process_selector
        .ok_or("BPSR manifest has no process selector")?
        .windows_executable_names;
    // SAFETY: the returned snapshot handle is checked, used with a correctly
    // sized PROCESSENTRY32W, and closed exactly once before return.
    let snapshot = unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) };
    if snapshot == INVALID_HANDLE_VALUE {
        return Err(format!(
            "process discovery failed: {}",
            std::io::Error::last_os_error()
        ));
    }
    let mut processes = Vec::new();
    let mut entry: PROCESSENTRY32W = unsafe { std::mem::zeroed() };
    entry.dwSize = std::mem::size_of::<PROCESSENTRY32W>() as u32;
    let mut has_entry = unsafe { Process32FirstW(snapshot, &mut entry) } != 0;
    while has_entry {
        let length = entry
            .szExeFile
            .iter()
            .position(|character| *character == 0)
            .unwrap_or(entry.szExeFile.len());
        let name = String::from_utf16_lossy(&entry.szExeFile[..length]);
        if names
            .iter()
            .any(|candidate| candidate.eq_ignore_ascii_case(&name))
        {
            processes.push(GameProcessView {
                process_id: entry.th32ProcessID,
                executable_name: name,
            });
        }
        has_entry = unsafe { Process32NextW(snapshot, &mut entry) } != 0;
    }
    unsafe {
        CloseHandle(snapshot);
    }
    processes.sort_by_key(|process| process.process_id);
    Ok(processes)
}

#[cfg(not(windows))]
fn discover_game_processes() -> Result<Vec<GameProcessView>, String> {
    Ok(Vec::new())
}

#[derive(Debug)]
struct HttpRequest {
    method: String,
    route: String,
    body: Vec<u8>,
}

fn handle_connection(
    mut stream: TcpStream,
    ui_root: &Path,
    controller: &RuntimeController,
) -> Result<(), Box<dyn std::error::Error>> {
    stream.set_read_timeout(Some(Duration::from_secs(5)))?;
    stream.set_write_timeout(Some(Duration::from_secs(10)))?;
    let request = read_request(&mut stream)?;
    match (request.method.as_str(), request.route.as_str()) {
        ("GET", "/api/runtime/status") => {
            write_json(&mut stream, 200, &controller.snapshot())?;
        }
        ("GET", "/api/runtime/environment") => {
            write_json(&mut stream, 200, &runtime_environment())?;
        }
        ("POST", "/api/runtime/reference-replay") => match controller.run_reference_replay() {
            Ok(result) => write_json(&mut stream, 200, &result)?,
            Err(error) => write_api_error(&mut stream, 409, error)?,
        },
        ("POST", "/api/runtime/offline") => {
            let request: OfflineSessionRequest = match serde_json::from_slice(&request.body) {
                Ok(request) => request,
                Err(error) => {
                    write_api_error(&mut stream, 400, format!("invalid request: {error}"))?;
                    return Ok(());
                }
            };
            match controller.start_offline(request) {
                Ok(()) => write_json(&mut stream, 202, &serde_json::json!({"accepted": true}))?,
                Err(error) => write_api_error(&mut stream, 409, error)?,
            }
        }
        ("POST", "/api/runtime/live/start") => {
            let request: LiveSessionRequest = match serde_json::from_slice(&request.body) {
                Ok(request) => request,
                Err(error) => {
                    write_api_error(&mut stream, 400, format!("invalid request: {error}"))?;
                    return Ok(());
                }
            };
            match controller.start_live(request) {
                Ok(()) => write_json(&mut stream, 202, &serde_json::json!({"accepted": true}))?,
                Err(error) => write_api_error(&mut stream, 409, error)?,
            }
        }
        ("POST", "/api/runtime/live/stop") => match controller.stop_live() {
            Ok(()) => write_json(&mut stream, 202, &serde_json::json!({"accepted": true}))?,
            Err(error) => write_api_error(&mut stream, 409, error)?,
        },
        ("GET", route) if !route.starts_with("/api/") => {
            write_static(&mut stream, ui_root, route)?;
        }
        _ => write_api_error(&mut stream, 404, "route not found".into())?,
    }
    Ok(())
}

fn read_request(stream: &mut TcpStream) -> Result<HttpRequest, Box<dyn std::error::Error>> {
    let mut bytes = Vec::new();
    let header_end;
    loop {
        if bytes.len() >= MAX_REQUEST_HEADER_BYTES {
            return Err("request headers exceed the local-host limit".into());
        }
        let mut buffer = [0_u8; 4096];
        let count = stream.read(&mut buffer)?;
        if count == 0 {
            return Err("connection closed before request headers completed".into());
        }
        bytes.extend_from_slice(&buffer[..count]);
        if let Some(index) = find_bytes(&bytes, b"\r\n\r\n") {
            header_end = index + 4;
            break;
        }
    }
    let headers = std::str::from_utf8(&bytes[..header_end])?;
    let mut lines = headers.split("\r\n");
    let request_line = lines.next().ok_or("missing HTTP request line")?;
    let mut request_parts = request_line.split_whitespace();
    let method = request_parts
        .next()
        .ok_or("missing HTTP method")?
        .to_owned();
    let route = request_parts
        .next()
        .ok_or("missing HTTP route")?
        .split('?')
        .next()
        .unwrap_or("/")
        .to_owned();
    let mut content_length = 0_usize;
    for line in lines {
        let Some((name, value)) = line.split_once(':') else {
            continue;
        };
        if name.eq_ignore_ascii_case("content-length") {
            content_length = value.trim().parse()?;
        }
    }
    if content_length > MAX_REQUEST_BODY_BYTES {
        return Err("request body exceeds the local-host limit".into());
    }
    while bytes.len() - header_end < content_length {
        let mut buffer = [0_u8; 8192];
        let count = stream.read(&mut buffer)?;
        if count == 0 {
            return Err("connection closed before request body completed".into());
        }
        bytes.extend_from_slice(&buffer[..count]);
    }
    Ok(HttpRequest {
        method,
        route,
        body: bytes[header_end..header_end + content_length].to_vec(),
    })
}

fn write_static(
    stream: &mut TcpStream,
    ui_root: &Path,
    route: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    let relative = if route == "/" {
        Path::new("index.html")
    } else {
        Path::new(route.trim_start_matches('/'))
    };
    if relative
        .components()
        .any(|component| !matches!(component, Component::Normal(_)))
    {
        write_text(stream, 404, "text/plain; charset=utf-8", b"Not found")?;
        return Ok(());
    }
    let requested = ui_root.join(relative);
    let path = match std::fs::canonicalize(&requested) {
        Ok(path) if path.starts_with(ui_root) && path.is_file() => path,
        _ => ui_root.join("index.html"),
    };
    let content_type = match path.extension().and_then(|extension| extension.to_str()) {
        Some("html") => "text/html; charset=utf-8",
        Some("css") => "text/css; charset=utf-8",
        Some("js") => "text/javascript; charset=utf-8",
        Some("map") | Some("json") => "application/json; charset=utf-8",
        Some("svg") => "image/svg+xml",
        Some("png") => "image/png",
        _ => "application/octet-stream",
    };
    write_text(stream, 200, content_type, &std::fs::read(path)?)?;
    Ok(())
}

fn write_json(
    stream: &mut TcpStream,
    status: u16,
    value: &impl Serialize,
) -> Result<(), Box<dyn std::error::Error>> {
    let body = serde_json::to_vec(value)?;
    write_text(stream, status, "application/json; charset=utf-8", &body)
}

fn write_api_error(
    stream: &mut TcpStream,
    status: u16,
    detail: String,
) -> Result<(), Box<dyn std::error::Error>> {
    write_json(stream, status, &serde_json::json!({"error": detail}))
}

fn write_text(
    stream: &mut TcpStream,
    status: u16,
    content_type: &str,
    body: &[u8],
) -> Result<(), Box<dyn std::error::Error>> {
    let reason = match status {
        200 => "OK",
        202 => "Accepted",
        400 => "Bad Request",
        404 => "Not Found",
        409 => "Conflict",
        _ => "Error",
    };
    write!(
        stream,
        "HTTP/1.1 {status} {reason}\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\nCache-Control: no-store\r\nX-Content-Type-Options: nosniff\r\nConnection: close\r\n\r\n",
        body.len()
    )?;
    stream.write_all(body)?;
    stream.flush()?;
    Ok(())
}

fn find_bytes(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack
        .windows(needle.len())
        .position(|window| window == needle)
}

#[cfg(test)]
mod tests {
    use std::net::{IpAddr, Ipv4Addr};

    use bytes::Bytes;
    use etherparse::{PacketBuilder, TcpHeader};
    use rlogs_capture::{CaptureLinkType, CapturedFrame, PcapWriter, TimestampNormalization};
    use rlogs_core::GameConnection;
    use rlogs_game_bpsr::FragmentKind;
    use rlogs_network::IpEndpoint;

    use super::*;

    fn temporary_root() -> PathBuf {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!(
            "rlogs-desktop-host-{}-{unique}",
            std::process::id()
        ))
    }

    fn endpoint(last: u8, port: u16) -> IpEndpoint {
        IpEndpoint::new(IpAddr::V4(Ipv4Addr::new(10, 0, 0, last)), port)
    }

    fn fixture_pcap(client: IpEndpoint, server: IpEndpoint) -> Vec<u8> {
        let protobuf = [0x0a, 0x00];
        let length = 6 + 16 + protobuf.len();
        let mut bpsr = Vec::with_capacity(length);
        bpsr.extend_from_slice(&(length as u32).to_be_bytes());
        bpsr.extend_from_slice(&FragmentKind::Notify.wire_id().to_be_bytes());
        bpsr.extend_from_slice(&1_664_308_034_u64.to_be_bytes());
        bpsr.extend_from_slice(&1_u32.to_be_bytes());
        bpsr.extend_from_slice(&3_u32.to_be_bytes());
        bpsr.extend_from_slice(&protobuf);

        let tcp = TcpHeader::new(server.port, client.port, 100, 16_384);
        let IpAddr::V4(server_address) = server.address else {
            unreachable!();
        };
        let IpAddr::V4(client_address) = client.address else {
            unreachable!();
        };
        let builder = PacketBuilder::ipv4(server_address.octets(), client_address.octets(), 64)
            .tcp_header(tcp);
        let mut packet = Vec::with_capacity(builder.size(bpsr.len()));
        builder.write(&mut packet, &bpsr).unwrap();
        let frame = CapturedFrame {
            sequence: 1,
            observed_micros: 0,
            source_timestamp_nanos: Some(1_000_000),
            timestamp_normalization: TimestampNormalization::Exact,
            interface_id: None,
            link_type: CaptureLinkType::RawIpv4,
            original_length: packet.len() as u32,
            bytes: Bytes::from(packet),
        };
        let mut writer = PcapWriter::new(Vec::new(), CaptureLinkType::RawIpv4).unwrap();
        writer.write_frame(&frame).unwrap();
        writer.flush().unwrap();
        writer.into_inner()
    }

    #[test]
    fn options_default_to_loopback_and_reject_unknown_flags() {
        let options = Options::parse(Vec::<String>::new()).unwrap();
        assert_eq!(options.bind, DEFAULT_BIND);
        assert!(Options::parse(["--wat".into()]).is_err());
    }

    #[test]
    fn full_offline_capture_reaches_sealed_log_and_combat_plugin() {
        let root = temporary_root();
        let input = root.join("input");
        let output = root.join("output");
        std::fs::create_dir_all(&input).unwrap();
        let client = endpoint(1, 31_000);
        let server = endpoint(2, 32_000);
        let capture = input.join("fixture.pcap");
        let connections = input.join("fixture.connections.json");
        std::fs::write(&capture, fixture_pcap(client, server)).unwrap();
        std::fs::write(
            &connections,
            serde_json::to_vec_pretty(&ResearchConnectionFile {
                schema_version: 1,
                connections: vec![GameConnection { client, server }],
            })
            .unwrap(),
        )
        .unwrap();
        let manifest_root = Path::new(env!("CARGO_MANIFEST_DIR"));
        let pack = manifest_root.join(
            "../../plugins/games/blue-protocol-star-resonance/protocol-packs/global/reference-v1/pack.json",
        );
        let result = process_offline_session(
            manifest_root.join("../..").as_path(),
            &OfflineSessionRequest {
                session_id: "fixture-session".into(),
                capture_path: capture.display().to_string(),
                connections_path: connections.display().to_string(),
                pack_path: Some(pack.display().to_string()),
                output_directory: Some(output.display().to_string()),
                region_id: None,
            },
        )
        .unwrap();

        assert_eq!(result.frame_count, Some(1));
        assert_eq!(result.framed_record_count, Some(1));
        assert_eq!(result.canonical_event_count, 2);
        assert_eq!(result.known_route_count, Some(1));
        assert_eq!(result.combat_plugin.rlog.event_count, 2);
        assert_eq!(result.combat_plugin.metrics.events_delivered, 1);
        assert!(Path::new(&result.output_rlog).is_file());
        assert!(Path::new(result.coverage_report.as_ref().unwrap()).is_file());

        std::fs::remove_dir_all(root).unwrap();
    }
}
