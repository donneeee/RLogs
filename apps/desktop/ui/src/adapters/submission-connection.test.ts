import { describe, expect, it } from "vitest";

import { parseSubmissionConnection } from "./submission-connection";

describe("submission connection", () => {
  it("accepts a credential-free disconnected view", () => {
    expect(
      parseSubmissionConnection({
        schemaVersion: 1,
        endpointUrl: null,
        credentialPresent: false,
        credentialStore: "Windows Credential Manager",
      }).endpointUrl,
    ).toBeNull();
  });

  it("accepts HTTPS without exposing the credential", () => {
    const view = parseSubmissionConnection({
      schemaVersion: 1,
      endpointUrl: "https://rlogs-submissions.example.workers.dev",
      credentialPresent: true,
      credentialStore: "Windows Credential Manager",
    });
    expect(view.credentialPresent).toBe(true);
    expect(view).not.toHaveProperty("deviceToken");
  });

  it("accepts loopback HTTP for local development", () => {
    expect(
      parseSubmissionConnection({
        schemaVersion: 1,
        endpointUrl: "http://127.0.0.1:8787",
        credentialPresent: true,
        credentialStore: "Windows Credential Manager",
      }).endpointUrl,
    ).toBe("http://127.0.0.1:8787");
  });

  it("rejects insecure and credential-bearing endpoints", () => {
    const base = {
      schemaVersion: 1,
      credentialPresent: true,
      credentialStore: "Windows Credential Manager",
    };
    expect(() =>
      parseSubmissionConnection({ ...base, endpointUrl: "http://receiver.example" }),
    ).toThrow("invalid submission connection");
    expect(() =>
      parseSubmissionConnection({
        ...base,
        endpointUrl: "https://token@receiver.example",
      }),
    ).toThrow("invalid submission connection");
  });
});
