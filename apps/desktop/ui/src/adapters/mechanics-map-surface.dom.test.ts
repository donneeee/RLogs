// @vitest-environment happy-dom

import { afterEach, describe, expect, it, vi } from "vitest";

import { mountMechanicsMapSurface } from "./mechanics-map-surface";
import type { MechanicsMapUpdate } from "./mechanics-map";

afterEach(() => {
  document.body.replaceChildren();
  vi.unstubAllGlobals();
});

function mapUpdate(revision: number, supported: boolean): MechanicsMapUpdate {
  return {
    schema_version: 14,
    revision,
    snapshot: {
      schema_version: 14, revision, session_id: "session", client_build: "24687926",
      scene_id: supported ? 1151 : 9999, map_id: supported ? 1151 : 9999,
      scene_name: supported ? "Void Towering Ruin" : "Unsupported scene",
      map_model: supported ? "absolute_scene_map" : "player_relative_radar", map_layout: null,
      world_radius: 140, map_origin_x: supported ? 0 : null, map_origin_z: supported ? 0 : null,
      map_span_x: supported ? 100 : null, map_span_z: supported ? 100 : null,
      background_asset_url: supported ? "/local-game-assets/24687926/reviewed.png" : null,
      local_actor_id: 1, local_position_observed: true, player: null, party: [], action_controls: [],
      resources: [], encounter_pack: supported ? "Void Towering Ruin" : null,
      encounter_pack_reviewed: supported, target: null, dungeon: null,
      entities: [{
        actor_id: 1, entity_uuid: 1, kind: "local", display_name: "Player", monster_id: null,
        mechanic_role: null, x: 50, y: 0, z: 50, facing_radians: 0, dead: false, stale: false,
        last_observed_micros: 1_000,
      }],
      mechanics: [{
        effect_id: 821076, mechanic_kind: "sticky_bomb", presentation_name: "Sticky bomb",
        instance_id: 1, target_actor_id: 1, source_actor_id: null, stacks: null,
        duration_millis: 5_000, origin_x: null, origin_z: null, facing_radians: null,
        applied_at_micros: 1_000,
      }],
      markers: [{ marker_id: 7, marker_number: 1, related_actor_id: 1, x: 50, y: 0, z: 50 }],
      data_gap: null, last_event_sequence: 1, last_observed_micros: 1_000,
    },
  };
}

describe("Mechanics Map overlay launch", () => {
  it("keeps native launch failures visible even without a missing-map notice", async () => {
    const container = document.createElement("main");
    document.body.append(container);
    const openOverlay = vi.fn().mockRejectedValue(new Error(
      "show_overlay_canvas_editable not allowed by capability",
    ));
    const mounted = mountMechanicsMapSurface(container, {
      loadSnapshot: () => new Promise(() => undefined),
      waitForSnapshot: () => new Promise(() => undefined),
      prepareLocalMaps: vi.fn(),
      openOverlay,
    });

    container.querySelector<HTMLButtonElement>(".mechanics-map-open-overlay")!.click();
    expect(container.querySelector("[role=status]")?.textContent).toBe("Opening the map overlay…");
    await vi.waitFor(() => {
      expect(container.querySelector("[role=status]")?.textContent)
        .toBe("show_overlay_canvas_editable not allowed by capability");
    });
    expect(container.querySelector<HTMLElement>("[role=status]")?.dataset.state).toBe("error");
    expect(container.querySelector<HTMLButtonElement>(".mechanics-map-open-overlay")?.disabled).toBe(false);
    expect(openOverlay).toHaveBeenCalledOnce();
    mounted.dispose();
  });

  it("reports each successful first-open and reopen request", async () => {
    const container = document.createElement("main");
    document.body.append(container);
    const openOverlay = vi.fn().mockResolvedValue(undefined);
    const mounted = mountMechanicsMapSurface(container, {
      loadSnapshot: () => new Promise(() => undefined),
      waitForSnapshot: () => new Promise(() => undefined),
      prepareLocalMaps: vi.fn(),
      openOverlay,
    });
    const button = container.querySelector<HTMLButtonElement>(".mechanics-map-open-overlay")!;

    button.click();
    await vi.waitFor(() => expect(button.disabled).toBe(false));
    expect(container.querySelector("[role=status]")?.textContent).toContain("Map overlay opened");
    button.click();
    await vi.waitFor(() => expect(openOverlay).toHaveBeenCalledTimes(2));
    await vi.waitFor(() => {
      expect(container.querySelector("[role=status]")?.textContent).toContain("Map overlay opened");
    });
    mounted.dispose();
  });

  it("clears all map pixels and annotations when a ready scene becomes unsupported", async () => {
    const images: Array<{ onload: (() => void) | null; onerror: (() => void) | null; src: string }> = [];
    vi.stubGlobal("Image", class {
      onload: (() => void) | null = null;
      onerror: (() => void) | null = null;
      src = "";
      constructor() { images.push(this); }
    });
    let resolveTransition!: (update: MechanicsMapUpdate) => void;
    const transition = new Promise<MechanicsMapUpdate>((resolve) => { resolveTransition = resolve; });
    let waited = false;
    const container = document.createElement("main");
    document.body.append(container);
    const mounted = mountMechanicsMapSurface(container, {
      loadSnapshot: async () => mapUpdate(1, true),
      waitForSnapshot: () => {
        if (!waited) { waited = true; return transition; }
        return new Promise(() => undefined);
      },
      prepareLocalMaps: vi.fn(),
      openOverlay: vi.fn(),
    });
    await vi.waitFor(() => expect(images).toHaveLength(1));
    images[0]!.onload?.();
    await vi.waitFor(() => {
      expect(container.querySelectorAll(".mechanics-map-point").length).toBeGreaterThan(0);
      expect(container.querySelectorAll(".mechanics-map-void-annotation").length).toBeGreaterThan(0);
    });

    resolveTransition(mapUpdate(2, false));
    await vi.waitFor(() => {
      expect(container.querySelectorAll(".mechanics-map-point")).toHaveLength(0);
      expect(container.querySelectorAll(".mechanics-map-marker")).toHaveLength(0);
      expect(container.querySelectorAll(".mechanics-map-void-annotation")).toHaveLength(0);
      expect(container.querySelectorAll(".mechanics-map-regions > *")).toHaveLength(0);
      expect(container.querySelector(".mechanics-map-empty")?.textContent).toContain("Map unavailable");
    });
    expect(container.querySelector<HTMLElement>(".mechanics-radar")?.dataset.assetState).toBe("none");
    mounted.dispose();
  });
});
