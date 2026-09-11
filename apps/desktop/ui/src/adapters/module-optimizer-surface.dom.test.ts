// @vitest-environment happy-dom
import { afterEach, describe, expect, it, vi } from "vitest";

import type {
  GpuSupport,
  LocalModuleInventory,
  OptimizerCatalog,
} from "./module-optimizer";
import { mountModuleOptimizerSurface } from "./module-optimizer-surface";

afterEach(() => {
  document.body.replaceChildren();
  localStorage.clear();
});

describe("desktop module optimizer surface", () => {
  it("keeps the equipped score visible when every effect has zero Link", async () => {
    const catalog: OptimizerCatalog = {
      game_id: "blue-protocol-star-resonance",
      catalog_revision: "reviewed-catalog",
      scoring_revision: "reviewed-scoring",
      client_builds: ["24252055"],
      attributes: [{
        id: 1110,
        name: "Module effect 1110",
        official_name: null,
        icon: null,
        thresholds: [1],
        fight_values: [1],
      }],
      link_power: [0],
      combination_sizes: [4, 5],
      default_max_solutions: 5,
    };
    const inventory: LocalModuleInventory = {
      schema_version: 1,
      characters: [{
        package_id: "package",
        character_id: "character",
        display_name: "Zero Link",
        deployment: "global",
        region: "na",
        source_client_build: "24687926",
        source_protocol_pack_digest: "sha256:pack",
        observed_unix_millis: 1_788_313_443_000,
        modules: [{
          instance_id: "equipped",
          config_id: 5_500_101,
          quality: 2,
          parts: [{ part_id: 1110, initial_link_points: 0 }],
        }],
        current_instance_ids: ["equipped"],
        module_snapshot_available: true,
        module_snapshot_detail: "1 owned module · 1 equipped",
        module_presentation: {
          schema_version: 1,
          locale: "en-US",
          deployment_id: "global",
          client_build: "24687926",
          protocol_pack_digest: "sha256:pack",
          modules: { "5500101": "Basic Attack Module" },
          module_effects: { "1110": "Strength Boost" },
        },
      }],
      issues: [],
    };
    const gpu: GpuSupport = {
      available: false,
      backend: "cpu",
      device_name: null,
      vendor: null,
      detail: "CPU only",
    };
    const container = document.createElement("div");
    document.body.append(container);
    const mounted = mountModuleOptimizerSurface(container, {
      loadCatalog: async () => catalog,
      loadInventory: async () => inventory,
      loadGpuSupport: async () => gpu,
      optimize: vi.fn(),
    });

    await vi.waitFor(() => {
      expect(container.querySelector(".module-runtime-status")?.textContent)
        .toContain("ordinary builds carry forward");
    });
    const summary = container.querySelector<HTMLElement>(
      ".module-current-card > .module-loadout-link-summary",
    );
    expect(summary?.hidden).toBe(false);
    expect(summary?.querySelector(".module-score-summary")?.textContent).toBe("Score 0");
    expect(summary?.querySelectorAll(".module-effect-chip")).toHaveLength(0);
    expect(container.querySelector(".module-attribute-identity strong")?.textContent)
      .toBe("Strength Boost");
    expect(container.querySelector(".module-minimum-input")?.getAttribute("aria-label"))
      .toBe("Minimum Strength Boost Link");

    mounted.dispose();
  });
});
