import type { MountedSurface } from "../shell/types";
import {
  type GpuSupport,
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
  loadGpuSupport(): Promise<GpuSupport>;
  optimize(request: OptimizeRequest): Promise<OptimizeResponse>;
}

interface AttributeControls {
  row: HTMLElement;
  target: HTMLInputElement;
  exclude: HTMLInputElement;
  minimum: HTMLInputElement;
  searchText: string;
}

interface StoredPreferences {
  schema_version: 1;
  combination_size: number;
  search_mode: OptimizeRequest["search_mode"];
  max_solutions: number;
  minimum_module_total: number | null;
  require_target_match: boolean;
  target_attributes: number[];
  exclude_attributes: number[];
  min_attr_requirements: Record<string, number>;
  use_gpu: boolean;
}

export interface ModuleLinkSummary {
  attributeId: number;
  name: string;
  icon: string | null;
  totalLink: number;
}

export function summarizeModuleLinks(
  modules: readonly ModuleCandidate[],
  catalog: OptimizerCatalog | null,
): ModuleLinkSummary[] {
  const attributes = new Map(
    (catalog?.attributes ?? []).map((attribute) => [attribute.id, attribute]),
  );
  const totals = new Map<number, number>();
  for (const module of modules) {
    for (const part of module.parts) {
      const link = part.initial_link_points ?? 0;
      if (link > 0) totals.set(part.part_id, (totals.get(part.part_id) ?? 0) + link);
    }
  }
  return [...totals.entries()]
    .map(([attributeId, totalLink]) => ({
      attributeId,
      name: attributes.get(attributeId)?.name ?? `Effect ${attributeId}`,
      icon: attributes.get(attributeId)?.icon ?? null,
      totalLink,
    }))
    .sort((left, right) =>
      right.totalLink - left.totalLink || left.name.localeCompare(right.name));
}

const GPU_PREFERENCE_KEY = "rlogs.module-optimizer.gpu";
const PREFERENCE_KEY_PREFIX = "rlogs.module-optimizer.preferences.v1.";

export function mountModuleOptimizerSurface(
  container: HTMLElement,
  loaders: OptimizerLoaders,
): MountedSurface {
  let alive = true;
  let busy = false;
  let catalog: OptimizerCatalog | null = null;
  let inventory: LocalModuleInventory | null = null;
  let gpuSupport: GpuSupport | null = null;
  let gpuChecking = false;
  let selectedCharacter: LocalModuleCharacter | null = null;
  const attributeControls = new Map<number, AttributeControls>();

  const root = element("div", "plugin-surface module-optimizer-surface");
  const toolbar = element("section", "content-card module-optimizer-toolbar");
  const identity = element("div", "module-toolbar-identity");
  const characterSelect = document.createElement("select");
  characterSelect.className = "settings-select module-character-select";
  characterSelect.setAttribute("aria-label", "Character module inventory");
  const characterCopy = element("div", "module-character-copy");
  const characterName = text("strong", "Loading character modules…");
  const characterMeta = text("span", "Reading the latest live profile snapshot.");
  characterCopy.append(characterName, characterMeta);
  identity.append(characterSelect, characterCopy);

  const refresh = button("Refresh", "secondary-button module-refresh-button");
  const run = button("Find best loadouts", "primary-button module-run-button");
  run.disabled = true;
  const toolbarActions = element("div", "module-toolbar-actions");
  toolbarActions.append(refresh, run);
  toolbar.append(identity, toolbarActions);

  const status = text("p", "Loading the reviewed module catalog…", "module-runtime-status");
  const workbench = element("div", "module-optimizer-workbench");

  const goals = element("aside", "content-card module-goal-card");
  const goalHeader = element("div", "module-panel-heading");
  const goalTitle = element("div");
  goalTitle.append(
    text("span", "BUILD RULES", "eyebrow"),
    text("h3", "What should this set do?"),
  );
  const selectionSummary = text("span", "No priorities yet", "module-selection-summary");
  goalHeader.append(goalTitle, selectionSummary);

  const searchGrid = element("div", "module-search-grid");
  const sizeSelect = selectControl("Loadout", [
    ["4", "4 modules"],
    ["5", "5 modules"],
  ]);
  const modeSelect = selectControl("Method", [
    ["auto", "Automatic"],
    ["exact", "Exact"],
    ["beam", "Fast search"],
  ]);
  const resultSelect = selectControl("Results", [
    ["5", "Top 5"],
    ["10", "Top 10"],
    ["20", "Top 20"],
  ]);
  const minimumTotal = numberControl("Minimum module Link", "No minimum", 0, 999);
  searchGrid.append(
    sizeSelect.wrapper,
    modeSelect.wrapper,
    resultSelect.wrapper,
    minimumTotal.wrapper,
  );

  const requireTarget = checkboxControl(
    "Only consider modules containing a wanted effect",
    true,
  );

  const gpuRow = element("div", "module-gpu-row");
  const gpuToggle = element("label", "module-gpu-toggle") as HTMLLabelElement;
  const gpuInput = document.createElement("input");
  gpuInput.type = "checkbox";
  gpuInput.checked = readGlobalGpuPreference();
  const gpuSwitch = element("span", "module-gpu-switch");
  const gpuToggleCopy = element("span", "module-gpu-toggle-copy");
  gpuToggleCopy.append(
    text("strong", "GPU acceleration"),
    text("small", "Optional · NVIDIA and AMD"),
  );
  gpuToggle.append(gpuInput, gpuSwitch, gpuToggleCopy);
  const gpuDetail = text("span", "Off · multi-core CPU", "module-gpu-detail");
  const checkGpu = button("Detect", "secondary-button module-check-gpu");
  gpuRow.append(gpuToggle, gpuDetail, checkGpu);

  const effectTools = element("div", "module-effect-tools");
  const effectSearch = document.createElement("input");
  effectSearch.type = "search";
  effectSearch.className = "settings-input module-effect-search";
  effectSearch.placeholder = "Search effects";
  effectSearch.setAttribute("aria-label", "Search module effects");
  const clearRules = button("Clear", "secondary-button module-clear-rules");
  effectTools.append(effectSearch, clearRules);

  const effectLegend = element("div", "module-effect-legend");
  effectLegend.append(
    text("span", "Want = score higher"),
    text("span", "Avoid = score lower"),
    text("span", "Minimum = required Link"),
  );
  const attributeList = element("div", "module-attribute-list");
  goals.append(
    goalHeader,
    searchGrid,
    requireTarget.wrapper,
    gpuRow,
    effectTools,
    effectLegend,
    attributeList,
  );

  const plan = element("main", "module-plan-column");
  const currentCard = element("section", "content-card module-current-card");
  const currentHeader = element("div", "module-panel-heading");
  const currentTitle = element("div");
  currentTitle.append(
    text("span", "LIVE INVENTORY", "eyebrow"),
    text("h3", "Currently equipped"),
  );
  const currentCount = text("span", "Waiting for a snapshot", "status-pill");
  currentHeader.append(currentTitle, currentCount);
  const currentLinkSummary = element("div", "module-loadout-link-summary");
  currentLinkSummary.hidden = true;
  const equippedPreview = element("div", "module-equipped-preview");
  currentCard.append(currentHeader, currentLinkSummary, equippedPreview);

  const results = element("section", "module-optimizer-results");
  const empty = element("div", "content-card module-results-empty");
  empty.append(
    text("span", "RECOMMENDATIONS", "eyebrow"),
    text("h3", "Your best sets will appear here"),
    text(
      "p",
      "Choose the effects you want, then run the optimizer. Results compare directly against your equipped loadout.",
      "runtime-empty-result",
    ),
  );
  results.append(empty);
  plan.append(currentCard, results);
  workbench.append(goals, plan);
  root.append(toolbar, status, workbench);
  container.append(root);

  function setSelectedCharacter(packageId: string): void {
    selectedCharacter =
      inventory?.characters.find((entry) => entry.package_id === packageId) ?? null;
    restorePreferences();
    renderCharacter();
  }

  function renderCharacter(): void {
    equippedPreview.replaceChildren();
    currentLinkSummary.replaceChildren();
    currentLinkSummary.hidden = true;
    if (selectedCharacter === null) {
      characterName.textContent = "No module snapshot yet";
      characterMeta.textContent =
        "Keep rLogs open while the game refreshes your character data.";
      currentCount.textContent = "Not synced";
      equippedPreview.append(
        text(
          "p",
          "Open the game on a character with modules, then refresh this page.",
          "runtime-empty-result",
        ),
      );
      run.disabled = true;
      return;
    }
    const observed = new Date(selectedCharacter.observed_unix_millis);
    characterName.textContent =
      selectedCharacter.display_name ?? `UID ${selectedCharacter.character_id}`;
    characterMeta.textContent = `${selectedCharacter.modules.length.toLocaleString()} owned · ${selectedCharacter.region} · synced ${formatObserved(observed)}`;
    const currentIds = new Set(selectedCharacter.current_instance_ids);
    const equipped = selectedCharacter.modules.filter((module) =>
      currentIds.has(module.instance_id),
    );
    currentCount.textContent = `${equipped.length} equipped`;
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
      renderModuleLinkSummary(currentLinkSummary, equipped, catalog);
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
      const identityNode = element("div", "module-attribute-identity");
      const icon = image(optimizerAssetUrl(attribute.icon), attribute.name);
      const copy = element("div");
      copy.append(text("strong", attribute.name));
      if (attribute.official_name && attribute.official_name !== attribute.name) {
        copy.title = `In game: ${attribute.official_name}`;
      }
      identityNode.append(icon, copy);
      const target = compactCheckbox("Want", "wanted");
      const exclude = compactCheckbox("Avoid", "avoided");
      const minimum = document.createElement("input");
      minimum.type = "number";
      minimum.min = "0";
      minimum.max = String(Math.max(...attribute.thresholds));
      minimum.placeholder = "Min";
      minimum.setAttribute("aria-label", `Minimum ${attribute.name} Link`);
      minimum.title = `Minimum required ${attribute.name} Link`;
      minimum.className = "module-minimum-input";
      target.input.addEventListener("change", () => {
        if (target.input.checked) exclude.input.checked = false;
        updateRulePresentation(attribute.id);
      });
      exclude.input.addEventListener("change", () => {
        if (exclude.input.checked) target.input.checked = false;
        updateRulePresentation(attribute.id);
      });
      minimum.addEventListener("change", () => updateRulePresentation(attribute.id));
      row.append(identityNode, target.label, exclude.label, minimum);
      attributeList.append(row);
      attributeControls.set(attribute.id, {
        row,
        target: target.input,
        exclude: exclude.input,
        minimum,
        searchText: `${attribute.name} ${attribute.official_name ?? ""}`.toLocaleLowerCase(),
      });
    }
    sizeSelect.select.replaceChildren(
      ...value.combination_sizes.map((size) => option(String(size), `${size} modules`)),
    );
  }

  function updateRulePresentation(attributeId: number): void {
    const controls = attributeControls.get(attributeId);
    if (!controls) return;
    controls.row.dataset.rule = controls.target.checked
      ? "wanted"
      : controls.exclude.checked
        ? "avoided"
        : controls.minimum.valueAsNumber > 0
          ? "minimum"
          : "neutral";
    updateSelectionSummary();
    persistPreferences();
  }

  function updateSelectionSummary(): void {
    let wanted = 0;
    let avoided = 0;
    let minimums = 0;
    for (const controls of attributeControls.values()) {
      wanted += Number(controls.target.checked);
      avoided += Number(controls.exclude.checked);
      minimums += Number(controls.minimum.valueAsNumber > 0);
    }
    requireTarget.input.disabled = wanted === 0;
    requireTarget.wrapper.title = wanted === 0
      ? "Choose at least one wanted effect to enable this inventory filter."
      : "Exclude inventory modules that contain none of the wanted effects.";
    const parts = [
      wanted > 0 ? `${wanted} wanted` : "",
      avoided > 0 ? `${avoided} avoided` : "",
      minimums > 0 ? `${minimums} minimum${minimums === 1 ? "" : "s"}` : "",
    ].filter(Boolean);
    selectionSummary.textContent = parts.length > 0 ? parts.join(" · ") : "No priorities yet";
  }

  function filterEffects(): void {
    const query = effectSearch.value.trim().toLocaleLowerCase();
    for (const controls of attributeControls.values()) {
      controls.row.hidden = query.length > 0 && !controls.searchText.includes(query);
    }
  }

  function clearEffectRules(): void {
    for (const controls of attributeControls.values()) {
      controls.target.checked = false;
      controls.exclude.checked = false;
      controls.minimum.value = "";
      controls.row.dataset.rule = "neutral";
    }
    effectSearch.value = "";
    filterEffects();
    updateSelectionSummary();
    persistPreferences();
  }

  function renderGpuSupport(value: GpuSupport): void {
    const identityText = [value.vendor, value.device_name].filter(Boolean).join(" · ");
    gpuDetail.textContent = value.available
      ? `${identityText || "OpenCL GPU"} · ready`
      : "Unavailable · CPU fallback";
    gpuDetail.title = value.detail;
    gpuRow.dataset.state = value.available ? "ready" : "unavailable";
    checkGpu.textContent = "Recheck";
  }

  function renderGpuUnchecked(): void {
    gpuDetail.textContent = gpuInput.checked
      ? "On · detect when optimizing"
      : "Off · multi-core CPU";
    gpuDetail.title =
      "OpenCL uses the installed NVIDIA or AMD graphics driver. CPU remains available as a safe fallback.";
    gpuRow.dataset.state = "unchecked";
    checkGpu.textContent = "Detect";
  }

  async function refreshGpuSupport(): Promise<GpuSupport | null> {
    if (gpuChecking) return gpuSupport;
    gpuChecking = true;
    checkGpu.disabled = true;
    checkGpu.textContent = "Checking…";
    gpuRow.dataset.state = "checking";
    gpuDetail.textContent = "Checking the graphics driver…";
    try {
      const support = await loaders.loadGpuSupport();
      if (!alive) return null;
      gpuSupport = support;
      renderGpuSupport(support);
      return support;
    } catch (error) {
      if (!alive) return null;
      gpuSupport = null;
      gpuRow.dataset.state = "unavailable";
      gpuDetail.textContent = "GPU check failed · CPU ready";
      gpuDetail.title = errorMessage(error);
      checkGpu.textContent = "Retry";
      return null;
    } finally {
      gpuChecking = false;
      checkGpu.disabled = false;
    }
  }

  async function load(): Promise<void> {
    if (busy) return;
    busy = true;
    refresh.disabled = true;
    run.disabled = true;
    status.classList.remove("error");
    status.textContent = "Refreshing the reviewed catalog and live module snapshot…";
    try {
      const [nextCatalog, nextInventory] = await Promise.all([
        loaders.loadCatalog(),
        loaders.loadInventory(),
      ]);
      if (!alive) return;
      catalog = nextCatalog;
      inventory = nextInventory;
      renderCatalog(nextCatalog);
      if (gpuSupport === null) renderGpuUnchecked();
      const previousId = selectedCharacter?.package_id;
      characterSelect.replaceChildren();
      if (nextInventory.characters.length === 0) {
        characterSelect.append(option("", "No synced character"));
        selectedCharacter = null;
      } else {
        for (const character of nextInventory.characters) {
          characterSelect.append(
            option(character.package_id, character.display_name ?? `UID ${character.character_id}`),
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
      restorePreferences();
      renderCharacter();
      const ready = nextInventory.characters.filter(
        (entry) => entry.module_snapshot_available,
      ).length;
      status.textContent = `${nextCatalog.attributes.length} localized effects · ${ready} usable character snapshot${ready === 1 ? "" : "s"} · exact-build scoring ${nextCatalog.scoring_revision}`;
      if (nextInventory.issues.length > 0) {
        status.textContent += ` · ${nextInventory.issues.length} snapshot warning${nextInventory.issues.length === 1 ? "" : "s"}`;
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
    root.classList.add("is-optimizing");
    refresh.disabled = true;
    run.disabled = true;
    run.textContent = "Searching…";
    status.classList.remove("error");
    const started = performance.now();
    status.textContent = "Preparing the observed inventory…";
    try {
      const target: number[] = [];
      const exclude: number[] = [];
      const minimums: Record<string, number> = {};
      for (const [attributeId, controls] of attributeControls) {
        if (controls.target.checked) target.push(attributeId);
        if (controls.exclude.checked) exclude.push(attributeId);
        const minimum = controls.minimum.valueAsNumber;
        if (Number.isSafeInteger(minimum) && minimum > 0) minimums[String(attributeId)] = minimum;
      }
      let selectedGpu = gpuInput.checked ? gpuSupport : null;
      if (gpuInput.checked && selectedGpu === null) {
        status.textContent = "Starting the cross-vendor GPU engine…";
        selectedGpu = await refreshGpuSupport();
      }
      const gpuEnabled = gpuInput.checked && selectedGpu?.available === true;
      status.textContent = gpuEnabled
        ? "Searching with the GPU and multi-core CPU…"
        : "Searching across CPU cores…";
      persistPreferences();
      const result = await loaders.optimize({
        modules: selectedCharacter.modules,
        current_instance_ids: selectedCharacter.current_instance_ids,
        target_attributes: target,
        exclude_attributes: exclude,
        min_attr_requirements: minimums,
        combination_size: Number(sizeSelect.select.value),
        max_solutions: Number(resultSelect.select.value),
        search_mode: modeSelect.select.value as OptimizeRequest["search_mode"],
        use_gpu: gpuEnabled,
        exact_combination_limit: gpuEnabled ? 10_000_000 : 500_000,
        beam_width: workstationBeamWidth(),
        minimum_parts: 2,
        minimum_module_total:
          Number.isSafeInteger(minimumTotal.input.valueAsNumber) && minimumTotal.input.valueAsNumber > 0
            ? minimumTotal.input.valueAsNumber
            : null,
        require_target_match: requireTarget.input.checked,
      });
      if (!alive) return;
      renderResults(results, result, catalog);
      const elapsed = Math.max(0, performance.now() - started);
      status.textContent = `${result.solutions.length} recommendation${result.solutions.length === 1 ? "" : "s"} · ${result.search.evaluated_states.toLocaleString()} states · ${result.search.exact ? "exact" : "guided search"} · ${backendLabel(result)} · ${formatElapsed(elapsed)}`;
      if (result.search.accelerator_fallback) status.textContent += ` · ${result.search.accelerator_fallback}`;
    } catch (error) {
      if (!alive) return;
      status.textContent = errorMessage(error);
      status.classList.add("error");
    } finally {
      busy = false;
      root.classList.remove("is-optimizing");
      refresh.disabled = false;
      run.disabled = !selectedCharacter?.module_snapshot_available;
      run.textContent = "Find best loadouts";
    }
  }

  function restorePreferences(): void {
    const saved = selectedCharacter ? readPreferences(selectedCharacter.package_id) : null;
    sizeSelect.select.value = String(saved?.combination_size ?? 4);
    modeSelect.select.value = saved?.search_mode ?? "auto";
    resultSelect.select.value = String(saved?.max_solutions ?? 5);
    minimumTotal.input.value = saved?.minimum_module_total ? String(saved.minimum_module_total) : "";
    requireTarget.input.checked = saved?.require_target_match ?? true;
    gpuInput.checked = saved?.use_gpu ?? readGlobalGpuPreference();
    const wanted = new Set(saved?.target_attributes ?? []);
    const avoided = new Set(saved?.exclude_attributes ?? []);
    for (const [attributeId, controls] of attributeControls) {
      controls.target.checked = wanted.has(attributeId);
      controls.exclude.checked = avoided.has(attributeId);
      controls.minimum.value = String(saved?.min_attr_requirements[String(attributeId)] ?? "");
      controls.row.dataset.rule = controls.target.checked
        ? "wanted"
        : controls.exclude.checked
          ? "avoided"
          : controls.minimum.valueAsNumber > 0
            ? "minimum"
            : "neutral";
    }
    updateSelectionSummary();
    if (gpuSupport === null) renderGpuUnchecked();
  }

  function persistPreferences(): void {
    if (selectedCharacter === null) return;
    const target: number[] = [];
    const exclude: number[] = [];
    const minimums: Record<string, number> = {};
    for (const [attributeId, controls] of attributeControls) {
      if (controls.target.checked) target.push(attributeId);
      if (controls.exclude.checked) exclude.push(attributeId);
      if (controls.minimum.valueAsNumber > 0) minimums[String(attributeId)] = controls.minimum.valueAsNumber;
    }
    writePreferences(selectedCharacter.package_id, {
      schema_version: 1,
      combination_size: Number(sizeSelect.select.value),
      search_mode: modeSelect.select.value as OptimizeRequest["search_mode"],
      max_solutions: Number(resultSelect.select.value),
      minimum_module_total: minimumTotal.input.valueAsNumber > 0 ? minimumTotal.input.valueAsNumber : null,
      require_target_match: requireTarget.input.checked,
      target_attributes: target,
      exclude_attributes: exclude,
      min_attr_requirements: minimums,
      use_gpu: gpuInput.checked,
    });
  }

  characterSelect.addEventListener("change", () => setSelectedCharacter(characterSelect.value));
  refresh.addEventListener("click", () => void load());
  effectSearch.addEventListener("input", filterEffects);
  clearRules.addEventListener("click", clearEffectRules);
  gpuInput.addEventListener("change", () => {
    writeGlobalGpuPreference(gpuInput.checked);
    persistPreferences();
    if (gpuSupport === null) renderGpuUnchecked();
  });
  checkGpu.addEventListener("click", () => void refreshGpuSupport());
  for (const control of [sizeSelect.select, modeSelect.select, resultSelect.select, minimumTotal.input, requireTarget.input]) {
    control.addEventListener("change", persistPreferences);
  }
  run.addEventListener("click", () => void optimize());
  void load();

  return { dispose() { alive = false; } };
}

function renderResults(container: HTMLElement, result: OptimizeResponse, catalog: OptimizerCatalog): void {
  container.replaceChildren();
  const heading = element("div", "module-results-heading");
  const title = element("div");
  title.append(text("span", "RESULTS", "eyebrow"), text("h3", "Recommended loadouts"));
  heading.append(title, text("span", result.search.exact ? "Exact result" : "Best guided candidates", "status-pill"));
  container.append(heading);
  if (result.solutions.length === 0) {
    const empty = element("div", "content-card module-results-empty");
    empty.append(text("p", "No set satisfies every exclusion and minimum. Relax one rule and try again.", "runtime-empty-result"));
    container.append(empty);
    return;
  }
  for (const [index, solution] of result.solutions.entries()) {
    container.append(solutionCard(`#${index + 1}`, solution, catalog));
  }
}

function solutionCard(label: string, solution: ModuleSolution, catalog: OptimizerCatalog): HTMLElement {
  const card = element("article", "content-card module-solution-card");
  const heading = element("div", "module-solution-heading");
  const linkSummary = element("div", "module-loadout-link-summary");
  renderModuleLinkSummary(linkSummary, solution.modules, catalog);
  heading.append(text("span", label, "module-solution-rank"), linkSummary);
  const modules = element("div", "module-solution-module-strip");
  modules.append(...solution.modules.map((module) => solutionModuleTile(module, catalog)));
  card.append(heading, modules);
  return card;
}

function renderModuleLinkSummary(
  container: HTMLElement,
  modules: readonly ModuleCandidate[],
  catalog: OptimizerCatalog | null,
): void {
  const effects = element("div", "module-solution-effects");
  for (const score of summarizeModuleLinks(modules, catalog)) {
    const chip = element("div", "module-effect-chip");
    chip.append(
      image(optimizerAssetUrl(score.icon), score.name),
      text("span", score.name),
      text("strong", `${score.totalLink.toLocaleString()} Link`),
    );
    effects.append(chip);
  }
  container.replaceChildren(
    text("span", "Combined links", "module-link-summary-label"),
    effects,
  );
  container.hidden = effects.childElementCount === 0;
}

function solutionModuleTile(value: ModuleCandidate, catalog: OptimizerCatalog): HTMLElement {
  const presentation = modulePresentation(value);
  const tile = element("div", "module-solution-tile");
  tile.dataset.quality = String(value.quality ?? presentation.quality);
  tile.append(image(presentation.icon, presentation.name));
  const copy = element("span");
  copy.append(
    text("strong", shortModuleName(presentation.name)),
    text("small", `${moduleQuality(value)} · ${moduleLinkTotal(value)} total Link`),
  );
  const named = new Map(catalog.attributes.map((attribute) => [attribute.id, attribute]));
  const effects = element("span", "module-solution-tile-effects");
  for (const part of value.parts) {
    effects.append(
      text(
        "span",
        `${named.get(part.part_id)?.name ?? "Unknown effect"} ${part.initial_link_points ?? 0}`,
      ),
    );
  }
  copy.append(effects);
  tile.append(copy);
  tile.title = presentation.name;
  return tile;
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
  card.dataset.quality = String(value.quality ?? presentation.quality);
  card.append(image(presentation.icon, presentation.name));
  const copy = element("div", "module-inventory-copy");
  copy.append(
    text("strong", presentation.name),
    text(
      "small",
      `${moduleQuality(value)} · ${moduleLinkTotal(value)} total Link`,
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

function selectControl(label: string, values: ReadonlyArray<readonly [string, string]>): { wrapper: HTMLElement; select: HTMLSelectElement } {
  const wrapper = element("label", "settings-field");
  wrapper.append(text("span", label, "settings-field-label"));
  const select = document.createElement("select");
  select.className = "settings-select";
  select.append(...values.map(([value, name]) => option(value, name)));
  wrapper.append(select);
  return { wrapper, select };
}

function numberControl(label: string, placeholder: string, min: number, max: number): { wrapper: HTMLElement; input: HTMLInputElement } {
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

function checkboxControl(labelText: string, checked: boolean): { wrapper: HTMLElement; input: HTMLInputElement } {
  const wrapper = element("label", "settings-checkbox module-search-checkbox");
  const input = document.createElement("input");
  input.type = "checkbox";
  input.checked = checked;
  wrapper.append(input, text("span", labelText));
  return { wrapper, input };
}

function compactCheckbox(labelText: string, state: string): { label: HTMLLabelElement; input: HTMLInputElement } {
  const label = element("label", `module-compact-check is-${state}`) as HTMLLabelElement;
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
  if (source !== null) {
    node.addEventListener("error", () => node.classList.add("is-missing"), { once: true });
    node.src = source;
  }
  return node;
}

function button(label: string, className: string): HTMLButtonElement {
  const node = document.createElement("button");
  node.type = "button";
  node.className = className;
  node.textContent = label;
  return node;
}

function element<K extends keyof HTMLElementTagNameMap>(tag: K, className?: string): HTMLElementTagNameMap[K] {
  const node = document.createElement(tag);
  if (className) node.className = className;
  return node;
}

function text<K extends keyof HTMLElementTagNameMap>(tag: K, value: string, className?: string): HTMLElementTagNameMap[K] {
  const node = element(tag, className);
  node.textContent = value;
  return node;
}

function moduleLinkTotal(value: ModuleCandidate): number {
  return value.parts.reduce((sum, part) => sum + Math.max(0, part.initial_link_points ?? 0), 0);
}

function shortModuleName(name: string): string {
  return name.replace(/ Module(?: - Premium)?$/u, "").replace(/^Excellent /u, "");
}

function backendLabel(result: OptimizeResponse): string {
  if (result.search.backend === "cpu_open_cl_hybrid") return `${result.search.accelerator_name ?? "GPU"} + CPU`;
  return result.search.backend === "open_cl" ? result.search.accelerator_name ?? "OpenCL GPU" : "multi-core CPU";
}

function workstationBeamWidth(): number {
  const threads = navigator.hardwareConcurrency || 4;
  if (threads >= 24) return 2_048;
  if (threads >= 12) return 1_024;
  if (threads >= 8) return 512;
  return 256;
}

function formatObserved(value: Date): string {
  const seconds = Math.max(0, Math.round((Date.now() - value.getTime()) / 1_000));
  if (seconds < 60) return `${seconds}s ago`;
  const minutes = Math.round(seconds / 60);
  if (minutes < 60) return `${minutes}m ago`;
  return value.toLocaleString();
}

function formatElapsed(milliseconds: number): string {
  if (milliseconds < 1_000) return `${Math.round(milliseconds)} ms`;
  return `${(milliseconds / 1_000).toFixed(milliseconds < 10_000 ? 1 : 0)} s`;
}

function errorMessage(error: unknown): string { return error instanceof Error ? error.message : String(error); }

function readGlobalGpuPreference(): boolean {
  try { return localStorage.getItem(GPU_PREFERENCE_KEY) === "true"; } catch { return false; }
}

function writeGlobalGpuPreference(enabled: boolean): void {
  try { localStorage.setItem(GPU_PREFERENCE_KEY, String(enabled)); } catch { /* session only */ }
}

function readPreferences(packageId: string): StoredPreferences | null {
  try {
    const parsed = JSON.parse(localStorage.getItem(`${PREFERENCE_KEY_PREFIX}${packageId}`) ?? "null") as unknown;
    return isStoredPreferences(parsed) ? parsed : null;
  } catch { return null; }
}

function writePreferences(packageId: string, preferences: StoredPreferences): void {
  try { localStorage.setItem(`${PREFERENCE_KEY_PREFIX}${packageId}`, JSON.stringify(preferences)); } catch { /* session only */ }
}

function isStoredPreferences(value: unknown): value is StoredPreferences {
  if (typeof value !== "object" || value === null || Array.isArray(value)) return false;
  const record = value as Partial<StoredPreferences>;
  return (
    record.schema_version === 1 &&
    Number.isSafeInteger(record.combination_size) &&
    (record.search_mode === "auto" || record.search_mode === "exact" || record.search_mode === "beam") &&
    Number.isSafeInteger(record.max_solutions) &&
    (record.minimum_module_total === null || Number.isSafeInteger(record.minimum_module_total)) &&
    typeof record.require_target_match === "boolean" &&
    Array.isArray(record.target_attributes) && record.target_attributes.every(Number.isSafeInteger) &&
    Array.isArray(record.exclude_attributes) && record.exclude_attributes.every(Number.isSafeInteger) &&
    typeof record.min_attr_requirements === "object" && record.min_attr_requirements !== null &&
    typeof record.use_gpu === "boolean"
  );
}
