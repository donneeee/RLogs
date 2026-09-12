import { describe, expect, it } from "vitest";
import {
  AUTOMARKER_EXCHANGE_MAX_BYTES,
  automarkerExportFilename,
  automarkerActivationRequest,
  automarkerPresetExchange,
  automarkerSaveRequest,
  activeAutomarkerPreview,
  automarkerResponseIsCurrent,
  newlyCreatedPresetId,
  parseAutomarkerLocalLoadResult,
  parseAutomarkerNativeActivationResult,
  parseAutomarkerPreview,
  parseAutomarkerPresetExchange,
  parseAutomarkerPresetView,
  parseObservedMarkerSnapshot,
  observedMarkersMatchPresetView,
  previewMatchesContext,
  publishAutomarkerPreview,
  readActiveAutomarkerPreview,
  serializeAutomarkerPresetExchange,
} from "./automarker-presets";
import { automarkerPresetContextKey } from "./automarker-presets-surface";

function view() {
  return {
    schemaVersion: 4 as const,
    context: { clientBuild: "24687926", sceneId: 1633, mapId: 1633, activityFamilyId: "dungeon.1633", sceneName: "Tina M1" },
    presets: [{
      presetId: "preset-000000000001-0000", name: "Opener",
      activityFamilyId: "dungeon.1633", savedAtUnixMillis: 1,
      points: [{ markerNumber: 1, x: 1.25, y: 2.5, z: -4.75 }],
    }],
    captureSupported: false,
    captureReason: "native_waymark_state_unverified",
    captureSessionId: null,
    deploymentId: null,
    protocolPackDigest: null,
    nativeLoadSupported: false,
    nativeLoadReason: "native_waymark_transport_unavailable",
    previewSessionId: "preview-test-session",
  };
}

describe("automarker preset catalog", () => {
  it("builds the exact bounded native activation request and rejects response expansion", () => {
    const catalog = parseAutomarkerPresetView(view());
    if (catalog.context === null) throw new Error("fixture context missing");
    const request = automarkerActivationRequest(catalog.presets[0]!.presetId, parseAutomarkerPresetView(catalog));
    expect(Object.keys(request).sort()).toEqual(["expectedContext", "presetId"]);
    expect(Object.keys(request.expectedContext).sort()).toEqual(["activityFamilyId", "clientBuild", "mapId", "sceneId", "sceneName"]);
    expect(JSON.stringify(request)).not.toMatch(/account|character|player|entity|actionUuid|skillUuid|sequence|timestamp|auth|positionSource|rawPayload/i);
    expect(parseAutomarkerNativeActivationResult({ activated: false, reason: "native_waymark_transport_unavailable" }))
      .toEqual({ activated: false, reason: "native_waymark_transport_unavailable" });
    expect(() => parseAutomarkerNativeActivationResult({ activated: false, reason: "native_waymark_transport_unavailable", sequence: 7 }))
      .toThrow(/invalid automarker activation result/i);
    expect(() => automarkerActivationRequest(catalog.presets[0]!.presetId, { ...catalog, context: null }))
      .toThrow(/matching current automarker scene/i);
  });

  it("round-trips an identity-free portable exchange document", () => {
    const preset = view().presets[0]!;
    const serialized = serializeAutomarkerPresetExchange(preset);
    const raw = JSON.parse(serialized) as Record<string, unknown>;
    expect(Object.keys(raw).sort()).toEqual(["activityFamilyId", "kind", "name", "points", "version"]);
    expect(serialized).not.toMatch(/presetId|savedAt|clientBuild|sceneId|mapId|session|uuid/i);
    expect(parseAutomarkerPresetExchange(serialized, "dungeon.1633")).toEqual(automarkerPresetExchange(preset));
  });

  it("rejects unknown identity fields, another family, malformed JSON, and oversized exchange files", () => {
    const portable = automarkerPresetExchange(view().presets[0]!);
    for (const field of ["accountUuid", "characterUuid", "playerUuid", "entityUuid", "sessionId", "skillUuid", "presetId", "savedAtUnixMillis", "clientBuild", "sceneId", "mapId"]) {
      expect(() => parseAutomarkerPresetExchange(JSON.stringify({ ...portable, [field]: "private" }), "dungeon.1633"), field)
        .toThrow(/invalid|unsupported/i);
    }
    expect(() => parseAutomarkerPresetExchange(JSON.stringify(portable), "mech-facility"))
      .toThrow(/another dungeon family/i);
    expect(() => parseAutomarkerPresetExchange("{", "dungeon.1633")).toThrow(/valid JSON/i);
    expect(() => parseAutomarkerPresetExchange(" ".repeat(AUTOMARKER_EXCHANGE_MAX_BYTES + 1), "dungeon.1633"))
      .toThrow(/64 KiB/i);
  });

  it("creates a bounded filesystem-safe export filename", () => {
    expect(automarkerExportFilename("../../Tína: M1? * opener")).toBe("Tina-M1-opener.rlogs-automarker.json");
    expect(automarkerExportFilename("CON")).toBe("automarker-preset.rlogs-automarker.json");
    expect(automarkerExportFilename("CON.txt")).toBe("automarker-preset.rlogs-automarker.json");
    expect(automarkerExportFilename("lpt1.setup")).toBe("automarker-preset.rlogs-automarker.json");
    expect(automarkerExportFilename("💥")).toBe("automarker-preset.rlogs-automarker.json");
    expect(automarkerExportFilename("x".repeat(200)).length).toBeLessThanOrEqual(64 + ".rlogs-automarker.json".length);
  });

  it("accepts only a fully stamped, internally consistent observed marker snapshot", () => {
    const value = {
      ...view(),
      captureSupported: true,
      captureReason: "observed_waymark_state_verified" as const,
      captureSessionId: "capture-session-1",
      deploymentId: "global",
      protocolPackDigest: `sha256:${"a".repeat(64)}`,
    };
    const snapshot = parseObservedMarkerSnapshot({
      schemaVersion: 2,
      revision: 7,
      captureActive: true,
      protocolSupported: true,
      requestObserverSupported: true,
      verifiedRequestCount: 1,
      lastVerifiedRequestMarkerNumber: 1,
      lastVerifiedRequestObservedMicros: 98,
      reason: "observed_markers_available",
      sessionId: value.captureSessionId,
      deploymentId: value.deploymentId,
      clientBuild: value.context.clientBuild,
      protocolPackDigest: value.protocolPackDigest,
      sceneId: value.context.sceneId,
      mapId: value.context.mapId,
      observedMicros: 99,
      markers: [{ markerNumber: 1, x: 1, y: 2, z: 3 }],
    });
    expect(observedMarkersMatchPresetView(snapshot, parseAutomarkerPresetView(value))).toBe(true);
    expect(observedMarkersMatchPresetView(
      { ...snapshot, sessionId: "stale-session" },
      parseAutomarkerPresetView(value),
    )).toBe(false);
  });

  it("rejects partial identities, duplicate markers, and capability contradictions", () => {
    const base = {
      schemaVersion: 2,
      revision: 7,
      captureActive: true,
      protocolSupported: true,
      requestObserverSupported: true,
      verifiedRequestCount: 1,
      lastVerifiedRequestMarkerNumber: 1,
      lastVerifiedRequestObservedMicros: 98,
      reason: "observed_markers_available",
      sessionId: "capture-session-1",
      deploymentId: "global",
      clientBuild: "24687926",
      protocolPackDigest: `sha256:${"a".repeat(64)}`,
      sceneId: 1_633,
      mapId: 1_633,
      observedMicros: 99,
      markers: [{ markerNumber: 1, x: 1, y: 2, z: 3 }],
    };
    expect(() => parseObservedMarkerSnapshot({ ...base, protocolPackDigest: null })).toThrow(/invalid|inconsistent/i);
    expect(() => parseObservedMarkerSnapshot({ ...base, markers: [...base.markers, { ...base.markers[0] }] }))
      .toThrow(/invalid/i);
    expect(() => parseObservedMarkerSnapshot({ ...base, protocolSupported: false })).toThrow(/inconsistent/i);
    expect(() => parseObservedMarkerSnapshot({ ...base, verifiedRequestCount: 0 })).toThrow(/inconsistent/i);
    expect(() => parseObservedMarkerSnapshot({ ...base, lastVerifiedRequestMarkerNumber: 7 })).toThrow(/invalid/i);

    const inconsistentView = view();
    inconsistentView.captureSupported = true;
    expect(() => parseAutomarkerPresetView(inconsistentView)).toThrow(/inconsistent/i);
  });

  it("refreshes scene provenance when difficulty or build changes within a family", () => {
    const original = view();
    const difficultyChange = view();
    difficultyChange.context.sceneId = 1631;
    difficultyChange.context.mapId = 1631;
    difficultyChange.context.activityFamilyId = "tina-mindrealm";
    const buildChange = view();
    buildChange.context.clientBuild = "24699999";
    const hostRestart = view();
    hostRestart.previewSessionId = "preview-next-session";

    expect(automarkerPresetContextKey(difficultyChange)).not.toBe(automarkerPresetContextKey(original));
    expect(automarkerPresetContextKey(buildChange)).not.toBe(automarkerPresetContextKey(original));
    expect(automarkerPresetContextKey(hostRestart)).not.toBe(automarkerPresetContextKey(original));
  });

  it("accepts compact family-scoped filesystem preset views", () => {
    expect(parseAutomarkerPresetView(view()).presets[0]?.points[0]?.y).toBe(2.5);
  });

  it("rejects Tina master presets in unrelated Tina scene families", () => {
    const value = view();
    value.context.sceneId = 1631;
    value.context.mapId = 1631;
    value.context.activityFamilyId = "tina-mindrealm";
    expect(() => parseAutomarkerPresetView(value)).toThrow(/another dungeon family/i);
  });

  it("lets the editor accept only the newest asynchronous catalog request", () => {
    expect(automarkerResponseIsCurrent(4, 4)).toBe(true);
    expect(automarkerResponseIsCurrent(3, 4)).toBe(false);
  });

  it("fails closed when the native host leaks a preset from another dungeon family", () => {
    const value = view();
    value.presets[0]!.activityFamilyId = "mech-facility";
    expect(() => parseAutomarkerPresetView(value)).toThrow(/another dungeon family/i);
  });

  it("keeps presets portable across build, scene, and map changes within a family", () => {
    const value = view();
    value.context.clientBuild = "24699999";
    value.context.sceneId = 9_999;
    value.context.mapId = 8_888;
    expect(parseAutomarkerPresetView(value).presets).toHaveLength(1);
  });

  it("rejects identity, session, and provenance fields in a portable preset", () => {
    for (const [field, fieldValue] of Object.entries({
      accountUuid: "account", characterUuid: "character", playerUuid: "player",
      entityUuid: "entity", skillUuid: 123, sessionSequence: 7,
      clientBuild: "24687926", sceneId: 1633, mapId: 1633,
    })) {
      const value = view() as unknown as { presets: Array<Record<string, unknown>> };
      value.presets[0]![field] = fieldValue;
      expect(() => parseAutomarkerPresetView(value), field).toThrow(/invalid automarker preset catalog/i);
    }
    const pointValue = view() as unknown as { presets: Array<{ points: Array<Record<string, unknown>> }> };
    pointValue.presets[0]!.points[0]!.entityUuid = "entity";
    expect(() => parseAutomarkerPresetView(pointValue)).toThrow(/invalid automarker preset catalog/i);
  });

  it("accepts only a family-compatible local load response", () => {
    const value = { context: view().context, preset: view().presets[0] };
    expect(parseAutomarkerLocalLoadResult(value)).toEqual(value);
    expect(() => parseAutomarkerLocalLoadResult({
      context: { ...view().context, activityFamilyId: "mech-facility" },
      preset: view().presets[0],
    })).toThrow(/invalid local automarker preset/i);
    expect(() => parseAutomarkerLocalLoadResult({ supported: false, reason: "native_waymark_transport_unavailable" })).toThrow();
  });

  it("distinguishes Save overwrite from Save As creation", () => {
    const points = [{ markerNumber: 2, x: 1, y: 2, z: 3 }];
    expect(automarkerSaveRequest("save", "preset-existing", " Adjusted ", points, view().context)).toEqual({
      presetId: "preset-existing", name: "Adjusted", points, expectedContext: view().context,
    });
    expect(automarkerSaveRequest("save-as", "preset-existing", "Alternate", points, view().context)).toEqual({
      presetId: null, name: "Alternate", points, expectedContext: view().context,
    });
    expect(() => automarkerSaveRequest("save", null, "Missing", points, view().context)).toThrow(/choose an existing/i);
  });

  it("rejects duplicate marker numbers and non-finite or out-of-bounds manual coordinates", () => {
    expect(() => automarkerSaveRequest("save-as", null, "Duplicate", [
      { markerNumber: 1, x: 0, y: 0, z: 0 }, { markerNumber: 1, x: 2, y: 3, z: 4 },
    ], view().context)).toThrow(/unique/i);
    expect(() => automarkerSaveRequest("save-as", null, "Infinite", [
      { markerNumber: 1, x: Number.POSITIVE_INFINITY, y: 0, z: 0 },
    ], view().context)).toThrow(/finite/i);
  });

  it("selects the distinct ID returned by Save As", () => {
    const next = view();
    next.presets.unshift({ ...next.presets[0]!, presetId: "preset-new-distinct", name: "Alternate" });
    expect(newlyCreatedPresetId(new Set(["preset-000000000001-0000"]), parseAutomarkerPresetView(next)))
      .toBe("preset-new-distinct");
  });

  it("publishes a scene-exact local preview without invoking an outbound adapter", () => {
    const writes: Array<[string, string]> = [];
    const preview = publishAutomarkerPreview(
      { setItem: (key, value) => { writes.push([key, value]); } },
      view().context,
      "Manual",
      [{ markerNumber: 6, x: -1.25, y: 9.5, z: 44.125 }],
      "preview-test-session",
      1_000,
    );
    expect(writes).toHaveLength(1);
    expect(parseAutomarkerPreview(JSON.parse(writes[0]![1]))).toEqual(preview);
    expect(previewMatchesContext(preview, view().context)).toBe(true);
    expect(previewMatchesContext(preview, { ...view().context, sceneId: 1100, activityFamilyId: "mech-facility" })).toBe(false);
    expect(activeAutomarkerPreview(preview, "preview-test-session", view().context, 1_001)).toBe(true);
    expect(activeAutomarkerPreview(preview, "preview-next-session", view().context, 1_001)).toBe(false);
    expect(activeAutomarkerPreview(preview, "preview-test-session", view().context, preview.expiresAtUnixMillis)).toBe(false);
  });

  it("clears expired and prior-host-session previews instead of resurrecting them", () => {
    let raw: string | null = null;
    let removals = 0;
    const storage = {
      setItem: (_key: string, value: string) => { raw = value; },
      getItem: () => raw,
      removeItem: () => { raw = null; removals += 1; },
    };
    publishAutomarkerPreview(storage, view().context, "Old", [{ markerNumber: 1, x: 0, y: 0, z: 0 }], "preview-old-session", 1_000);
    expect(readActiveAutomarkerPreview(storage, "preview-new-session", view().context, 1_001)).toBeNull();
    expect(removals).toBe(1);
    publishAutomarkerPreview(storage, view().context, "Expired", [{ markerNumber: 1, x: 0, y: 0, z: 0 }], "preview-new-session", 1_000);
    expect(readActiveAutomarkerPreview(storage, "preview-new-session", view().context, 301_000)).toBeNull();
    expect(removals).toBe(2);
  });
});
