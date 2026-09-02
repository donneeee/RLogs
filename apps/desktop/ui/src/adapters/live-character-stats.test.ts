import { describe, expect, it } from "vitest";

import {
  formatFightAttributeValue,
  parseFightAttributePresentationCatalog,
  parseLiveCharacterStatsSnapshot,
  resolveLiveCharacterStatFamilies,
  type FightAttributePresentationCatalog,
  type LiveCharacterStatsSnapshot,
} from "./live-character-stats";

const CATALOG = {
  schema_version: 1,
  game_build: "24687926",
  locale: "en-US",
  source: "test",
  source_sha256: "a".repeat(64),
  attributes: [
    attribute(11_330, 11_330, "final", "ATK", 0, 0),
    attribute(11_331, 11_330, "total", "ATK", 0, 1),
    attribute(11_334, 11_330, "percent", "ATK", 0, 4),
    { ...attribute(20_050, 20_050, "final", "AttrLevel", 0, 0), displayable: false },
  ],
} satisfies FightAttributePresentationCatalog;

const SNAPSHOT = {
  schema_version: 1,
  revision: 4,
  character: null,
  snapshot_values: { "11330": 10_000, "11331": 9_500, "11334": 125, "20050": 99 },
  current_values: { "11330": 11_000, "11331": 9_500, "11334": 125, "20050": 99 },
  last_event_sequence: 42,
  last_game_time_millis: 1_000,
} satisfies LiveCharacterStatsSnapshot;

describe("live character stats", () => {
  it("validates the host catalog and snapshot boundaries", () => {
    expect(parseFightAttributePresentationCatalog(CATALOG).game_build).toBe("24687926");
    expect(parseLiveCharacterStatsSnapshot(SNAPSHOT).revision).toBe(4);
    expect(() => parseLiveCharacterStatsSnapshot({ ...SNAPSHOT, current_values: { raw: 1 } }))
      .toThrow("invalid live character-stat snapshot");
  });

  it("groups exact components, hides internal attributes, and promotes temporary changes", () => {
    const families = resolveLiveCharacterStatFamilies(SNAPSHOT, CATALOG);
    expect(families).toHaveLength(1);
    expect(families[0]).toMatchObject({ familyId: 11_330, name: "ATK", changed: true });
    expect(families[0]?.components.map((component) => component.presentation.component))
      .toEqual(["final", "total", "percent"]);
    expect(families[0]?.components[0]).toMatchObject({ snapshotValue: 10_000, currentValue: 11_000 });
  });

  it("uses the same percent and time formatting rules as the website profile", () => {
    expect(formatFightAttributeValue(1_250, 1, 0)).toBe("12.5%");
    expect(formatFightAttributeValue(350, 0, 4)).toBe("3.5%");
    expect(formatFightAttributeValue(2_500, 2, 0)).toBe("2.5s");
    expect(formatFightAttributeValue(12_345, 0, 0)).toBe("12,345");
  });
});

function attribute(
  attribute_id: number,
  family_id: number,
  component: "final" | "total" | "add" | "extra_add" | "percent" | "extra_percent",
  name: string,
  number_type: number,
  format_type: number,
) {
  return {
    attribute_id,
    family_id,
    component,
    name,
    description: `${name} description`,
    number_type,
    format_type,
    icon: null,
    displayable: true,
  } as const;
}
