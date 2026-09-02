import type { MountedSurface } from "../shell/types";
import { requestWorkspaceNavigation } from "../shell/workspace-navigation";

const OVERLAY_WORKSPACE_ID = "app.rlogs.overlay";
const CUSTOM_TRIGGERS_WORKSPACE_ID = "app.rlogs.custom-triggers";

export type OverlayWorkspacePage =
  | "overview"
  | "setups"
  | "editor"
  | "trackers"
  | "mechanics-map"
  | "settings";

interface MenuItem {
  title: string;
  description: string;
  items?: readonly string[];
  destination?: {
    workspaceId: string;
    entrypoint: string;
  };
}

interface PageDefinition {
  eyebrow: string;
  title: string;
  description: string;
  items: readonly MenuItem[];
}

const PAGE_DEFINITIONS: Record<OverlayWorkspacePage, PageDefinition> = {
  overview: {
    eyebrow: "OVERLAY WORKSPACE",
    title: "One place for every on-screen tool",
    description:
      "Design what appears over the game, decide when it appears, and keep combat tools separate from the Combat Meter overlay.",
    items: [
      {
        title: "Setups",
        description: "Use a ready-made setup or share a complete configuration with someone else.",
        items: ["My setups", "Browse", "Import", "Share"],
        destination: destination(OVERLAY_WORKSPACE_ID, "setups"),
      },
      {
        title: "Trackers",
        description: "Group the information that changes while you play.",
        items: ["Combat stats", "Skills & cooldowns", "Energy & resources"],
        destination: destination(OVERLAY_WORKSPACE_ID, "trackers"),
      },
      {
        title: "Mechanics Map",
        description: "Configure the game-like map and encounter guidance layer.",
        items: ["Live map", "Encounter guides", "Markers"],
        destination: destination(OVERLAY_WORKSPACE_ID, "mechanics-map"),
      },
      {
        title: "Automation Connections",
        description: "Choose which layouts, widgets, alerts, and map markers Custom Triggers may control.",
        items: ["Show or hide", "Display an alert", "Map markers"],
        destination: destination(CUSTOM_TRIGGERS_WORKSPACE_ID, "overview"),
      },
    ],
  },
  setups: {
    eyebrow: "QUICK SETUP & SHARING",
    title: "Setups",
    description:
      "A Setup Profile can carry selected layouts, trackers, map preferences, sounds, and linked trigger packs.",
    items: [
      {
        title: "My Setups",
        description: "Switch between installed setups for raids, dungeons, roles, or personal preferences.",
      },
      {
        title: "Browse Setups",
        description: "Find shared Setup Profiles and preview exactly what each one includes.",
      },
      {
        title: "Import",
        description: "Review layouts, trackers, sounds, maps, and triggers before choosing Use this setup.",
      },
      {
        title: "Share",
        description: "Select what to include, remove personal data, and publish or export the Setup Profile.",
      },
    ],
  },
  editor: {
    eyebrow: "VISUAL EDITOR",
    title: "Editor",
    description:
      "Adjust the selected setup visually. Basic controls appear first; precise positioning and rule-linked behavior stay under Advanced.",
    items: [
      {
        title: "Canvas",
        description: "Move, resize, align, layer, lock, and preview overlay windows and groups.",
      },
      {
        title: "Widget Library",
        description: "Add stats, trackers, alerts, timers, counters, map elements, and future widgets.",
      },
      {
        title: "Display Groups",
        description: "Move and style related icons, bars, text, gauges, and alerts together.",
      },
      {
        title: "Visibility",
        description: "Choose simple combat and scene behavior, or open Advanced for trigger-controlled visibility.",
      },
    ],
  },
  trackers: {
    eyebrow: "LIVE INFORMATION",
    title: "Trackers",
    description:
      "Keep related live information together without turning each tracker into another top-level workspace tab.",
    items: [
      {
        title: "Combat Stats",
        description: "Current values, permanent snapshot values, and temporary combat changes.",
      },
      {
        title: "Skills & Cooldowns",
        description: "Equipped skills, charges, cooldowns, recasts, and availability states.",
      },
      {
        title: "Effects & Auras",
        description: "Buffs, debuffs, durations, stacks, missing effects, and proc states.",
      },
      {
        title: "Energy & Gauges",
        description: "Class resources, gauges, stacks, and other spend-and-gain systems.",
      },
      {
        title: "Party & Support",
        description: "Party cooldowns, mitigation, support effects, and role-relevant availability.",
      },
      {
        title: "Encounter Timeline",
        description: "Upcoming mechanics, phase changes, priority warnings, and countdown bars supplied by encounter packs.",
      },
      {
        title: "Tracker Groups",
        description: "Reusable icon, bar, text, and gauge groups that can be added to any layout.",
      },
    ],
  },
  "mechanics-map": {
    eyebrow: "SPATIAL OVERLAY",
    title: "Mechanics Map",
    description:
      "Keep the familiar feel of the in-game map while adding encounter information and configurable guidance.",
    items: [
      {
        title: "Live Map",
        description: "The map view, player orientation, party positions, and useful game markers.",
      },
      {
        title: "Encounter Guides",
        description: "Boss phases, mechanic regions, safe areas, paths, role guidance, and timing context.",
      },
      {
        title: "Markers",
        description: "Built-in, encounter, party, and personal markers with clear ownership.",
      },
      {
        title: "Mechanic Layers",
        description: "Show only the hazards, targets, routes, ranges, and instructions relevant to the current role.",
      },
      {
        title: "Map Appearance",
        description: "Scale, rotation, opacity, labels, layers, and game-like visual styling.",
      },
    ],
  },
  settings: {
    eyebrow: "WORKSPACE OPTIONS",
    title: "Overlay Settings",
    description:
      "Global behavior for the Overlay workspace. Individual widget styling stays with the layout that owns it.",
    items: [
      {
        title: "General",
        description: "Enablement, startup behavior, monitor selection, and overlay interaction mode.",
      },
      {
        title: "Performance",
        description: "Refresh pacing, rendering limits, and hardware acceleration controls.",
      },
      {
        title: "Hotkeys",
        description: "Show, hide, interact with, and switch overlay presets.",
      },
      {
        title: "Accessibility",
        description: "Text size, contrast, motion, sound, and color-independent indicators.",
      },
      {
        title: "Localization",
        description: "Language and locale behavior shared by this workspace and its future additions.",
        items: [
          "Deutsch",
          "English",
          "Español",
          "Français",
          "Bahasa Indonesia",
          "日本語",
          "한국어",
          "Português",
          "ไทย",
          "简体中文",
          "繁體中文",
        ],
      },
    ],
  },
};

export function mountOverlayWorkspaceSurface(
  container: HTMLElement,
  page: OverlayWorkspacePage,
): MountedSurface {
  const definition = PAGE_DEFINITIONS[page];
  const root = element("div", "plugin-surface overlay-workspace-surface");

  const header = element("section", "content-card overlay-workspace-intro");
  const heading = element("div", "overlay-workspace-heading");
  heading.append(
    text("span", definition.eyebrow, "eyebrow"),
    text("h2", definition.title),
    text("p", definition.description, "card-copy"),
  );
  const status = text("span", "DESIGN PREVIEW", "overlay-menu-preview-badge");
  header.append(heading, status);

  const menu = element("section", "overlay-menu-grid");
  for (const item of definition.items) {
    const destination = item.destination;
    const card = destination === undefined
      ? element("article", "content-card overlay-menu-card")
      : element("button", "content-card overlay-menu-card overlay-menu-card-action");
    if (card instanceof HTMLButtonElement) {
      card.type = "button";
      card.setAttribute("aria-label", `Open ${item.title}`);
      card.addEventListener("click", () => {
        requestWorkspaceNavigation(destination!);
      });
    }
    const cardHeader = element("div", "overlay-menu-card-heading");
    cardHeader.append(text("h3", item.title));
    if (destination !== undefined) {
      cardHeader.append(text("span", "›", "overlay-menu-chevron"));
    }
    card.append(cardHeader, text("p", item.description, "card-copy"));
    if (item.items !== undefined) {
      const children = element("ul", "overlay-menu-children");
      for (const child of item.items) {
        children.append(text("li", child));
      }
      card.append(children);
    }
    menu.append(card);
  }

  const note = element("section", "overlay-menu-note");
  note.append(
    text("strong", "Layout first"),
    text(
      "span",
      "These planned overlay sections do not read packets or render over the game yet. Event inspection stays isolated in Custom Triggers.",
    ),
  );

  root.append(header, menu, note);
  container.replaceChildren(root);
  return {
    dispose() {
      root.remove();
    },
  };
}

function destination(workspaceId: string, page: string) {
  return {
    workspaceId,
    entrypoint: `builtin://${workspaceId}/${page}`,
  };
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
