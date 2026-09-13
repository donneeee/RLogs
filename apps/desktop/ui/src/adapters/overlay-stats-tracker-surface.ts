import type { MountedSurface } from "../shell/types";
import type { UiLocalizer } from "../localization/ui-locale";
import {
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
  localizer: UiLocalizer,
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
    text("span", localizer.t("ui.overlay_stats.eyebrow"), "eyebrow"),
    text("h2", localizer.t("ui.overlay_stats.title")),
    text(
      "p",
      localizer.t("ui.overlay_stats.description"),
      "card-copy",
    ),
  );
  const state = text("span", localizer.t("ui.overlay_stats.status.connecting"), "overlay-menu-preview-badge");
  header.append(heading, state);

  const statsCard = element("section", "content-card overlay-stats-card");
  const statsHeading = element("header", "overlay-stats-heading");
  const statsCopy = element("div");
  statsCopy.append(
    text("span", localizer.t("ui.overlay_stats.local_character"), "eyebrow"),
    text("h3", localizer.t("ui.overlay_stats.snapshot.waiting")),
    text("p", localizer.t("ui.overlay_stats.snapshot.help"), "card-copy"),
  );
  const search = document.createElement("input");
  search.type = "search";
  search.placeholder = localizer.t("ui.overlay_stats.filter.placeholder");
  search.setAttribute("aria-label", localizer.t("ui.overlay_stats.filter.aria"));
  search.hidden = true;
  const viewAll = element("button", "secondary-button overlay-stats-view-all");
  viewAll.type = "button";
  viewAll.textContent = localizer.t("ui.overlay_stats.view_all");
  const headingActions = element("div", "overlay-stats-heading-actions");
  headingActions.append(search, viewAll);
  statsHeading.append(statsCopy, headingActions);
  const statsBody = element("div", "overlay-stats-grid");
  statsBody.append(text("p", localizer.t("ui.overlay_stats.snapshot.empty"), "runtime-empty-result"));
  statsCard.append(statsHeading, statsBody);

  const planned = element("section", "overlay-menu-grid");
  for (const [titleKey, descriptionKey] of [
    ["skills", "skills"], ["effects", "effects"], ["energy", "energy"], ["party", "party"],
  ] as const) {
    const card = element("article", "content-card overlay-menu-card overlay-planned-card");
    card.append(
      text("span", localizer.t("ui.overlay_stats.planned.eyebrow"), "eyebrow"),
      text("h3", localizer.t(`ui.overlay_stats.planned.${titleKey}.title`)),
      text("p", localizer.t(`ui.overlay_stats.planned.${descriptionKey}.description`), "card-copy"),
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
    viewAll.textContent = localizer.t(showAllStats ? "ui.overlay_stats.hide_all" : "ui.overlay_stats.view_all");
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
      state.textContent = localizer.t("ui.overlay_stats.status.unavailable");
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
    state.textContent = localizer.t(snapshot.character === null
      ? "ui.overlay_stats.status.waiting"
      : "ui.overlay_stats.status.live_local");
    state.dataset.state = snapshot.character === null ? "waiting" : "live";
    const changed = observedFamilies.filter((family) => family.changed).length;
    const observedMainCount = mainFamilies.filter((family) => family.components.length > 0).length;
    statsCopy.querySelector("h3")!.textContent = snapshot.character === null
      ? localizer.t("ui.overlay_stats.snapshot.waiting")
      : observedMainCount === mainFamilies.length
        ? localizer.t("ui.overlay_stats.snapshot.main_complete", { count: localizer.formatNumber(mainFamilies.length) })
        : localizer.t("ui.overlay_stats.snapshot.main_partial", {
            observed: localizer.formatNumber(observedMainCount), total: localizer.formatNumber(mainFamilies.length),
          });
    statsCopy.querySelector("p")!.textContent = snapshot.character === null
      ? localizer.t("ui.overlay_stats.snapshot.help")
      : localizer.t("ui.overlay_stats.snapshot.summary", {
          families: localizer.formatNumber(observedFamilies.length), changed: localizer.formatNumber(changed),
        });
    statsBody.replaceChildren();
    if (mainFamilies.length === 0) {
      statsBody.append(text(
        "p",
        localizer.t("ui.overlay_stats.snapshot.no_main"),
        "runtime-empty-result",
      ));
      return;
    }
    const main = element("section", "overlay-main-stats");
    main.append(text("h4", localizer.t("ui.overlay_stats.main.title"), "overlay-stats-section-title"));
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
            ? localizer.t("ui.overlay_stats.value.not_observed")
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
    detail.append(text("h4", localizer.t("ui.overlay_stats.all.title"), "overlay-stats-section-title"));
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
        text("span", family.description ?? localizer.t("ui.overlay_stats.value.description_fallback")),
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
          ? localizer.t("ui.overlay_stats.value.snapshot_unavailable")
          : localizer.t("ui.overlay_stats.value.snapshot", { value: formatFightAttributeValue(
              primary.snapshotValue,
              primary.presentation.number_type,
              primary.presentation.format_type,
            ) });
        card.append(text("span", prior, "overlay-stat-change"));
      }
      const details = element("details", "overlay-stat-breakdown");
      details.append(text("summary", localizer.t("ui.overlay_stats.breakdown.title")));
      const rows = element("dl", "overlay-stat-component-list");
      for (const component of family.components) {
        rows.append(
          text("dt", localizer.t(`ui.overlay_stats.component.${component.presentation.component}`)),
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
      detailGrid.append(text("p", localizer.t("ui.overlay_stats.filter.empty"), "runtime-empty-result"));
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
