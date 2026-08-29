use std::collections::{BTreeMap, BTreeSet, VecDeque};
mod character_identities;
mod combat_history;
mod combat_meter_settings;
mod combat_overlay_settings;
mod core_settings;
mod hotkey_settings;
mod layout_settings;
mod native_plugin_processes;
mod profile_packages;
mod submission_connection;
mod submission_policy;
mod submission_queue;
mod submission_transport;
mod theme_settings;

use std::fs::{File, OpenOptions};
use std::io::{BufRead, BufReader, BufWriter, Read, Seek, SeekFrom, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::path::{Component, Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::mpsc::{SyncSender, TrySendError, sync_channel};
use std::sync::{Arc, Condvar, Mutex};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use character_identities::{
    CaptureTimeCharacterIdentityStore, CharacterIdentityResolver, CharacterIdentityStore,
};
use combat_history::{CombatHistoryCatalog, CombatHistoryDeleteResult, CombatHistoryStore};
use combat_meter_settings::{CombatMeterSettings, CombatMeterSettingsStore};
use combat_overlay_settings::{CombatOverlaySettings, CombatOverlaySettingsStore};
use core_settings::{CoreSettings, CoreSettingsStore};
use hotkey_settings::HotkeySettingsStore;
pub use hotkey_settings::{
    COMBAT_OVERLAY_TOGGLE_ACTION_ID, HotkeyAssignmentRequest, HotkeyAssignmentResult,
    HotkeySettingsView,
};
use layout_settings::{LayoutSettings, LayoutSettingsStore};
use native_plugin_processes::{NativePluginLaunch, NativePluginProcesses};
use profile_packages::{
    LocalProfilePackageStore, ProfilePackageInspection, ProfilePackageStoreView, ProfilePackageView,
};
use rlogs_capture::{CaptureSource, OfflineCapture};
#[cfg(windows)]
use rlogs_capture::{
    DumpcapLiveConfig, LiveCaptureStopHandle, OwnedProcessCaptureConfig, WindowsCaptureAdapter,
    WindowsCaptureAdapterRecommendationSource, WindowsOwnedDumpcapCapture,
    recommend_windows_capture_adapter, windows_capture_adapters,
};
use rlogs_core::{GameConnection, ResearchConnectionFile};
use rlogs_events::{
    ActorLoadoutSlot, CanonicalEvent, DataGapKind, DungeonEventKind, EntityRef, EventEnvelope,
    EventTopic, RegionContext, RegionEvidence, RegionEvidenceKind, RegionIdentity, RunState,
    TimelineEventKind,
};
use rlogs_game_bpsr::{
    BPSR_GAME_PLUGIN_ID, BpsrRemoteFactorLearner, BpsrRemoteFactorTimeline, BpsrSceneRunIdentity,
    BpsrStateDamageContributionProjector, ContinuousBpsrRecorder, ContinuousRecordingConfig,
    ContinuousResearchJournalConfig, GameBuild, LiveProtocolPackKind, NetworkEndpoint,
    OfflineRecordingConfig, OfflineRecordingLimits, OfflineRecordingReport, ProtocolPack,
    ProtocolRuntimeConfig, RDPS_VALIDATION_REPORT_SCHEMA_VERSION, RdpsValidationAnalyzer,
    RdpsValidationProgress, RdpsValidationReport, RegionResolverError, ResolvedRegion,
    SealedDungeonRunLog, ServerRealmCatalog, auxiliary_action_presentation,
    battle_imagine_presentation, bundled_run_reducer_config, bundled_scene_run_identities,
    character_id_from_entity_uuid, combat_action_presentation, confirmed_damage_contribution_rules,
    is_boss_monster, is_localized_class_name, localized_auxiliary_action_name,
    localized_battle_imagine_name, localized_class_identities, localized_combat_action_name,
    localized_monster_name, localized_recount_group_name, localized_scene_name,
    localized_specialization_identities, localized_status_effect_name,
    project_local_profile_packages, record_offline_capture, resolve_actor_combat_identity,
    resolve_actor_combat_presentation, resolve_live_steam_protocol_pack, scene_boss_monster_ids,
    state_damage_contribution_formula_identity, state_damage_contribution_target_matches,
    status_effect_presentation, weapon_level_presentation, weapon_presentation,
};
use rlogs_log_format::{RlogHeader, RlogLimits, RlogReader, RlogReplaySummary};
use rlogs_plugin_api::{PluginCapability, PluginDependency, PluginRuntime, PluginWorkspaceTabKind};
use rlogs_plugin_combat_meter::{
    COMBAT_SNAPSHOT_SCHEMA_ID, COMBAT_SNAPSHOT_SCHEMA_VERSION, CombatHistorySnapshot,
    CombatRunHistory, CombatTimelinePlugin, CombatTimelineSnapshot, HistoryLoadoutSlot,
    LiveHealthAttributeMapping,
};
use rlogs_plugin_encounter_recorder::{
    EncounterRecorderPlugin, RUN_PROJECTION_SCHEMA_ID, RUN_PROJECTION_SCHEMA_VERSION,
    RunProjectionSnapshot,
};
use rlogs_plugin_host::{
    PluginDiscoveryIssue, PluginDiscoveryReport, PluginOrderError, PluginPackage,
    PluginWorkspaceError, ResolvedPluginWorkspace, discover_installed_plugins,
    discover_plugin_packages, resolve_plugin_load_order, resolve_plugin_settings_tabs,
    resolve_plugin_workspaces,
};
use rlogs_plugin_runtime::{
    PluginOutput, PluginRunLimits, PluginRunMetrics, PluginRunReport, ReplayPlugin,
    replay_rlog_pair,
};
use rlogs_submission::{
    ArtifactBuildLimits, LocalLogArtifact, LogArtifactTrackingReader,
    MAXIMUM_LOCAL_ARTIFACT_PATH_BYTES, MAXIMUM_UPLOAD_CHUNK_BYTES, MockSubmissionReceiver,
    QueuedSubmission, ReportVisibility, Sha256Digest, SubmissionMetadata, SubmissionState,
    build_privacy_verified_submission_artifact, build_sealed_log_artifact,
    submission_privacy_policy_digest, write_privacy_filtered_submission_log,
};
use serde::{Deserialize, Serialize};
use submission_connection::{SubmissionConnectionStore, SubmissionConnectionView};
use submission_policy::{SubmissionPolicy, SubmissionPolicyStore, SubmissionPolicyView};
use submission_queue::{LocalSubmissionQueue, QueueInsertOutcome, SubmissionQueueView};
use submission_transport::{SubmissionTransport, SubmissionTransportResult};
use theme_settings::{ThemeSettings, ThemeSettingsStore};
#[cfg(windows)]
use windows_sys::Win32::{
    Foundation::{CloseHandle, INVALID_HANDLE_VALUE},
    System::Diagnostics::ToolHelp::{
        CreateToolhelp32Snapshot, PROCESSENTRY32W, Process32FirstW, Process32NextW,
        TH32CS_SNAPPROCESS,
    },
    System::Threading::{
        OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION, QueryFullProcessImageNameW,
    },
};

const DEFAULT_BIND: &str = "127.0.0.1:7419";
const MAX_REQUEST_HEADER_BYTES: usize = 64 * 1024;
const MAX_REQUEST_BODY_BYTES: usize = 1024 * 1024;
const MAX_OVERLAY_BACKGROUND_BYTES: usize = 8 * 1024 * 1024;
const PROVISIONAL_RESEARCH_SERVICE_NAMES: [&str; 3] = ["World", "WorldNtf", "GrpcTeamNtf"];

fn provisional_research_service_ids(pack: &ProtocolPack) -> BTreeSet<u64> {
    pack.definition()
        .routes
        .iter()
        .filter(|route| PROVISIONAL_RESEARCH_SERVICE_NAMES.contains(&route.service_name.as_str()))
        .map(|route| route.route.service_id)
        .collect()
}
const MAX_CONCURRENT_LOCAL_REQUESTS: usize = 16;
const PLUGIN_CATALOG_SCHEMA_VERSION: u16 = 2;
const PLUGIN_ENABLEMENT_SCHEMA_VERSION: u16 = 1;
const MAX_PLUGIN_ENABLEMENT_BYTES: u64 = 256 * 1024;
const MAX_PLUGIN_SURFACE_BYTES: u64 = 4 * 1024 * 1024;
const EVENT_VIEWER_SCHEMA_VERSION: u16 = 1;
const DEFAULT_EVENT_VIEWER_PAGE_SIZE: usize = 100;
const MAX_EVENT_VIEWER_PAGE_SIZE: usize = 200;
const MAX_EVENT_VIEWER_SCAN_PER_PAGE: u64 = 50_000;
const MAX_EVENT_VIEWER_PAGE_BYTES: usize = 8 * 1024 * 1024;
const MAX_EVENT_VIEWER_FILTER_BYTES: usize = 128;
const MAX_EVENT_VIEWER_SCAN_TIME: Duration = Duration::from_millis(100);

pub fn run_browser_host_from_env() -> Result<(), Box<dyn std::error::Error>> {
    let options = Options::parse(std::env::args().skip(1))?;
    let install_root = std::fs::canonicalize(&options.install_root)?;
    let ui_root = resolve_ui_root(&install_root)?;
    let bind: SocketAddr = options.bind.parse()?;
    if !bind.ip().is_loopback() {
        return Err("the local control host may bind only to a loopback address".into());
    }
    let listener = TcpListener::bind(bind)?;
    let controller = Arc::new(RuntimeController::new(install_root)?);
    controller.start_automatic_monitor(None)?;
    controller.start_history_rdps_backfill_worker(None)?;
    controller.start_automatic_submission_uploader(None)?;
    let address = listener.local_addr()?;
    println!("rLogs local controls: http://{address}");
    println!("Press Ctrl+C to stop the local host.");
    serve_local_host(listener, ui_root, controller, None)?;
    Ok(())
}

pub struct EmbeddedLocalHost {
    address: SocketAddr,
    shutdown: Arc<AtomicBool>,
    controller: Arc<RuntimeController>,
    worker: Option<JoinHandle<()>>,
}

#[derive(Debug, Clone)]
pub struct LiveCombatActivityObserver {
    feed: Arc<LiveCombatFeed>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LiveCombatActivityUpdate {
    pub revision: u64,
    pub combat_active: bool,
    pub last_hostile_micros: Option<u64>,
    /// Monotonic count of damage rows retained by the live reducer.
    ///
    /// This is deliberately independent from `combat_active`: a physically
    /// hidden WebView must be woken by the damage event itself, even if its
    /// JavaScript loop has not yet observed the reducer's boundary state.
    pub damage_event_count: u64,
}

impl LiveCombatActivityObserver {
    pub fn current(&self) -> LiveCombatActivityUpdate {
        self.feed.current_activity()
    }

    pub fn wait_after(&self, after_revision: u64, timeout: Duration) -> LiveCombatActivityUpdate {
        self.feed.wait_activity_after(after_revision, timeout)
    }
}

impl EmbeddedLocalHost {
    pub fn address(&self) -> SocketAddr {
        self.address
    }

    pub fn hotkey_settings(&self) -> HotkeySettingsView {
        self.controller.hotkey_settings()
    }

    /// Settings owned by the native host rather than an individual plug-in.
    pub fn core_settings(&self) -> CoreSettings {
        self.controller.core_settings()
    }

    /// Foreground process names declared by the active game integration.
    ///
    /// Native window policy uses these declarations without embedding a game
    /// executable name in the game-agnostic overlay host.
    pub fn foreground_game_process_names(&self) -> Vec<String> {
        let Ok(manifest) = rlogs_game_bpsr::bundled_manifest() else {
            return Vec::new();
        };
        let Some(selector) = manifest.process_selector else {
            return Vec::new();
        };
        #[cfg(windows)]
        {
            selector.windows_executable_names
        }
        #[cfg(not(windows))]
        {
            selector.linux_process_names
        }
    }

    /// Native startup state for the preloaded Combat Overlay window.
    ///
    /// Reading this before the hidden WebView is built avoids making saved
    /// enablement depend on JavaScript running inside an invisible window.
    pub fn combat_overlay_startup_state(&self) -> (bool, bool) {
        let settings = self.controller.combat_overlay_settings();
        (
            settings.live_overlay_enabled,
            settings.auto_hide_outside_combat,
        )
    }

    /// A low-cost native visibility signal for the preloaded Combat Overlay.
    ///
    /// The renderer still owns all meter calculations and visibility timing.
    /// This observer only wakes a physically hidden WebView when reducer-owned
    /// combat becomes active, because Windows may suspend a hidden WebView's
    /// JavaScript polling loop.
    pub fn live_combat_activity_observer(&self) -> LiveCombatActivityObserver {
        LiveCombatActivityObserver {
            feed: Arc::clone(&self.controller.live_combat_feed),
        }
    }

    pub fn assign_hotkey(
        &self,
        request: HotkeyAssignmentRequest,
    ) -> Result<HotkeyAssignmentResult, String> {
        self.controller.assign_hotkey(request)
    }

    pub fn restore_hotkey_bindings(
        &self,
        bindings: BTreeMap<String, String>,
    ) -> Result<HotkeySettingsView, String> {
        self.controller.restore_hotkey_bindings(bindings)
    }

    pub fn shutdown_and_wait(&self) {
        self.shutdown.store(true, Ordering::Release);
        #[cfg(windows)]
        {
            let capture_active = self
                .controller
                .live_stop
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .is_some();
            if capture_active {
                let _ = self.controller.stop_live();
            }
            let deadline = Instant::now() + Duration::from_secs(30);
            while self
                .controller
                .live_stop
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .is_some()
            {
                if Instant::now() >= deadline {
                    eprintln!(
                        "rLogs shutdown timed out while finalizing the active capture; retained artifacts may require recovery"
                    );
                    break;
                }
                thread::sleep(Duration::from_millis(25));
            }
        }
    }
}

impl Drop for EmbeddedLocalHost {
    fn drop(&mut self) {
        self.shutdown_and_wait();
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

pub fn start_embedded_local_host(
    install_root: impl AsRef<Path>,
) -> Result<EmbeddedLocalHost, Box<dyn std::error::Error>> {
    let install_root = std::fs::canonicalize(install_root)?;
    let ui_root = resolve_ui_root(&install_root)?;
    let listener = TcpListener::bind("127.0.0.1:0")?;
    listener.set_nonblocking(true)?;
    let address = listener.local_addr()?;
    let controller = Arc::new(RuntimeController::new(install_root)?);
    let managed_controller = Arc::clone(&controller);
    let shutdown = Arc::new(AtomicBool::new(false));
    controller.start_automatic_monitor(Some(Arc::clone(&shutdown)))?;
    controller.start_history_rdps_backfill_worker(Some(Arc::clone(&shutdown)))?;
    controller.start_automatic_submission_uploader(Some(Arc::clone(&shutdown)))?;
    let worker_shutdown = Arc::clone(&shutdown);
    let worker = thread::Builder::new()
        .name("rlogs-embedded-host".into())
        .spawn(move || {
            if let Err(error) =
                serve_local_host(listener, ui_root, controller, Some(&worker_shutdown))
            {
                eprintln!("embedded rLogs host stopped unexpectedly: {error}");
            }
        })?;
    Ok(EmbeddedLocalHost {
        address,
        shutdown,
        controller: managed_controller,
        worker: Some(worker),
    })
}

fn resolve_ui_root(install_root: &Path) -> Result<PathBuf, Box<dyn std::error::Error>> {
    std::fs::canonicalize(install_root.join("apps/desktop/ui/dist")).map_err(|error| {
        format!("desktop UI build was not found; run `npm run build` in apps/desktop/ui: {error}")
            .into()
    })
}

fn serve_local_host(
    listener: TcpListener,
    ui_root: PathBuf,
    controller: Arc<RuntimeController>,
    shutdown: Option<&AtomicBool>,
) -> std::io::Result<()> {
    let ui_root = Arc::new(ui_root);
    let active_requests = Arc::new(AtomicUsize::new(0));
    loop {
        if shutdown.is_some_and(|shutdown| shutdown.load(Ordering::Acquire)) {
            return Ok(());
        }
        match listener.accept() {
            Ok((stream, _peer)) => {
                // A socket accepted from a non-blocking listener inherits that
                // mode on Windows. Request workers perform bounded blocking
                // reads, so restore blocking mode before handing it off.
                if let Err(error) = stream.set_nonblocking(false) {
                    eprintln!("local HTTP connection setup failed: {error}");
                    continue;
                }
                dispatch_local_request(
                    stream,
                    Arc::clone(&ui_root),
                    Arc::clone(&controller),
                    Arc::clone(&active_requests),
                );
            }
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                thread::sleep(Duration::from_millis(10));
            }
            Err(error) if error.kind() == std::io::ErrorKind::Interrupted => {}
            Err(error) => {
                eprintln!("local HTTP accept failed: {error}");
                if shutdown.is_some() {
                    return Err(error);
                }
            }
        }
    }
}

fn dispatch_local_request(
    stream: TcpStream,
    ui_root: Arc<PathBuf>,
    controller: Arc<RuntimeController>,
    active_requests: Arc<AtomicUsize>,
) {
    if active_requests.fetch_add(1, Ordering::AcqRel) >= MAX_CONCURRENT_LOCAL_REQUESTS {
        active_requests.fetch_sub(1, Ordering::AcqRel);
        eprintln!(
            "local HTTP request rejected: {MAX_CONCURRENT_LOCAL_REQUESTS} handlers are active"
        );
        return;
    }
    let request_counter = Arc::clone(&active_requests);
    let worker = thread::Builder::new()
        .name("rlogs-local-http".into())
        .spawn(move || {
            let _request = ActiveRequestGuard(request_counter);
            if let Err(error) = handle_connection(stream, &ui_root, &controller) {
                eprintln!("local HTTP request failed: {error}");
            }
        });
    if let Err(error) = worker {
        active_requests.fetch_sub(1, Ordering::AcqRel);
        eprintln!("local HTTP handler could not start: {error}");
    }
}

struct ActiveRequestGuard(Arc<AtomicUsize>);

impl Drop for ActiveRequestGuard {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::AcqRel);
    }
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
    monitored_frame_count: u64,
    decoded_event_count: u64,
    saving_run: bool,
    sealed_run_count: u64,
    last_result: Option<SessionResult>,
}

impl Default for RuntimeSnapshot {
    fn default() -> Self {
        Self {
            schema_version: 2,
            phase: RuntimePhase::Idle,
            active_session_id: None,
            detail: "Ready for a safe replay or offline capture.".into(),
            started_unix_millis: None,
            completed_unix_millis: None,
            live_capture_can_stop: false,
            monitored_frame_count: 0,
            decoded_event_count: 0,
            saving_run: false,
            sealed_run_count: 0,
            last_result: None,
        }
    }
}

const LIVE_COMBAT_FEED_SCHEMA_VERSION: u16 = 1;
const DEFAULT_LIVE_COMBAT_WAIT_MILLIS: u64 = 1_000;
const MAXIMUM_LIVE_COMBAT_WAIT_MILLIS: u64 = 5_000;
const COMBAT_HISTORY_FEED_SCHEMA_VERSION: u16 = 2;
const DEFAULT_COMBAT_HISTORY_WAIT_MILLIS: u64 = 5_000;
const MAXIMUM_COMBAT_HISTORY_WAIT_MILLIS: u64 = 30_000;

#[derive(Debug, Clone, Serialize)]
struct LiveCombatUpdate {
    schema_version: u16,
    revision: u64,
    snapshot: Option<CombatTimelineSnapshot>,
    run_projection: Option<CombatRunHistory>,
}

#[derive(Debug, Clone, Serialize)]
struct PresentedLiveCombatUpdate {
    schema_version: u16,
    revision: u64,
    snapshot: Option<CombatTimelineSnapshot>,
    actor_presentations: BTreeMap<String, LiveOverlayActorPresentation>,
    encounter_presentation: LiveOverlayEncounterPresentation,
}

#[derive(Debug, Clone, Default, Serialize)]
struct LiveOverlayEncounterPresentation {
    scene_id: Option<i32>,
    scene_name: Option<String>,
    bosses: Vec<LiveOverlayBossPresentation>,
    timer_source: String,
    run_projection: Option<CombatRunHistory>,
}

#[derive(Debug, Clone, Serialize)]
struct LiveOverlayBossPresentation {
    actor_id: String,
    monster_id: i64,
    name: String,
    current_hp: i64,
    max_hp: i64,
    bdps: f64,
    team_damage: i64,
}

#[derive(Debug, Clone)]
struct LiveOverlayBossCandidate {
    presentation: LiveOverlayBossPresentation,
    was_damaged: bool,
}

#[derive(Debug, Clone, Serialize)]
struct LiveOverlayActorPresentation {
    character_id: Option<String>,
    class_id: Option<i32>,
    specialization_id: Option<i32>,
    class_name: Option<String>,
    specialization_name: Option<String>,
    class_spec_icon_asset_path: Option<String>,
    role: Option<String>,
    accent: Option<String>,
    weapon: Option<LiveOverlayBadgePresentation>,
    primary_imagines: Vec<LiveOverlayBadgePresentation>,
}

#[derive(Debug, Clone, Serialize)]
struct LiveOverlayBadgePresentation {
    slot_id: Option<i32>,
    ability_id: Option<i64>,
    item_id: Option<i64>,
    tier: Option<u32>,
    level: Option<u32>,
    level_min: Option<u32>,
    level_max: Option<u32>,
    badge_kind: Option<String>,
    label: String,
    icon_asset_path: Option<String>,
}

fn live_boss_dps(damage_taken: i64, active_combat_micros: u64) -> f64 {
    if active_combat_micros == 0 {
        return 0.0;
    }
    damage_taken.max(0) as f64 * 1_000_000.0 / active_combat_micros.max(1_000_000) as f64
}

fn select_live_overlay_bosses(
    mut candidates: Vec<LiveOverlayBossCandidate>,
) -> Vec<LiveOverlayBossPresentation> {
    candidates.sort_by(|left, right| {
        right
            .presentation
            .max_hp
            .cmp(&left.presentation.max_hp)
            .then_with(|| left.presentation.actor_id.cmp(&right.presentation.actor_id))
    });
    if !candidates.iter().any(|boss| boss.was_damaged) {
        return Vec::new();
    }
    candidates
        .into_iter()
        .map(|boss| boss.presentation)
        .collect()
}

#[derive(Debug, Clone, Serialize)]
struct OverlayBarColorIdentityCatalog {
    classes: Vec<OverlayBarColorIdentity>,
    specializations: Vec<OverlayBarColorIdentity>,
}

#[derive(Debug, Clone, Serialize)]
struct OverlayBarColorIdentity {
    id: i32,
    label: String,
}

fn overlay_bar_color_identity_catalog() -> Result<OverlayBarColorIdentityCatalog, String> {
    Ok(OverlayBarColorIdentityCatalog {
        classes: localized_class_identities("en-US")?
            .into_iter()
            .map(|(id, label)| OverlayBarColorIdentity { id, label })
            .collect(),
        specializations: localized_specialization_identities("en-US")?
            .into_iter()
            .map(|(id, label)| OverlayBarColorIdentity { id, label })
            .collect(),
    })
}

fn live_overlay_primary_imagine_badge(slot: &ActorLoadoutSlot) -> LiveOverlayBadgePresentation {
    let presentation = slot
        .ability_id
        .and_then(|ability_id| battle_imagine_presentation(ability_id).ok().flatten());
    let item_id = slot
        .item_id
        .or_else(|| presentation.map(|value| value.item_id));
    // `tier` is the equipped remodel tier observed from the character's packet data.
    // `item_tier` in the presentation table is the Imagine's static catalog rarity;
    // it must never be substituted for missing runtime evidence.
    let tier = slot.tier;
    let label = item_id
        .and_then(|item_id| {
            localized_battle_imagine_name(item_id, "en-US")
                .ok()
                .flatten()
                .map(str::to_owned)
        })
        .or_else(|| item_id.map(|item_id| format!("Imagine item {item_id}")))
        .or_else(|| {
            slot.ability_id
                .map(|ability_id| format!("Imagine ability {ability_id}"))
        })
        .unwrap_or_else(|| format!("Primary Imagine slot {}", slot.slot_id));
    LiveOverlayBadgePresentation {
        slot_id: Some(slot.slot_id),
        ability_id: slot.ability_id,
        item_id,
        tier,
        level: None,
        level_min: None,
        level_max: None,
        badge_kind: None,
        label,
        icon_asset_path: presentation.map(|value| {
            format!(
                "/game-assets/blue-protocol-star-resonance/shared/{}",
                value.icon
            )
        }),
    }
}

fn present_live_combat_update(update: LiveCombatUpdate) -> PresentedLiveCombatUpdate {
    let timer_source = if update.run_projection.is_some() {
        "reviewed_dungeon"
    } else {
        "ambient_inactivity"
    };
    let encounter_presentation = update
        .snapshot
        .as_ref()
        .map(|snapshot| {
            let exact_scene_boss_ids = snapshot
                .scene_id
                .and_then(|scene_id| scene_boss_monster_ids(scene_id).ok().flatten());
            let boss_candidates = snapshot
                .actors
                .iter()
                .filter(|actor| actor.actor_kind.as_deref() == Some("monster"))
                .filter_map(|actor| {
                    let monster_id = actor.monster_id?;
                    let is_boss = exact_scene_boss_ids
                        .map(|ids| ids.contains(&monster_id))
                        .unwrap_or_else(|| is_boss_monster(monster_id).unwrap_or(false));
                    if !is_boss {
                        return None;
                    }
                    let current_hp = actor.current_hp?;
                    let max_hp = actor.max_hp?;
                    (max_hp > 0).then(|| LiveOverlayBossCandidate {
                        presentation: LiveOverlayBossPresentation {
                            actor_id: actor.actor_id.clone(),
                            monster_id,
                            name: localized_monster_name(monster_id, "en-US")
                                .ok()
                                .flatten()
                                .map(str::to_owned)
                                .or_else(|| actor.display_name.clone())
                                .unwrap_or_else(|| format!("Monster {monster_id}")),
                            current_hp,
                            max_hp,
                            bdps: live_boss_dps(actor.damage_taken, snapshot.active_combat_micros),
                            team_damage: actor.damage_taken.max(0),
                        },
                        was_damaged: actor.damage_taken > 0 || current_hp < max_hp,
                    })
                })
                .collect::<Vec<_>>();
            let bosses = select_live_overlay_bosses(boss_candidates);
            LiveOverlayEncounterPresentation {
                scene_id: snapshot.scene_id,
                scene_name: snapshot.scene_id.and_then(|scene_id| {
                    localized_scene_name(i64::from(scene_id), "en-US")
                        .ok()
                        .flatten()
                        .map(str::to_owned)
                }),
                bosses,
                timer_source: timer_source.into(),
                run_projection: update.run_projection.clone(),
            }
        })
        .unwrap_or_else(|| LiveOverlayEncounterPresentation {
            timer_source: timer_source.into(),
            run_projection: update.run_projection.clone(),
            ..LiveOverlayEncounterPresentation::default()
        });
    let actor_presentations = update
        .snapshot
        .as_ref()
        .map(|snapshot| {
            snapshot
                .actors
                .iter()
                .map(|actor| {
                    let ability_ids = actor
                        .abilities
                        .iter()
                        .filter_map(|ability| ability.ability_id.parse::<i64>().ok());
                    let presentation = resolve_actor_combat_presentation(
                        actor.class_id,
                        actor.specialization_id,
                        ability_ids,
                        "en-US",
                    )
                    .unwrap_or({
                        rlogs_game_bpsr::ActorCombatPresentation {
                            class_id: actor.class_id,
                            specialization_id: None,
                            class_name: None,
                            specialization_name: None,
                            icon: None,
                            role: None,
                            accent: None,
                        }
                    });
                    let weapon = actor.weapon_item_id.map(|item_id| {
                        let metadata = weapon_presentation(item_id);
                        let level =
                            weapon_level_presentation(item_id, actor.weapon_breakthrough_count);
                        LiveOverlayBadgePresentation {
                            slot_id: None,
                            ability_id: None,
                            item_id: Some(item_id),
                            tier: None,
                            level: level.and_then(|value| value.exact),
                            level_min: level.map(|value| value.minimum),
                            level_max: level.map(|value| value.maximum),
                            badge_kind: metadata.map(|value| value.badge_kind.to_owned()),
                            label: metadata
                                .map(|value| value.english_name.to_owned())
                                .unwrap_or_else(|| format!("Weapon item {item_id}")),
                            icon_asset_path: metadata.map(|value| {
                                format!(
                                    "/game-assets/blue-protocol-star-resonance/shared/{}",
                                    value.icon
                                )
                            }),
                        }
                    });
                    let primary_imagines = actor
                        .primary_loadout
                        .iter()
                        .take(2)
                        .map(live_overlay_primary_imagine_badge)
                        .collect();
                    (
                        actor.actor_id.clone(),
                        LiveOverlayActorPresentation {
                            character_id: live_overlay_character_id(
                                actor.actor_kind.as_deref(),
                                &actor.entity_uuid,
                            ),
                            class_id: presentation.class_id,
                            specialization_id: presentation.specialization_id,
                            class_name: presentation.class_name,
                            specialization_name: presentation.specialization_name,
                            class_spec_icon_asset_path: presentation.icon.map(|path| {
                                format!("/game-assets/blue-protocol-star-resonance/shared/{path}")
                            }),
                            role: presentation.role,
                            accent: presentation.accent,
                            weapon,
                            primary_imagines,
                        },
                    )
                })
                .collect()
        })
        .unwrap_or_default();
    PresentedLiveCombatUpdate {
        schema_version: update.schema_version,
        revision: update.revision,
        snapshot: update.snapshot,
        actor_presentations,
        encounter_presentation,
    }
}

#[derive(Debug, Default)]
struct LiveCombatFeed {
    state: Mutex<LiveCombatFeedState>,
    changed: Condvar,
}

#[derive(Debug, Default)]
struct LiveCombatFeedState {
    /// Advances only when the complete snapshot/projection changes.
    /// Browser long-poll clients use this revision, so lightweight damage
    /// wakeups must never advance it ahead of the data they are waiting for.
    revision: u64,
    /// Advances for native visibility activity, including the immediate
    /// decoded-damage signal that intentionally precedes snapshot reduction.
    activity_revision: u64,
    snapshot: Option<CombatTimelineSnapshot>,
    combat_active: bool,
    last_hostile_micros: Option<u64>,
    damage_event_count: u64,
    run_projection: Option<CombatRunHistory>,
    ambient_active_micros: u64,
    ambient_last_damage_micros: Option<u64>,
}

impl LiveCombatFeed {
    fn publish(&self, snapshot: Option<CombatTimelineSnapshot>) {
        self.publish_with_projection(snapshot, None, false);
    }

    fn publish_with_projection(
        &self,
        mut snapshot: Option<CombatTimelineSnapshot>,
        run_projection: Option<CombatRunHistory>,
        reviewed_dungeon: bool,
    ) {
        let mut state = self
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        state.revision = state.revision.saturating_add(1);
        state.activity_revision = state.activity_revision.saturating_add(1);
        if !reviewed_dungeon && let Some(snapshot) = snapshot.as_mut() {
            snapshot.active_combat_micros = state.ambient_active_micros;
            snapshot.game_time_micros = None;
            snapshot.true_time_micros = None;
        }
        if let Some(snapshot) = snapshot.as_ref() {
            state.combat_active = snapshot.combat_active;
            state.last_hostile_micros = snapshot.last_hostile_micros;
            let retained_hits = snapshot
                .actors
                .iter()
                .fold(0_u64, |total, actor| total.saturating_add(actor.hits));
            state.damage_event_count = state.damage_event_count.max(retained_hits);
        } else {
            state.combat_active = false;
            state.last_hostile_micros = None;
            state.damage_event_count = 0;
            state.ambient_active_micros = 0;
            state.ambient_last_damage_micros = None;
        }
        state.snapshot = snapshot;
        state.run_projection = run_projection;
        self.changed.notify_all();
    }

    /// Wakes native overlay visibility as soon as decoding yields damage.
    ///
    /// Full snapshots intentionally remain on their bounded publication path;
    /// visibility must not wait for projection, identity enrichment, or JSON
    /// presentation work that does not affect whether combat just occurred.
    fn signal_damage(&self, observed_micros: u64, reviewed_dungeon: bool) {
        let mut state = self
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        state.activity_revision = state.activity_revision.saturating_add(1);
        state.combat_active = true;
        state.last_hostile_micros = Some(observed_micros);
        state.damage_event_count = state.damage_event_count.saturating_add(1);
        if reviewed_dungeon {
            state.ambient_active_micros = 0;
            state.ambient_last_damage_micros = None;
        } else {
            if let Some(previous) = state.ambient_last_damage_micros {
                let delta = observed_micros.saturating_sub(previous);
                if delta <= 3_000_000 {
                    state.ambient_active_micros = state.ambient_active_micros.saturating_add(delta);
                }
            }
            state.ambient_last_damage_micros = Some(observed_micros);
        }
        self.changed.notify_all();
    }

    fn current_activity(&self) -> LiveCombatActivityUpdate {
        let state = self
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        Self::activity_update(&state)
    }

    fn wait_activity_after(
        &self,
        after_revision: u64,
        timeout: Duration,
    ) -> LiveCombatActivityUpdate {
        let deadline = Instant::now() + timeout;
        let mut state = self
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        while state.activity_revision <= after_revision {
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                break;
            }
            state = match self.changed.wait_timeout(state, remaining) {
                Ok((state, _)) => state,
                Err(poisoned) => poisoned.into_inner().0,
            };
        }
        Self::activity_update(&state)
    }

    fn activity_update(state: &LiveCombatFeedState) -> LiveCombatActivityUpdate {
        LiveCombatActivityUpdate {
            revision: state.activity_revision,
            combat_active: state.combat_active,
            last_hostile_micros: state.last_hostile_micros,
            damage_event_count: state.damage_event_count,
        }
    }

    fn current(&self) -> LiveCombatUpdate {
        let state = self
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        LiveCombatUpdate {
            schema_version: LIVE_COMBAT_FEED_SCHEMA_VERSION,
            revision: state.revision,
            snapshot: state.snapshot.clone(),
            run_projection: state.run_projection.clone(),
        }
    }

    fn wait_after(&self, after_revision: u64, timeout: Duration) -> LiveCombatUpdate {
        let deadline = Instant::now() + timeout;
        let mut state = self
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        while state.revision <= after_revision {
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                break;
            }
            state = match self.changed.wait_timeout(state, remaining) {
                Ok((state, _)) => state,
                Err(poisoned) => poisoned.into_inner().0,
            };
        }
        LiveCombatUpdate {
            schema_version: LIVE_COMBAT_FEED_SCHEMA_VERSION,
            revision: state.revision,
            snapshot: state.snapshot.clone(),
            run_projection: state.run_projection.clone(),
        }
    }
}

#[derive(Debug, Deserialize)]
struct LiveCombatWaitRequest {
    after_revision: u64,
    #[serde(default = "default_live_combat_wait_millis")]
    timeout_millis: u64,
}

#[derive(Debug, Clone, Serialize)]
struct CombatHistoryRevisionUpdate {
    schema_version: u16,
    revision: u64,
    catalog_changed: bool,
    rdps_refreshes: Vec<HistoryRdpsRefreshProgress>,
}

#[derive(Debug, Default)]
struct CombatHistoryRevisionFeed {
    state: Mutex<CombatHistoryRevisionState>,
    changed: Condvar,
}

#[derive(Debug, Default)]
struct CombatHistoryRevisionState {
    revision: u64,
    last_catalog_revision: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
enum HistoryRdpsRefreshStage {
    Queued,
    WaitingForLiveCapture,
    Replaying,
    ValidatingAndSaving,
    Failed,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
struct HistoryRdpsRefreshProgress {
    session_id: String,
    stage: HistoryRdpsRefreshStage,
    processed_events: u64,
    processed_bytes: u64,
    total_bytes: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    detail: Option<String>,
}

impl HistoryRdpsRefreshProgress {
    fn queued(session_id: String) -> Self {
        Self {
            session_id,
            stage: HistoryRdpsRefreshStage::Queued,
            processed_events: 0,
            processed_bytes: 0,
            total_bytes: 0,
            detail: None,
        }
    }
}

#[derive(Debug, Default)]
struct HistoryRdpsBackfillQueue {
    state: Mutex<HistoryRdpsBackfillState>,
    changed: Condvar,
}

#[derive(Debug, Default)]
struct HistoryRdpsBackfillState {
    pending: BTreeSet<String>,
    active: Option<String>,
    progress: BTreeMap<String, HistoryRdpsRefreshProgress>,
}

impl HistoryRdpsBackfillQueue {
    fn enqueue(&self, session_id: String) -> bool {
        let mut state = self
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if state.active.as_deref() == Some(session_id.as_str())
            || state.pending.contains(&session_id)
        {
            return false;
        }
        state.progress.insert(
            session_id.clone(),
            HistoryRdpsRefreshProgress::queued(session_id.clone()),
        );
        state.pending.insert(session_id);
        self.changed.notify_one();
        true
    }

    fn next(&self, shutdown: Option<&AtomicBool>) -> Option<String> {
        let mut state = self
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        loop {
            if shutdown.is_some_and(|shutdown| shutdown.load(Ordering::Acquire)) {
                return None;
            }
            if let Some(session_id) = state.pending.pop_first() {
                state.active = Some(session_id.clone());
                return Some(session_id);
            }
            state = match self.changed.wait_timeout(state, Duration::from_millis(500)) {
                Ok((state, _)) => state,
                Err(poisoned) => poisoned.into_inner().0,
            };
        }
    }

    fn update_progress(&self, progress: HistoryRdpsRefreshProgress) {
        self.state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .progress
            .insert(progress.session_id.clone(), progress);
    }

    fn requeue(&self, session_id: String) {
        let mut state = self
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if state.active.as_deref() == Some(session_id.as_str()) {
            state.active = None;
        }
        state.pending.insert(session_id);
        self.changed.notify_one();
    }

    fn finish(&self, session_id: &str) {
        let mut state = self
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if state.active.as_deref() == Some(session_id) {
            state.active = None;
        }
        state.progress.remove(session_id);
    }

    fn fail(&self, session_id: &str, detail: String) {
        let mut state = self
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if state.active.as_deref() == Some(session_id) {
            state.active = None;
        }
        state.progress.insert(
            session_id.to_owned(),
            HistoryRdpsRefreshProgress {
                session_id: session_id.to_owned(),
                stage: HistoryRdpsRefreshStage::Failed,
                processed_events: 0,
                processed_bytes: 0,
                total_bytes: 0,
                detail: Some(detail),
            },
        );
    }

    fn progress_snapshot(&self) -> Vec<HistoryRdpsRefreshProgress> {
        self.state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .progress
            .values()
            .cloned()
            .collect()
    }
}

impl CombatHistoryRevisionFeed {
    fn publish(&self) {
        let mut state = self
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        state.revision = state.revision.saturating_add(1);
        state.last_catalog_revision = state.revision;
        self.changed.notify_all();
    }

    fn publish_progress(&self) {
        let mut state = self
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        state.revision = state.revision.saturating_add(1);
        self.changed.notify_all();
    }

    fn wait_after(&self, after_revision: u64, timeout: Duration) -> (u64, bool) {
        let deadline = Instant::now() + timeout;
        let mut state = self
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        while state.revision <= after_revision {
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                break;
            }
            state = match self.changed.wait_timeout(state, remaining) {
                Ok((state, _)) => state,
                Err(poisoned) => poisoned.into_inner().0,
            };
        }
        (state.revision, state.last_catalog_revision > after_revision)
    }
}

#[derive(Debug, Deserialize)]
struct CombatHistoryWaitRequest {
    after_revision: u64,
    #[serde(default = "default_combat_history_wait_millis")]
    timeout_millis: u64,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct CombatHistoryDetailRequest {
    session_id: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct CombatHistoryFavoriteRequest {
    history_id: String,
    is_favorite: bool,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct CombatHistoryDeleteRequest {
    history_ids: Vec<String>,
}

fn default_live_combat_wait_millis() -> u64 {
    DEFAULT_LIVE_COMBAT_WAIT_MILLIS
}

fn default_combat_history_wait_millis() -> u64 {
    DEFAULT_COMBAT_HISTORY_WAIT_MILLIS
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
    combat_snapshot: CombatTimelineSnapshot,
    encounter_recorder: PluginRunReport,
    upload_artifact: Option<UploadArtifactView>,
    submission_queue_id: Option<String>,
    submission_queue_status: String,
    profile_package_count: usize,
    profile_sync_status: String,
    #[serde(skip_serializing)]
    verified_artifact: Option<LocalLogArtifact>,
}

#[derive(Debug, Clone)]
struct CapturedRunProjection {
    combat: CombatTimelineSnapshot,
    run: RunProjectionSnapshot,
    history: CombatHistorySnapshot,
}

#[derive(Debug, Clone, Serialize)]
struct UploadArtifactView {
    file_byte_length: u64,
    file_sha256: String,
    chunk_count: usize,
    canonical_content_sha256: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct RunReportView {
    schema_version: u16,
    source_rlog: String,
    artifact_digest: String,
    integrity_verified: bool,
    replay_metrics: PluginRunMetrics,
    projection: RunProjectionSnapshot,
}

impl From<&LocalLogArtifact> for UploadArtifactView {
    fn from(artifact: &LocalLogArtifact) -> Self {
        Self {
            file_byte_length: artifact.file_byte_length,
            file_sha256: artifact.file_sha256.to_string(),
            chunk_count: artifact.chunks.len(),
            canonical_content_sha256: artifact.rlog.content_sha256.clone(),
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct EventViewerFilter {
    #[serde(default)]
    topic: Option<EventTopic>,
    #[serde(default)]
    kind: Option<String>,
    #[serde(default)]
    search: Option<String>,
}

impl EventViewerFilter {
    fn normalized(self) -> Result<Self, String> {
        Ok(Self {
            topic: self.topic,
            kind: normalize_event_viewer_filter("kind", self.kind)?.map(|kind| kind.to_lowercase()),
            search: normalize_event_viewer_filter("search", self.search)?
                .map(|search| search.to_lowercase()),
        })
    }

    fn matches(&self, event: &EventViewerEventView) -> bool {
        if self.topic.is_some_and(|topic| event.topic != topic) {
            return false;
        }
        if self.kind.as_ref().is_some_and(|kind| event.kind != *kind) {
            return false;
        }
        self.search
            .as_ref()
            .is_none_or(|search| event.search_text().contains(search))
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct EventViewerPageRequest {
    #[serde(default)]
    query_id: Option<String>,
    #[serde(default)]
    limit: Option<usize>,
    #[serde(default)]
    filter: Option<EventViewerFilter>,
}

impl EventViewerPageRequest {
    fn page_size(&self) -> Result<usize, String> {
        let limit = self.limit.unwrap_or(DEFAULT_EVENT_VIEWER_PAGE_SIZE);
        if !(1..=MAX_EVENT_VIEWER_PAGE_SIZE).contains(&limit) {
            return Err(format!(
                "event page size must be between 1 and {MAX_EVENT_VIEWER_PAGE_SIZE}"
            ));
        }
        Ok(limit)
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct EventViewerPage {
    schema_version: u16,
    query_id: String,
    session_id: String,
    artifact_digest: String,
    header: RlogHeader,
    filter: EventViewerFilter,
    page_index: u64,
    scanned_this_page: u64,
    scanned_total: u64,
    matched_total: u64,
    integrity_verified: bool,
    complete: bool,
    events: Vec<EventViewerEventView>,
}

#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
struct EventViewerIdentifiersView {
    actor: Option<EventViewerEntityView>,
    source: Option<EventViewerEntityView>,
    direct_source: Option<EventViewerEntityView>,
    target: Option<EventViewerEntityView>,
    ability: Option<String>,
    status: Option<String>,
    status_instance: Option<String>,
    status_origin_type: Option<String>,
    status_origin_config: Option<String>,
    status_state: Option<String>,
    status_stacks: Option<String>,
    status_duration_millis: Option<String>,
    monster: Option<String>,
    scene: Option<String>,
    map: Option<String>,
    dungeon: Option<String>,
    character_id: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct EventViewerEntityView {
    actor_id: String,
    entity_uuid: String,
}

impl From<EntityRef> for EventViewerEntityView {
    fn from(entity: EntityRef) -> Self {
        Self {
            actor_id: entity.actor_id.0.to_string(),
            entity_uuid: entity.entity_uuid.0.to_string(),
        }
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct EventViewerEventView {
    sequence: u64,
    timeline_sequence: Option<u64>,
    observed_micros: u64,
    game_time_millis: Option<i64>,
    topic: EventTopic,
    kind: String,
    summary: String,
    amount: Option<String>,
    identifiers: EventViewerIdentifiersView,
    canonical_json: String,
}

const LIVE_EVENT_FEED_SCHEMA_VERSION: u16 = 1;
const LIVE_EVENT_RING_CAPACITY: usize = 8_192;
const DEFAULT_LIVE_EVENT_WAIT_MILLIS: u64 = 1_000;
const MAXIMUM_LIVE_EVENT_WAIT_MILLIS: u64 = 5_000;
const DEFAULT_LIVE_EVENT_BATCH_SIZE: usize = 256;
const MAXIMUM_LIVE_EVENT_BATCH_SIZE: usize = 512;
const LIVE_EVENT_COALESCE_MILLIS: u64 = 8;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct LiveEventLine {
    revision: u64,
    sequence: u64,
    observed_micros: u64,
    topic: EventTopic,
    kind: String,
    raw_ids: String,
}

impl LiveEventLine {
    fn from_envelope(canonical: &EventEnvelope) -> Self {
        let (topic, _timeline_sequence, kind, summary, _amount, _identifiers) =
            event_viewer_fields(canonical);
        Self {
            revision: 0,
            sequence: canonical.sequence,
            observed_micros: canonical.time.observed_micros,
            topic,
            kind: kind.into(),
            raw_ids: summary,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct LiveEventBatch {
    schema_version: u16,
    session_id: Option<String>,
    revision: u64,
    dropped_before: u64,
    has_more: bool,
    events: Vec<LiveEventLine>,
}

#[derive(Debug, Deserialize)]
struct LiveEventWaitRequest {
    after_revision: u64,
    #[serde(default = "default_live_event_wait_millis")]
    timeout_millis: u64,
    #[serde(default = "default_live_event_batch_size")]
    limit: usize,
    #[serde(default)]
    tail: bool,
}

fn default_live_event_wait_millis() -> u64 {
    DEFAULT_LIVE_EVENT_WAIT_MILLIS
}

fn default_live_event_batch_size() -> usize {
    DEFAULT_LIVE_EVENT_BATCH_SIZE
}

#[derive(Debug, Default)]
struct LiveEventFeed {
    state: Mutex<LiveEventFeedState>,
    changed: Condvar,
}

#[derive(Debug, Default)]
struct LiveEventFeedState {
    session_id: Option<String>,
    revision: u64,
    events: VecDeque<LiveEventLine>,
}

impl LiveEventFeed {
    fn reset(&self, session_id: String) {
        let mut state = self
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        state.session_id = Some(session_id);
        state.revision = state.revision.saturating_add(1);
        state.events.clear();
        self.changed.notify_all();
    }

    fn publish_batch(&self, mut events: Vec<LiveEventLine>) {
        if events.is_empty() {
            return;
        }
        let mut state = self
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        for event in &mut events {
            state.revision = state.revision.saturating_add(1);
            event.revision = state.revision;
        }
        state.events.extend(events);
        while state.events.len() > LIVE_EVENT_RING_CAPACITY {
            state.events.pop_front();
        }
        self.changed.notify_all();
    }

    fn wait_after(
        &self,
        after_revision: u64,
        timeout: Duration,
        limit: usize,
        tail: bool,
    ) -> LiveEventBatch {
        let deadline = Instant::now() + timeout;
        let mut state = self
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        while state.revision <= after_revision {
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                break;
            }
            state = match self.changed.wait_timeout(state, remaining) {
                Ok((state, _)) => state,
                Err(poisoned) => poisoned.into_inner().0,
            };
        }
        let received_new_event = state
            .events
            .back()
            .is_some_and(|event| event.revision > after_revision);
        if received_new_event {
            drop(state);
            thread::sleep(Duration::from_millis(LIVE_EVENT_COALESCE_MILLIS));
            state = self
                .state
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
        }
        let effective_after_revision = if tail {
            state.events.back().map_or(after_revision, |event| {
                event.revision.saturating_sub(limit as u64)
            })
        } else {
            after_revision
        };
        let events = state
            .events
            .iter()
            .filter(|event| event.revision > effective_after_revision)
            .take(limit)
            .cloned()
            .collect::<Vec<_>>();
        let dropped_before = events.first().map_or(0, |event| {
            event
                .revision
                .saturating_sub(after_revision.saturating_add(1))
        });
        let revision = events.last().map_or(state.revision, |event| event.revision);
        LiveEventBatch {
            schema_version: LIVE_EVENT_FEED_SCHEMA_VERSION,
            session_id: state.session_id.clone(),
            revision,
            dropped_before,
            has_more: state
                .events
                .back()
                .is_some_and(|event| event.revision > revision),
            events,
        }
    }
}

impl EventViewerEventView {
    fn from_envelope(canonical: EventEnvelope) -> Result<Self, String> {
        let (topic, timeline_sequence, kind, summary, amount, identifiers) =
            event_viewer_fields(&canonical);
        let canonical_json = serde_json::to_string(&canonical)
            .map_err(|error| format!("could not preserve canonical event JSON: {error}"))?;
        Ok(Self {
            sequence: canonical.sequence,
            timeline_sequence,
            observed_micros: canonical.time.observed_micros,
            game_time_millis: canonical.time.game_time_millis,
            topic,
            kind: kind.into(),
            summary,
            amount,
            identifiers,
            canonical_json,
        })
    }

    fn search_text(&self) -> String {
        let identifiers = serde_json::to_string(&self.identifiers).unwrap_or_default();
        format!(
            "{} {} {} {} {:?} {}",
            self.sequence,
            self.timeline_sequence.unwrap_or_default(),
            self.kind,
            self.summary,
            self.topic,
            identifiers
        )
        .to_lowercase()
    }
}

fn event_viewer_fields(
    canonical: &EventEnvelope,
) -> (
    EventTopic,
    Option<u64>,
    &'static str,
    String,
    Option<String>,
    EventViewerIdentifiersView,
) {
    let topic = canonical.event.topic();
    let mut identifiers = EventViewerIdentifiersView::default();
    let mut amount = None;
    let (timeline_sequence, kind) = match &canonical.event {
        CanonicalEvent::Timeline(timeline) => {
            let kind = timeline_event_view(&timeline.kind, &mut identifiers, &mut amount);
            (Some(timeline.sequence), kind)
        }
        CanonicalEvent::CharacterProfileObserved { profile } => {
            identifiers.character_id = Some(profile.character.character_id.clone());
            (None, "character_profile_observed")
        }
        CanonicalEvent::PartyRosterObserved(_) => (None, "party_roster_observed"),
        CanonicalEvent::PartyChanged { .. } => (None, "party_changed"),
        CanonicalEvent::WorldChanged(world) => {
            identifiers.scene = world.scene_id.map(|id| id.0.to_string());
            identifiers.map = world.map_id.map(|id| id.to_string());
            (None, "world_changed")
        }
        CanonicalEvent::Map(event) => {
            identifiers.actor = event.related_entity.map(EventViewerEntityView::from);
            identifiers.map = event.map_id.map(|id| id.0.to_string());
            (None, "map")
        }
        CanonicalEvent::Dungeon(event) => {
            identifiers.dungeon = event.dungeon_id.map(|id| id.0.to_string());
            (None, "dungeon")
        }
        CanonicalEvent::Chat(event) => {
            identifiers.actor = event.sender.map(EventViewerEntityView::from);
            identifiers.character_id = event
                .sender_character
                .as_ref()
                .map(|character| character.character_id.clone());
            (None, "chat")
        }
    };
    let summary = match &canonical.event {
        CanonicalEvent::Timeline(timeline) => match &timeline.kind {
            TimelineEventKind::DataGap(gap) => format!(
                "data_gap | {} | {}",
                data_gap_kind_label(gap.kind),
                gap.detail
            ),
            _ => event_viewer_summary(kind, &identifiers, amount.as_deref()),
        },
        _ => event_viewer_summary(kind, &identifiers, amount.as_deref()),
    };
    (topic, timeline_sequence, kind, summary, amount, identifiers)
}

fn data_gap_kind_label(kind: DataGapKind) -> &'static str {
    match kind {
        DataGapKind::CaptureDrop => "capture_drop",
        DataGapKind::TcpGap => "tcp_gap",
        DataGapKind::UnknownRoute => "unknown_route",
        DataGapKind::DecodeFailure => "decode_failure",
        DataGapKind::UnsupportedFragment => "unsupported_fragment",
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct EventViewerFileIdentity {
    path: PathBuf,
    length: u64,
    modified: SystemTime,
}

#[derive(Debug, Clone)]
struct VerifiedEventViewerArtifact {
    identity: EventViewerFileIdentity,
    session_id: String,
    digest: String,
    event_count: u64,
    header: RlogHeader,
    prevalidated: bool,
}

struct ActiveEventViewerQuery {
    id: String,
    artifact: VerifiedEventViewerArtifact,
    filter: EventViewerFilter,
    reader: RlogReader<BufReader<File>>,
    pending: Option<EventViewerEventView>,
    page_index: u64,
    scanned_total: u64,
    matched_total: u64,
    complete: bool,
}

#[derive(Default)]
struct EventViewerState {
    next_query_id: u64,
    verified: Option<VerifiedEventViewerArtifact>,
    active: Option<ActiveEventViewerQuery>,
}

fn normalize_event_viewer_filter(
    field: &str,
    value: Option<String>,
) -> Result<Option<String>, String> {
    let Some(value) = value else {
        return Ok(None);
    };
    let value = value.trim();
    if value.is_empty() {
        return Ok(None);
    }
    if value.len() > MAX_EVENT_VIEWER_FILTER_BYTES {
        return Err(format!(
            "event viewer {field} exceeds {MAX_EVENT_VIEWER_FILTER_BYTES} bytes"
        ));
    }
    Ok(Some(value.to_owned()))
}

fn timeline_event_view(
    event: &TimelineEventKind,
    identifiers: &mut EventViewerIdentifiersView,
    amount: &mut Option<String>,
) -> &'static str {
    match event {
        TimelineEventKind::RunBoundary { scene_id, .. } => {
            identifiers.scene = scene_id.map(|id| id.0.to_string());
            "run_boundary"
        }
        TimelineEventKind::EncounterBoundary { .. } => "encounter_boundary",
        TimelineEventKind::CombatBoundary { .. } => "combat_boundary",
        TimelineEventKind::Actor(event) => {
            identifiers.actor = Some(event.actor.into());
            identifiers.monster = event.monster_id.map(|id| id.0.to_string());
            "actor"
        }
        TimelineEventKind::EntityAttributes(event) => {
            identifiers.actor = Some(event.actor.into());
            "entity_attributes"
        }
        TimelineEventKind::TemporaryAttributes(event) => {
            identifiers.actor = Some(event.actor.into());
            "temporary_attributes"
        }
        TimelineEventKind::Cast(event) => {
            identifiers.source = Some(event.source.into());
            identifiers.target = event.target.map(EventViewerEntityView::from);
            identifiers.ability = Some(event.ability.0.to_string());
            "cast"
        }
        TimelineEventKind::Cooldown(event) => {
            identifiers.actor = Some(event.actor.into());
            identifiers.ability = Some(event.ability.0.to_string());
            "cooldown"
        }
        TimelineEventKind::Resource(event) => {
            identifiers.actor = Some(event.actor.into());
            *amount = Some(event.resource_values.len().to_string());
            "resource"
        }
        TimelineEventKind::Damage(event) => {
            identifiers.source = Some(event.source.into());
            identifiers.direct_source = event.direct_source.map(EventViewerEntityView::from);
            identifiers.target = Some(event.target.into());
            identifiers.ability = event.ability.map(|id| id.0.to_string());
            *amount = Some(event.amount.to_string());
            "damage"
        }
        TimelineEventKind::Healing(event) => {
            identifiers.source = Some(event.source.into());
            identifiers.direct_source = event.direct_source.map(EventViewerEntityView::from);
            identifiers.target = Some(event.target.into());
            identifiers.ability = event.ability.map(|id| id.0.to_string());
            *amount = Some(event.amount.to_string());
            "healing"
        }
        TimelineEventKind::Shield(event) => {
            identifiers.source = Some(event.source.into());
            identifiers.target = Some(event.target.into());
            identifiers.ability = Some(event.ability.0.to_string());
            *amount = Some(event.amount.to_string());
            "shield"
        }
        TimelineEventKind::Life { actor, .. } => {
            identifiers.actor = Some((*actor).into());
            "life"
        }
        TimelineEventKind::Status(event) => {
            identifiers.source = event.source.map(EventViewerEntityView::from);
            identifiers.target = Some(event.target.into());
            identifiers.status = Some(event.effect.0.to_string());
            identifiers.status_instance = event.instance_id.map(|id| id.0.to_string());
            identifiers.status_origin_type =
                event.origin.map(|origin| origin.source_type_id.to_string());
            identifiers.status_origin_config = event
                .origin
                .map(|origin| origin.source_config_id.to_string());
            identifiers.status_state = Some(
                match event.state {
                    rlogs_events::StatusState::Applied => "applied",
                    rlogs_events::StatusState::Refreshed => "refreshed",
                    rlogs_events::StatusState::Stacked => "stacked",
                    rlogs_events::StatusState::Consumed => "consumed",
                    rlogs_events::StatusState::Removed => "removed",
                }
                .into(),
            );
            identifiers.status_stacks = event.stacks.map(|value| value.to_string());
            identifiers.status_duration_millis =
                event.duration_millis.map(|value| value.to_string());
            "status"
        }
        TimelineEventKind::UnresolvedStatus(event) => {
            identifiers.source = event.source.map(EventViewerEntityView::from);
            identifiers.target = Some(event.target.into());
            identifiers.status_instance = event.instance_id.map(|value| value.0.to_string());
            identifiers.status_state = event.state.map(|state| {
                match state {
                    rlogs_events::StatusState::Applied => "applied",
                    rlogs_events::StatusState::Refreshed => "refreshed",
                    rlogs_events::StatusState::Stacked => "stacked",
                    rlogs_events::StatusState::Consumed => "consumed",
                    rlogs_events::StatusState::Removed => "removed",
                }
                .into()
            });
            "unresolved_status"
        }
        TimelineEventKind::UnresolvedAction(event) => {
            // The container is exact wire context, not a proven source.
            identifiers.actor = event.container.map(EventViewerEntityView::from);
            identifiers.target = event.target.map(EventViewerEntityView::from);
            "unresolved_action"
        }
        TimelineEventKind::Position(event) => {
            identifiers.actor = Some(event.actor.into());
            "position"
        }
        TimelineEventKind::RecorderPause(event) => {
            *amount = Some(
                event
                    .resumed_micros
                    .saturating_sub(event.started_micros)
                    .to_string(),
            );
            "recorder_pause"
        }
        TimelineEventKind::DataGap(_) => "data_gap",
    }
}

fn event_viewer_summary(
    kind: &str,
    identifiers: &EventViewerIdentifiersView,
    amount: Option<&str>,
) -> String {
    let mut parts = Vec::with_capacity(8);
    if let Some(source) = &identifiers.source {
        if let Some(target) = &identifiers.target {
            parts.push(format!(
                "{} -> {}",
                event_viewer_entity(source),
                event_viewer_entity(target)
            ));
        } else {
            parts.push(format!("source {}", event_viewer_entity(source)));
        }
    } else if let Some(actor) = &identifiers.actor {
        parts.push(event_viewer_entity(actor));
    } else if let Some(target) = &identifiers.target {
        parts.push(format!("target {}", event_viewer_entity(target)));
    }
    if let Some(direct_source) = &identifiers.direct_source {
        parts.push(format!("direct {}", event_viewer_entity(direct_source)));
    }
    if let Some(ability) = &identifiers.ability {
        parts.push(format!("ability:{ability}"));
    }
    if let Some(status) = &identifiers.status {
        parts.push(format!("status:{status}"));
    }
    if let Some(instance) = &identifiers.status_instance {
        parts.push(format!("instance:{instance}"));
    }
    if let (Some(source_type), Some(source_config)) = (
        &identifiers.status_origin_type,
        &identifiers.status_origin_config,
    ) {
        parts.push(format!("origin:{source_type}:{source_config}"));
    }
    if let Some(state) = &identifiers.status_state {
        parts.push(format!("state:{state}"));
    }
    if let Some(stacks) = &identifiers.status_stacks {
        parts.push(format!("stacks:{stacks}"));
    }
    if let Some(duration) = &identifiers.status_duration_millis {
        parts.push(format!("duration_ms:{duration}"));
    }
    if let Some(monster) = &identifiers.monster {
        parts.push(format!("monster:{monster}"));
    }
    if let Some(scene) = &identifiers.scene {
        parts.push(format!("scene:{scene}"));
    }
    if let Some(map) = &identifiers.map {
        parts.push(format!("map:{map}"));
    }
    if let Some(dungeon) = &identifiers.dungeon {
        parts.push(format!("dungeon:{dungeon}"));
    }
    if let Some(character_id) = &identifiers.character_id {
        parts.push(format!("character:{character_id}"));
    }
    if let Some(amount) = amount {
        parts.push(format!("amount:{amount}"));
    }
    if parts.is_empty() {
        kind.to_owned()
    } else {
        parts.join(" · ")
    }
}

fn event_viewer_entity(entity: &EventViewerEntityView) -> String {
    format!("entity:{} [actor:{}]", entity.entity_uuid, entity.actor_id)
}

impl EventViewerState {
    fn next_query_id(&mut self) -> Result<String, String> {
        self.next_query_id = self
            .next_query_id
            .checked_add(1)
            .ok_or_else(|| "event viewer query ID space is exhausted".to_owned())?;
        Ok(format!("events-{}", self.next_query_id))
    }

    fn verified_artifact(
        &mut self,
        result: &SessionResult,
    ) -> Result<VerifiedEventViewerArtifact, String> {
        let path = std::fs::canonicalize(&result.output_rlog)
            .map_err(|error| format!("could not resolve sealed rlog: {error}"))?;
        let identity = event_viewer_file_identity(path)?;
        let digest = &result.combat_plugin.rlog.content_sha256;
        if let Some(verified) = &self.verified
            && verified.identity == identity
            && verified.session_id == result.session_id
            && verified.digest == *digest
            && verified.event_count == result.canonical_event_count
        {
            return Ok(verified.clone());
        }

        let verified = verify_event_viewer_artifact(result, identity)?;
        self.active = None;
        self.verified = Some(verified.clone());
        Ok(verified)
    }
}

impl ActiveEventViewerQuery {
    fn open(
        id: String,
        artifact: VerifiedEventViewerArtifact,
        filter: EventViewerFilter,
    ) -> Result<Self, String> {
        let file = File::open(&artifact.identity.path)
            .map_err(|error| format!("could not open sealed rlog: {error}"))?;
        let reader = RlogReader::new(BufReader::new(file), RlogLimits::default())
            .map_err(|error| format!("could not start event replay: {error}"))?;
        if reader.header() != &artifact.header {
            return Err("sealed rlog header changed after verification".into());
        }
        Ok(Self {
            id,
            artifact,
            filter,
            reader,
            pending: None,
            page_index: 0,
            scanned_total: 0,
            matched_total: 0,
            complete: false,
        })
    }

    fn read_page(&mut self, page_size: usize) -> Result<EventViewerPage, String> {
        let current_identity = event_viewer_file_identity(self.artifact.identity.path.clone())?;
        if current_identity != self.artifact.identity {
            return Err("sealed rlog changed after Event Viewer verification".into());
        }

        let mut events = Vec::with_capacity(page_size);
        let mut response_bytes = 0_usize;
        let mut scanned_this_page = 0_u64;
        let scan_started = Instant::now();
        if let Some(pending) = self.pending.take() {
            push_event_viewer_row(&mut events, &mut response_bytes, pending)?;
        }

        while events.len() < page_size
            && scanned_this_page < MAX_EVENT_VIEWER_SCAN_PER_PAGE
            && (scanned_this_page == 0 || scan_started.elapsed() < MAX_EVENT_VIEWER_SCAN_TIME)
            && !self.complete
        {
            let Some(envelope) = self
                .reader
                .next_event()
                .map_err(|error| format!("event replay failed: {error}"))?
            else {
                let summary = self
                    .reader
                    .summary()
                    .ok_or_else(|| "event replay ended without an integrity summary".to_owned())?;
                if summary.content_sha256 != self.artifact.digest
                    || summary.event_count != self.artifact.event_count
                {
                    return Err("event replay no longer matches the verified artifact".into());
                }
                self.complete = true;
                break;
            };
            scanned_this_page += 1;
            self.scanned_total = self
                .scanned_total
                .checked_add(1)
                .ok_or_else(|| "event viewer scan counter is exhausted".to_owned())?;
            let event = EventViewerEventView::from_envelope(envelope)?;
            if !self.filter.matches(&event) {
                continue;
            }
            self.matched_total = self
                .matched_total
                .checked_add(1)
                .ok_or_else(|| "event viewer match counter is exhausted".to_owned())?;
            let encoded_bytes = event_viewer_row_bytes(&event)?;
            if !events.is_empty()
                && response_bytes.saturating_add(encoded_bytes) > MAX_EVENT_VIEWER_PAGE_BYTES
            {
                self.pending = Some(event);
                break;
            }
            if encoded_bytes > MAX_EVENT_VIEWER_PAGE_BYTES {
                return Err("one canonical event exceeds the Event Viewer response limit".into());
            }
            response_bytes = response_bytes.saturating_add(encoded_bytes);
            events.push(event);
        }

        self.page_index = self
            .page_index
            .checked_add(1)
            .ok_or_else(|| "event viewer page counter is exhausted".to_owned())?;
        Ok(EventViewerPage {
            schema_version: EVENT_VIEWER_SCHEMA_VERSION,
            query_id: self.id.clone(),
            session_id: self.artifact.session_id.clone(),
            artifact_digest: self.artifact.digest.clone(),
            header: self.artifact.header.clone(),
            filter: self.filter.clone(),
            page_index: self.page_index,
            scanned_this_page,
            scanned_total: self.scanned_total,
            matched_total: self.matched_total,
            integrity_verified: self.artifact.prevalidated || self.complete,
            complete: self.complete && self.pending.is_none(),
            events,
        })
    }
}

fn event_viewer_file_identity(path: PathBuf) -> Result<EventViewerFileIdentity, String> {
    let metadata = std::fs::metadata(&path)
        .map_err(|error| format!("could not inspect sealed rlog: {error}"))?;
    if !metadata.is_file() {
        return Err("the completed session rlog is not a file".into());
    }
    let modified = metadata
        .modified()
        .map_err(|error| format!("could not read sealed rlog modification time: {error}"))?;
    Ok(EventViewerFileIdentity {
        path,
        length: metadata.len(),
        modified,
    })
}

fn verify_event_viewer_artifact(
    result: &SessionResult,
    identity: EventViewerFileIdentity,
) -> Result<VerifiedEventViewerArtifact, String> {
    let file = File::open(&identity.path)
        .map_err(|error| format!("could not open sealed rlog for verification: {error}"))?;
    let reader = RlogReader::new(BufReader::new(file), RlogLimits::default())
        .map_err(|error| format!("sealed rlog header is invalid: {error}"))?;
    let header = reader.header().clone();
    if header.session_id != result.session_id {
        return Err(format!(
            "sealed rlog session {} does not match completed session {}",
            header.session_id, result.session_id
        ));
    }
    if event_viewer_file_identity(identity.path.clone())? != identity {
        return Err("sealed rlog changed while Event Viewer inspected its header".into());
    }
    Ok(VerifiedEventViewerArtifact {
        identity,
        session_id: result.session_id.clone(),
        digest: result.combat_plugin.rlog.content_sha256.clone(),
        event_count: result.canonical_event_count,
        header,
        prevalidated: result.upload_artifact.is_some(),
    })
}

fn event_viewer_row_bytes(event: &EventViewerEventView) -> Result<usize, String> {
    serde_json::to_vec(event)
        .map(|bytes| bytes.len())
        .map_err(|error| format!("could not encode canonical event: {error}"))
}

fn push_event_viewer_row(
    events: &mut Vec<EventViewerEventView>,
    response_bytes: &mut usize,
    event: EventViewerEventView,
) -> Result<(), String> {
    let encoded_bytes = event_viewer_row_bytes(&event)?;
    if encoded_bytes > MAX_EVENT_VIEWER_PAGE_BYTES {
        return Err("one canonical event exceeds the Event Viewer response limit".into());
    }
    *response_bytes = response_bytes.saturating_add(encoded_bytes);
    events.push(event);
    Ok(())
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
    log_output_directory: Option<String>,
    #[serde(default)]
    pack_path: Option<String>,
    #[serde(default)]
    region_id: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct SubmissionImportRequest {
    artifact_path: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct SubmissionVerificationRequest {
    queue_id: String,
}

#[derive(Debug, Clone, Serialize)]
struct SubmissionImportResult {
    schema_version: u16,
    outcome: &'static str,
    queue_id: String,
    capture_session_id: String,
    artifact: UploadArtifactView,
}

#[derive(Debug, Clone, Serialize)]
struct SubmissionVerificationResult {
    schema_version: u16,
    queue_id: String,
    capture_session_id: String,
    verified_unix_millis: u64,
    artifact: UploadArtifactView,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct MockSubmissionRequest {
    queue_id: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct SubmissionUploadRequest {
    queue_id: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct SubmissionConnectionUpdateRequest {
    endpoint_url: String,
    device_token: String,
}

#[derive(Debug, Clone, Serialize)]
struct MockSubmissionResult {
    schema_version: u16,
    queue_id: String,
    capture_session_id: String,
    report_id: String,
    final_state: SubmissionState,
    verification_tier: rlogs_submission::VerificationTier,
    chunk_count: usize,
    uploaded_bytes: u64,
    resumed_after_restart: bool,
    external_network_requests: u64,
    artifact: UploadArtifactView,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ProfilePackageInspectionRequest {
    package_id: String,
}

#[derive(Debug, Clone, Serialize)]
struct ProfileProjectionResult {
    schema_version: u16,
    source_session_id: String,
    projected_package_count: usize,
    stored_packages: Vec<ProfilePackageView>,
    external_network_requests: u64,
}

#[derive(Debug, Clone, Serialize)]
struct RuntimeEnvironment {
    platform: &'static str,
    game_processes: Vec<GameProcessView>,
    dumpcap_path: Option<String>,
    capture_interfaces: Vec<CaptureInterfaceView>,
    recommended_capture_interface: Option<String>,
    recommended_capture_source: Option<&'static str>,
    recommended_capture_reason: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct PluginCatalogView {
    schema_version: u16,
    installed_root: String,
    packages: Vec<InstalledPluginView>,
    issues: Vec<PluginIssueView>,
    workspaces: Vec<PluginWorkspaceView>,
    settings_tabs: Vec<PluginSettingsTabView>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct InstalledPluginView {
    id: String,
    name: String,
    version: String,
    folder_name: String,
    runtime: PluginRuntime,
    capabilities: Vec<PluginCapability>,
    subscriptions: Vec<rlogs_events::EventTopic>,
    allowed_network_domains: Vec<String>,
    dependencies: Vec<PluginDependencyView>,
    publishes_workspace: bool,
    enabled: bool,
    active: bool,
    status_detail: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct PluginDependencyView {
    plugin_id: String,
    optional: bool,
}

impl From<&PluginDependency> for PluginDependencyView {
    fn from(value: &PluginDependency) -> Self {
        Self {
            plugin_id: value.plugin_id.clone(),
            optional: value.optional,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct PluginIssueView {
    kind: &'static str,
    plugin_id: Option<String>,
    package_path: Option<String>,
    detail: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct PluginWorkspaceView {
    id: String,
    name: String,
    description: String,
    version: String,
    icon_url: Option<String>,
    icon_fallback: String,
    default_order: i32,
    tabs: Vec<PluginWorkspaceTabView>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct PluginWorkspaceTabView {
    id: String,
    label: String,
    kind: PluginWorkspaceTabKind,
    entrypoint: String,
    contributor_plugin_id: String,
    section_id: String,
    default_order: i32,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct PluginSettingsTabView {
    id: String,
    label: String,
    kind: PluginWorkspaceTabKind,
    entrypoint: String,
    contributor_plugin_id: String,
    section_id: String,
    default_order: i32,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct PluginEnablementRequest {
    plugin_id: String,
    enabled: bool,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct StoredPluginEnablement {
    schema_version: u16,
    #[serde(default)]
    enabled_plugin_ids: BTreeSet<String>,
    #[serde(default)]
    disabled_bundled_plugin_ids: BTreeSet<String>,
}

#[derive(Debug)]
struct DesktopPluginManager {
    installed_root: PathBuf,
    bundled_root: PathBuf,
    bundled_plugin_ids: BTreeSet<String>,
    state_path: PathBuf,
    report: PluginDiscoveryReport,
    enabled_plugin_ids: BTreeSet<String>,
    disabled_bundled_plugin_ids: BTreeSet<String>,
    state_issue: Option<String>,
}

#[derive(Debug)]
struct ActivePluginResolution {
    active_plugin_ids: BTreeSet<String>,
    blocked: BTreeMap<String, String>,
    workspaces: Vec<ResolvedPluginWorkspace>,
    settings_tabs: Vec<rlogs_plugin_host::ResolvedSettingsTab>,
}

impl DesktopPluginManager {
    fn new(install_root: &Path) -> Result<Self, String> {
        let installed_root = install_root.join("plugins/installed");
        let bundled_root = bundled_desktop_plugins_root(install_root);
        std::fs::create_dir_all(&installed_root).map_err(|error| {
            format!(
                "could not create installed plug-ins folder {}: {error}",
                display_path(&installed_root)
            )
        })?;
        let state_path = install_root.join("runtime-data/settings/plugin-enablement.v1.json");
        let (mut enabled_plugin_ids, disabled_bundled_plugin_ids, state_issue) =
            load_plugin_enablement(&state_path);
        let installed = discover_installed_plugins(&installed_root)
            .map_err(|error| format!("plug-in discovery failed: {error}"))?;
        let bundled = discover_plugin_packages(&bundled_root, install_root)
            .map_err(|error| format!("bundled plug-in discovery failed: {error}"))?;
        let bundled_plugin_ids = bundled
            .packages
            .iter()
            .map(|package| package.manifest().id.clone())
            .collect::<BTreeSet<_>>();
        enabled_plugin_ids.extend(
            bundled_plugin_ids
                .difference(&disabled_bundled_plugin_ids)
                .cloned(),
        );
        let report = merge_plugin_reports(bundled, installed);
        Ok(Self {
            installed_root,
            bundled_root,
            bundled_plugin_ids,
            state_path,
            report,
            enabled_plugin_ids,
            disabled_bundled_plugin_ids,
            state_issue,
        })
    }

    fn refresh(&mut self) -> Result<(), String> {
        let installed = discover_installed_plugins(&self.installed_root)
            .map_err(|error| format!("plug-in discovery failed: {error}"))?;
        let install_root = self
            .installed_root
            .parent()
            .and_then(Path::parent)
            .ok_or_else(|| "installed plug-in folder has no rLogs root".to_owned())?;
        let bundled = discover_plugin_packages(&self.bundled_root, install_root)
            .map_err(|error| format!("bundled plug-in discovery failed: {error}"))?;
        self.bundled_plugin_ids = bundled
            .packages
            .iter()
            .map(|package| package.manifest().id.clone())
            .collect();
        self.enabled_plugin_ids.extend(
            self.bundled_plugin_ids
                .difference(&self.disabled_bundled_plugin_ids)
                .cloned(),
        );
        self.report = merge_plugin_reports(bundled, installed);
        Ok(())
    }

    fn set_enabled(&mut self, plugin_id: &str, enabled: bool) -> Result<(), String> {
        if !self
            .report
            .packages
            .iter()
            .any(|package| package.manifest().id == plugin_id)
        {
            return Err(format!("installed plug-in {plugin_id} was not found"));
        }

        let mut candidate = self.enabled_plugin_ids.clone();
        let mut disabled_bundled = self.disabled_bundled_plugin_ids.clone();
        if enabled {
            candidate.insert(plugin_id.to_owned());
            disabled_bundled.remove(plugin_id);
            let resolution = resolve_active_plugins(&self.report.packages, &candidate);
            if !resolution.active_plugin_ids.contains(plugin_id) {
                let detail = resolution
                    .blocked
                    .get(plugin_id)
                    .cloned()
                    .unwrap_or_else(|| "the plug-in could not be activated".into());
                return Err(format!("cannot enable {plugin_id}: {detail}"));
            }
        } else {
            candidate.remove(plugin_id);
            if self.bundled_plugin_ids.contains(plugin_id) {
                disabled_bundled.insert(plugin_id.to_owned());
            }
        }

        save_plugin_enablement(&self.state_path, &candidate, &disabled_bundled)?;
        self.enabled_plugin_ids = candidate;
        self.disabled_bundled_plugin_ids = disabled_bundled;
        self.state_issue = None;
        Ok(())
    }

    fn snapshot(&self) -> PluginCatalogView {
        let resolution = resolve_active_plugins(&self.report.packages, &self.enabled_plugin_ids);
        let installed_ids = self
            .report
            .packages
            .iter()
            .map(|package| package.manifest().id.as_str())
            .collect::<BTreeSet<_>>();
        let mut issues = self
            .report
            .issues
            .iter()
            .map(|issue| PluginIssueView {
                kind: "invalid_package",
                plugin_id: None,
                package_path: Some(display_path(&issue.package_path)),
                detail: issue.detail.clone(),
            })
            .collect::<Vec<_>>();
        if let Some(detail) = &self.state_issue {
            issues.push(PluginIssueView {
                kind: "enablement_state",
                plugin_id: None,
                package_path: Some(display_path(&self.state_path)),
                detail: detail.clone(),
            });
        }
        for plugin_id in self
            .enabled_plugin_ids
            .iter()
            .filter(|plugin_id| !installed_ids.contains(plugin_id.as_str()))
        {
            issues.push(PluginIssueView {
                kind: "missing_enabled_package",
                plugin_id: Some(plugin_id.clone()),
                package_path: None,
                detail: "The plug-in remains enabled in settings but is not installed.".into(),
            });
        }
        for (plugin_id, detail) in &resolution.blocked {
            issues.push(PluginIssueView {
                kind: "blocked_plugin",
                plugin_id: Some(plugin_id.clone()),
                package_path: None,
                detail: detail.clone(),
            });
        }

        let packages = self
            .report
            .packages
            .iter()
            .map(|package| {
                let manifest = package.manifest();
                let enabled = self.enabled_plugin_ids.contains(&manifest.id);
                let active = resolution.active_plugin_ids.contains(&manifest.id);
                let status_detail = if active {
                    "Enabled and validated.".into()
                } else if enabled {
                    resolution
                        .blocked
                        .get(&manifest.id)
                        .cloned()
                        .unwrap_or_else(|| "Enabled but not active.".into())
                } else {
                    "Disabled by user.".into()
                };
                InstalledPluginView {
                    id: manifest.id.clone(),
                    name: manifest.name.clone(),
                    version: manifest.version.clone(),
                    folder_name: package.folder_name().to_owned(),
                    runtime: manifest.runtime,
                    capabilities: manifest.capabilities.iter().copied().collect(),
                    subscriptions: manifest.subscriptions.iter().copied().collect(),
                    allowed_network_domains: manifest.allowed_network_domains.clone(),
                    dependencies: manifest
                        .dependencies
                        .iter()
                        .map(PluginDependencyView::from)
                        .collect(),
                    publishes_workspace: manifest.workspace.is_some()
                        || !manifest.workspace_tab_contributions.is_empty()
                        || !manifest.settings_tab_contributions.is_empty(),
                    enabled,
                    active,
                    status_detail,
                }
            })
            .collect();
        let package_by_id = self
            .report
            .packages
            .iter()
            .map(|package| (package.manifest().id.as_str(), package))
            .collect::<BTreeMap<_, _>>();
        let workspaces = resolution
            .workspaces
            .into_iter()
            .filter_map(|workspace| {
                let owner = package_by_id.get(workspace.owner_plugin_id.as_str())?;
                let manifest = owner.manifest();
                let source_label = if self.bundled_plugin_ids.contains(&manifest.id) {
                    "Bundled"
                } else {
                    "Installed"
                };
                Some(PluginWorkspaceView {
                    id: workspace.owner_plugin_id,
                    name: workspace.name,
                    description: format!(
                        "{source_label} {} plug-in surface.",
                        runtime_label(manifest.runtime)
                    ),
                    version: manifest.version.clone(),
                    icon_url: None,
                    icon_fallback: icon_fallback(&manifest.name),
                    default_order: workspace.default_order,
                    tabs: workspace
                        .tabs
                        .into_iter()
                        .map(|tab| PluginWorkspaceTabView {
                            entrypoint: format!(
                                "{}://{}/{}",
                                if self.bundled_plugin_ids.contains(&tab.contributor_plugin_id) {
                                    "builtin"
                                } else {
                                    "installed"
                                },
                                tab.contributor_plugin_id,
                                tab.local_id
                            ),
                            id: tab.id,
                            label: tab.label,
                            kind: tab.kind,
                            section_id: tab.section_id,
                            default_order: tab.default_order,
                            contributor_plugin_id: tab.contributor_plugin_id,
                        })
                        .collect(),
                })
            })
            .collect();
        let settings_tabs = resolution
            .settings_tabs
            .into_iter()
            .map(|tab| PluginSettingsTabView {
                entrypoint: format!(
                    "{}://{}/{}",
                    if self.bundled_plugin_ids.contains(&tab.contributor_plugin_id) {
                        "builtin"
                    } else {
                        "installed"
                    },
                    tab.contributor_plugin_id,
                    tab.local_id
                ),
                id: tab.id,
                label: tab.label,
                kind: PluginWorkspaceTabKind::Options,
                contributor_plugin_id: tab.contributor_plugin_id,
                section_id: tab.section_id,
                default_order: tab.default_order,
            })
            .collect();

        PluginCatalogView {
            schema_version: PLUGIN_CATALOG_SCHEMA_VERSION,
            installed_root: display_path(&self.installed_root),
            packages,
            issues,
            workspaces,
            settings_tabs,
        }
    }

    fn active_native_launches(&self) -> Result<Vec<NativePluginLaunch>, String> {
        let resolution = resolve_active_plugins(&self.report.packages, &self.enabled_plugin_ids);
        self.report
            .packages
            .iter()
            .filter(|package| {
                resolution
                    .active_plugin_ids
                    .contains(&package.manifest().id)
                    && package.manifest().runtime == PluginRuntime::NativeDeveloper
            })
            .map(|package| {
                let manifest = package.manifest();
                let relative = manifest
                    .entrypoint
                    .as_deref()
                    .ok_or_else(|| format!("native plug-in {} has no entrypoint", manifest.id))?;
                let entrypoint =
                    std::fs::canonicalize(package.root().join(relative)).map_err(|error| {
                        format!(
                            "could not resolve native plug-in {} entrypoint: {error}",
                            manifest.id
                        )
                    })?;
                if !entrypoint.starts_with(package.root()) || !entrypoint.is_file() {
                    return Err(format!(
                        "native plug-in {} entrypoint escaped its validated package",
                        manifest.id
                    ));
                }
                Ok(NativePluginLaunch {
                    plugin_id: manifest.id.clone(),
                    package_root: package.root().to_owned(),
                    entrypoint,
                    data_root: package.root().join("private-data"),
                    asset_root: package.asset_root().to_owned(),
                    shared_asset_root: package.shared_asset_root().to_owned(),
                })
            })
            .collect()
    }

    fn active_surface_entrypoint(
        &self,
        plugin_id: &str,
        surface_id: &str,
    ) -> Result<PathBuf, String> {
        let resolution = resolve_active_plugins(&self.report.packages, &self.enabled_plugin_ids);
        if !resolution.active_plugin_ids.contains(plugin_id) {
            return Err(format!("plug-in {plugin_id} is not active"));
        }
        let package = self
            .report
            .packages
            .iter()
            .find(|package| package.manifest().id == plugin_id)
            .ok_or_else(|| format!("plug-in {plugin_id} was not found"))?;
        let manifest = package.manifest();
        let relative = manifest
            .workspace
            .iter()
            .flat_map(|workspace| workspace.tabs.iter())
            .find(|tab| tab.id == surface_id)
            .map(|tab| tab.entrypoint.as_str())
            .or_else(|| {
                manifest
                    .workspace_tab_contributions
                    .iter()
                    .find(|tab| tab.id == surface_id)
                    .map(|tab| tab.entrypoint.as_str())
            })
            .or_else(|| {
                manifest
                    .settings_tab_contributions
                    .iter()
                    .find(|tab| tab.id == surface_id)
                    .map(|tab| tab.entrypoint.as_str())
            })
            .ok_or_else(|| format!("plug-in {plugin_id} did not publish surface {surface_id}"))?;
        let path = std::fs::canonicalize(package.root().join(relative))
            .map_err(|error| format!("could not resolve plug-in surface: {error}"))?;
        if !path.starts_with(package.root()) || !path.is_file() {
            return Err("plug-in surface escaped its validated package".into());
        }
        let length = std::fs::metadata(&path)
            .map_err(|error| format!("could not inspect plug-in surface: {error}"))?
            .len();
        if length > MAX_PLUGIN_SURFACE_BYTES {
            return Err("plug-in surface exceeds the 4 MiB host limit".into());
        }
        Ok(path)
    }
}

fn bundled_desktop_plugins_root(install_root: &Path) -> PathBuf {
    let installed_layout = install_root.join("plugins/builtin/desktop");
    if installed_layout.is_dir() {
        return installed_layout;
    }
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../plugins/builtin/desktop")
}

fn merge_plugin_reports(
    mut bundled: PluginDiscoveryReport,
    mut installed: PluginDiscoveryReport,
) -> PluginDiscoveryReport {
    let mut package_ids = bundled
        .packages
        .iter()
        .map(|package| package.manifest().id.clone())
        .collect::<BTreeSet<_>>();
    for package in installed.packages.drain(..) {
        let plugin_id = package.manifest().id.clone();
        if package_ids.insert(plugin_id.clone()) {
            bundled.packages.push(package);
        } else {
            installed.issues.push(PluginDiscoveryIssue {
                package_path: package.root().to_owned(),
                detail: format!(
                    "installed plug-in ID {plugin_id} conflicts with a bundled package; the installed copy is disabled"
                ),
            });
        }
    }
    bundled.issues.append(&mut installed.issues);
    bundled
}

fn resolve_active_plugins(
    packages: &[PluginPackage],
    enabled_plugin_ids: &BTreeSet<String>,
) -> ActivePluginResolution {
    let mut active = packages
        .iter()
        .filter(|package| enabled_plugin_ids.contains(&package.manifest().id))
        .map(|package| (package.manifest().id.clone(), package.clone()))
        .collect::<BTreeMap<_, _>>();
    let mut blocked = BTreeMap::new();

    loop {
        let active_ids = active.keys().cloned().collect::<BTreeSet<_>>();
        let missing_dependencies = active
            .values()
            .filter_map(|package| {
                package
                    .manifest()
                    .dependencies
                    .iter()
                    .find(|dependency| {
                        !dependency.optional && !active_ids.contains(&dependency.plugin_id)
                    })
                    .map(|dependency| (package.manifest().id.clone(), dependency.plugin_id.clone()))
            })
            .collect::<Vec<_>>();
        if !missing_dependencies.is_empty() {
            for (plugin_id, dependency) in missing_dependencies {
                active.remove(&plugin_id);
                blocked.insert(
                    plugin_id,
                    format!("Requires disabled or unavailable plug-in {dependency}."),
                );
            }
            continue;
        }

        let active_packages = active.values().cloned().collect::<Vec<_>>();
        match resolve_plugin_load_order(&active_packages) {
            Ok(_) => {}
            Err(PluginOrderError::MissingRequiredDependency {
                plugin_id,
                dependency,
            }) => {
                active.remove(&plugin_id);
                blocked.insert(
                    plugin_id,
                    format!("Requires disabled or unavailable plug-in {dependency}."),
                );
                continue;
            }
            Err(PluginOrderError::DependencyCycle { plugin_ids }) => {
                let detail = format!("Dependency cycle: {}.", plugin_ids.join(", "));
                for plugin_id in plugin_ids {
                    active.remove(&plugin_id);
                    blocked.insert(plugin_id, detail.clone());
                }
                continue;
            }
            Err(PluginOrderError::HookCycle { .. }) => {
                unreachable!("load-order validation does not resolve operation hooks")
            }
        }

        match resolve_plugin_workspaces(&active_packages) {
            Ok(workspaces) => {
                let active_packages = active.values().cloned().collect::<Vec<_>>();
                return ActivePluginResolution {
                    active_plugin_ids: active.keys().cloned().collect(),
                    blocked,
                    workspaces,
                    settings_tabs: resolve_plugin_settings_tabs(&active_packages),
                };
            }
            Err(PluginWorkspaceError::TargetHasNoWorkspace {
                contributor_plugin_id,
                target_plugin_id,
            }) => {
                active.remove(&contributor_plugin_id);
                blocked.insert(
                    contributor_plugin_id,
                    format!("Contributed tab target {target_plugin_id} has no active workspace."),
                );
            }
        }
    }
}

fn load_plugin_enablement(path: &Path) -> (BTreeSet<String>, BTreeSet<String>, Option<String>) {
    let metadata = match std::fs::metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return (BTreeSet::new(), BTreeSet::new(), None);
        }
        Err(error) => {
            return (
                BTreeSet::new(),
                BTreeSet::new(),
                Some(format!(
                    "Could not read plug-in enablement settings: {error}"
                )),
            );
        }
    };
    if metadata.len() > MAX_PLUGIN_ENABLEMENT_BYTES {
        return (
            BTreeSet::new(),
            BTreeSet::new(),
            Some("Plug-in enablement settings exceed the 256 KiB safety limit.".into()),
        );
    }
    let result = std::fs::read(path)
        .map_err(|error| error.to_string())
        .and_then(|bytes| {
            serde_json::from_slice::<StoredPluginEnablement>(&bytes)
                .map_err(|error| error.to_string())
        });
    match result {
        Ok(state) if state.schema_version == PLUGIN_ENABLEMENT_SCHEMA_VERSION => (
            state.enabled_plugin_ids,
            state.disabled_bundled_plugin_ids,
            None,
        ),
        Ok(state) => (
            BTreeSet::new(),
            BTreeSet::new(),
            Some(format!(
                "Unsupported plug-in enablement schema {}; expected {}.",
                state.schema_version, PLUGIN_ENABLEMENT_SCHEMA_VERSION
            )),
        ),
        Err(error) => (
            BTreeSet::new(),
            BTreeSet::new(),
            Some(format!("Plug-in enablement settings are invalid: {error}")),
        ),
    }
}

fn save_plugin_enablement(
    path: &Path,
    enabled_plugin_ids: &BTreeSet<String>,
    disabled_bundled_plugin_ids: &BTreeSet<String>,
) -> Result<(), String> {
    let parent = path
        .parent()
        .ok_or_else(|| "plug-in enablement path has no parent".to_owned())?;
    std::fs::create_dir_all(parent)
        .map_err(|error| format!("could not create plug-in settings folder: {error}"))?;
    let state = StoredPluginEnablement {
        schema_version: PLUGIN_ENABLEMENT_SCHEMA_VERSION,
        enabled_plugin_ids: enabled_plugin_ids.clone(),
        disabled_bundled_plugin_ids: disabled_bundled_plugin_ids.clone(),
    };
    let mut bytes = serde_json::to_vec_pretty(&state)
        .map_err(|error| format!("could not encode plug-in enablement settings: {error}"))?;
    bytes.push(b'\n');
    std::fs::write(path, bytes)
        .map_err(|error| format!("could not write plug-in enablement settings: {error}"))
}

fn runtime_label(runtime: PluginRuntime) -> &'static str {
    match runtime {
        PluginRuntime::DataOnly => "data-only",
        PluginRuntime::WasmComponent => "WebAssembly component",
        PluginRuntime::BrowserOverlay => "browser",
        PluginRuntime::ExternalProcess => "external-process",
        PluginRuntime::NativeDeveloper => "native developer",
    }
}

fn icon_fallback(name: &str) -> String {
    let words = name
        .split_whitespace()
        .filter_map(|word| word.chars().next())
        .take(2)
        .collect::<String>();
    if words.chars().count() >= 2 {
        return words.to_uppercase();
    }
    name.chars().take(2).collect::<String>().to_uppercase()
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
    friendly_name: Option<String>,
    description: Option<String>,
    mac_address: Option<String>,
    is_up: Option<bool>,
    is_virtual: Option<bool>,
    recommendation: Option<&'static str>,
}

struct RuntimeController {
    install_root: PathBuf,
    state: Arc<Mutex<RuntimeSnapshot>>,
    plugins: Mutex<DesktopPluginManager>,
    native_plugin_processes: Mutex<NativePluginProcesses>,
    event_viewer: Mutex<EventViewerState>,
    submission_queue: Arc<Mutex<LocalSubmissionQueue>>,
    combat_history: Arc<Mutex<CombatHistoryStore>>,
    character_identities: Arc<Mutex<CharacterIdentityStore>>,
    profile_packages: Arc<Mutex<LocalProfilePackageStore>>,
    submission_policy: Mutex<SubmissionPolicyStore>,
    submission_connection: Mutex<SubmissionConnectionStore>,
    submission_transport: Mutex<Option<SubmissionTransport>>,
    core_settings: Mutex<CoreSettingsStore>,
    hotkey_settings: Mutex<HotkeySettingsStore>,
    layout_settings: Mutex<LayoutSettingsStore>,
    theme_settings: Mutex<ThemeSettingsStore>,
    combat_meter_settings: Mutex<CombatMeterSettingsStore>,
    combat_overlay_settings: Mutex<CombatOverlaySettingsStore>,
    artifact_verification: Mutex<()>,
    profile_projection: Mutex<()>,
    live_combat_feed: Arc<LiveCombatFeed>,
    live_event_feed: Arc<LiveEventFeed>,
    combat_history_feed: Arc<CombatHistoryRevisionFeed>,
    history_rdps_backfill: Arc<HistoryRdpsBackfillQueue>,
    #[cfg(windows)]
    live_stop: Arc<Mutex<Option<LiveCaptureStopHandle>>>,
    #[cfg(windows)]
    live_process_id: Arc<Mutex<Option<u32>>>,
}

impl RuntimeController {
    fn new(install_root: PathBuf) -> Result<Self, String> {
        let plugins = DesktopPluginManager::new(&install_root)?;
        let mut native_plugin_processes = NativePluginProcesses::default();
        native_plugin_processes.sync(plugins.active_native_launches()?)?;
        let submission_queue =
            LocalSubmissionQueue::open(install_root.join("runtime-data/submissions/queue"))?;
        let combat_history =
            CombatHistoryStore::open(install_root.join("runtime-data/history/combat-meter"))?;
        let character_identities = CharacterIdentityStore::open(
            install_root.join("runtime-data/identity/characters.v1.json"),
        )?;
        let profile_packages = LocalProfilePackageStore::open(
            install_root.join("runtime-data/profile-sync/packages"),
        )?;
        let submission_policy = SubmissionPolicyStore::open(
            install_root.join("runtime-data/settings/submission-policy.v1.json"),
        )?;
        let submission_connection = SubmissionConnectionStore::open(
            install_root.join("runtime-data/settings/submission-connection.v1.json"),
        )?;
        let submission_transport = match SubmissionTransport::from_environment()? {
            Some(transport) => Some(transport),
            None => match submission_connection.endpoint_url() {
                Some(endpoint) => Some(SubmissionTransport::new(
                    endpoint,
                    submission_connection.device_token()?.as_deref(),
                )?),
                None => None,
            },
        };
        let core_settings =
            CoreSettingsStore::open(install_root.join("runtime-data/settings/core.v1.json"))?;
        let hotkey_settings =
            HotkeySettingsStore::open(install_root.join("runtime-data/settings/hotkeys.v1.json"))?;
        let layout_settings =
            LayoutSettingsStore::open(install_root.join("runtime-data/settings/layout.v1.json"))?;
        let theme_settings = ThemeSettingsStore::open(
            install_root.join("runtime-data/settings/plugins/app.rlogs.themes.v1.json"),
        )?;
        let combat_meter_settings = CombatMeterSettingsStore::open(
            install_root.join("runtime-data/settings/plugins/app.rlogs.combat-meter.v1.json"),
        )?;
        let combat_overlay_settings = CombatOverlaySettingsStore::open(
            install_root.join("runtime-data/settings/plugins/app.rlogs.combat-overlay.v1.json"),
        )?;
        Ok(Self {
            install_root,
            state: Arc::new(Mutex::new(RuntimeSnapshot::default())),
            plugins: Mutex::new(plugins),
            native_plugin_processes: Mutex::new(native_plugin_processes),
            event_viewer: Mutex::new(EventViewerState::default()),
            submission_queue: Arc::new(Mutex::new(submission_queue)),
            combat_history: Arc::new(Mutex::new(combat_history)),
            character_identities: Arc::new(Mutex::new(character_identities)),
            profile_packages: Arc::new(Mutex::new(profile_packages)),
            submission_policy: Mutex::new(submission_policy),
            submission_connection: Mutex::new(submission_connection),
            submission_transport: Mutex::new(submission_transport),
            core_settings: Mutex::new(core_settings),
            hotkey_settings: Mutex::new(hotkey_settings),
            layout_settings: Mutex::new(layout_settings),
            theme_settings: Mutex::new(theme_settings),
            combat_meter_settings: Mutex::new(combat_meter_settings),
            combat_overlay_settings: Mutex::new(combat_overlay_settings),
            artifact_verification: Mutex::new(()),
            profile_projection: Mutex::new(()),
            live_combat_feed: Arc::new(LiveCombatFeed::default()),
            live_event_feed: Arc::new(LiveEventFeed::default()),
            combat_history_feed: Arc::new(CombatHistoryRevisionFeed::default()),
            history_rdps_backfill: Arc::new(HistoryRdpsBackfillQueue::default()),
            #[cfg(windows)]
            live_stop: Arc::new(Mutex::new(None)),
            #[cfg(windows)]
            live_process_id: Arc::new(Mutex::new(None)),
        })
    }

    fn start_automatic_submission_uploader(
        self: &Arc<Self>,
        shutdown: Option<Arc<AtomicBool>>,
    ) -> Result<(), String> {
        let controller = Arc::clone(self);
        thread::Builder::new()
            .name("rlogs-submission-uploader".into())
            .spawn(move || loop {
                if shutdown
                    .as_ref()
                    .is_some_and(|shutdown| shutdown.load(Ordering::Acquire))
                {
                    return;
                }
                let automatic = controller
                    .submission_policy
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner)
                    .policy()
                    .log_uploader
                    .automatic_combat_logs;
                let enabled = controller
                    .submission_policy
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner)
                    .policy()
                    .log_uploader
                    .enabled;
                let connected = controller
                    .submission_transport
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner)
                    .is_some();
                if enabled && automatic && connected {
                    let next = controller
                        .submission_queue()
                        .entries
                        .into_iter()
                        .find(|entry| {
                            entry.state == SubmissionState::Draft
                                && entry.artifact_exists
                                && entry.artifact_byte_length_matches
                        });
                    if let Some(entry) = next {
                        match controller.upload_queued_submission(SubmissionUploadRequest {
                            queue_id: entry.queue_id,
                        }) {
                            Ok(_) => continue,
                            Err(error)
                                if error.contains("another full artifact import")
                                    || error.contains("was not found") => {}
                            Err(error) => eprintln!(
                                "automatic research submission will retry after an error: {error}"
                            ),
                        }
                    }
                }
                for _ in 0..120 {
                    if shutdown
                        .as_ref()
                        .is_some_and(|shutdown| shutdown.load(Ordering::Acquire))
                    {
                        return;
                    }
                    thread::sleep(Duration::from_millis(250));
                }
            })
            .map(|_| ())
            .map_err(|error| format!("could not start automatic submission uploader: {error}"))
    }

    fn start_history_rdps_backfill_worker(
        self: &Arc<Self>,
        shutdown: Option<Arc<AtomicBool>>,
    ) -> Result<(), String> {
        // Do not scan or replay archived sessions at startup. A stale session
        // is queued only when the user explicitly opens it; the refreshed
        // projection is then persisted into that history artifact.
        let controller = Arc::clone(self);
        thread::Builder::new()
            .name("rlogs-history-rdps".into())
            .spawn(move || {
                while let Some(session_id) =
                    controller.history_rdps_backfill.next(shutdown.as_deref())
                {
                    if controller.snapshot().phase == RuntimePhase::Processing {
                        controller.history_rdps_backfill.update_progress(
                            HistoryRdpsRefreshProgress {
                                session_id: session_id.clone(),
                                stage: HistoryRdpsRefreshStage::WaitingForLiveCapture,
                                processed_events: 0,
                                processed_bytes: 0,
                                total_bytes: 0,
                                detail: None,
                            },
                        );
                        controller.combat_history_feed.publish_progress();
                        controller.history_rdps_backfill.requeue(session_id);
                        thread::sleep(Duration::from_millis(250));
                        continue;
                    }
                    match controller.refresh_history_rdps(&session_id, shutdown.as_deref()) {
                        Ok(HistoryRdpsRefresh::Current) => {
                            controller.history_rdps_backfill.finish(&session_id);
                            controller.combat_history_feed.publish_progress();
                        }
                        Ok(HistoryRdpsRefresh::Refreshed) => {
                            controller.history_rdps_backfill.finish(&session_id);
                            controller.combat_history_feed.publish();
                        }
                        Ok(HistoryRdpsRefresh::Deferred) => {
                            controller.history_rdps_backfill.update_progress(
                                HistoryRdpsRefreshProgress {
                                    session_id: session_id.clone(),
                                    stage: HistoryRdpsRefreshStage::WaitingForLiveCapture,
                                    processed_events: 0,
                                    processed_bytes: 0,
                                    total_bytes: 0,
                                    detail: None,
                                },
                            );
                            controller.combat_history_feed.publish_progress();
                            controller.history_rdps_backfill.requeue(session_id);
                            thread::sleep(Duration::from_millis(250));
                        }
                        Err(error) => {
                            controller
                                .history_rdps_backfill
                                .fail(&session_id, bounded_history_rdps_failure_detail(&error));
                            controller.combat_history_feed.publish_progress();
                            eprintln!(
                                "could not refresh derived rDPS for history {session_id}: {error}"
                            );
                        }
                    }
                }
            })
            .map(|_| ())
            .map_err(|error| format!("could not start history rDPS worker: {error}"))
    }

    fn refresh_history_rdps(
        &self,
        session_id: &str,
        shutdown: Option<&AtomicBool>,
    ) -> Result<HistoryRdpsRefresh, String> {
        let snapshot = self
            .combat_history
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .detail(session_id)?;
        if !state_damage_contribution_target_matches(
            &snapshot.deployment_id,
            &snapshot.client_build,
            &snapshot.protocol_pack_digest,
        )? || snapshot.rdps_formula_identity.as_deref()
            == Some(state_damage_contribution_formula_identity())
        {
            return Ok(HistoryRdpsRefresh::Current);
        }

        let raw_log = self
            .install_root
            .join("runtime-data/logs")
            .join(format!("{session_id}.rlog"));
        let total_bytes = raw_log
            .metadata()
            .map(|metadata| metadata.len())
            .unwrap_or(0);
        self.history_rdps_backfill
            .update_progress(HistoryRdpsRefreshProgress {
                session_id: session_id.to_owned(),
                stage: HistoryRdpsRefreshStage::Replaying,
                processed_events: 0,
                processed_bytes: 0,
                total_bytes,
                detail: None,
            });
        self.combat_history_feed.publish_progress();
        let state = Arc::clone(&self.state);
        let mut last_progress_publish = Instant::now();
        let mut last_percent = 0_u64;
        let mut final_processed_events = 0_u64;
        let mut final_processed_bytes = 0_u64;
        let projection = replay_bpsr_combat_history_interruptible(
            &raw_log,
            || {
                shutdown.is_some_and(|shutdown| shutdown.load(Ordering::Acquire))
                    || state
                        .lock()
                        .unwrap_or_else(std::sync::PoisonError::into_inner)
                        .phase
                        == RuntimePhase::Processing
            },
            |processed_events, processed_bytes, total_bytes| {
                final_processed_events = processed_events;
                final_processed_bytes = processed_bytes;
                let percent = processed_bytes
                    .saturating_mul(100)
                    .checked_div(total_bytes)
                    .unwrap_or(0);
                if percent <= last_percent
                    && last_progress_publish.elapsed() < Duration::from_millis(250)
                {
                    return;
                }
                last_percent = percent;
                last_progress_publish = Instant::now();
                self.history_rdps_backfill
                    .update_progress(HistoryRdpsRefreshProgress {
                        session_id: session_id.to_owned(),
                        stage: HistoryRdpsRefreshStage::Replaying,
                        processed_events,
                        processed_bytes,
                        total_bytes,
                        detail: None,
                    });
                self.combat_history_feed.publish_progress();
            },
        )?;
        let Some(projection) = projection else {
            return Ok(HistoryRdpsRefresh::Deferred);
        };
        if shutdown.is_some_and(|shutdown| shutdown.load(Ordering::Acquire))
            || self.snapshot().phase == RuntimePhase::Processing
        {
            return Ok(HistoryRdpsRefresh::Deferred);
        }
        self.history_rdps_backfill
            .update_progress(HistoryRdpsRefreshProgress {
                session_id: session_id.to_owned(),
                stage: HistoryRdpsRefreshStage::ValidatingAndSaving,
                processed_events: final_processed_events,
                processed_bytes: final_processed_bytes,
                total_bytes,
                detail: None,
            });
        self.combat_history_feed.publish_progress();
        self.combat_history
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .refresh_rdps_projection(&projection)?;
        Ok(HistoryRdpsRefresh::Refreshed)
    }

    #[cfg(windows)]
    fn start_automatic_monitor(
        self: &Arc<Self>,
        shutdown: Option<Arc<AtomicBool>>,
    ) -> Result<(), String> {
        let controller = Arc::clone(self);
        thread::Builder::new()
            .name("rlogs-auto-monitor".into())
            .spawn(move || {
                loop {
                    if shutdown
                        .as_ref()
                        .is_some_and(|shutdown| shutdown.load(Ordering::Acquire))
                    {
                        if controller
                            .live_stop
                            .lock()
                            .unwrap_or_else(std::sync::PoisonError::into_inner)
                            .is_some()
                        {
                            let _ = controller.stop_live();
                        }
                        return;
                    }

                    let processes = discover_game_processes().unwrap_or_default();
                    let active_process = *controller
                        .live_process_id
                        .lock()
                        .unwrap_or_else(std::sync::PoisonError::into_inner);
                    if let Some(process_id) = active_process {
                        if !processes
                            .iter()
                            .any(|process| process.process_id == process_id)
                        {
                            let _ = controller.stop_live();
                        }
                    } else if controller.snapshot().phase != RuntimePhase::Processing
                        && let Some(process) = processes.first()
                    {
                        let settings = controller.core_settings();
                        let dumpcap_path = settings
                            .dumpcap_path
                            .map(PathBuf::from)
                            .or_else(default_dumpcap_path);
                        if let (Some(interface), Some(dumpcap_path)) =
                            (settings.capture_interface, dumpcap_path)
                            && dumpcap_path.is_file()
                        {
                            let request = LiveSessionRequest {
                                session_id: format!("monitor-{}", unix_millis()),
                                process_id: process.process_id,
                                interface,
                                dumpcap_path: display_path(&dumpcap_path),
                                // Zero removes dumpcap's wall-clock deadline.
                                // Process exit or host shutdown stops ingress.
                                duration_seconds: 0,
                                log_output_directory: None,
                                pack_path: None,
                                region_id: None,
                            };
                            if let Err(error) = controller.start_live(request) {
                                let mut state = controller
                                    .state
                                    .lock()
                                    .unwrap_or_else(std::sync::PoisonError::into_inner);
                                state.detail =
                                    format!("Automatic packet monitoring will retry: {error}");
                            }
                        }
                    }
                    thread::sleep(Duration::from_secs(1));
                }
            })
            .map(|_| ())
            .map_err(|error| format!("could not start automatic packet monitor: {error}"))
    }

    #[cfg(not(windows))]
    fn start_automatic_monitor(self: &Arc<Self>, _: Option<Arc<AtomicBool>>) -> Result<(), String> {
        Ok(())
    }

    fn snapshot(&self) -> RuntimeSnapshot {
        self.state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone()
    }

    fn live_combat_snapshot(&self) -> LiveCombatUpdate {
        self.live_combat_feed.current()
    }

    fn wait_for_live_combat(&self, request: LiveCombatWaitRequest) -> LiveCombatUpdate {
        let timeout = Duration::from_millis(
            request
                .timeout_millis
                .clamp(1, MAXIMUM_LIVE_COMBAT_WAIT_MILLIS),
        );
        self.live_combat_feed
            .wait_after(request.after_revision, timeout)
    }

    fn wait_for_live_events(&self, request: LiveEventWaitRequest) -> LiveEventBatch {
        let timeout = Duration::from_millis(
            request
                .timeout_millis
                .clamp(1, MAXIMUM_LIVE_EVENT_WAIT_MILLIS),
        );
        let limit = request.limit.clamp(1, MAXIMUM_LIVE_EVENT_BATCH_SIZE);
        self.live_event_feed
            .wait_after(request.after_revision, timeout, limit, request.tail)
    }

    fn combat_history_catalog(&self) -> Result<CombatHistoryCatalog, String> {
        let mut catalog = self
            .combat_history
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .catalog();
        enrich_bpsr_catalog_public_names(
            &mut catalog,
            &*self
                .character_identities
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner),
        )?;
        enrich_bpsr_catalog_presentation(&mut catalog, "en-US")?;
        Ok(catalog)
    }

    fn wait_for_combat_history(
        &self,
        request: CombatHistoryWaitRequest,
    ) -> CombatHistoryRevisionUpdate {
        let timeout = Duration::from_millis(
            request
                .timeout_millis
                .clamp(1, MAXIMUM_COMBAT_HISTORY_WAIT_MILLIS),
        );
        let (revision, catalog_changed) = self
            .combat_history_feed
            .wait_after(request.after_revision, timeout);
        CombatHistoryRevisionUpdate {
            schema_version: COMBAT_HISTORY_FEED_SCHEMA_VERSION,
            revision,
            catalog_changed,
            rdps_refreshes: self.history_rdps_backfill.progress_snapshot(),
        }
    }

    fn combat_history_detail(
        &self,
        request: CombatHistoryDetailRequest,
    ) -> Result<CombatHistorySnapshot, String> {
        let mut snapshot = self
            .combat_history
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .detail(&request.session_id)?;
        let formula_identity = state_damage_contribution_formula_identity();
        let rdps_target_matches = state_damage_contribution_target_matches(
            &snapshot.deployment_id,
            &snapshot.client_build,
            &snapshot.protocol_pack_digest,
        )?;
        if !rdps_target_matches {
            clear_history_rdps_projection(
                &mut snapshot,
                "formula_runtime_blocked: exact-build promotion proof gates are incomplete",
            );
        } else if snapshot.rdps_formula_identity.as_deref() != Some(formula_identity) {
            if self
                .history_rdps_backfill
                .enqueue(snapshot.session_id.clone())
            {
                self.combat_history_feed.publish_progress();
            }
            mark_history_rdps_projection_refreshing(
                &mut snapshot,
                "formula_refresh_queued: recalculating archived rDPS in the background",
            );
        }
        // A public character name may be learned before or after the saved
        // run. Resolve that label by exact UID, but never borrow the current
        // class, specialization, scores, weapon, or loadout into history.
        enrich_bpsr_history_public_names(
            &mut snapshot,
            &*self
                .character_identities
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner),
        )?;
        enrich_bpsr_history_presentation(&mut snapshot, "en-US")?;
        Ok(snapshot)
    }

    fn set_combat_history_favorite(
        &self,
        request: CombatHistoryFavoriteRequest,
    ) -> Result<CombatHistoryCatalog, String> {
        let mut catalog = {
            let mut store = self
                .combat_history
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            store.set_favorite(&request.history_id, request.is_favorite)?
        };
        enrich_bpsr_catalog_public_names(
            &mut catalog,
            &*self
                .character_identities
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner),
        )?;
        enrich_bpsr_catalog_presentation(&mut catalog, "en-US")?;
        self.combat_history_feed.publish();
        Ok(catalog)
    }

    fn delete_combat_history(
        &self,
        request: CombatHistoryDeleteRequest,
    ) -> Result<CombatHistoryDeleteResult, String> {
        let result = self
            .combat_history
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .delete_entries(&request.history_ids)?;
        if result.deleted_count > 0 {
            self.combat_history_feed.publish();
        }
        Ok(result)
    }

    fn core_settings(&self) -> CoreSettings {
        self.core_settings
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .snapshot()
    }

    fn update_core_settings(&self, settings: CoreSettings) -> Result<CoreSettings, String> {
        self.core_settings
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .update(settings)
    }

    fn hotkey_settings(&self) -> HotkeySettingsView {
        self.hotkey_settings
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .snapshot()
    }

    fn assign_hotkey(
        &self,
        request: HotkeyAssignmentRequest,
    ) -> Result<HotkeyAssignmentResult, String> {
        self.hotkey_settings
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .assign(request)
    }

    fn restore_hotkey_bindings(
        &self,
        bindings: BTreeMap<String, String>,
    ) -> Result<HotkeySettingsView, String> {
        self.hotkey_settings
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .restore_bindings(bindings)
    }

    fn layout_settings(&self) -> LayoutSettings {
        self.layout_settings
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .snapshot()
    }

    fn update_layout_settings(&self, settings: LayoutSettings) -> Result<LayoutSettings, String> {
        self.layout_settings
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .update(settings)
    }

    fn theme_settings(&self) -> ThemeSettings {
        self.theme_settings
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .snapshot()
    }

    fn update_theme_settings(&self, settings: ThemeSettings) -> Result<ThemeSettings, String> {
        self.theme_settings
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .update(settings)
    }

    fn combat_meter_settings(&self) -> CombatMeterSettings {
        self.combat_meter_settings
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .snapshot()
    }

    fn update_combat_meter_settings(
        &self,
        settings: CombatMeterSettings,
    ) -> Result<CombatMeterSettings, String> {
        self.combat_meter_settings
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .update(settings)
    }

    fn combat_overlay_settings(&self) -> CombatOverlaySettings {
        self.combat_overlay_settings
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .snapshot()
    }

    fn update_combat_overlay_settings(
        &self,
        settings: CombatOverlaySettings,
    ) -> Result<CombatOverlaySettings, String> {
        self.combat_overlay_settings
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .update(settings)
    }

    fn save_combat_overlay_background(&self, bytes: &[u8]) -> Result<u64, String> {
        let (extension, _) = combat_overlay_image_format(bytes)?;
        let folder = self
            .install_root
            .join("assets/app.rlogs.combat-overlay/backgrounds");
        std::fs::create_dir_all(&folder).map_err(|error| {
            format!("could not create Combat Overlay background folder: {error}")
        })?;
        let target = folder.join(format!("custom-background.{extension}"));
        std::fs::write(&target, bytes)
            .map_err(|error| format!("could not save Combat Overlay background: {error}"))?;
        for other_extension in ["png", "jpg", "webp", "gif"] {
            if other_extension == extension {
                continue;
            }
            let other = folder.join(format!("custom-background.{other_extension}"));
            match std::fs::remove_file(other) {
                Ok(()) => {}
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Err(error) => {
                    return Err(format!(
                        "could not replace the previous Combat Overlay background: {error}"
                    ));
                }
            }
        }
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|duration| duration.as_millis().min(u64::MAX as u128) as u64)
            .map_err(|error| format!("could not timestamp Combat Overlay background: {error}"))
    }

    fn combat_overlay_background(&self) -> Result<(&'static str, Vec<u8>), String> {
        let folder = self
            .install_root
            .join("assets/app.rlogs.combat-overlay/backgrounds");
        for (extension, content_type) in [
            ("png", "image/png"),
            ("jpg", "image/jpeg"),
            ("webp", "image/webp"),
            ("gif", "image/gif"),
        ] {
            let path = folder.join(format!("custom-background.{extension}"));
            if path.is_file() {
                let bytes = std::fs::read(path).map_err(|error| {
                    format!("could not read Combat Overlay background: {error}")
                })?;
                return Ok((content_type, bytes));
            }
        }
        Err("Combat Overlay custom background has not been uploaded".into())
    }

    fn submission_queue(&self) -> SubmissionQueueView {
        self.submission_queue
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .snapshot()
    }

    fn refresh_submission_queue(&self) -> Result<SubmissionQueueView, String> {
        let mut queue = self
            .submission_queue
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        queue.reload()?;
        Ok(queue.snapshot())
    }

    fn profile_packages(&self) -> ProfilePackageStoreView {
        self.profile_packages
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .snapshot()
    }

    fn refresh_profile_packages(&self) -> Result<ProfilePackageStoreView, String> {
        let mut packages = self
            .profile_packages
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        packages.reload()?;
        Ok(packages.snapshot())
    }

    fn inspect_profile_package(
        &self,
        request: ProfilePackageInspectionRequest,
    ) -> Result<ProfilePackageInspection, String> {
        self.profile_packages
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .inspect(&request.package_id)
    }

    fn project_last_profile_packages(&self) -> Result<ProfileProjectionResult, String> {
        let policy = self
            .submission_policy
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .policy()
            .bpsr_profile_sync
            .clone();
        if !policy.enabled {
            return Err(
                "BPSR Profile Sync is disabled; enable it before building a package".into(),
            );
        }
        let _projection = self
            .profile_projection
            .try_lock()
            .map_err(|_| "another profile package projection is already running".to_owned())?;
        let result = self
            .snapshot()
            .last_result
            .ok_or_else(|| "no completed canonical log is available".to_owned())?;
        let (views, status) = project_completed_profile_session(
            &self.profile_packages,
            &result.output_rlog,
            unix_millis(),
        )?;
        {
            let mut state = self
                .state
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            if let Some(last) = state
                .last_result
                .as_mut()
                .filter(|last| last.session_id == result.session_id)
            {
                last.profile_package_count = views.len();
                last.profile_sync_status = status;
            }
        }
        Ok(ProfileProjectionResult {
            schema_version: 1,
            source_session_id: result.session_id,
            projected_package_count: views.len(),
            stored_packages: views,
            external_network_requests: 0,
        })
    }

    fn submission_policy(&self) -> SubmissionPolicyView {
        let mut view = self
            .submission_policy
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .snapshot();
        if let Some(transport) = self
            .submission_transport
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .as_ref()
        {
            view.transport_mode = "http";
            view.endpoint_url = Some(transport.endpoint_url());
        }
        view
    }

    fn submission_connection(&self) -> Result<SubmissionConnectionView, String> {
        self.submission_connection
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .view()
    }

    fn update_submission_connection(
        &self,
        request: SubmissionConnectionUpdateRequest,
    ) -> Result<SubmissionConnectionView, String> {
        let transport =
            SubmissionTransport::new(&request.endpoint_url, Some(request.device_token.as_str()))?;
        let view = self
            .submission_connection
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .update(request.endpoint_url, request.device_token)?;
        *self
            .submission_transport
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(transport);
        Ok(view)
    }

    fn disconnect_submission_connection(&self) -> Result<SubmissionConnectionView, String> {
        let view = self
            .submission_connection
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .disconnect()?;
        *self
            .submission_transport
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = None;
        Ok(view)
    }

    fn update_submission_policy(
        &self,
        policy: SubmissionPolicy,
    ) -> Result<SubmissionPolicyView, String> {
        self.submission_policy
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .update(policy)?;
        Ok(self.submission_policy())
    }

    fn import_submission_artifact(
        &self,
        request: SubmissionImportRequest,
    ) -> Result<SubmissionImportResult, String> {
        let _verification = self.artifact_verification.try_lock().map_err(|_| {
            "another full artifact import or re-verification is already running".to_owned()
        })?;
        validate_local_artifact_path(&request.artifact_path)?;
        let path = existing_file(&request.artifact_path, "sealed rlog")?;
        if !path
            .extension()
            .and_then(|extension| extension.to_str())
            .is_some_and(|extension| extension.eq_ignore_ascii_case("rlog"))
        {
            return Err("submission import requires a .rlog file".into());
        }
        let artifact = build_upload_artifact(&path)?;
        let visibility = self
            .submission_policy
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .policy()
            .log_uploader
            .default_visibility;
        let (outcome, queue_id) = enqueue_verified_artifact(
            &self.submission_queue,
            &artifact,
            display_path(&path),
            unix_millis(),
            visibility,
        )?;
        Ok(SubmissionImportResult {
            schema_version: 1,
            outcome: outcome.label(),
            queue_id,
            capture_session_id: artifact.header.session_id.clone(),
            artifact: UploadArtifactView::from(&artifact),
        })
    }

    fn verify_queued_submission(
        &self,
        request: SubmissionVerificationRequest,
    ) -> Result<SubmissionVerificationResult, String> {
        let _verification = self.artifact_verification.try_lock().map_err(|_| {
            "another full artifact import or re-verification is already running".to_owned()
        })?;
        let queue_id = Sha256Digest::parse(request.queue_id)
            .map_err(|error| format!("queue ID is invalid: {error}"))?;
        let entry = self
            .submission_queue
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .entry(queue_id.as_str())
            .ok_or_else(|| format!("submission draft {} was not found", queue_id.as_str()))?;
        let path = existing_file(&entry.local_artifact_path, "queued sealed rlog")?;
        let artifact = build_upload_artifact(&path)?;
        entry
            .verify_artifact(&artifact)
            .map_err(|error| format!("queued artifact re-verification failed: {error}"))?;
        Ok(SubmissionVerificationResult {
            schema_version: 1,
            queue_id: queue_id.to_string(),
            capture_session_id: entry.capture_session_id().to_owned(),
            verified_unix_millis: unix_millis(),
            artifact: UploadArtifactView::from(&artifact),
        })
    }

    fn run_mock_submission(
        &self,
        request: MockSubmissionRequest,
    ) -> Result<MockSubmissionResult, String> {
        let policy = self
            .submission_policy
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .policy()
            .clone();
        if !policy.log_uploader.enabled {
            return Err("Log Uploader is disabled; enable it before running a dry run".into());
        }
        let _verification = self.artifact_verification.try_lock().map_err(|_| {
            "another full artifact import, re-verification, or dry run is already running"
                .to_owned()
        })?;
        let queue_id = Sha256Digest::parse(request.queue_id)
            .map_err(|error| format!("queue ID is invalid: {error}"))?;
        let entry = self
            .submission_queue
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .entry(queue_id.as_str())
            .ok_or_else(|| format!("submission draft {} was not found", queue_id.as_str()))?;
        let path = existing_file(&entry.local_artifact_path, "queued sealed rlog")?;
        let artifact = build_upload_artifact(&path)?;
        entry
            .verify_artifact(&artifact)
            .map_err(|error| format!("mock preflight verification failed: {error}"))?;

        let mut session = entry.session.clone();
        session
            .start_upload()
            .map_err(|error| format!("mock upload could not start: {error}"))?;
        let mut receiver = MockSubmissionReceiver::begin(session.manifest())
            .map_err(|error| format!("mock receiver rejected the manifest: {error}"))?;
        let mut file = File::open(&path)
            .map_err(|error| format!("could not reopen verified artifact: {error}"))?;
        let restart_after = artifact.chunks.len().div_ceil(2);
        let mut transmitted = 0_usize;
        let mut resumed_after_restart = false;

        while let Some(chunk) = session
            .pending_chunks(1)
            .map_err(|error| format!("mock upload could not read pending chunks: {error}"))?
            .first()
            .cloned()
            .cloned()
        {
            let bytes = read_artifact_chunk(&mut file, &chunk)?;
            let acknowledgement = receiver
                .receive_chunk(chunk.sequence, &bytes)
                .map_err(|error| format!("mock receiver rejected chunk: {error}"))?;
            session
                .acknowledge_chunk(acknowledgement.sequence, &acknowledgement.sha256)
                .map_err(|error| format!("mock acknowledgement was rejected: {error}"))?;
            transmitted += 1;

            if !resumed_after_restart && transmitted >= restart_after {
                session =
                    serde_json::from_slice(&serde_json::to_vec(&session).map_err(|error| {
                        format!("could not persist mock sender state: {error}")
                    })?)
                    .map_err(|error| format!("could not resume mock sender state: {error}"))?;
                receiver =
                    serde_json::from_slice(&serde_json::to_vec(&receiver).map_err(|error| {
                        format!("could not persist mock receiver state: {error}")
                    })?)
                    .map_err(|error| format!("could not resume mock receiver state: {error}"))?;
                resumed_after_restart = true;
            }
        }
        session
            .begin_finalization()
            .map_err(|error| format!("mock upload could not finalize: {error}"))?;
        let receipt = receiver
            .finalize()
            .map_err(|error| format!("mock receiver could not finalize: {error}"))?;
        let report_id = receipt.report_id.clone();
        let verification_tier = receipt.verification_tier;
        session
            .complete(receipt)
            .map_err(|error| format!("mock receipt was rejected: {error}"))?;
        Ok(MockSubmissionResult {
            schema_version: 1,
            queue_id: queue_id.to_string(),
            capture_session_id: entry.capture_session_id().to_owned(),
            report_id,
            final_state: session.state(),
            verification_tier,
            chunk_count: receiver.acknowledged_chunk_count(),
            uploaded_bytes: receiver.received_bytes(),
            resumed_after_restart,
            external_network_requests: 0,
            artifact: UploadArtifactView::from(&artifact),
        })
    }

    fn upload_queued_submission(
        &self,
        request: SubmissionUploadRequest,
    ) -> Result<SubmissionTransportResult, String> {
        let policy = self
            .submission_policy
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .policy()
            .clone();
        if !policy.log_uploader.enabled {
            return Err("Log Uploader is disabled; enable it before submitting a parse".into());
        }
        let transport = self
            .submission_transport
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .as_ref()
            .cloned()
            .ok_or_else(|| {
                "no submission receiver is connected; connect one in contributor settings or set RLOGS_SUBMISSION_API_URL"
                    .to_owned()
            })?;
        let _verification = self.artifact_verification.try_lock().map_err(|_| {
            "another full artifact import, re-verification, or upload is already running".to_owned()
        })?;
        let queue_id = Sha256Digest::parse(request.queue_id)
            .map_err(|error| format!("queue ID is invalid: {error}"))?;
        let entry = self
            .submission_queue
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .entry(queue_id.as_str())
            .ok_or_else(|| format!("submission draft {} was not found", queue_id.as_str()))?;
        let path = existing_file(&entry.local_artifact_path, "queued sealed rlog")?;
        let artifact = build_upload_artifact(&path)?;
        entry
            .verify_artifact(&artifact)
            .map_err(|error| format!("upload preflight verification failed: {error}"))?;
        let result = transport.upload(&entry, &path)?;
        self.submission_queue
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .mark_submitted(
                result.queue_id.as_str(),
                rlogs_submission::ServerReportReceipt {
                    report_id: result.report_id.clone(),
                    accepted_log_digest: queue_id,
                    verification_tier: result.verification_tier,
                },
            )?;
        Ok(result)
    }

    fn plugin_catalog(&self) -> PluginCatalogView {
        let plugins = self
            .plugins
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if let Ok(launches) = plugins.active_native_launches() {
            let _ = self
                .native_plugin_processes
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .sync(launches);
        }
        plugins.snapshot()
    }

    fn refresh_plugins(&self) -> Result<PluginCatalogView, String> {
        let mut plugins = self
            .plugins
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        plugins.refresh()?;
        self.native_plugin_processes
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .sync(plugins.active_native_launches()?)?;
        Ok(plugins.snapshot())
    }

    fn set_plugin_enabled(
        &self,
        request: PluginEnablementRequest,
    ) -> Result<PluginCatalogView, String> {
        let mut plugins = self
            .plugins
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        plugins.set_enabled(&request.plugin_id, request.enabled)?;
        self.native_plugin_processes
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .sync(plugins.active_native_launches()?)?;
        Ok(plugins.snapshot())
    }

    fn plugin_surface_entrypoint(
        &self,
        plugin_id: &str,
        surface_id: &str,
    ) -> Result<PathBuf, String> {
        self.plugins
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .active_surface_entrypoint(plugin_id, surface_id)
    }

    fn event_viewer_page(
        &self,
        request: EventViewerPageRequest,
    ) -> Result<EventViewerPage, String> {
        let page_size = request.page_size()?;
        let result = self
            .snapshot()
            .last_result
            .ok_or_else(|| "no completed canonical log is available".to_owned())?;
        let mut viewer = self
            .event_viewer
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if let Some(query_id) = request.query_id {
            if request.filter.is_some() {
                return Err("continuing an event query cannot replace its filter".into());
            }
            let query = viewer
                .active
                .as_ref()
                .filter(|query| query.id == query_id)
                .ok_or_else(|| "event viewer query expired; apply the filters again".to_owned())?;
            if query.artifact.session_id != result.session_id
                || query.artifact.digest != result.combat_plugin.rlog.content_sha256
            {
                viewer.active = None;
                return Err("the completed session changed; apply the filters again".into());
            }
            return viewer
                .active
                .as_mut()
                .expect("active event query was checked above")
                .read_page(page_size);
        }

        let filter = request.filter.unwrap_or_default().normalized()?;
        let artifact = viewer.verified_artifact(&result)?;
        let query_id = viewer.next_query_id()?;
        let mut query = ActiveEventViewerQuery::open(query_id, artifact, filter)?;
        let page = query.read_page(page_size)?;
        viewer.active = Some(query);
        Ok(page)
    }

    fn run_report(&self) -> Result<RunReportView, String> {
        let result = self
            .snapshot()
            .last_result
            .ok_or_else(|| "no completed canonical log is available".to_owned())?;
        let projection = run_projection_snapshot(&result.encounter_recorder)?;
        if projection.session_id != result.session_id {
            return Err("cached run projection session does not match local history".into());
        }
        Ok(RunReportView {
            schema_version: 1,
            source_rlog: result.output_rlog,
            artifact_digest: result.encounter_recorder.rlog.content_sha256,
            integrity_verified: result.upload_artifact.is_some(),
            replay_metrics: result.encounter_recorder.metrics,
            projection,
        })
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
        let (combat_plugin, encounter_recorder, upload_artifact) =
            replay_builtins_and_build_artifact(&input)?;
        verify_replay_artifact(&combat_plugin, &encounter_recorder, &upload_artifact)?;
        let combat_snapshot = combat_timeline_snapshot(&combat_plugin)?;
        let session = SessionResult {
            session_id: "fixture-reference-combat".into(),
            source_kind: "sanitized_reference_rlog".into(),
            output_rlog: display_path(&input),
            coverage_report: None,
            frame_count: None,
            framed_record_count: None,
            canonical_event_count: combat_plugin.rlog.event_count,
            known_route_count: None,
            unknown_route_count: None,
            data_gap_count: None,
            private_capture: None,
            connection_evidence: None,
            combat_plugin,
            combat_snapshot,
            encounter_recorder,
            upload_artifact: Some(UploadArtifactView::from(&upload_artifact)),
            submission_queue_id: None,
            submission_queue_status: "not_queued_reference_fixture".into(),
            profile_package_count: 0,
            profile_sync_status: "not_projected_reference_fixture".into(),
            verified_artifact: None,
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
        let submission_queue = Arc::clone(&self.submission_queue);
        let profile_packages = Arc::clone(&self.profile_packages);
        let policy = self
            .submission_policy
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .policy()
            .clone();
        let automatic_submissions =
            policy.log_uploader.enabled && policy.log_uploader.automatic_combat_logs;
        let default_visibility = policy.log_uploader.default_visibility;
        let profile_sync = policy.bpsr_profile_sync;
        thread::Builder::new()
            .name(format!("rlogs-offline-{}", request.session_id))
            .spawn(move || {
                let result = process_offline_session(&install_root, &request).map(|mut result| {
                    let queue_warning = if automatic_submissions {
                        queue_completed_session(&submission_queue, &mut result, default_visibility)
                            .err()
                    } else {
                        result.verified_artifact = None;
                        result.submission_queue_status = "disabled".into();
                        None
                    };
                    let profile_warning = apply_profile_sync_policy(
                        &profile_packages,
                        &mut result,
                        profile_sync.enabled,
                        profile_sync.automatic_profiles,
                    );
                    (result, queue_warning, profile_warning)
                });
                let mut state = state
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner);
                state.active_session_id = None;
                state.completed_unix_millis = Some(unix_millis());
                match result {
                    Ok((result, queue_warning, profile_warning)) => {
                        state.phase = RuntimePhase::Complete;
                        state.detail = completed_session_detail(
                            "Sealed",
                            &result,
                            queue_warning,
                            profile_warning,
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

        let executable_path = process_executable_path(request.process_id)?;
        let plugin_root = self
            .install_root
            .join("plugins/games/blue-protocol-star-resonance");
        let automatic_selection = resolve_live_steam_protocol_pack(&plugin_root, &executable_path)
            .map_err(|error| format!("could not select an exact BPSR live pack: {error}"))?;
        let (pack_kind, pack_source_build, pack) = match &request.pack_path {
            Some(path) if !path.trim().is_empty() => {
                let path = existing_file(path, "protocol pack")?;
                let promoted_root = std::fs::canonicalize(plugin_root.join("protocol-packs"))
                    .map_err(|error| format!("BPSR promoted pack root is unavailable: {error}"))?;
                let kind = if path.starts_with(promoted_root) {
                    LiveProtocolPackKind::Promoted
                } else {
                    LiveProtocolPackKind::ResearchCandidate
                };
                let pack = ProtocolPack::from_json(
                    &std::fs::read(&path)
                        .map_err(|error| format!("could not read protocol pack: {error}"))?,
                )
                .map_err(|error| format!("protocol pack is invalid: {error}"))?;
                let source_build = pack.definition().target.build_id.clone();
                (kind, source_build, pack)
            }
            _ => {
                let pack = automatic_selection
                    .load_pack()
                    .map_err(|error| format!("could not load the selected BPSR pack: {error}"))?;
                (
                    automatic_selection.kind,
                    automatic_selection.pack_build_id.clone(),
                    pack,
                )
            }
        };
        let target = &pack.definition().target;
        if target.deployment_id != "global"
            || target.channel != "steam"
            || target.build_id != automatic_selection.build_id
        {
            return Err(format!(
                "live pack targets {}/{}/{}, but the running client is global/steam/{}",
                target.deployment_id, target.channel, target.build_id, automatic_selection.build_id
            ));
        }
        let provisional_pack = pack_kind != LiveProtocolPackKind::Promoted;
        let mut pack_warning = match pack_kind {
            LiveProtocolPackKind::Promoted => None,
            LiveProtocolPackKind::ResearchCandidate => Some(format!(
                "PROVISIONAL exact-build BPSR protocol pack for client build {}. History, overlay, submissions, and rDPS remain active; unresolved protocol evidence is retained while this pack awaits promotion.",
                target.build_id,
            )),
            LiveProtocolPackKind::CompatibilityFallback => Some(format!(
                "PROVISIONAL BPSR compatibility decode using pack build {} on client build {}. History, overlay, submissions, and rDPS remain active; results may be affected by changed routes and every unresolved protocol record is retained.",
                pack_source_build, target.build_id,
            )),
        };
        let research_service_ids =
            provisional_pack.then(|| provisional_research_service_ids(&pack));
        let region_id = request
            .region_id
            .clone()
            .or_else(|| target.region_id.clone())
            .unwrap_or_else(|| target.deployment_id.clone());
        let region = RegionIdentity {
            deployment_id: target.deployment_id.clone(),
            region_id: region_id.clone(),
            realm_id: None,
            world_id: None,
        };
        let build = GameBuild {
            deployment_id: target.deployment_id.clone(),
            region_id: Some(region_id.clone()),
            channel: target.channel.clone(),
            build_id: target.build_id.clone(),
            executable_version: target.executable_version.clone(),
        };
        let output_directory = request
            .log_output_directory
            .as_ref()
            .filter(|path| !path.trim().is_empty())
            .map(PathBuf::from)
            .unwrap_or_else(|| self.install_root.join("runtime-data/logs"));
        std::fs::create_dir_all(&output_directory)
            .map_err(|error| format!("could not create log output directory: {error}"))?;
        let output_directory = std::fs::canonicalize(output_directory)
            .map_err(|error| format!("could not resolve log output directory: {error}"))?;
        let research_journal_path = provisional_pack.then(|| {
            self.install_root
                .join("private-research/live-journals")
                .join(format!(
                    "{}-steam-{}.protocol.jsonl",
                    request.session_id, target.build_id
                ))
        });
        let validation_preflight_analyzer = RdpsValidationAnalyzer::bundled()
            .map_err(|error| format!("could not load the BPSR rDPS validation watch: {error}"))?;
        let validation_preflight = validation_preflight_analyzer.capture_preflight(&pack);
        if !validation_preflight.capture_capable {
            let capability_warning = format!(
                "The selected pack cannot currently emit these rDPS evidence families: {}. Capture will continue; available calculations remain active and undecoded evidence is retained.",
                validation_preflight.missing_event_kinds.join(", "),
            );
            pack_warning = Some(match pack_warning {
                Some(existing) => format!("{existing} {capability_warning}"),
                None => format!("PROVISIONAL BPSR decode. {capability_warning}"),
            });
        }
        let validation_manifest_game_build = validation_preflight.manifest_game_build.clone();
        let validation_build_mismatch = validation_manifest_game_build != target.build_id;
        let validation_report_name = if validation_build_mismatch {
            format!(
                "{}-observed-steam-{}-manifest-steam-{}.provisional.v{}.validation.json",
                request.session_id,
                target.build_id,
                validation_manifest_game_build,
                RDPS_VALIDATION_REPORT_SCHEMA_VERSION,
            )
        } else {
            format!(
                "{}-steam-{}.v{}.validation.json",
                request.session_id,
                validation_manifest_game_build,
                RDPS_VALIDATION_REPORT_SCHEMA_VERSION,
            )
        };
        let validation_checkpoint_name = if validation_build_mismatch {
            format!(
                "{}-observed-steam-{}-manifest-steam-{}.provisional.v{}.checkpoint.validation.json",
                request.session_id,
                target.build_id,
                validation_manifest_game_build,
                RDPS_VALIDATION_REPORT_SCHEMA_VERSION,
            )
        } else {
            format!(
                "{}-steam-{}.v{}.checkpoint.validation.json",
                request.session_id,
                validation_manifest_game_build,
                RDPS_VALIDATION_REPORT_SCHEMA_VERSION,
            )
        };
        let rdps_validation_report_path = self
            .install_root
            .join("runtime-data/research/rdps/live-validation")
            .join(validation_report_name);
        let rdps_validation_checkpoint_path = self
            .install_root
            .join("runtime-data/research/rdps/live-validation")
            .join(validation_checkpoint_name);
        let rdps_validation_cumulative_path = self
            .install_root
            .join("runtime-data/research/rdps/live-validation")
            .join(format!(
                "steam-{}.v{}.cumulative.validation.json",
                validation_manifest_game_build, RDPS_VALIDATION_REPORT_SCHEMA_VERSION,
            ));
        if let Some(parent) = rdps_validation_report_path.parent() {
            std::fs::create_dir_all(parent).map_err(|error| {
                format!("could not create live rDPS validation directory: {error}")
            })?;
        }
        if rdps_validation_report_path.exists() || rdps_validation_checkpoint_path.exists() {
            return Err(format!(
                "live rDPS validation session {} already exists; use a new session ID so immutable evidence is never overwritten",
                request.session_id
            ));
        }
        let rdps_validation_baseline = if validation_build_mismatch {
            None
        } else {
            let validation_directory = rdps_validation_report_path
                .parent()
                .ok_or_else(|| "live rDPS validation report has no parent directory".to_owned())?;
            let recovered = update_rdps_validation_cumulative_from_sessions(
                validation_directory,
                &validation_manifest_game_build,
                &rdps_validation_cumulative_path,
            )?;
            let mut baseline = RdpsValidationAnalyzer::bundled().map_err(|error| {
                format!("could not load the cumulative BPSR rDPS validation watch: {error}")
            })?;
            baseline.merge_report(&recovered.report).map_err(|error| {
                format!("could not restore cumulative rDPS validation progress: {error}")
            })?;
            Some(baseline)
        };

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
            let mut live_process_id = self
                .live_process_id
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            *live_process_id = Some(request.process_id);
        }
        {
            let mut state = self
                .state
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            state.phase = RuntimePhase::Processing;
            state.active_session_id = Some(request.session_id.clone());
            state.detail = pack_warning.clone().map_or_else(
                || {
                    "Monitoring live BPSR combat everywhere. Dungeon entry is only required to save a run."
                        .into()
                },
                |warning| {
                    format!(
                        "{warning} Monitoring live combat everywhere. Dungeon entry is only required to save a run."
                    )
                },
            );
            state.started_unix_millis = Some(unix_millis());
            state.completed_unix_millis = None;
            state.live_capture_can_stop = true;
            state.monitored_frame_count = 0;
            state.decoded_event_count = 0;
            state.saving_run = false;
            state.sealed_run_count = 0;
        }
        self.live_combat_feed.publish(None);
        self.live_event_feed.reset(request.session_id.clone());

        let state = Arc::clone(&self.state);
        let live_combat_feed = Arc::clone(&self.live_combat_feed);
        let live_event_feed = Arc::clone(&self.live_event_feed);
        let submission_queue = Arc::clone(&self.submission_queue);
        let combat_history = Arc::clone(&self.combat_history);
        let combat_history_feed = Arc::clone(&self.combat_history_feed);
        let character_identities = Arc::clone(&self.character_identities);
        let profile_packages = Arc::clone(&self.profile_packages);
        let policy = self
            .submission_policy
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .policy()
            .clone();
        let automatic_submissions =
            policy.log_uploader.enabled && policy.log_uploader.automatic_combat_logs;
        let default_visibility = policy.log_uploader.default_visibility;
        let profile_sync = policy.bpsr_profile_sync;
        let live_stop = Arc::clone(&self.live_stop);
        let live_process_id = Arc::clone(&self.live_process_id);
        let session_id = request.session_id.clone();
        let validation_game_build = target.build_id.clone();
        let validation_manifest_build = validation_manifest_game_build;
        let research_journal_display = research_journal_path
            .as_ref()
            .map(|path| display_path(path.as_path()));
        let worker_pack_warning = pack_warning;
        let rdps_validation_report_display = display_path(&rdps_validation_report_path);
        let rdps_validation_cumulative_display = display_path(&rdps_validation_cumulative_path);
        let worker = thread::Builder::new()
            .name(format!("rlogs-live-{session_id}"))
            .spawn(move || {
                let mut capture = capture;
                let capture_result = (|| -> Result<_, String> {
                    let producer = format!("rlogs-desktop-host/{}", env!("CARGO_PKG_VERSION"));
                    let region_evidence = vec![RegionEvidence {
                        kind: RegionEvidenceKind::ReplayManifest,
                        reference: format!("continuous-region:{region_id}"),
                    }];
                    let live_header = RlogHeader::new(
                        session_id.clone(),
                        RegionContext {
                            identity: region.clone(),
                            client_build: build.build_id.clone(),
                            protocol_pack_digest: pack.digest().to_owned(),
                            evidence: region_evidence.clone(),
                        },
                        producer.clone(),
                    );
                    let mut live_meter = bpsr_combat_timeline_plugin()?;
                    live_meter.begin_live(&live_header);
                    let mut rdps_validation = RdpsValidationAnalyzer::bundled().map_err(|error| {
                        format!("could not load the BPSR rDPS validation watch: {error}")
                    })?;
                    if rdps_validation.manifest_game_build() != validation_manifest_build {
                        return Err(format!(
                            "bundled rDPS validation manifest changed from {} to {} while starting capture",
                            validation_manifest_build,
                            rdps_validation.manifest_game_build(),
                        ));
                    }
                    rdps_validation.observe_game_build(validation_game_build.clone());
                    let checkpoint_writer = RdpsValidationCheckpointWriter::spawn(
                        rdps_validation_checkpoint_path.clone(),
                    )?;
                    let encounter_config = bundled_run_reducer_config()
                        .map_err(|error| format!("could not load BPSR run rules: {error}"))?;
                    let mut live_encounter = EncounterRecorderPlugin::new(encounter_config);
                    live_encounter.begin_live(&live_header);
                    let mut captured_run_projections = VecDeque::new();
                    let mut capture_time_identities = CaptureTimeCharacterIdentityStore::default();
                    let mut live_dungeon_active = false;
                    let mut live_dungeon_scene_id = None;
                    let mut last_world_context_event: Option<EventEnvelope> = None;
                    let mut live_run_projection: Option<CombatRunHistory> = None;
                    let mut last_live_projection_refresh = Instant::now()
                        .checked_sub(Duration::from_millis(250))
                        .unwrap_or_else(Instant::now);
                    let mut last_live_publish = Instant::now()
                        .checked_sub(Duration::from_millis(16))
                        .unwrap_or_else(Instant::now);
                    let mut initial_live_snapshot =
                        live_meter.live_overlay_snapshot().map_err(|error| {
                            format!("live Combat Meter failed to initialize: {error}")
                        })?;
                    enrich_bpsr_live_character_state(
                        &mut initial_live_snapshot,
                        &capture_time_identities,
                        LiveCharacterIdentityAuthority::CaptureTime,
                    );
                    enrich_bpsr_live_character_state(
                        &mut initial_live_snapshot,
                        &*character_identities
                            .lock()
                            .unwrap_or_else(std::sync::PoisonError::into_inner),
                        LiveCharacterIdentityAuthority::PersistentFallback,
                    );
                    live_combat_feed.publish(Some(initial_live_snapshot));
                    let mut recorder = ContinuousBpsrRecorder::new(
                        &pack,
                        ContinuousRecordingConfig {
                            base_session_id: session_id.clone(),
                            producer,
                            build,
                            region,
                            region_evidence,
                            decoder: ProtocolRuntimeConfig::default(),
                            output_directory,
                            persist_dungeon_logs: true,
                            research_journal: research_journal_path.map(|path| {
                                ContinuousResearchJournalConfig {
                                    path,
                                    allowed_service_ids: research_service_ids
                                        .expect("research service IDs accompany journal path"),
                                    retain_opaque_client_frame_up: true,
                                }
                            }),
                        },
                    )
                    .map_err(|error| {
                        format!("continuous BPSR recorder failed to start: {error}")
                    })?;
                    let mut saving_run = false;
                    let mut last_status_update = Instant::now();
                    let mut last_validation_checkpoint = Instant::now();
                    let mut checkpointed_validation_event_count = 0;
                    let mut validation_checkpoint_failed = false;
                    // Keep pending changes across capture frames. A frame can
                    // arrive inside the 16 ms publish window; dropping its
                    // dirty bit would leave profile/loadout enrichment stale
                    // until an unrelated later combat or terminal event.
                    let mut live_dirty = false;
                    loop {
                        let Some(frame) = capture
                            .next_frame()
                            .map_err(|error| format!("live capture failed: {error}"))?
                        else {
                            break;
                        };
                        recorder
                            .add_connections(
                                capture.confirmed_connections().into_iter().map(
                                    |connection| GameConnection {
                                        client: rlogs_network::IpEndpoint::new(
                                            connection.client.address,
                                            connection.client.port,
                                        ),
                                        server: rlogs_network::IpEndpoint::new(
                                            connection.server.address,
                                            connection.server.port,
                                        ),
                                    },
                                ),
                            )
                            .map_err(|error| {
                                format!("could not extend live BPSR connections: {error}")
                            })?;
                        let mut live_boundary_changed = false;
                        let mut freeze_history = false;
                        let mut frozen_capture_time_identities = None;
                        let mut live_snapshot_error = None;
                        let mut live_event_lines = Vec::new();
                        let sealed = recorder
                            .process_frame_with_events(frame, |event| {
                                if matches!(
                                    &event.event,
                                    CanonicalEvent::Timeline(timeline)
                                        if matches!(&timeline.kind, TimelineEventKind::Damage(_))
                                ) {
                                    live_combat_feed.signal_damage(
                                        event.time.observed_micros,
                                        live_dungeon_active,
                                    );
                                }
                                rdps_validation.observe(event);
                                if matches!(
                                    event.event.topic(),
                                    EventTopic::CharacterProfile | EventTopic::Actor
                                ) {
                                    let mut persistent_identities = character_identities
                                        .lock()
                                        .unwrap_or_else(std::sync::PoisonError::into_inner);
                                    let _ = persistent_identities.observe(event);
                                    if let Err(error) = capture_time_identities
                                        .observe_with_name_fallback(event, &*persistent_identities)
                                    {
                                        live_snapshot_error = Some(format!(
                                            "could not retain capture-time character identity: {error}"
                                        ));
                                    }
                                }
                                let next_world_scene_id = world_scene_id(&event.event);
                                let departed_live_dungeon = live_dungeon_active
                                    && live_dungeon_scene_id
                                        .zip(next_world_scene_id)
                                        .is_some_and(|(active, next)| active != next);
                                if event.event.topic() == EventTopic::World {
                                    last_world_context_event = Some(event.clone());
                                }
                                let opening = matches!(
                                    &event.event,
                                    CanonicalEvent::Dungeon(dungeon)
                                        if matches!(
                                            dungeon.kind,
                                            DungeonEventKind::Entered | DungeonEventKind::Started
                                        )
                                );
                                if opening && !live_dungeon_active {
                                    rdps_validation.clear_transient_context();
                                    begin_live_combat_preserving_world(
                                        &mut live_meter,
                                        &live_header,
                                        last_world_context_event.as_ref(),
                                    );
                                    if let Err(error) = begin_live_encounter_preserving_world(
                                        &mut live_encounter,
                                        &live_header,
                                        last_world_context_event.as_ref(),
                                    ) {
                                        live_snapshot_error = Some(error);
                                    }
                                    live_dungeon_active = true;
                                    live_run_projection = None;
                                    live_dungeon_scene_id = last_world_context_event
                                        .as_ref()
                                        .and_then(|event| world_scene_id(&event.event));
                                    live_boundary_changed = true;
                                }
                                let topic = event.event.topic();
                                if matches!(
                                    topic,
                                    EventTopic::World
                                        | EventTopic::Actor
                                        | EventTopic::Combat
                                        | EventTopic::Encounter
                                        | EventTopic::Dungeon
                                        | EventTopic::DataQuality
                                ) {
                                    live_meter.observe_live(event);
                                    if !live_meter.latest_exact_contributions().is_empty()
                                        || !live_meter
                                            .latest_exact_rational_contributions()
                                            .is_empty()
                                    {
                                        let projection_status =
                                            live_meter.damage_contribution_status();
                                        rdps_validation.observe_projected_contributions(
                                            event.sequence,
                                            live_meter.latest_exact_contributions(),
                                            live_meter.latest_exact_rational_contributions(),
                                            &projection_status,
                                        );
                                    }
                                }
                                if matches!(
                                    topic,
                                    EventTopic::World
                                        | EventTopic::Actor
                                        | EventTopic::Combat
                                        | EventTopic::Encounter
                                        | EventTopic::Dungeon
                                        | EventTopic::DataQuality
                                ) && let Err(error) = live_encounter.observe_live(event)
                                {
                                    live_snapshot_error = Some(format!(
                                        "live Encounter Recorder failed: {error}"
                                    ));
                                }
                                if live_overlay_topic_invalidates(topic) {
                                    live_dirty = true;
                                }
                                let terminal = live_dungeon_active
                                    && (closes_live_run_history(&event.event)
                                        || departed_live_dungeon);
                                if terminal {
                                    freeze_history = true;
                                    frozen_capture_time_identities =
                                        Some(capture_time_identities.clone());
                                    capture_time_identities.clear();
                                    live_dungeon_active = false;
                                    live_dungeon_scene_id = None;
                                    live_boundary_changed = true;
                                    rdps_validation.clear_transient_context();
                                }
                                if live_boundary_changed
                                    || (live_dirty
                                        && last_live_publish.elapsed()
                                            >= Duration::from_millis(16))
                                {
                                    let reviewed_dungeon = live_dungeon_active || freeze_history;
                                    if reviewed_dungeon
                                        && (live_boundary_changed
                                            || last_live_projection_refresh.elapsed()
                                                >= Duration::from_millis(250))
                                    {
                                        match live_encounter.live_snapshot().and_then(|run| {
                                            live_meter.history_snapshot(&run.runs)
                                        }) {
                                            Ok(mut history) => {
                                                let identities = frozen_capture_time_identities
                                                    .as_ref()
                                                    .unwrap_or(&capture_time_identities);
                                                if let Err(error) = freeze_bpsr_history_character_state(
                                                    &mut history,
                                                    identities,
                                                ) {
                                                    live_snapshot_error = Some(error);
                                                } else {
                                                    live_run_projection = history.runs.last().cloned();
                                                    last_live_projection_refresh = Instant::now();
                                                }
                                            }
                                            Err(error) => {
                                                live_snapshot_error = Some(format!(
                                                    "live reviewed projection failed: {error}"
                                                ));
                                            }
                                        }
                                    } else if !reviewed_dungeon {
                                        live_run_projection = None;
                                    }
                                    match live_meter.live_overlay_snapshot() {
                                        Ok(mut snapshot) => {
                                            enrich_bpsr_live_character_state(
                                                &mut snapshot,
                                                &capture_time_identities,
                                                LiveCharacterIdentityAuthority::CaptureTime,
                                            );
                                            enrich_bpsr_live_character_state(
                                                &mut snapshot,
                                                &*character_identities.lock().unwrap_or_else(
                                                    std::sync::PoisonError::into_inner,
                                                ),
                                                LiveCharacterIdentityAuthority::PersistentFallback,
                                            );
                                            apply_live_run_projection_clocks(
                                                &mut snapshot,
                                                live_run_projection.as_ref(),
                                            );
                                            live_combat_feed.publish_with_projection(
                                                Some(snapshot),
                                                live_run_projection.clone(),
                                                reviewed_dungeon,
                                            );
                                            last_live_publish = Instant::now();
                                            live_boundary_changed = false;
                                            live_dirty = false;
                                        }
                                        Err(error) => {
                                            live_snapshot_error = Some(format!(
                                                "live Combat Meter snapshot failed: {error}"
                                            ));
                                        }
                                    }
                                }
                                // The overlay has already consumed and published
                                // this typed event. Only then do we format the
                                // compact, pre-localization Event Viewer line.
                                live_event_lines.push(LiveEventLine::from_envelope(event));
                            })
                            .map_err(|error| format!("live BPSR decoding failed: {error}"))?;
                        if let Some(error) = live_snapshot_error {
                            return Err(error);
                        }
                        live_event_feed.publish_batch(live_event_lines);
                        if !validation_checkpoint_failed
                            && last_validation_checkpoint.elapsed()
                            >= RDPS_VALIDATION_CHECKPOINT_INTERVAL
                        {
                            let checkpoint = rdps_validation.report();
                            if checkpoint.total_events != checkpointed_validation_event_count {
                                let checkpoint_event_count = checkpoint.total_events;
                                match checkpoint_writer.try_checkpoint(checkpoint) {
                                    Ok(true) => {
                                        checkpointed_validation_event_count = checkpoint_event_count;
                                    }
                                    Ok(false) => {}
                                    Err(_) => {
                                        // Evidence durability is subordinate to
                                        // live decoding. A failed checkpoint
                                        // disables only later checkpoint attempts;
                                        // final report persistence is still tried.
                                        validation_checkpoint_failed = true;
                                    }
                                }
                            }
                            last_validation_checkpoint = Instant::now();
                        }
                        if freeze_history {
                            let run = live_encounter.live_snapshot().map_err(|error| {
                                format!("could not freeze live run history: {error}")
                            })?;
                            let mut history = live_meter.history_snapshot(&run.runs).map_err(
                                |error| {
                                    format!("could not freeze filterable combat history: {error}")
                                },
                            )?;
                            freeze_bpsr_history_character_state(
                                &mut history,
                                frozen_capture_time_identities.as_ref().ok_or_else(|| {
                                    "terminal run has no capture-time character identity ledger"
                                        .to_owned()
                                })?,
                            )?;
                            captured_run_projections.push_back(CapturedRunProjection {
                                combat: live_meter.live_snapshot().map_err(|error| {
                                    format!("could not freeze live Combat Meter history: {error}")
                                })?,
                                history,
                                run,
                            });
                        }
                        let now_saving = recorder.is_saving_run();
                        let phase_changed = now_saving != saving_run;
                        if phase_changed || last_status_update.elapsed() >= Duration::from_millis(500)
                        {
                            saving_run = now_saving;
                            last_status_update = Instant::now();
                            let metrics = recorder.metrics();
                            let validation_progress = if let Some(baseline) =
                                rdps_validation_baseline.as_ref()
                            {
                                rdps_validation
                                    .progress_with_baseline(baseline)
                                    .map_err(|error| {
                                        format!(
                                            "could not combine current and cumulative rDPS validation progress: {error}"
                                        )
                                    })?
                            } else {
                                rdps_validation.progress()
                            };
                            let validation_domains =
                                format_rdps_validation_remaining_domains(&validation_progress);
                            let mut snapshot = state
                                .lock()
                                .unwrap_or_else(std::sync::PoisonError::into_inner);
                            snapshot.monitored_frame_count = metrics.frame_count;
                            snapshot.decoded_event_count = metrics.decoded_event_count;
                            snapshot.saving_run = saving_run;
                            snapshot.sealed_run_count = metrics
                                .completed_run_count
                                .saturating_add(metrics.incomplete_run_count);
                            let provisional_prefix = worker_pack_warning
                                .as_deref()
                                .map(|warning| format!("{warning} "))
                                .unwrap_or_default();
                            snapshot.detail = if saving_run {
                                format!(
                                    "{provisional_prefix}Monitoring live BPSR combat everywhere; saving the active dungeon segment. {} frames and {} canonical events inspected. rDPS candidate coverage: {} complete, {} partial, {} untouched. Remaining by domain: {}.",
                                    metrics.frame_count,
                                    metrics.decoded_event_count,
                                    validation_progress.candidate_event_coverage_complete,
                                    validation_progress.partial_candidate_event_coverage,
                                    validation_progress.no_candidate_evidence,
                                    validation_domains,
                                )
                            } else {
                                format!(
                                    "{provisional_prefix}Monitoring live BPSR combat everywhere; dungeon entry is only required to save history. {} frames and {} canonical events inspected. rDPS candidate coverage: {} complete, {} partial, {} untouched. Remaining by domain: {}.",
                                    metrics.frame_count,
                                    metrics.decoded_event_count,
                                    validation_progress.candidate_event_coverage_complete,
                                    validation_progress.partial_candidate_event_coverage,
                                    validation_progress.no_candidate_evidence,
                                    validation_domains,
                                )
                            };
                        }
                        for log in sealed {
                            let projection = captured_run_projections.pop_front().ok_or_else(|| {
                                format!(
                                    "sealed run {} has no capture-time history projection",
                                    log.session_id
                                )
                            })?;
                            postprocess_continuous_run(
                                log,
                                projection,
                                Arc::clone(&state),
                                Arc::clone(&submission_queue),
                                Arc::clone(&combat_history),
                                Arc::clone(&combat_history_feed),
                                Arc::clone(&profile_packages),
                                automatic_submissions,
                                default_visibility,
                                profile_sync.clone(),
                            );
                        }
                    }
                    let sealed = recorder
                        .finish()
                        .map_err(|error| format!("could not drain continuous BPSR state: {error}"))?;
                    if live_dungeon_active && !sealed.is_empty() {
                        let run = live_encounter.live_snapshot().map_err(|error| {
                            format!("could not freeze incomplete run history: {error}")
                        })?;
                        let mut history = live_meter.history_snapshot(&run.runs).map_err(
                            |error| format!("could not freeze incomplete filterable history: {error}"),
                        )?;
                        freeze_bpsr_history_character_state(
                            &mut history,
                            &capture_time_identities,
                        )?;
                        captured_run_projections.push_back(CapturedRunProjection {
                            combat: live_meter.live_snapshot().map_err(|error| {
                                format!("could not freeze incomplete Combat Meter history: {error}")
                            })?,
                            history,
                            run,
                        });
                    }
                    for log in sealed {
                        let projection = captured_run_projections.pop_front().ok_or_else(|| {
                            format!(
                                "sealed run {} has no capture-time history projection",
                                log.session_id
                            )
                        })?;
                        postprocess_continuous_run(
                            log,
                            projection,
                            Arc::clone(&state),
                            Arc::clone(&submission_queue),
                            Arc::clone(&combat_history),
                            Arc::clone(&combat_history_feed),
                            Arc::clone(&profile_packages),
                            automatic_submissions,
                            default_visibility,
                            profile_sync.clone(),
                        );
                    }
                    let _ = checkpoint_writer.finish();
                    let session_validation_report = rdps_validation.report();
                    write_rdps_validation_session_report_once(
                        &rdps_validation_report_path,
                        &session_validation_report,
                    )?;

                    let validation_summary = if session_validation_report
                        .provisional_build_mismatch
                    {
                        // Retain and report hotfix evidence, but do not allow it
                        // to advance an exact manifest-build proof counter.
                        session_validation_report.summary.clone()
                    } else {
                        // Treat immutable exact-build per-session reports as the
                        // source of truth. Scanning for session IDs absent from
                        // the cumulative index recovers the report left behind
                        // if the process stops between these durable writes.
                        let cumulative_bundle =
                            update_rdps_validation_cumulative_from_sessions(
                                rdps_validation_report_path.parent().ok_or_else(|| {
                                    "live rDPS validation report has no parent directory"
                                        .to_string()
                                })?,
                                &validation_game_build,
                                &rdps_validation_cumulative_path,
                            )?;
                        write_rdps_validation_cumulative_report_atomic(
                            &rdps_validation_cumulative_path,
                            &cumulative_bundle,
                        )?;
                        cumulative_bundle.report.summary
                    };
                    match std::fs::remove_file(&rdps_validation_checkpoint_path) {
                        Ok(()) => {}
                        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                        Err(_) => {
                            // A stale checkpoint is harmless: cumulative recovery
                            // prefers the immutable final report for this session.
                        }
                    }
                    Ok((
                        recorder.metrics().clone(),
                        validation_summary,
                        session_validation_report.provisional_build_mismatch,
                    ))
                })();
                {
                    let mut stop = live_stop
                        .lock()
                        .unwrap_or_else(std::sync::PoisonError::into_inner);
                    *stop = None;
                }
                {
                    let mut process_id = live_process_id
                        .lock()
                        .unwrap_or_else(std::sync::PoisonError::into_inner);
                    *process_id = None;
                }

                let mut state = state
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner);
                state.active_session_id = None;
                state.completed_unix_millis = Some(unix_millis());
                state.live_capture_can_stop = false;
                match capture_result {
                    Ok((metrics, validation_summary, validation_is_provisional)) => {
                        state.phase = RuntimePhase::Complete;
                        state.monitored_frame_count = metrics.frame_count;
                        state.decoded_event_count = metrics.decoded_event_count;
                        state.saving_run = false;
                        state.sealed_run_count = metrics
                            .completed_run_count
                            .saturating_add(metrics.incomplete_run_count);
                        let validation_report_display = if validation_is_provisional {
                            rdps_validation_report_display.as_str()
                        } else {
                            rdps_validation_cumulative_display.as_str()
                        };
                        let validation_label = if validation_is_provisional {
                            "provisional build-mismatch evidence"
                        } else {
                            "exact-build evidence"
                        };
                        let provisional_prefix = worker_pack_warning
                            .as_deref()
                            .map(|warning| format!("{warning} "))
                            .unwrap_or_default();
                        let journal_suffix = research_journal_display
                            .as_deref()
                            .map(|journal| format!(" Provisional protocol journal: {journal}."))
                            .unwrap_or_default();
                        state.detail = format!(
                            "{provisional_prefix}Monitoring stopped after {} owned frames and {} decoded events; {} completed and {} incomplete dungeon segments were sealed. rDPS {}: {} complete, {} partial, {} remaining; report: {}.{journal_suffix}",
                            metrics.frame_count,
                            metrics.decoded_event_count,
                            metrics.completed_run_count,
                            metrics.incomplete_run_count,
                            validation_label,
                            validation_summary.candidate_event_coverage_complete,
                            validation_summary.partial_candidate_event_coverage,
                            validation_summary.no_candidate_evidence,
                            validation_report_display,
                        );
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
            let mut process_id = self
                .live_process_id
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            *process_id = None;
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
                objective_catalog: None,
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
    let (combat_plugin, encounter_recorder, upload_artifact) =
        replay_builtins_and_build_artifact(&output_path)?;
    verify_replay_artifact(&combat_plugin, &encounter_recorder, &upload_artifact)?;
    let combat_snapshot = combat_timeline_snapshot(&combat_plugin)?;
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
        combat_snapshot,
        encounter_recorder,
        upload_artifact: Some(UploadArtifactView::from(&upload_artifact)),
        submission_queue_id: None,
        submission_queue_status: "pending_local_queue".into(),
        profile_package_count: 0,
        profile_sync_status: "pending_policy".into(),
        verified_artifact: Some(upload_artifact),
    })
}

fn capture_time_continuous_run_result(
    log: &SealedDungeonRunLog,
    mut projection: CapturedRunProjection,
    automatic_submissions: bool,
    profile_sync: &submission_policy::ProfileSyncPolicy,
) -> Result<SessionResult, String> {
    projection.combat.session_id = log.session_id.clone();
    projection.run.session_id = log.session_id.clone();
    projection.history.session_id = log.session_id.clone();
    for run in &mut projection.run.runs {
        run.source_session_id = log.session_id.clone();
    }
    let combat_plugin = capture_time_plugin_report(
        CombatTimelinePlugin::new().descriptor(),
        log,
        COMBAT_SNAPSHOT_SCHEMA_ID,
        COMBAT_SNAPSHOT_SCHEMA_VERSION,
        &projection.combat,
        projection.combat.event_count,
    )?;
    let encounter_recorder = capture_time_plugin_report(
        EncounterRecorderPlugin::default().descriptor(),
        log,
        RUN_PROJECTION_SCHEMA_ID,
        RUN_PROJECTION_SCHEMA_VERSION,
        &projection.run,
        log.seal.event_count,
    )?;
    let submission_queue_status = if !log.is_completed() {
        "incomplete_local_history"
    } else if automatic_submissions {
        "validation_pending"
    } else {
        "disabled"
    };
    let profile_sync_status = if !profile_sync.enabled {
        "disabled"
    } else if profile_sync.automatic_profiles {
        "projection_pending"
    } else {
        "manual_only"
    };
    Ok(SessionResult {
        session_id: log.session_id.clone(),
        source_kind: "continuous_process_owned_capture".into(),
        output_rlog: display_path(&log.path),
        coverage_report: None,
        frame_count: None,
        framed_record_count: None,
        canonical_event_count: log.seal.event_count,
        known_route_count: None,
        unknown_route_count: None,
        data_gap_count: None,
        private_capture: None,
        connection_evidence: None,
        combat_plugin,
        combat_snapshot: projection.combat,
        encounter_recorder,
        upload_artifact: None,
        submission_queue_id: None,
        submission_queue_status: submission_queue_status.into(),
        profile_package_count: 0,
        profile_sync_status: profile_sync_status.into(),
        verified_artifact: None,
    })
}

fn capture_time_plugin_report(
    descriptor: rlogs_plugin_runtime::ReplayPluginDescriptor,
    log: &SealedDungeonRunLog,
    schema_id: &str,
    schema_version: u16,
    snapshot: &impl Serialize,
    delivered_events: u64,
) -> Result<PluginRunReport, String> {
    let output = PluginOutput::Snapshot {
        schema_id: schema_id.into(),
        schema_version,
        payload: serde_json::to_value(snapshot)
            .map_err(|error| format!("could not freeze capture-time history: {error}"))?,
    };
    let output_bytes = serde_json::to_vec(&output)
        .map_err(|error| format!("could not size capture-time history: {error}"))?
        .len();
    Ok(PluginRunReport {
        descriptor,
        rlog: RlogReplaySummary {
            event_count: log.seal.event_count,
            first_observed_micros: Some(log.started.time.observed_micros),
            last_observed_micros: Some(log.ended.time.observed_micros),
            content_sha256: log.seal.content_sha256.clone(),
        },
        metrics: PluginRunMetrics {
            events_seen: log.seal.event_count,
            events_delivered: delivered_events.min(log.seal.event_count),
            outputs_emitted: 1,
            output_bytes,
            plugin_elapsed_micros: 0,
            wall_elapsed_micros: 0,
        },
        outputs: vec![output],
    })
}

#[allow(clippy::too_many_arguments)]
fn postprocess_continuous_run(
    log: SealedDungeonRunLog,
    mut projection: CapturedRunProjection,
    state: Arc<Mutex<RuntimeSnapshot>>,
    submission_queue: Arc<Mutex<LocalSubmissionQueue>>,
    combat_history: Arc<Mutex<CombatHistoryStore>>,
    combat_history_feed: Arc<CombatHistoryRevisionFeed>,
    profile_packages: Arc<Mutex<LocalProfilePackageStore>>,
    automatic_submissions: bool,
    default_visibility: ReportVisibility,
    profile_sync: submission_policy::ProfileSyncPolicy,
) {
    let completed = log.is_completed();
    let session_id = log.session_id.clone();
    projection.history.session_id.clone_from(&session_id);
    let history = projection.history.clone();
    let mut result = match capture_time_continuous_run_result(
        &log,
        projection,
        automatic_submissions,
        &profile_sync,
    ) {
        Ok(result) => result,
        Err(error) => {
            let mut snapshot = state
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            snapshot.detail =
                format!("Monitoring continues; run {session_id} history failed: {error}");
            return;
        }
    };
    {
        let history_result = combat_history
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .record(&history, unix_millis());
        let history_recorded = history_result.is_ok();
        let mut snapshot = state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        snapshot.detail = match history_result {
            Ok(_) => format!(
                "Monitoring; run {session_id} history is ready from capture-time projections. Submission: {}; profile sync: {}.",
                result.submission_queue_status, result.profile_sync_status
            ),
            Err(error) => format!(
                "Monitoring; run {session_id} is available in memory, but its history index could not be saved: {error}"
            ),
        };
        snapshot.last_result = Some(result.clone());
        drop(snapshot);
        if history_recorded {
            combat_history_feed.publish();
        }
    }

    let build_submission = completed && automatic_submissions;
    let build_profile = profile_sync.enabled && profile_sync.automatic_profiles;
    if !build_submission && !build_profile {
        return;
    }

    let worker_state = Arc::clone(&state);
    let worker_session_id = session_id.clone();
    let worker = thread::Builder::new()
        .name(format!("rlogs-optional-{session_id}"))
        .spawn(move || {
            let queue_warning = if build_submission {
                match build_upload_artifact(&log.path) {
                    Ok(artifact)
                        if artifact.rlog.content_sha256
                            == result.combat_plugin.rlog.content_sha256
                            && artifact.rlog.event_count == result.canonical_event_count =>
                    {
                        result.upload_artifact = Some(UploadArtifactView::from(&artifact));
                        result.verified_artifact = Some(artifact);
                        queue_completed_session(&submission_queue, &mut result, default_visibility)
                            .err()
                    }
                    Ok(_) => {
                        Some("submission validation did not match the capture-time seal".into())
                    }
                    Err(error) => Some(error),
                }
            } else {
                None
            };
            let profile_warning = if build_profile {
                apply_profile_sync_policy(&profile_packages, &mut result, true, true)
            } else {
                None
            };
            let mut snapshot = worker_state
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            if snapshot
                .last_result
                .as_ref()
                .is_some_and(|current| current.session_id == worker_session_id)
            {
                snapshot.detail = completed_session_detail(
                    if completed {
                        "Monitoring; optional validation finished for"
                    } else {
                        "Monitoring; optional profile projection finished for"
                    },
                    &result,
                    queue_warning,
                    profile_warning,
                );
                snapshot.last_result = Some(result);
            }
        });
    if let Err(error) = worker {
        let mut snapshot = state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        snapshot.detail =
            format!("Monitoring continues; could not start run finalizer {session_id}: {error}");
    }
}

fn closes_live_run_history(event: &CanonicalEvent) -> bool {
    match event {
        CanonicalEvent::Dungeon(dungeon) => matches!(
            dungeon.kind,
            DungeonEventKind::Completed | DungeonEventKind::Failed | DungeonEventKind::Exited
        ),
        CanonicalEvent::Timeline(timeline) => matches!(
            timeline.kind,
            TimelineEventKind::RunBoundary {
                state: RunState::Completed | RunState::Failed | RunState::Exited,
                ..
            }
        ),
        _ => false,
    }
}

fn world_scene_id(event: &CanonicalEvent) -> Option<i32> {
    match event {
        CanonicalEvent::WorldChanged(world) => world.scene_id.map(|scene| scene.0),
        _ => None,
    }
}

fn begin_live_encounter_preserving_world(
    encounter: &mut EncounterRecorderPlugin,
    header: &RlogHeader,
    last_world_context_event: Option<&EventEnvelope>,
) -> Result<(), String> {
    encounter.begin_live(header);
    if let Some(event) = last_world_context_event {
        encounter
            .observe_live(event)
            .map_err(|error| format!("could not restore world context for live run: {error}"))?;
    }
    Ok(())
}

fn begin_live_combat_preserving_world(
    meter: &mut CombatTimelinePlugin,
    header: &RlogHeader,
    last_world_context_event: Option<&EventEnvelope>,
) {
    meter.begin_live_preserving_player_identities(header);
    if let Some(event) = last_world_context_event {
        meter.observe_live(event);
    }
}

fn apply_live_run_projection_clocks(
    snapshot: &mut CombatTimelineSnapshot,
    run: Option<&CombatRunHistory>,
) {
    let Some(run) = run else {
        return;
    };
    if let Some(entire_run) = run.views.iter().find(|view| view.id == "all") {
        snapshot.game_time_micros = run.game_time_micros.or(Some(entire_run.elapsed_micros));
        snapshot.active_combat_micros = entire_run.active_combat_micros;
    } else {
        snapshot.game_time_micros = run.game_time_micros;
    }
    snapshot.true_time_micros = run.true_time_micros;
}

fn queue_completed_session(
    queue: &Arc<Mutex<LocalSubmissionQueue>>,
    result: &mut SessionResult,
    visibility: ReportVisibility,
) -> Result<QueueInsertOutcome, String> {
    let outcome = (|| {
        let artifact = result
            .verified_artifact
            .clone()
            .ok_or_else(|| "completed session has no verified upload artifact".to_owned())?;
        let (outcome, queue_id) = enqueue_verified_artifact(
            queue,
            &artifact,
            result.output_rlog.clone(),
            unix_millis(),
            visibility,
        )?;
        result.submission_queue_id = Some(queue_id);
        result.submission_queue_status = outcome.label().into();
        result.verified_artifact = None;
        Ok(outcome)
    })();
    if let Err(error) = &outcome {
        result.submission_queue_status = format!("queue_failed: {error}");
    }
    outcome
}

fn completed_session_detail(
    prefix: &str,
    result: &SessionResult,
    queue_warning: Option<String>,
    profile_warning: Option<String>,
) -> String {
    let mut detail = format!(
        "{prefix} {} canonical events; submission draft: {}; profile sync: {}.",
        result.canonical_event_count, result.submission_queue_status, result.profile_sync_status
    );
    let warnings = [
        queue_warning.map(|warning| format!("submission queue: {warning}")),
        profile_warning.map(|warning| format!("profile projection: {warning}")),
    ]
    .into_iter()
    .flatten()
    .collect::<Vec<_>>();
    if !warnings.is_empty() {
        detail.push_str(" Warnings: ");
        detail.push_str(&warnings.join("; "));
    }
    detail
}

fn apply_profile_sync_policy(
    store: &Arc<Mutex<LocalProfilePackageStore>>,
    result: &mut SessionResult,
    enabled: bool,
    automatic_profiles: bool,
) -> Option<String> {
    if !enabled {
        result.profile_sync_status = "disabled".into();
        return None;
    }
    if !automatic_profiles {
        result.profile_sync_status = "manual_only".into();
        return None;
    }
    match project_completed_profile_session(store, &result.output_rlog, unix_millis()) {
        Ok((packages, status)) => {
            result.profile_package_count = packages.len();
            result.profile_sync_status = status;
            None
        }
        Err(error) => {
            result.profile_sync_status = format!("projection_failed: {error}");
            Some(error)
        }
    }
}

fn project_completed_profile_session(
    store: &Arc<Mutex<LocalProfilePackageStore>>,
    rlog_path: &str,
    created_unix_millis: u64,
) -> Result<(Vec<ProfilePackageView>, String), String> {
    let path = existing_file(rlog_path, "sealed profile source log")?;
    let file = File::open(&path)
        .map_err(|error| format!("could not open sealed profile source log: {error}"))?;
    let packages = project_local_profile_packages(
        BufReader::new(file),
        RlogLimits::default(),
        created_unix_millis,
    )
    .map_err(|error| format!("BPSR profile projection failed: {error}"))?;
    let mut stored = Vec::with_capacity(packages.len());
    let mut store = store
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    for package in packages {
        stored.push(store.upsert(package)?);
    }
    let status = if stored.is_empty() {
        "no_personal_profile_observations"
    } else {
        "packaged_locally"
    };
    Ok((stored, status.into()))
}

fn enqueue_verified_artifact(
    queue: &Arc<Mutex<LocalSubmissionQueue>>,
    source_artifact: &LocalLogArtifact,
    local_artifact_path: String,
    created_unix_millis: u64,
    visibility: ReportVisibility,
) -> Result<(QueueInsertOutcome, String), String> {
    let (artifact, local_artifact_path) =
        prepare_submission_artifact(queue, source_artifact, Path::new(&local_artifact_path))?;
    let protocol_pack_digest = parse_prefixed_sha256(&artifact.header.region.protocol_pack_digest)?;
    let metadata = SubmissionMetadata::new(
        BPSR_GAME_PLUGIN_ID,
        artifact.file_sha256.to_string(),
        artifact.header.schema_version,
        artifact.header.session_id.clone(),
        artifact.header.region.identity.region_id.clone(),
        artifact.header.region.client_build.clone(),
        protocol_pack_digest,
        submission_privacy_policy_digest(),
        visibility,
    );
    let entry = QueuedSubmission::new_post_run(
        metadata,
        &artifact,
        local_artifact_path,
        created_unix_millis,
    )
    .map_err(|error| format!("could not create submission queue entry: {error}"))?;
    entry
        .verify_artifact(&artifact)
        .map_err(|error| format!("new submission draft did not match its artifact: {error}"))?;
    let queue_id = entry.queue_id.to_string();
    let outcome = queue
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .enqueue(entry)?;
    Ok((outcome, queue_id))
}

fn prepare_submission_artifact(
    queue: &Arc<Mutex<LocalSubmissionQueue>>,
    source_artifact: &LocalLogArtifact,
    source_path: &Path,
) -> Result<(LocalLogArtifact, String), String> {
    let source_path = existing_file(source_path.to_string_lossy().as_ref(), "sealed local log")?;
    let artifact_directory = queue
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .artifact_directory();
    std::fs::create_dir_all(&artifact_directory).map_err(|error| {
        format!(
            "could not create private submission artifact directory {}: {error}",
            artifact_directory.display()
        )
    })?;
    let partial_path = artifact_directory.join(format!(
        ".{}.privacy-v1.partial.rlog",
        source_artifact.file_sha256
    ));
    match std::fs::remove_file(&partial_path) {
        Ok(()) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => {
            return Err(format!(
                "could not remove interrupted privacy export {}: {error}",
                partial_path.display()
            ));
        }
    }

    let export = (|| -> Result<LocalLogArtifact, String> {
        let input = File::open(&source_path).map_err(|error| {
            format!(
                "could not open local log for privacy export {}: {error}",
                source_path.display()
            )
        })?;
        let output = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&partial_path)
            .map_err(|error| {
                format!(
                    "could not create private submission export {}: {error}",
                    partial_path.display()
                )
            })?;
        let (mut output, _, _) = write_privacy_filtered_submission_log(
            BufReader::new(input),
            BufWriter::new(output),
            RlogLimits::default(),
        )
        .map_err(|error| format!("submission privacy export failed: {error}"))?;
        output
            .flush()
            .and_then(|_| output.get_ref().sync_all())
            .map_err(|error| format!("could not finalize private submission export: {error}"))?;
        drop(output);
        let file = File::open(&partial_path)
            .map_err(|error| format!("could not reopen private submission export: {error}"))?;
        build_privacy_verified_submission_artifact(
            file,
            ArtifactBuildLimits::default(),
            RlogLimits::default(),
        )
        .map_err(|error| format!("private submission export verification failed: {error}"))
    })();
    let artifact = match export {
        Ok(artifact) => artifact,
        Err(error) => {
            let _ = std::fs::remove_file(&partial_path);
            return Err(error);
        }
    };

    let final_path = artifact_directory.join(format!("{}.rlog", artifact.file_sha256));
    if final_path.is_file() {
        let existing = build_privacy_verified_submission_artifact(
            File::open(&final_path)
                .map_err(|error| format!("could not open existing submission export: {error}"))?,
            ArtifactBuildLimits::default(),
            RlogLimits::default(),
        )
        .map_err(|error| format!("existing submission export is invalid: {error}"))?;
        if existing.file_sha256 != artifact.file_sha256 {
            let _ = std::fs::remove_file(&partial_path);
            return Err("submission export digest collision".into());
        }
        std::fs::remove_file(&partial_path)
            .map_err(|error| format!("could not remove duplicate privacy export: {error}"))?;
    } else {
        std::fs::rename(&partial_path, &final_path).map_err(|error| {
            let _ = std::fs::remove_file(&partial_path);
            format!("could not publish private submission export: {error}")
        })?;
    }
    Ok((artifact, display_path(&final_path)))
}

fn read_artifact_chunk(
    file: &mut File,
    chunk: &rlogs_submission::LogChunkDescriptor,
) -> Result<Vec<u8>, String> {
    let byte_length = usize::try_from(chunk.byte_length)
        .map_err(|_| format!("chunk {} is too large for this platform", chunk.sequence))?;
    if byte_length == 0 || byte_length > MAXIMUM_UPLOAD_CHUNK_BYTES {
        return Err(format!(
            "chunk {} exceeds the {}-byte mock upload limit",
            chunk.sequence, MAXIMUM_UPLOAD_CHUNK_BYTES
        ));
    }
    file.seek(SeekFrom::Start(chunk.file_offset))
        .map_err(|error| format!("could not seek to chunk {}: {error}", chunk.sequence))?;
    let mut bytes = vec![0_u8; byte_length];
    file.read_exact(&mut bytes)
        .map_err(|error| format!("could not read chunk {}: {error}", chunk.sequence))?;
    Ok(bytes)
}

fn parse_prefixed_sha256(value: &str) -> Result<Sha256Digest, String> {
    Sha256Digest::parse(value.strip_prefix("sha256:").unwrap_or(value))
        .map_err(|error| format!("protocol pack digest is invalid: {error}"))
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

const MAXIMUM_RDPS_VALIDATION_REPORT_BYTES: u64 = 64 * 1024 * 1024;
const RDPS_VALIDATION_CUMULATIVE_SCHEMA_VERSION: u16 = 1;
const RDPS_VALIDATION_CHECKPOINT_INTERVAL: Duration = Duration::from_secs(60);

struct RdpsValidationCheckpointWriter {
    sender: SyncSender<Option<RdpsValidationReport>>,
    worker: JoinHandle<Result<(), String>>,
}

impl RdpsValidationCheckpointWriter {
    fn spawn(path: PathBuf) -> Result<Self, String> {
        let (sender, receiver) = sync_channel::<Option<RdpsValidationReport>>(1);
        let worker = thread::Builder::new()
            .name("rlogs-rdps-checkpoint".into())
            .spawn(move || {
                while let Ok(message) = receiver.recv() {
                    let Some(report) = message else {
                        return Ok(());
                    };
                    write_rdps_validation_report_atomic(&path, &report)?;
                }
                Ok(())
            })
            .map_err(|error| format!("could not start live rDPS checkpoint writer: {error}"))?;
        Ok(Self { sender, worker })
    }

    fn try_checkpoint(&self, report: RdpsValidationReport) -> Result<bool, String> {
        match self.sender.try_send(Some(report)) {
            Ok(()) => Ok(true),
            Err(TrySendError::Full(_)) => Ok(false),
            Err(TrySendError::Disconnected(_)) => {
                Err("live rDPS checkpoint writer stopped unexpectedly".into())
            }
        }
    }

    fn finish(self) -> Result<(), String> {
        self.sender
            .send(None)
            .map_err(|_| "live rDPS checkpoint writer stopped unexpectedly".to_string())?;
        self.worker
            .join()
            .map_err(|_| "live rDPS checkpoint writer panicked".to_string())?
    }
}

#[derive(Debug, Serialize, Deserialize)]
struct RdpsValidationCumulativeReport {
    schema_version: u16,
    manifest_game_build: String,
    session_ids: BTreeSet<String>,
    report: RdpsValidationReport,
}

fn read_rdps_validation_report(path: &Path) -> Result<RdpsValidationReport, String> {
    read_json_with_limit(
        path,
        MAXIMUM_RDPS_VALIDATION_REPORT_BYTES,
        "live rDPS validation report",
    )
}

fn read_json_with_limit<T>(path: &Path, maximum_bytes: u64, label: &str) -> Result<T, String>
where
    T: for<'de> Deserialize<'de>,
{
    let metadata = std::fs::metadata(path)
        .map_err(|error| format!("could not inspect {label} {}: {error}", path.display()))?;
    if metadata.len() > maximum_bytes {
        return Err(format!(
            "{label} {} exceeds its {}-byte safety limit",
            path.display(),
            maximum_bytes
        ));
    }
    let input = File::open(path)
        .map_err(|error| format!("could not open {label} {}: {error}", path.display()))?;
    serde_json::from_reader(BufReader::new(input))
        .map_err(|error| format!("could not decode {label} {}: {error}", path.display()))
}

fn write_rdps_validation_report_atomic(
    path: &Path,
    report: &RdpsValidationReport,
) -> Result<(), String> {
    write_json_atomic_with_limit(
        path,
        report,
        MAXIMUM_RDPS_VALIDATION_REPORT_BYTES,
        "live rDPS validation report",
    )
}

fn write_rdps_validation_session_report_once(
    path: &Path,
    report: &RdpsValidationReport,
) -> Result<(), String> {
    if path.exists() {
        let existing = read_rdps_validation_report(path)?;
        let existing = serde_json::to_vec(&existing)
            .map_err(|error| format!("could not compare existing validation report: {error}"))?;
        let requested = serde_json::to_vec(report)
            .map_err(|error| format!("could not compare current validation report: {error}"))?;
        if existing == requested {
            return Ok(());
        }
        return Err(format!(
            "refusing to replace immutable live rDPS validation report {} with different evidence",
            path.display()
        ));
    }
    write_rdps_validation_report_atomic(path, report)
}

fn write_json_atomic_with_limit(
    path: &Path,
    value: &impl Serialize,
    maximum_bytes: u64,
    label: &str,
) -> Result<(), String> {
    let mut encoded = serde_json::to_vec_pretty(value)
        .map_err(|error| format!("could not encode {label}: {error}"))?;
    encoded.push(b'\n');
    if encoded.len() as u64 > maximum_bytes {
        return Err(format!(
            "{label} exceeds its {}-byte safety limit",
            maximum_bytes
        ));
    }
    let partial = path.with_extension("partial");
    match std::fs::remove_file(&partial) {
        Ok(()) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => {
            return Err(format!("could not replace {label} partial: {error}"));
        }
    }
    let output = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&partial)
        .map_err(|error| format!("could not create {label} partial: {error}"))?;
    let write_result = (|| {
        let mut writer = BufWriter::new(output);
        writer.write_all(&encoded)?;
        writer.flush()?;
        writer.get_ref().sync_all()
    })();
    if let Err(error) = write_result {
        let _ = std::fs::remove_file(&partial);
        return Err(format!("could not persist {label}: {error}"));
    }
    let backup = path.with_extension("backup");
    let had_existing = path.exists();
    if had_existing {
        let _ = std::fs::remove_file(&backup);
        std::fs::rename(path, &backup)
            .map_err(|error| format!("could not stage prior {label}: {error}"))?;
    }
    if let Err(error) = std::fs::rename(&partial, path) {
        if had_existing {
            let _ = std::fs::rename(&backup, path);
        }
        let _ = std::fs::remove_file(&partial);
        return Err(format!("could not publish {label}: {error}"));
    }
    if had_existing {
        std::fs::remove_file(&backup)
            .map_err(|error| format!("could not remove {label} backup: {error}"))?;
    }
    Ok(())
}

fn read_rdps_validation_cumulative_report(
    path: &Path,
) -> Result<RdpsValidationCumulativeReport, String> {
    let bundle: RdpsValidationCumulativeReport = read_json_with_limit(
        path,
        MAXIMUM_RDPS_VALIDATION_REPORT_BYTES,
        "cumulative rDPS validation report",
    )?;
    if bundle.schema_version != RDPS_VALIDATION_CUMULATIVE_SCHEMA_VERSION {
        return Err(format!(
            "cumulative rDPS validation report {} uses unsupported schema {}",
            path.display(),
            bundle.schema_version
        ));
    }
    if bundle.manifest_game_build != bundle.report.manifest_game_build {
        return Err(format!(
            "cumulative rDPS validation report {} has mismatched manifest builds",
            path.display()
        ));
    }
    Ok(bundle)
}

fn write_rdps_validation_cumulative_report_atomic(
    path: &Path,
    bundle: &RdpsValidationCumulativeReport,
) -> Result<(), String> {
    write_json_atomic_with_limit(
        path,
        bundle,
        MAXIMUM_RDPS_VALIDATION_REPORT_BYTES,
        "cumulative rDPS validation report",
    )
}

fn update_rdps_validation_cumulative_from_sessions(
    directory: &Path,
    game_build: &str,
    cumulative_path: &Path,
) -> Result<RdpsValidationCumulativeReport, String> {
    let mut analyzer = RdpsValidationAnalyzer::bundled().map_err(|error| {
        format!("could not load the cumulative BPSR rDPS validation watch: {error}")
    })?;
    let initial = analyzer.report();
    if initial.manifest_game_build != game_build {
        return Err(format!(
            "bundled rDPS validation manifest build {} does not match active game build {game_build}",
            initial.manifest_game_build
        ));
    }

    let mut session_ids = BTreeSet::new();
    if cumulative_path.exists() {
        let cumulative = read_rdps_validation_cumulative_report(cumulative_path)?;
        if cumulative.manifest_game_build != game_build {
            return Err(format!(
                "cumulative rDPS validation manifest build {} does not match active game build {game_build}",
                cumulative.manifest_game_build
            ));
        }
        analyzer.merge_report(&cumulative.report).map_err(|error| {
            format!("could not resume cumulative rDPS validation evidence: {error}")
        })?;
        session_ids = cumulative.session_ids;
    }

    let final_suffix = format!(
        "-steam-{game_build}.v{}.validation.json",
        RDPS_VALIDATION_REPORT_SCHEMA_VERSION
    );
    let checkpoint_suffix = format!(
        "-steam-{game_build}.v{}.checkpoint.validation.json",
        RDPS_VALIDATION_REPORT_SCHEMA_VERSION
    );
    let mut pending_by_session = BTreeMap::<String, (bool, PathBuf)>::new();
    let entries = std::fs::read_dir(directory).map_err(|error| {
        format!(
            "could not enumerate live rDPS validation reports in {}: {error}",
            directory.display()
        )
    })?;
    for entry in entries {
        let entry = entry.map_err(|error| {
            format!("could not inspect a live rDPS validation directory entry: {error}")
        })?;
        if !entry
            .file_type()
            .map_err(|error| format!("could not inspect validation entry type: {error}"))?
            .is_file()
        {
            continue;
        }
        let name = entry.file_name().into_string().map_err(|_| {
            format!(
                "live rDPS validation report name in {} is not Unicode",
                directory.display()
            )
        })?;
        let (session_id, is_final) = if let Some(session_id) = name.strip_suffix(&checkpoint_suffix)
        {
            (session_id, false)
        } else if let Some(session_id) = name.strip_suffix(&final_suffix) {
            (session_id, true)
        } else {
            continue;
        };
        if session_id.is_empty() {
            return Err(format!(
                "live rDPS validation report {name} has no session ID"
            ));
        }
        if !session_ids.contains(session_id) {
            let candidate = (is_final, entry.path());
            match pending_by_session.entry(session_id.to_string()) {
                std::collections::btree_map::Entry::Vacant(entry) => {
                    entry.insert(candidate);
                }
                std::collections::btree_map::Entry::Occupied(mut entry)
                    if is_final && !entry.get().0 =>
                {
                    entry.insert(candidate);
                }
                _ => {}
            }
        }
    }

    for (session_id, (_, path)) in pending_by_session {
        let report = read_rdps_validation_report(&path)?;
        analyzer.merge_report(&report).map_err(|error| {
            format!(
                "could not merge live rDPS validation report {}: {error}",
                path.display()
            )
        })?;
        session_ids.insert(session_id);
    }

    let report = analyzer.report();
    Ok(RdpsValidationCumulativeReport {
        schema_version: RDPS_VALIDATION_CUMULATIVE_SCHEMA_VERSION,
        manifest_game_build: report.manifest_game_build.clone(),
        session_ids,
        report,
    })
}

fn format_rdps_validation_remaining_domains(progress: &RdpsValidationProgress) -> String {
    const DOMAINS: [(&str, &str); 5] = [
        ("psychoscope-factor", "factors"),
        ("offensive-runtime-gate", "offense"),
        ("mastery-property", "mastery"),
        ("packet-output-route", "packet formulas"),
        ("target-mitigation", "mitigation"),
    ];
    DOMAINS
        .into_iter()
        .filter_map(|(key, label)| {
            progress.by_domain.get(key).map(|domain| {
                let open = domain
                    .no_candidate_evidence
                    .saturating_add(domain.partial_candidate_event_coverage);
                format!(
                    "{label} {open}/{} open ({} partial)",
                    domain.total, domain.partial_candidate_event_coverage
                )
            })
        })
        .collect::<Vec<_>>()
        .join(", ")
}

fn replay_builtins_and_build_artifact(
    path: &Path,
) -> Result<(PluginRunReport, PluginRunReport, LocalLogArtifact), String> {
    let file =
        File::open(path).map_err(|error| format!("could not open {}: {error}", path.display()))?;
    let mut tracked = LogArtifactTrackingReader::new(file, ArtifactBuildLimits::default())
        .map_err(|error| format!("could not prepare sealed upload artifact: {error}"))?;
    let encounter_config = bundled_run_reducer_config()
        .map_err(|error| format!("could not load BPSR run rules: {error}"))?;
    let (header, combat_plugin, encounter_recorder) = replay_rlog_pair(
        BufReader::new(&mut tracked),
        bpsr_combat_timeline_plugin()?,
        EncounterRecorderPlugin::new(encounter_config),
        RlogLimits::default(),
        PluginRunLimits::default(),
        PluginRunLimits::default(),
    )
    .map_err(|error| format!("built-in plug-in replay failed: {error}"))?;
    let upload_artifact = tracked
        .finish(header, combat_plugin.rlog.clone())
        .map_err(|error| format!("could not build sealed upload artifact: {error}"))?;
    verify_replay_artifact(&combat_plugin, &encounter_recorder, &upload_artifact)?;
    Ok((combat_plugin, encounter_recorder, upload_artifact))
}

fn bpsr_combat_timeline_plugin() -> Result<CombatTimelinePlugin, String> {
    bpsr_combat_timeline_plugin_with_remote_factors(None)
}

fn bpsr_combat_timeline_plugin_with_remote_factors(
    remote_factors: Option<BpsrRemoteFactorTimeline>,
) -> Result<CombatTimelinePlugin, String> {
    let projector = match remote_factors {
        Some(remote_factors) => {
            BpsrStateDamageContributionProjector::new_with_remote_factor_timeline(remote_factors)?
        }
        None => BpsrStateDamageContributionProjector::new_live()?,
    };
    Ok(CombatTimelinePlugin::with_damage_contribution_projection(
        confirmed_damage_contribution_rules()?,
        Some(Box::new(projector)),
    )?
    .with_live_health_attributes(LiveHealthAttributeMapping {
        current_hp: 11_310,
        max_hp: 11_320,
    }))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum HistoryRdpsRefresh {
    Current,
    Refreshed,
    Deferred,
}

#[cfg(test)]
fn replay_bpsr_combat_history(path: &Path) -> Result<CombatHistorySnapshot, String> {
    replay_bpsr_combat_history_interruptible(path, || false, |_, _, _| {})?.ok_or_else(|| {
        "sealed combat history replay was unexpectedly deferred without live capture".into()
    })
}

#[cfg(test)]
fn replay_bpsr_combat_history_live_one_pass(path: &Path) -> Result<CombatHistorySnapshot, String> {
    let file = File::open(path).map_err(|error| {
        format!(
            "could not open sealed combat log {}: {error}",
            path.display()
        )
    })?;
    let mut reader = RlogReader::new(BufReader::new(file), RlogLimits::default())
        .map_err(|error| format!("could not validate combat log header: {error}"))?;
    let header = reader.header().clone();
    let mut meter = bpsr_combat_timeline_plugin()?;
    meter.begin_live(&header);
    let mut encounter = EncounterRecorderPlugin::new(
        bundled_run_reducer_config()
            .map_err(|error| format!("could not load BPSR run rules: {error}"))?,
    );
    encounter.begin_live(&header);
    while let Some(event) = reader
        .next_event()
        .map_err(|error| format!("sealed one-pass live replay failed: {error}"))?
    {
        meter.observe_live(&event);
        encounter
            .observe_live(&event)
            .map_err(|error| format!("encounter replay failed: {error}"))?;
    }
    if reader.summary().is_none() {
        return Err("sealed combat log has no validated integrity summary".into());
    }
    let run_projection = encounter
        .live_snapshot()
        .map_err(|error| format!("could not project replayed runs: {error}"))?;
    meter
        .history_snapshot(&run_projection.runs)
        .map_err(|error| format!("could not project one-pass live combat history: {error}"))
}

struct ProgressTrackingBufRead<R> {
    inner: R,
    consumed: Arc<AtomicU64>,
}

impl<R> ProgressTrackingBufRead<R> {
    fn new(inner: R, consumed: Arc<AtomicU64>) -> Self {
        Self { inner, consumed }
    }
}

impl<R: Read> Read for ProgressTrackingBufRead<R> {
    fn read(&mut self, buffer: &mut [u8]) -> std::io::Result<usize> {
        let read = self.inner.read(buffer)?;
        self.consumed
            .fetch_add(u64::try_from(read).unwrap_or(u64::MAX), Ordering::Relaxed);
        Ok(read)
    }
}

impl<R: BufRead> BufRead for ProgressTrackingBufRead<R> {
    fn fill_buf(&mut self) -> std::io::Result<&[u8]> {
        self.inner.fill_buf()
    }

    fn consume(&mut self, amount: usize) {
        self.inner.consume(amount);
        self.consumed
            .fetch_add(u64::try_from(amount).unwrap_or(u64::MAX), Ordering::Relaxed);
    }
}

fn replay_bpsr_combat_history_interruptible(
    path: &Path,
    should_defer: impl Fn() -> bool,
    mut on_progress: impl FnMut(u64, u64, u64),
) -> Result<Option<CombatHistorySnapshot>, String> {
    if should_defer() {
        return Ok(None);
    }
    let total_bytes = path.metadata().map(|metadata| metadata.len()).unwrap_or(0);
    let total_work_bytes = total_bytes.saturating_mul(2);
    let inference_file = File::open(path).map_err(|error| {
        format!(
            "could not open sealed combat log for remote-factor reconstruction {}: {error}",
            path.display()
        )
    })?;
    let inference_consumed = Arc::new(AtomicU64::new(0));
    let inference_tracked = ProgressTrackingBufRead::new(
        BufReader::new(inference_file),
        Arc::clone(&inference_consumed),
    );
    let mut inference_reader = RlogReader::new(inference_tracked, RlogLimits::default())
        .map_err(|error| format!("could not validate combat log header: {error}"))?;
    let mut remote_factor_learner = BpsrRemoteFactorLearner::new()?;
    let mut progress_event_count = 0_u64;
    while let Some(event) = inference_reader
        .next_event()
        .map_err(|error| format!("remote-factor reconstruction replay failed: {error}"))?
    {
        remote_factor_learner.observe(&event);
        progress_event_count = progress_event_count.saturating_add(1);
        if progress_event_count % 4_096 == 0 {
            on_progress(
                progress_event_count,
                inference_consumed.load(Ordering::Relaxed).min(total_bytes),
                total_work_bytes,
            );
            if should_defer() {
                return Ok(None);
            }
        }
    }
    if inference_reader.summary().is_none() {
        return Err("sealed combat log has no validated integrity summary".into());
    }
    let remote_factors = remote_factor_learner.finish();
    if should_defer() {
        return Ok(None);
    }
    let file = File::open(path).map_err(|error| {
        format!(
            "could not open sealed combat log {}: {error}",
            path.display()
        )
    })?;
    let consumed = Arc::new(AtomicU64::new(0));
    let tracked = ProgressTrackingBufRead::new(BufReader::new(file), Arc::clone(&consumed));
    let mut reader = RlogReader::new(tracked, RlogLimits::default())
        .map_err(|error| format!("could not validate combat log header: {error}"))?;
    let header = reader.header().clone();
    let mut meter = bpsr_combat_timeline_plugin_with_remote_factors(Some(remote_factors))?;
    meter.begin_live(&header);
    let mut encounter = EncounterRecorderPlugin::new(
        bundled_run_reducer_config()
            .map_err(|error| format!("could not load BPSR run rules: {error}"))?,
    );
    encounter.begin_live(&header);
    let mut event_count = 0_u64;
    while let Some(event) = reader
        .next_event()
        .map_err(|error| format!("sealed combat log replay failed: {error}"))?
    {
        meter.observe_live(&event);
        encounter
            .observe_live(&event)
            .map_err(|error| format!("encounter replay failed: {error}"))?;
        event_count = event_count.saturating_add(1);
        if event_count % 4_096 == 0 {
            progress_event_count = progress_event_count.saturating_add(4_096);
            on_progress(
                progress_event_count,
                total_bytes.saturating_add(consumed.load(Ordering::Relaxed).min(total_bytes)),
                total_work_bytes,
            );
            if should_defer() {
                return Ok(None);
            }
        }
    }
    on_progress(
        progress_event_count.saturating_add(event_count % 4_096),
        total_bytes.saturating_add(consumed.load(Ordering::Relaxed).min(total_bytes)),
        total_work_bytes,
    );
    if reader.summary().is_none() {
        return Err("sealed combat log has no validated integrity summary".into());
    }
    if should_defer() {
        return Ok(None);
    }
    let run_projection = encounter
        .live_snapshot()
        .map_err(|error| format!("could not project replayed runs: {error}"))?;
    meter
        .history_snapshot(&run_projection.runs)
        .map(Some)
        .map_err(|error| format!("could not project replayed combat history: {error}"))
}

fn bounded_history_rdps_failure_detail(error: &str) -> String {
    const MAXIMUM_DETAIL_CHARS: usize = 600;
    let normalized = error.split_whitespace().collect::<Vec<_>>().join(" ");
    if normalized.chars().count() <= MAXIMUM_DETAIL_CHARS {
        return normalized;
    }
    normalized
        .chars()
        .take(MAXIMUM_DETAIL_CHARS.saturating_sub(1))
        .chain(std::iter::once('…'))
        .collect()
}

fn clear_history_rdps_projection(snapshot: &mut CombatHistorySnapshot, status: &str) {
    snapshot.rdps_formula_identity = None;
    for run in &mut snapshot.runs {
        run.rdps_status = status.into();
        for view in &mut run.views {
            view.damage_influences.clear();
            for actor in &mut view.actors {
                actor.rdps = None;
                actor.rdps_damage = None;
                actor.rdps_contribution_given = None;
                actor.rdps_contribution_received = None;
                actor.rdps_incomplete = false;
            }
        }
    }
}

fn mark_history_rdps_projection_refreshing(snapshot: &mut CombatHistorySnapshot, status: &str) {
    // A saved projection was already conservation-validated before it was
    // committed to history. Keep that packet-proven subtotal visible while a
    // newer formula identity is replayed, then replace it atomically after the
    // new projection passes the same validation. An absent projection remains
    // absent because its formula identity and rDPS fields are already null.
    for run in &mut snapshot.runs {
        run.rdps_status = status.into();
    }
}

fn combat_timeline_snapshot(report: &PluginRunReport) -> Result<CombatTimelineSnapshot, String> {
    let mut matching = report.outputs.iter().filter_map(|output| match output {
        PluginOutput::Snapshot {
            schema_id,
            schema_version,
            payload,
        } if schema_id == COMBAT_SNAPSHOT_SCHEMA_ID => Some((*schema_version, payload)),
        _ => None,
    });
    let (schema_version, payload) = matching
        .next()
        .ok_or_else(|| "Combat Meter returned no combat timeline snapshot".to_owned())?;
    if matching.next().is_some() {
        return Err("Combat Meter returned multiple combat timeline snapshots".into());
    }
    if schema_version != COMBAT_SNAPSHOT_SCHEMA_VERSION {
        return Err(format!(
            "unsupported combat snapshot schema {schema_version}; expected {COMBAT_SNAPSHOT_SCHEMA_VERSION}"
        ));
    }
    let snapshot: CombatTimelineSnapshot = serde_json::from_value(payload.clone())
        .map_err(|error| format!("combat timeline snapshot is invalid: {error}"))?;
    if snapshot.schema_version != COMBAT_SNAPSHOT_SCHEMA_VERSION {
        return Err(format!(
            "combat timeline payload schema {} does not match output schema {schema_version}",
            snapshot.schema_version
        ));
    }
    Ok(snapshot)
}

fn run_projection_snapshot(report: &PluginRunReport) -> Result<RunProjectionSnapshot, String> {
    let mut matching = report.outputs.iter().filter_map(|output| match output {
        PluginOutput::Snapshot {
            schema_id,
            schema_version,
            payload,
        } if schema_id == RUN_PROJECTION_SCHEMA_ID => Some((*schema_version, payload)),
        _ => None,
    });
    let (schema_version, payload) = matching
        .next()
        .ok_or_else(|| "Encounter Recorder returned no run projection snapshot".to_owned())?;
    if matching.next().is_some() {
        return Err("Encounter Recorder returned multiple run projection snapshots".into());
    }
    if schema_version != RUN_PROJECTION_SCHEMA_VERSION {
        return Err(format!(
            "unsupported run projection schema {schema_version}; expected {RUN_PROJECTION_SCHEMA_VERSION}"
        ));
    }
    let snapshot: RunProjectionSnapshot = serde_json::from_value(payload.clone())
        .map_err(|error| format!("run projection snapshot is invalid: {error}"))?;
    if snapshot.schema_version != RUN_PROJECTION_SCHEMA_VERSION {
        return Err(format!(
            "run projection payload schema {} does not match output schema {schema_version}",
            snapshot.schema_version
        ));
    }
    Ok(snapshot)
}

fn build_upload_artifact(path: &Path) -> Result<LocalLogArtifact, String> {
    let file =
        File::open(path).map_err(|error| format!("could not open {}: {error}", path.display()))?;
    build_sealed_log_artifact(file, ArtifactBuildLimits::default(), RlogLimits::default())
        .map_err(|error| format!("could not build sealed upload artifact: {error}"))
}

fn verify_replay_artifact(
    combat_plugin: &PluginRunReport,
    encounter_recorder: &PluginRunReport,
    artifact: &LocalLogArtifact,
) -> Result<(), String> {
    let expected = &artifact.rlog.content_sha256;
    for (name, actual) in [
        ("combat plug-in", &combat_plugin.rlog.content_sha256),
        (
            "encounter recorder",
            &encounter_recorder.rlog.content_sha256,
        ),
    ] {
        if actual != expected {
            return Err(format!(
                "{name} replay digest {actual} does not match upload artifact digest {expected}"
            ));
        }
    }
    Ok(())
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

fn validate_local_artifact_path(value: &str) -> Result<(), String> {
    let value = value.trim();
    if value.is_empty() || value.len() > MAXIMUM_LOCAL_ARTIFACT_PATH_BYTES || value.contains('\0') {
        return Err(format!(
            "local artifact path must use 1-{MAXIMUM_LOCAL_ARTIFACT_PATH_BYTES} non-NUL bytes"
        ));
    }
    Ok(())
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
    let game_processes = discover_game_processes().unwrap_or_default();
    let mut capture_interfaces = dumpcap
        .as_deref()
        .and_then(|path| discover_capture_interfaces(path).ok())
        .unwrap_or_default();
    #[cfg(windows)]
    let recommendation =
        enrich_windows_capture_interfaces(&mut capture_interfaces, &game_processes);
    #[cfg(not(windows))]
    let recommendation: Option<(String, &'static str, String)> = None;
    RuntimeEnvironment {
        platform: std::env::consts::OS,
        game_processes,
        dumpcap_path: dumpcap.as_deref().map(display_path),
        capture_interfaces,
        recommended_capture_interface: recommendation.as_ref().map(|(value, _, _)| value.clone()),
        recommended_capture_source: recommendation.as_ref().map(|(_, source, _)| *source),
        recommended_capture_reason: recommendation.map(|(_, _, reason)| reason),
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
                friendly_name: None,
                description: None,
                mac_address: None,
                is_up: None,
                is_virtual: None,
                recommendation: None,
            })
        })
        .collect())
}

#[cfg(windows)]
fn enrich_windows_capture_interfaces(
    interfaces: &mut [CaptureInterfaceView],
    game_processes: &[GameProcessView],
) -> Option<(String, &'static str, String)> {
    let adapters = windows_capture_adapters().ok()?;
    let process_ids = game_processes
        .iter()
        .map(|process| process.process_id)
        .collect::<Vec<_>>();
    let recommendation = recommend_windows_capture_adapter(&adapters, &process_ids);
    let recommended_key = recommendation
        .as_ref()
        .and_then(|value| normalized_adapter_key(&value.adapter_name));
    let adapter_by_key = adapters
        .iter()
        .filter_map(|adapter| {
            normalized_adapter_key(&adapter.adapter_name).map(|key| (key, adapter))
        })
        .collect::<BTreeMap<_, _>>();
    let mut recommended_interface = None;
    let mut recommended_adapter = None;

    for interface in interfaces {
        let Some(key) = normalized_adapter_key(&interface.label) else {
            continue;
        };
        let Some(adapter) = adapter_by_key.get(&key).copied() else {
            continue;
        };
        let is_recommended = recommended_key.as_deref() == Some(key.as_str());
        let recommendation_label = recommendation.as_ref().and_then(|value| {
            is_recommended.then_some(match value.source {
                WindowsCaptureAdapterRecommendationSource::GameTraffic => "game_traffic",
                WindowsCaptureAdapterRecommendationSource::SystemRoute => "system_route",
            })
        });
        interface.friendly_name = nonempty(adapter.friendly_name.as_str());
        interface.description = nonempty(adapter.description.as_str());
        interface.mac_address = format_physical_address(&adapter.physical_address);
        interface.is_up = Some(adapter.operational);
        interface.is_virtual = Some(is_likely_virtual_adapter(adapter));
        interface.recommendation = recommendation_label;
        interface.label = capture_interface_label(interface, adapter);
        if is_recommended {
            recommended_interface = Some(interface.value.clone());
            recommended_adapter = Some(adapter);
        }
    }

    let (interface_value, adapter, recommendation) =
        match (recommended_interface, recommended_adapter, recommendation) {
            (Some(interface_value), Some(adapter), Some(recommendation)) => {
                (interface_value, adapter, recommendation)
            }
            _ => return None,
        };
    let source = match recommendation.source {
        WindowsCaptureAdapterRecommendationSource::GameTraffic => "game_traffic",
        WindowsCaptureAdapterRecommendationSource::SystemRoute => "system_route",
    };
    let adapter_name = adapter_display_name(adapter);
    let reason = match recommendation.source {
        WindowsCaptureAdapterRecommendationSource::GameTraffic => format!(
            "{adapter_name} carries BPSR traffic (matched {} active game connection{}).",
            recommendation.matched_game_connections,
            if recommendation.matched_game_connections == 1 {
                ""
            } else {
                "s"
            }
        ),
        WindowsCaptureAdapterRecommendationSource::SystemRoute => format!(
            "{adapter_name} is Windows' active routed adapter. No active BPSR connection was available for a direct match."
        ),
    };
    Some((interface_value, source, reason))
}

#[cfg(windows)]
fn normalized_adapter_key(value: &str) -> Option<String> {
    let value = value.to_ascii_lowercase();
    let start = value
        .find('{')
        .map(|index| index + 1)
        .or_else(|| value.find("npf_").map(|index| index + "npf_".len()))?;
    let key = value[start..]
        .chars()
        .take_while(|character| character.is_ascii_hexdigit() || *character == '-')
        .collect::<String>();
    (key.len() == 36 && key.bytes().filter(|byte| *byte == b'-').count() == 4).then_some(key)
}

#[cfg(windows)]
fn nonempty(value: &str) -> Option<String> {
    (!value.trim().is_empty()).then(|| value.trim().to_owned())
}

#[cfg(windows)]
fn adapter_display_name(adapter: &WindowsCaptureAdapter) -> &str {
    if adapter.friendly_name.trim().is_empty() {
        adapter.description.as_str()
    } else {
        adapter.friendly_name.as_str()
    }
}

#[cfg(windows)]
fn format_physical_address(address: &[u8]) -> Option<String> {
    (!address.is_empty()).then(|| {
        address
            .iter()
            .map(|byte| format!("{byte:02X}"))
            .collect::<Vec<_>>()
            .join(":")
    })
}

#[cfg(windows)]
fn is_likely_virtual_adapter(adapter: &WindowsCaptureAdapter) -> bool {
    let searchable = format!(
        "{} {}",
        adapter.friendly_name.to_ascii_lowercase(),
        adapter.description.to_ascii_lowercase()
    );
    [
        "virtual",
        "vpn",
        "tunnel",
        "loopback",
        "speedify",
        "wireguard",
        "openvpn",
        "tailscale",
    ]
    .iter()
    .any(|marker| searchable.contains(marker))
}

#[cfg(windows)]
fn capture_interface_label(
    interface: &CaptureInterfaceView,
    adapter: &WindowsCaptureAdapter,
) -> String {
    let mut parts = vec![interface.value.clone()];
    let friendly_name = adapter_display_name(adapter).trim();
    if !friendly_name.is_empty() {
        parts.push(friendly_name.to_owned());
    }
    let description = adapter.description.trim();
    if !description.is_empty() && !description.eq_ignore_ascii_case(friendly_name) {
        parts.push(description.to_owned());
    }
    if let Some(mac_address) = interface.mac_address.as_deref() {
        parts.push(format!("MAC {mac_address}"));
    }
    let mut state = Vec::new();
    if let Some(recommendation) = interface.recommendation {
        state.push(match recommendation {
            "game_traffic" => "Recommended: BPSR traffic",
            _ => "Recommended: active route",
        });
    }
    state.push(if adapter.operational {
        "active"
    } else {
        "disconnected"
    });
    if interface.is_virtual == Some(true) {
        state.push("virtual");
    }
    format!("{} [{}]", parts.join(" — "), state.join(", "))
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

#[cfg(windows)]
fn process_executable_path(process_id: u32) -> Result<PathBuf, String> {
    // SAFETY: the requested access is read-only, the handle is checked, and it
    // is closed exactly once before this function returns.
    let process = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, process_id) };
    if process.is_null() {
        return Err(format!(
            "could not inspect BPSR process {process_id}: {}",
            std::io::Error::last_os_error()
        ));
    }
    let result = (|| {
        let mut path = vec![0_u16; 32_768];
        let mut length = u32::try_from(path.len()).expect("Windows path buffer fits u32");
        // SAFETY: the buffer is writable for `length` UTF-16 units and the
        // valid process handle remains open throughout the call.
        if unsafe { QueryFullProcessImageNameW(process, 0, path.as_mut_ptr(), &mut length) } == 0 {
            return Err(format!(
                "could not resolve BPSR executable path for process {process_id}: {}",
                std::io::Error::last_os_error()
            ));
        }
        path.truncate(length as usize);
        Ok(PathBuf::from(String::from_utf16_lossy(&path)))
    })();
    unsafe {
        CloseHandle(process);
    }
    result
}

#[cfg(not(windows))]
fn discover_game_processes() -> Result<Vec<GameProcessView>, String> {
    Ok(Vec::new())
}

fn enrich_bpsr_history_presentation(
    snapshot: &mut CombatHistorySnapshot,
    locale: &str,
) -> Result<(), String> {
    let run_identities = bundled_scene_run_identities()
        .map_err(|error| format!("could not load BPSR run identity rules: {error}"))?;
    for run in &mut snapshot.runs {
        let observed_specializations = observed_run_specializations(run)?;
        enrich_bpsr_scene_run_identity(
            run.scene_id,
            &mut run.activity_id,
            &mut run.activity_family_id,
            &mut run.difficulty_family,
            &run_identities,
        );
        run.presentation_scene_name = run
            .scene_id
            .map(|scene_id| localized_scene_name(i64::from(scene_id), locale))
            .transpose()?
            .flatten()
            .map(str::to_owned);
        for view in &mut run.views {
            for actor in &mut view.actors {
                let ability_ids = actor
                    .abilities
                    .iter()
                    .filter_map(|ability| ability.ability_id.parse::<i64>().ok())
                    .collect::<BTreeSet<_>>();
                if let Some((class_id, specialization_id)) =
                    observed_specializations.get(&actor.entity_uuid)
                {
                    // Captured combat abilities describe this run. They must
                    // override a stale or later profile snapshot rather than
                    // letting current equipment rewrite historical identity.
                    actor.class_id = Some(*class_id);
                    actor.specialization_id = Some(*specialization_id);
                }
                let combat_presentation = resolve_actor_combat_presentation(
                    actor.class_id,
                    actor.specialization_id,
                    ability_ids.iter().copied(),
                    locale,
                )?;
                actor.class_id = combat_presentation.class_id;
                actor.specialization_id = combat_presentation.specialization_id;
                enrich_bpsr_actor_combat_presentation(actor, locale)?;
                if actor.actor_kind.as_deref() != Some("player") {
                    actor.presentation_name = if actor.actor_kind.as_deref() == Some("monster") {
                        actor
                            .monster_id
                            .as_deref()
                            .map(|monster_id| localized_bpsr_monster_name(monster_id, locale))
                            .transpose()?
                            .flatten()
                    } else {
                        None
                    }
                    .or_else(|| actor.display_name.clone());
                    continue;
                }
                let entity_uuid = actor.entity_uuid.parse::<i64>().map_err(|error| {
                    format!(
                        "history actor {} has an invalid entity UUID: {error}",
                        actor.actor_id
                    )
                })?;
                actor.character_id = character_id_from_entity_uuid(entity_uuid);
                enrich_bpsr_loadout_presentation(&mut actor.primary_loadout, locale)?;
                enrich_bpsr_loadout_presentation(&mut actor.auxiliary_loadout, locale)?;
                enrich_bpsr_weapon_presentation(actor);

                let class_named_companion = match (actor.class_id, actor.display_name.as_deref()) {
                    (Some(class_id), Some(display_name)) => {
                        is_localized_class_name(class_id, display_name)?
                    }
                    _ => false,
                };
                if class_named_companion {
                    let companion_presentation = resolve_actor_combat_presentation(
                        actor.class_id,
                        None,
                        std::iter::empty(),
                        locale,
                    )?;
                    actor.presentation_kind = Some("party_npc".into());
                    actor.presentation_class_name = companion_presentation.class_name.clone();
                    actor.presentation_specialization_name = None;
                    actor.icon_asset_path = bpsr_game_asset_path(companion_presentation.icon);
                    actor.presentation_role = companion_presentation.role;
                    actor.presentation_accent = companion_presentation.accent;
                    actor.presentation_name = companion_presentation
                        .class_name
                        .or_else(|| actor.display_name.clone());
                    continue;
                }
                actor.presentation_class_name = combat_presentation.class_name;
                actor.presentation_specialization_name = combat_presentation.specialization_name;
                actor.icon_asset_path = bpsr_game_asset_path(combat_presentation.icon);
                actor.presentation_role = combat_presentation.role;
                actor.presentation_accent = combat_presentation.accent;
                actor.presentation_kind = Some("player".into());
                actor.presentation_name = actor.display_name.clone();
            }
            for target in &mut view.targets {
                target.presentation_name = if target.actor_kind.as_deref() == Some("monster") {
                    target
                        .monster_id
                        .as_deref()
                        .map(|monster_id| localized_bpsr_monster_name(monster_id, locale))
                        .transpose()?
                        .flatten()
                } else {
                    None
                }
                .or_else(|| target.display_name.clone());
            }
        }
    }
    Ok(())
}

fn freeze_bpsr_history_character_state(
    snapshot: &mut CombatHistorySnapshot,
    identities: &impl CharacterIdentityResolver,
) -> Result<(), String> {
    for run in &mut snapshot.runs {
        for view in &mut run.views {
            for actor in &mut view.actors {
                if actor.actor_kind.as_deref() != Some("player") {
                    continue;
                }
                if actor.character_id.is_none() {
                    let entity_uuid = actor.entity_uuid.parse::<i64>().map_err(|error| {
                        format!(
                            "history actor {} has an invalid entity UUID: {error}",
                            actor.actor_id
                        )
                    })?;
                    actor.character_id = character_id_from_entity_uuid(entity_uuid);
                }
                let Some(identity) = actor.character_id.as_deref().and_then(|character_id| {
                    identities.resolve_identity(
                        &snapshot.deployment_id,
                        &snapshot.region_id,
                        snapshot.world_id.as_deref(),
                        character_id,
                    )
                }) else {
                    continue;
                };

                if actor_display_name_needs_identity(actor.display_name.as_deref()) {
                    actor.display_name = Some(identity.display_name.clone());
                }
                if actor.class_id.is_none() {
                    actor.class_id = identity.class_id;
                }
                if actor.specialization_id.is_none() {
                    actor.specialization_id = identity.specialization_id;
                }
                if actor.level.is_none() {
                    actor.level = identity.level;
                }
                if actor.ability_score.is_none() {
                    actor.ability_score = identity.ability_score;
                }
                if actor.weapon_item_id.is_none() {
                    actor.weapon_item_id = identity.weapon_item_id;
                }
                if actor.weapon_breakthrough_count.is_none() {
                    actor.weapon_breakthrough_count = identity.weapon_breakthrough_count;
                }
                if actor.seasonal_score.is_none() {
                    actor.seasonal_score = identity.seasonal_strength;
                }
                if actor.primary_loadout.is_empty() {
                    actor.primary_loadout = identity
                        .primary_loadout
                        .iter()
                        .map(HistoryLoadoutSlot::from)
                        .collect();
                }
                if actor.auxiliary_loadout.is_empty() {
                    actor.auxiliary_loadout = identity
                        .auxiliary_loadout
                        .iter()
                        .map(HistoryLoadoutSlot::from)
                        .collect();
                }
            }
        }
    }
    Ok(())
}

/// Resolve only public display labels for saved history rows.
///
/// Character names are keyed by the public character UID and are presentation
/// metadata. All mutable combat state remains exactly as captured in the run;
/// this function intentionally cannot copy class, specialization, equipment,
/// scores, or loadouts from the latest profile catalog.
fn enrich_bpsr_history_public_names(
    snapshot: &mut CombatHistorySnapshot,
    identities: &impl CharacterIdentityResolver,
) -> Result<(), String> {
    let deployment_id = snapshot.deployment_id.clone();
    let region_id = snapshot.region_id.clone();
    let world_id = snapshot.world_id.clone();
    for run in &mut snapshot.runs {
        for view in &mut run.views {
            for actor in &mut view.actors {
                if actor.actor_kind.as_deref() != Some("player")
                    || !actor_display_name_needs_identity(actor.display_name.as_deref())
                {
                    continue;
                }
                if actor.character_id.is_none() {
                    let entity_uuid = actor.entity_uuid.parse::<i64>().map_err(|error| {
                        format!(
                            "history actor {} has an invalid entity UUID: {error}",
                            actor.actor_id
                        )
                    })?;
                    actor.character_id = character_id_from_entity_uuid(entity_uuid);
                }
                let character_id = actor.character_id.clone();
                let Some(identity) = character_id.as_deref().and_then(|character_id| {
                    identities.resolve_identity(
                        &deployment_id,
                        &region_id,
                        world_id.as_deref(),
                        character_id,
                    )
                }) else {
                    if let Some(character_id) = character_id {
                        actor.display_name = Some(format!("UID {character_id}"));
                    }
                    continue;
                };
                actor.display_name = Some(identity.display_name.clone());
            }
        }
    }
    Ok(())
}

fn enrich_bpsr_catalog_public_names(
    catalog: &mut CombatHistoryCatalog,
    identities: &impl CharacterIdentityResolver,
) -> Result<(), String> {
    for entry in &mut catalog.entries {
        for actor in &mut entry.participants {
            if actor.actor_kind.as_deref() != Some("player")
                || !actor_display_name_needs_identity(actor.display_name.as_deref())
            {
                continue;
            }
            if actor.character_id.is_none() {
                let entity_uuid = actor.entity_uuid.parse::<i64>().map_err(|error| {
                    format!(
                        "history actor {} has an invalid entity UUID: {error}",
                        actor.actor_id
                    )
                })?;
                actor.character_id = character_id_from_entity_uuid(entity_uuid);
            }
            let character_id = actor.character_id.clone();
            let Some(identity) = character_id.as_deref().and_then(|character_id| {
                identities.resolve_identity(
                    &entry.deployment_id,
                    &entry.region_id,
                    entry.world_id.as_deref(),
                    character_id,
                )
            }) else {
                if let Some(character_id) = character_id {
                    actor.display_name = Some(format!("UID {character_id}"));
                }
                continue;
            };
            actor.display_name = Some(identity.display_name.clone());
        }
    }
    Ok(())
}

/// Fill live rows from UID-matched character evidence without replacing any
/// identity or loadout that the current packet stream already supplied.
///
/// This lives in the BPSR desktop integration because deriving a character UID
/// from an entity UUID is game-specific. The Combat Meter reducer remains
/// game-neutral.
fn live_overlay_character_id(actor_kind: Option<&str>, entity_uuid: &str) -> Option<String> {
    // Entity UUIDs only encode a public character UID for confirmed player
    // actors. Applying that transform to unresolved damage targets fabricates
    // tiny character IDs for monsters and can later enrich them as players.
    if actor_kind != Some("player") {
        return None;
    }
    entity_uuid
        .parse::<i64>()
        .ok()
        .and_then(character_id_from_entity_uuid)
}

/// Return the UID encoded by a player-shaped entity UUID while an actor row is
/// still awaiting its canonical kind. This is only a lookup candidate: callers
/// must require an exact character-identity match before promoting an
/// unclassified row to `player`.
fn live_overlay_character_identity_candidate(
    actor_kind: Option<&str>,
    entity_uuid: &str,
) -> Option<String> {
    if actor_kind.is_some_and(|kind| kind != "player" && !kind.starts_with("unknown")) {
        return None;
    }
    entity_uuid
        .parse::<i64>()
        .ok()
        .and_then(character_id_from_entity_uuid)
}

fn live_overlay_topic_invalidates(topic: EventTopic) -> bool {
    matches!(
        topic,
        EventTopic::Actor
            | EventTopic::CharacterProfile
            | EventTopic::Combat
            | EventTopic::Encounter
            | EventTopic::Dungeon
            | EventTopic::DataQuality
    )
}

fn actor_display_name_needs_identity(name: Option<&str>) -> bool {
    let Some(name) = name.map(str::trim).filter(|name| !name.is_empty()) else {
        return true;
    };
    let lower = name.to_ascii_lowercase();
    if matches!(lower.as_str(), "player" | "actor") {
        return true;
    }
    ["player ", "actor ", "uid "].into_iter().any(|prefix| {
        lower.strip_prefix(prefix).is_some_and(|suffix| {
            !suffix.is_empty() && suffix.bytes().all(|byte| byte.is_ascii_digit())
        })
    })
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum LiveCharacterIdentityAuthority {
    /// Exact evidence decoded during the current capture. Present values replace
    /// an older persisted fallback but never erase fields omitted by the patch.
    CaptureTime,
    /// Last-known presentation used only until current-session evidence arrives.
    PersistentFallback,
}

fn enrich_bpsr_live_character_state(
    snapshot: &mut CombatTimelineSnapshot,
    identities: &impl CharacterIdentityResolver,
    authority: LiveCharacterIdentityAuthority,
) {
    let deployment_id = snapshot.deployment_id.clone();
    let region_id = snapshot.region_id.clone();
    let world_id = snapshot.world_id.clone();
    for actor in &mut snapshot.actors {
        let confirmed_player = actor.actor_kind.as_deref() == Some("player");
        let Some(character_id) = live_overlay_character_identity_candidate(
            actor.actor_kind.as_deref(),
            &actor.entity_uuid,
        ) else {
            continue;
        };
        let Some(identity) = identities.resolve_identity(
            &deployment_id,
            &region_id,
            world_id.as_deref(),
            &character_id,
        ) else {
            if confirmed_player && actor_display_name_needs_identity(actor.display_name.as_deref())
            {
                actor.display_name = Some(format!("UID {character_id}"));
            }
            continue;
        };

        // An exact deployment/region/world/UID match is positive player
        // evidence. This lets capture-time profile and loadout data enrich the
        // live row before a later AOI update/finalizer supplies ActorKind.
        actor.actor_kind = Some("player".into());
        let replace_existing = authority == LiveCharacterIdentityAuthority::CaptureTime;
        if replace_existing || actor_display_name_needs_identity(actor.display_name.as_deref()) {
            actor.display_name = Some(identity.display_name.clone());
        }
        if identity.class_id.is_some() && (replace_existing || actor.class_id.is_none()) {
            actor.class_id = identity.class_id;
        }
        if identity.specialization_id.is_some()
            && (replace_existing || actor.specialization_id.is_none())
        {
            actor.specialization_id = identity.specialization_id;
        }
        if identity.level.is_some() && (replace_existing || actor.level.is_none()) {
            actor.level = identity.level;
        }
        if identity.ability_score.is_some() && (replace_existing || actor.ability_score.is_none()) {
            actor.ability_score = identity.ability_score;
        }
        if identity.weapon_item_id.is_some() && (replace_existing || actor.weapon_item_id.is_none())
        {
            actor.weapon_item_id = identity.weapon_item_id;
        }
        if identity.weapon_breakthrough_count.is_some()
            && (replace_existing || actor.weapon_breakthrough_count.is_none())
        {
            actor.weapon_breakthrough_count = identity.weapon_breakthrough_count;
        }
        if identity.seasonal_strength.is_some()
            && (replace_existing || actor.seasonal_score.is_none())
        {
            actor.seasonal_score = identity.seasonal_strength;
        }
        if !identity.primary_loadout.is_empty()
            && (replace_existing || actor.primary_loadout.is_empty())
        {
            actor.primary_loadout.clone_from(&identity.primary_loadout);
        }
        if !identity.auxiliary_loadout.is_empty()
            && (replace_existing || actor.auxiliary_loadout.is_empty())
        {
            actor
                .auxiliary_loadout
                .clone_from(&identity.auxiliary_loadout);
        }
    }
}

fn observed_run_specializations(
    run: &rlogs_plugin_combat_meter::CombatRunHistory,
) -> Result<BTreeMap<String, (i32, i32)>, String> {
    let mut actor_evidence = BTreeMap::<String, (BTreeSet<i32>, BTreeSet<i64>)>::new();
    for view in &run.views {
        for actor in &view.actors {
            let evidence = actor_evidence
                .entry(actor.entity_uuid.clone())
                .or_insert_with(|| (BTreeSet::new(), BTreeSet::new()));
            if let Some(class_id) = actor.class_id {
                evidence.0.insert(class_id);
            }
            evidence.1.extend(
                actor
                    .abilities
                    .iter()
                    .filter_map(|ability| ability.ability_id.parse::<i64>().ok()),
            );
        }
    }

    let mut detected = BTreeMap::new();
    for (entity_uuid, (class_ids, ability_ids)) in actor_evidence {
        let supplied_class_id =
            (class_ids.len() == 1).then(|| *class_ids.first().expect("one observed class"));
        if class_ids.len() <= 1 {
            let identity = resolve_actor_combat_identity(
                supplied_class_id,
                None,
                ability_ids.iter().copied(),
            )?;
            if let (Some(class_id), Some(specialization_id)) =
                (identity.class_id, identity.specialization_id)
            {
                detected.insert(entity_uuid, (class_id, specialization_id));
            }
        }
    }
    Ok(detected)
}

fn enrich_bpsr_actor_combat_presentation(
    actor: &mut rlogs_plugin_combat_meter::HistoryActorSummary,
    locale: &str,
) -> Result<(), String> {
    for ability in &mut actor.abilities {
        let Ok(ability_id) = ability.ability_id.parse::<i64>() else {
            continue;
        };
        let Some(presentation) = combat_action_presentation(ability_id)? else {
            continue;
        };
        ability.presentation_name =
            localized_combat_action_name(ability_id, locale)?.map(str::to_owned);
        ability.presentation_kind = Some(presentation.kind.clone());
        ability.presentation_resolution = Some(presentation.resolution.clone());
        ability.icon_asset_path = presentation
            .icon
            .as_ref()
            .map(|path| format!("/game-assets/blue-protocol-star-resonance/shared/{path}"));
        ability.presentation_recount_group_id = presentation
            .recount_group_id
            .map(|group_id| group_id.to_string());
        ability.presentation_recount_group_name =
            localized_recount_group_name(ability_id, locale)?.map(str::to_owned);
    }
    for effect in &mut actor.effects {
        let Ok(effect_id) = effect.effect_id.parse::<i64>() else {
            continue;
        };
        let Some(presentation) = status_effect_presentation(effect_id)? else {
            continue;
        };
        effect.presentation_name = localized_status_effect_name(effect_id, locale)?
            .map(str::to_owned)
            .or_else(|| presentation.technical_name.clone());
        effect.presentation_kind = Some(presentation.kind.clone());
        effect.presentation_resolution = Some(presentation.resolution.clone());
        effect.icon_asset_path = presentation
            .icon
            .as_ref()
            .map(|path| format!("/game-assets/blue-protocol-star-resonance/shared/{path}"));
    }
    Ok(())
}

fn localized_bpsr_monster_name(monster_id: &str, locale: &str) -> Result<Option<String>, String> {
    let parsed = monster_id
        .parse::<i64>()
        .map_err(|error| format!("history monster ID {monster_id} is invalid: {error}"))?;
    localized_monster_name(parsed, locale).map(|name| name.map(str::to_owned))
}

fn enrich_bpsr_loadout_presentation(
    slots: &mut [HistoryLoadoutSlot],
    locale: &str,
) -> Result<(), String> {
    for slot in slots {
        let Some(skill_id) = slot.ability_id else {
            continue;
        };
        if let Some(presentation) = battle_imagine_presentation(skill_id)? {
            slot.item_id = Some(presentation.item_id);
            slot.item_tier = Some(presentation.item_tier);
            slot.maximum_tier = Some(presentation.maximum_tier);
            slot.presentation_name =
                localized_battle_imagine_name(presentation.item_id, locale)?.map(str::to_owned);
            slot.icon_asset_path = Some(format!(
                "/game-assets/blue-protocol-star-resonance/shared/{}",
                presentation.icon
            ));
            continue;
        }
        let Some(presentation) = auxiliary_action_presentation(skill_id)? else {
            continue;
        };
        slot.presentation_name =
            localized_auxiliary_action_name(skill_id, locale)?.map(str::to_owned);
        slot.icon_asset_path = Some(format!(
            "/game-assets/blue-protocol-star-resonance/shared/{}",
            presentation.icon
        ));
        if let Some(imagine_skill_id) = presentation.replacement_imagine_skill_id
            && let Some(imagine) = battle_imagine_presentation(imagine_skill_id)?
        {
            slot.item_id = Some(imagine.item_id);
            slot.item_tier = Some(imagine.item_tier);
            slot.maximum_tier = Some(imagine.maximum_tier);
        }
    }
    Ok(())
}

fn bpsr_game_asset_path(relative_path: Option<String>) -> Option<String> {
    relative_path.map(|path| format!("/game-assets/blue-protocol-star-resonance/shared/{path}"))
}

fn enrich_bpsr_weapon_presentation(actor: &mut rlogs_plugin_combat_meter::HistoryActorSummary) {
    actor.weapon_icon_asset_path = None;
    actor.weapon_presentation_name = None;
    actor.weapon_level = None;
    actor.weapon_level_min = None;
    actor.weapon_level_max = None;
    actor.weapon_badge_kind = None;
    let Some(item_id) = actor.weapon_item_id else {
        return;
    };
    let Some(metadata) = weapon_presentation(item_id) else {
        return;
    };
    let level = weapon_level_presentation(item_id, actor.weapon_breakthrough_count);
    actor.weapon_icon_asset_path = Some(format!(
        "/game-assets/blue-protocol-star-resonance/shared/{}",
        metadata.icon
    ));
    actor.weapon_presentation_name = Some(metadata.english_name.to_owned());
    actor.weapon_level = level.and_then(|value| value.exact);
    actor.weapon_level_min = level.map(|value| value.minimum);
    actor.weapon_level_max = level.map(|value| value.maximum);
    actor.weapon_badge_kind = Some(metadata.badge_kind.to_owned());
}

fn enrich_bpsr_participant_weapon_presentation(
    actor: &mut combat_history::CombatHistoryParticipant,
) {
    actor.weapon_icon_asset_path = None;
    actor.weapon_presentation_name = None;
    actor.weapon_level = None;
    actor.weapon_level_min = None;
    actor.weapon_level_max = None;
    actor.weapon_badge_kind = None;
    let Some(item_id) = actor.weapon_item_id else {
        return;
    };
    let Some(metadata) = weapon_presentation(item_id) else {
        return;
    };
    let level = weapon_level_presentation(item_id, actor.weapon_breakthrough_count);
    actor.weapon_icon_asset_path = Some(format!(
        "/game-assets/blue-protocol-star-resonance/shared/{}",
        metadata.icon
    ));
    actor.weapon_presentation_name = Some(metadata.english_name.to_owned());
    actor.weapon_level = level.and_then(|value| value.exact);
    actor.weapon_level_min = level.map(|value| value.minimum);
    actor.weapon_level_max = level.map(|value| value.maximum);
    actor.weapon_badge_kind = Some(metadata.badge_kind.to_owned());
}

fn enrich_bpsr_catalog_presentation(
    catalog: &mut CombatHistoryCatalog,
    locale: &str,
) -> Result<(), String> {
    let run_identities = bundled_scene_run_identities()
        .map_err(|error| format!("could not load BPSR run identity rules: {error}"))?;
    for entry in &mut catalog.entries {
        enrich_bpsr_scene_run_identity(
            entry.scene_id,
            &mut entry.activity_id,
            &mut entry.activity_family_id,
            &mut entry.difficulty_family,
            &run_identities,
        );
        entry.presentation_scene_name = entry
            .scene_id
            .map(|scene_id| localized_scene_name(i64::from(scene_id), locale))
            .transpose()?
            .flatten()
            .map(str::to_owned);
        for actor in &mut entry.participants {
            if actor.actor_kind.as_deref() != Some("player") {
                continue;
            }
            let entity_uuid = actor.entity_uuid.parse::<i64>().map_err(|error| {
                format!(
                    "history actor {} has an invalid entity UUID: {error}",
                    actor.actor_id
                )
            })?;
            actor.character_id = character_id_from_entity_uuid(entity_uuid);
            enrich_bpsr_loadout_presentation(&mut actor.primary_loadout, locale)?;
            enrich_bpsr_loadout_presentation(&mut actor.auxiliary_loadout, locale)?;
            enrich_bpsr_participant_weapon_presentation(actor);

            let combat_presentation = resolve_actor_combat_presentation(
                actor.class_id,
                actor.specialization_id,
                std::iter::empty(),
                locale,
            )?;
            actor.class_id = combat_presentation.class_id;
            actor.specialization_id = combat_presentation.specialization_id;

            let class_named_companion = match (actor.class_id, actor.display_name.as_deref()) {
                (Some(class_id), Some(display_name)) => {
                    is_localized_class_name(class_id, display_name)?
                }
                _ => false,
            };
            if class_named_companion {
                let companion_presentation = resolve_actor_combat_presentation(
                    actor.class_id,
                    None,
                    std::iter::empty(),
                    locale,
                )?;
                actor.presentation_kind = Some("party_npc".into());
                actor.presentation_class_name = companion_presentation.class_name.clone();
                actor.presentation_specialization_name = None;
                actor.icon_asset_path = bpsr_game_asset_path(companion_presentation.icon);
                actor.presentation_role = companion_presentation.role;
                actor.presentation_accent = companion_presentation.accent;
                actor.presentation_name = companion_presentation
                    .class_name
                    .or_else(|| actor.display_name.clone());
                continue;
            }
            actor.presentation_class_name = combat_presentation.class_name;
            actor.presentation_specialization_name = combat_presentation.specialization_name;
            actor.icon_asset_path = bpsr_game_asset_path(combat_presentation.icon);
            actor.presentation_role = combat_presentation.role;
            actor.presentation_accent = combat_presentation.accent;
            actor.presentation_kind = Some("player".into());
            actor.presentation_name = actor.display_name.clone();
        }
    }
    Ok(())
}

fn enrich_bpsr_scene_run_identity(
    scene_id: Option<i32>,
    activity_id: &mut Option<String>,
    activity_family_id: &mut Option<String>,
    difficulty_family: &mut Option<String>,
    scene_identities: &BTreeMap<i32, BpsrSceneRunIdentity>,
) {
    let Some(identity) = scene_id.and_then(|scene_id| scene_identities.get(&scene_id)) else {
        return;
    };
    if activity_id.is_none() {
        *activity_id = Some(identity.activity_id.clone());
    }
    if activity_family_id.is_none() {
        *activity_family_id = identity.activity_family_id.clone();
    }
    if difficulty_family.is_none() {
        *difficulty_family = identity.difficulty_family.clone();
    }
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
        ("GET", "/api/runtime/live/combat") => {
            write_json(
                &mut stream,
                200,
                &present_live_combat_update(controller.live_combat_snapshot()),
            )?;
        }
        ("POST", "/api/runtime/live/combat/wait") => {
            let request: LiveCombatWaitRequest = match serde_json::from_slice(&request.body) {
                Ok(request) => request,
                Err(error) => {
                    write_api_error(&mut stream, 400, format!("invalid request: {error}"))?;
                    return Ok(());
                }
            };
            write_json(
                &mut stream,
                200,
                &present_live_combat_update(controller.wait_for_live_combat(request)),
            )?;
        }
        ("POST", "/api/runtime/live/events/wait") => {
            let request: LiveEventWaitRequest = match serde_json::from_slice(&request.body) {
                Ok(request) => request,
                Err(error) => {
                    write_api_error(&mut stream, 400, format!("invalid request: {error}"))?;
                    return Ok(());
                }
            };
            write_json(&mut stream, 200, &controller.wait_for_live_events(request))?;
        }
        ("GET", "/api/runtime/environment") => {
            write_json(&mut stream, 200, &runtime_environment())?;
        }
        ("GET", "/api/settings/core") => {
            write_json(&mut stream, 200, &controller.core_settings())?;
        }
        ("POST", "/api/settings/core") => {
            let settings: CoreSettings = match serde_json::from_slice(&request.body) {
                Ok(settings) => settings,
                Err(error) => {
                    write_api_error(&mut stream, 400, format!("invalid request: {error}"))?;
                    return Ok(());
                }
            };
            match controller.update_core_settings(settings) {
                Ok(settings) => write_json(&mut stream, 200, &settings)?,
                Err(error) => write_api_error(&mut stream, 409, error)?,
            }
        }
        ("GET", "/api/settings/hotkeys") => {
            write_json(&mut stream, 200, &controller.hotkey_settings())?;
        }
        ("POST", "/api/settings/hotkeys/assign") => {
            let assignment: HotkeyAssignmentRequest = match serde_json::from_slice(&request.body) {
                Ok(assignment) => assignment,
                Err(error) => {
                    write_api_error(&mut stream, 400, format!("invalid request: {error}"))?;
                    return Ok(());
                }
            };
            match controller.assign_hotkey(assignment) {
                Ok(result) => write_json(&mut stream, 200, &result)?,
                Err(error) => write_api_error(&mut stream, 409, error)?,
            }
        }
        ("GET", "/api/settings/layout") => {
            write_json(&mut stream, 200, &controller.layout_settings())?;
        }
        ("POST", "/api/settings/layout") => {
            let settings: LayoutSettings = match serde_json::from_slice(&request.body) {
                Ok(settings) => settings,
                Err(error) => {
                    write_api_error(&mut stream, 400, format!("invalid request: {error}"))?;
                    return Ok(());
                }
            };
            match controller.update_layout_settings(settings) {
                Ok(settings) => write_json(&mut stream, 200, &settings)?,
                Err(error) => write_api_error(&mut stream, 409, error)?,
            }
        }
        ("GET", "/api/settings/themes") => {
            write_json(&mut stream, 200, &controller.theme_settings())?;
        }
        ("POST", "/api/settings/themes") => {
            let settings: ThemeSettings = match serde_json::from_slice(&request.body) {
                Ok(settings) => settings,
                Err(error) => {
                    write_api_error(&mut stream, 400, format!("invalid request: {error}"))?;
                    return Ok(());
                }
            };
            match controller.update_theme_settings(settings) {
                Ok(settings) => write_json(&mut stream, 200, &settings)?,
                Err(error) => write_api_error(&mut stream, 409, error)?,
            }
        }
        ("GET", "/api/settings/combat-meter") => {
            write_json(&mut stream, 200, &controller.combat_meter_settings())?;
        }
        ("POST", "/api/settings/combat-meter") => {
            let settings: CombatMeterSettings = match serde_json::from_slice(&request.body) {
                Ok(settings) => settings,
                Err(error) => {
                    write_api_error(&mut stream, 400, format!("invalid request: {error}"))?;
                    return Ok(());
                }
            };
            match controller.update_combat_meter_settings(settings) {
                Ok(settings) => write_json(&mut stream, 200, &settings)?,
                Err(error) => write_api_error(&mut stream, 409, error)?,
            }
        }
        ("GET", "/api/settings/combat-overlay") => {
            write_json(&mut stream, 200, &controller.combat_overlay_settings())?;
        }
        ("GET", "/api/settings/combat-overlay/bar-color-identities") => {
            match overlay_bar_color_identity_catalog() {
                Ok(catalog) => write_json(&mut stream, 200, &catalog)?,
                Err(error) => write_api_error(&mut stream, 500, error)?,
            }
        }
        ("POST", "/api/settings/combat-overlay") => {
            let settings: CombatOverlaySettings = match serde_json::from_slice(&request.body) {
                Ok(settings) => settings,
                Err(error) => {
                    write_api_error(&mut stream, 400, format!("invalid request: {error}"))?;
                    return Ok(());
                }
            };
            match controller.update_combat_overlay_settings(settings) {
                Ok(settings) => write_json(&mut stream, 200, &settings)?,
                Err(error) => write_api_error(&mut stream, 409, error)?,
            }
        }
        ("POST", "/api/settings/combat-overlay/background") => {
            match controller.save_combat_overlay_background(&request.body) {
                Ok(revision) => {
                    write_json(&mut stream, 200, &serde_json::json!({"revision": revision}))?
                }
                Err(error) => write_api_error(&mut stream, 400, error)?,
            }
        }
        ("GET", "/api/settings/combat-overlay/background") => {
            match controller.combat_overlay_background() {
                Ok((content_type, bytes)) => write_text(&mut stream, 200, content_type, &bytes)?,
                Err(error) => write_api_error(&mut stream, 404, error)?,
            }
        }
        ("GET", "/api/submissions/queue") => {
            write_json(&mut stream, 200, &controller.submission_queue())?;
        }
        ("GET", "/api/submissions/connection") => match controller.submission_connection() {
            Ok(connection) => write_json(&mut stream, 200, &connection)?,
            Err(error) => write_api_error(&mut stream, 409, error)?,
        },
        ("POST", "/api/submissions/connection") => {
            let update: SubmissionConnectionUpdateRequest =
                match serde_json::from_slice(&request.body) {
                    Ok(update) => update,
                    Err(error) => {
                        write_api_error(&mut stream, 400, format!("invalid request: {error}"))?;
                        return Ok(());
                    }
                };
            match controller.update_submission_connection(update) {
                Ok(connection) => write_json(&mut stream, 200, &connection)?,
                Err(error) => write_api_error(&mut stream, 409, error)?,
            }
        }
        ("POST", "/api/submissions/connection/disconnect") => {
            match controller.disconnect_submission_connection() {
                Ok(connection) => write_json(&mut stream, 200, &connection)?,
                Err(error) => write_api_error(&mut stream, 409, error)?,
            }
        }
        ("GET", "/api/submissions/policy") => {
            write_json(&mut stream, 200, &controller.submission_policy())?;
        }
        ("POST", "/api/submissions/policy") => {
            let policy: SubmissionPolicy = match serde_json::from_slice(&request.body) {
                Ok(policy) => policy,
                Err(error) => {
                    write_api_error(&mut stream, 400, format!("invalid request: {error}"))?;
                    return Ok(());
                }
            };
            match controller.update_submission_policy(policy) {
                Ok(policy) => write_json(&mut stream, 200, &policy)?,
                Err(error) => write_api_error(&mut stream, 409, error)?,
            }
        }
        ("POST", "/api/submissions/queue/refresh") => match controller.refresh_submission_queue() {
            Ok(queue) => write_json(&mut stream, 200, &queue)?,
            Err(error) => write_api_error(&mut stream, 409, error)?,
        },
        ("POST", "/api/submissions/queue/import") => {
            let request: SubmissionImportRequest = match serde_json::from_slice(&request.body) {
                Ok(request) => request,
                Err(error) => {
                    write_api_error(&mut stream, 400, format!("invalid request: {error}"))?;
                    return Ok(());
                }
            };
            match controller.import_submission_artifact(request) {
                Ok(result) => write_json(&mut stream, 200, &result)?,
                Err(error) => write_api_error(&mut stream, 409, error)?,
            }
        }
        ("POST", "/api/submissions/queue/verify") => {
            let request: SubmissionVerificationRequest = match serde_json::from_slice(&request.body)
            {
                Ok(request) => request,
                Err(error) => {
                    write_api_error(&mut stream, 400, format!("invalid request: {error}"))?;
                    return Ok(());
                }
            };
            match controller.verify_queued_submission(request) {
                Ok(result) => write_json(&mut stream, 200, &result)?,
                Err(error) => write_api_error(&mut stream, 409, error)?,
            }
        }
        ("POST", "/api/submissions/mock/run") => {
            let request: MockSubmissionRequest = match serde_json::from_slice(&request.body) {
                Ok(request) => request,
                Err(error) => {
                    write_api_error(&mut stream, 400, format!("invalid request: {error}"))?;
                    return Ok(());
                }
            };
            match controller.run_mock_submission(request) {
                Ok(result) => write_json(&mut stream, 200, &result)?,
                Err(error) => write_api_error(&mut stream, 409, error)?,
            }
        }
        ("POST", "/api/submissions/queue/upload") => {
            let request: SubmissionUploadRequest = match serde_json::from_slice(&request.body) {
                Ok(request) => request,
                Err(error) => {
                    write_api_error(&mut stream, 400, format!("invalid request: {error}"))?;
                    return Ok(());
                }
            };
            match controller.upload_queued_submission(request) {
                Ok(result) => write_json(&mut stream, 200, &result)?,
                Err(error) => write_api_error(&mut stream, 409, error)?,
            }
        }
        ("GET", "/api/profiles/packages") => {
            write_json(&mut stream, 200, &controller.profile_packages())?;
        }
        ("POST", "/api/profiles/packages/refresh") => match controller.refresh_profile_packages() {
            Ok(packages) => write_json(&mut stream, 200, &packages)?,
            Err(error) => write_api_error(&mut stream, 409, error)?,
        },
        ("POST", "/api/profiles/packages/inspect") => {
            let request: ProfilePackageInspectionRequest =
                match serde_json::from_slice(&request.body) {
                    Ok(request) => request,
                    Err(error) => {
                        write_api_error(&mut stream, 400, format!("invalid request: {error}"))?;
                        return Ok(());
                    }
                };
            match controller.inspect_profile_package(request) {
                Ok(package) => write_json(&mut stream, 200, &package)?,
                Err(error) => write_api_error(&mut stream, 409, error)?,
            }
        }
        ("POST", "/api/profiles/project-last") => {
            match controller.project_last_profile_packages() {
                Ok(result) => write_json(&mut stream, 200, &result)?,
                Err(error) => write_api_error(&mut stream, 409, error)?,
            }
        }
        ("POST", "/api/runtime/events/page") => {
            let request: EventViewerPageRequest = match serde_json::from_slice(&request.body) {
                Ok(request) => request,
                Err(error) => {
                    write_api_error(&mut stream, 400, format!("invalid request: {error}"))?;
                    return Ok(());
                }
            };
            match controller.event_viewer_page(request) {
                Ok(page) => write_json(&mut stream, 200, &page)?,
                Err(error) => write_api_error(&mut stream, 409, error)?,
            }
        }
        ("GET", "/api/runtime/run-report") => match controller.run_report() {
            Ok(report) => write_json(&mut stream, 200, &report)?,
            Err(error) => write_api_error(&mut stream, 409, error)?,
        },
        ("GET", "/api/runtime/combat-history") => match controller.combat_history_catalog() {
            Ok(catalog) => write_json(&mut stream, 200, &catalog)?,
            Err(error) => write_api_error(&mut stream, 409, error)?,
        },
        ("POST", "/api/runtime/combat-history/wait") => {
            let request: CombatHistoryWaitRequest = match serde_json::from_slice(&request.body) {
                Ok(request) => request,
                Err(error) => {
                    write_api_error(&mut stream, 400, format!("invalid request: {error}"))?;
                    return Ok(());
                }
            };
            write_json(
                &mut stream,
                200,
                &controller.wait_for_combat_history(request),
            )?;
        }
        ("POST", "/api/runtime/combat-history/detail") => {
            let request: CombatHistoryDetailRequest = match serde_json::from_slice(&request.body) {
                Ok(request) => request,
                Err(error) => {
                    write_api_error(&mut stream, 400, format!("invalid request: {error}"))?;
                    return Ok(());
                }
            };
            match controller.combat_history_detail(request) {
                Ok(history) => write_json(&mut stream, 200, &history)?,
                Err(error) => write_api_error(&mut stream, 404, error)?,
            }
        }
        ("POST", "/api/runtime/combat-history/favorite") => {
            let request: CombatHistoryFavoriteRequest = match serde_json::from_slice(&request.body)
            {
                Ok(request) => request,
                Err(error) => {
                    write_api_error(&mut stream, 400, format!("invalid request: {error}"))?;
                    return Ok(());
                }
            };
            match controller.set_combat_history_favorite(request) {
                Ok(catalog) => write_json(&mut stream, 200, &catalog)?,
                Err(error) => write_api_error(&mut stream, 404, error)?,
            }
        }
        ("POST", "/api/runtime/combat-history/delete") => {
            let request: CombatHistoryDeleteRequest = match serde_json::from_slice(&request.body) {
                Ok(request) => request,
                Err(error) => {
                    write_api_error(&mut stream, 400, format!("invalid request: {error}"))?;
                    return Ok(());
                }
            };
            match controller.delete_combat_history(request) {
                Ok(result) => write_json(&mut stream, 200, &result)?,
                Err(error) => write_api_error(&mut stream, 409, error)?,
            }
        }
        ("GET", "/api/plugins/catalog") => {
            write_json(&mut stream, 200, &controller.plugin_catalog())?;
        }
        ("POST", "/api/plugins/refresh") => match controller.refresh_plugins() {
            Ok(catalog) => write_json(&mut stream, 200, &catalog)?,
            Err(error) => write_api_error(&mut stream, 409, error)?,
        },
        ("POST", "/api/plugins/enablement") => {
            let request: PluginEnablementRequest = match serde_json::from_slice(&request.body) {
                Ok(request) => request,
                Err(error) => {
                    write_api_error(&mut stream, 400, format!("invalid request: {error}"))?;
                    return Ok(());
                }
            };
            match controller.set_plugin_enabled(request) {
                Ok(catalog) => write_json(&mut stream, 200, &catalog)?,
                Err(error) => write_api_error(&mut stream, 409, error)?,
            }
        }
        ("GET", route) if route.starts_with("/api/plugins/surface/") => {
            let identifiers = route
                .trim_start_matches("/api/plugins/surface/")
                .split('/')
                .collect::<Vec<_>>();
            if identifiers.len() != 2
                || identifiers
                    .iter()
                    .any(|value| value.is_empty() || !is_safe_plugin_route_identifier(value))
            {
                write_api_error(&mut stream, 404, "plug-in surface not found".into())?;
                return Ok(());
            }
            match controller.plugin_surface_entrypoint(identifiers[0], identifiers[1]) {
                Ok(path) => {
                    let content_type =
                        match path.extension().and_then(|extension| extension.to_str()) {
                            Some("html") | Some("htm") => "text/html; charset=utf-8",
                            Some("svg") => "image/svg+xml",
                            Some("png") => "image/png",
                            Some("jpg") | Some("jpeg") => "image/jpeg",
                            _ => "application/octet-stream",
                        };
                    write_text(&mut stream, 200, content_type, &std::fs::read(path)?)?;
                }
                Err(error) => write_api_error(&mut stream, 404, error)?,
            }
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
        ("GET", route) if route.starts_with("/game-assets/") => {
            write_game_asset(&mut stream, &controller.install_root.join("assets"), route)?;
        }
        ("GET", route) if !route.starts_with("/api/") => {
            write_static(&mut stream, ui_root, route)?;
        }
        _ => write_api_error(&mut stream, 404, "route not found".into())?,
    }
    Ok(())
}

fn is_safe_plugin_route_identifier(value: &str) -> bool {
    value.len() <= 128
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'-' | b'_'))
}

fn write_game_asset(
    stream: &mut TcpStream,
    asset_root: &Path,
    route: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    let relative = Path::new(route.trim_start_matches("/game-assets/"));
    if relative
        .components()
        .any(|component| !matches!(component, Component::Normal(_)))
    {
        write_text(stream, 404, "text/plain; charset=utf-8", b"Not found")?;
        return Ok(());
    }
    let root = match std::fs::canonicalize(asset_root) {
        Ok(root) => root,
        Err(_) => {
            write_text(stream, 404, "text/plain; charset=utf-8", b"Not found")?;
            return Ok(());
        }
    };
    let path = match std::fs::canonicalize(root.join(relative)) {
        Ok(path) if path.starts_with(&root) && path.is_file() => path,
        _ => {
            write_text(stream, 404, "text/plain; charset=utf-8", b"Not found")?;
            return Ok(());
        }
    };
    let content_type = match path.extension().and_then(|extension| extension.to_str()) {
        Some("png") => "image/png",
        Some("webp") => "image/webp",
        Some("jpg") | Some("jpeg") => "image/jpeg",
        Some("svg") => "image/svg+xml",
        _ => {
            write_text(stream, 404, "text/plain; charset=utf-8", b"Not found")?;
            return Ok(());
        }
    };
    write_text(stream, 200, content_type, &std::fs::read(path)?)?;
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
    let request_body_limit = if route == "/api/settings/combat-overlay/background" {
        MAX_OVERLAY_BACKGROUND_BYTES
    } else {
        MAX_REQUEST_BODY_BYTES
    };
    if content_length > request_body_limit {
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

fn combat_overlay_image_format(bytes: &[u8]) -> Result<(&'static str, &'static str), String> {
    if bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
        return Ok(("png", "image/png"));
    }
    if bytes.starts_with(&[0xff, 0xd8, 0xff]) {
        return Ok(("jpg", "image/jpeg"));
    }
    if bytes.len() >= 12 && &bytes[..4] == b"RIFF" && &bytes[8..12] == b"WEBP" {
        return Ok(("webp", "image/webp"));
    }
    if bytes.starts_with(b"GIF87a") || bytes.starts_with(b"GIF89a") {
        return Ok(("gif", "image/gif"));
    }
    Err("Combat Overlay backgrounds must be PNG, JPEG, WebP, or GIF images".into())
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
    use std::sync::atomic::{AtomicU64, Ordering};

    use bytes::Bytes;
    use etherparse::{PacketBuilder, TcpHeader};
    use rlogs_capture::{CaptureLinkType, CapturedFrame, PcapWriter, TimestampNormalization};
    use rlogs_core::GameConnection;
    use rlogs_events::{
        ActorId, BoundaryReason, CanonicalEvent, CharacterIdentity, DungeonEvent,
        EVENT_SCHEMA_VERSION, EntityRef, EntityUuid, EventEnvelope, EventProvenance,
        EventSensitivity, EventTime, SceneId, StatusEffectId, StatusEvent, StatusState,
        TimelineEvent, WorldContext,
    };
    use rlogs_game_bpsr::{
        CharacterProfilePatch, DungeonSegmentBoundary, DungeonSegmentEndReason,
        DungeonSegmentStartReason, FragmentKind, state_damage_contribution_game_build,
        state_damage_contribution_protocol_pack_digest,
    };
    use rlogs_log_format::{RlogSeal, RlogWriter};
    use rlogs_network::IpEndpoint;

    #[test]
    fn provisional_research_journal_retains_team_lifecycle_service() {
        let pack = ProtocolPack::from_json(include_bytes!(
            "../../../plugins/games/blue-protocol-star-resonance/protocol-packs/global/steam-24252055/pack.json"
        ))
        .expect("bundled protocol pack");
        let service_ids = provisional_research_service_ids(&pack);

        assert_eq!(
            service_ids,
            BTreeSet::from([103_198_054, 966_773_353, 1_664_308_034])
        );
        assert!(!service_ids.contains(&78_136_601));
        assert!(!service_ids.contains(&164_931_432));
    }

    #[test]
    fn live_overlay_character_identity_requires_a_confirmed_player() {
        let entity_uuid = "216009015936";
        assert_eq!(
            live_overlay_character_id(Some("player"), entity_uuid).as_deref(),
            Some("3296036")
        );
        assert_eq!(
            live_overlay_character_id(Some("monster"), entity_uuid),
            None
        );
        assert_eq!(
            live_overlay_character_id(Some("unknown:0"), entity_uuid),
            None
        );
        assert_eq!(live_overlay_character_id(None, entity_uuid), None);
    }

    #[test]
    fn live_overlay_identity_enrichment_accepts_only_unclassified_player_candidates() {
        let entity_uuid = "216009015936";
        for actor_kind in [Some("player"), Some("unknown:0"), None] {
            assert_eq!(
                live_overlay_character_identity_candidate(actor_kind, entity_uuid).as_deref(),
                Some("3296036")
            );
        }
        for actor_kind in [
            Some("monster"),
            Some("pet"),
            Some("projectile"),
            Some("npc"),
        ] {
            assert_eq!(
                live_overlay_character_identity_candidate(actor_kind, entity_uuid),
                None
            );
        }
    }

    #[test]
    fn live_overlay_enriches_an_unclassified_row_from_an_exact_uid_match() {
        struct ExactIdentity(character_identities::CharacterPresentationIdentity);

        impl CharacterIdentityResolver for ExactIdentity {
            fn resolve_identity(
                &self,
                deployment_id: &str,
                region_id: &str,
                world_id: Option<&str>,
                character_id: &str,
            ) -> Option<&character_identities::CharacterPresentationIdentity> {
                let identity = &self.0;
                (identity.deployment_id == deployment_id
                    && identity.region_id == region_id
                    && identity.world_id.as_deref() == world_id
                    && identity.character_id == character_id)
                    .then_some(identity)
            }
        }

        let identities = ExactIdentity(character_identities::CharacterPresentationIdentity {
            game_plugin_id: BPSR_GAME_PLUGIN_ID.into(),
            deployment_id: "global-steam".into(),
            region_id: "global".into(),
            world_id: Some("asteria".into()),
            character_id: "3296036".into(),
            display_name: "MarieRose".into(),
            class_id: Some(11),
            specialization_id: Some(117),
            level: Some(60),
            ability_score: Some(61_782),
            weapon_item_id: Some(2_000_631),
            weapon_breakthrough_count: Some(280),
            seasonal_strength: Some(4_627),
            primary_loadout: vec![ActorLoadoutSlot {
                slot_id: 7,
                ability_id: Some(3_948),
                item_id: Some(3_000_101),
                tier: Some(5),
            }],
            auxiliary_loadout: Vec::new(),
        });
        let actor = rlogs_plugin_combat_meter::ActorCombatSummary {
            actor_id: "67".into(),
            entity_uuid: "216009015936".into(),
            character_id: None,
            display_name: Some("Actor 67".into()),
            actor_kind: Some("unknown:0".into()),
            monster_id: None,
            current_hp: None,
            max_hp: None,
            class_id: Some(1),
            specialization_id: Some(101),
            level: None,
            ability_score: None,
            weapon_item_id: Some(999),
            weapon_breakthrough_count: Some(1),
            seasonal_score: None,
            primary_loadout: vec![ActorLoadoutSlot {
                slot_id: 7,
                ability_id: Some(1),
                item_id: Some(1),
                tier: Some(1),
            }],
            auxiliary_loadout: Vec::new(),
            reported_damage: 1,
            effective_damage: 1,
            hp_damage: 1,
            shield_damage: 0,
            damage_during_combat: 1,
            damage_taken: 0,
            dps: 1.0,
            hps: 0.0,
            tps: 0.0,
            rdps_damage: None,
            rdps: None,
            rdps_contribution_given: None,
            rdps_contribution_received: None,
            rdps_incomplete: false,
            reported_healing: 0,
            effective_healing: 0,
            overheal: 0,
            shielding: 0,
            casts: 0,
            hits: 1,
            critical_hits: 0,
            deaths: 0,
            revives: 0,
            position_samples: 0,
            path_distance: 0.0,
            abilities: Vec::new(),
        };
        let mut snapshot = CombatTimelineSnapshot {
            schema_version: 1,
            session_id: "live".into(),
            deployment_id: "global-steam".into(),
            region_id: "global".into(),
            world_id: Some("asteria".into()),
            client_build: "test".into(),
            protocol_pack_digest: "test".into(),
            rdps_status: "observed".into(),
            encounter_id: None,
            encounter_state: None,
            scene_id: None,
            event_count: 1,
            data_gap_count: 0,
            combat_window_count: 1,
            combat_active: true,
            last_hostile_micros: Some(1),
            latest_event_micros: Some(1),
            combat_inactivity_timeout_micros: 0,
            combat_started_micros: Some(1),
            combat_ended_micros: None,
            active_combat_micros: 1,
            run_elapsed_micros: None,
            game_time_micros: None,
            true_time_micros: None,
            closed_at_log_end: false,
            actors: vec![actor],
        };

        enrich_bpsr_live_character_state(
            &mut snapshot,
            &identities,
            LiveCharacterIdentityAuthority::CaptureTime,
        );

        let actor = &snapshot.actors[0];
        assert_eq!(actor.actor_kind.as_deref(), Some("player"));
        assert_eq!(actor.display_name.as_deref(), Some("MarieRose"));
        assert_eq!(actor.class_id, Some(11));
        assert_eq!(actor.specialization_id, Some(117));
        assert_eq!(actor.weapon_item_id, Some(2_000_631));
        assert_eq!(actor.primary_loadout, identities.0.primary_loadout);
    }

    #[test]
    fn live_overlay_republishes_capture_time_identity_before_run_completion() {
        assert!(live_overlay_topic_invalidates(EventTopic::Actor));
        assert!(live_overlay_topic_invalidates(EventTopic::CharacterProfile));
        assert!(live_overlay_topic_invalidates(EventTopic::Combat));
        assert!(!live_overlay_topic_invalidates(EventTopic::Chat));
    }

    #[test]
    fn boss_dps_uses_only_damage_received_by_that_boss() {
        assert_eq!(live_boss_dps(24_000_000, 3_000_000), 8_000_000.0);
        assert_eq!(live_boss_dps(12_000_000, 3_000_000), 4_000_000.0);
        assert_eq!(live_boss_dps(12_000_000, 0), 0.0);
    }

    #[test]
    fn live_overlay_never_substitutes_catalog_rarity_for_equipped_imagine_tier() {
        let missing_runtime_tier = ActorLoadoutSlot {
            slot_id: 7,
            ability_id: Some(3_948),
            item_id: None,
            tier: None,
        };
        let badge = live_overlay_primary_imagine_badge(&missing_runtime_tier);

        assert_eq!(badge.item_id, Some(3_000_101));
        assert_eq!(badge.tier, None);
        assert!(
            badge
                .icon_asset_path
                .as_deref()
                .is_some_and(|path| path.ends_with("3000101-rorola.png"))
        );

        let observed_runtime_tier = ActorLoadoutSlot {
            tier: Some(5),
            ..missing_runtime_tier
        };
        assert_eq!(
            live_overlay_primary_imagine_badge(&observed_runtime_tier).tier,
            Some(5)
        );
    }

    fn boss_candidate(actor_id: &str, max_hp: i64, was_damaged: bool) -> LiveOverlayBossCandidate {
        LiveOverlayBossCandidate {
            presentation: LiveOverlayBossPresentation {
                actor_id: actor_id.into(),
                monster_id: max_hp,
                name: actor_id.into(),
                current_hp: if was_damaged { max_hp - 1 } else { max_hp },
                max_hp,
                bdps: 0.0,
                team_damage: 0,
            },
            was_damaged,
        }
    }

    #[test]
    fn boss_selector_keeps_every_packet_present_canonical_boss_after_combat_starts() {
        let bosses = select_live_overlay_bosses(vec![
            boss_candidate("primary", 200_000_000, true),
            boss_candidate("secondary", 100_000_000, false),
            boss_candidate("tertiary", 10_000_000, false),
        ]);
        assert_eq!(
            bosses
                .iter()
                .map(|boss| boss.actor_id.as_str())
                .collect::<Vec<_>>(),
            ["primary", "secondary", "tertiary"]
        );
    }

    #[test]
    fn boss_selector_returns_nothing_before_boss_participation() {
        let bosses = select_live_overlay_bosses(vec![
            boss_candidate("primary-a", 100_000_000, false),
            boss_candidate("primary-b", 100_000_000, false),
            boss_candidate("tertiary", 20_000_000, false),
        ]);
        assert!(bosses.is_empty());
    }

    #[test]
    fn boss_selector_orders_an_arbitrary_count_deterministically() {
        let bosses = select_live_overlay_bosses(vec![
            boss_candidate("small", 8_000_000, false),
            boss_candidate("large-b", 120_000_000, true),
            boss_candidate("large-a", 120_000_000, false),
            boss_candidate("medium", 95_000_000, false),
        ]);
        assert_eq!(
            bosses
                .iter()
                .map(|boss| boss.actor_id.as_str())
                .collect::<Vec<_>>(),
            ["large-a", "large-b", "medium", "small"]
        );
    }

    #[test]
    fn combat_overlay_background_recognizes_animated_gif() {
        assert_eq!(
            combat_overlay_image_format(b"GIF89a\x01\x00\x01\x00").unwrap(),
            ("gif", "image/gif")
        );
        assert_eq!(
            combat_overlay_image_format(b"GIF87a\x01\x00\x01\x00").unwrap(),
            ("gif", "image/gif")
        );
        assert!(combat_overlay_image_format(b"not an image").is_err());
    }

    use super::*;

    static TEMPORARY_ROOT_SEQUENCE: AtomicU64 = AtomicU64::new(1);

    fn temporary_root() -> PathBuf {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let sequence = TEMPORARY_ROOT_SEQUENCE.fetch_add(1, Ordering::Relaxed);
        std::env::temp_dir().join(format!(
            "rlogs-desktop-host-{}-{unique}-{sequence}",
            std::process::id(),
        ))
    }

    fn endpoint(last: u8, port: u16) -> IpEndpoint {
        IpEndpoint::new(IpAddr::V4(Ipv4Addr::new(10, 0, 0, last)), port)
    }

    #[test]
    fn rdps_validation_report_persists_atomically_and_replaces_cleanly() {
        let root = temporary_root();
        std::fs::create_dir_all(&root).unwrap();
        let path = root.join(format!(
            "steam-24609362.v{}.cumulative.validation.json",
            RDPS_VALIDATION_REPORT_SCHEMA_VERSION
        ));
        let mut analyzer = RdpsValidationAnalyzer::bundled().unwrap();
        let initial = analyzer.report();
        write_rdps_validation_report_atomic(&path, &initial).unwrap();
        let restored = read_rdps_validation_report(&path).unwrap();
        assert_eq!(restored.schema_version, initial.schema_version);
        assert_eq!(restored.summary.total_obligations, 350);

        analyzer.merge_report(&restored).unwrap();
        let replacement = analyzer.report();
        let bundle = RdpsValidationCumulativeReport {
            schema_version: RDPS_VALIDATION_CUMULATIVE_SCHEMA_VERSION,
            manifest_game_build: replacement.manifest_game_build.clone(),
            session_ids: BTreeSet::from(["session-a".into()]),
            report: replacement,
        };
        write_rdps_validation_cumulative_report_atomic(&path, &bundle).unwrap();
        let restored_replacement = read_rdps_validation_cumulative_report(&path).unwrap();
        assert_eq!(restored_replacement.report.total_events, 0);
        assert_eq!(restored_replacement.session_ids.len(), 1);
        assert!(!path.with_extension("partial").exists());
        assert!(!path.with_extension("backup").exists());
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn rdps_validation_checkpoint_writer_persists_before_clean_shutdown() {
        let root = temporary_root();
        std::fs::create_dir_all(&root).unwrap();
        let path = root.join(format!(
            "session-a-steam-24609362.v{}.checkpoint.validation.json",
            RDPS_VALIDATION_REPORT_SCHEMA_VERSION
        ));
        let mut report = RdpsValidationAnalyzer::bundled().unwrap().report();
        report.total_events = 7;

        let writer = RdpsValidationCheckpointWriter::spawn(path.clone()).unwrap();
        assert!(writer.try_checkpoint(report).unwrap());
        writer.finish().unwrap();

        let restored = read_rdps_validation_report(&path).unwrap();
        assert_eq!(restored.total_events, 7);
        assert!(!path.with_extension("partial").exists());
        assert!(!path.with_extension("backup").exists());
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn cumulative_rdps_validation_recovers_orphaned_session_reports_once() {
        let root = temporary_root();
        std::fs::create_dir_all(&root).unwrap();
        let cumulative = root.join(format!(
            "steam-24609362.v{}.cumulative.validation.json",
            RDPS_VALIDATION_REPORT_SCHEMA_VERSION
        ));

        let analyzer = RdpsValidationAnalyzer::bundled().unwrap();
        let mut first = analyzer.report();
        first.total_events = 2;
        let mut second = analyzer.report();
        second.total_events = 3;
        let first_path = root.join(format!(
            "session-a-steam-24609362.v{}.validation.json",
            RDPS_VALIDATION_REPORT_SCHEMA_VERSION
        ));
        let second_path = root.join(format!(
            "session-b-steam-24609362.v{}.validation.json",
            RDPS_VALIDATION_REPORT_SCHEMA_VERSION
        ));
        write_rdps_validation_session_report_once(&first_path, &first).unwrap();

        let first_bundle =
            update_rdps_validation_cumulative_from_sessions(&root, "24609362", &cumulative)
                .unwrap();
        assert_eq!(
            first_bundle.session_ids,
            BTreeSet::from(["session-a".into()])
        );
        assert_eq!(first_bundle.report.total_events, 2);
        write_rdps_validation_cumulative_report_atomic(&cumulative, &first_bundle).unwrap();

        // This is the crash window: the immutable second session exists but
        // the cumulative index has not yet been updated.
        write_rdps_validation_session_report_once(&second_path, &second).unwrap();
        let recovered =
            update_rdps_validation_cumulative_from_sessions(&root, "24609362", &cumulative)
                .unwrap();
        assert_eq!(
            recovered.session_ids,
            BTreeSet::from(["session-a".into(), "session-b".into()])
        );
        assert_eq!(recovered.report.total_events, 5);
        write_rdps_validation_cumulative_report_atomic(&cumulative, &recovered).unwrap();

        let idempotent =
            update_rdps_validation_cumulative_from_sessions(&root, "24609362", &cumulative)
                .unwrap();
        assert_eq!(idempotent.report.total_events, 5);
        assert_eq!(idempotent.session_ids.len(), 2);

        let mut conflicting = first.clone();
        conflicting.total_events = 9;
        assert!(
            write_rdps_validation_session_report_once(&first_path, &conflicting).is_err(),
            "immutable session evidence must never be silently replaced"
        );
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn cumulative_rdps_validation_recovers_checkpoint_and_prefers_final_report() {
        let root = temporary_root();
        std::fs::create_dir_all(&root).unwrap();
        let cumulative = root.join(format!(
            "steam-24609362.v{}.cumulative.validation.json",
            RDPS_VALIDATION_REPORT_SCHEMA_VERSION
        ));
        let checkpoint_path = root.join(format!(
            "session-a-steam-24609362.v{}.checkpoint.validation.json",
            RDPS_VALIDATION_REPORT_SCHEMA_VERSION
        ));
        let final_path = root.join(format!(
            "session-a-steam-24609362.v{}.validation.json",
            RDPS_VALIDATION_REPORT_SCHEMA_VERSION
        ));

        let analyzer = RdpsValidationAnalyzer::bundled().unwrap();
        let mut checkpoint = analyzer.report();
        checkpoint.total_events = 2;
        write_rdps_validation_report_atomic(&checkpoint_path, &checkpoint).unwrap();

        let recovered_checkpoint =
            update_rdps_validation_cumulative_from_sessions(&root, "24609362", &cumulative)
                .unwrap();
        assert_eq!(recovered_checkpoint.report.total_events, 2);
        assert_eq!(
            recovered_checkpoint.session_ids,
            BTreeSet::from(["session-a".into()])
        );

        let mut final_report = analyzer.report();
        final_report.total_events = 5;
        write_rdps_validation_report_atomic(&final_path, &final_report).unwrap();

        // Both artifacts can coexist in the normal shutdown crash window:
        // the final report has been persisted, but the cumulative report has
        // not yet been updated and the checkpoint has not yet been removed.
        let preferred_final =
            update_rdps_validation_cumulative_from_sessions(&root, "24609362", &cumulative)
                .unwrap();
        assert_eq!(
            preferred_final.report.total_events, 5,
            "an immutable final report must supersede a same-session checkpoint"
        );
        assert_eq!(preferred_final.session_ids.len(), 1);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn validation_progress_detail_lists_every_runtime_proof_domain() {
        let progress = RdpsValidationAnalyzer::bundled().unwrap().progress();
        let detail = format_rdps_validation_remaining_domains(&progress);
        assert!(detail.contains("factors 227/227 open (0 partial)"));
        assert!(detail.contains("offense 84/84 open (0 partial)"));
        assert!(detail.contains("mastery 26/26 open (0 partial)"));
        assert!(detail.contains("packet formulas 11/11 open (0 partial)"));
        assert!(detail.contains("mitigation 2/2 open (0 partial)"));
    }

    #[test]
    fn canonical_run_terminal_events_freeze_local_history() {
        for state in [RunState::Completed, RunState::Failed, RunState::Exited] {
            let event = CanonicalEvent::Timeline(TimelineEvent {
                sequence: 1,
                time: EventTime {
                    observed_micros: 1,
                    game_time_millis: None,
                },
                provenance: EventProvenance::wire(1, 1, 1),
                kind: TimelineEventKind::RunBoundary {
                    state,
                    scene_id: None,
                    reason: BoundaryReason::AuthoritativePacket,
                },
            });
            assert!(closes_live_run_history(&event));
        }

        let nonterminal = CanonicalEvent::Timeline(TimelineEvent {
            sequence: 1,
            time: EventTime {
                observed_micros: 1,
                game_time_millis: None,
            },
            provenance: EventProvenance::wire(1, 1, 1),
            kind: TimelineEventKind::RunBoundary {
                state: RunState::Started,
                scene_id: None,
                reason: BoundaryReason::AuthoritativePacket,
            },
        });
        assert!(!closes_live_run_history(&nonterminal));
    }

    #[test]
    fn live_run_reset_restores_world_context_observed_before_entry() {
        let region = RegionContext {
            identity: RegionIdentity {
                deployment_id: "global".into(),
                region_id: "global".into(),
                realm_id: None,
                world_id: Some("asteria".into()),
            },
            client_build: "24252055".into(),
            protocol_pack_digest: "sha256:fixture".into(),
            evidence: Vec::new(),
        };
        let header = RlogHeader::new("monitor", region.clone(), "unit-test");
        let world = EventEnvelope {
            schema_version: EVENT_SCHEMA_VERSION,
            session_id: header.session_id.clone(),
            sequence: 1,
            region: region.clone(),
            time: EventTime {
                observed_micros: 10,
                game_time_millis: None,
            },
            provenance: EventProvenance::wire(1, 1, 1),
            sensitivity: EventSensitivity::PublicGameplay,
            event: CanonicalEvent::WorldChanged(WorldContext {
                scene_id: Some(SceneId(6_515)),
                map_id: Some(6_515),
                line_id: None,
                scene_instance_id: None,
                dungeon_instance_id: Some("instance".into()),
            }),
        };
        let entered = EventEnvelope {
            schema_version: EVENT_SCHEMA_VERSION,
            session_id: header.session_id.clone(),
            sequence: 2,
            region,
            time: EventTime {
                observed_micros: 20,
                game_time_millis: None,
            },
            provenance: EventProvenance::wire(2, 2, 2),
            sensitivity: EventSensitivity::PublicGameplay,
            event: CanonicalEvent::Dungeon(DungeonEvent {
                kind: DungeonEventKind::Entered,
                dungeon_id: None,
                instance_id: Some("instance".into()),
                difficulty_id: Some(16),
                objective_map_key: None,
                objective_id: None,
                objective_value: None,
                objective_complete: None,
                objective_catalog: None,
                flow: None,
            }),
        };
        let mut started = entered.clone();
        started.sequence = 3;
        started.time.observed_micros = 30;
        if let CanonicalEvent::Dungeon(event) = &mut started.event {
            event.kind = DungeonEventKind::Started;
        }
        let mut encounter = EncounterRecorderPlugin::new(bundled_run_reducer_config().unwrap());
        let mut meter = bpsr_combat_timeline_plugin().unwrap();

        begin_live_encounter_preserving_world(&mut encounter, &header, Some(&world)).unwrap();
        begin_live_combat_preserving_world(&mut meter, &header, Some(&world));
        encounter.observe_live(&entered).unwrap();
        encounter.observe_live(&started).unwrap();

        let snapshot = encounter.live_snapshot().unwrap();
        assert_eq!(snapshot.runs.len(), 1);
        assert_eq!(snapshot.runs[0].identity.scene_id, Some(6_515));
        assert_eq!(
            snapshot.runs[0].identity.activity_family_id.as_deref(),
            Some("dungeon.6515")
        );
        assert_eq!(snapshot.runs[0].identity.difficulty_tier, Some(16));
        assert_eq!(meter.live_overlay_snapshot().unwrap().scene_id, Some(6_515));
    }

    #[test]
    fn reviewed_scene_identity_backfills_missing_history_difficulty() {
        let identities = bundled_scene_run_identities().unwrap();
        for (scene_id, expected_activity, expected_difficulty) in [
            (1_621, "tina-mindrealm", "unstable"),
            (12_022, "guild-hunt", "normal"),
            (12_023, "guild-hunt", "hard"),
            (6_515, "dungeon.6515", "master"),
            (6_525, "mech-facility", "master"),
            (6_565, "sea-ringed-reef", "master"),
        ] {
            let mut activity_id = None;
            let mut activity_family_id = None;
            let mut difficulty_family = None;
            enrich_bpsr_scene_run_identity(
                Some(scene_id),
                &mut activity_id,
                &mut activity_family_id,
                &mut difficulty_family,
                &identities,
            );
            assert_eq!(activity_id, Some(format!("scene.{scene_id}")));
            assert_eq!(activity_family_id.as_deref(), Some(expected_activity));
            assert_eq!(difficulty_family.as_deref(), Some(expected_difficulty));
        }
    }

    #[test]
    fn reviewed_scene_identity_does_not_replace_captured_values() {
        let identities = bundled_scene_run_identities().unwrap();
        let mut activity_id = Some("captured.activity".into());
        let mut activity_family_id = Some("captured-family".into());
        let mut difficulty_family = Some("captured-difficulty".into());
        enrich_bpsr_scene_run_identity(
            Some(12_023),
            &mut activity_id,
            &mut activity_family_id,
            &mut difficulty_family,
            &identities,
        );
        assert_eq!(activity_id.as_deref(), Some("captured.activity"));
        assert_eq!(activity_family_id.as_deref(), Some("captured-family"));
        assert_eq!(difficulty_family.as_deref(), Some("captured-difficulty"));
    }

    #[test]
    fn combat_identity_replaces_incompatible_profile_specialization_with_run_evidence() {
        let falconry_abilities = BTreeSet::from([2_233_i64]);
        let observed =
            resolve_actor_combat_identity(Some(11), Some(119), falconry_abilities.iter().copied())
                .unwrap();
        assert_eq!(
            (observed.class_id, observed.specialization_id),
            (Some(11), Some(117))
        );

        let incompatible =
            resolve_actor_combat_identity(Some(11), Some(119), std::iter::empty()).unwrap();
        assert_eq!(
            (incompatible.class_id, incompatible.specialization_id),
            (Some(11), None)
        );
    }

    fn captured_marksman_history() -> CombatHistorySnapshot {
        serde_json::from_str(
            r#"{
            "schema_version": 1,
            "session_id": "captured-marksman-run",
            "deployment_id": "global",
            "region_id": "global",
            "world_id": "asteria",
            "client_build": "test-build",
            "protocol_pack_digest": "test-pack",
            "runs": [{
                "run_index": 0,
                "activity_id": "scene.1632",
                "activity_family_id": "tina-mindrealm",
                "scene_id": 1632,
                "presentation_scene_name": null,
                "instance_id": "captured-instance",
                "difficulty_family": "hard",
                "difficulty_tier": null,
                "terminal_state": "completed",
                "entered_micros": 1,
                "started_micros": 2,
                "first_combat_micros": 3,
                "ended_micros": 4,
                "load_time_micros": 1,
                "precombat_time_micros": 1,
                "total_run_time_micros": 3,
                "game_time_micros": 2,
                "true_time_micros": 2,
                "retry_count": 0,
                "boss_retry_count": 0,
                "rdps_status": "unavailable",
                "apm_status": "unavailable",
                "views": [{
                    "id": "entire-run",
                    "label": "Entire run",
                    "kind": "entire_run",
                    "segment_indices": [0],
                    "elapsed_micros": 2,
                    "active_combat_micros": 1,
                    "actors": [{
                        "actor_id": "player:marierose",
                        "entity_uuid": "216009015936",
                        "monster_id": null,
                        "character_id": null,
                        "display_name": "MarieRose",
                        "actor_kind": "player",
                        "presentation_name": "stale presentation",
                        "presentation_kind": null,
                        "class_id": null,
                        "specialization_id": null,
                        "presentation_class_name": null,
                        "presentation_specialization_name": null,
                        "icon_asset_path": null,
                        "presentation_role": null,
                        "presentation_accent": null,
                        "level": null,
                        "ability_score": null,
                        "weapon_item_id": null,
                        "seasonal_score": null,
                        "primary_loadout": [],
                        "auxiliary_loadout": [],
                        "damage": 1,
                        "effective_damage": 1,
                        "damage_taken": 0,
                        "healing": 0,
                        "effective_healing": 0,
                        "shielding": 0,
                        "hits": 1,
                        "critical_hits": 0,
                        "deaths": 0,
                        "death_seconds": [],
                        "dps": 1.0,
                        "encounter_dps": 1.0,
                        "hps": 0.0,
                        "tps": 0.0,
                        "rdps": null,
                        "apm": null,
                        "observed_cast_events": 0,
                        "abilities": [{
                            "ability_id": "2233",
                            "presentation_name": null,
                            "presentation_kind": null,
                            "presentation_resolution": null,
                            "icon_asset_path": null,
                            "presentation_recount_group_id": null,
                            "presentation_recount_group_name": null,
                            "casts": 0,
                            "hits": 1,
                            "critical_hits": 0,
                            "damage": 1,
                            "effective_damage": 1,
                            "healing": 0,
                            "effective_healing": 0,
                            "shielding": 0,
                            "dps": 1.0,
                            "encounter_dps": 1.0,
                            "hps": 0.0,
                            "targets": []
                        }],
                        "targets": [],
                        "effects": [],
                        "series": []
                    }],
                    "targets": []
                }]
            }]
        }"#,
        )
        .unwrap()
    }

    #[test]
    fn blocked_formula_runtime_clears_saved_history_rdps_without_touching_damage() {
        let mut snapshot = captured_marksman_history();
        snapshot.rdps_formula_identity = Some("sha256:stale".into());
        let run = &mut snapshot.runs[0];
        run.rdps_status = "partial_packet_proven_rules".into();
        let actor = &mut run.views[0].actors[0];
        let ordinary_damage = actor.damage;
        actor.rdps = Some(2.0);
        actor.rdps_damage = Some(2);
        actor.rdps_contribution_given = Some(1);
        actor.rdps_contribution_received = Some(1);

        clear_history_rdps_projection(
            &mut snapshot,
            "formula_runtime_blocked: exact-build promotion proof gates are incomplete",
        );

        assert_eq!(snapshot.rdps_formula_identity, None);
        assert_eq!(
            snapshot.runs[0].rdps_status,
            "formula_runtime_blocked: exact-build promotion proof gates are incomplete"
        );
        let actor = &snapshot.runs[0].views[0].actors[0];
        assert_eq!(actor.damage, ordinary_damage);
        assert_eq!(actor.rdps, None);
        assert_eq!(actor.rdps_damage, None);
        assert_eq!(actor.rdps_contribution_given, None);
        assert_eq!(actor.rdps_contribution_received, None);
    }

    #[test]
    fn stale_history_returns_cached_rdps_and_queues_background_refresh_without_raw_log() {
        let root = temporary_root();
        let controller = RuntimeController::new(root.clone()).unwrap();
        let mut snapshot = captured_marksman_history();
        snapshot.session_id = "history-background-rdps".into();
        snapshot.client_build = state_damage_contribution_game_build().unwrap().into();
        snapshot.protocol_pack_digest = state_damage_contribution_protocol_pack_digest()
            .unwrap()
            .into();
        snapshot.rdps_formula_identity = Some("sha256:stale".into());
        snapshot.runs[0].rdps_status = "partial_packet_proven_rules".into();
        let ordinary_damage = snapshot.runs[0].views[0].actors[0].damage;
        let actor = &mut snapshot.runs[0].views[0].actors[0];
        actor.rdps = Some(2.0);
        actor.rdps_damage = Some(2);
        actor.rdps_contribution_given = Some(1);
        actor.rdps_contribution_received = Some(1);
        controller
            .combat_history
            .lock()
            .unwrap()
            .record(&snapshot, 1)
            .unwrap();

        // No .rlog exists. The detail request must still succeed because raw
        // replay belongs exclusively to the serialized background worker.
        let returned = controller
            .combat_history_detail(CombatHistoryDetailRequest {
                session_id: snapshot.session_id.clone(),
            })
            .unwrap();

        assert_eq!(returned.runs[0].views[0].actors[0].damage, ordinary_damage);
        assert_eq!(
            returned.rdps_formula_identity.as_deref(),
            Some("sha256:stale")
        );
        assert_eq!(
            returned.runs[0].rdps_status,
            "formula_refresh_queued: recalculating archived rDPS in the background"
        );
        let actor = &returned.runs[0].views[0].actors[0];
        assert_eq!(actor.rdps, Some(2.0));
        assert_eq!(actor.rdps_damage, Some(2));
        assert_eq!(actor.rdps_contribution_given, Some(1));
        assert_eq!(actor.rdps_contribution_received, Some(1));
        let progress = controller.history_rdps_backfill.progress_snapshot();
        assert_eq!(progress.len(), 1);
        assert_eq!(progress[0].session_id, snapshot.session_id);
        assert_eq!(progress[0].stage, HistoryRdpsRefreshStage::Queued);
        assert_eq!(
            controller.history_rdps_backfill.next(None).as_deref(),
            Some(snapshot.session_id.as_str())
        );
        assert!(
            !controller
                .history_rdps_backfill
                .enqueue(snapshot.session_id.clone())
        );

        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn interruptible_history_replay_defers_before_touching_the_log() {
        let result = replay_bpsr_combat_history_interruptible(
            Path::new("this-history-log-does-not-exist.rlog"),
            || true,
            |_, _, _| {},
        )
        .unwrap();

        assert_eq!(result, None);
    }

    #[test]
    #[ignore = "set RLOGS_EXTERNAL_RLOG and RLOGS_EXTERNAL_HISTORY_DETAIL to reviewed artifacts"]
    fn external_history_rdps_refresh_preview_is_conserved() {
        let rlog = std::env::var_os("RLOGS_EXTERNAL_RLOG")
            .map(PathBuf::from)
            .expect("RLOGS_EXTERNAL_RLOG must name a reviewed sealed log");
        let history_path = std::env::var_os("RLOGS_EXTERNAL_HISTORY_DETAIL")
            .map(PathBuf::from)
            .expect("RLOGS_EXTERNAL_HISTORY_DETAIL must name its saved detail artifact");
        let saved: CombatHistorySnapshot = serde_json::from_reader(BufReader::new(
            File::open(&history_path).expect("saved history detail should open"),
        ))
        .expect("saved history detail should decode");
        let projection = replay_bpsr_combat_history(&rlog).expect("sealed replay should project");
        let live_projection = replay_bpsr_combat_history_live_one_pass(&rlog)
            .expect("one-pass live replay should project");
        let refreshed = combat_history::merge_rdps_projection(&saved, &projection)
            .expect("current formula projection should match the saved ordinary combat cube");

        assert_eq!(refreshed.session_id, saved.session_id);
        assert_eq!(refreshed.runs.len(), saved.runs.len());
        for (run, saved_run) in refreshed.runs.iter().zip(&saved.runs) {
            assert_eq!(run.rdps_status, "partial_packet_proven_rules");
            for (view, saved_view) in run.views.iter().zip(&saved_run.views) {
                assert_eq!(view.id, saved_view.id);
                assert_eq!(
                    view.actors.iter().map(|actor| actor.damage).sum::<i64>(),
                    saved_view
                        .actors
                        .iter()
                        .map(|actor| actor.damage)
                        .sum::<i64>()
                );
                let mut given = 0_i128;
                let mut received = 0_i128;
                let mut unresolved = 0_usize;
                for actor in &view.actors {
                    match (
                        actor.rdps_damage,
                        actor.rdps_contribution_given,
                        actor.rdps_contribution_received,
                    ) {
                        (Some(_), Some(actor_given), Some(actor_received)) => {
                            given += i128::from(actor_given);
                            received += i128::from(actor_received);
                        }
                        (None, None, None) => unresolved += 1,
                        _ => panic!("actor {} has a partial rDPS tuple", actor.actor_id),
                    }
                }
                if unresolved == 0 {
                    assert_eq!(given, received);
                }
                let attributed = view
                    .damage_influences
                    .iter()
                    .try_fold(0_i128, |total, influence| {
                        let amount = influence
                            .attributed_rdps
                            .as_deref()
                            .unwrap_or("0")
                            .parse::<i128>()
                            .map_err(|_| "invalid exact influence amount".to_string())?;
                        total
                            .checked_add(amount)
                            .ok_or_else(|| "exact influence sum overflowed".to_string())
                    })
                    .expect("exact influence amounts should be valid and bounded");
                eprintln!(
                    "run={} view={} actors={} resolved_given={given} resolved_received={received} unresolved_actors={unresolved} attributed_rows={attributed}",
                    run.run_index,
                    view.id,
                    view.actors.len()
                );
                for influence in view.damage_influences.iter().filter(|influence| {
                    influence.effect_id == "3003052"
                        && influence.attribution_component.as_deref()
                            == Some("harmony-grace-remote-paired-output")
                }) {
                    eprintln!(
                        "remote_harmony provider={} recipient={} ability={:?} target={:?} events={} observed_damage={} exact_delta={} attributed_rdps={:?}",
                        influence.provider_actor_id,
                        influence.recipient_actor_id,
                        influence.affected_ability_id,
                        influence.target_actor_id,
                        influence.damage_event_count,
                        influence.observed_damage,
                        influence.exact_integer_delta,
                        influence.attributed_rdps,
                    );
                }
                if view.id == "all" {
                    for actor in view.actors.iter().filter(|actor| {
                        actor.damage > 0 && actor.actor_kind.as_deref() == Some("player")
                    }) {
                        eprintln!(
                            "party actor={} name={} damage={} rdps_damage={:?} given={:?} received={:?} incomplete={}",
                            actor.actor_id,
                            actor.display_name.as_deref().unwrap_or("unknown"),
                            actor.damage,
                            actor.rdps_damage,
                            actor.rdps_contribution_given,
                            actor.rdps_contribution_received,
                            actor.rdps_incomplete,
                        );
                    }
                }
            }
        }
        eprintln!(
            "formula_identity={}",
            refreshed.rdps_formula_identity.as_deref().unwrap()
        );
        let history_remote_harmony = refreshed
            .runs
            .iter()
            .flat_map(|run| run.views.iter())
            .filter(|view| view.id == "all")
            .flat_map(|view| view.damage_influences.iter())
            .filter(|influence| {
                influence.effect_id == "3003052"
                    && influence.attribution_component.as_deref()
                        == Some("harmony-grace-remote-paired-output")
            })
            .map(|influence| {
                influence
                    .attributed_rdps
                    .as_deref()
                    .unwrap_or("0")
                    .parse::<i128>()
                    .expect("history remote Harmony amount should be exact")
            })
            .sum::<i128>();
        let live_remote_harmony = live_projection
            .runs
            .iter()
            .flat_map(|run| run.views.iter())
            .find(|view| view.id == "all")
            .expect("live replay should include the all view")
            .damage_influences
            .iter()
            .filter(|influence| {
                influence.effect_id == "3003052"
                    && influence.attribution_component.as_deref()
                        == Some("harmony-grace-remote-paired-output")
            })
            .map(|influence| {
                influence
                    .attributed_rdps
                    .as_deref()
                    .unwrap_or("0")
                    .parse::<i128>()
                    .expect("live remote Harmony amount should be exact")
            })
            .sum::<i128>();
        eprintln!("live_one_pass_remote_harmony={live_remote_harmony}");
        assert!(
            live_remote_harmony > 0,
            "the live projector should attribute at least one proven remote Harmony row"
        );
        assert_eq!(
            live_remote_harmony, history_remote_harmony,
            "one-pass live and sealed two-pass history must transfer the same remote Harmony amount"
        );
    }

    #[test]
    #[ignore = "set RLOGS_EXTERNAL_RLOG to an explicitly reviewed local capture"]
    fn external_live_remote_rdps_preview_is_conserved() {
        let rlog = std::env::var_os("RLOGS_EXTERNAL_RLOG")
            .map(PathBuf::from)
            .expect("RLOGS_EXTERNAL_RLOG must name a reviewed sealed log");
        let (_, combat_report, _) = replay_rlog_pair(
            BufReader::new(File::open(rlog).expect("reviewed rlog should open")),
            bpsr_combat_timeline_plugin().expect("live BPSR projector should build"),
            EncounterRecorderPlugin::new(
                bundled_run_reducer_config().expect("run reducer config should load"),
            ),
            RlogLimits::default(),
            PluginRunLimits::default(),
            PluginRunLimits::default(),
        )
        .expect("reviewed rlog should replay through the live projector");
        let combat =
            combat_timeline_snapshot(&combat_report).expect("live combat projection should decode");
        let players = combat
            .actors
            .iter()
            .filter(|actor| actor.actor_kind.as_deref() == Some("player"))
            .collect::<Vec<_>>();
        let given = players
            .iter()
            .map(|actor| i128::from(actor.rdps_contribution_given.unwrap_or_default()))
            .sum::<i128>();
        let received = players
            .iter()
            .map(|actor| i128::from(actor.rdps_contribution_received.unwrap_or_default()))
            .sum::<i128>();
        for actor in &players {
            eprintln!(
                "live party actor={} name={} damage={} rdps_damage={:?} given={:?} received={:?} incomplete={}",
                actor.actor_id,
                actor.display_name.as_deref().unwrap_or("unknown"),
                actor.reported_damage,
                actor.rdps_damage,
                actor.rdps_contribution_given,
                actor.rdps_contribution_received,
                actor.rdps_incomplete,
            );
        }
        assert_eq!(given, received);
        assert!(
            players.iter().any(|actor| {
                actor.actor_id != "17" && actor.rdps_contribution_received.unwrap_or_default() > 0
            }),
            "at least one remote teammate must receive packet-reconstructed live attribution"
        );
    }

    #[test]
    fn history_presentation_uses_only_captured_run_evidence() {
        let mut snapshot = captured_marksman_history();

        enrich_bpsr_history_presentation(&mut snapshot, "en-US").unwrap();

        let actor = &snapshot.runs[0].views[0].actors[0];
        assert_eq!(actor.display_name.as_deref(), Some("MarieRose"));
        assert_eq!(actor.presentation_name.as_deref(), Some("MarieRose"));
        assert_eq!(actor.class_id, Some(11));
        assert_eq!(actor.presentation_class_name.as_deref(), Some("Marksman"));
        assert_eq!(actor.specialization_id, Some(117));
        assert_eq!(
            actor.presentation_specialization_name.as_deref(),
            Some("Falconry")
        );
        assert_eq!(actor.level, None);
        assert_eq!(actor.ability_score, None);
        assert_eq!(actor.weapon_item_id, None);
        assert_eq!(actor.seasonal_score, None);
        assert!(actor.primary_loadout.is_empty());
        assert!(actor.auxiliary_loadout.is_empty());
    }

    #[test]
    fn saved_history_resolves_public_name_without_borrowing_current_build_state() {
        let root = temporary_root();
        std::fs::create_dir_all(&root).unwrap();
        let path = root.join("current-character.json");
        let catalog = serde_json::json!({
            "schemaVersion": 1,
            "entries": [{
                "gamePluginId": BPSR_GAME_PLUGIN_ID,
                "deploymentId": "global",
                "regionId": "global",
                "worldId": null,
                "characterId": "3296036",
                "displayName": "MarieRose",
                "classId": 13,
                "specializationId": 119,
                "level": 60,
                "abilityScore": 99_999,
                "weaponItemId": 2_000_999,
                "weaponBreakthroughCount": 9,
                "seasonalStrength": 9_999,
                "primaryLoadout": [],
                "auxiliaryLoadout": []
            }]
        });
        std::fs::write(&path, serde_json::to_vec(&catalog).unwrap()).unwrap();
        let current = CharacterIdentityStore::open(path).unwrap();
        let mut snapshot = captured_marksman_history();
        snapshot.world_id = None;
        let actor = &mut snapshot.runs[0].views[0].actors[0];
        actor.display_name = Some("Player 6".into());
        actor.character_id = Some("3296036".into());
        actor.ability_score = Some(61_734);

        enrich_bpsr_history_public_names(&mut snapshot, &current).unwrap();
        enrich_bpsr_history_presentation(&mut snapshot, "en-US").unwrap();

        let actor = &snapshot.runs[0].views[0].actors[0];
        assert_eq!(actor.display_name.as_deref(), Some("MarieRose"));
        assert_eq!(actor.presentation_name.as_deref(), Some("MarieRose"));
        assert_eq!(actor.class_id, Some(11));
        assert_eq!(actor.specialization_id, Some(117));
        assert_eq!(actor.ability_score, Some(61_734));
        assert_ne!(actor.specialization_id, Some(119));
        assert_ne!(actor.weapon_item_id, Some(2_000_999));

        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn saved_history_uses_exact_public_uid_until_a_name_is_observed() {
        let root = temporary_root();
        std::fs::create_dir_all(&root).unwrap();
        let current =
            CharacterIdentityStore::open(root.join("empty-character-catalog.json")).unwrap();
        let mut snapshot = captured_marksman_history();
        let actor = &mut snapshot.runs[0].views[0].actors[0];
        actor.display_name = Some("Player 6".into());
        actor.character_id = None;

        enrich_bpsr_history_public_names(&mut snapshot, &current).unwrap();

        let actor = &snapshot.runs[0].views[0].actors[0];
        assert_eq!(actor.character_id.as_deref(), Some("3296036"));
        assert_eq!(actor.display_name.as_deref(), Some("UID 3296036"));
        assert!(actor_display_name_needs_identity(
            actor.display_name.as_deref()
        ));

        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn history_recount_parent_does_not_replace_the_raw_child_action() {
        let mut snapshot = captured_marksman_history();
        snapshot.runs[0].views[0].actors[0].abilities[0].ability_id = "220106".into();

        enrich_bpsr_history_presentation(&mut snapshot, "en-US").unwrap();

        let ability = &snapshot.runs[0].views[0].actors[0].abilities[0];
        assert_eq!(ability.presentation_name.as_deref(), Some("Bullseye"));
        assert_eq!(ability.presentation_recount_group_id.as_deref(), Some("77"));
        assert_eq!(
            ability.presentation_recount_group_name.as_deref(),
            Some("Double Arrow")
        );
    }

    #[test]
    fn capture_time_character_state_is_frozen_and_never_replaced() {
        fn identity_store(
            root: &Path,
            name: &str,
            class_id: i32,
            specialization_id: i32,
            ability_score: i64,
            primary_item_id: i64,
        ) -> CharacterIdentityStore {
            let path = root.join(format!("{name}.json"));
            let catalog = serde_json::json!({
                "schemaVersion": 1,
                "entries": [{
                    "gamePluginId": BPSR_GAME_PLUGIN_ID,
                    "deploymentId": "global",
                    "regionId": "global",
                    "worldId": "asteria",
                    "characterId": "3296036",
                    "displayName": "MarieRose",
                    "classId": class_id,
                    "specializationId": specialization_id,
                    "level": 60,
                    "abilityScore": ability_score,
                    "weaponItemId": 2_000_631,
                    "weaponBreakthroughCount": 3,
                    "seasonalStrength": 3_505,
                    "primaryLoadout": [{
                        "slot_id": 7,
                        "ability_id": 3_948,
                        "item_id": primary_item_id,
                        "tier": 5
                    }],
                    "auxiliaryLoadout": [{
                        "slot_id": 21,
                        "ability_id": 3_612,
                        "item_id": null,
                        "tier": null
                    }]
                }]
            });
            std::fs::write(&path, serde_json::to_vec(&catalog).unwrap()).unwrap();
            CharacterIdentityStore::open(path).unwrap()
        }

        let root = temporary_root();
        std::fs::create_dir_all(&root).unwrap();
        let marksman = identity_store(&root, "marksman", 11, 117, 61_382, 3_000_101);
        let mut snapshot = captured_marksman_history();
        freeze_bpsr_history_character_state(&mut snapshot, &marksman).unwrap();

        let actor = &snapshot.runs[0].views[0].actors[0];
        assert_eq!(actor.class_id, Some(11));
        assert_eq!(actor.specialization_id, Some(117));
        assert_eq!(actor.level, Some(60));
        assert_eq!(actor.ability_score, Some(61_382));
        assert_eq!(actor.weapon_item_id, Some(2_000_631));
        assert_eq!(actor.weapon_breakthrough_count, Some(3));
        assert_eq!(actor.seasonal_score, Some(3_505));
        assert_eq!(actor.primary_loadout[0].item_id, Some(3_000_101));
        assert_eq!(actor.auxiliary_loadout[0].ability_id, Some(3_612));

        let current_dissonance =
            identity_store(&root, "current-dissonance", 13, 119, 99_999, 3_000_043);
        freeze_bpsr_history_character_state(&mut snapshot, &current_dissonance).unwrap();
        enrich_bpsr_history_presentation(&mut snapshot, "en-US").unwrap();

        let actor = &snapshot.runs[0].views[0].actors[0];
        assert_eq!(actor.presentation_class_name.as_deref(), Some("Marksman"));
        assert_eq!(actor.specialization_id, Some(117));
        assert_ne!(
            actor.presentation_specialization_name.as_deref(),
            Some("Dissonance")
        );
        assert_eq!(actor.ability_score, Some(61_382));
        assert_eq!(actor.primary_loadout[0].item_id, Some(3_000_101));
        assert_eq!(
            actor.weapon_icon_asset_path.as_deref(),
            Some(
                "/game-assets/blue-protocol-star-resonance/shared/icons/weapons/items/ch_wp_rodri_06_01.png"
            )
        );
        assert_eq!(
            actor.weapon_presentation_name.as_deref(),
            Some("Ember - Gaze of the Far Sea")
        );
        assert_eq!(actor.weapon_level, Some(280));
        assert_eq!(actor.weapon_level_min, Some(220));
        assert_eq!(actor.weapon_level_max, Some(280));
        assert_eq!(actor.weapon_badge_kind.as_deref(), Some("ember_far_sea"));

        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn auxiliary_loadout_enrichment_only_joins_bundled_presentation() {
        let mut slots = vec![
            HistoryLoadoutSlot {
                slot_id: 21,
                ability_id: Some(3_021),
                item_id: Some(3_000_009),
                tier: Some(5),
                presentation_name: None,
                icon_asset_path: None,
                item_tier: None,
                maximum_tier: None,
            },
            HistoryLoadoutSlot {
                slot_id: 22,
                ability_id: Some(3_612),
                item_id: None,
                tier: None,
                presentation_name: None,
                icon_asset_path: None,
                item_tier: None,
                maximum_tier: None,
            },
        ];

        enrich_bpsr_loadout_presentation(&mut slots, "en-US").unwrap();

        assert_eq!(slots[0].ability_id, Some(3_021));
        assert_eq!(slots[0].item_id, Some(3_000_009));
        assert_eq!(slots[0].tier, Some(5));
        assert_eq!(slots[0].maximum_tier, Some(5));
        assert_eq!(
            slots[0].presentation_name.as_deref(),
            Some("Thunderfall Grasp")
        );
        assert!(
            slots[0]
                .icon_asset_path
                .as_deref()
                .is_some_and(|path| path.ends_with("3021-thunderfall-grasp.png"))
        );
        assert_eq!(
            slots[1].presentation_name.as_deref(),
            Some("Unyielding Spirit")
        );
        assert_eq!(slots[1].tier, None);
    }

    #[test]
    fn live_combat_feed_wakes_waiters_on_the_next_revision() {
        let feed = Arc::new(LiveCombatFeed::default());
        let publisher = Arc::clone(&feed);
        let worker = thread::spawn(move || {
            thread::sleep(Duration::from_millis(10));
            publisher.publish(None);
        });

        let update = feed.wait_after(0, Duration::from_secs(1));
        worker.join().unwrap();
        assert_eq!(update.schema_version, LIVE_COMBAT_FEED_SCHEMA_VERSION);
        assert_eq!(update.revision, 1);
        assert!(update.snapshot.is_none());
    }

    #[test]
    fn live_combat_activity_wakes_directly_on_decoded_damage() {
        let feed = Arc::new(LiveCombatFeed::default());
        let publisher = Arc::clone(&feed);
        let worker = thread::spawn(move || {
            thread::sleep(Duration::from_millis(10));
            publisher.signal_damage(42_000, false);
        });

        let update = feed.wait_activity_after(0, Duration::from_secs(1));
        worker.join().unwrap();
        assert_eq!(update.revision, 1);
        assert!(update.combat_active);
        assert_eq!(update.last_hostile_micros, Some(42_000));
        assert_eq!(update.damage_event_count, 1);
        assert!(feed.current().snapshot.is_none());
    }

    #[test]
    fn decoded_damage_cannot_starve_the_next_complete_live_snapshot() {
        let feed = LiveCombatFeed::default();
        feed.publish(None);
        let first_snapshot_revision = feed.current().revision;
        assert_eq!(first_snapshot_revision, 1);

        // Real capture order is damage visibility first, then bounded reducer
        // publication. Repeated damage must not make browser consumers believe
        // they already received revisions that still contain the old snapshot.
        for observed_micros in 1..=64 {
            feed.signal_damage(observed_micros, false);
        }
        assert_eq!(feed.current().revision, first_snapshot_revision);
        assert_eq!(feed.current_activity().revision, 65);

        let unchanged = feed.wait_after(first_snapshot_revision, Duration::from_millis(1));
        assert_eq!(unchanged.revision, first_snapshot_revision);

        feed.publish(None);
        let reduced = feed.wait_after(first_snapshot_revision, Duration::from_millis(1));
        assert_eq!(reduced.revision, first_snapshot_revision + 1);
    }

    #[test]
    fn ambient_live_clock_counts_only_nearby_damage_windows() {
        let feed = LiveCombatFeed::default();

        feed.signal_damage(1_000_000, false);
        feed.signal_damage(2_000_000, false);
        feed.signal_damage(8_000_000, false);

        let state = feed
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        assert_eq!(state.ambient_active_micros, 1_000_000);
        assert_eq!(state.ambient_last_damage_micros, Some(8_000_000));
    }

    #[test]
    fn combat_history_feed_distinguishes_progress_from_saved_catalog_changes() {
        let feed = Arc::new(CombatHistoryRevisionFeed::default());
        let publisher = Arc::clone(&feed);
        let worker = thread::spawn(move || {
            thread::sleep(Duration::from_millis(10));
            publisher.publish_progress();
        });

        let (revision, catalog_changed) = feed.wait_after(0, Duration::from_secs(1));
        worker.join().unwrap();
        assert_eq!(revision, 1);
        assert!(!catalog_changed);

        feed.publish();
        let (saved_revision, catalog_changed) = feed.wait_after(revision, Duration::from_millis(1));
        assert_eq!(saved_revision, 2);
        assert!(catalog_changed);

        let unchanged = feed.wait_after(saved_revision, Duration::from_millis(1));
        assert_eq!(unchanged, (saved_revision, false));
    }

    #[test]
    fn live_event_feed_batches_id_first_lines_without_canonical_json() {
        let feed = LiveEventFeed::default();
        feed.reset("live-session".into());
        feed.publish_batch(vec![LiveEventLine {
            revision: 0,
            sequence: 7,
            observed_micros: 99,
            topic: EventTopic::Combat,
            kind: "damage".into(),
            raw_ids: "entity:20 -> entity:30 · ability:40 · amount:50".into(),
        }]);

        let batch = feed.wait_after(0, Duration::from_millis(1), 16, false);
        assert_eq!(batch.session_id.as_deref(), Some("live-session"));
        assert_eq!(batch.events.len(), 1);
        assert_eq!(batch.events[0].sequence, 7);
        assert_eq!(batch.events[0].kind, "damage");
        assert!(!batch.has_more);
    }

    #[test]
    fn live_event_line_exposes_the_exact_data_gap_reason() {
        let time = EventTime {
            observed_micros: 99,
            game_time_millis: None,
        };
        let provenance = EventProvenance::wire(1, 45, 7);
        let event = EventEnvelope {
            schema_version: EVENT_SCHEMA_VERSION,
            session_id: "live-session".into(),
            sequence: 7,
            region: RegionContext {
                identity: RegionIdentity {
                    deployment_id: "global".into(),
                    region_id: "global".into(),
                    realm_id: None,
                    world_id: Some("asteria".into()),
                },
                client_build: "24687926".into(),
                protocol_pack_digest: "sha256:fixture".into(),
                evidence: Vec::new(),
            },
            time,
            provenance: provenance.clone(),
            sensitivity: EventSensitivity::PublicGameplay,
            event: CanonicalEvent::Timeline(TimelineEvent {
                sequence: 7,
                time,
                provenance,
                kind: TimelineEventKind::DataGap(rlogs_events::DataGapEvent {
                    kind: DataGapKind::UnknownRoute,
                    connection_id: Some(1),
                    stream_id: Some(2),
                    detail: "WorldNtf notify method 45 was not decoded".into(),
                }),
            }),
        };

        let line = LiveEventLine::from_envelope(&event);
        assert_eq!(line.kind, "data_gap");
        assert_eq!(
            line.raw_ids,
            "data_gap | unknown_route | WorldNtf notify method 45 was not decoded"
        );
    }

    #[test]
    fn live_event_feed_can_open_at_the_tail_without_draining_session_history() {
        let feed = LiveEventFeed::default();
        feed.reset("live-session".into());
        feed.publish_batch(
            (1..=20)
                .map(|sequence| LiveEventLine {
                    revision: 0,
                    sequence,
                    observed_micros: sequence,
                    topic: EventTopic::Actor,
                    kind: "actor".into(),
                    raw_ids: format!("entity:{sequence}"),
                })
                .collect(),
        );

        let batch = feed.wait_after(0, Duration::from_millis(1), 5, true);
        assert_eq!(batch.events.len(), 5);
        assert_eq!(batch.events[0].sequence, 16);
        assert_eq!(batch.events[4].sequence, 20);
        assert_eq!(batch.dropped_before, 16);
        assert!(!batch.has_more);
    }

    #[test]
    fn capture_time_history_does_not_read_or_build_the_submission_artifact() {
        let region = RegionContext {
            identity: RegionIdentity {
                deployment_id: "global".into(),
                region_id: "global".into(),
                realm_id: None,
                world_id: Some("asteria".into()),
            },
            client_build: "fixture".into(),
            protocol_pack_digest: "sha256:fixture".into(),
            evidence: Vec::new(),
        };
        let header = RlogHeader::new("monitor", region, "unit-test");
        let mut meter = CombatTimelinePlugin::new();
        meter.begin_live(&header);
        let mut encounter = EncounterRecorderPlugin::default();
        encounter.begin_live(&header);
        let missing_path = temporary_root().join("intentionally-not-read.rlog");
        let log = SealedDungeonRunLog {
            session_id: "monitor.run-0001".into(),
            path: missing_path.clone(),
            start_reason: DungeonSegmentStartReason::Entered,
            end_reason: DungeonSegmentEndReason::Completed,
            started: DungeonSegmentBoundary {
                instance_id: Some("instance".into()),
                time: EventTime {
                    observed_micros: 10,
                    game_time_millis: None,
                },
            },
            ended: DungeonSegmentBoundary {
                instance_id: Some("instance".into()),
                time: EventTime {
                    observed_micros: 20,
                    game_time_millis: None,
                },
            },
            seal: RlogSeal {
                event_count: 0,
                content_sha256: "sha256:fixture".into(),
            },
        };

        let run = encounter.live_snapshot().unwrap();
        let history = meter.history_snapshot(&run.runs).unwrap();
        let result = capture_time_continuous_run_result(
            &log,
            CapturedRunProjection {
                combat: meter.live_snapshot().unwrap(),
                run,
                history,
            },
            false,
            &submission_policy::ProfileSyncPolicy {
                enabled: false,
                automatic_profiles: true,
            },
        )
        .unwrap();

        assert!(!missing_path.exists());
        assert!(result.upload_artifact.is_none());
        assert!(result.verified_artifact.is_none());
        assert_eq!(result.submission_queue_status, "disabled");
        assert_eq!(result.profile_sync_status, "disabled");
    }

    #[test]
    #[ignore = "set RLOGS_EXTERNAL_RLOG to an explicitly reviewed local capture"]
    fn external_scene_1632_capture_replays_into_mobbing_and_boss_history() {
        let path = std::env::var_os("RLOGS_EXTERNAL_RLOG")
            .map(PathBuf::from)
            .expect("RLOGS_EXTERNAL_RLOG must name the reviewed local capture");
        let file = File::open(&path).unwrap();
        let config = bundled_run_reducer_config().unwrap();
        let (_, combat_report, encounter_report) = replay_rlog_pair(
            BufReader::new(file),
            CombatTimelinePlugin::new(),
            EncounterRecorderPlugin::new(config),
            RlogLimits::default(),
            PluginRunLimits::default(),
            PluginRunLimits::default(),
        )
        .unwrap();
        let combat = combat_timeline_snapshot(&combat_report).unwrap();
        let projection = run_projection_snapshot(&encounter_report).unwrap();
        let run = projection
            .runs
            .iter()
            .find(|run| run.identity.scene_id == Some(1632))
            .expect("scene 1632 run projection");

        assert!(combat.active_combat_micros > 0);
        assert_eq!(combat.data_gap_count, 0);
        assert!(combat.actors.iter().any(|actor| actor.dps > 0.0));
        assert_eq!(
            serde_json::to_value(run.terminal_state).unwrap(),
            serde_json::json!("completed")
        );
        assert!(run.authoritative_start);
        assert!(run.authoritative_completion);
        assert_eq!(run.segments.len(), 2);
        assert_eq!(
            serde_json::to_value(run.segments[0].kind).unwrap(),
            serde_json::json!("mobbing")
        );
        assert_eq!(
            serde_json::to_value(run.segments[1].kind).unwrap(),
            serde_json::json!("boss")
        );
        assert_eq!(run.encounters.len(), 2);
        assert_eq!(run.data_gap_count, 0);
        assert!(
            run.encounters
                .iter()
                .all(|encounter| encounter.is_successful_attempt)
        );
        eprintln!(
            "scene=1632 events={} combat_us={} actors={} run_us={} segments={} encounters={}",
            combat.event_count,
            combat.active_combat_micros,
            combat.actors.len(),
            run.timing.active_combat_micros,
            run.segments.len(),
            run.encounters.len()
        );

        if let Some(install_root) = std::env::var_os("RLOGS_HISTORY_IMPORT_ROOT") {
            let mut reader = RlogReader::new(
                BufReader::new(File::open(&path).unwrap()),
                RlogLimits::default(),
            )
            .unwrap();
            let mut meter = CombatTimelinePlugin::new();
            meter.begin_live(reader.header());
            let mut encounter = EncounterRecorderPlugin::new(bundled_run_reducer_config().unwrap());
            encounter.begin_live(reader.header());
            while let Some(event) = reader.next_event().unwrap() {
                meter.observe_live(&event);
                encounter.observe_live(&event).unwrap();
            }
            let run_projection = encounter.live_snapshot().unwrap();
            let history = meter.history_snapshot(&run_projection.runs).unwrap();
            for required_view in ["all", "mobbing", "boss"] {
                assert!(
                    history.runs[0]
                        .views
                        .iter()
                        .any(|view| view.id == required_view),
                    "history is missing required {required_view} view"
                );
            }
            let mapped_targets = history
                .runs
                .iter()
                .flat_map(|run| &run.views)
                .flat_map(|view| &view.targets)
                .filter(|target| target.monster_id.is_some())
                .count();
            assert!(
                mapped_targets > 0,
                "reviewed capture did not preserve any packet-derived monster IDs"
            );
            assert!(history.runs[0].views[0].actors.iter().any(|actor| {
                actor.actor_kind.as_deref() == Some("player") && actor.encounter_dps > 0.0
            }));
            eprintln!("history mapped static monster targets={mapped_targets}");
            let captured_unix_millis = std::fs::metadata(&path)
                .and_then(|metadata| metadata.modified())
                .ok()
                .and_then(|modified| modified.duration_since(UNIX_EPOCH).ok())
                .map_or_else(unix_millis, |duration| duration.as_millis() as u64);
            CombatHistoryStore::open(
                PathBuf::from(install_root).join("runtime-data/history/combat-meter"),
            )
            .unwrap()
            .record(&history, captured_unix_millis)
            .unwrap();
        }
    }

    #[test]
    #[ignore = "set RLOGS_EXTERNAL_RLOG to an explicitly reviewed local capture"]
    fn external_dungeon_boundary_evidence() {
        let path = std::env::var_os("RLOGS_EXTERNAL_RLOG")
            .map(PathBuf::from)
            .expect("RLOGS_EXTERNAL_RLOG must name the reviewed local capture");
        let mut reader = RlogReader::new(
            BufReader::new(File::open(&path).unwrap()),
            RlogLimits::default(),
        )
        .unwrap();

        while let Some(event) = reader.next_event().unwrap() {
            match &event.event {
                CanonicalEvent::WorldChanged(world) => eprintln!(
                    "world seq={} us={} scene={:?} map={:?} instance={:?}",
                    event.sequence,
                    event.time.observed_micros,
                    world.scene_id.map(|scene| scene.0),
                    world.map_id,
                    world.dungeon_instance_id,
                ),
                CanonicalEvent::Dungeon(dungeon) => eprintln!(
                    "dungeon seq={} us={} kind={:?} difficulty={:?} objective_map={:?} objective={:?} value={:?} complete={:?} flow={:?}",
                    event.sequence,
                    event.time.observed_micros,
                    dungeon.kind,
                    dungeon.difficulty_id,
                    dungeon.objective_map_key,
                    dungeon.objective_id,
                    dungeon.objective_value,
                    dungeon.objective_complete,
                    dungeon.flow,
                ),
                CanonicalEvent::Timeline(timeline)
                    if matches!(timeline.kind, TimelineEventKind::RunBoundary { .. }) =>
                {
                    eprintln!(
                        "run-boundary seq={} us={} kind={:?}",
                        event.sequence, event.time.observed_micros, timeline.kind
                    );
                }
                _ => {}
            }
        }
    }

    #[test]
    #[ignore = "set RLOGS_EXTERNAL_RLOG to an explicitly reviewed local capture"]
    fn external_live_identity_order_evidence() {
        let path = std::env::var_os("RLOGS_EXTERNAL_RLOG")
            .map(PathBuf::from)
            .expect("RLOGS_EXTERNAL_RLOG must name the reviewed local capture");
        let mut reader = RlogReader::new(
            BufReader::new(File::open(&path).unwrap()),
            RlogLimits::default(),
        )
        .unwrap();
        let mut meter = bpsr_combat_timeline_plugin().unwrap();
        meter.begin_live(reader.header());
        let mut capture_identities = CaptureTimeCharacterIdentityStore::default();
        let persistent_identity_path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../runtime-data/identity/characters.v1.json");
        let mut persistent_identities =
            CharacterIdentityStore::open(persistent_identity_path).unwrap();
        let mut first_damage = BTreeSet::new();
        let mut identity_versions = BTreeSet::new();

        while let Some(event) = reader.next_event().unwrap() {
            if event.event.topic() == EventTopic::CharacterProfile {
                let _ = persistent_identities.observe(&event);
                capture_identities
                    .observe_with_name_fallback(&event, &persistent_identities)
                    .unwrap();
            }
            if matches!(
                event.event.topic(),
                EventTopic::World
                    | EventTopic::Actor
                    | EventTopic::Combat
                    | EventTopic::Encounter
                    | EventTopic::Dungeon
                    | EventTopic::DataQuality
            ) {
                meter.observe_live(&event);
            }
            match &event.event {
                CanonicalEvent::CharacterProfileObserved { profile } => eprintln!(
                    "profile seq={} us={} payload={}",
                    event.sequence,
                    event.time.observed_micros,
                    serde_json::to_string(profile).unwrap()
                ),
                CanonicalEvent::Timeline(timeline) => match &timeline.kind {
                    TimelineEventKind::Actor(actor)
                        if actor.display_name.is_some()
                            || actor.class_id.is_some()
                            || actor.specialization_id.is_some()
                            || actor.weapon_item_id.is_some()
                            || !actor.primary_loadout.is_empty()
                            || !actor.auxiliary_loadout.is_empty() =>
                    {
                        let identity_key = (
                            actor.actor.actor_id.0,
                            actor.actor.entity_uuid.0,
                            actor.display_name.clone(),
                            actor.class_id,
                            actor.specialization_id,
                            actor.weapon_item_id,
                            actor.primary_loadout.len(),
                            actor.auxiliary_loadout.len(),
                        );
                        if identity_versions.insert(identity_key) {
                            eprintln!(
                                "actor seq={} us={} actor_id={} entity_uuid={} kind={:?} name={:?} class={:?} spec={:?} weapon={:?} primary={:?} auxiliary={:?}",
                                event.sequence,
                                event.time.observed_micros,
                                actor.actor.actor_id.0,
                                actor.actor.entity_uuid.0,
                                actor.kind,
                                actor.display_name,
                                actor.class_id,
                                actor.specialization_id,
                                actor.weapon_item_id,
                                actor.primary_loadout,
                                actor.auxiliary_loadout,
                            );
                        }
                    }
                    TimelineEventKind::Damage(damage) => {
                        let damage_key = (damage.source.actor_id.0, damage.source.entity_uuid.0);
                        if first_damage.insert(damage_key) {
                            let mut snapshot = meter.live_overlay_snapshot().unwrap();
                            enrich_bpsr_live_character_state(
                                &mut snapshot,
                                &capture_identities,
                                LiveCharacterIdentityAuthority::CaptureTime,
                            );
                            enrich_bpsr_live_character_state(
                                &mut snapshot,
                                &persistent_identities,
                                LiveCharacterIdentityAuthority::PersistentFallback,
                            );
                            let live_actor = snapshot.actors.iter().find(|actor| {
                                actor.actor_id == damage.source.actor_id.0.to_string()
                                    && actor.entity_uuid == damage.source.entity_uuid.0.to_string()
                            });
                            eprintln!(
                                "first-damage seq={} us={} actor_id={} entity_uuid={} target_actor_id={} target_entity_uuid={} ability={} amount={} live_actor={:?}",
                                event.sequence,
                                event.time.observed_micros,
                                damage.source.actor_id.0,
                                damage.source.entity_uuid.0,
                                damage.target.actor_id.0,
                                damage.target.entity_uuid.0,
                                damage.ability.map(|ability| ability.0).unwrap_or_default(),
                                damage.amount,
                                live_actor,
                            );
                        }
                    }
                    _ => {}
                },
                _ => {}
            }
        }
    }

    #[test]
    #[ignore = "set RLOGS_EXTERNAL_RLOG to an explicitly reviewed local capture"]
    fn external_combat_capture_builds_nonzero_history_bars() {
        let path = std::env::var_os("RLOGS_EXTERNAL_RLOG")
            .map(PathBuf::from)
            .expect("RLOGS_EXTERNAL_RLOG must name the reviewed local capture");
        let mut reader = RlogReader::new(
            BufReader::new(File::open(&path).unwrap()),
            RlogLimits::default(),
        )
        .unwrap();
        let mut meter = CombatTimelinePlugin::new();
        meter.begin_live(reader.header());
        let mut encounter = EncounterRecorderPlugin::new(bundled_run_reducer_config().unwrap());
        encounter.begin_live(reader.header());

        while let Some(event) = reader.next_event().unwrap() {
            match &event.event {
                CanonicalEvent::WorldChanged(world) => eprintln!(
                    "world seq={} us={} scene={:?} map={:?} instance={:?}",
                    event.sequence,
                    event.time.observed_micros,
                    world.scene_id.map(|scene| scene.0),
                    world.map_id,
                    world.dungeon_instance_id,
                ),
                CanonicalEvent::Dungeon(dungeon) => eprintln!(
                    "dungeon seq={} us={} kind={:?} difficulty={:?} objective_map={:?} objective={:?} value={:?} complete={:?} flow={:?}",
                    event.sequence,
                    event.time.observed_micros,
                    dungeon.kind,
                    dungeon.difficulty_id,
                    dungeon.objective_map_key,
                    dungeon.objective_id,
                    dungeon.objective_value,
                    dungeon.objective_complete,
                    dungeon.flow,
                ),
                _ => {}
            }
            meter.observe_live(&event);
            encounter.observe_live(&event).unwrap();
        }

        let projection = encounter.live_snapshot().unwrap();
        eprintln!(
            "reviewed capture projected runs: {}",
            projection
                .runs
                .iter()
                .map(|run| format!(
                    "scene={:?} activity={:?} family={:?} difficulty={:?}/{:?}/tier={:?} state={:?} segments={:?}",
                    run.identity.scene_id,
                    run.identity.activity_id,
                    run.identity.activity_family_id,
                    run.identity.difficulty_family,
                    run.identity.difficulty_id,
                    run.identity.difficulty_tier,
                    run.terminal_state,
                    run.segments
                        .iter()
                        .map(|segment| {
                            (
                                segment.kind,
                                segment.attempt_count,
                                segment.closed_at_run_end,
                            )
                        })
                        .collect::<Vec<_>>()
                ))
                .collect::<Vec<_>>()
                .join("; ")
        );
        let mut history = meter.history_snapshot(&projection.runs).unwrap();
        match (
            std::env::var("RLOGS_HISTORY_EXPECTED_SCENE_ID").ok(),
            std::env::var("RLOGS_HISTORY_REVIEWED_DIFFICULTY_TIER").ok(),
        ) {
            (None, None) => {}
            (Some(expected_scene), Some(reviewed_tier)) => {
                let expected_scene = expected_scene
                    .parse::<i32>()
                    .expect("RLOGS_HISTORY_EXPECTED_SCENE_ID must be an i32");
                let reviewed_tier = reviewed_tier
                    .parse::<u32>()
                    .expect("RLOGS_HISTORY_REVIEWED_DIFFICULTY_TIER must be a u32");
                let matching_runs = history
                    .runs
                    .iter_mut()
                    .filter(|run| run.scene_id == Some(expected_scene))
                    .collect::<Vec<_>>();
                assert!(
                    !matching_runs.is_empty(),
                    "reviewed difficulty annotation refused: capture did not contain expected scene {expected_scene}"
                );
                for run in matching_runs {
                    if let Some(captured_tier) = run.difficulty_tier {
                        assert_eq!(
                            captured_tier, reviewed_tier,
                            "reviewed difficulty annotation refused to overwrite captured tier"
                        );
                    } else {
                        run.difficulty_tier = Some(reviewed_tier);
                    }
                }
            }
            _ => panic!(
                "reviewed history annotation requires both RLOGS_HISTORY_EXPECTED_SCENE_ID and RLOGS_HISTORY_REVIEWED_DIFFICULTY_TIER"
            ),
        }
        eprintln!(
            "reviewed history timing: {}",
            history
                .runs
                .iter()
                .map(|run| format!(
                    "scene={:?} total_us={:?} game_us={:?} true_us={:?} active_us={}",
                    run.scene_id,
                    run.total_run_time_micros,
                    run.game_time_micros,
                    run.true_time_micros,
                    run.views
                        .iter()
                        .find(|view| view.id == "all")
                        .map_or(0, |view| view.active_combat_micros),
                ))
                .collect::<Vec<_>>()
                .join("; ")
        );
        let combat_views = history
            .runs
            .iter()
            .flat_map(|run| &run.views)
            .filter(|view| view.actors.iter().any(|actor| actor.damage > 0))
            .collect::<Vec<_>>();

        assert!(
            !combat_views.is_empty(),
            "capture did not produce a combat history view"
        );
        assert!(
            combat_views
                .iter()
                .all(|view| view.active_combat_micros > 0),
            "a damage-bearing history view retained a zero active-combat denominator"
        );
        assert!(
            combat_views
                .iter()
                .flat_map(|view| &view.actors)
                .any(|actor| { actor.damage > 0 && actor.encounter_dps > 0.0 }),
            "damage-bearing actors did not receive nonzero encounter DPS"
        );

        if let Some(install_root) = std::env::var_os("RLOGS_HISTORY_IMPORT_ROOT") {
            let captured_unix_millis = std::fs::metadata(&path)
                .and_then(|metadata| metadata.modified())
                .ok()
                .and_then(|modified| modified.duration_since(UNIX_EPOCH).ok())
                .map_or_else(unix_millis, |duration| duration.as_millis() as u64);
            CombatHistoryStore::open(
                PathBuf::from(install_root).join("runtime-data/history/combat-meter"),
            )
            .unwrap()
            .record(&history, captured_unix_millis)
            .unwrap();
        }
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

    fn write_workspace_plugin(
        install_root: &Path,
        folder: &str,
        plugin_id: &str,
        dependency: Option<&str>,
    ) {
        let package = install_root.join("plugins/installed").join(folder);
        let web = package.join("web");
        std::fs::create_dir_all(&web).unwrap();
        std::fs::write(web.join("index.html"), "<p>package surface</p>").unwrap();
        let dependencies = dependency.map_or_else(String::new, |dependency| {
            format!(
                r#"
[[dependencies]]
plugin_id = "{dependency}"
optional = false
"#
            )
        });
        let manifest = format!(
            r#"schema_version = 1
id = "{plugin_id}"
name = "{folder}"
version = "0.1.0"
api_version = 1
runtime = "browser_overlay"
entrypoint = "web/index.html"
capabilities = ["ui_workspace_publish"]
subscriptions = []
allowed_network_domains = []
{dependencies}
[workspace]
default_order = 10

[[workspace.tabs]]
id = "main"
label = "Main"
entrypoint = "web/index.html"
kind = "content"
"#
        );
        std::fs::write(package.join("plugin.toml"), manifest).unwrap();
    }

    #[test]
    fn options_default_to_loopback_and_reject_unknown_flags() {
        let options = Options::parse(Vec::<String>::new()).unwrap();
        assert_eq!(options.bind, DEFAULT_BIND);
        assert!(Options::parse(["--wat".into()]).is_err());
    }

    #[test]
    fn event_viewer_keeps_exact_status_lifecycle_fields_pre_localization() {
        let region = RegionContext {
            identity: RegionIdentity {
                deployment_id: "global".into(),
                region_id: "global".into(),
                realm_id: None,
                world_id: Some("asteria".into()),
            },
            client_build: "24252055".into(),
            protocol_pack_digest: "sha256:fixture".into(),
            evidence: Vec::new(),
        };
        let event = EventEnvelope {
            schema_version: EVENT_SCHEMA_VERSION,
            session_id: "status-view".into(),
            sequence: 1,
            region,
            time: EventTime {
                observed_micros: 42,
                game_time_millis: None,
            },
            provenance: EventProvenance::wire(1, 7, 9),
            sensitivity: EventSensitivity::PublicGameplay,
            event: CanonicalEvent::Timeline(TimelineEvent {
                sequence: 1,
                time: EventTime {
                    observed_micros: 42,
                    game_time_millis: None,
                },
                provenance: EventProvenance::wire(1, 7, 9),
                kind: TimelineEventKind::Status(StatusEvent {
                    source: Some(EntityRef {
                        actor_id: ActorId(1),
                        entity_uuid: EntityUuid(100),
                    }),
                    target: EntityRef {
                        actor_id: ActorId(2),
                        entity_uuid: EntityUuid(200),
                    },
                    effect: StatusEffectId(2_203_031),
                    instance_id: Some(rlogs_events::StatusEffectInstanceId(44)),
                    origin: Some(rlogs_events::StatusOrigin {
                        source_type_id: 1,
                        source_config_id: 2_203_030,
                    }),
                    state: StatusState::Stacked,
                    stacks: Some(3),
                    duration_millis: Some(5_000),
                    level: None,
                    part_id: None,
                    count: None,
                    created_at_millis: None,
                }),
            }),
        };

        let view = EventViewerEventView::from_envelope(event).unwrap();
        assert_eq!(view.kind, "status");
        assert_eq!(view.identifiers.status.as_deref(), Some("2203031"));
        assert_eq!(view.identifiers.status_instance.as_deref(), Some("44"));
        assert_eq!(view.identifiers.status_origin_type.as_deref(), Some("1"));
        assert_eq!(
            view.identifiers.status_origin_config.as_deref(),
            Some("2203030")
        );
        assert_eq!(view.identifiers.status_state.as_deref(), Some("stacked"));
        assert_eq!(view.identifiers.status_stacks.as_deref(), Some("3"));
        assert_eq!(
            view.identifiers.status_duration_millis.as_deref(),
            Some("5000")
        );
        assert!(view.summary.contains("state:stacked"));
        assert!(view.summary.contains("instance:44"));
        assert!(view.summary.contains("origin:1:2203030"));
        assert!(view.summary.contains("stacks:3"));
        assert!(view.summary.contains("duration_ms:5000"));
    }

    #[test]
    fn sealed_canonical_events_are_filtered_and_streamed_in_bounded_pages() {
        let install_root = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../..")
            .canonicalize()
            .unwrap();
        let controller = RuntimeController::new(install_root).unwrap();
        let queued_before = controller.submission_queue().entry_count;
        let import_error = controller
            .import_submission_artifact(SubmissionImportRequest {
                artifact_path: controller
                    .install_root
                    .join("tests/fixtures/replay/reference-combat.rlog")
                    .display()
                    .to_string(),
            })
            .unwrap_err();
        assert!(
            import_error.contains("protocol pack digest is invalid"),
            "unexpected import error: {import_error}",
        );
        let reference = controller.run_reference_replay().unwrap();
        assert_eq!(
            reference.submission_queue_status,
            "not_queued_reference_fixture"
        );
        assert_eq!(reference.combat_snapshot.session_id, reference.session_id);
        assert_eq!(
            reference.combat_snapshot.schema_version,
            COMBAT_SNAPSHOT_SCHEMA_VERSION
        );
        assert_eq!(reference.combat_snapshot.active_combat_micros, 10_000_000);
        assert!(
            reference
                .combat_snapshot
                .actors
                .iter()
                .any(|actor| actor.actor_id == "1" && actor.dps == 2_000.0)
        );
        assert_eq!(controller.submission_queue().entry_count, queued_before);

        let first = controller
            .event_viewer_page(EventViewerPageRequest {
                query_id: None,
                limit: Some(1),
                filter: Some(EventViewerFilter::default()),
            })
            .unwrap();
        assert_eq!(first.events.len(), 1);
        assert_eq!(first.events[0].sequence, 1);
        assert!(first.integrity_verified);
        assert!(!first.complete);

        let second = controller
            .event_viewer_page(EventViewerPageRequest {
                query_id: Some(first.query_id),
                limit: Some(1),
                filter: None,
            })
            .unwrap();
        assert_eq!(second.events.len(), 1);
        assert_eq!(second.events[0].sequence, 2);
        assert_eq!(second.page_index, 2);

        let damage = controller
            .event_viewer_page(EventViewerPageRequest {
                query_id: None,
                limit: Some(MAX_EVENT_VIEWER_PAGE_SIZE),
                filter: Some(EventViewerFilter {
                    kind: Some("damage".into()),
                    ..EventViewerFilter::default()
                }),
            })
            .unwrap();
        assert!(damage.complete);
        assert!(!damage.events.is_empty());
        assert!(damage.events.iter().all(|event| event.kind == "damage"));
        assert!(
            damage
                .events
                .iter()
                .all(|event| event.identifiers.source.is_some()
                    && event.identifiers.target.is_some())
        );

        let report = controller.run_report().unwrap();
        assert!(report.integrity_verified);
        assert_eq!(report.projection.session_id, reference.session_id);
        assert_eq!(
            report.artifact_digest,
            reference.combat_plugin.rlog.content_sha256
        );
        assert_eq!(
            report.replay_metrics.events_seen,
            reference.canonical_event_count
        );
        assert!(report.projection.runs.is_empty());
    }

    #[test]
    fn event_viewer_keeps_64_bit_entity_ids_out_of_javascript_numbers() {
        let entity = EventViewerEntityView::from(EntityRef {
            actor_id: ActorId(u64::MAX),
            entity_uuid: EntityUuid(i64::MIN),
        });
        let json = serde_json::to_value(entity).unwrap();

        assert_eq!(json["actorId"], u64::MAX.to_string());
        assert_eq!(json["entityUuid"], i64::MIN.to_string());
        assert!(json["actorId"].is_string());
        assert!(json["entityUuid"].is_string());
    }

    #[test]
    fn installed_plugin_enablement_is_persisted_and_publishes_its_workspace() {
        let root = temporary_root();
        write_workspace_plugin(&root, "timeline-tools", "dev.rlogs.timeline-tools", None);

        let controller = RuntimeController::new(root.clone()).unwrap();
        let catalog = controller.plugin_catalog();
        let package = catalog
            .packages
            .iter()
            .find(|package| package.id == "dev.rlogs.timeline-tools")
            .unwrap();
        assert!(!package.enabled);
        assert!(!package.active);
        assert_eq!(catalog.workspaces.len(), 2);
        assert!(
            catalog
                .workspaces
                .iter()
                .all(|workspace| workspace.id == "app.rlogs.session-recorder"
                    || workspace.id == "app.rlogs.combat-meter")
        );

        let catalog = controller
            .set_plugin_enabled(PluginEnablementRequest {
                plugin_id: "dev.rlogs.timeline-tools".into(),
                enabled: true,
            })
            .unwrap();
        let package = catalog
            .packages
            .iter()
            .find(|package| package.id == "dev.rlogs.timeline-tools")
            .unwrap();
        assert!(package.enabled);
        assert!(package.active);
        assert_eq!(catalog.workspaces.len(), 3);
        let workspace = catalog
            .workspaces
            .iter()
            .find(|workspace| workspace.id == "dev.rlogs.timeline-tools")
            .unwrap();
        assert_eq!(
            workspace.tabs[0].entrypoint,
            "installed://dev.rlogs.timeline-tools/main"
        );
        drop(controller);

        let restarted = RuntimeController::new(root.clone()).unwrap();
        let catalog = restarted.plugin_catalog();
        let package = catalog
            .packages
            .iter()
            .find(|package| package.id == "dev.rlogs.timeline-tools")
            .unwrap();
        assert!(package.enabled);
        assert!(package.active);
        assert_eq!(catalog.workspaces.len(), 3);

        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn disabling_a_dependency_blocks_but_does_not_forget_its_dependent() {
        let root = temporary_root();
        write_workspace_plugin(&root, "base", "dev.rlogs.base", None);
        write_workspace_plugin(
            &root,
            "extension",
            "dev.rlogs.extension",
            Some("dev.rlogs.base"),
        );
        let controller = RuntimeController::new(root.clone()).unwrap();
        controller
            .set_plugin_enabled(PluginEnablementRequest {
                plugin_id: "dev.rlogs.base".into(),
                enabled: true,
            })
            .unwrap();
        controller
            .set_plugin_enabled(PluginEnablementRequest {
                plugin_id: "dev.rlogs.extension".into(),
                enabled: true,
            })
            .unwrap();

        let catalog = controller
            .set_plugin_enabled(PluginEnablementRequest {
                plugin_id: "dev.rlogs.base".into(),
                enabled: false,
            })
            .unwrap();
        let extension = catalog
            .packages
            .iter()
            .find(|plugin| plugin.id == "dev.rlogs.extension")
            .unwrap();
        assert!(extension.enabled);
        assert!(!extension.active);
        assert!(extension.status_detail.contains("dev.rlogs.base"));
        assert!(
            catalog
                .issues
                .iter()
                .any(|issue| issue.plugin_id.as_deref() == Some("dev.rlogs.extension"))
        );

        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn full_offline_capture_reaches_sealed_log_and_builtin_projections() {
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
        let mut result = process_offline_session(
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
        assert_eq!(result.combat_snapshot.session_id, result.session_id);
        assert_eq!(
            result.combat_snapshot.schema_version,
            COMBAT_SNAPSHOT_SCHEMA_VERSION
        );
        assert_eq!(result.encounter_recorder.rlog.event_count, 2);
        assert_eq!(result.encounter_recorder.metrics.events_delivered, 2);
        let upload_artifact = result.upload_artifact.clone().unwrap();
        assert_eq!(
            upload_artifact.canonical_content_sha256,
            result.combat_plugin.rlog.content_sha256
        );
        assert!(upload_artifact.file_byte_length > 0);
        assert_eq!(upload_artifact.file_sha256.len(), 64);
        assert_eq!(upload_artifact.chunk_count, 1);
        assert!(Path::new(&result.output_rlog).is_file());
        assert!(Path::new(result.coverage_report.as_ref().unwrap()).is_file());
        let queue_path = root.join("runtime-data/submissions/queue");
        let queue = Arc::new(Mutex::new(
            LocalSubmissionQueue::open(queue_path.clone()).unwrap(),
        ));
        assert_eq!(
            queue_completed_session(&queue, &mut result, ReportVisibility::Unlisted).unwrap(),
            QueueInsertOutcome::Queued
        );
        assert_eq!(result.submission_queue_status, "queued");
        let sanitized_queue_id = result.submission_queue_id.clone().unwrap();
        assert_ne!(sanitized_queue_id, upload_artifact.file_sha256.as_str());
        let restored = LocalSubmissionQueue::open(queue_path).unwrap();
        let queue_snapshot = restored.snapshot();
        assert_eq!(queue_snapshot.entry_count, 1);
        assert_eq!(
            queue_snapshot.entries[0].capture_session_id,
            "fixture-session"
        );
        assert!(queue_snapshot.entries[0].artifact_byte_length_matches);
        assert_eq!(
            queue_snapshot.entries[0].canonical_content_sha256,
            build_privacy_verified_submission_artifact(
                File::open(&queue_snapshot.entries[0].local_artifact_path).unwrap(),
                ArtifactBuildLimits::default(),
                RlogLimits::default(),
            )
            .unwrap()
            .rlog
            .content_sha256
            .strip_prefix("sha256:")
            .unwrap()
        );
        assert!(
            Path::new(&queue_snapshot.entries[0].local_artifact_path)
                .starts_with(root.join("runtime-data/submissions/artifacts"))
        );
        let queued_entry = restored.entry(&sanitized_queue_id).unwrap();
        assert_eq!(
            queued_entry.session.metadata().privacy_policy_digest,
            submission_privacy_policy_digest()
        );
        assert_eq!(
            queued_entry.canonical_content_sha256.as_str(),
            build_privacy_verified_submission_artifact(
                File::open(&queued_entry.local_artifact_path).unwrap(),
                ArtifactBuildLimits::default(),
                RlogLimits::default(),
            )
            .unwrap()
            .rlog
            .content_sha256
            .strip_prefix("sha256:")
            .unwrap()
        );
        drop(restored);

        let controller = RuntimeController::new(root.clone()).unwrap();
        let imported = controller
            .import_submission_artifact(SubmissionImportRequest {
                artifact_path: result.output_rlog.clone(),
            })
            .unwrap();
        assert_eq!(imported.outcome, "already_queued");
        assert_eq!(imported.queue_id, sanitized_queue_id);
        let verified = controller
            .verify_queued_submission(SubmissionVerificationRequest {
                queue_id: imported.queue_id.clone(),
            })
            .unwrap();
        assert_eq!(verified.capture_session_id, "fixture-session");
        assert_eq!(verified.artifact.file_sha256, imported.queue_id);
        assert_ne!(
            verified.artifact.canonical_content_sha256,
            upload_artifact.canonical_content_sha256
        );
        assert!(
            controller
                .run_mock_submission(MockSubmissionRequest {
                    queue_id: imported.queue_id.clone(),
                })
                .unwrap_err()
                .contains("disabled")
        );
        let mut policy = SubmissionPolicy::default();
        policy.log_uploader.enabled = true;
        controller.update_submission_policy(policy).unwrap();
        let dry_run = controller
            .run_mock_submission(MockSubmissionRequest {
                queue_id: imported.queue_id.clone(),
            })
            .unwrap();
        assert_eq!(dry_run.final_state, SubmissionState::Submitted);
        assert_eq!(dry_run.chunk_count, verified.artifact.chunk_count);
        assert_eq!(dry_run.uploaded_bytes, verified.artifact.file_byte_length);
        assert!(dry_run.resumed_after_restart);
        assert_eq!(dry_run.external_network_requests, 0);
        assert!(dry_run.report_id.starts_with("mock-report-"));

        let source_reader = RlogReader::new(
            BufReader::new(File::open(&result.output_rlog).unwrap()),
            RlogLimits::default(),
        )
        .unwrap();
        let mut profile_header = source_reader.header().clone();
        profile_header.session_id = "profile-package-session".into();
        let character = CharacterIdentity {
            region: profile_header.region.identity.clone(),
            character_id: "123456789".into(),
        };
        let profile = CharacterProfilePatch {
            character: character.clone(),
            display_name: Some("MarieRose".into()),
            display_id: Some("123456789".into()),
            server_id: Some("7".into()),
            class_id: Some(5),
            specialization_id: Some(2),
            level: Some(60),
            progression: None,
            combat_power: Some(45_000),
            combat_power_breakdown: None,
            season_strength: Some(300),
            season: None,
            appearance: None,
            equipment: None,
            equipment_suit_entries: None,
            modules: None,
            owned_imagines: None,
            battle_imagine_skills: None,
            equipped_action_slots: None,
            active_skills: None,
            talents: None,
            talent_progress: None,
            combat_professions: None,
            life_professions: None,
            cosmetics: None,
            collection_summary: None,
            activity_progress: None,
            season_medals: None,
            season_cultivation: None,
            reputations: None,
            current_profession_project_id: None,
            social_display: None,
        };
        let profile_event = EventEnvelope {
            schema_version: EVENT_SCHEMA_VERSION,
            session_id: profile_header.session_id.clone(),
            sequence: 1,
            region: profile_header.region.clone(),
            time: EventTime {
                observed_micros: 1,
                game_time_millis: None,
            },
            provenance: EventProvenance::wire(1, 1, 1),
            sensitivity: EventSensitivity::PersonalGameplay,
            event: CanonicalEvent::CharacterProfileObserved {
                profile: Box::new(profile.into_game_event().unwrap()),
            },
        };
        let mut profile_writer = RlogWriter::new(Vec::new(), profile_header).unwrap();
        profile_writer.push(&profile_event).unwrap();
        let profile_path = root.join("profile-package-source.rlog");
        std::fs::write(&profile_path, profile_writer.finish().unwrap()).unwrap();
        let mut profile_result = result.clone();
        profile_result.session_id = "profile-package-session".into();
        profile_result.output_rlog = profile_path.display().to_string();
        controller.state.lock().unwrap().last_result = Some(profile_result);
        assert!(
            controller
                .project_last_profile_packages()
                .unwrap_err()
                .contains("disabled")
        );
        let mut policy = SubmissionPolicy::default();
        policy.log_uploader.enabled = true;
        policy.bpsr_profile_sync.enabled = true;
        controller.update_submission_policy(policy).unwrap();
        let projection = controller.project_last_profile_packages().unwrap();
        assert_eq!(projection.projected_package_count, 1);
        assert_eq!(projection.external_network_requests, 0);
        assert_eq!(projection.stored_packages[0].character_id, "123456789");
        assert_eq!(
            projection.stored_packages[0].display_name.as_deref(),
            Some("MarieRose")
        );
        assert!(
            projection.stored_packages[0]
                .local_package_path
                .contains("123456789")
        );
        let inspection = controller
            .inspect_profile_package(ProfilePackageInspectionRequest {
                package_id: projection.stored_packages[0].package_id.clone(),
            })
            .unwrap();
        let inspection_json = serde_json::to_string(&inspection).unwrap();
        assert!(!inspection_json.contains("password"));
        assert!(!inspection_json.contains("account"));
        assert!(!inspection_json.contains("token"));

        OpenOptions::new()
            .append(true)
            .open(&result.output_rlog)
            .unwrap()
            .write_all(b"\n")
            .unwrap();
        controller
            .verify_queued_submission(SubmissionVerificationRequest {
                queue_id: sanitized_queue_id.clone(),
            })
            .unwrap();
        OpenOptions::new()
            .append(true)
            .open(&queue_snapshot.entries[0].local_artifact_path)
            .unwrap()
            .write_all(b"\n")
            .unwrap();
        assert!(
            controller
                .verify_queued_submission(SubmissionVerificationRequest {
                    queue_id: sanitized_queue_id,
                })
                .unwrap_err()
                .contains("verification")
        );

        std::fs::remove_dir_all(root).unwrap();
    }
}
