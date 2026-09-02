import type { MountedSurface } from "../shell/types";
import {
  fightAttributeComponentLabel,
  formatFightAttributeValue,
  resolveLiveCharacterStatFamilies,
  type FightAttributePresentationCatalog,
  type LiveCharacterStatsSnapshot,
} from "./live-character-stats";

export interface OverlayStatsTrackerDependencies {
  loadCatalog(): Promise<FightAttributePresentationCatalog>;
  loadSnapshot(): Promise<LiveCharacterStatsSnapshot>;
  waitForSnapshot(afterRevision: number): Promise<LiveCharacterStatsSnapshot>;
}

export function mountOverlayStatsTrackerSurface(
  container: HTMLElement,
  dependencies: OverlayStatsTrackerDependencies,
): MountedSurface {
  let alive = true;
  let catalog: FightAttributePresentationCatalog | null = null;
  let snapshot: LiveCharacterStatsSnapshot | null = null;
  let searchValue = "";

  const root = element("div", "plugin-surface overlay-workspace-surface overlay-stats-surface");
  const header = element("section", "content-card overlay-workspace-intro");
  const heading = element("div", "overlay-workspace-heading");
  heading.append(
    text("span", "LIVE INFORMATION", "eyebrow"),
    text("h2", "Combat Stats"),
    text(
      "p",
      "See the latest complete local snapshot and temporary packet-observed changes without publishing combat-only values to your profile.",
      "card-copy",
    ),
  );
  const state = text("span", "CONNECTING", "overlay-menu-preview-badge");
  header.append(heading, state);

  const statsCard = element("section", "content-card overlay-stats-card");
  const statsHeading = element("header", "overlay-stats-heading");
  const statsCopy = element("div");
  statsCopy.append(
    text("span", "LOCAL CHARACTER", "eyebrow"),
    text("h3", "Waiting for a complete character snapshot"),
    text("p", "Open the game and enter a scene that publishes your character attributes.", "card-copy"),
  );
  const search = document.createElement("input");
  search.type = "search";
  search.placeholder = "Filter stats";
  search.setAttribute("aria-label", "Filter combat stats");
  statsHeading.append(statsCopy, search);
  const statsBody = element("div", "overlay-stats-grid");
  statsBody.append(text("p", "No local character-stat snapshot has been observed yet.", "runtime-empty-result"));
  statsCard.append(statsHeading, statsBody);

  const planned = element("section", "overlay-menu-grid");
  for (const [title, description] of [
    ["Skills & Cooldowns", "Equipped skills, charges, cooldowns, recasts, and availability states."],
    ["Effects & Auras", "Buffs, debuffs, durations, stacks, missing effects, and proc states."],
    ["Energy & Gauges", "Class resources, gauges, stacks, and spend-and-gain systems."],
    ["Party & Support", "Party cooldowns, mitigation, support effects, and role-relevant availability."],
  ] as const) {
    const card = element("article", "content-card overlay-menu-card overlay-planned-card");
    card.append(
      text("span", "PLANNED", "eyebrow"),
      text("h3", title),
      text("p", description, "card-copy"),
    );
    planned.append(card);
  }
  root.append(header, statsCard, planned);
  container.replaceChildren(root);

  search.addEventListener("input", () => {
    searchValue = search.value.trim().toLocaleLowerCase();
    render();
  });

  void connect();

  async function connect(): Promise<void> {
    try {
      [catalog, snapshot] = await Promise.all([
        dependencies.loadCatalog(),
        dependencies.loadSnapshot(),
      ]);
      if (!alive) return;
      render();
      while (alive) {
        snapshot = await dependencies.waitForSnapshot(snapshot.revision);
        if (!alive) return;
        render();
      }
    } catch (error) {
      if (!alive) return;
      state.textContent = "UNAVAILABLE";
      state.dataset.state = "error";
      statsBody.replaceChildren(
        text("p", error instanceof Error ? error.message : String(error), "runtime-empty-result"),
      );
    }
  }

  function render(): void {
    if (catalog === null || snapshot === null) return;
    const families = resolveLiveCharacterStatFamilies(snapshot, catalog).filter(
      (family) =>
        searchValue === "" ||
        family.name.toLocaleLowerCase().includes(searchValue) ||
        family.description?.toLocaleLowerCase().includes(searchValue),
    );
    state.textContent = snapshot.character === null ? "WAITING" : "LIVE LOCAL";
    state.dataset.state = snapshot.character === null ? "waiting" : "live";
    const changed = families.filter((family) => family.changed).length;
    statsCopy.querySelector("h3")!.textContent = snapshot.character === null
      ? "Waiting for a complete character snapshot"
      : `${families.length.toLocaleString()} current stat families`;
    statsCopy.querySelector("p")!.textContent = snapshot.character === null
      ? "Open the game and enter a scene that publishes your character attributes."
      : `${changed.toLocaleString()} temporarily changed · exact ${catalog.locale} catalog for game build ${catalog.game_build}`;
    statsBody.replaceChildren();
    if (families.length === 0) {
      statsBody.append(text(
        "p",
        searchValue === ""
          ? "The current snapshot contains no nonzero displayable stats."
          : "No current stats match this filter.",
        "runtime-empty-result",
      ));
      return;
    }
    for (const family of families) {
      const primary = family.components.find(
        (component) => component.presentation.component === "final",
      ) ?? family.components[0]!;
      const card = element("article", "overlay-stat-family");
      card.dataset.changed = String(family.changed);
      const cardHeading = element("div", "overlay-stat-family-heading");
      const copy = element("div");
      copy.append(
        text("strong", family.name),
        text("span", family.description ?? "Observed combat attribute"),
      );
      const value = text(
        "strong",
        formatFightAttributeValue(
          primary.currentValue,
          primary.presentation.number_type,
          primary.presentation.format_type,
        ),
        "overlay-stat-current-value",
      );
      cardHeading.append(copy, value);
      card.append(cardHeading);
      if (family.changed) {
        const prior = primary.snapshotValue === null
          ? "Snapshot unavailable"
          : `Snapshot ${formatFightAttributeValue(
              primary.snapshotValue,
              primary.presentation.number_type,
              primary.presentation.format_type,
            )}`;
        card.append(text("span", prior, "overlay-stat-change"));
      }
      const details = element("details", "overlay-stat-breakdown");
      details.append(text("summary", "Exact breakdown"));
      const rows = element("dl", "overlay-stat-component-list");
      for (const component of family.components) {
        rows.append(
          text("dt", fightAttributeComponentLabel(component.presentation.component)),
          text(
            "dd",
            formatFightAttributeValue(
              component.currentValue,
              component.presentation.number_type,
              component.presentation.format_type,
            ),
          ),
        );
      }
      details.append(rows);
      card.append(details);
      statsBody.append(card);
    }
  }

  return {
    dispose() {
      alive = false;
      root.remove();
    },
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
