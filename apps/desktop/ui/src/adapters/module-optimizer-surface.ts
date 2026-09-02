import type { MountedSurface } from "../shell/types";
import {
  type LocalModuleCharacter,
  type LocalModuleInventory,
  type ModuleCandidate,
  type ModuleSolution,
  type OptimizeRequest,
  type OptimizeResponse,
  type OptimizerCatalog,
  modulePresentation,
  moduleQuality,
  optimizerAssetUrl,
} from "./module-optimizer";

interface OptimizerLoaders {
  loadCatalog(): Promise<OptimizerCatalog>;
  loadInventory(): Promise<LocalModuleInventory>;
  optimize(request: OptimizeRequest): Promise<OptimizeResponse>;
}

interface AttributeControls {
  target: HTMLInputElement;
  exclude: HTMLInputElement;
  minimum: HTMLInputElement;
}

export function mountModuleOptimizerSurface(
  container: HTMLElement,
  loaders: OptimizerLoaders,
): MountedSurface {
  let alive = true;
  let busy = false;
  let catalog: OptimizerCatalog | null = null;
  let inventory: LocalModuleInventory | null = null;
  let selectedCharacter: LocalModuleCharacter | null = null;
  const attributeControls = new Map<number, AttributeControls>();

  const root = element("div", "plugin-surface module-optimizer-surface");
  const heading = element("section", "content-card module-optimizer-hero");
  const headingCopy = element("div", "module-optimizer-hero-copy");
  headingCopy.append(
    text("span", "LOCAL · EXACT GAME CATALOG", "eyebrow"),
    text("h2", "Module Optimizer"),
    text(
      "p",
      "Build the strongest module set from the inventory rLogs observed live. Calculations run locally with the reviewed game formula; no sign-in or upload is required.",
      "section-copy",
    ),
  );
  const refresh = button("Refresh local snapshot", "secondary-button");
  heading.append(headingCopy, refresh);

  const status = text("p", "Loading the local module catalog…", "runtime-action-message");
  const setup = element("div", "module-optimizer-layout");
  const left = element("div", "module-optimizer-controls");
  const right = element("div", "module-optimizer-results");

  const characterCard = element("section", "content-card module-character-card");
  characterCard.append(text("h3", "Character inventory"));
  const characterSelect = document.createElement("select");
  characterSelect.className = "settings-select module-character-select";
  characterSelect.setAttribute("aria-label", "Character module inventory");
  const characterMeta = text("p", "Waiting for a live profile snapshot.", "section-copy");
  const equippedPreview = element("div", "module-equipped-preview");
  characterCard.append(characterSelect, characterMeta, equippedPreview);

  const preferenceCard = element("section", "content-card module-preference-card");
  preferenceCard.append(
    text("h3", "Effect priorities"),
    text(
      "p",
      "Prioritize effects you want, exclude effects you do not want, and optionally require a minimum total Link value.",
      "section-copy",
    ),
  );
  const attributeList = element("div", "module-attribute-list");
  preferenceCard.append(attributeList);

  const searchCard = element("section", "content-card module-search-card");
  searchCard.append(text("h3", "Search settings"));
  const searchGrid = element("div", "module-search-grid");
  const sizeSelect = selectControl("Modules equipped", [
    ["4", "4 modules"],
    ["5", "5 modules"],
  ]);
  const modeSelect = selectControl("Search", [
    ["auto", "Automatic"],
    ["exact", "Exact"],
    ["beam", "Fast beam"],
  ]);
  const resultSelect = selectControl("Recommendations", [
    ["5", "Top 5"],
    ["10", "Top 10"],
    ["20", "Top 20"],
  ]);
  const minimumTotal = numberControl("Minimum total Link", "Optional", 0, 999);
  const requireTarget = checkboxControl(
    "Require at least one prioritized effect",
    true,
  );
  searchGrid.append(
    sizeSelect.wrapper,
    modeSelect.wrapper,
    resultSelect.wrapper,
    minimumTotal.wrapper,
    requireTarget.wrapper,
  );
  const run = button("Find best module sets", "primary-button");
  run.disabled = true;
  const searchStatus = text(
    "span",
    "Choose a character with an observed module inventory.",
    "runtime-action-message",
  );
  const actions = element("div", "runtime-card-actions");
  actions.append(run, searchStatus);
  searchCard.append(searchGrid, actions);

  const empty = element("section", "content-card module-results-empty");
  empty.append(
    text("h3", "Recommendations"),
    text(
      "p",
      "Your equipped set and optimized alternatives will appear here as readable module cards.",
      "runtime-empty-result",
    ),
  );
  right.append(empty);
  left.append(characterCard, preferenceCard, searchCard);
  setup.append(left, right);
  root.append(heading, status, setup);
  container.append(root);

  function setSelectedCharacter(packageId: string): void {
    selectedCharacter =
      inventory?.characters.find((entry) => entry.package_id === packageId) ?? null;
    renderCharacter();
  }

  function renderCharacter(): void {
    equippedPreview.replaceChildren();
    if (selectedCharacter === null) {
      characterMeta.textContent =
        "No local BPSR character profile has been observed yet. Keep rLogs open while the game refreshes your character data.";
      run.disabled = true;
      return;
    }
    const observed = new Date(selectedCharacter.observed_unix_millis);
    characterMeta.textContent = `${selectedCharacter.module_snapshot_detail} · build ${selectedCharacter.source_client_build} · observed ${observed.toLocaleString()}`;
    const currentIds = new Set(selectedCharacter.current_instance_ids);
    const equipped = selectedCharacter.modules.filter((module) =>
      currentIds.has(module.instance_id),
    );
    if (equipped.length === 0) {
      equippedPreview.append(
        text(
          "p",
          selectedCharacter.module_snapshot_available
            ? "No equipped module slots were present in this snapshot."
            : "A live module snapshot is required before optimization.",
          "runtime-empty-result",
        ),
      );
    } else {
      equippedPreview.append(
        ...equipped.map((module) => compactModuleCard(module, catalog)),
      );
    }
    run.disabled = busy || !selectedCharacter.module_snapshot_available;
  }

  function renderCatalog(value: OptimizerCatalog): void {
    attributeControls.clear();
    attributeList.replaceChildren();
    for (const attribute of value.attributes) {
      const row = element("article", "module-attribute-row");
      const identity = element("div", "module-attribute-identity");
      const icon = image(optimizerAssetUrl(attribute.icon), attribute.name);
      const copy = element("div");
      copy.append(text("strong", attribute.name));
      if (attribute.official_name && attribute.official_name !== attribute.name) {
        copy.append(text("small", attribute.official_name, "module-official-name"));
      }
      identity.append(icon, copy);
      const target = compactCheckbox("Prioritize");
      const exclude = compactCheckbox("Exclude");
      const minimum = document.createElement("input");
      minimum.type = "number";
      minimum.min = "0";
      minimum.max = String(Math.max(...attribute.thresholds));
      minimum.placeholder = "Min";
      minimum.className = "module-minimum-input";
      target.input.addEventListener("change", () => {
        if (target.input.checked) exclude.input.checked = false;
      });
      exclude.input.addEventListener("change", () => {
        if (exclude.input.checked) target.input.checked = false;
      });
      row.append(identity, target.label, exclude.label, minimum);
      attributeList.append(row);
      attributeControls.set(attribute.id, {
        target: target.input,
        exclude: exclude.input,
        minimum,
      });
    }
    sizeSelect.select.replaceChildren(
      ...value.combination_sizes.map((size) => option(String(size), `${size} modules`)),
    );
  }

  async function load(): Promise<void> {
    if (busy) return;
    busy = true;
    refresh.disabled = true;
    run.disabled = true;
    status.classList.remove("error");
    status.textContent = "Refreshing the reviewed catalog and local profile snapshots…";
    try {
      const [nextCatalog, nextInventory] = await Promise.all([
        loaders.loadCatalog(),
        loaders.loadInventory(),
      ]);
      if (!alive) return;
      catalog = nextCatalog;
      inventory = nextInventory;
      renderCatalog(nextCatalog);
      const previousId = selectedCharacter?.package_id;
      characterSelect.replaceChildren();
      if (nextInventory.characters.length === 0) {
        characterSelect.append(option("", "No observed characters"));
        selectedCharacter = null;
      } else {
        for (const character of nextInventory.characters) {
          characterSelect.append(
            option(
              character.package_id,
              `${character.display_name ?? `UID ${character.character_id}`} · ${character.region}`,
            ),
          );
        }
        const selected =
          nextInventory.characters.find((entry) => entry.package_id === previousId) ??
          nextInventory.characters.find((entry) => entry.module_snapshot_available) ??
          nextInventory.characters[0] ??
          null;
        selectedCharacter = selected;
        if (selected) characterSelect.value = selected.package_id;
      }
      renderCharacter();
      const ready = nextInventory.characters.filter(
        (entry) => entry.module_snapshot_available,
      ).length;
      status.textContent = `${nextCatalog.attributes.length} localized effects · ${ready} usable character snapshot${ready === 1 ? "" : "s"} · all calculations remain on this PC`;
      if (nextInventory.issues.length > 0) {
        status.textContent += ` · ${nextInventory.issues.length} local snapshot warning${nextInventory.issues.length === 1 ? "" : "s"}`;
      }
    } catch (error) {
      if (!alive) return;
      status.textContent = errorMessage(error);
      status.classList.add("error");
      selectedCharacter = null;
      renderCharacter();
    } finally {
      busy = false;
      refresh.disabled = false;
      run.disabled = !selectedCharacter?.module_snapshot_available;
    }
  }

  async function optimize(): Promise<void> {
    if (busy || selectedCharacter === null || catalog === null) return;
    busy = true;
    refresh.disabled = true;
    run.disabled = true;
    searchStatus.classList.remove("error");
    searchStatus.textContent = "Scoring your observed inventory locally…";
    try {
      const target: number[] = [];
      const exclude: number[] = [];
      const minimums: Record<string, number> = {};
      for (const [attributeId, controls] of attributeControls) {
        if (controls.target.checked) target.push(attributeId);
        if (controls.exclude.checked) exclude.push(attributeId);
        const minimum = controls.minimum.valueAsNumber;
        if (Number.isSafeInteger(minimum) && minimum > 0) {
          minimums[String(attributeId)] = minimum;
        }
      }
      const result = await loaders.optimize({
        modules: selectedCharacter.modules,
        current_instance_ids: selectedCharacter.current_instance_ids,
        target_attributes: target,
        exclude_attributes: exclude,
        min_attr_requirements: minimums,
        combination_size: Number(sizeSelect.select.value),
        max_solutions: Number(resultSelect.select.value),
        search_mode: modeSelect.select.value as OptimizeRequest["search_mode"],
        exact_combination_limit: 500_000,
        beam_width: workstationBeamWidth(),
        minimum_parts: 2,
        minimum_module_total:
          Number.isSafeInteger(minimumTotal.input.valueAsNumber) &&
          minimumTotal.input.valueAsNumber > 0
            ? minimumTotal.input.valueAsNumber
            : null,
        require_target_match: requireTarget.input.checked,
      });
      if (!alive) return;
      renderResults(right, result, catalog);
      searchStatus.textContent = `${result.solutions.length} recommendation${result.solutions.length === 1 ? "" : "s"} · ${result.search.evaluated_states.toLocaleString()} states · ${result.search.exact ? "exact search" : `${result.search.used_mode} search`}`;
    } catch (error) {
      if (!alive) return;
      searchStatus.textContent = errorMessage(error);
      searchStatus.classList.add("error");
    } finally {
      busy = false;
      refresh.disabled = false;
      run.disabled = !selectedCharacter?.module_snapshot_available;
    }
  }

  characterSelect.addEventListener("change", () =>
    setSelectedCharacter(characterSelect.value),
  );
  refresh.addEventListener("click", () => void load());
  run.addEventListener("click", () => void optimize());
  void load();

  return {
    dispose() {
      alive = false;
    },
  };
}

function renderResults(
  container: HTMLElement,
  result: OptimizeResponse,
  catalog: OptimizerCatalog,
): void {
  container.replaceChildren();
  if (result.current_setup) {
    container.append(solutionCard("Currently equipped", result.current_setup, catalog, true));
  }
  const heading = element("div", "module-results-heading");
  heading.append(
    text("h3", "Recommended sets"),
    text(
      "span",
      result.search.exact ? "Exact result" : "Best reviewed candidates",
      "status-pill",
    ),
  );
  container.append(heading);
  if (result.solutions.length === 0) {
    const empty = element("section", "content-card module-results-empty");
    empty.append(
      text(
        "p",
        "No set satisfies the current exclusions and minimums. Relax one requirement and try again.",
        "runtime-empty-result",
      ),
    );
    container.append(empty);
    return;
  }
  result.solutions.forEach((solution, index) => {
    container.append(solutionCard(`#${index + 1} recommendation`, solution, catalog, false));
  });
}

function solutionCard(
  label: string,
  solution: ModuleSolution,
  catalog: OptimizerCatalog,
  current: boolean,
): HTMLElement {
  const card = element(
    "section",
    `content-card module-solution-card${current ? " is-current" : ""}`,
  );
  const heading = element("div", "module-solution-heading");
  heading.append(
    text("div", label, "eyebrow"),
    metric("Power", solution.score),
    metric("Total Link", solution.breakdown.total_link_points),
  );
  const modules = element("div", "module-solution-modules");
  modules.append(...solution.modules.map((module) => compactModuleCard(module, catalog)));
  const effects = element("div", "module-solution-effects");
  const named = new Map(catalog.attributes.map((attribute) => [attribute.id, attribute]));
  for (const score of solution.breakdown.attributes.filter((entry) => entry.total > 0)) {
    const attribute = named.get(score.attribute_id);
    const chip = element("div", "module-effect-chip");
    chip.append(
      image(optimizerAssetUrl(attribute?.icon ?? null), attribute?.name ?? "Unknown effect"),
      text("span", attribute?.name ?? "Unknown effect"),
      text("strong", `${score.total} Link`),
    );
    effects.append(chip);
  }
  card.append(heading, modules, effects);
  return card;
}

function compactModuleCard(
  value: ModuleCandidate,
  catalog: OptimizerCatalog | null,
): HTMLElement {
  const presentation = modulePresentation(value);
  const attributes = new Map(
    (catalog?.attributes ?? []).map((attribute) => [attribute.id, attribute]),
  );
  const card = element("article", "module-inventory-card");
  card.append(image(presentation.icon, presentation.name));
  const copy = element("div", "module-inventory-copy");
  copy.append(
    text("strong", presentation.name),
    text(
      "small",
      `${moduleQuality(value)} · ${value.parts.reduce((sum, part) => sum + Math.max(0, part.initial_link_points ?? 0), 0)} total Link`,
      "module-card-meta",
    ),
  );
  const effects = element("div", "module-card-effects");
  for (const part of value.parts) {
    const attribute = attributes.get(part.part_id);
    effects.append(
      text(
        "span",
        `${attribute?.name ?? "Unknown effect"} ${part.initial_link_points ?? 0}`,
        "module-mini-effect",
      ),
    );
  }
  copy.append(effects);
  card.append(copy);
  return card;
}

function metric(label: string, value: number): HTMLElement {
  const node = element("div", "module-solution-metric");
  node.append(text("span", label), text("strong", value.toLocaleString()));
  return node;
}

function selectControl(
  label: string,
  values: ReadonlyArray<readonly [string, string]>,
): { wrapper: HTMLElement; select: HTMLSelectElement } {
  const wrapper = element("label", "settings-field");
  wrapper.append(text("span", label, "settings-field-label"));
  const select = document.createElement("select");
  select.className = "settings-select";
  select.append(...values.map(([value, name]) => option(value, name)));
  wrapper.append(select);
  return { wrapper, select };
}

function numberControl(
  label: string,
  placeholder: string,
  min: number,
  max: number,
): { wrapper: HTMLElement; input: HTMLInputElement } {
  const wrapper = element("label", "settings-field");
  wrapper.append(text("span", label, "settings-field-label"));
  const input = document.createElement("input");
  input.type = "number";
  input.min = String(min);
  input.max = String(max);
  input.placeholder = placeholder;
  input.className = "settings-input";
  wrapper.append(input);
  return { wrapper, input };
}

function checkboxControl(
  labelText: string,
  checked: boolean,
): { wrapper: HTMLElement; input: HTMLInputElement } {
  const wrapper = element("label", "settings-checkbox module-search-checkbox");
  const input = document.createElement("input");
  input.type = "checkbox";
  input.checked = checked;
  wrapper.append(input, text("span", labelText));
  return { wrapper, input };
}

function compactCheckbox(labelText: string): {
  label: HTMLLabelElement;
  input: HTMLInputElement;
} {
  const label = element("label", "module-compact-check") as HTMLLabelElement;
  const input = document.createElement("input");
  input.type = "checkbox";
  label.append(input, text("span", labelText));
  return { label, input };
}

function option(value: string, label: string): HTMLOptionElement {
  const node = document.createElement("option");
  node.value = value;
  node.textContent = label;
  return node;
}

function image(source: string | null, alternative: string): HTMLImageElement {
  const node = document.createElement("img");
  node.className = "module-icon";
  node.alt = alternative;
  node.loading = "lazy";
  if (source !== null) node.src = source;
  return node;
}

function button(label: string, className: string): HTMLButtonElement {
  const node = document.createElement("button");
  node.type = "button";
  node.className = className;
  node.textContent = label;
  return node;
}

function element<K extends keyof HTMLElementTagNameMap>(
  tag: K,
  className?: string,
): HTMLElementTagNameMap[K] {
  const node = document.createElement(tag);
  if (className) node.className = className;
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

function workstationBeamWidth(): number {
  return (navigator.hardwareConcurrency || 4) >= 8 ? 512 : 256;
}

function errorMessage(error: unknown): string {
  return error instanceof Error ? error.message : String(error);
}
