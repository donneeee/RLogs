import { describe, expect, it } from "vitest";

import {
  parseProfilePackageInspection,
  parseProfilePackageStore,
  parseProfilePublishResult,
  parseProfileProjectionResult,
} from "./profile-packages";

function entry() {
  return {
    package_id: "a".repeat(64),
    created_unix_millis: 10,
    local_package_path:
      "C:/rLogs/runtime-data/profile-sync/packages/game/global/region/realm/123/current.profile.json",
    package_byte_length: 100,
    game_plugin_id: "app.rlogs.game.blue-protocol-star-resonance",
    deployment: "global",
    region: "north-america",
    realm: "asteria",
    world: null,
    character_id: "123",
    display_name: "MarieRose",
    server_id: "7",
    class_id: 5,
    specialization_id: null,
    level: 60,
    profile_field_count: 8,
    source_session_id: "session-1",
    source_client_build: "steam-24252055",
    source_observation_count: 3,
    source_last_event_sequence: 9,
  };
}

describe("profile package contracts", () => {
  it("validates bounded inventory totals", () => {
    const store = parseProfilePackageStore({
      schema_version: 1,
      package_root: "C:/rLogs/runtime-data/profile-sync/packages",
      entry_count: 1,
      total_package_bytes: 100,
      entries: [entry()],
      issues: [],
    });
    expect(store.entries[0]?.character_id).toBe("123");
  });

  it("validates exact package inspection without credentials", () => {
    const inspection = parseProfilePackageInspection({
      schema_version: 1,
      local_package_path: entry().local_package_path,
      package_byte_length: 100,
      package: {
        schema_version: 1,
        package_id: "a".repeat(64),
        created_unix_millis: 10,
        source: {
          session_id: "session-1",
          client_build: "steam-24252055",
          protocol_pack_digest: "sha256:pack",
          canonical_content_sha256: `sha256:${"b".repeat(64)}`,
          observation_count: 3,
          last_event_sequence: 9,
        },
        request: {
          relative_endpoint:
            "/v1/games/blue-protocol-star-resonance/profiles",
          payload: {
            schema_version: 1,
            game_plugin_id:
              "app.rlogs.game.blue-protocol-star-resonance",
            payload_kind: "character-profile",
            payload_schema_id: "app.rlogs.bpsr.character-profile",
            payload_schema_version: 1,
            routing: {
              deployment: "global",
              region: "north-america",
              "character-id": "123",
            },
            body: { display_name: "MarieRose", level: 60 },
          },
        },
      },
    });
    expect(inspection.package).not.toHaveProperty("credentials");
  });

  it("requires projection to report zero external requests", () => {
    const result = parseProfileProjectionResult({
      schema_version: 1,
      source_session_id: "session-1",
      projected_package_count: 1,
      stored_packages: [entry()],
      external_network_requests: 0,
    });
    expect(result.projected_package_count).toBe(1);
    expect(() =>
      parseProfileProjectionResult({
        ...result,
        external_network_requests: 1,
      }),
    ).toThrow("invalid profile projection");
  });

  it("validates an authenticated UID claim receipt with module counts", () => {
    const result = parseProfilePublishResult({
      schema_version: 1,
      profile_id: `prf_${"a".repeat(32)}`,
      character_id: "123",
      package_id: "b".repeat(64),
      claimed: true,
      duplicate: false,
      module_inventory_count: 649,
      equipped_module_count: 5,
      profile_url: `https://example.test/?profile=prf_${"a".repeat(32)}#profile-lab`,
    });
    expect(result.module_inventory_count).toBe(649);
  });
});
