import type { MountedSurface } from "../shell/types";
import { projectMechanicsMapEntities, projectMechanicsMapPoint, type MechanicsMapUpdate } from "./mechanics-map";

export interface MechanicsMapDependencies {
  loadSnapshot(): Promise<MechanicsMapUpdate>;
  waitForSnapshot(afterRevision: number): Promise<MechanicsMapUpdate>;
}

export function mountMechanicsMapSurface(container: HTMLElement, dependencies: MechanicsMapDependencies): MountedSurface {
  let alive = true;
  let update: MechanicsMapUpdate | null = null;
  let rotateWithPlayer = true;
  let showMonsters = true;

  const root = el("div", "plugin-surface overlay-workspace-surface mechanics-map-surface");
  const header = el("section", "content-card overlay-workspace-intro");
  const heading = el("div", "overlay-workspace-heading");
  heading.append(text("span", "SPATIAL OVERLAY", "eyebrow"), text("h2", "Mechanics Map"), text("p", "An exact player-relative radar using packet-observed positions and current-build encounter signals.", "card-copy"));
  const badge = text("span", "CONNECTING", "overlay-menu-preview-badge");
  header.append(heading, badge);

  const layout = el("section", "mechanics-map-layout");
  const mapCard = el("article", "content-card mechanics-map-card");
  const mapHeading = el("header", "mechanics-map-heading");
  const mapTitle = text("h3", "Waiting for scene");
  const mapMeta = text("p", "No packet-observed world context yet.", "card-copy");
  const mapCopy = el("div"); mapCopy.append(mapTitle, mapMeta);
  const controls = el("div", "mechanics-map-controls");
  const rotate = check("Rotate with player", true, (checked) => { rotateWithPlayer = checked; render(); });
  const monsters = check("Monsters", true, (checked) => { showMonsters = checked; render(); });
  controls.append(rotate, monsters);
  mapHeading.append(mapCopy, controls);
  const radar = el("div", "mechanics-radar");
  radar.setAttribute("role", "img");
  radar.setAttribute("aria-label", "Live player-relative mechanics map");
  const fallback = el("div", "mechanics-radar-fallback");
  fallback.append(el("span", "mechanics-ring ring-one"), el("span", "mechanics-ring ring-two"), el("span", "mechanics-crosshair"));
  const points = el("div", "mechanics-radar-points");
  const empty = text("p", "Waiting for a local player position.", "mechanics-map-empty");
  radar.append(fallback, points, empty);
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
    if (update === null) return;
    const snapshot = update.snapshot;
    const sceneMap = snapshot.map_model === "absolute_scene_map";
    const projected = projectMechanicsMapEntities(snapshot, sceneMap ? false : rotateWithPlayer).filter((entity) => entity.visible && (showMonsters || !["monster", "npc", "object"].includes(entity.kind)));
    badge.textContent = snapshot.local_position_observed ? "LIVE" : snapshot.scene_id === null ? "WAITING" : "POSITION NEEDED";
    badge.dataset.state = snapshot.local_position_observed ? "live" : "waiting";
    mapTitle.textContent = snapshot.scene_name ?? (snapshot.scene_id === null ? "Waiting for scene" : `Scene ${snapshot.scene_id}`);
    mapMeta.textContent = [snapshot.map_id === null ? null : `Map ${snapshot.map_id}`, snapshot.encounter_pack ?? "No reviewed encounter pack", sceneMap ? "Game scene map" : `${snapshot.world_radius}u radius`].filter(Boolean).join(" · ");
    radar.dataset.model = snapshot.map_model;
    rotate.hidden = sceneMap;
    radar.style.setProperty("--mechanics-map-background", snapshot.background_asset_url === null ? "none" : `url(${JSON.stringify(snapshot.background_asset_url)})`);
    points.replaceChildren();
    for (const entity of projected) {
      const point = el("span", "mechanics-map-point");
      const mechanic = snapshot.mechanics.find((signal) => signal.target_actor_id === entity.actor_id);
      point.dataset.kind = entity.kind;
      point.dataset.dead = String(entity.dead);
      point.dataset.stale = String(entity.stale);
      point.dataset.mechanic = String(mechanic !== undefined);
      point.style.left = `${entity.mapX}%`;
      point.style.top = `${entity.mapY}%`;
      if (mechanic !== undefined) point.style.setProperty("--point-color", mechanicColor(mechanic.effect_id));
      point.title = entity.display_name ?? (entity.monster_id === null ? entity.kind : `${entity.kind} ${entity.monster_id}`);
      if (entity.facing_radians !== null) point.style.setProperty("--facing", `${entity.facing_radians}rad`);
      points.append(point);
    }
    for (const marker of snapshot.markers) {
      if (marker.x === null || marker.z === null) continue;
      const projectedMarker = projectMechanicsMapPoint(snapshot, marker.x, marker.z, sceneMap ? false : rotateWithPlayer);
      if (projectedMarker === null || !projectedMarker.visible) continue;
      const point = text("span", marker.marker_id === null ? "•" : String(marker.marker_id), "mechanics-map-marker");
      point.style.left = `${projectedMarker.mapX}%`;
      point.style.top = `${projectedMarker.mapY}%`;
      point.title = marker.related_actor_id === null ? "Packet-observed map marker" : `Marker for actor ${marker.related_actor_id}`;
      points.append(point);
    }
    empty.hidden = snapshot.local_position_observed;
    signals.replaceChildren();
    if (snapshot.data_gap !== null) signals.append(notice("Data gap", snapshot.data_gap, "error"));
    if (!snapshot.encounter_pack_reviewed) signals.append(notice("Encounter pack", "No reviewed pack matches this exact scene. Positions remain exact; guidance stays disabled.", "waiting"));
    else signals.append(notice(snapshot.encounter_pack ?? "Encounter pack", "Current-build effect identities are enabled. No safe-area geometry is inferred.", "live"));
    for (const signal of snapshot.mechanics) {
      const target = snapshot.entities.find((entity) => entity.actor_id === signal.target_actor_id);
      const row = el("article", "mechanics-signal-row");
      const identity = signal.presentation_name ?? (signal.effect_id < 0 ? `Cast ${-signal.effect_id}` : `Effect ${signal.effect_id}`);
      row.append(text("strong", identity), text("span", `${target?.display_name ?? `Actor ${signal.target_actor_id}`}${signal.stacks === null ? "" : ` · ${signal.stacks} stacks`}${signal.duration_millis === null ? "" : ` · ${(signal.duration_millis / 1000).toFixed(1)}s`}`));
      signals.append(row);
    }
    if (snapshot.mechanics.length === 0 && snapshot.data_gap === null) signals.append(text("p", "No active reviewed mechanic effects or targeted casts.", "runtime-empty-result"));
  }

  return { dispose() { alive = false; root.remove(); } };
}

function notice(title: string, detail: string, state: string): HTMLElement { const item = el("article", "mechanics-signal-row"); item.dataset.state = state; item.append(text("strong", title), text("span", detail)); return item; }
function mechanicColor(effectId: number): string {
  const colors: Record<number, string> = {
    884102: "#5fa8ff", 884103: "#f2c36b", 884129: "#b38aff", 884141: "#ff8fb8",
    884162: "#5fa8ff", 884163: "#f2c36b", 884168: "#ff6f83", 884169: "#80e09b", 884170: "#ff9d5c",
  };
  return colors[Math.abs(effectId)] ?? "#ff6f83";
}
function check(label: string, checked: boolean, changed: (checked: boolean) => void): HTMLLabelElement { const wrapper = el("label", "mechanics-map-check") as HTMLLabelElement; const input = document.createElement("input"); input.type = "checkbox"; input.checked = checked; input.addEventListener("change", () => changed(input.checked)); wrapper.append(input, document.createTextNode(label)); return wrapper; }
function el<K extends keyof HTMLElementTagNameMap>(tag: K, className?: string): HTMLElementTagNameMap[K] { const value = document.createElement(tag); if (className !== undefined) value.className = className; return value; }
function text<K extends keyof HTMLElementTagNameMap>(tag: K, value: string, className?: string): HTMLElementTagNameMap[K] { const node = el(tag, className); node.textContent = value; return node; }
