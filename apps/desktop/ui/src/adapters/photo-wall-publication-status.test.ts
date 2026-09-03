import { describe, expect, it } from "vitest";

import {
  PHOTO_WALL_CAPTURE_STEPS,
  parsePhotoWallPublicationStatus,
  photoWallLastCaptureSummary,
  photoWallPublicationSummary,
} from "./photo-wall-publication-status";

function publishedStatus() {
  return {
    schemaVersion: 1,
    state: "published",
    observedCount: 1,
    queuedCount: 1,
    publishedCount: 1,
    retryableFailureCount: 0,
    lastActivityUnixMillis: 1_788_261_496_785,
    lastCharacterId: "3296036",
    lastPhotoId: 1,
    lastPictureType: 2,
    lastVersion: 7,
    lastError: null,
  };
}

describe("Photo Wall publication status", () => {
  it("spells out the packet-triggering actions for every wall page", () => {
    expect(PHOTO_WALL_CAPTURE_STEPS).toHaveLength(5);
    expect(PHOTO_WALL_CAPTURE_STEPS[0]).toContain(
      "Publish my Photo Wall images",
    );
    expect(PHOTO_WALL_CAPTURE_STEPS[3]).toContain("pages 1, 2, 3, and 4");
    expect(PHOTO_WALL_CAPTURE_STEPS[4]).toContain("Optional for full quality");
  });

  it("validates and describes an exact full-render publication", () => {
    const status = parsePhotoWallPublicationStatus(publishedStatus());
    expect(photoWallPublicationSummary(status)).toBe("Published (1)");
    expect(photoWallLastCaptureSummary(status)).toBe(
      "UID 3296036 · Photo 1 · full render · version 7",
    );
  });

  it("gives an actionable message before a live observation", () => {
    const status = parsePhotoWallPublicationStatus({
      ...publishedStatus(),
      state: "waiting_for_photo_wall",
      observedCount: 0,
      queuedCount: 0,
      publishedCount: 0,
      lastActivityUnixMillis: null,
      lastCharacterId: null,
      lastPhotoId: null,
      lastPictureType: null,
      lastVersion: null,
    });
    expect(photoWallPublicationSummary(status)).toContain(
      "visit pages 1–4",
    );
    expect(photoWallLastCaptureSummary(status)).toContain("No exact in-game");
  });

  it("rejects unknown states and malformed counters", () => {
    expect(() =>
      parsePhotoWallPublicationStatus({
        ...publishedStatus(),
        state: "uploaded_somehow",
      }),
    ).toThrow("invalid Photo Wall publication status");
    expect(() =>
      parsePhotoWallPublicationStatus({
        ...publishedStatus(),
        publishedCount: -1,
      }),
    ).toThrow("invalid Photo Wall publication status");
  });
});
