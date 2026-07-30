use std::collections::{BTreeMap, BTreeSet};
mod profile_packages;
mod submission_policy;
mod submission_queue;

use std::fs::{File, OpenOptions};
use std::io::{BufReader, BufWriter, Read, Seek, SeekFrom, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::path::{Component, Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use profile_packages::{
    LocalProfilePackageStore, ProfilePackageInspection, ProfilePackageStoreView, ProfilePackageView,
};
use rlogs_capture::OfflineCapture;
#[cfg(windows)]
use rlogs_capture::{
    DumpcapLiveConfig, LiveCaptureStopHandle, OwnedProcessCaptureConfig,
    WindowsOwnedDumpcapCapture, record_owned_capture_to_files,
};
use rlogs_core::ResearchConnectionFile;
use rlogs_events::{
    CanonicalEvent, EntityRef, EventEnvelope, EventTopic, RegionEvidence, RegionEvidenceKind,
    RegionIdentity, TimelineEventKind,
};
use rlogs_game_bpsr::{
    BPSR_GAME_PLUGIN_ID, GameBuild, NetworkEndpoint, OfflineRecordingConfig,
    OfflineRecordingLimits, OfflineRecordingReport, ProtocolPack, ProtocolRuntimeConfig,
    RegionResolverError, ResolvedRegion, ServerRealmCatalog, project_local_profile_packages,
    record_offline_capture,
};
use rlogs_log_format::{RlogHeader, RlogLimits, RlogReader};
use rlogs_plugin_api::{PluginCapability, PluginDependency, PluginRuntime, PluginWorkspaceTabKind};
use rlogs_plugin_combat_meter::CombatTimelinePlugin;
use rlogs_plugin_encounter_recorder::EncounterRecorderPlugin;
use rlogs_plugin_host::{
    PluginDiscoveryReport, PluginOrderError, PluginPackage, PluginWorkspaceError,
    ResolvedPluginWorkspace, discover_installed_plugins, resolve_plugin_load_order,
    resolve_plugin_workspaces,
};
use rlogs_plugin_runtime::{PluginRunLimits, PluginRunReport, replay_rlog};
use rlogs_submission::{
    ArtifactBuildLimits, LocalLogArtifact, MAXIMUM_LOCAL_ARTIFACT_PATH_BYTES,
    MAXIMUM_UPLOAD_CHUNK_BYTES, MockSubmissionReceiver, QueuedSubmission, ReportVisibility,
    Sha256Digest, SubmissionMetadata, SubmissionState, build_sealed_log_artifact,
};
use serde::{Deserialize, Serialize};
use submission_policy::{SubmissionPolicy, SubmissionPolicyStore, SubmissionPolicyView};
use submission_queue::{LocalSubmissionQueue, QueueInsertOutcome, SubmissionQueueView};
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
const MAX_CONCURRENT_LOCAL_REQUESTS: usize = 16;
const PLUGIN_CATALOG_SCHEMA_VERSION: u16 = 1;
const PLUGIN_ENABLEMENT_SCHEMA_VERSION: u16 = 1;
const MAX_PLUGIN_ENABLEMENT_BYTES: u64 = 256 * 1024;
const EVENT_VIEWER_SCHEMA_VERSION: u16 = 1;
const DEFAULT_EVENT_VIEWER_PAGE_SIZE: usize = 100;
const MAX_EVENT_VIEWER_PAGE_SIZE: usize = 200;
const MAX_EVENT_VIEWER_SCAN_PER_PAGE: u64 = 50_000;
const MAX_EVENT_VIEWER_PAGE_BYTES: usize = 8 * 1024 * 1024;
const MAX_EVENT_VIEWER_FILTER_BYTES: usize = 128;
const MAX_EVENT_VIEWER_SCAN_TIME: Duration = Duration::from_millis(100);

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
    let controller = Arc::new(RuntimeController::new(install_root)?);
    let ui_root = Arc::new(ui_root);
    let active_requests = Arc::new(AtomicUsize::new(0));

    println!("rLogs local controls: http://{bind}");
    println!("Press Ctrl+C to stop the local host.");
    for stream in listener.incoming() {
        match stream {
            Ok(stream) => {
                if active_requests.fetch_add(1, Ordering::AcqRel) >= MAX_CONCURRENT_LOCAL_REQUESTS {
                    active_requests.fetch_sub(1, Ordering::AcqRel);
                    eprintln!(
                        "local HTTP request rejected: {MAX_CONCURRENT_LOCAL_REQUESTS} handlers are active"
                    );
                    continue;
                }
                let ui_root = Arc::clone(&ui_root);
                let controller = Arc::clone(&controller);
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
            Err(error) => eprintln!("local HTTP accept failed: {error}"),
        }
    }
    Ok(())
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
    encounter_recorder: PluginRunReport,
    upload_artifact: UploadArtifactView,
    submission_queue_id: Option<String>,
    submission_queue_status: String,
    profile_package_count: usize,
    profile_sync_status: String,
    #[serde(skip_serializing)]
    verified_artifact: Option<LocalLogArtifact>,
}

#[derive(Debug, Clone, Serialize)]
struct UploadArtifactView {
    file_byte_length: u64,
    file_sha256: String,
    chunk_count: usize,
    canonical_content_sha256: String,
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

impl EventViewerEventView {
    fn from_envelope(canonical: EventEnvelope) -> Result<Self, String> {
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
        let summary = event_viewer_summary(kind, &identifiers, amount.as_deref());
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
            "status"
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
            integrity_verified: true,
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
    let summary = reader
        .replay(|_| Ok(()))
        .map_err(|error| format!("sealed rlog verification failed: {error}"))?;
    if summary.content_sha256 != result.combat_plugin.rlog.content_sha256
        || summary.event_count != result.canonical_event_count
    {
        return Err("sealed rlog does not match the completed pipeline result".into());
    }
    if event_viewer_file_identity(identity.path.clone())? != identity {
        return Err("sealed rlog changed while Event Viewer verified it".into());
    }
    Ok(VerifiedEventViewerArtifact {
        identity,
        session_id: result.session_id.clone(),
        digest: summary.content_sha256,
        event_count: summary.event_count,
        header,
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
    private_output_directory: Option<String>,
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
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct PluginCatalogView {
    schema_version: u16,
    installed_root: String,
    packages: Vec<InstalledPluginView>,
    issues: Vec<PluginIssueView>,
    workspaces: Vec<PluginWorkspaceView>,
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
}

#[derive(Debug)]
struct DesktopPluginManager {
    installed_root: PathBuf,
    state_path: PathBuf,
    report: PluginDiscoveryReport,
    enabled_plugin_ids: BTreeSet<String>,
    state_issue: Option<String>,
}

#[derive(Debug)]
struct ActivePluginResolution {
    active_plugin_ids: BTreeSet<String>,
    blocked: BTreeMap<String, String>,
    workspaces: Vec<ResolvedPluginWorkspace>,
}

impl DesktopPluginManager {
    fn new(install_root: &Path) -> Result<Self, String> {
        let installed_root = install_root.join("plugins/installed");
        std::fs::create_dir_all(&installed_root).map_err(|error| {
            format!(
                "could not create installed plug-ins folder {}: {error}",
                display_path(&installed_root)
            )
        })?;
        let state_path = install_root.join("runtime-data/settings/plugin-enablement.v1.json");
        let (enabled_plugin_ids, state_issue) = load_plugin_enablement(&state_path);
        let report = discover_installed_plugins(&installed_root)
            .map_err(|error| format!("plug-in discovery failed: {error}"))?;
        Ok(Self {
            installed_root,
            state_path,
            report,
            enabled_plugin_ids,
            state_issue,
        })
    }

    fn refresh(&mut self) -> Result<(), String> {
        self.report = discover_installed_plugins(&self.installed_root)
            .map_err(|error| format!("plug-in discovery failed: {error}"))?;
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
        if enabled {
            candidate.insert(plugin_id.to_owned());
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
        }

        save_plugin_enablement(&self.state_path, &candidate)?;
        self.enabled_plugin_ids = candidate;
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
                        || !manifest.workspace_tab_contributions.is_empty(),
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
                Some(PluginWorkspaceView {
                    id: workspace.owner_plugin_id,
                    name: workspace.name,
                    description: format!(
                        "Installed {} folder package. Executable surfaces remain isolated until their runtime adapter is available.",
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
                            id: tab.id,
                            label: tab.label,
                            kind: tab.kind,
                            entrypoint: format!(
                                "installed://{}/{}",
                                tab.contributor_plugin_id, tab.local_id
                            ),
                            contributor_plugin_id: tab.contributor_plugin_id,
                        })
                        .collect(),
                })
            })
            .collect();

        PluginCatalogView {
            schema_version: PLUGIN_CATALOG_SCHEMA_VERSION,
            installed_root: display_path(&self.installed_root),
            packages,
            issues,
            workspaces,
        }
    }
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
                return ActivePluginResolution {
                    active_plugin_ids: active.keys().cloned().collect(),
                    blocked,
                    workspaces,
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

fn load_plugin_enablement(path: &Path) -> (BTreeSet<String>, Option<String>) {
    let metadata = match std::fs::metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return (BTreeSet::new(), None);
        }
        Err(error) => {
            return (
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
        Ok(state) if state.schema_version == PLUGIN_ENABLEMENT_SCHEMA_VERSION => {
            (state.enabled_plugin_ids, None)
        }
        Ok(state) => (
            BTreeSet::new(),
            Some(format!(
                "Unsupported plug-in enablement schema {}; expected {}.",
                state.schema_version, PLUGIN_ENABLEMENT_SCHEMA_VERSION
            )),
        ),
        Err(error) => (
            BTreeSet::new(),
            Some(format!("Plug-in enablement settings are invalid: {error}")),
        ),
    }
}

fn save_plugin_enablement(
    path: &Path,
    enabled_plugin_ids: &BTreeSet<String>,
) -> Result<(), String> {
    let parent = path
        .parent()
        .ok_or_else(|| "plug-in enablement path has no parent".to_owned())?;
    std::fs::create_dir_all(parent)
        .map_err(|error| format!("could not create plug-in settings folder: {error}"))?;
    let state = StoredPluginEnablement {
        schema_version: PLUGIN_ENABLEMENT_SCHEMA_VERSION,
        enabled_plugin_ids: enabled_plugin_ids.clone(),
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
}

struct RuntimeController {
    install_root: PathBuf,
    state: Arc<Mutex<RuntimeSnapshot>>,
    plugins: Mutex<DesktopPluginManager>,
    event_viewer: Mutex<EventViewerState>,
    submission_queue: Arc<Mutex<LocalSubmissionQueue>>,
    profile_packages: Arc<Mutex<LocalProfilePackageStore>>,
    submission_policy: Mutex<SubmissionPolicyStore>,
    artifact_verification: Mutex<()>,
    profile_projection: Mutex<()>,
    #[cfg(windows)]
    live_stop: Arc<Mutex<Option<LiveCaptureStopHandle>>>,
}

impl RuntimeController {
    fn new(install_root: PathBuf) -> Result<Self, String> {
        let plugins = DesktopPluginManager::new(&install_root)?;
        let submission_queue =
            LocalSubmissionQueue::open(install_root.join("runtime-data/submissions/queue"))?;
        let profile_packages = LocalProfilePackageStore::open(
            install_root.join("runtime-data/profile-sync/packages"),
        )?;
        let submission_policy = SubmissionPolicyStore::open(
            install_root.join("runtime-data/settings/submission-policy.v1.json"),
        )?;
        Ok(Self {
            install_root,
            state: Arc::new(Mutex::new(RuntimeSnapshot::default())),
            plugins: Mutex::new(plugins),
            event_viewer: Mutex::new(EventViewerState::default()),
            submission_queue: Arc::new(Mutex::new(submission_queue)),
            profile_packages: Arc::new(Mutex::new(profile_packages)),
            submission_policy: Mutex::new(submission_policy),
            artifact_verification: Mutex::new(()),
            profile_projection: Mutex::new(()),
            #[cfg(windows)]
            live_stop: Arc::new(Mutex::new(None)),
        })
    }

    fn snapshot(&self) -> RuntimeSnapshot {
        self.state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone()
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
        self.submission_policy
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .snapshot()
    }

    fn update_submission_policy(
        &self,
        policy: SubmissionPolicy,
    ) -> Result<SubmissionPolicyView, String> {
        self.submission_policy
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .update(policy)
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

        loop {
            let Some(chunk) = session
                .pending_chunks(1)
                .map_err(|error| format!("mock upload could not read pending chunks: {error}"))?
                .first()
                .cloned()
                .cloned()
            else {
                break;
            };
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

    fn plugin_catalog(&self) -> PluginCatalogView {
        self.plugins
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .snapshot()
    }

    fn refresh_plugins(&self) -> Result<PluginCatalogView, String> {
        let mut plugins = self
            .plugins
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        plugins.refresh()?;
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
        Ok(plugins.snapshot())
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
        let combat_plugin = replay_combat_log(&input).map_err(|error| error.to_string())?;
        let encounter_recorder = replay_encounter_log(&input).map_err(|error| error.to_string())?;
        let upload_artifact = build_upload_artifact(&input)?;
        verify_replay_artifact(&combat_plugin, &encounter_recorder, &upload_artifact)?;
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
            encounter_recorder,
            upload_artifact: UploadArtifactView::from(&upload_artifact),
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
        let default_visibility = policy.log_uploader.default_visibility;
        let profile_sync = policy.bpsr_profile_sync;
        thread::Builder::new()
            .name(format!("rlogs-offline-{}", request.session_id))
            .spawn(move || {
                let result = process_offline_session(&install_root, &request).map(|mut result| {
                    let queue_warning =
                        queue_completed_session(&submission_queue, &mut result, default_visibility)
                            .err();
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
        let submission_queue = Arc::clone(&self.submission_queue);
        let profile_packages = Arc::clone(&self.profile_packages);
        let policy = self
            .submission_policy
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .policy()
            .clone();
        let default_visibility = policy.log_uploader.default_visibility;
        let profile_sync = policy.bpsr_profile_sync;
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
                    let queue_warning =
                        queue_completed_session(&submission_queue, &mut result, default_visibility)
                            .err();
                    let profile_warning = apply_profile_sync_policy(
                        &profile_packages,
                        &mut result,
                        profile_sync.enabled,
                        profile_sync.automatic_profiles,
                    );
                    Ok((result, queue_warning, profile_warning))
                });

                let mut state = state
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner);
                state.active_session_id = None;
                state.completed_unix_millis = Some(unix_millis());
                state.live_capture_can_stop = false;
                match result {
                    Ok((result, queue_warning, profile_warning)) => {
                        state.phase = RuntimePhase::Complete;
                        state.detail = completed_session_detail(
                            "Captured and sealed",
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
    let encounter_recorder = replay_encounter_log(&output_path)
        .map_err(|error| format!("encounter recorder failed: {error}"))?;
    let upload_artifact = build_upload_artifact(&output_path)?;
    verify_replay_artifact(&combat_plugin, &encounter_recorder, &upload_artifact)?;
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
        encounter_recorder,
        upload_artifact: UploadArtifactView::from(&upload_artifact),
        submission_queue_id: None,
        submission_queue_status: "pending_local_queue".into(),
        profile_package_count: 0,
        profile_sync_status: "pending_policy".into(),
        verified_artifact: Some(upload_artifact),
    })
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
    artifact: &LocalLogArtifact,
    local_artifact_path: String,
    created_unix_millis: u64,
    visibility: ReportVisibility,
) -> Result<(QueueInsertOutcome, String), String> {
    let protocol_pack_digest = parse_prefixed_sha256(&artifact.header.region.protocol_pack_digest)?;
    let metadata = SubmissionMetadata::new(
        BPSR_GAME_PLUGIN_ID,
        artifact.file_sha256.to_string(),
        artifact.header.schema_version,
        artifact.header.session_id.clone(),
        artifact.header.region.identity.region_id.clone(),
        artifact.header.region.client_build.clone(),
        protocol_pack_digest.clone(),
        protocol_pack_digest,
        visibility,
    );
    let entry = QueuedSubmission::new_post_run(
        metadata,
        artifact,
        local_artifact_path,
        created_unix_millis,
    )
    .map_err(|error| format!("could not create submission queue entry: {error}"))?;
    entry
        .verify_artifact(artifact)
        .map_err(|error| format!("new submission draft did not match its artifact: {error}"))?;
    let queue_id = entry.queue_id.to_string();
    let outcome = queue
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .enqueue(entry)?;
    Ok((outcome, queue_id))
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

fn replay_encounter_log(path: &Path) -> Result<PluginRunReport, String> {
    let file =
        File::open(path).map_err(|error| format!("could not open {}: {error}", path.display()))?;
    replay_rlog(
        BufReader::new(file),
        EncounterRecorderPlugin::default(),
        RlogLimits::default(),
        PluginRunLimits::default(),
    )
    .map_err(|error| error.to_string())
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
        ("GET", "/api/submissions/queue") => {
            write_json(&mut stream, 200, &controller.submission_queue())?;
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
    use std::sync::atomic::{AtomicU64, Ordering};

    use bytes::Bytes;
    use etherparse::{PacketBuilder, TcpHeader};
    use rlogs_capture::{CaptureLinkType, CapturedFrame, PcapWriter, TimestampNormalization};
    use rlogs_core::GameConnection;
    use rlogs_events::{
        ActorId, CanonicalEvent, CharacterIdentity, EVENT_SCHEMA_VERSION, EntityUuid,
        EventEnvelope, EventProvenance, EventSensitivity, EventTime,
    };
    use rlogs_game_bpsr::{CharacterProfilePatch, FragmentKind};
    use rlogs_log_format::RlogWriter;
    use rlogs_network::IpEndpoint;

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
    fn sealed_canonical_events_are_filtered_and_streamed_in_bounded_pages() {
        let install_root = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../..")
            .canonicalize()
            .unwrap();
        let controller = RuntimeController::new(install_root).unwrap();
        let queued_before = controller.submission_queue().entry_count;
        assert!(
            controller
                .import_submission_artifact(SubmissionImportRequest {
                    artifact_path: controller
                        .install_root
                        .join("tests/fixtures/replay/reference-combat.rlog")
                        .display()
                        .to_string(),
                })
                .unwrap_err()
                .contains("protocol pack digest is invalid")
        );
        let reference = controller.run_reference_replay().unwrap();
        assert_eq!(
            reference.submission_queue_status,
            "not_queued_reference_fixture"
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
        assert_eq!(catalog.packages.len(), 1);
        assert!(!catalog.packages[0].enabled);
        assert!(!catalog.packages[0].active);
        assert!(catalog.workspaces.is_empty());

        let catalog = controller
            .set_plugin_enabled(PluginEnablementRequest {
                plugin_id: "dev.rlogs.timeline-tools".into(),
                enabled: true,
            })
            .unwrap();
        assert!(catalog.packages[0].enabled);
        assert!(catalog.packages[0].active);
        assert_eq!(catalog.workspaces.len(), 1);
        assert_eq!(catalog.workspaces[0].id, "dev.rlogs.timeline-tools");
        assert_eq!(
            catalog.workspaces[0].tabs[0].entrypoint,
            "installed://dev.rlogs.timeline-tools/main"
        );
        drop(controller);

        let restarted = RuntimeController::new(root.clone()).unwrap();
        let catalog = restarted.plugin_catalog();
        assert!(catalog.packages[0].enabled);
        assert!(catalog.packages[0].active);
        assert_eq!(catalog.workspaces.len(), 1);

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
        assert_eq!(result.encounter_recorder.rlog.event_count, 2);
        assert_eq!(result.encounter_recorder.metrics.events_delivered, 1);
        assert_eq!(
            result.upload_artifact.canonical_content_sha256,
            result.combat_plugin.rlog.content_sha256
        );
        assert!(result.upload_artifact.file_byte_length > 0);
        assert_eq!(result.upload_artifact.file_sha256.len(), 64);
        assert_eq!(result.upload_artifact.chunk_count, 1);
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
        assert_eq!(
            result.submission_queue_id.as_deref(),
            Some(result.upload_artifact.file_sha256.as_str())
        );
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
            result
                .upload_artifact
                .canonical_content_sha256
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
        assert_eq!(imported.queue_id, result.upload_artifact.file_sha256);
        let verified = controller
            .verify_queued_submission(SubmissionVerificationRequest {
                queue_id: imported.queue_id.clone(),
            })
            .unwrap();
        assert_eq!(verified.capture_session_id, "fixture-session");
        assert_eq!(
            verified.artifact.canonical_content_sha256,
            result.upload_artifact.canonical_content_sha256
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
                queue_id: imported.queue_id,
            })
            .unwrap();
        assert_eq!(dry_run.final_state, SubmissionState::Submitted);
        assert_eq!(dry_run.chunk_count, result.upload_artifact.chunk_count);
        assert_eq!(
            dry_run.uploaded_bytes,
            result.upload_artifact.file_byte_length
        );
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
            modules: None,
            owned_imagines: None,
            battle_imagine_skills: None,
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
        assert!(
            controller
                .verify_queued_submission(SubmissionVerificationRequest {
                    queue_id: result.upload_artifact.file_sha256.clone(),
                })
                .unwrap_err()
                .contains("verification")
        );

        std::fs::remove_dir_all(root).unwrap();
    }
}
