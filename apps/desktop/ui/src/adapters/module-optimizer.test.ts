import { describe, expect, it } from "vitest";

import {
  modulePresentation,
  parseGpuSupport,
  parseLocalModuleInventory,
  parseOptimizeResponse,
  parseOptimizerCatalog,
} from "./module-optimizer";
import { moduleSolutionScoreSummary, summarizeModuleLinks } from "./module-optimizer-surface";

describe("local module optimizer contracts", () => {
  it("shows the score and only adds a priority score when preferences change ranking", () => {
    const solution = {
      instance_ids: [],
      modules: [],
      score: 420,
      ranking_score: 420,
      breakdown: {
        threshold_power: 0,
        ranking_threshold_power: 0,
        total_link_points: 0,
        total_link_power: 0,
        attributes: [],
      },
    };
    expect(moduleSolutionScoreSummary(solution)).toBe("Score 420");
    expect(moduleSolutionScoreSummary({ ...solution, ranking_score: 460 })).toBe(
      "Score 420 · Priority 460",
    );
  });

  it("combines effect links across a loadout without exposing power or thresholds", () => {
    const catalog = parseOptimizerCatalog({
      game_id: "blue-protocol-star-resonance",
      catalog_revision: "sha256:test",
      scoring_revision: "reviewed",
      client_builds: ["24687926"],
      attributes: [
        { id: 1110, name: "Strength Boost", official_name: null, icon: null, thresholds: [4], fight_values: [1] },
        { id: 1120, name: "Agility Boost", official_name: null, icon: null, thresholds: [4], fight_values: [1] },
      ],
      combination_sizes: [5],
      default_max_solutions: 5,
    });
    expect(summarizeModuleLinks([
      { instance_id: "1", config_id: 1, quality: 4, parts: [{ part_id: 1110, initial_link_points: 10 }, { part_id: 1120, initial_link_points: 4 }] },
      { instance_id: "2", config_id: 1, quality: 4, parts: [{ part_id: 1110, initial_link_points: 5 }] },
    ], catalog)).toEqual([
      { attributeId: 1110, name: "Strength Boost", icon: null, totalLink: 15 },
      { attributeId: 1120, name: "Agility Boost", icon: null, totalLink: 4 },
    ]);
  });

  it("keeps module instance IDs as strings beyond JavaScript's safe integer range", () => {
    const inventory = parseLocalModuleInventory({
      schema_version: 1,
      characters: [
        {
          package_id: "a".repeat(64),
          character_id: "3296036",
          display_name: "MarieRose",
          deployment: "global",
          region: "na",
          source_client_build: "24687926",
          observed_unix_millis: 1_788_313_443_000,
          modules: [
            {
              instance_id: "9007199254740993",
              config_id: 5_500_104,
              quality: 4,
              parts: [{ part_id: 1110, initial_link_points: 20 }],
            },
          ],
          current_instance_ids: ["9007199254740993"],
          module_snapshot_available: true,
          module_snapshot_detail: "1 owned module · 1 equipped",
        },
      ],
      issues: [],
    });
    expect(inventory.characters[0]?.modules[0]?.instance_id).toBe(
      "9007199254740993",
    );
  });

  it("accepts the reviewed native optimizer catalog", () => {
    const catalog = parseOptimizerCatalog({
      game_id: "blue-protocol-star-resonance",
      catalog_revision: "sha256:test",
      scoring_revision: "cn-pinned",
      client_builds: ["24687926"],
      attributes: [
        {
          id: 1110,
          name: "Strength",
          official_name: "Strength Boost",
          icon: "icons/modules/effects/1110-strength-boost.png",
          thresholds: [1, 4, 8, 12, 16, 20],
          fight_values: [7, 14, 29, 44, 167, 254],
        },
      ],
      combination_sizes: [4, 5],
      default_max_solutions: 10,
    });
    expect(catalog.attributes[0]?.name).toBe("Strength");
  });

  it("presents module cards with localized names and bundled icons", () => {
    const presentation = modulePresentation({
      instance_id: "1",
      config_id: 5_500_204,
      quality: 4,
      parts: [],
    });
    expect(presentation.name).toBe("Excellent Support Module - Premium");
    expect(presentation.icon).toContain("item_icons_mod_device_5.png");
  });

  it("accepts a dynamically discovered cross-vendor GPU", () => {
    const support = parseGpuSupport({
      available: true,
      backend: "open_cl",
      device_name: "AMD Radeon RX 7900 XTX",
      vendor: "Advanced Micro Devices, Inc.",
      detail: "OpenCL exact search is ready.",
    });
    expect(support.backend).toBe("open_cl");
    expect(support.device_name).toContain("Radeon");
  });

  it("requires optimizer results to disclose the engine and fallback state", () => {
    const result = parseOptimizeResponse({
      scoring_revision: "reviewed",
      catalog_revision: "catalog",
      current_setup: null,
      solutions: [],
      search: {
        requested_mode: "auto",
        used_mode: "exact",
        exact: true,
        input_module_count: 12,
        candidate_module_count: 10,
        excluded_module_count: 2,
        total_combinations: 210,
        evaluated_states: 210,
        combination_size: 4,
        beam_width: null,
        backend: "open_cl",
        accelerator_name: "GeForce RTX 5060",
        accelerator_fallback: null,
      },
    });
    expect(result.search.backend).toBe("open_cl");
    expect(result.search.accelerator_fallback).toBeNull();
  });

  it("accepts the disclosed cross-vendor CPU and OpenCL hybrid", () => {
    const result = parseOptimizeResponse({
      scoring_revision: "reviewed",
      catalog_revision: "catalog",
      current_setup: null,
      solutions: [],
      search: {
        requested_mode: "auto",
        used_mode: "beam",
        exact: false,
        input_module_count: 922,
        candidate_module_count: 218,
        excluded_module_count: 704,
        total_combinations: 89_000_000,
        evaluated_states: 3_400_000,
        combination_size: 4,
        beam_width: 2_048,
        backend: "cpu_open_cl_hybrid",
        accelerator_name: "AMD Radeon RX 7900 XTX",
        accelerator_fallback: null,
      },
    });
    expect(result.search.backend).toBe("cpu_open_cl_hybrid");
    expect(result.search.used_mode).toBe("beam");
  });
});
