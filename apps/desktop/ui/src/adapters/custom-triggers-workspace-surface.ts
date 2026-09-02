import type { MountedSurface } from "../shell/types";

export type CustomTriggersWorkspacePage =
  | "overview"
  | "rules"
  | "event-inspector"
  | "library"
  | "settings";

interface MenuItem {
  title: string;
  description: string;
  items?: readonly string[];
}

interface PageDefinition {
  eyebrow: string;
  title: string;
  description: string;
  items: readonly MenuItem[];
}

const PAGE_DEFINITIONS: Record<CustomTriggersWorkspacePage, PageDefinition> = {
  overview: {
    eyebrow: "IF / THEN AUTOMATION",
    title: "Custom Triggers",
    description:
      "Build readable rules from verified game events, then send their actions to overlays, audio, counters, timers, and map markers.",
    items: [
      {
        title: "Rules",
        description: "Create, group, enable, test, and review readable When / If / Then rules.",
        items: ["My rules", "Folders", "Enable or disable groups", "Search"],
      },
      {
        title: "Library",
        description: "Start from reviewed encounter packs, timelines, class packs, and utility patterns.",
        items: ["Encounter packs", "Timelines", "Class packs", "Imports"],
      },
      {
        title: "Event Inspector",
        description: "Watch a bounded live event stream, inspect decoded fields, and use a selected value to start a rule.",
        items: ["Live follow", "Decoded fields", "Pin and compare", "Create rule from event"],
      },
      {
        title: "Integrations",
        description: "Choose where trigger actions are allowed to appear or play.",
        items: ["Overlay", "Sound", "Mechanics Map"],
      },
    ],
  },
  rules: {
    eyebrow: "RULE BUILDER",
    title: "Rules",
    description:
      "Keep the full automation sentence visible: When an event occurs, optionally check conditions, then perform one or more actions.",
    items: [
      {
        title: "My Rules & Folders",
        description: "Organize rules by encounter, class, role, purpose, or personal workflow, then enable or disable a whole branch.",
        items: ["Nested folders", "Enable as a group", "Scope", "Search and filters"],
      },
      {
        title: "When",
        description: "Choose the event that begins the rule.",
        items: ["Source or target UID", "Skill or effect", "Stat or resource", "Scene or timer"],
      },
      {
        title: "If",
        description: "Add optional conditions without hiding the rule’s event source.",
        items: ["All / any / one / none", "Equals or changes", "Above or below", "Present or absent"],
      },
      {
        title: "Then",
        description: "Run one or more clearly ordered actions.",
        items: ["Show or hide overlay", "Display alert", "Play sound", "Map marker"],
      },
      {
        title: "Timing & Repeat",
        description: "Control delays, action order, duplicate suppression, refire timing, and what happens to actions already waiting.",
        items: ["Delay", "Sequential or together", "Cooldown / refire", "Keep or interrupt queued actions"],
      },
      {
        title: "State & Variables",
        description: "Reuse values, counters, timers, lists, and rule or folder state when a simple rule is not enough.",
        items: ["Named values", "Counters and timers", "Enable or disable another rule", "Reset scope"],
      },
      {
        title: "Test & Review",
        description: "Read the complete rule, run it against a chosen event, and inspect every decision before enabling it.",
        items: ["Plain-language summary", "Event evidence", "Condition trace", "Action preview and history"],
      },
      {
        title: "Advanced",
        description: "Reveal regex, raw IDs, expressions, loops, and precise multi-action control only when needed.",
        items: ["Hidden by default", "Triggernometry-style power", "Safe capability limits", "Summary always stays visible"],
      },
    ],
  },
  "event-inspector": {
    eyebrow: "LIVE EVENT DISCOVERY",
    title: "Event Inspector",
    description:
      "Find the event you need without keeping an unlimited packet log. Follow the live stream, freeze a useful moment, inspect its decoded structure, and turn selected fields into a rule.",
    items: [
      {
        title: "Live Stream",
        description: "Follow privacy-reviewed events with a fixed memory budget and visible overflow counters during heavy bursts.",
        items: ["Live or frozen review", "Direction and route", "Timestamp", "Rate and dropped-display counters"],
      },
      {
        title: "Filters",
        description: "Reduce noise before rendering or decoding details.",
        items: ["Event type", "Source or target", "Skill or effect", "Scene", "Changed fields only"],
      },
      {
        title: "Decoded Details",
        description: "Expand the selected message as a field tree with names, protobuf tags, wire types, values, and localized labels where proven.",
        items: ["Canonical event", "Protocol route", "Protobuf field tree", "Exact decimal and hex values"],
      },
      {
        title: "Pin & Compare",
        description: "Keep a small explicit set of events outside the rolling view and compare what changed between them.",
        items: ["Pinned events", "Field diff", "Before and after", "Clear all pins"],
      },
      {
        title: "Create Trigger",
        description: "Use the selected event kind or field path to prefill a readable When clause, then continue in Rules.",
        items: ["Use event", "Use selected field", "Choose comparison", "Open rule builder"],
      },
      {
        title: "Memory & Recording",
        description: "Keep inspection bounded by bytes and time; recording is off by default and must be explicitly started with a size limit.",
        items: ["Memory budget", "Retention window", "Display sampling", "Explicit bounded recording"],
      },
      {
        title: "Privacy Boundary",
        description: "Authentication, account secrets, private chat, and unreviewed sensitive fields are never available to this workspace.",
        items: ["Local only", "Sensitive routes blocked", "No automatic uploads", "No raw access for shared packs"],
      },
    ],
  },
  library: {
    eyebrow: "REUSABLE CONTENT",
    title: "Library",
    description:
      "Browse reviewed and imported content without mixing the library with your enabled personal rules.",
    items: [
      {
        title: "Encounter Packs",
        description: "Scene-scoped mechanics, phases, hazards, assignments, role filters, and callout rules.",
      },
      {
        title: "Timelines",
        description: "Upcoming encounter events, countdowns, and emphasis levels that can drive visual, sound, and map actions.",
      },
      {
        title: "Class & Specialization Packs",
        description: "Skill, resource, effect, rotation, and cooldown-oriented starting points.",
      },
      {
        title: "Imports & Utility",
        description: "Shared trigger packs, Setup Profile dependencies, party tools, and non-combat notifications with a permission preview.",
      },
      {
        title: "Installed & Updates",
        description: "See pack authors, versions, dependencies, enabled folders, update notes, and available updates.",
      },
    ],
  },
  settings: {
    eyebrow: "WORKSPACE OPTIONS",
    title: "Custom Trigger Settings",
    description:
      "Global rule behavior and safety controls. Display styling remains in the destination workspace.",
    items: [
      {
        title: "Editor Mode",
        description: "Keep Guided mode as the default, or always show Advanced rule controls.",
      },
      {
        title: "Rule Execution",
        description: "Enablement, action ordering, duplicate suppression, and failure behavior.",
      },
      {
        title: "Testing",
        description: "Choose recorded or sample events, keep test actions visibly simulated, and retain a readable decision history.",
      },
      {
        title: "Event Inspector",
        description: "Set bounded live retention, display sampling, selected-event decode limits, and explicit recording limits.",
      },
      {
        title: "Import & Export",
        description: "Share versioned trigger packs or include them in a Setup Profile without silently enabling rules.",
      },
      {
        title: "Safety & Permissions",
        description: "Review every pack capability. Shared packs cannot run arbitrary code, launch programs, write files, or use undeclared network access.",
      },
      {
        title: "Localization",
        description: "Localize rule-builder labels, templates, alerts, and action descriptions.",
      },
    ],
  },
};

export function mountCustomTriggersWorkspaceSurface(
  container: HTMLElement,
  page: CustomTriggersWorkspacePage,
): MountedSurface {
  const definition = PAGE_DEFINITIONS[page];
  const root = element("div", "plugin-surface overlay-workspace-surface custom-triggers-workspace-surface");

  const header = element("section", "content-card overlay-workspace-intro");
  const heading = element("div", "overlay-workspace-heading");
  heading.append(
    text("span", definition.eyebrow, "eyebrow"),
    text("h2", definition.title),
    text("p", definition.description, "card-copy"),
  );
  header.append(heading, text("span", "MENU PREVIEW", "overlay-menu-preview-badge"));

  const menu = element("section", "overlay-menu-grid");
  for (const item of definition.items) {
    const card = element("article", "content-card overlay-menu-card");
    const cardHeader = element("div", "overlay-menu-card-heading");
    cardHeader.append(text("h3", item.title), text("span", "›", "overlay-menu-chevron"));
    card.append(cardHeader, text("p", item.description, "card-copy"));
    if (item.items !== undefined) {
      const children = element("ul", "overlay-menu-children");
      for (const child of item.items) children.append(text("li", child));
      card.append(children);
    }
    menu.append(card);
  }

  const note = element("section", "overlay-menu-note");
  note.append(
    text("strong", "Navigation only"),
    text("span", "No rules subscribe to events or execute actions in this menu preview."),
  );
  root.append(header, menu, note);
  container.replaceChildren(root);
  return { dispose: () => root.remove() };
}

function element<K extends keyof HTMLElementTagNameMap>(
  tag: K,
  className?: string,
): HTMLElementTagNameMap[K] {
  const node = document.createElement(tag);
  if (className !== undefined) node.className = className;
  return node;
}

function text<K extends keyof HTMLElementTagNameMap>(
  tag: K,
  value: string,
  className?: string,
): HTMLElementTagNameMap[K] {
  const node = element(tag, className);
  node.textContent = value;
  return node;
}
