import { describe, expect, it } from "vitest";

import {
  CUSTOM_TRIGGER_DRAFT_STORAGE_KEY,
  createCustomTriggerDraft,
  readStoredDrafts,
  storeCustomTriggerDraft,
} from "./custom-trigger-draft";
import type { LiveEventDetail } from "./event-viewer";

const DETAIL: LiveEventDetail = {
  schemaVersion: 1,
  sessionId: "capture-1",
  revision: 14,
  sequence: 9,
      timelineSequence: 7,
      observedMicros: 12_345,
      sourceKind: "canonical",
      gameTimeMillis: null,
  topic: "combat",
      kind: "damage",
      protocolCaptureSequence: 88,
      protocol: null,
      fields: [
    {
      path: "event.kind",
      label: "Event kind",
      value: "damage",
      valueType: "enum",
      usableInTrigger: true,
    },
    {
      path: "event.ability_id",
      label: "Ability ID",
      value: "2203521",
      valueType: "u32",
      usableInTrigger: true,
    },
    {
      path: "provenance.source.connection_id",
      label: "Connection ID",
      value: "4",
      valueType: "u64",
      usableInTrigger: false,
    },
  ],
};

describe("custom trigger drafts", () => {
  it("copies only explicitly selected trigger-safe fields", () => {
    const draft = createCustomTriggerDraft(
      DETAIL,
      new Set([
        "event.kind",
        "event.ability_id",
        "provenance.source.connection_id",
      ]),
      100,
    );

    expect(draft.enabled).toBe(false);
    expect(draft.when.eventKind).toBe("damage");
    expect(draft.when.criteria.map((criterion) => criterion.path)).toEqual([
      "event.kind",
      "event.ability_id",
    ]);
    expect(draft.actions).toEqual([]);
  });

  it("stores disabled drafts in a bounded local collection", () => {
    const values = new Map<string, string>();
    const storage = {
      getItem(key: string) {
        return values.get(key) ?? null;
      },
      setItem(key: string, value: string) {
        values.set(key, value);
      },
    };
    const draft = createCustomTriggerDraft(DETAIL, new Set(["event.kind"]), 100);

    storeCustomTriggerDraft(storage, draft);

    expect(values.has(CUSTOM_TRIGGER_DRAFT_STORAGE_KEY)).toBe(true);
    expect(readStoredDrafts(storage)).toEqual([draft]);
  });
});
