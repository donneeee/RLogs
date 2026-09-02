import { describe, expect, it } from "vitest";

import {
  modulePresentation,
  parseLocalModuleInventory,
  parseOptimizerCatalog,
} from "./module-optimizer";

describe("local module optimizer contracts", () => {
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
});
