import type {
  DesktopHostAdapter,
  MountedSurface,
  ShellPreferences,
  WorkspaceDescriptor,
  WorkspaceTabDescriptor,
} from "../shell/types";
import { mountModuleOptimizerSurface } from "./module-optimizer-surface";
import type {
  LocalModuleInventory,
  ModuleCandidate,
  OptimizeResponse,
  OptimizerCatalog,
} from "./module-optimizer";

const PREFERENCES_KEY = "rlogs.desktop-shell.preferences.v1";
const EXAMPLES_KEY = "rlogs.desktop-shell.examples-enabled.v1";

const SAMPLE_WORKSPACES: readonly WorkspaceDescriptor[] = [
  {
    id: "app.rlogs.combat-meter",
    name: "Combat Meter",
    description:
      "A live, local view assembled from canonical encounter events.",
    version: "0.1.0",
    iconUrl: null,
    iconFallback: "CM",
    defaultOrder: 10,
    tabs: [
      {
        id: "app.rlogs.combat-meter:live",
        label: "Live",
        kind: "content",
        entrypoint: "development://combat-meter/live",
        contributorPluginId: "app.rlogs.combat-meter",
        sectionId: "app.rlogs.combat-meter:main",
        defaultOrder: 0,
      },
    ],
  },
  {
    id: "app.rlogs.character-profiles",
    name: "Profile Sync",
    description:
      "Review character data and explicitly choose whether to pair with rLogs Website.",
    version: "0.1.0",
    iconUrl: null,
    iconFallback: "PS",
    defaultOrder: 20,
    tabs: [
      {
        id: "app.rlogs.character-profiles:profile",
        label: "Profile",
        kind: "content",
        entrypoint: "development://profile/profile",
        contributorPluginId: "app.rlogs.character-profiles",
        sectionId: "app.rlogs.character-profiles:main",
        defaultOrder: 0,
      },
      {
        id: "app.rlogs.character-profiles:sync",
        label: "Sync",
        kind: "content",
        entrypoint: "development://profile/sync",
        contributorPluginId: "app.rlogs.character-profiles",
        sectionId: "app.rlogs.character-profiles:main",
        defaultOrder: 1,
      },
      {
        id: "app.rlogs.module-optimizer:modules",
        label: "Modules",
        kind: "content",
        entrypoint: "development://profile/modules",
        contributorPluginId: "app.rlogs.module-optimizer",
        sectionId: "app.rlogs.module-optimizer:modules",
        defaultOrder: 200,
      },
      {
        id: "app.rlogs.character-profiles:options",
        label: "Options",
        kind: "options",
        entrypoint: "development://profile/options",
        contributorPluginId: "app.rlogs.character-profiles",
        sectionId: "app.rlogs.character-profiles:main",
        defaultOrder: 2,
      },
    ],
  },
  {
    id: "app.rlogs.log-library",
    name: "Log Library",
    description:
      "Browse complete local run timelines before choosing what to submit.",
    version: "0.1.0",
    iconUrl: null,
    iconFallback: "LL",
    defaultOrder: 30,
    tabs: [
      {
        id: "app.rlogs.log-library:runs",
        label: "Runs",
        kind: "content",
        entrypoint: "development://logs/runs",
        contributorPluginId: "app.rlogs.log-library",
        sectionId: "app.rlogs.log-library:main",
        defaultOrder: 0,
      },
      {
        id: "app.rlogs.log-library:uploads",
        label: "Uploads",
        kind: "content",
        entrypoint: "development://logs/uploads",
        contributorPluginId: "app.rlogs.log-library",
        sectionId: "app.rlogs.log-library:main",
        defaultOrder: 1,
      },
    ],
  },
];

const DEFAULT_PREFERENCES: ShellPreferences = {
  schemaVersion: 1,
  workspaceOrder: [],
  activeWorkspaceId: null,
  activeTabs: {},
  tabOrders: {},
  sectionOrders: {},
  lockTabDragging: false,
  lockSectionDragging: false,
};

export function createDevelopmentAdapter(): DesktopHostAdapter {
  return {
    modeLabel: "Shell prototype",

    async loadWorkspaces() {
      return examplesEnabled() ? SAMPLE_WORKSPACES : [];
    },

    async loadPreferences() {
      return readPreferences();
    },

    async savePreferences(preferences) {
      try {
        localStorage.setItem(PREFERENCES_KEY, JSON.stringify(preferences));
      } catch {
        // A restricted WebView may reject persistence. The shell can continue
        // with session-only state until the native settings adapter is ready.
      }
    },

    async mountSurface(workspace, tab, container) {
      container.replaceChildren();
      if (tab.entrypoint === "development://profile/modules") {
        return mountDevelopmentModuleOptimizer(container);
      }
      const surface = renderDevelopmentSurface(workspace, tab);
      container.append(surface);
      return noOpSurface();
    },

    async setExampleWorkspacesEnabled(enabled) {
      try {
        localStorage.setItem(EXAMPLES_KEY, enabled ? "true" : "false");
      } catch {
        // The query string still provides a deterministic blank-shell route.
      }
    },
  };
}

const DEVELOPMENT_OPTIMIZER_CATALOG: OptimizerCatalog = {
  game_id: "blue-protocol-star-resonance",
  catalog_revision: "development-visual-fixture",
  scoring_revision: "reviewed-cn-formula",
  client_builds: ["24687926"],
  combination_sizes: [4, 5],
  default_max_solutions: 10,
  attributes: [
    [1110, "Strength", "Strength Boost", "1110-strength-boost.png"],
    [1111, "Agility", "Agility Boost", "1111-agility-boost.png"],
    [1112, "Intellect", "Intellect Boost", "1112-intellect-boost.png"],
    [1113, "Willpower", "Willpower Boost", "1113-willpower-boost.png"],
    [1114, "Endurance", "Endurance Boost", "1114-endurance-boost.png"],
    [1407, "Critical", "Critical Boost", "1407-critical-boost.png"],
    [1408, "Haste", "Haste Boost", "1408-haste-boost.png"],
    [1409, "Mastery", "Mastery Boost", "1409-mastery-boost.png"],
    [1410, "Luck", "Luck Boost", "1410-luck-boost.png"],
    [2104, "Attack", "Attack Boost", "2104-attack-boost.png"],
  ].map(([id, name, officialName, icon]) => ({
    id: Number(id),
    name: String(name),
    official_name: String(officialName),
    icon: `icons/modules/effects/${icon}`,
    thresholds: [1, 4, 8, 12, 16, 20],
    fight_values: [7, 14, 29, 44, 167, 254],
  })),
};

const DEVELOPMENT_MODULES: readonly ModuleCandidate[] = Array.from(
  { length: 12 },
  (_, index) => ({
    instance_id: `9007199254741${String(index).padStart(3, "0")}`,
    config_id: 5_500_101 + (index % 8),
    quality: 3 + (index % 3),
    parts: [
      { part_id: 1110 + (index % 5), initial_link_points: 4 + (index % 7) },
      { part_id: 1407 + (index % 4), initial_link_points: 3 + ((index * 3) % 8) },
      { part_id: 2104, initial_link_points: 2 + ((index * 5) % 9) },
    ],
  }),
);

const DEVELOPMENT_OPTIMIZER_INVENTORY: LocalModuleInventory = {
  schema_version: 1,
  characters: [
    {
      package_id: "development-character",
      character_id: "3296036",
      display_name: "Local Character",
      deployment: "global",
      region: "na",
      source_client_build: "24687926",
      observed_unix_millis: Date.now(),
      modules: DEVELOPMENT_MODULES,
      current_instance_ids: DEVELOPMENT_MODULES.slice(0, 4).map(
        (module) => module.instance_id,
      ),
      module_snapshot_available: true,
      module_snapshot_detail: "12 owned modules · 4 equipped",
    },
  ],
  issues: [],
};

function mountDevelopmentModuleOptimizer(container: HTMLElement): MountedSurface {
  return mountModuleOptimizerSurface(container, {
    async loadCatalog() {
      return DEVELOPMENT_OPTIMIZER_CATALOG;
    },
    async loadInventory() {
      return DEVELOPMENT_OPTIMIZER_INVENTORY;
    },
    async loadGpuSupport() {
      return {
        available: true,
        backend: "open_cl",
        device_name: "GeForce RTX 5060",
        vendor: "NVIDIA",
        detail: "OpenCL exact search is ready.",
      };
    },
    async optimize(request) {
      const modules = DEVELOPMENT_MODULES.slice(0, request.combination_size);
      const score = developmentSolution(modules);
      return {
        scoring_revision: "reviewed-cn-formula",
        catalog_revision: "development-visual-fixture",
        current_setup: score,
        solutions: [
          developmentSolution(
            DEVELOPMENT_MODULES.slice(-request.combination_size),
          ),
          developmentSolution(
            DEVELOPMENT_MODULES.slice(2, 2 + request.combination_size),
          ),
        ],
        search: {
          requested_mode: request.search_mode,
          used_mode: "exact",
          exact: true,
          input_module_count: DEVELOPMENT_MODULES.length,
          candidate_module_count: DEVELOPMENT_MODULES.length,
          excluded_module_count: 0,
          total_combinations: 495,
          evaluated_states: 495,
          combination_size: request.combination_size,
          beam_width: null,
          backend: "cpu",
          accelerator_name: null,
          accelerator_fallback: request.use_gpu
            ? "multi-core CPU is faster below 50000 exact combinations"
            : null,
        },
      } satisfies OptimizeResponse;
    },
  });
}

function developmentSolution(modules: readonly ModuleCandidate[]) {
  const totals = new Map<number, number>();
  let totalLink = 0;
  for (const module of modules) {
    for (const part of module.parts) {
      const value = Math.max(0, part.initial_link_points ?? 0);
      totals.set(part.part_id, (totals.get(part.part_id) ?? 0) + value);
      totalLink += value;
    }
  }
  const attributes = [...totals].map(([attribute_id, total]) => ({
    attribute_id,
    total,
    reached_threshold: total,
    base_power: total * 7,
    multiplier: 1,
    applied_power: total * 7,
  }));
  const thresholdPower = attributes.reduce(
    (sum, attribute) => sum + attribute.applied_power,
    0,
  );
  return {
    instance_ids: modules.map((module) => module.instance_id),
    modules,
    score: thresholdPower + totalLink * 4,
    ranking_score: thresholdPower + totalLink * 4,
    breakdown: {
      threshold_power: thresholdPower,
      ranking_threshold_power: thresholdPower,
      total_link_points: totalLink,
      total_link_power: totalLink * 4,
      attributes,
    },
  };
}

function examplesEnabled(): boolean {
  const query = new URLSearchParams(window.location.search);
  if (query.get("empty") === "1") {
    return false;
  }
  try {
    return localStorage.getItem(EXAMPLES_KEY) !== "false";
  } catch {
    return true;
  }
}

function readPreferences(): ShellPreferences {
  try {
    const raw = localStorage.getItem(PREFERENCES_KEY);
    if (raw === null) {
      return DEFAULT_PREFERENCES;
    }
    const value: unknown = JSON.parse(raw);
    if (!isRecord(value)) {
      return DEFAULT_PREFERENCES;
    }
    const workspaceOrder = Array.isArray(value.workspaceOrder)
      ? value.workspaceOrder.filter(
          (entry): entry is string => typeof entry === "string",
        )
      : [];
    const activeWorkspaceId =
      typeof value.activeWorkspaceId === "string"
        ? value.activeWorkspaceId
        : null;
    const activeTabs = isRecord(value.activeTabs)
      ? Object.fromEntries(
          Object.entries(value.activeTabs).filter(
            (entry): entry is [string, string] =>
              typeof entry[1] === "string",
          ),
        )
      : {};
    const tabOrders = readOrderMap(value.tabOrders);
    const sectionOrders = readOrderMap(value.sectionOrders);
    return {
      schemaVersion: 1,
      workspaceOrder,
      activeWorkspaceId,
      activeTabs,
      tabOrders,
      sectionOrders,
      lockTabDragging: value.lockTabDragging === true,
      lockSectionDragging: value.lockSectionDragging === true,
    };
  } catch {
    return DEFAULT_PREFERENCES;
  }
}

function readOrderMap(value: unknown): Record<string, readonly string[]> {
  if (!isRecord(value)) return {};
  return Object.fromEntries(
    Object.entries(value).flatMap(([key, entries]) =>
      Array.isArray(entries) &&
      entries.every((entry): entry is string => typeof entry === "string")
        ? [[key, entries]]
        : [],
    ),
  );
}

function renderDevelopmentSurface(
  workspace: WorkspaceDescriptor,
  tab: WorkspaceTabDescriptor,
): HTMLElement {
  switch (tab.entrypoint) {
    case "development://combat-meter/live":
      return combatMeterSurface();
    case "development://profile/profile":
      return profileSurface();
    case "development://profile/sync":
      return profileSyncSurface();
    case "development://profile/options":
      return profileOptionsSurface();
    case "development://profile/modules":
      return profileModulesSurface();
    case "development://logs/runs":
      return logRunsSurface();
    case "development://logs/uploads":
      return logUploadsSurface();
    default:
      return unknownSurface(workspace, tab);
  }
}

function combatMeterSurface(): HTMLElement {
  const surface = surfaceRoot("meter-surface");
  const state = statusStrip(
    "Waiting for canonical events",
    "The capture and game plug-ins are not attached to this browser prototype.",
  );
  const metrics = document.createElement("div");
  metrics.className = "metric-grid";
  const metricValues: readonly (readonly [string, string])[] = [
    ["—", "Encounter time"],
    ["0", "Active players"],
    ["0", "Events received"],
    ["Idle", "Parser state"],
  ];
  for (const [value, label] of metricValues) {
    const card = document.createElement("article");
    card.className = "metric-card";
    card.append(textElement("strong", value), textElement("span", label));
    metrics.append(card);
  }
  const tableCard = document.createElement("section");
  tableCard.className = "content-card table-card";
  tableCard.append(
    cardHeading(
      "Live party",
      "Only the active tab surface is mounted by the host.",
    ),
  );
  const empty = document.createElement("div");
  empty.className = "inline-empty";
  empty.textContent = "Combat rows will appear when an encounter begins.";
  tableCard.append(empty);
  surface.append(state, metrics, tableCard);
  return surface;
}

function profileSurface(): HTMLElement {
  const surface = surfaceRoot("profile-surface");
  const card = document.createElement("section");
  card.className = "content-card profile-card";
  const portrait = document.createElement("div");
  portrait.className = "portrait-placeholder";
  portrait.textContent = "?";
  portrait.setAttribute("aria-hidden", "true");
  const identity = document.createElement("div");
  identity.className = "profile-identity";
  identity.append(
    textElement("span", "LOCAL CHARACTER PREVIEW", "surface-kicker"),
    textElement("h2", "No character observed yet"),
    textElement(
      "p",
      "The game plug-in will project public character UID, region, server, class, level, guild, gear, talents, modules, and owned progression fields as each route is decoded.",
    ),
  );
  card.append(portrait, identity);

  const fields = document.createElement("div");
  fields.className = "profile-fields";
  const profileFields: readonly (readonly [string, string])[] = [
    ["Region", "Waiting"],
    ["Server", "Waiting"],
    ["Character UID", "Waiting"],
    ["Guild", "Waiting"],
    ["Last observed", "Never"],
    ["Website link", "Not paired"],
  ];
  for (const [label, value] of profileFields) {
    const field = document.createElement("div");
    field.append(
      textElement("span", label),
      textElement("strong", value),
    );
    fields.append(field);
  }
  surface.append(card, fields);
  return surface;
}

function profileSyncSurface(): HTMLElement {
  const surface = surfaceRoot("sync-surface");
  surface.append(
    statusStrip(
      "Local only",
      "Nothing is sent while Profile Sync is disabled or unpaired.",
    ),
  );
  const grid = document.createElement("div");
  grid.className = "two-column-grid";
  grid.append(
    informationCard(
      "Profile payload",
      "Character identity, region, server, progression, equipment, talents, modules, and selected public guild fields.",
      ["Explicit opt-in", "Schema versioned", "No account credentials"],
    ),
    informationCard(
      "Pairing state",
      "Website device pairing has not been connected to the native host.",
      ["No key generated", "No pending upload", "No Discord link"],
    ),
  );
  surface.append(grid);
  return surface;
}

function profileOptionsSurface(): HTMLElement {
  const surface = surfaceRoot("options-surface");
  const panel = document.createElement("section");
  panel.className = "content-card options-card";
  panel.append(
    cardHeading(
      "Website pairing",
      "This setting belongs to Profile Sync, not rLogs Core.",
    ),
  );
  const row = document.createElement("div");
  row.className = "option-row";
  const copy = document.createElement("div");
  copy.append(
    textElement("strong", "Share this character profile"),
    textElement(
      "p",
      "When enabled, the native host can generate a one-time pairing credential and submit approved profile updates.",
    ),
  );
  const toggle = document.createElement("button");
  toggle.className = "toggle-control";
  toggle.type = "button";
  toggle.setAttribute("role", "switch");
  toggle.setAttribute("aria-checked", "false");
  toggle.setAttribute(
    "aria-label",
    "Share this character profile. Native pairing is not connected.",
  );
  toggle.disabled = true;
  toggle.append(document.createElement("span"));
  row.append(copy, toggle);

  const credential = document.createElement("div");
  credential.className = "credential-box";
  const credentialCopy = document.createElement("div");
  credentialCopy.append(
    textElement("span", "PAIRING CREDENTIAL", "surface-kicker"),
    textElement("strong", "Not generated"),
    textElement(
      "p",
      "The finished desktop host will store this secret in Windows Credential Manager or the Linux Secret Service. It will not be written to plug-in assets, logs, or browser storage.",
    ),
  );
  const connect = document.createElement("button");
  connect.className = "primary-button";
  connect.type = "button";
  connect.textContent = "Connect native host first";
  connect.disabled = true;
  credential.append(credentialCopy, connect);
  panel.append(row, credential);
  surface.append(panel);
  return surface;
}

function profileModulesSurface(): HTMLElement {
  const surface = surfaceRoot("modules-surface");
  const notice = statusStrip(
    "Contributed by Module Optimizer",
    "This surface remains owned, versioned, and removable by the add-on package.",
  );
  const card = document.createElement("section");
  card.className = "content-card";
  card.append(
    cardHeading(
      "Current module build",
      "Profile data is shared through a typed host capability, not copied between plug-ins.",
    ),
  );
  const empty = document.createElement("div");
  empty.className = "inline-empty tall";
  empty.textContent =
    "Observed modules and optimization controls will appear here when both plug-ins are active.";
  card.append(empty);
  surface.append(notice, card);
  return surface;
}

function logRunsSurface(): HTMLElement {
  const surface = surfaceRoot("logs-surface");
  surface.append(
    statusStrip(
      "No completed runs",
      "Complete timelines will remain local until the user chooses to submit one.",
    ),
  );
  const card = document.createElement("section");
  card.className = "content-card";
  card.append(
    cardHeading(
      "Run library",
      "Region, server, scene, participants, and parser revision will travel with each log.",
    ),
  );
  const empty = document.createElement("div");
  empty.className = "inline-empty tall";
  empty.textContent = "Your first completed dungeon run will appear here.";
  card.append(empty);
  surface.append(card);
  return surface;
}

function logUploadsSurface(): HTMLElement {
  const surface = surfaceRoot("uploads-surface");
  surface.append(
    informationCard(
      "Submission queue",
      "The upload plug-in will validate, sign, retry, and report each website submission independently from local capture.",
      ["0 queued", "0 uploading", "0 failed"],
    ),
  );
  return surface;
}

function unknownSurface(
  workspace: WorkspaceDescriptor,
  tab: WorkspaceTabDescriptor,
): HTMLElement {
  return informationCard(
    "Surface unavailable",
    `${workspace.name} published ${tab.entrypoint}, but the development adapter has no renderer for it.`,
    [],
  );
}

function surfaceRoot(className: string): HTMLElement {
  const root = document.createElement("div");
  root.className = `plugin-surface ${className}`;
  return root;
}

function statusStrip(title: string, detail: string): HTMLElement {
  const strip = document.createElement("div");
  strip.className = "status-strip";
  const dot = document.createElement("span");
  dot.className = "status-strip-dot";
  dot.setAttribute("aria-hidden", "true");
  const copy = document.createElement("span");
  copy.append(textElement("strong", title), textElement("small", detail));
  strip.append(dot, copy);
  return strip;
}

function informationCard(
  title: string,
  detail: string,
  facts: readonly string[],
): HTMLElement {
  const card = document.createElement("section");
  card.className = "content-card information-card";
  card.append(textElement("h2", title), textElement("p", detail));
  if (facts.length > 0) {
    const list = document.createElement("ul");
    for (const fact of facts) {
      const item = document.createElement("li");
      item.textContent = fact;
      list.append(item);
    }
    card.append(list);
  }
  return card;
}

function cardHeading(title: string, detail: string): HTMLElement {
  const heading = document.createElement("header");
  heading.className = "card-heading";
  heading.append(textElement("h2", title), textElement("span", detail));
  return heading;
}

function textElement<K extends keyof HTMLElementTagNameMap>(
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

function noOpSurface(): MountedSurface {
  return { dispose() {} };
}

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null && !Array.isArray(value);
}
