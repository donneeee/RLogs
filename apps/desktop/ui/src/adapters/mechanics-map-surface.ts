import type { MountedSurface } from "../shell/types";
import { projectCoralMatrixBeam, projectCoralPizzaRegions, projectCursedTombChargeRegion, projectMechanicsMapEntities, projectMechanicsMapPoint, projectRaidFloorRegions, projectVoidTowerMapAnnotations, zoomMechanicsMapAt, type MechanicsMapUpdate } from "./mechanics-map";

export interface MechanicsMapDependencies {
  loadSnapshot(): Promise<MechanicsMapUpdate>;
  waitForSnapshot(afterRevision: number): Promise<MechanicsMapUpdate>;
  prepareLocalMaps(): Promise<LocalMapPreparationResult>;
  openOverlay(): Promise<void>;
}

export interface LocalMapPreparationResult {
  clientBuild: string;
  preparedAssets: number;
  preparedLocales: number;
  message: string;
}

export function mountMechanicsMapSurface(container: HTMLElement, dependencies: MechanicsMapDependencies): MountedSurface {
  let alive = true;
  let update: MechanicsMapUpdate | null = null;
  let showMonsters = true;
  let mapAssetUrl: string | null = null;
  let mapAssetState: "none" | "loading" | "ready" | "missing" = "none";
  let preparingMaps = false;
  let preparationMessage: string | null = null;
  let preparationError = false;
  let mapScale = 1;
  let mapPanX = 0;
  let mapPanY = 0;
  const automaticPreparationAttempts = new Set<string>();
  let dragging: { pointerId: number; x: number; y: number } | null = null;

  const root = el("div", "plugin-surface overlay-workspace-surface mechanics-map-surface");
  const header = el("section", "content-card overlay-workspace-intro");
  const heading = el("div", "overlay-workspace-heading");
  heading.append(text("span", "MAP OVERLAY", "eyebrow"), text("h2", "Map"), text("p", "The current in-game map, reproduced from this game installation and augmented with live positions and encounter mechanics.", "card-copy"));
  const badge = text("span", "CONNECTING", "overlay-menu-preview-badge");
  header.append(heading, badge);

  const layout = el("section", "mechanics-map-layout");
  const mapCard = el("article", "content-card mechanics-map-card");
  const mapHeading = el("header", "mechanics-map-heading");
  const mapTitle = text("h3", "Waiting for scene");
  const mapMeta = text("p", "No packet-observed world context yet.", "card-copy");
  const overlayStatus = text("p", "", "mechanics-map-overlay-launch-status");
  overlayStatus.hidden = true;
  overlayStatus.setAttribute("role", "status");
  overlayStatus.setAttribute("aria-live", "polite");
  const mapCopy = el("div"); mapCopy.append(mapTitle, mapMeta, overlayStatus);
  const controls = el("div", "mechanics-map-controls");
  const openOverlay = text("button", "Open overlay", "primary-button mechanics-map-open-overlay");
  openOverlay.type = "button";
  openOverlay.addEventListener("click", () => {
    openOverlay.disabled = true;
    preparationMessage = "Opening the map overlay…";
    preparationError = false;
    render();
    void dependencies.openOverlay()
      .then(() => {
        preparationMessage = "Map overlay opened. Hold the overlay interaction key to move or resize it.";
      })
      .catch((error) => {
        preparationMessage = error instanceof Error ? error.message : String(error);
        preparationError = true;
      })
      .finally(() => {
        openOverlay.disabled = false;
        if (alive) render();
      });
  });
  const refreshMaps = text("button", "Refresh maps from game files", "quiet-button mechanics-map-refresh-assets");
  refreshMaps.type = "button";
  refreshMaps.addEventListener("click", () => { void prepareReviewedMaps(); });
  const monsters = check("Monsters", true, (checked) => { showMonsters = checked; render(); });
  const resetView = text("button", "Reset view", "quiet-button mechanics-map-reset-view");
  resetView.type = "button";
  resetView.addEventListener("click", resetMapView);
  controls.append(openOverlay, refreshMaps, monsters, resetView);
  mapHeading.append(mapCopy, controls);
  const radar = el("div", "mechanics-radar");
  radar.setAttribute("role", "img");
  radar.setAttribute("aria-label", "Live player-relative mechanics map");
  const fallback = el("div", "mechanics-radar-fallback");
  fallback.hidden = true;
  fallback.append(el("span", "mechanics-ring ring-one"), el("span", "mechanics-ring ring-two"), el("span", "mechanics-crosshair"));
  const arena = document.createElementNS("http://www.w3.org/2000/svg", "svg");
  arena.classList.add("mechanics-map-arena");
  arena.setAttribute("viewBox", "0 0 100 100");
  arena.setAttribute("aria-hidden", "true");
  const regions = document.createElementNS("http://www.w3.org/2000/svg", "svg");
  regions.classList.add("mechanics-map-regions");
  regions.setAttribute("viewBox", "0 0 100 100");
  regions.setAttribute("aria-hidden", "true");
  const points = el("div", "mechanics-radar-points");
  const plane = el("div", "mechanics-map-plane");
  const empty = text("p", "Waiting for a local player position.", "mechanics-map-empty");
  plane.append(fallback, arena, regions, points);
  radar.append(plane, empty);
  radar.addEventListener("wheel", zoomMap, { passive: false });
  radar.addEventListener("pointerdown", beginPan);
  radar.addEventListener("pointermove", continuePan);
  radar.addEventListener("pointerup", endPan);
  radar.addEventListener("pointercancel", endPan);
  mapCard.append(mapHeading, radar);

  const signalCard = el("article", "content-card mechanics-signal-card");
  signalCard.append(text("span", "ENCOUNTER EVIDENCE", "eyebrow"), text("h3", "Live signals"));
  const signals = el("div", "mechanics-signal-list");
  signalCard.append(signals);
  layout.append(mapCard, signalCard);
  root.append(header, layout);
  container.replaceChildren(root);
  void connect();

  async function connect(): Promise<void> {
    try {
      update = await dependencies.loadSnapshot();
      if (!alive) return;
      render();
      while (alive) {
        update = await dependencies.waitForSnapshot(update.revision);
        if (!alive) return;
        render();
      }
    } catch (error) {
      if (!alive) return;
      badge.textContent = "UNAVAILABLE";
      badge.dataset.state = "error";
      signals.replaceChildren(text("p", error instanceof Error ? error.message : String(error), "runtime-empty-result"));
    }
  }

  function render(): void {
    overlayStatus.hidden = preparationMessage === null;
    overlayStatus.textContent = preparationMessage ?? "";
    overlayStatus.dataset.state = preparationError ? "error" : "status";
    if (update === null) return;
    const snapshot = update.snapshot;
    const reviewedMechanics = snapshot.mechanics.filter((signal) => signal.mechanic_kind !== null);
    const sceneMap = snapshot.map_model === "absolute_scene_map";
    prepareMapAsset(snapshot.background_asset_url);
    const mapReady = sceneMap && snapshot.background_asset_url !== null && mapAssetState === "ready";
    const projected = mapReady
      ? projectMechanicsMapEntities(snapshot, false).filter((entity) => entity.visible && (showMonsters || !["monster", "npc", "object"].includes(entity.kind)))
      : [];
    badge.textContent = snapshot.local_position_observed ? "LIVE" : snapshot.scene_id === null ? "WAITING" : "POSITION NEEDED";
    badge.dataset.state = snapshot.local_position_observed ? "live" : "waiting";
    mapTitle.textContent = snapshot.scene_name ?? (snapshot.scene_id === null ? "Waiting for scene" : `Scene ${snapshot.scene_id}`);
    mapMeta.textContent = [snapshot.map_id === null ? null : `Map ${snapshot.map_id}`, snapshot.encounter_pack ?? "No reviewed encounter pack", mapReady ? "Game scene map" : "Game map unavailable"].filter(Boolean).join(" · ");
    radar.dataset.model = snapshot.map_model;
    radar.dataset.layout = snapshot.map_layout ?? "none";
    arena.replaceChildren();
    radar.dataset.assetState = mapAssetState;
    plane.style.setProperty("--mechanics-map-background", mapReady && mapAssetUrl !== null ? `url(${JSON.stringify(mapAssetUrl)})` : "none");
    applyMapTransform();
    regions.replaceChildren();
    if (mapReady) {
      for (const signal of reviewedMechanics) {
        const polygonPoints = projectCursedTombChargeRegion(snapshot, signal);
        if (polygonPoints.length >= 3) {
          const polygon = document.createElementNS("http://www.w3.org/2000/svg", "polygon");
          polygon.setAttribute("points", polygonPoints.map((point) => `${point.mapX},${point.mapY}`).join(" "));
          polygon.dataset.side = signal.mechanic_kind === "clone_charge_left" ? "left" : "right";
          regions.append(polygon);
        }

        const beamPoints = projectCoralMatrixBeam(snapshot, signal);
        if (beamPoints.length === 2) {
          const line = document.createElementNS("http://www.w3.org/2000/svg", "line");
          line.setAttribute("x1", String(beamPoints[0]!.mapX)); line.setAttribute("y1", String(beamPoints[0]!.mapY));
          line.setAttribute("x2", String(beamPoints[1]!.mapX)); line.setAttribute("y2", String(beamPoints[1]!.mapY));
          line.dataset.kind = "matrix_callout";
          regions.append(line);
        }
      }
      for (const pizza of projectCoralPizzaRegions(snapshot)) {
        if (pizza.points.length < 3) continue;
        const polygon = document.createElementNS("http://www.w3.org/2000/svg", "polygon");
        polygon.setAttribute("points", pizza.points.map((point) => `${point.mapX},${point.mapY}`).join(" "));
        polygon.dataset.kind = pizza.kind;
        regions.append(polygon);
      }
      for (const floor of projectRaidFloorRegions(snapshot)) {
        if (floor.points.length < 3) continue;
        const polygon = document.createElementNS("http://www.w3.org/2000/svg", "polygon");
        polygon.setAttribute("points", floor.points.map((point) => `${point.mapX},${point.mapY}`).join(" "));
        polygon.dataset.kind = floor.kind;
        if (floor.label !== undefined) polygon.dataset.label = floor.label;
        regions.append(polygon);
        if (floor.label !== undefined) {
          const label = document.createElementNS("http://www.w3.org/2000/svg", "text");
          label.textContent = floor.label;
          label.setAttribute("x", String(floor.points.reduce((sum, point) => sum + point.mapX, 0) / floor.points.length));
          label.setAttribute("y", String(floor.points.reduce((sum, point) => sum + point.mapY, 0) / floor.points.length));
          label.dataset.kind = "floor_label";
          regions.append(label);
        }
      }
      renderRaidRings(regions, snapshot);
    }
    points.replaceChildren();
    for (const entity of projected) {
      const point = el("span", "mechanics-map-point");
      const mechanic = reviewedMechanics.find((signal) => signal.target_actor_id === entity.actor_id);
      point.dataset.kind = entity.kind;
      point.dataset.dead = String(entity.dead);
      point.dataset.stale = String(entity.stale);
      point.dataset.mechanic = String(mechanic !== undefined);
      point.style.left = `${entity.mapX}%`;
      point.style.top = `${entity.mapY}%`;
      if (mechanic !== undefined) point.style.setProperty("--point-color", mechanicColor(mechanic.effect_id, mechanic.mechanic_kind));
      point.title = entity.display_name ?? (entity.monster_id === null ? entity.kind : `${entity.kind} ${entity.monster_id}`);
      if (entity.facing_radians !== null) point.style.setProperty("--facing", `${entity.facing_radians}rad`);
      points.append(point);
    }
    for (const marker of snapshot.markers) {
      if (marker.x === null || marker.z === null) continue;
      const projectedMarker = mapReady ? projectMechanicsMapPoint(snapshot, marker.x, marker.z, false) : null;
      if (projectedMarker === null || !projectedMarker.visible) continue;
      const point = text("span", marker.marker_number === null ? (marker.marker_id === null ? "•" : String(marker.marker_id)) : String(marker.marker_number), "mechanics-map-marker");
      point.style.left = `${projectedMarker.mapX}%`;
      point.style.top = `${projectedMarker.mapY}%`;
      point.title = marker.related_actor_id === null ? "Packet-observed map marker" : `Marker for actor ${marker.related_actor_id}`;
      points.append(point);
    }
    for (const annotation of mapReady ? projectVoidTowerMapAnnotations(snapshot) : []) {
      const point = el("span", "mechanics-map-void-annotation");
      point.append(text(
        "span",
        annotation.kind === "correct_portal" ? "✓" : annotation.kind === "other_portal" ? "×" : "!",
      ));
      point.dataset.kind = annotation.kind;
      point.dataset.actorId = String(annotation.actorId);
      point.style.left = `${annotation.mapX}%`;
      point.style.top = `${annotation.mapY}%`;
      point.title = annotation.kind === "correct_portal"
        ? "Packet-observed correct portal"
        : annotation.kind === "other_portal"
          ? "Packet-observed other portal"
          : "Packet-observed sticky-bomb target";
      points.append(point);
    }
    empty.hidden = mapReady && snapshot.local_position_observed;
    empty.textContent = !sceneMap
      ? "Map unavailable: this identified scene has no reviewed game-map asset."
      : mapAssetState === "ready"
        ? "Waiting for a local player position."
        : "Map unavailable until the reviewed game-map asset is available locally.";
    signals.replaceChildren();
    if (snapshot.data_gap !== null) signals.append(notice("Data gap", snapshot.data_gap, "error"));
    if (sceneMap && mapAssetState !== "ready") {
      const assetNotice = notice("Game map asset", preparationMessage ?? "The exact reviewed texture is available in the installed game but has not been prepared locally. Packet positions remain available on the coordinate canvas.", preparationError ? "error" : "waiting");
      const prepare = text("button", preparingMaps ? "Preparing…" : "Prepare local maps", "quiet-button");
      prepare.type = "button";
      prepare.disabled = preparingMaps;
      prepare.addEventListener("click", () => { void prepareReviewedMaps(); });
      assetNotice.append(prepare);
      signals.append(assetNotice);
    }
    if (!sceneMap) signals.append(notice("Game map unavailable", "No reviewed game-map asset matches this exact scene and runtime identity. The map remains blank.", "waiting"));
    if (!snapshot.encounter_pack_reviewed) signals.append(notice("Encounter pack", "No reviewed pack matches this exact scene. Positions remain exact; guidance stays disabled.", "waiting"));
    else signals.append(notice(snapshot.encounter_pack ?? "Encounter pack", "Current-build effect identities are enabled. No safe-area geometry is inferred.", "live"));
    for (const signal of reviewedMechanics) {
      const target = snapshot.entities.find((entity) => entity.actor_id === signal.target_actor_id);
      const row = el("article", "mechanics-signal-row");
      const identity = mechanicKindLabel(signal.mechanic_kind) ?? signal.presentation_name ?? (signal.effect_id < 0 ? `Cast ${-signal.effect_id}` : `Effect ${signal.effect_id}`);
      row.append(text("strong", identity), text("span", `${target?.display_name ?? `Actor ${signal.target_actor_id}`}${signal.stacks === null ? "" : ` · ${signal.stacks} stacks`}${signal.duration_millis === null ? "" : ` · ${(signal.duration_millis / 1000).toFixed(1)}s`}`));
      signals.append(row);
    }
    if (reviewedMechanics.length === 0 && snapshot.data_gap === null) signals.append(text("p", "No active reviewed mechanic effects or targeted casts.", "runtime-empty-result"));
  }

  function zoomMap(event: WheelEvent): void {
    event.preventDefault();
    const rect = radar.getBoundingClientRect();
    if (rect.width <= 0 || rect.height <= 0) return;
    const cursorX = event.clientX - rect.left - rect.width / 2;
    const cursorY = event.clientY - rect.top - rect.height / 2;
    const next = zoomMechanicsMapAt({ scale: mapScale, panX: mapPanX, panY: mapPanY }, cursorX, cursorY, event.deltaY);
    mapScale = next.scale;
    mapPanX = next.panX;
    mapPanY = next.panY;
    applyMapTransform();
  }

  function beginPan(event: PointerEvent): void {
    if (event.button !== 0) return;
    dragging = { pointerId: event.pointerId, x: event.clientX, y: event.clientY };
    radar.setPointerCapture(event.pointerId);
    radar.dataset.dragging = "true";
  }

  function continuePan(event: PointerEvent): void {
    if (dragging?.pointerId !== event.pointerId) return;
    mapPanX += event.clientX - dragging.x;
    mapPanY += event.clientY - dragging.y;
    dragging = { pointerId: event.pointerId, x: event.clientX, y: event.clientY };
    applyMapTransform();
  }

  function endPan(event: PointerEvent): void {
    if (dragging?.pointerId !== event.pointerId) return;
    dragging = null;
    delete radar.dataset.dragging;
    if (radar.hasPointerCapture(event.pointerId)) radar.releasePointerCapture(event.pointerId);
  }

  function resetMapView(): void {
    mapScale = 1;
    mapPanX = 0;
    mapPanY = 0;
    applyMapTransform();
  }

  function applyMapTransform(): void {
    plane.style.transform = `translate(${mapPanX}px, ${mapPanY}px) scale(${mapScale})`;
    radar.setAttribute("aria-description", `Map zoom ${formatZoom(mapScale)}. Drag to pan and use the mouse wheel to zoom without a fixed limit.`);
  }

  function prepareMapAsset(url: string | null): void {
    if (url === mapAssetUrl) return;
    mapAssetUrl = url;
    if (url === null) {
      mapAssetState = "none";
      return;
    }
    mapAssetState = "loading";
    const image = new Image();
    image.onload = () => {
      if (!alive || mapAssetUrl !== url) return;
      mapAssetState = "ready";
      render();
    };
    image.onerror = () => {
      if (!alive || mapAssetUrl !== url) return;
      mapAssetState = "missing";
      if (claimAutomaticMapPreparation(url, automaticPreparationAttempts)) {
        void prepareReviewedMaps();
      } else {
        render();
      }
    };
    image.src = url;
  }

  async function prepareReviewedMaps(): Promise<void> {
    if (preparingMaps) return;
    preparingMaps = true;
    refreshMaps.disabled = true;
    refreshMaps.textContent = "Refreshing maps…";
    preparationMessage = null;
    preparationError = false;
    render();
    try {
      const result = await dependencies.prepareLocalMaps();
      if (!alive) return;
      preparationMessage = result.message;
      mapAssetUrl = null;
      mapAssetState = "none";
      render();
    } catch (error) {
      if (!alive) return;
      preparationMessage = error instanceof Error ? error.message : String(error);
      preparationError = true;
      render();
    } finally {
      preparingMaps = false;
      refreshMaps.disabled = false;
      refreshMaps.textContent = "Refresh maps from game files";
      if (alive) render();
    }
  }

  return { dispose() { alive = false; mapAssetUrl = null; root.remove(); } };
}

export function claimAutomaticMapPreparation(url: string, attempts: Set<string>): boolean {
  if (attempts.has(url)) return false;
  attempts.add(url);
  return true;
}

function notice(title: string, detail: string, state: string): HTMLElement { const item = el("article", "mechanics-signal-row"); item.dataset.state = state; item.append(text("strong", title), text("span", detail)); return item; }
function mechanicColor(effectId: number, kind: string | null): string {
  if (kind?.includes("ice") || kind === "matrix_rune_a" || kind === "electromagnetic_pulse_a") return "#6da9ff";
  if (kind?.includes("water") || kind === "matrix_rune_b" || kind === "electromagnetic_pulse_b") return "#5ce4d4";
  if (kind?.includes("orange") || kind?.includes("sticky") || kind?.includes("pinball")) return "#ff9d5c";
  if (kind?.includes("purple") || kind?.includes("mirage") || kind === "matrix_rune_d") return "#b38aff";
  if (kind?.includes("gold") || kind?.includes("order") || kind?.includes("count") || kind === "matrix_rune_c") return "#f2c36b";
  if (kind?.includes("correct") || kind?.includes("complete")) return "#80e09b";
  const colors: Record<number, string> = {
    884102: "#5fa8ff", 884103: "#f2c36b", 884129: "#b38aff", 884141: "#ff8fb8",
    884162: "#5fa8ff", 884163: "#f2c36b", 884168: "#ff6f83", 884169: "#80e09b", 884170: "#ff9d5c",
  };
  return colors[Math.abs(effectId)] ?? "#ff6f83";
}
function mechanicKindLabel(kind: string | null): string | null {
  if (kind === null) return null;
  const labels: Record<string, string> = {
    tower_activating: "Tower activating", tower_blue_complete: "Blue tower complete", tower_gold_complete: "Gold tower complete",
    energy_pillar: "Energy pillar", energy_pillar_short: "Short energy pillar",
    charge_target_left: "Left-side charge target", charge_target_right: "Right-side charge target", charge_target_random: "Random charge target",
    puzzle_piece_one: "Puzzle piece 1", puzzle_piece_two: "Puzzle piece 2",
    clone_charge_left: "Left clone charge", clone_charge_right: "Right clone charge",
    sticky_bomb: "Sticky bomb", gravity_blast: "Gravity blast", heavy_wound: "Heavy wound",
    void_corruption_binding: "Void Corruption Binding", wudi_slash_order: "Slash order",
    matrix_rune_a: "Matrix rune A", matrix_rune_b: "Matrix rune B", matrix_rune_c: "Matrix rune C", matrix_rune_d: "Matrix rune D",
    matrix_initializer: "Matrix initialization", death_sentence_target: "Death sentence target", matrix_callout: "Matrix callout",
    double_echo_ice: "Double Echo — Ice", double_echo_water: "Double Echo — Water", dual_element_gravity: "Dual-element gravity",
    ice_water_floor: "Ice/water floor",
    pizza_orange: "Orange pizza sector", pizza_purple: "Purple pizza sector", pizza_indicator: "Pizza sectors",
    electromagnetic_pulse_a: "Electromagnetic Pulse A", electromagnetic_pulse_b: "Electromagnetic Pulse B", electromagnetic_pulse_c: "Electromagnetic Pulse C",
    share: "Share", mirage_share: "Mirage share", phase_corner: "Corner phase", phase_edge: "Edge phase",
    normal_target: "Normal target", decay_target: "Decay target", hit_order_one: "Hit order 1", hit_order_two: "Hit order 2", hit_order_three: "Hit order 3",
    normal_share: "Normal share", mirage_share_callout: "Mirage share", normal_decay: "Normal decay", mirage_decay: "Mirage decay",
    normal_spread: "Normal spread", mirage_spread: "Mirage spread", pinball_countdown: "Pinball countdown", causal_jump: "Causal jump",
    floor_link: "Floor link", divine_sentence: "Divine sentence", cumulative_sentence: "Cumulative Sentence", mirage_sentence: "Mirage sentence",
    return_top_left: "Return — top left", return_middle_left: "Return — middle left", return_bottom_left: "Return — bottom left",
    return_top_right: "Return — top right", return_middle_right: "Return — middle right", return_bottom_right: "Return — bottom right",
    return_count_one: "Return count 1", return_count_two: "Return count 2", return_count_three: "Return count 3",
    ring_inner: "Inner electromagnetic ring", ring_middle: "Middle electromagnetic ring", ring_outer: "Outer electromagnetic ring",
    near_chain: "Near chain", far_chain: "Far chain", wheel_blue: "Blue wheel", wheel_red: "Red wheel", wheel_doom: "Doom wheel",
    energy_target: "Energy target", pair_mark: "Pair mark", pair_settle: "Pair settle", pair_penalty: "Pair penalty", pair_swap: "Pair swap",
    near_chain_cast: "Near-chain cast", far_chain_cast: "Far-chain cast", shadow_cast: "Shadow cast",
    pair_settle_cast: "Pair-settle cast", pair_resolve_cast: "Pair-resolve cast",
  };
  return labels[kind] ?? null;
}
function renderRaidRings(regions: SVGSVGElement, snapshot: MechanicsMapUpdate["snapshot"]): void {
  if (snapshot.map_layout !== "raid_ring") return;
  const bands = { ring_inner: [0, 12.5], ring_middle: [12.5, 17.5], ring_outer: [18.5, 30] } as const;
  const active = snapshot.mechanics.filter((signal) => signal.mechanic_kind !== null && signal.mechanic_kind in bands).slice(-3);
  const center = projectMechanicsMapPoint(snapshot, 0, 0, false);
  if (center === null || snapshot.map_span_x === null || snapshot.map_span_z === null) return;
  for (const signal of active) {
    const kind = signal.mechanic_kind as keyof typeof bands;
    const [inner, outer] = bands[kind];
    const circle = document.createElementNS("http://www.w3.org/2000/svg", "ellipse");
    circle.setAttribute("cx", String(center.mapX)); circle.setAttribute("cy", String(center.mapY));
    const radius = inner === 0 ? outer : (inner + outer) / 2;
    circle.setAttribute("rx", String((radius / snapshot.map_span_x) * 100));
    circle.setAttribute("ry", String((radius / snapshot.map_span_z) * 100));
    if (inner !== 0) {
      circle.style.strokeWidth = String(((outer - inner) / snapshot.map_span_x) * 100);
    }
    circle.dataset.kind = kind;
    regions.append(circle);
  }
}
function formatZoom(scale: number): string { return `${Math.round(scale * 100)}%`; }
function check(label: string, checked: boolean, changed: (checked: boolean) => void): HTMLLabelElement { const wrapper = el("label", "mechanics-map-check") as HTMLLabelElement; const input = document.createElement("input"); input.type = "checkbox"; input.checked = checked; input.addEventListener("change", () => changed(input.checked)); wrapper.append(input, document.createTextNode(label)); return wrapper; }
function el<K extends keyof HTMLElementTagNameMap>(tag: K, className?: string): HTMLElementTagNameMap[K] { const value = document.createElement(tag); if (className !== undefined) value.className = className; return value; }
function text<K extends keyof HTMLElementTagNameMap>(tag: K, value: string, className?: string): HTMLElementTagNameMap[K] { const node = el(tag, className); node.textContent = value; return node; }
