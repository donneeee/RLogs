import { createDevelopmentAdapter } from "./development-adapter";
import {
  EVENT_VIEWER_TOPICS,
  type EventViewerFilter,
  type EventViewerPage,
  type EventViewerTopic,
  parseEventViewerPage,
} from "./event-viewer";
import {
  type LocalPluginCatalog,
  parsePluginCatalog,
} from "./plugin-catalog";
import {
  type ProfilePackageStoreView,
  parseProfilePackageInspection,
  parseProfilePackageStore,
  parseProfileProjectionResult,
} from "./profile-packages";
import { projectedRunCount } from "./run-projection";
import {
  type SubmissionQueueView,
  parseSubmissionImportResult,
  parseSubmissionQueue,
  parseSubmissionVerificationResult,
} from "./submission-queue";
import {
  type SubmissionPolicy,
  type SubmissionPolicyView,
  editableSubmissionPolicy,
  parseMockSubmissionResult,
  parseSubmissionPolicy,
} from "./submission-policy";
import type {
  DesktopHostAdapter,
  InstalledPluginDescriptor,
  MountedSurface,
  WorkspaceDescriptor,
  WorkspaceTabDescriptor,
} from "../shell/types";

const RUNTIME_WORKSPACE_ID = "host.rlogs.session-runtime";
const LOG_UPLOADER_WORKSPACE_ID = "app.rlogs.log-uploader";
const PROFILE_SYNC_WORKSPACE_ID = "app.rlogs.bpsr.profile-sync";

interface RuntimeResult {
  session_id: string;
  source_kind: string;
  output_rlog: string;
  coverage_report: string | null;
  frame_count: number | null;
  framed_record_count: number | null;
  canonical_event_count: number;
  known_route_count: number | null;
  unknown_route_count: number | null;
  data_gap_count: number | null;
  private_capture: string | null;
  connection_evidence: string | null;
  combat_plugin: RuntimePluginResult;
  encounter_recorder: RuntimePluginResult;
  submission_queue_id: string | null;
  submission_queue_status: string;
  profile_package_count: number;
  profile_sync_status: string;
  upload_artifact: {
    file_byte_length: number;
    file_sha256: string;
    chunk_count: number;
    canonical_content_sha256: string;
  };
}

interface RuntimePluginResult {
  metrics: {
    events_seen: number;
    events_delivered: number;
    outputs_emitted: number;
  };
  outputs: Array<{
    type?: string;
    schema_id?: string;
    payload?: unknown;
  }>;
}

interface RuntimeSnapshot {
  schema_version: number;
  phase: "idle" | "processing" | "complete" | "failed";
  active_session_id: string | null;
  detail: string;
  started_unix_millis: number | null;
  completed_unix_millis: number | null;
  live_capture_can_stop: boolean;
  last_result: RuntimeResult | null;
}

interface ApiError {
  error: string;
}

interface RuntimeEnvironment {
  platform: string;
  game_processes: Array<{
    process_id: number;
    executable_name: string;
  }>;
  dumpcap_path: string | null;
  capture_interfaces: Array<{
    value: string;
    label: string;
  }>;
}

const RUNTIME_WORKSPACE: WorkspaceDescriptor = {
  id: RUNTIME_WORKSPACE_ID,
  name: "Session Recorder",
  description:
    "Control the real local capture, decode, canonical log, and plug-in pipeline.",
  version: "0.1.0",
  iconUrl: null,
  iconFallback: "SR",
  defaultOrder: -100,
  tabs: [
    {
      id: "host.rlogs.session-runtime:control",
      label: "Control",
      kind: "content",
      entrypoint: "host://runtime/control",
      contributorPluginId: RUNTIME_WORKSPACE_ID,
    },
    {
      id: "host.rlogs.session-runtime:sessions",
      label: "Last Session",
      kind: "content",
      entrypoint: "host://runtime/sessions",
      contributorPluginId: RUNTIME_WORKSPACE_ID,
    },
    {
      id: "host.rlogs.session-runtime:events",
      label: "Event Viewer",
      kind: "content",
      entrypoint: "host://runtime/events",
      contributorPluginId: RUNTIME_WORKSPACE_ID,
    },
  ],
};

const LOG_UPLOADER_WORKSPACE: WorkspaceDescriptor = {
  id: LOG_UPLOADER_WORKSPACE_ID,
  name: "Log Uploader",
  description:
    "Manage verified combat-log drafts and opt in to future website submissions.",
  version: "0.1.0",
  iconUrl: null,
  iconFallback: "LU",
  defaultOrder: -90,
  tabs: [
    {
      id: "app.rlogs.log-uploader:queue",
      label: "Queue",
      kind: "content",
      entrypoint: "host://log-uploader/queue",
      contributorPluginId: LOG_UPLOADER_WORKSPACE_ID,
    },
    {
      id: "app.rlogs.log-uploader:options",
      label: "Options",
      kind: "options",
      entrypoint: "host://log-uploader/options",
      contributorPluginId: LOG_UPLOADER_WORKSPACE_ID,
    },
  ],
};

const PROFILE_SYNC_WORKSPACE: WorkspaceDescriptor = {
  id: PROFILE_SYNC_WORKSPACE_ID,
  name: "BPSR Profile Sync",
  description:
    "Control the separate opt-in path for Blue Protocol: Star Resonance character profiles.",
  version: "0.1.0",
  iconUrl: null,
  iconFallback: "PS",
  defaultOrder: -80,
  tabs: [
    {
      id: "app.rlogs.bpsr.profile-sync:status",
      label: "Status",
      kind: "content",
      entrypoint: "host://profile-sync/status",
      contributorPluginId: PROFILE_SYNC_WORKSPACE_ID,
    },
    {
      id: "app.rlogs.bpsr.profile-sync:options",
      label: "Options",
      kind: "options",
      entrypoint: "host://profile-sync/options",
      contributorPluginId: PROFILE_SYNC_WORKSPACE_ID,
    },
  ],
};

export async function createLocalHostAdapterIfAvailable(): Promise<DesktopHostAdapter | null> {
  try {
    const response = await fetch("/api/runtime/status", {
      cache: "no-store",
      headers: { Accept: "application/json" },
      signal: AbortSignal.timeout(1_500),
    });
    if (
      !response.ok ||
      !response.headers.get("content-type")?.includes("application/json")
    ) {
      return null;
    }
    await response.json();
    return createLocalHostAdapter();
  } catch {
    return null;
  }
}

function createLocalHostAdapter(): DesktopHostAdapter {
  const development = createDevelopmentAdapter();
  let pluginCatalogRequest: Promise<LocalPluginCatalog> | null = null;
  const loadPluginCatalog = (force = false): Promise<LocalPluginCatalog> => {
    if (force || pluginCatalogRequest === null) {
      pluginCatalogRequest = apiJson<unknown>("/api/plugins/catalog").then(
        parsePluginCatalog,
      );
    }
    return pluginCatalogRequest;
  };
  const updatePluginCatalog = async (
    route: string,
    init: RequestInit,
  ): Promise<LocalPluginCatalog> => {
    const catalog = parsePluginCatalog(await apiJson<unknown>(route, init));
    pluginCatalogRequest = Promise.resolve(catalog);
    return catalog;
  };

  return {
    modeLabel: "Local runtime",

    async loadWorkspaces() {
      const catalog = await loadPluginCatalog();
      return [
        RUNTIME_WORKSPACE,
        LOG_UPLOADER_WORKSPACE,
        PROFILE_SYNC_WORKSPACE,
        ...catalog.workspaces,
      ];
    },

    async loadPreferences() {
      return development.loadPreferences();
    },

    async savePreferences(preferences) {
      await development.savePreferences(preferences);
    },

    async mountSurface(workspace, tab, container) {
      if (workspace.id === RUNTIME_WORKSPACE_ID) {
        container.replaceChildren();
        switch (tab.entrypoint) {
          case "host://runtime/control":
            return mountControlSurface(container);
          case "host://runtime/sessions":
            return mountLastSessionSurface(container);
          case "host://runtime/events":
            return mountEventViewerSurface(container);
          default:
            throw new Error(`Unknown host surface ${tab.entrypoint}`);
        }
      }
      if (workspace.id === LOG_UPLOADER_WORKSPACE_ID) {
        container.replaceChildren();
        switch (tab.entrypoint) {
          case "host://log-uploader/queue":
            return mountSubmissionQueueSurface(container);
          case "host://log-uploader/options":
            return mountSubmissionPolicyOptionsSurface(
              container,
              "log_uploader",
            );
          default:
            throw new Error(`Unknown Log Uploader surface ${tab.entrypoint}`);
        }
      }
      if (workspace.id === PROFILE_SYNC_WORKSPACE_ID) {
        container.replaceChildren();
        switch (tab.entrypoint) {
          case "host://profile-sync/status":
            return mountProfileSyncStatusSurface(container);
          case "host://profile-sync/options":
            return mountSubmissionPolicyOptionsSurface(
              container,
              "bpsr_profile_sync",
            );
          default:
            throw new Error(`Unknown Profile Sync surface ${tab.entrypoint}`);
        }
      }
      const catalog = await loadPluginCatalog();
      return mountInstalledPackageSurface(
        container,
        workspace,
        tab,
        catalog.packages,
      );
    },

    async loadPluginCatalog() {
      return loadPluginCatalog();
    },

    async setPluginEnabled(pluginId, enabled) {
      return updatePluginCatalog("/api/plugins/enablement", {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({ pluginId, enabled }),
      });
    },

    async refreshPlugins() {
      return updatePluginCatalog("/api/plugins/refresh", {
        method: "POST",
      });
    },
  };
}

function mountInstalledPackageSurface(
  container: HTMLElement,
  workspace: WorkspaceDescriptor,
  tab: WorkspaceTabDescriptor,
  packages: readonly InstalledPluginDescriptor[],
): MountedSurface {
  container.replaceChildren();
  const owner = packages.find((plugin) => plugin.id === workspace.id);
  const contributor = packages.find(
    (plugin) => plugin.id === tab.contributorPluginId,
  );
  const surface = document.createElement("div");
  surface.className = "plugin-surface installed-package-surface";
  const status = actionCard(
    `${workspace.name} · ${tab.label}`,
    "This folder package and its workspace declaration passed host validation. Its executable surface remains unmounted until the matching sandboxed runtime adapter is available.",
  );
  const details = document.createElement("section");
  details.className = "content-card package-inspection-card";
  details.append(
    fileRow("Workspace owner", owner?.id ?? workspace.id),
    fileRow("Surface contributor", contributor?.id ?? tab.contributorPluginId),
    fileRow("Declared runtime", formatIdentifier(contributor?.runtime ?? "unknown")),
    fileRow(
      "Requested capabilities",
      contributor?.capabilities.length
        ? contributor.capabilities.map(formatIdentifier).join(", ")
        : "None",
    ),
    fileRow(
      "Event subscriptions",
      contributor?.subscriptions.length
        ? contributor.subscriptions.map(formatIdentifier).join(", ")
        : "None",
    ),
  );
  const boundary = actionCard(
    "Execution boundary",
    "rLogs has not executed package code, scripts, overlays, or external processes. Enablement currently publishes validated metadata and navigation only.",
  );
  surface.append(status, details, boundary);
  container.append(surface);
  return {
    dispose() {
      surface.remove();
    },
  };
}

function mountControlSurface(container: HTMLElement): MountedSurface {
  let alive = true;
  const root = document.createElement("div");
  root.className = "plugin-surface runtime-control-surface";

  const status = runtimeStatusCard();
  const safeReplay = actionCard(
    "Safe pipeline check",
    "Replay the sanitized canonical fixture through the bounded Combat Meter plug-in. This does not read packets or start capture.",
  );
  const replayButton = button("Run safe replay", "primary-button");
  const replayMessage = text("span", "Ready.", "runtime-action-message");
  replayButton.addEventListener("click", async () => {
    replayButton.disabled = true;
    replayMessage.textContent = "Running…";
    try {
      await apiJson<RuntimeResult>("/api/runtime/reference-replay", {
        method: "POST",
      });
      replayMessage.textContent = "Replay completed.";
      await refreshStatus(status);
    } catch (error) {
      replayMessage.textContent = errorMessage(error);
    } finally {
      replayButton.disabled = false;
    }
  });
  const safeActions = document.createElement("div");
  safeActions.className = "runtime-card-actions";
  safeActions.append(replayButton, replayMessage);
  safeReplay.append(safeActions);

  const offline = actionCard(
    "Process an existing BPSR capture",
    "Runs PCAP → exact connection filter → TCP reconstruction → BPSR protocol pack → canonical .rlog → Combat Meter. The source PCAP remains private and local.",
  );
  const form = document.createElement("form");
  form.className = "runtime-form";
  const sessionId = field(
    "Session ID",
    "text",
    defaultSessionId(),
    "letters, digits, dots, underscores, or dashes",
  );
  const capturePath = field(
    "Private PCAP or PCAPNG",
    "text",
    "",
    "C:\\path\\to\\capture.pcapng",
  );
  const connectionsPath = field(
    "Exact connection evidence",
    "text",
    "",
    "C:\\path\\to\\capture.connections.json",
  );
  const packPath = field(
    "Protocol pack override",
    "text",
    "",
    "leave blank for the current bundled Global pack",
  );
  const outputDirectory = field(
    "Log output folder",
    "text",
    "",
    "leave blank for RLogs/runtime-data/logs",
  );
  const formGrid = document.createElement("div");
  formGrid.className = "runtime-form-grid";
  formGrid.append(
    sessionId.label,
    capturePath.label,
    connectionsPath.label,
    packPath.label,
    outputDirectory.label,
  );
  const processButton = button("Process capture", "primary-button");
  processButton.type = "submit";
  const processMessage = text(
    "span",
    "No capture is started by this form.",
    "runtime-action-message",
  );
  const formActions = document.createElement("div");
  formActions.className = "runtime-card-actions";
  formActions.append(processButton, processMessage);
  form.append(formGrid, formActions);
  form.addEventListener("submit", async (event) => {
    event.preventDefault();
    processButton.disabled = true;
    processMessage.textContent = "Starting background session…";
    try {
      await apiJson<{ accepted: boolean }>("/api/runtime/offline", {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({
          session_id: sessionId.input.value.trim(),
          capture_path: capturePath.input.value.trim(),
          connections_path: connectionsPath.input.value.trim(),
          pack_path: emptyToNull(packPath.input.value),
          output_directory: emptyToNull(outputDirectory.input.value),
        }),
      });
      processMessage.textContent =
        "Accepted. The page will keep polling while the capture is decoded.";
      await refreshStatus(status);
    } catch (error) {
      processMessage.textContent = errorMessage(error);
    } finally {
      processButton.disabled = false;
    }
  });
  offline.append(form);

  const live = actionCard(
    "Live process-owned capture",
    "Starts only when you ask. The Windows adapter retains exact TCP flows owned by the selected BPSR_STEAM process, then Stop cooperatively finalizes the private PCAP, connection evidence, canonical .rlog, and plug-in result.",
  );
  const liveForm = document.createElement("form");
  liveForm.className = "runtime-form";
  const liveSessionId = field(
    "Session ID",
    "text",
    defaultSessionId(),
    "use a new ID for every capture",
  );
  const processId = field(
    "BPSR_STEAM process ID",
    "number",
    "",
    "Task Manager PID",
  );
  processId.input.min = "1";
  processId.input.step = "1";
  const captureInterface = field(
    "dumpcap interface",
    "text",
    "1",
    "interface number or \\\\Device\\\\NPF_…",
  );
  const dumpcapPath = field(
    "dumpcap executable",
    "text",
    "C:\\Program Files\\Wireshark\\dumpcap.exe",
    "absolute path to dumpcap.exe",
  );
  const duration = field(
    "Maximum duration in seconds",
    "number",
    "900",
    "1-3600",
  );
  duration.input.min = "1";
  duration.input.max = "3600";
  const privateOutput = field(
    "Private capture folder",
    "text",
    "",
    "leave blank for private-research/live-captures",
  );
  const logOutput = field(
    "Canonical log folder",
    "text",
    "",
    "leave blank for runtime-data/logs",
  );
  const liveGrid = document.createElement("div");
  liveGrid.className = "runtime-form-grid";
  liveGrid.append(
    liveSessionId.label,
    processId.label,
    captureInterface.label,
    dumpcapPath.label,
    duration.label,
    privateOutput.label,
    logOutput.label,
  );
  const startLive = button("Start live capture", "primary-button");
  startLive.type = "submit";
  const stopLive = button("Stop and finalize", "quiet-button");
  stopLive.disabled = true;
  const refreshEnvironment = button("Refresh detection", "quiet-button");
  const liveMessage = text(
    "span",
    "Start after entering the world if you do not want login traffic observed. Login/authentication routes are prohibited from canonical output regardless.",
    "runtime-action-message",
  );
  const liveActions = document.createElement("div");
  liveActions.className = "runtime-card-actions";
  liveActions.append(
    startLive,
    stopLive,
    refreshEnvironment,
    liveMessage,
  );
  liveForm.append(liveGrid, liveActions);
  liveForm.addEventListener("submit", async (event) => {
    event.preventDefault();
    startLive.disabled = true;
    liveMessage.textContent = "Starting process-owned capture…";
    try {
      await apiJson<{ accepted: boolean }>("/api/runtime/live/start", {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({
          session_id: liveSessionId.input.value.trim(),
          process_id: Number(processId.input.value),
          interface: captureInterface.input.value.trim(),
          dumpcap_path: dumpcapPath.input.value.trim(),
          duration_seconds: Number(duration.input.value),
          private_output_directory: emptyToNull(privateOutput.input.value),
          log_output_directory: emptyToNull(logOutput.input.value),
        }),
      });
      liveMessage.textContent =
        "Capturing exact process-owned flows. Stop when the run is complete.";
      stopLive.disabled = false;
      await updateRuntimeControls();
    } catch (error) {
      liveMessage.textContent = errorMessage(error);
      startLive.disabled = false;
    }
  });
  stopLive.addEventListener("click", async () => {
    stopLive.disabled = true;
    liveMessage.textContent = "Stop requested; finalizing owned data…";
    try {
      await apiJson<{ accepted: boolean }>("/api/runtime/live/stop", {
        method: "POST",
      });
      await updateRuntimeControls();
    } catch (error) {
      liveMessage.textContent = errorMessage(error);
      await updateRuntimeControls();
    }
  });
  const detectEnvironment = async () => {
    refreshEnvironment.disabled = true;
    try {
      const environment = await apiJson<RuntimeEnvironment>(
        "/api/runtime/environment",
      );
      if (environment.dumpcap_path !== null) {
        dumpcapPath.input.value = environment.dumpcap_path;
      }
      const process = environment.game_processes[0];
      if (process !== undefined) {
        processId.input.value = String(process.process_id);
      }
      const captureDevice = environment.capture_interfaces[0];
      if (captureDevice !== undefined) {
        captureInterface.input.value = captureDevice.value;
        captureInterface.input.title = captureDevice.label;
      }
      const processDetail =
        process === undefined
          ? "BPSR_STEAM is not currently detected"
          : environment.game_processes.length === 1
            ? `detected ${process.executable_name} PID ${process.process_id}`
            : `detected ${environment.game_processes.length} matching processes; using PID ${process.process_id}`;
      const interfaceDetail =
        captureDevice === undefined
          ? "no dumpcap interface was auto-selected"
          : `using ${captureDevice.label}`;
      liveMessage.textContent = `${processDetail}; ${interfaceDetail}.`;
    } catch (error) {
      liveMessage.textContent = errorMessage(error);
    } finally {
      refreshEnvironment.disabled = false;
    }
  };
  refreshEnvironment.addEventListener("click", () => {
    void detectEnvironment();
  });
  live.append(liveForm);

  root.append(status.card, safeReplay, offline, live);
  container.append(root);
  const updateRuntimeControls = async () => {
    const snapshot = await refreshStatus(status);
    if (snapshot === null) {
      startLive.disabled = false;
      stopLive.disabled = true;
      return;
    }
    const liveActive =
      snapshot.phase === "processing" && snapshot.live_capture_can_stop;
    startLive.disabled = snapshot.phase === "processing";
    stopLive.disabled = !liveActive;
  };
  void detectEnvironment();
  void updateRuntimeControls();
  const interval = window.setInterval(() => {
    if (alive) {
      void updateRuntimeControls();
    }
  }, 1_000);

  return {
    dispose() {
      alive = false;
      window.clearInterval(interval);
    },
  };
}

function mountLastSessionSurface(container: HTMLElement): MountedSurface {
  let alive = true;
  const root = document.createElement("div");
  root.className = "plugin-surface runtime-session-surface";
  const heading = actionCard(
    "Latest completed pipeline",
    "This view reports privacy-safe runtime counts and output locations. It never renders raw packet payloads or connection endpoints.",
  );
  const result = document.createElement("div");
  result.className = "runtime-result";
  root.append(heading, result);
  container.append(root);

  const refresh = async () => {
    try {
      const snapshot = await apiJson<RuntimeSnapshot>("/api/runtime/status");
      if (!alive) {
        return;
      }
      renderLastResult(result, snapshot);
    } catch (error) {
      result.replaceChildren(
        text("p", errorMessage(error), "runtime-action-message error"),
      );
    }
  };
  void refresh();
  const interval = window.setInterval(() => void refresh(), 1_000);
  return {
    dispose() {
      alive = false;
      window.clearInterval(interval);
    },
  };
}

function mountEventViewerSurface(container: HTMLElement): MountedSurface {
  let alive = true;
  let queryId: string | null = null;
  let busy = false;
  const root = document.createElement("div");
  root.className = "plugin-surface event-viewer-surface";
  const heading = actionCard(
    "Canonical Event Viewer",
    "Sealed, privacy-reviewed events after protocol decoding and before localization. IDs and amounts remain exact decimal strings; raw packet bytes and login/account payloads never enter this view.",
  );
  const controls = document.createElement("section");
  controls.className = "content-card event-viewer-controls";
  const filterGrid = document.createElement("div");
  filterGrid.className = "event-filter-grid";

  const topicLabel = document.createElement("label");
  topicLabel.className = "runtime-field";
  topicLabel.append(text("span", "Topic"));
  const topicSelect = document.createElement("select");
  topicSelect.append(new Option("All canonical topics", ""));
  for (const topic of EVENT_VIEWER_TOPICS) {
    topicSelect.append(new Option(formatIdentifier(topic), topic));
  }
  topicLabel.append(topicSelect);

  const kind = field(
    "Event kind",
    "text",
    "",
    "damage, cast, status, data_gap…",
  );
  const search = field(
    "Canonical ID search",
    "search",
    "",
    "entity UUID, actor, ability, status, scene…",
  );
  const pageSizeLabel = document.createElement("label");
  pageSizeLabel.className = "runtime-field";
  pageSizeLabel.append(text("span", "Rows per page"));
  const pageSize = document.createElement("select");
  for (const size of [50, 100, 200]) {
    pageSize.append(new Option(String(size), String(size), false, size === 100));
  }
  pageSizeLabel.append(pageSize);
  filterGrid.append(topicLabel, kind.label, search.label, pageSizeLabel);

  const actions = document.createElement("div");
  actions.className = "runtime-card-actions event-viewer-actions";
  const apply = button("Apply filters", "primary-button");
  const next = button("Next page", "secondary-button");
  next.disabled = true;
  const message = text(
    "span",
    "Waiting for a completed canonical log.",
    "runtime-action-message",
  );
  actions.append(apply, next, message);
  controls.append(filterGrid, actions);

  const metadata = document.createElement("section");
  metadata.className = "content-card event-viewer-metadata";
  metadata.hidden = true;
  const tableCard = document.createElement("section");
  tableCard.className = "content-card event-viewer-table-card";
  const tableMessage = text(
    "p",
    "Run the safe reference replay or complete a capture to inspect its canonical events.",
    "runtime-empty-result",
  );
  tableCard.append(tableMessage);

  const detail = document.createElement("section");
  detail.className = "content-card event-viewer-detail";
  detail.hidden = true;
  const detailHeader = document.createElement("div");
  detailHeader.className = "event-detail-header";
  const detailTitle = text("h2", "Canonical event");
  const copy = button("Copy exact JSON", "quiet-button");
  const canonical = document.createElement("pre");
  detailHeader.append(detailTitle, copy);
  detail.append(detailHeader, canonical);
  copy.addEventListener("click", () => {
    void navigator.clipboard
      .writeText(canonical.textContent ?? "")
      .then(() => {
        copy.textContent = "Copied";
        window.setTimeout(() => {
          if (alive) {
            copy.textContent = "Copy exact JSON";
          }
        }, 1_200);
      })
      .catch((error: unknown) => {
        message.textContent = errorMessage(error);
        message.classList.add("error");
      });
  });

  root.append(heading, controls, metadata, tableCard, detail);
  container.append(root);

  const selectedFilter = (): EventViewerFilter => ({
    topic:
      topicSelect.value === ""
        ? null
        : (topicSelect.value as EventViewerTopic),
    kind: emptyToNull(kind.input.value.toLowerCase()),
    search: emptyToNull(search.input.value),
  });

  const renderPage = (page: EventViewerPage) => {
    queryId = page.queryId;
    metadata.hidden = false;
    metadata.replaceChildren(
      fileRow("Session", page.sessionId),
      fileRow(
        "Region / realm",
        [
          page.header.region.identity.deployment_id,
          page.header.region.identity.region_id,
          page.header.region.identity.realm_id,
          page.header.region.identity.world_id,
        ]
          .filter((value): value is string => value !== null && value !== "")
          .join(" / "),
      ),
      fileRow("Client build", page.header.region.client_build),
      fileRow("Protocol pack", page.header.region.protocol_pack_digest),
      fileRow("Sealed digest", page.artifactDigest),
    );

    const tableWrap = document.createElement("div");
    tableWrap.className = "event-table-wrap";
    const table = document.createElement("table");
    table.className = "event-table";
    const header = document.createElement("thead");
    const headerRow = document.createElement("tr");
    for (const label of ["Seq", "Time", "Topic / event", "Canonical IDs", "Amount"]) {
      headerRow.append(text("th", label));
    }
    header.append(headerRow);
    const body = document.createElement("tbody");
    for (const event of page.events) {
      const row = document.createElement("tr");
      const sequenceCell = document.createElement("td");
      const open = button(String(event.sequence), "event-sequence-button");
      open.title = `Open canonical event ${event.sequence}`;
      open.addEventListener("click", () => {
        canonical.textContent = event.canonicalJson;
        detailTitle.textContent =
          `#${event.sequence} · ${formatIdentifier(event.kind)}`;
        detail.hidden = false;
        for (const selected of body.querySelectorAll("tr[data-selected]")) {
          selected.removeAttribute("data-selected");
        }
        row.dataset.selected = "true";
      });
      sequenceCell.append(open);
      const timeCell = text("td", formatObservedMicros(event.observedMicros));
      timeCell.title = `${event.observedMicros} observed microseconds`;
      const kindCell = document.createElement("td");
      kindCell.append(
        text("strong", formatIdentifier(event.kind)),
        text("small", formatIdentifier(event.topic)),
      );
      row.append(
        sequenceCell,
        timeCell,
        kindCell,
        text("td", event.summary),
        text("td", event.amount ?? "—"),
      );
      body.append(row);
    }
    table.append(header, body);
    tableWrap.append(table);

    if (page.events.length === 0) {
      tableCard.replaceChildren(
        text(
          "p",
          page.complete
            ? "No canonical events matched these filters."
            : "No matches in this bounded scan window. Continue to scan later events.",
          "runtime-empty-result",
        ),
      );
    } else {
      tableCard.replaceChildren(tableWrap);
    }
    next.disabled = page.complete;
    next.textContent = page.complete ? "End of log" : "Next page";
    message.classList.remove("error");
    message.textContent =
      `Page ${page.pageIndex} · scanned ${page.scannedTotal.toLocaleString()} · ` +
      `${page.matchedTotal.toLocaleString()} matches discovered` +
      (page.integrityVerified ? " · seal verified" : "");
  };

  const loadPage = async (continuation: boolean) => {
    if (busy) {
      return;
    }
    busy = true;
    apply.disabled = true;
    next.disabled = true;
    message.classList.remove("error");
    message.textContent = continuation
      ? "Scanning the next bounded event window…"
      : "Verifying the sealed log and applying filters…";
    try {
      const page = parseEventViewerPage(
        await apiJson<unknown>("/api/runtime/events/page", {
          method: "POST",
          headers: { "Content-Type": "application/json" },
          body: JSON.stringify(
            continuation
              ? {
                  queryId,
                  limit: Number(pageSize.value),
                }
              : {
                  queryId: null,
                  limit: Number(pageSize.value),
                  filter: selectedFilter(),
                },
          ),
        }),
      );
      if (alive) {
        renderPage(page);
      }
    } catch (error) {
      if (alive) {
        message.textContent = errorMessage(error);
        message.classList.add("error");
        next.disabled = queryId === null;
      }
    } finally {
      busy = false;
      apply.disabled = false;
    }
  };

  apply.addEventListener("click", () => {
    queryId = null;
    detail.hidden = true;
    void loadPage(false);
  });
  next.addEventListener("click", () => void loadPage(true));
  void loadPage(false);

  return {
    dispose() {
      alive = false;
    },
  };
}

function mountSubmissionPolicyOptionsSurface(
  container: HTMLElement,
  section: "log_uploader" | "bpsr_profile_sync",
): MountedSurface {
  let alive = true;
  let current: SubmissionPolicyView | null = null;
  const isUploader = section === "log_uploader";
  const root = document.createElement("div");
  root.className = "plugin-surface submission-policy-surface";
  const heading = actionCard(
    isUploader ? "Log Uploader consent" : "BPSR Profile Sync consent",
    isUploader
      ? "Controls permission to submit sealed combat artifacts. Local drafts continue to be created while this is off, so capture and recovery never depend on a website."
      : "Controls permission to build and submit Blue Protocol: Star Resonance character-profile packages. This is intentionally separate from combat-log consent.",
  );
  const form = document.createElement("form");
  form.className = "content-card submission-policy-form";
  const enable = checkboxOption(
    isUploader ? "Enable Log Uploader" : "Enable BPSR Profile Sync",
    isUploader
      ? "Permits local dry runs now and external submission only after a website transport is implemented."
      : "Reserves permission for profile projection and submission; neither is connected yet.",
  );
  const automatic = checkboxOption(
    isUploader
      ? "Automatically submit completed combat logs"
      : "Automatically sync character profiles",
    isUploader
      ? "Applies only after a real external transport exists. Sealed local drafts are still created either way."
      : "Applies only after a profile projection and external transport exist.",
  );
  form.append(enable.label, automatic.label);

  let visibility: HTMLSelectElement | null = null;
  let retention: HTMLSelectElement | null = null;
  if (isUploader) {
    const visibilityField = selectOption(
      "Default report visibility",
      "Used when a new local draft is created.",
      [
        ["private", "Private"],
        ["unlisted", "Unlisted"],
        ["public", "Public"],
      ],
    );
    visibility = visibilityField.select;
    const retentionField = selectOption(
      "Successful artifact retention",
      "Removal will occur only after a future server receipt is verified; the local mock never deletes artifacts.",
      [
        ["keep", "Keep local artifact"],
        [
          "remove_after_verified_receipt",
          "Remove after verified receipt",
        ],
      ],
    );
    retention = retentionField.select;
    form.append(visibilityField.label, retentionField.label);
  }

  const details = document.createElement("section");
  details.className = "content-card runtime-file-list submission-policy-details";
  const actions = document.createElement("div");
  actions.className = "runtime-card-actions submission-policy-actions";
  const save = button("Save options", "primary-button");
  save.type = "submit";
  save.disabled = true;
  const message = text(
    "span",
    "Loading fail-closed host settings…",
    "runtime-action-message",
  );
  actions.append(save, message);
  form.append(actions);
  root.append(heading, form, details);
  container.append(root);

  const applyView = (view: SubmissionPolicyView) => {
    current = view;
    if (isUploader) {
      enable.input.checked = view.log_uploader.enabled;
      automatic.input.checked = view.log_uploader.automatic_combat_logs;
      if (visibility !== null) {
        visibility.value = view.log_uploader.default_visibility;
      }
      if (retention !== null) {
        retention.value =
          view.log_uploader.successful_artifact_retention;
      }
    } else {
      enable.input.checked = view.bpsr_profile_sync.enabled;
      automatic.input.checked =
        view.bpsr_profile_sync.automatic_profiles;
    }
    details.replaceChildren(
      fileRow("Settings file", view.settings_path),
      fileRow("External transport", formatIdentifier(view.transport_mode)),
      fileRow(
        "Stored settings",
        view.issue === null ? "Valid" : `Fail-closed: ${view.issue}`,
      ),
    );
    message.classList.toggle("error", view.issue !== null);
    message.textContent =
      view.issue ??
      (isUploader
        ? "External networking remains unavailable in this build."
        : "Profile projection and external networking remain unavailable in this build.");
    save.disabled = false;
  };

  form.addEventListener("submit", async (event) => {
    event.preventDefault();
    if (current === null) {
      return;
    }
    save.disabled = true;
    message.classList.remove("error");
    message.textContent = "Writing options atomically…";
    const updated: SubmissionPolicy = editableSubmissionPolicy(current);
    if (isUploader && visibility !== null && retention !== null) {
      updated.log_uploader.enabled = enable.input.checked;
      updated.log_uploader.automatic_combat_logs =
        automatic.input.checked;
      updated.log_uploader.default_visibility =
        visibility.value as SubmissionPolicy["log_uploader"]["default_visibility"];
      updated.log_uploader.successful_artifact_retention =
        retention.value as SubmissionPolicy["log_uploader"]["successful_artifact_retention"];
    } else {
      updated.bpsr_profile_sync.enabled = enable.input.checked;
      updated.bpsr_profile_sync.automatic_profiles =
        automatic.input.checked;
    }
    try {
      const view = parseSubmissionPolicy(
        await apiJson<unknown>("/api/submissions/policy", {
          method: "POST",
          headers: { "Content-Type": "application/json" },
          body: JSON.stringify(updated),
        }),
      );
      if (alive) {
        applyView(view);
        message.classList.remove("error");
        message.textContent = `${isUploader ? "Log Uploader" : "BPSR Profile Sync"} options saved.`;
      }
    } catch (error) {
      if (alive) {
        message.textContent = errorMessage(error);
        message.classList.add("error");
        save.disabled = false;
      }
    }
  });

  void apiJson<unknown>("/api/submissions/policy")
    .then(parseSubmissionPolicy)
    .then((view) => {
      if (alive) {
        applyView(view);
      }
    })
    .catch((error: unknown) => {
      if (alive) {
        message.textContent = errorMessage(error);
        message.classList.add("error");
      }
    });

  return {
    dispose() {
      alive = false;
    },
  };
}

function mountProfileSyncStatusSurface(
  container: HTMLElement,
): MountedSurface {
  let alive = true;
  let busy = false;
  let currentPolicy: SubmissionPolicyView | null = null;
  const root = document.createElement("div");
  root.className = "plugin-surface profile-sync-status-surface";
  const heading = actionCard(
    "Reviewable character-profile packages",
    "Profile Sync merges only personal-gameplay BPSR character observations from a fully sealed log. Public lookups for other characters are excluded. Packages contain no website host, credentials, passwords, login tokens, account containers, or transport state.",
  );
  const headingActions = document.createElement("div");
  headingActions.className = "runtime-card-actions";
  const buildButton = button("Build from last sealed log", "primary-button");
  buildButton.disabled = true;
  const refreshButton = button("Refresh packages", "quiet-button");
  const message = text(
    "span",
    "Loading Profile Sync policy and local packages…",
    "runtime-action-message",
  );
  headingActions.append(buildButton, refreshButton, message);
  heading.append(headingActions);
  const status = document.createElement("section");
  status.className = "content-card runtime-file-list";
  status.append(fileRow("Policy", "Loading…"));
  const content = document.createElement("div");
  content.className = "profile-package-content";
  const inspection = document.createElement("section");
  inspection.className = "content-card profile-package-inspection";
  inspection.hidden = true;
  const boundary = actionCard(
    "Current boundary",
    "Projection and local review are implemented. Device pairing, authentication, automatic external submission, and website transport remain disconnected. Building a package makes zero external requests.",
  );
  root.append(heading, status, content, inspection, boundary);
  container.append(root);

  const render = (
    policy: SubmissionPolicyView,
    packages: ProfilePackageStoreView,
  ) => {
    currentPolicy = policy;
    buildButton.disabled = !policy.bpsr_profile_sync.enabled;
    status.replaceChildren(
      fileRow(
        "Profile Sync",
        policy.bpsr_profile_sync.enabled ? "Enabled" : "Disabled",
      ),
      fileRow(
        "Automatic packages",
        policy.bpsr_profile_sync.enabled &&
          policy.bpsr_profile_sync.automatic_profiles
          ? "Enabled after completed real sessions"
          : "Inactive",
      ),
      fileRow("External transport", formatIdentifier(policy.transport_mode)),
      fileRow("Package folder", packages.package_root),
    );

    const metrics = document.createElement("div");
    metrics.className = "runtime-result-grid profile-package-metrics";
    for (const [value, label] of [
      [packages.entry_count.toLocaleString(), "Current profiles"],
      [formatBytes(packages.total_package_bytes), "Local package storage"],
      [packages.issues.length.toLocaleString(), "Store diagnostics"],
    ] as const) {
      const metric = document.createElement("article");
      metric.append(text("strong", value), text("span", label));
      metrics.append(metric);
    }
    const children: HTMLElement[] = [metrics];
    if (packages.issues.length > 0) {
      const diagnostics = document.createElement("section");
      diagnostics.className = "content-card diagnostic-panel";
      diagnostics.append(
        text("h2", "Package diagnostics"),
        text(
          "p",
          "Invalid or tampered files remain excluded from review and future transport.",
        ),
      );
      const list = document.createElement("div");
      list.className = "diagnostic-list";
      for (const issue of packages.issues) {
        const item = document.createElement("article");
        item.className = "diagnostic-item";
        item.append(text("strong", issue));
        list.append(item);
      }
      diagnostics.append(list);
      children.push(diagnostics);
    }
    if (packages.entries.length === 0) {
      children.push(
        text(
          "p",
          policy.bpsr_profile_sync.enabled
            ? "No local profile package exists yet. Complete a world-load capture with Profile Sync active, or build from the last sealed log."
            : "Profile Sync is disabled. Existing packages would remain reviewable, but no new package will be generated.",
          "runtime-empty-result",
        ),
      );
    } else {
      const list = document.createElement("div");
      list.className = "profile-package-list";
      for (const profile of packages.entries) {
        const card = document.createElement("section");
        card.className = "content-card profile-package-entry";
        const header = document.createElement("header");
        const copy = document.createElement("div");
        copy.append(
          text(
            "strong",
            profile.display_name ?? `Character ${profile.character_id}`,
          ),
          text(
            "small",
            `UID ${profile.character_id} · ${profile.realm ?? profile.world ?? profile.region} · ${new Date(profile.created_unix_millis).toLocaleString()}`,
          ),
        );
        const pill = text(
          "span",
          `${profile.profile_field_count.toLocaleString()} fields`,
          "state-pill",
        );
        pill.dataset.state = "ready";
        header.append(copy, pill);
        const details = document.createElement("div");
        details.className = "submission-queue-entry-details";
        details.append(
          fileRow("Package SHA-256", profile.package_id),
          fileRow(
            "Class / specialization",
            `${profile.class_id ?? "—"} / ${profile.specialization_id ?? "—"}`,
          ),
          fileRow("Level", profile.level?.toLocaleString() ?? "—"),
          fileRow(
            "Region",
            `${profile.deployment} / ${profile.region} / ${profile.realm ?? profile.world ?? "unresolved"}`,
          ),
          fileRow(
            "Source evidence",
            `${profile.source_observation_count.toLocaleString()} observations through event ${profile.source_last_event_sequence.toLocaleString()}`,
          ),
          fileRow("Client build", profile.source_client_build),
          fileRow("Source session", profile.source_session_id),
          fileRow("Local package", profile.local_package_path),
        );
        const actions = document.createElement("div");
        actions.className =
          "runtime-card-actions submission-verification-actions";
        const inspectButton = button("Inspect exact JSON", "quiet-button");
        const inspectMessage = text(
          "span",
          `${formatBytes(profile.package_byte_length)} · local review only`,
          "runtime-action-message",
        );
        inspectButton.addEventListener("click", async () => {
          inspectButton.disabled = true;
          inspectMessage.classList.remove("error");
          inspectMessage.textContent = "Re-reading and validating package…";
          try {
            const result = parseProfilePackageInspection(
              await apiJson<unknown>("/api/profiles/packages/inspect", {
                method: "POST",
                headers: { "Content-Type": "application/json" },
                body: JSON.stringify({ packageId: profile.package_id }),
              }),
            );
            if (alive) {
              renderProfilePackageInspection(inspection, result.package);
              inspectMessage.textContent =
                "Stored contract revalidated before display.";
            }
          } catch (error) {
            if (alive) {
              inspectMessage.textContent = errorMessage(error);
              inspectMessage.classList.add("error");
            }
          } finally {
            inspectButton.disabled = false;
          }
        });
        actions.append(inspectButton, inspectMessage);
        card.append(header, details, actions);
        list.append(card);
      }
      children.push(list);
    }
    content.replaceChildren(...children);
    message.classList.toggle("error", policy.issue !== null);
    message.textContent =
      policy.issue ??
      `${packages.entry_count.toLocaleString()} reviewable package${packages.entry_count === 1 ? "" : "s"} · no network activity`;
  };

  const load = async (rescan: boolean) => {
    if (busy) {
      return;
    }
    busy = true;
    refreshButton.disabled = true;
    buildButton.disabled = true;
    message.classList.remove("error");
    message.textContent = "Reading bounded local package metadata…";
    try {
      const [policy, packages] = await Promise.all([
        apiJson<unknown>("/api/submissions/policy").then(
          parseSubmissionPolicy,
        ),
        apiJson<unknown>(
          rescan
            ? "/api/profiles/packages/refresh"
            : "/api/profiles/packages",
          rescan ? { method: "POST" } : undefined,
        ).then(parseProfilePackageStore),
      ]);
      if (alive) {
        render(policy, packages);
      }
    } catch (error) {
      if (alive) {
        message.textContent = errorMessage(error);
        message.classList.add("error");
        content.replaceChildren(
          text("p", errorMessage(error), "runtime-empty-result"),
        );
      }
    } finally {
      busy = false;
      refreshButton.disabled = false;
      buildButton.disabled =
        currentPolicy?.bpsr_profile_sync.enabled !== true;
    }
  };

  buildButton.addEventListener("click", async () => {
    if (busy) {
      return;
    }
    busy = true;
    buildButton.disabled = true;
    refreshButton.disabled = true;
    message.classList.remove("error");
    message.textContent =
      "Verifying the last sealed log and merging personal profile observations…";
    try {
      const result = parseProfileProjectionResult(
        await apiJson<unknown>("/api/profiles/project-last", {
          method: "POST",
        }),
      );
      const [policy, packages] = await Promise.all([
        apiJson<unknown>("/api/submissions/policy").then(
          parseSubmissionPolicy,
        ),
        apiJson<unknown>("/api/profiles/packages").then(
          parseProfilePackageStore,
        ),
      ]);
      if (alive) {
        render(policy, packages);
        message.classList.remove("error");
        message.textContent =
          result.projected_package_count === 0
            ? `No personal profile observations were present in ${result.source_session_id}; ${result.external_network_requests} external requests.`
            : `Built ${result.projected_package_count.toLocaleString()} package${result.projected_package_count === 1 ? "" : "s"} from ${result.source_session_id}; ${result.external_network_requests} external requests.`;
      }
    } catch (error) {
      if (alive) {
        message.textContent = errorMessage(error);
        message.classList.add("error");
      }
    } finally {
      busy = false;
      refreshButton.disabled = false;
      buildButton.disabled =
        currentPolicy?.bpsr_profile_sync.enabled !== true;
    }
  });
  refreshButton.addEventListener("click", () => void load(true));
  void load(false);

  return {
    dispose() {
      alive = false;
    },
  };
}

function renderProfilePackageInspection(
  container: HTMLElement,
  profilePackage: Record<string, unknown>,
) {
  const json = JSON.stringify(profilePackage, null, 2);
  const header = document.createElement("header");
  const copy = document.createElement("div");
  copy.append(
    text("h2", "Exact local profile package"),
    text(
      "p",
      "Raw IDs and privacy-reviewed field names are shown without localization. This is the exact contract a future authenticated transport would receive.",
    ),
  );
  const copyButton = button("Copy JSON", "quiet-button");
  const copyMessage = text(
    "span",
    "Nothing is transmitted.",
    "runtime-action-message",
  );
  copyButton.addEventListener("click", async () => {
    copyButton.disabled = true;
    copyMessage.classList.remove("error");
    try {
      await navigator.clipboard.writeText(json);
      copyMessage.textContent = "Copied exact JSON.";
    } catch (error) {
      copyMessage.textContent = errorMessage(error);
      copyMessage.classList.add("error");
    } finally {
      copyButton.disabled = false;
    }
  });
  const actions = document.createElement("div");
  actions.className = "runtime-card-actions";
  actions.append(copyButton, copyMessage);
  header.append(copy, actions);
  const pre = document.createElement("pre");
  pre.textContent = json;
  container.replaceChildren(header, pre);
  container.hidden = false;
}

function mountSubmissionQueueSurface(container: HTMLElement): MountedSurface {
  let alive = true;
  let busy = false;
  let currentPolicy: SubmissionPolicyView | null = null;
  const root = document.createElement("div");
  root.className = "plugin-surface submission-queue-surface";
  const heading = actionCard(
    "Local submission drafts",
    "Every real completed session creates one crash-safe local draft after the sealed .rlog passes integrity verification. The dry run below exercises resumable submission locally; no authentication or external network transport exists yet.",
  );
  const headingActions = document.createElement("div");
  headingActions.className = "runtime-card-actions";
  const refreshButton = button("Refresh local queue", "quiet-button");
  const refreshMessage = text(
    "span",
    "Reference fixtures are never queued.",
    "runtime-action-message",
  );
  headingActions.append(refreshButton, refreshMessage);
  heading.append(headingActions);

  const recovery = actionCard(
    "Recover an existing sealed log",
    "Add a previously completed .rlog without its original PCAP. The host streams the whole file once, validates its canonical seal and EOF, calculates exact file and chunk hashes, and only then creates a local draft using the configured default visibility.",
  );
  const importForm = document.createElement("form");
  importForm.className = "runtime-form submission-import-form";
  const artifactPath = field(
    "Existing sealed .rlog",
    "text",
    "",
    "C:\\path\\to\\completed-session.rlog",
  );
  const importActions = document.createElement("div");
  importActions.className = "runtime-card-actions";
  const importButton = button("Verify and add draft", "primary-button");
  importButton.type = "submit";
  const importMessage = text(
    "span",
    "The source file remains local and is not copied or transmitted.",
    "runtime-action-message",
  );
  importActions.append(importButton, importMessage);
  importForm.append(artifactPath.label, importActions);
  recovery.append(importForm);

  const content = document.createElement("div");
  content.className = "submission-queue-content";
  root.append(heading, recovery, content);
  container.append(root);

  async function verifyArtifact(
    queueId: string,
    verifyButton: HTMLButtonElement,
    verifyMessage: HTMLElement,
  ) {
    if (busy) {
      return;
    }
    busy = true;
    refreshButton.disabled = true;
    importButton.disabled = true;
    verifyButton.disabled = true;
    verifyMessage.classList.remove("error");
    verifyMessage.textContent =
      "Streaming and re-verifying the exact local artifact…";
    try {
      const result = parseSubmissionVerificationResult(
        await apiJson<unknown>("/api/submissions/queue/verify", {
          method: "POST",
          headers: { "Content-Type": "application/json" },
          body: JSON.stringify({ queueId }),
        }),
      );
      if (alive) {
        verifyMessage.textContent =
          `Exact file, seal, canonical digest, and ${result.artifact.chunk_count.toLocaleString()} chunk${result.artifact.chunk_count === 1 ? "" : "s"} verified at ` +
          new Date(result.verified_unix_millis).toLocaleString();
      }
    } catch (error) {
      if (alive) {
        verifyMessage.textContent = errorMessage(error);
        verifyMessage.classList.add("error");
      }
    } finally {
      busy = false;
      refreshButton.disabled = false;
      importButton.disabled = false;
      verifyButton.disabled = false;
    }
  }

  async function dryRunSubmission(
    queueId: string,
    dryRunButton: HTMLButtonElement,
    dryRunMessage: HTMLElement,
  ) {
    if (busy) {
      return;
    }
    busy = true;
    refreshButton.disabled = true;
    importButton.disabled = true;
    dryRunButton.disabled = true;
    dryRunMessage.classList.remove("error");
    dryRunMessage.textContent =
      "Re-verifying, chunking, interrupting, and resuming against the local mock receiver…";
    try {
      const result = parseMockSubmissionResult(
        await apiJson<unknown>("/api/submissions/mock/run", {
          method: "POST",
          headers: { "Content-Type": "application/json" },
          body: JSON.stringify({ queueId }),
        }),
      );
      if (alive) {
        dryRunMessage.textContent =
          `Local receipt ${result.report_id}: ${result.chunk_count.toLocaleString()} chunk${result.chunk_count === 1 ? "" : "s"}, ` +
          `${formatBytes(result.uploaded_bytes)}, restart recovered, ${result.external_network_requests} external requests.`;
      }
    } catch (error) {
      if (alive) {
        dryRunMessage.textContent = errorMessage(error);
        dryRunMessage.classList.add("error");
      }
    } finally {
      busy = false;
      refreshButton.disabled = false;
      importButton.disabled = false;
      dryRunButton.disabled = currentPolicy?.log_uploader.enabled !== true;
    }
  }

  const render = (
    queue: SubmissionQueueView,
    policy: SubmissionPolicyView,
  ) => {
    currentPolicy = policy;
    const metrics = document.createElement("div");
    metrics.className = "runtime-result-grid submission-queue-metrics";
    for (const [value, label] of [
      [queue.entry_count.toLocaleString(), "Local drafts"],
      [formatBytes(queue.total_artifact_bytes), "Artifact storage"],
      [queue.issues.length.toLocaleString(), "Queue diagnostics"],
    ] as const) {
      const metric = document.createElement("article");
      metric.append(text("strong", value), text("span", label));
      metrics.append(metric);
    }

    const location = document.createElement("section");
    location.className = "content-card runtime-file-list";
    location.append(
      fileRow("Queue folder", queue.queue_directory),
      fileRow(
        "Transmission",
        policy.log_uploader.enabled
          ? "Opted in — local mock transport only"
          : "Disabled — local queue inspection only",
      ),
      fileRow(
        "Default visibility",
        formatIdentifier(policy.log_uploader.default_visibility),
      ),
      fileRow("Transport", "Disconnected — 0 external network requests"),
    );

    const children: HTMLElement[] = [metrics, location];
    if (queue.issues.length > 0) {
      const diagnostics = document.createElement("section");
      diagnostics.className = "content-card diagnostic-panel";
      diagnostics.append(
        text("h2", "Queue diagnostics"),
        text(
          "p",
          "Invalid or unreadable queue files are isolated and are not treated as uploadable drafts.",
        ),
      );
      const list = document.createElement("div");
      list.className = "diagnostic-list";
      for (const issue of queue.issues) {
        const item = document.createElement("article");
        item.className = "diagnostic-item";
        item.append(text("strong", issue));
        list.append(item);
      }
      diagnostics.append(list);
      children.push(diagnostics);
    }

    if (queue.entries.length === 0) {
      children.push(
        text(
          "p",
          "No real completed sessions are queued. A sanitized reference replay deliberately does not create a draft.",
          "runtime-empty-result",
        ),
      );
    } else {
      const entries = document.createElement("div");
      entries.className = "submission-queue-list";
      for (const entry of queue.entries) {
        const card = document.createElement("section");
        card.className = "content-card submission-queue-entry";
        const header = document.createElement("header");
        const copy = document.createElement("div");
        copy.append(
          text("strong", entry.capture_session_id),
          text(
            "small",
            `${formatIdentifier(entry.state)} · ${formatIdentifier(entry.visibility)} · ${new Date(entry.created_unix_millis).toLocaleString()}`,
          ),
        );
        const integrity = text(
          "span",
          entry.artifact_exists && entry.artifact_byte_length_matches
            ? "Artifact present"
            : entry.artifact_exists
              ? "Length changed"
              : "Artifact missing",
          "state-pill",
        );
        integrity.dataset.state =
          entry.artifact_exists && entry.artifact_byte_length_matches
            ? "ready"
            : "blocked";
        header.append(copy, integrity);

        const details = document.createElement("div");
        details.className = "submission-queue-entry-details";
        details.append(
          fileRow("Queue ID / file SHA-256", entry.queue_id),
          fileRow("Canonical SHA-256", entry.canonical_content_sha256),
          fileRow(
            "Artifact",
            `${formatBytes(entry.file_byte_length)} in ${entry.chunk_count.toLocaleString()} chunk${entry.chunk_count === 1 ? "" : "s"}`,
          ),
          fileRow(
            "Game / region",
            `${entry.game_plugin_id} / ${entry.game_region}`,
          ),
          fileRow("Client build", entry.client_build),
          fileRow(
            "Local-only path",
            entry.local_artifact_path,
          ),
        );
        const verificationActions = document.createElement("div");
        verificationActions.className =
          "runtime-card-actions submission-verification-actions";
        const verifyButton = button(
          "Re-verify exact artifact",
          "quiet-button",
        );
        const verifyMessage = text(
          "span",
          "Size is a quick diagnostic; full seal and hash verification runs only when requested or immediately before a future upload.",
          "runtime-action-message",
        );
        verifyButton.addEventListener("click", () => {
          void verifyArtifact(entry.queue_id, verifyButton, verifyMessage);
        });
        const dryRunActions = document.createElement("div");
        dryRunActions.className =
          "runtime-card-actions submission-verification-actions";
        const dryRunButton = button(
          "Dry-run resumable upload",
          "primary-button",
        );
        dryRunButton.disabled = !policy.log_uploader.enabled;
        const dryRunMessage = text(
          "span",
          policy.log_uploader.enabled
            ? "Uses the exact upload state machine against an in-process mock receiver, including a forced restart."
            : "Enable Log Uploader in Options to permit this local-only test.",
          "runtime-action-message",
        );
        dryRunButton.addEventListener("click", () => {
          void dryRunSubmission(
            entry.queue_id,
            dryRunButton,
            dryRunMessage,
          );
        });
        dryRunActions.append(dryRunButton, dryRunMessage);
        verificationActions.append(verifyButton, verifyMessage);
        card.append(header, details, verificationActions, dryRunActions);
        entries.append(card);
      }
      children.push(entries);
    }
    content.replaceChildren(...children);
    refreshMessage.classList.remove("error");
    refreshMessage.textContent =
      `${queue.entry_count.toLocaleString()} local draft${queue.entry_count === 1 ? "" : "s"} · ${policy.log_uploader.enabled ? "mock enabled" : "uploader disabled"} · no network activity`;
  };

  importForm.addEventListener("submit", async (event) => {
    event.preventDefault();
    if (busy) {
      return;
    }
    busy = true;
    refreshButton.disabled = true;
    importButton.disabled = true;
    importMessage.classList.remove("error");
    importMessage.textContent =
      "Streaming and verifying the existing sealed log…";
    try {
      const result = parseSubmissionImportResult(
        await apiJson<unknown>("/api/submissions/queue/import", {
          method: "POST",
          headers: { "Content-Type": "application/json" },
          body: JSON.stringify({ artifactPath: artifactPath.input.value }),
        }),
      );
      const [queue, policy] = await Promise.all([
        apiJson<unknown>("/api/submissions/queue").then(parseSubmissionQueue),
        apiJson<unknown>("/api/submissions/policy").then(
          parseSubmissionPolicy,
        ),
      ]);
      if (alive) {
        render(queue, policy);
        importMessage.textContent =
          result.outcome === "queued"
            ? `Added ${result.capture_session_id} as a verified local draft.`
            : `${result.capture_session_id} is already queued with the same exact file hash.`;
      }
    } catch (error) {
      if (alive) {
        importMessage.textContent = errorMessage(error);
        importMessage.classList.add("error");
      }
    } finally {
      busy = false;
      refreshButton.disabled = false;
      importButton.disabled = false;
    }
  });

  const refresh = async (rescan: boolean) => {
    if (busy) {
      return;
    }
    busy = true;
    refreshButton.disabled = true;
    refreshMessage.classList.remove("error");
    refreshMessage.textContent = "Reading bounded local queue metadata…";
    try {
      const [queue, policy] = await Promise.all([
        apiJson<unknown>(
          rescan
            ? "/api/submissions/queue/refresh"
            : "/api/submissions/queue",
          rescan ? { method: "POST" } : undefined,
        ).then(parseSubmissionQueue),
        apiJson<unknown>("/api/submissions/policy").then(
          parseSubmissionPolicy,
        ),
      ]);
      if (alive) {
        render(queue, policy);
      }
    } catch (error) {
      if (alive) {
        refreshMessage.textContent = errorMessage(error);
        refreshMessage.classList.add("error");
        content.replaceChildren(
          text("p", errorMessage(error), "runtime-empty-result"),
        );
      }
    } finally {
      busy = false;
      refreshButton.disabled = false;
    }
  };
  refreshButton.addEventListener("click", () => void refresh(true));
  void refresh(false);

  return {
    dispose() {
      alive = false;
    },
  };
}

function runtimeStatusCard(): {
  card: HTMLElement;
  phase: HTMLElement;
  detail: HTMLElement;
  session: HTMLElement;
} {
  const card = document.createElement("section");
  card.className = "content-card runtime-status-card";
  const dot = document.createElement("span");
  dot.className = "runtime-status-dot";
  dot.setAttribute("aria-hidden", "true");
  const copy = document.createElement("div");
  const phase = text("strong", "Loading runtime…");
  const detail = text("p", "Checking the localhost controller.");
  copy.append(phase, detail);
  const session = text("span", "—", "runtime-session-id");
  card.append(dot, copy, session);
  return { card, phase, detail, session };
}

async function refreshStatus(
  status: ReturnType<typeof runtimeStatusCard>,
): Promise<RuntimeSnapshot | null> {
  try {
    const snapshot = await apiJson<RuntimeSnapshot>("/api/runtime/status");
    status.card.dataset.phase = snapshot.phase;
    status.phase.textContent = titleCase(snapshot.phase);
    status.detail.textContent = snapshot.detail;
    status.session.textContent =
      snapshot.active_session_id ??
      snapshot.last_result?.session_id ??
      "No session";
    return snapshot;
  } catch (error) {
    status.card.dataset.phase = "failed";
    status.phase.textContent = "Runtime unavailable";
    status.detail.textContent = errorMessage(error);
    status.session.textContent = "Disconnected";
    return null;
  }
}

function renderLastResult(container: HTMLElement, snapshot: RuntimeSnapshot) {
  const result = snapshot.last_result;
  if (result === null) {
    container.replaceChildren(
      text(
        "p",
        snapshot.phase === "processing"
          ? "A session is processing. Results appear after the .rlog seal is verified."
          : "No completed session yet. Run the safe replay or process an offline capture.",
        "runtime-empty-result",
      ),
    );
    return;
  }
  const metrics = document.createElement("div");
  metrics.className = "runtime-result-grid";
  const values: readonly (readonly [string, string])[] = [
    [String(result.frame_count ?? "—"), "Capture frames"],
    [String(result.framed_record_count ?? "—"), "BPSR records"],
    [String(result.canonical_event_count), "Canonical events"],
    [String(result.known_route_count ?? "—"), "Known routes"],
    [String(result.unknown_route_count ?? "—"), "Unknown routes"],
    [String(result.data_gap_count ?? "—"), "Data gaps"],
    [
      String(result.combat_plugin.metrics.events_delivered),
      "Events delivered",
    ],
    [
      String(result.combat_plugin.metrics.outputs_emitted),
      "Plug-in outputs",
    ],
    [
      String(projectedRunCount(result.encounter_recorder.outputs)),
      "Projected runs",
    ],
    [String(result.profile_package_count), "Profile packages"],
    [
      String(result.encounter_recorder.metrics.events_delivered),
      "Run evidence events",
    ],
    [String(result.upload_artifact.chunk_count), "Upload chunks"],
    [String(result.upload_artifact.file_byte_length), "Artifact bytes"],
  ];
  for (const [value, label] of values) {
    const metric = document.createElement("article");
    metric.append(text("strong", value), text("span", label));
    metrics.append(metric);
  }
  const files = document.createElement("section");
  files.className = "content-card runtime-file-list";
  files.append(
    fileRow("Sealed rlog", result.output_rlog),
    fileRow("Artifact SHA-256", result.upload_artifact.file_sha256),
    fileRow(
      "Canonical SHA-256",
      result.upload_artifact.canonical_content_sha256,
    ),
    fileRow("Queue status", formatIdentifier(result.submission_queue_status)),
    fileRow("Profile Sync", formatIdentifier(result.profile_sync_status)),
    fileRow(
      "Queue ID",
      result.submission_queue_id ?? "Not queued",
    ),
    fileRow("Coverage report", result.coverage_report ?? "Not applicable"),
    fileRow("Private capture", result.private_capture ?? "Not applicable"),
    fileRow(
      "Connection evidence",
      result.connection_evidence ?? "Not applicable",
    ),
  );
  container.replaceChildren(metrics, files);
}

function actionCard(title: string, detail: string): HTMLElement {
  const card = document.createElement("section");
  card.className = "content-card runtime-action-card";
  card.append(text("h2", title), text("p", detail));
  return card;
}

function field(
  labelText: string,
  type: string,
  value: string,
  placeholder: string,
): { label: HTMLLabelElement; input: HTMLInputElement } {
  const label = document.createElement("label");
  label.className = "runtime-field";
  const name = text("span", labelText);
  const input = document.createElement("input");
  input.type = type;
  input.value = value;
  input.placeholder = placeholder;
  input.autocomplete = "off";
  input.spellcheck = false;
  label.append(name, input);
  return { label, input };
}

function checkboxOption(
  title: string,
  detail: string,
): { label: HTMLLabelElement; input: HTMLInputElement } {
  const label = document.createElement("label");
  label.className = "submission-policy-toggle";
  const input = document.createElement("input");
  input.type = "checkbox";
  const copy = document.createElement("span");
  copy.append(text("strong", title), text("small", detail));
  label.append(input, copy);
  return { label, input };
}

function selectOption(
  title: string,
  detail: string,
  options: readonly (readonly [string, string])[],
): { label: HTMLLabelElement; select: HTMLSelectElement } {
  const label = document.createElement("label");
  label.className = "submission-policy-select";
  const copy = document.createElement("span");
  copy.append(text("strong", title), text("small", detail));
  const select = document.createElement("select");
  for (const [value, optionLabel] of options) {
    const option = document.createElement("option");
    option.value = value;
    option.textContent = optionLabel;
    select.append(option);
  }
  label.append(copy, select);
  return { label, select };
}

function fileRow(label: string, value: string): HTMLElement {
  const row = document.createElement("div");
  row.append(text("span", label), text("code", value));
  return row;
}

function button(label: string, className: string): HTMLButtonElement {
  const node = document.createElement("button");
  node.type = "button";
  node.className = className;
  node.textContent = label;
  return node;
}

function text<K extends keyof HTMLElementTagNameMap>(
  tagName: K,
  value: string,
  className?: string,
): HTMLElementTagNameMap[K] {
  const node = document.createElement(tagName);
  node.textContent = value;
  if (className !== undefined) {
    node.className = className;
  }
  return node;
}

async function apiJson<T>(
  route: string,
  init?: RequestInit,
): Promise<T> {
  const response = await fetch(route, {
    cache: "no-store",
    headers: { Accept: "application/json", ...init?.headers },
    ...init,
  });
  const body: unknown = await response.json();
  if (!response.ok) {
    const detail =
      isApiError(body) && body.error.trim() !== ""
        ? body.error
        : `Local runtime returned HTTP ${response.status}`;
    throw new Error(detail);
  }
  return body as T;
}

function isApiError(value: unknown): value is ApiError {
  return (
    typeof value === "object" &&
    value !== null &&
    "error" in value &&
    typeof value.error === "string"
  );
}

function emptyToNull(value: string): string | null {
  const trimmed = value.trim();
  return trimmed === "" ? null : trimmed;
}

function defaultSessionId(): string {
  const now = new Date();
  const pad = (value: number) => value.toString().padStart(2, "0");
  return `session-${now.getFullYear()}${pad(now.getMonth() + 1)}${pad(now.getDate())}-${pad(now.getHours())}${pad(now.getMinutes())}${pad(now.getSeconds())}`;
}

function titleCase(value: string): string {
  return value.charAt(0).toUpperCase() + value.slice(1);
}

function formatIdentifier(value: string): string {
  return value
    .split("_")
    .filter(Boolean)
    .map(titleCase)
    .join(" ");
}

function formatObservedMicros(value: number): string {
  return `${(value / 1_000_000).toFixed(3)}s`;
}

function formatBytes(value: number): string {
  if (value < 1_024) {
    return `${value.toLocaleString()} B`;
  }
  const units = ["KiB", "MiB", "GiB", "TiB"] as const;
  let amount = value / 1_024;
  let index = 0;
  while (amount >= 1_024 && index < units.length - 1) {
    amount /= 1_024;
    index += 1;
  }
  return `${amount.toFixed(amount >= 10 ? 1 : 2)} ${units[index]}`;
}

function errorMessage(error: unknown): string {
  return error instanceof Error ? error.message : String(error);
}
