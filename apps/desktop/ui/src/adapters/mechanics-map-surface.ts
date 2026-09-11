import type { MountedSurface } from "../shell/types";
import type { UiLocalizer } from "../localization/ui-locale";
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

export function mountMechanicsMapSurface(container: HTMLElement, dependencies: MechanicsMapDependencies, localizer: UiLocalizer): MountedSurface {
  const ui = localizer;
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
  heading.append(text("span", ui.t("ui.mechanics_map.surface.eyebrow"), "eyebrow"), text("h2", ui.t("ui.mechanics_map.surface.title")), text("p", ui.t("ui.mechanics_map.surface.description"), "card-copy"));
  const badge = text("span", ui.t("ui.mechanics_map.status.connecting"), "overlay-menu-preview-badge");
  header.append(heading, badge);

  const layout = el("section", "mechanics-map-layout");
  const mapCard = el("article", "content-card mechanics-map-card");
  const mapHeading = el("header", "mechanics-map-heading");
  const mapTitle = text("h3", ui.t("ui.mechanics_map.status.waiting_for_scene"));
  const mapMeta = text("p", ui.t("ui.mechanics_map.surface.no_world_context"), "card-copy");
  const overlayStatus = text("p", "", "mechanics-map-overlay-launch-status");
  overlayStatus.hidden = true;
  overlayStatus.setAttribute("role", "status");
  overlayStatus.setAttribute("aria-live", "polite");
  const mapCopy = el("div"); mapCopy.append(mapTitle, mapMeta, overlayStatus);
  const controls = el("div", "mechanics-map-controls");
  const openOverlay = text("button", ui.t("ui.mechanics_map.surface.open_overlay"), "primary-button mechanics-map-open-overlay");
  openOverlay.type = "button";
  openOverlay.addEventListener("click", () => {
    openOverlay.disabled = true;
    preparationMessage = ui.t("ui.mechanics_map.surface.opening_overlay");
    preparationError = false;
    render();
    void dependencies.openOverlay()
      .then(() => {
        preparationMessage = ui.t("ui.mechanics_map.surface.overlay_opened");
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
  const refreshMaps = text("button", ui.t("ui.mechanics_map.surface.refresh_maps"), "quiet-button mechanics-map-refresh-assets");
  refreshMaps.type = "button";
  refreshMaps.addEventListener("click", () => { void prepareReviewedMaps(); });
  const monsters = check(ui.t("ui.mechanics_map.surface.monsters"), true, (checked) => { showMonsters = checked; render(); });
  const resetView = text("button", ui.t("ui.mechanics_map.surface.reset_view"), "quiet-button mechanics-map-reset-view");
  resetView.type = "button";
  resetView.addEventListener("click", resetMapView);
  controls.append(openOverlay, refreshMaps, monsters, resetView);
  mapHeading.append(mapCopy, controls);
  const radar = el("div", "mechanics-radar");
  radar.setAttribute("role", "img");
  radar.setAttribute("aria-label", ui.t("ui.mechanics_map.surface.map_aria"));
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
  const empty = text("p", ui.t("ui.mechanics_map.surface.waiting_for_position"), "mechanics-map-empty");
  plane.append(fallback, arena, regions, points);
  radar.append(plane, empty);
  radar.addEventListener("wheel", zoomMap, { passive: false });
  radar.addEventListener("pointerdown", beginPan);
  radar.addEventListener("pointermove", continuePan);
  radar.addEventListener("pointerup", endPan);
  radar.addEventListener("pointercancel", endPan);
  mapCard.append(mapHeading, radar);

  const signalCard = el("article", "content-card mechanics-signal-card");
  signalCard.append(text("span", ui.t("ui.mechanics_map.surface.evidence_eyebrow"), "eyebrow"), text("h3", ui.t("ui.mechanics_map.surface.live_signals")));
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
      badge.textContent = ui.t("ui.mechanics_map.status.unavailable");
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
    badge.textContent = snapshot.local_position_observed ? ui.t("ui.mechanics_map.status.live") : snapshot.scene_id === null ? ui.t("ui.mechanics_map.status.waiting") : ui.t("ui.mechanics_map.status.position_needed");
    badge.dataset.state = snapshot.local_position_observed ? "live" : "waiting";
    mapTitle.textContent = snapshot.scene_name ?? (snapshot.scene_id === null ? ui.t("ui.mechanics_map.status.waiting_for_scene") : ui.t("ui.mechanics_map.surface.scene", { id: snapshot.scene_id }));
    mapMeta.textContent = [snapshot.map_id === null ? null : ui.t("ui.mechanics_map.surface.map_id", { id: snapshot.map_id }), snapshot.encounter_pack ?? ui.t("ui.mechanics_map.surface.no_encounter_pack"), mapReady ? ui.t("ui.mechanics_map.surface.game_scene_map") : ui.t("ui.mechanics_map.surface.game_map_unavailable")].filter(Boolean).join(" · ");
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
      point.title = entity.display_name ?? (entity.monster_id === null
        ? ui.t("ui.mechanics_map.surface.entity_kind", { kind: entity.kind })
        : ui.t("ui.mechanics_map.surface.entity_kind_id", { kind: entity.kind, id: entity.monster_id }));
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
      point.title = marker.related_actor_id === null
        ? ui.t("ui.mechanics_map.surface.packet_marker")
        : ui.t("ui.mechanics_map.surface.actor_marker", { id: marker.related_actor_id });
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
        ? ui.t("ui.mechanics_map.surface.correct_portal")
        : annotation.kind === "other_portal"
          ? ui.t("ui.mechanics_map.surface.other_portal")
          : ui.t("ui.mechanics_map.surface.sticky_bomb_target");
      points.append(point);
    }
    empty.hidden = mapReady && snapshot.local_position_observed;
    empty.textContent = !sceneMap
      ? ui.t("ui.mechanics_map.surface.scene_map_unavailable")
      : mapAssetState === "ready"
        ? ui.t("ui.mechanics_map.surface.waiting_for_position")
        : ui.t("ui.mechanics_map.surface.local_map_unavailable");
    signals.replaceChildren();
    if (snapshot.data_gap !== null) signals.append(notice(ui.t("ui.mechanics_map.surface.data_gap"), snapshot.data_gap, "error"));
    if (sceneMap && mapAssetState !== "ready") {
      const assetNotice = notice(ui.t("ui.mechanics_map.surface.game_map_asset"), preparationMessage ?? ui.t("ui.mechanics_map.surface.map_not_prepared"), preparationError ? "error" : "waiting");
      const prepare = text("button", preparingMaps ? ui.t("ui.mechanics_map.surface.preparing") : ui.t("ui.mechanics_map.surface.prepare_maps"), "quiet-button");
      prepare.type = "button";
      prepare.disabled = preparingMaps;
      prepare.addEventListener("click", () => { void prepareReviewedMaps(); });
      assetNotice.append(prepare);
      signals.append(assetNotice);
    }
    if (!sceneMap) signals.append(notice(ui.t("ui.mechanics_map.surface.game_map_unavailable"), ui.t("ui.mechanics_map.surface.no_matching_map"), "waiting"));
    if (!snapshot.encounter_pack_reviewed) signals.append(notice(ui.t("ui.mechanics_map.surface.encounter_pack"), ui.t("ui.mechanics_map.surface.no_matching_pack"), "waiting"));
    else signals.append(notice(snapshot.encounter_pack ?? ui.t("ui.mechanics_map.surface.encounter_pack"), ui.t("ui.mechanics_map.surface.reviewed_pack_active"), "live"));
    for (const signal of reviewedMechanics) {
      const target = snapshot.entities.find((entity) => entity.actor_id === signal.target_actor_id);
      const row = el("article", "mechanics-signal-row");
      const identity = mechanicKindLabel(ui, signal.mechanic_kind) ?? signal.presentation_name ?? (signal.effect_id < 0
        ? ui.t("ui.mechanics_map.surface.cast_id", { id: -signal.effect_id })
        : ui.t("ui.mechanics_map.surface.effect_id", { id: signal.effect_id }));
      const targetLabel = target?.display_name ?? ui.t("ui.mechanics_map.surface.actor_id", { id: signal.target_actor_id });
      const stackLabel = signal.stacks === null ? "" : ` · ${ui.t("ui.mechanics_map.surface.stacks", { count: signal.stacks })}`;
      const durationLabel = signal.duration_millis === null ? "" : ` · ${ui.t("ui.mechanics_map.surface.seconds", { seconds: (signal.duration_millis / 1000).toFixed(1) })}`;
      row.append(text("strong", identity), text("span", `${targetLabel}${stackLabel}${durationLabel}`));
      signals.append(row);
    }
    if (reviewedMechanics.length === 0 && snapshot.data_gap === null) signals.append(text("p", ui.t("ui.mechanics_map.surface.no_active_mechanics"), "runtime-empty-result"));
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
    radar.setAttribute("aria-description", ui.t("ui.mechanics_map.surface.zoom_help", { zoom: formatZoom(mapScale) }));
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
    refreshMaps.textContent = ui.t("ui.mechanics_map.surface.refreshing_maps");
    preparationMessage = null;
    preparationError = false;
    render();
    try {
      const result = await dependencies.prepareLocalMaps();
      if (!alive) return;
      preparationMessage = ui.t("ui.mechanics_map.surface.maps_prepared", {
        assets: ui.formatNumber(result.preparedAssets),
        locales: ui.formatNumber(result.preparedLocales),
        build: result.clientBuild,
      });
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
      refreshMaps.textContent = ui.t("ui.mechanics_map.surface.refresh_maps");
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
function mechanicKindLabel(ui: UiLocalizer, kind: string | null): string | null {
  if (kind === null) return null;
  if (!REVIEWED_MECHANIC_KINDS.has(kind)) return null;
  return ui.t(`ui.mechanics_map.mechanic.${kind}`);
}

export const REVIEWED_MECHANIC_KINDS: ReadonlySet<string> = new Set([
  "tower_activating", "tower_blue_complete", "tower_gold_complete", "energy_pillar", "energy_pillar_short",
  "charge_target_left", "charge_target_right", "charge_target_random", "puzzle_piece_one", "puzzle_piece_two",
  "clone_charge_left", "clone_charge_right", "sticky_bomb", "gravity_blast", "heavy_wound", "void_corruption_binding",
  "wudi_slash_order", "matrix_rune_a", "matrix_rune_b", "matrix_rune_c", "matrix_rune_d", "matrix_initializer",
  "death_sentence_target", "matrix_callout", "double_echo_ice", "double_echo_water", "dual_element_gravity", "ice_water_floor",
  "pizza_orange", "pizza_purple", "pizza_indicator", "electromagnetic_pulse_a", "electromagnetic_pulse_b",
  "electromagnetic_pulse_c", "share", "mirage_share", "phase_corner", "phase_edge", "normal_target", "decay_target",
  "hit_order_one", "hit_order_two", "hit_order_three", "normal_share", "mirage_share_callout", "normal_decay",
  "mirage_decay", "normal_spread", "mirage_spread", "pinball_countdown", "causal_jump", "floor_link", "divine_sentence",
  "cumulative_sentence", "mirage_sentence", "return_top_left", "return_middle_left", "return_bottom_left", "return_top_right",
  "return_middle_right", "return_bottom_right", "return_count_one", "return_count_two", "return_count_three", "ring_inner",
  "ring_middle", "ring_outer", "near_chain", "far_chain", "wheel_blue", "wheel_red", "wheel_doom", "energy_target",
  "pair_mark", "pair_settle", "pair_penalty", "pair_swap", "near_chain_cast", "far_chain_cast", "shadow_cast",
  "pair_settle_cast", "pair_resolve_cast",
]);
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
