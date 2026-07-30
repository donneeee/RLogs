import { createDevelopmentAdapter } from "./development-adapter";
import type {
  DesktopHostAdapter,
  MountedSurface,
  WorkspaceDescriptor,
} from "../shell/types";

const RUNTIME_WORKSPACE_ID = "host.rlogs.session-runtime";

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
  combat_plugin: {
    metrics: {
      events_seen: number;
      events_delivered: number;
      outputs_emitted: number;
    };
    outputs: unknown[];
  };
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
  return {
    modeLabel: "Local runtime",

    async loadWorkspaces() {
      const workspaces = await development.loadWorkspaces();
      return [RUNTIME_WORKSPACE, ...workspaces];
    },

    async loadPreferences() {
      return development.loadPreferences();
    },

    async savePreferences(preferences) {
      await development.savePreferences(preferences);
    },

    async mountSurface(workspace, tab, container) {
      if (workspace.id !== RUNTIME_WORKSPACE_ID) {
        return development.mountSurface(workspace, tab, container);
      }
      container.replaceChildren();
      switch (tab.entrypoint) {
        case "host://runtime/control":
          return mountControlSurface(container);
        case "host://runtime/sessions":
          return mountLastSessionSurface(container);
        default:
          throw new Error(`Unknown host surface ${tab.entrypoint}`);
      }
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

function errorMessage(error: unknown): string {
  return error instanceof Error ? error.message : String(error);
}
