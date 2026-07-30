import { describe, expect, it } from "vitest";

import { parseEventViewerPage } from "./event-viewer";

function examplePage(): Record<string, unknown> {
  return {
    schemaVersion: 1,
    queryId: "events-1",
    sessionId: "fixture",
    artifactDigest: "sha256:fixture",
    header: {
      schema_version: 1,
      event_schema_version: 1,
      session_id: "fixture",
      producer: "test",
      region: {
        identity: {
          deployment_id: "global",
          region_id: "north-america",
          realm_id: null,
          world_id: "asteria",
        },
        client_build: "build",
        protocol_pack_digest: "sha256:pack",
      },
    },
    filter: { topic: "combat", kind: "damage", search: null },
    pageIndex: 1,
    scannedThisPage: 1,
    scannedTotal: 1,
    matchedTotal: 1,
    integrityVerified: true,
    complete: true,
    events: [
      {
        sequence: 1,
        timelineSequence: 1,
        observedMicros: 42,
        gameTimeMillis: null,
        topic: "combat",
        kind: "damage",
        summary:
          "entity:9223372036854775807 [actor:1] -> entity:-9223372036854775808 [actor:2]",
        amount: "9223372036854775807",
        identifiers: {
          actor: null,
          source: {
            actorId: "1",
            entityUuid: "9223372036854775807",
          },
          directSource: null,
          target: {
            actorId: "2",
            entityUuid: "-9223372036854775808",
          },
          ability: "9007199254740993",
          status: null,
          monster: null,
          scene: null,
          map: null,
          dungeon: null,
          characterId: null,
        },
        canonicalJson:
          '{"sequence":1,"event":{"type":"timeline","data":{"entity_uuid":9223372036854775807}}}',
      },
    ],
  };
}

describe("Event Viewer page contract", () => {
  it("preserves 64-bit canonical values as strings", () => {
    const page = parseEventViewerPage(examplePage());

    expect(page.events[0]?.identifiers.source?.entityUuid).toBe(
      "9223372036854775807",
    );
    expect(page.events[0]?.identifiers.ability).toBe("9007199254740993");
    expect(page.events[0]?.canonicalJson).toContain("9223372036854775807");
  });

  it("rejects numeric identifiers that JavaScript could round", () => {
    const page = examplePage();
    const events = page.events as Array<Record<string, unknown>>;
    const identifiers = events[0]?.identifiers as Record<string, unknown>;
    identifiers.ability = 9_007_199_254_740_992;

    expect(() => parseEventViewerPage(page)).toThrow(
      "invalid Event Viewer page",
    );
  });
});
