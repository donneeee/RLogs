import type { MountedSurface } from "../shell/types";
import {
  type CombatActorSortKey,
  type CombatTimelineSnapshot,
  actorLabel,
  sortCombatActors,
} from "./combat-meter";
import { describeRdpsStatus } from "./rdps-status";

const NUMBER_FORMAT = new Intl.NumberFormat(undefined, {
  maximumFractionDigits: 1,
});

const INTEGER_FORMAT = new Intl.NumberFormat(undefined, {
  maximumFractionDigits: 0,
});

const SORT_COLUMNS: ReadonlyArray<{
  key: CombatActorSortKey;
  label: string;
  className: string;
}> = [
  { key: "run_dps", label: "DPS", className: "meter-number" },
  { key: "encounter_dps", label: "eDPS", className: "meter-number" },
  { key: "active_dps", label: "aDPS", className: "meter-number" },
  { key: "hps", label: "HPS", className: "meter-number" },
  { key: "tps", label: "TPS", className: "meter-number" },
  { key: "rdps_damage", label: "rDMG", className: "meter-number" },
  { key: "rdps", label: "rDPS", className: "meter-number" },
  {
    key: "rdps_contribution_given",
    label: "rDMG granted",
    className: "meter-number",
  },
  {
    key: "rdps_contribution_received",
    label: "rDMG received",
    className: "meter-number",
  },
  {
    key: "reported_damage",
    label: "Damage",
    className: "meter-number",
  },
  {
    key: "effective_damage",
    label: "Effective",
    className: "meter-number meter-secondary-column",
  },
  {
    key: "effective_healing",
    label: "Healing",
    className: "meter-number meter-secondary-column",
  },
  { key: "deaths", label: "Deaths", className: "meter-number" },
];

export function mountCombatMeterSurface(
  container: HTMLElement,
  loadSnapshot: () => Promise<CombatTimelineSnapshot | null>,
  options: {
    live?: boolean;
    subscribe?: (
      onSnapshot: (snapshot: CombatTimelineSnapshot | null) => void,
      onError: (error: unknown) => void,
    ) => () => void;
  } = {},
): MountedSurface {
  const live = options.live === true;
  let alive = true;
  let busy = false;
  let snapshot: CombatTimelineSnapshot | null = null;
  let renderFrame: number | null = null;
  let sortKey: CombatActorSortKey = "run_dps";
  let sortDirection: "ascending" | "descending" = "descending";

  const root = document.createElement("div");
  root.className = "plugin-surface combat-meter-surface";

  const heading = document.createElement("section");
  heading.className = "content-card runtime-action-card";
  heading.append(
    text("h2", live ? "Live Combat Meter" : "Completed Session Meter"),
    text(
      "p",
      live
        ? "An incremental host-calculated view updated from canonical combat events. Packet decoding and totals stay native; this surface receives only bounded snapshots."
        : "A post-run view backed by the Combat Meter plug-in snapshot. The same contract feeds history drill-downs and the live overlay; the browser only sorts and presents host-calculated totals.",
      "card-copy",
    ),
  );
  const actions = document.createElement("div");
  actions.className = "runtime-card-actions";
  const refresh = button(live ? "Refresh now" : "Refresh session", "primary-button");
  const message = text(
    "span",
    live
      ? "Waiting for the native live event stream."
      : "Waiting for a completed canonical session.",
    "runtime-action-message",
  );
  actions.append(refresh, message);
  heading.append(actions);

  const content = document.createElement("div");
  content.className = "combat-meter-content";
  content.append(
    text(
      "p",
      live
        ? "rLogs is monitoring continuously; combat totals appear as soon as canonical combat events arrive."
        : "Complete a capture or run the sanitized reference replay to populate the meter.",
      "runtime-empty-result",
    ),
  );
  root.append(heading, content);
  container.append(root);

  const render = () => {
    if (snapshot === null) {
      content.replaceChildren(
        text(
          "p",
          live
            ? "No live combat snapshot is available yet. Start the game and confirm a capture interface in Settings."
            : "No completed combat snapshot is available yet. Complete a capture or run the sanitized reference replay.",
          "runtime-empty-result",
        ),
      );
      return;
    }

    const playerCount = snapshot.actors.filter(
      (actor) => actor.actor_kind === "player",
    ).length;
    const activeActors = snapshot.actors.filter(hasMeterActivity);
    const summary = document.createElement("section");
    summary.className = "metric-grid combat-meter-summary";
    summary.append(
      metric("Run clock / DPS", optionalDuration(snapshot.run_elapsed_micros)),
      metric("Encounter / eDPS", optionalDuration(snapshot.encounter_elapsed_micros)),
      metric("Active combat / aDPS", formatDuration(snapshot.active_combat_micros)),
      metric("Players", INTEGER_FORMAT.format(playerCount)),
      metric("Combat windows", INTEGER_FORMAT.format(snapshot.combat_window_count)),
      metric(
        "Data gaps",
        INTEGER_FORMAT.format(snapshot.data_gap_count),
        snapshot.data_gap_count > 0 ? "warning" : "",
      ),
    );

    const metadata = document.createElement("section");
    metadata.className = "content-card combat-meter-metadata";
    metadata.append(
      metadataItem("Session", snapshot.session_id),
      metadataItem("Region", formatRegion(snapshot)),
      metadataItem("Encounter", snapshot.encounter_id ?? "Unclassified"),
      metadataItem(
        "State",
        snapshot.encounter_state ?? (snapshot.closed_at_log_end ? "Log ended" : "Unknown"),
      ),
      metadataItem(
        "rDPS formulas",
        describeRdpsStatus(snapshot.rdps_status).compactLabel,
      ),
    );

    const tableCard = document.createElement("section");
    tableCard.className = "content-card combat-meter-table-card";
    const tableHeading = document.createElement("header");
    tableHeading.className = "card-heading";
    tableHeading.append(
      text("h2", "Combatants"),
      text(
        "span",
        `${activeActors.length.toLocaleString()} active / ${snapshot.actors.length.toLocaleString()} observed`,
      ),
    );
    tableCard.append(tableHeading);

    if (activeActors.length === 0) {
      tableCard.append(
        text(
          "p",
          "The session contains no damage, healing, casts, hits, shielding, or deaths.",
          "runtime-empty-result",
        ),
      );
    } else {
      const scroller = document.createElement("div");
      scroller.className = "meter-table-scroll";
      const table = document.createElement("table");
      table.className = "meter-table";
      const tableHeader = document.createElement("thead");
      const headingRow = document.createElement("tr");
      headingRow.append(text("th", "Combatant"));
      for (const column of SORT_COLUMNS) {
        const cell = document.createElement("th");
        cell.className = column.className;
        cell.setAttribute(
          "aria-sort",
          sortKey === column.key ? sortDirection : "none",
        );
        const sort = button(
          `${column.label}${sortKey === column.key ? (sortDirection === "descending" ? " ↓" : " ↑") : ""}`,
          "meter-sort-button",
        );
        sort.type = "button";
        sort.dataset.sortKey = column.key;
        cell.append(sort);
        headingRow.append(cell);
      }
      tableHeader.append(headingRow);

      const body = document.createElement("tbody");
      for (const actor of sortCombatActors(
        activeActors,
        sortKey,
        sortDirection,
      )) {
        const row = document.createElement("tr");
        const identity = document.createElement("td");
        identity.className = "meter-actor";
        identity.append(
          text("strong", actorLabel(actor)),
          text(
            "span",
            `${formatActorKind(actor.actor_kind)} · ID ${actor.actor_id}`,
          ),
        );
        identity.title = `Entity UUID ${actor.entity_uuid}`;
        row.append(identity);
        for (const column of SORT_COLUMNS) {
          row.append(
            numericCell(
              combatMeterActorColumnText(actor, column.key),
              column.className.replace("meter-number", "").trim(),
            ),
          );
        }
        body.append(row);
      }
      table.append(tableHeader, body);
      scroller.append(table);
      tableCard.append(scroller);

      tableHeader.addEventListener("click", (event) => {
        const target = event.target;
        if (!(target instanceof HTMLButtonElement)) return;
        const requested = target.dataset.sortKey as CombatActorSortKey | undefined;
        if (!requested) return;
        if (sortKey === requested) {
          sortDirection =
            sortDirection === "descending" ? "ascending" : "descending";
        } else {
          sortKey = requested;
          sortDirection = "descending";
        }
        render();
      });
    }

    content.replaceChildren(summary, metadata, tableCard);
  };

  const load = async () => {
    if (busy) return;
    busy = true;
    refresh.disabled = true;
    message.classList.remove("error");
    message.textContent = live
      ? "Reading the latest native snapshot..."
      : "Loading the latest validated snapshot...";
    try {
      snapshot = await loadSnapshot();
      if (!alive) return;
      render();
      message.textContent =
        snapshot === null
          ? live
            ? "No live session is available."
            : "No completed session is available."
          : `${snapshot.actors.length.toLocaleString()} actor(s) · schema ${snapshot.schema_version}`;
    } catch (error) {
      if (!alive) return;
      snapshot = null;
      render();
      message.textContent =
        error instanceof Error ? error.message : "Could not load Combat Meter.";
      message.classList.add("error");
    } finally {
      busy = false;
      refresh.disabled = false;
    }
  };

  const acceptLiveSnapshot = (next: CombatTimelineSnapshot | null) => {
    snapshot = next;
    if (renderFrame !== null) return;
    renderFrame = window.requestAnimationFrame(() => {
      renderFrame = null;
      if (!alive) return;
      render();
      message.classList.remove("error");
      message.textContent =
        snapshot === null
          ? "No live session is available."
          : `${snapshot.actors.length.toLocaleString()} active actor(s) · event ${snapshot.event_count.toLocaleString()}`;
    });
  };

  refresh.addEventListener("click", () => void load());
  void load();
  const unsubscribe =
    live && options.subscribe
      ? options.subscribe(acceptLiveSnapshot, (error) => {
          if (!alive) return;
          message.textContent =
            error instanceof Error ? error.message : "The live event feed stopped.";
          message.classList.add("error");
        })
      : null;

  return {
    dispose() {
      alive = false;
      unsubscribe?.();
      if (renderFrame !== null) {
        window.cancelAnimationFrame(renderFrame);
      }
    },
  };
}

export function combatMeterActorColumnText(
  actor: CombatTimelineSnapshot["actors"][number],
  key: CombatActorSortKey,
): string {
  switch (key) {
    case "run_dps":
    case "encounter_dps":
    case "active_dps":
    case "hps":
    case "tps":
    case "rdps": {
      const value = actor[key];
      return value === null ? "—" : NUMBER_FORMAT.format(value);
    }
    case "rdps_damage":
    case "rdps_contribution_given":
    case "rdps_contribution_received": {
      const value = actor[key];
      return value === null ? "—" : INTEGER_FORMAT.format(value);
    }
    case "reported_damage":
    case "effective_damage":
    case "effective_healing":
    case "deaths":
      return INTEGER_FORMAT.format(actor[key]);
  }
}

function hasMeterActivity(actor: CombatTimelineSnapshot["actors"][number]): boolean {
  return (
    actor.reported_damage !== 0 ||
    actor.reported_healing !== 0 ||
    actor.effective_healing !== 0 ||
    actor.overheal !== 0 ||
    actor.shielding !== 0 ||
    actor.casts !== 0 ||
    actor.hits !== 0 ||
    actor.deaths !== 0
  );
}

function formatDuration(micros: number): string {
  const totalMillis = Math.floor(micros / 1_000);
  const minutes = Math.floor(totalMillis / 60_000);
  const seconds = Math.floor((totalMillis % 60_000) / 1_000);
  const milliseconds = totalMillis % 1_000;
  return `${minutes}:${seconds.toString().padStart(2, "0")}.${milliseconds
    .toString()
    .padStart(3, "0")}`;
}

function optionalDuration(micros: number | null): string {
  return micros === null ? "—" : formatDuration(micros);
}

function formatRegion(snapshot: CombatTimelineSnapshot): string {
  return snapshot.world_id
    ? `${snapshot.region_id} / ${snapshot.world_id}`
    : snapshot.region_id;
}

function formatActorKind(kind: string | null): string {
  if (kind === null) return "Unclassified";
  return kind
    .split("_")
    .map((part) => part.charAt(0).toUpperCase() + part.slice(1))
    .join(" ");
}

function metric(
  label: string,
  value: string,
  modifier: string = "",
): HTMLElement {
  const article = document.createElement("article");
  if (modifier) article.classList.add(modifier);
  article.append(text("span", label), text("strong", value));
  return article;
}

function metadataItem(label: string, value: string): HTMLElement {
  const item = document.createElement("div");
  item.append(text("span", label), text("strong", value));
  return item;
}

function numericCell(value: string, extraClass: string = ""): HTMLTableCellElement {
  const cell = document.createElement("td");
  cell.className = `meter-number ${extraClass}`.trim();
  cell.textContent = value;
  return cell;
}

function button(label: string, className: string): HTMLButtonElement {
  const element = document.createElement("button");
  element.type = "button";
  element.className = className;
  element.textContent = label;
  return element;
}

function text<K extends keyof HTMLElementTagNameMap>(
  tagName: K,
  value: string,
  className: string = "",
): HTMLElementTagNameMap[K] {
  const element = document.createElement(tagName);
  element.textContent = value;
  if (className) element.className = className;
  return element;
}
