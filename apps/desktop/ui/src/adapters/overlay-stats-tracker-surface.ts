import type { MountedSurface } from "../shell/types";
import {
  fightAttributeComponentLabel,
  formatFightAttributeValue,
  resolveLiveCharacterStatFamilies,
  type FightAttributePresentationCatalog,
  type LiveCharacterStatFamilyView,
  type LiveCharacterStatsSnapshot,
} from "./live-character-stats";

export interface OverlayStatsTrackerDependencies {
  loadCatalog(): Promise<FightAttributePresentationCatalog>;
  loadSnapshot(): Promise<LiveCharacterStatsSnapshot>;
  waitForSnapshot(afterRevision: number): Promise<LiveCharacterStatsSnapshot>;
}

// Exact-build Fight Attribute families used by the compact in-game profile
// summary. Percentage families are intentionally selected for the rate stats;
// their raw rating families remain available in the complete observed view.
export const MAIN_CHARACTER_STAT_FAMILY_IDS = [
  11_320, // Max HP
  11_330, // ATK
  11_030, // Agility
  11_040, // Endurance
  11_440, // Illusion-Breaking Strength
  11_710, // Crit %
  11_930, // Haste %
  11_780, // Luck %
  11_940, // Mastery %
  11_950, // Versatility %
  11_970, // Block %
] as const;

export function selectMainCharacterStatFamilies(
  families: readonly LiveCharacterStatFamilyView[],
  catalog: FightAttributePresentationCatalog,
): LiveCharacterStatFamilyView[] {
  const byId = new Map(families.map((family) => [family.familyId, family]));
  const catalogByFamily = new Map<number, FightAttributePresentationCatalog["attributes"][number]>();
  for (const attribute of catalog.attributes) {
    if (!attribute.displayable) continue;
    const previous = catalogByFamily.get(attribute.family_id);
    if (previous === undefined || attribute.component === "final") {
      catalogByFamily.set(attribute.family_id, attribute);
    }
  }
  return MAIN_CHARACTER_STAT_FAMILY_IDS
    .map((familyId) => {
      const observed = byId.get(familyId);
      if (observed !== undefined) return observed;
      const presentation = catalogByFamily.get(familyId);
      return presentation === undefined
        ? undefined
        : {
            familyId,
            name: presentation.name,
            description: presentation.description,
            changed: false,
            components: [],
          };
    })
    .filter((family): family is LiveCharacterStatFamilyView =>
      family !== undefined);
}

export function mountOverlayStatsTrackerSurface(
  container: HTMLElement,
  dependencies: OverlayStatsTrackerDependencies,
): MountedSurface {
  let alive = true;
  let catalog: FightAttributePresentationCatalog | null = null;
  let snapshot: LiveCharacterStatsSnapshot | null = null;
  let searchValue = "";
  let showAllStats = false;

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
  search.hidden = true;
  const viewAll = element("button", "secondary-button overlay-stats-view-all");
  viewAll.type = "button";
  viewAll.textContent = "View all observed stats";
  const headingActions = element("div", "overlay-stats-heading-actions");
  headingActions.append(search, viewAll);
  statsHeading.append(statsCopy, headingActions);
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
  viewAll.addEventListener("click", () => {
    showAllStats = !showAllStats;
    search.hidden = !showAllStats;
    viewAll.textContent = showAllStats ? "Hide observed stats" : "View all observed stats";
    if (!showAllStats) {
      search.value = "";
      searchValue = "";
    }
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
    const observedFamilies = resolveLiveCharacterStatFamilies(snapshot, catalog);
    const mainFamilies = selectMainCharacterStatFamilies(observedFamilies, catalog);
    const families = observedFamilies.filter(
      (family) =>
        searchValue === "" ||
        family.name.toLocaleLowerCase().includes(searchValue) ||
        family.description?.toLocaleLowerCase().includes(searchValue),
    );
    state.textContent = snapshot.character === null ? "WAITING" : "LIVE LOCAL";
    state.dataset.state = snapshot.character === null ? "waiting" : "live";
    const changed = observedFamilies.filter((family) => family.changed).length;
    const observedMainCount = mainFamilies.filter((family) => family.components.length > 0).length;
    statsCopy.querySelector("h3")!.textContent = snapshot.character === null
      ? "Waiting for a complete character snapshot"
      : observedMainCount === mainFamilies.length
        ? `${mainFamilies.length.toLocaleString()} main stats`
        : `${observedMainCount.toLocaleString()} of ${mainFamilies.length.toLocaleString()} main stats observed`;
    statsCopy.querySelector("p")!.textContent = snapshot.character === null
      ? "Open the game and enter a scene that publishes your character attributes."
      : `${observedFamilies.length.toLocaleString()} observed stat families · ${changed.toLocaleString()} temporarily changed`;
    statsBody.replaceChildren();
    if (mainFamilies.length === 0) {
      statsBody.append(text(
        "p",
        "The current snapshot does not contain the main character-stat families yet.",
        "runtime-empty-result",
      ));
      return;
    }
    const main = element("section", "overlay-main-stats");
    main.append(text("h4", "Main stats", "overlay-stats-section-title"));
    const mainGrid = element("div", "overlay-main-stats-grid");
    for (const family of mainFamilies) {
      const primary = family.components.find(
        (component) => component.presentation.component === "final",
      ) ?? family.components[0] ?? null;
      const row = element("article", "overlay-main-stat-row");
      row.dataset.changed = String(family.changed);
      row.dataset.observed = String(primary !== null);
      row.append(
        text("span", family.name, "overlay-main-stat-name"),
        text(
          "strong",
          primary === null
            ? "Not observed"
            : formatFightAttributeValue(
                primary.currentValue,
                primary.presentation.number_type,
                primary.presentation.format_type,
              ),
          "overlay-main-stat-value",
        ),
      );
      mainGrid.append(row);
    }
    main.append(mainGrid);
    statsBody.append(main);
    if (!showAllStats) return;

    const detail = element("section", "overlay-observed-stats");
    detail.append(text("h4", "All observed stats", "overlay-stats-section-title"));
    const detailGrid = element("div", "overlay-observed-stats-grid");
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
      detailGrid.append(card);
    }
    if (families.length === 0) {
      detailGrid.append(text("p", "No observed stats match this filter.", "runtime-empty-result"));
    }
    detail.append(detailGrid);
    statsBody.append(detail);
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
