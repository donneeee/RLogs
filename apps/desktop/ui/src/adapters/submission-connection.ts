export interface SubmissionConnectionView {
  schemaVersion: 1;
  endpointUrl: string | null;
  credentialPresent: boolean;
  credentialStore: string;
}

export function parseSubmissionConnection(value: unknown): SubmissionConnectionView {
  if (
    !isRecord(value) ||
    value.schemaVersion !== 1 ||
    (value.endpointUrl !== null && !isSubmissionUrl(value.endpointUrl)) ||
    typeof value.credentialPresent !== "boolean" ||
    typeof value.credentialStore !== "string" ||
    value.credentialStore.length === 0
  ) {
    throw new Error("The local host returned an invalid submission connection.");
  }
  return value as unknown as SubmissionConnectionView;
}

function isSubmissionUrl(value: unknown): value is string {
  if (typeof value !== "string") return false;
  try {
    const url = new URL(value);
    const loopback =
      url.protocol === "http:" &&
      (url.hostname === "127.0.0.1" || url.hostname === "localhost" || url.hostname === "[::1]");
    return (
      (url.protocol === "https:" || loopback) &&
      url.username === "" &&
      url.password === "" &&
      url.search === "" &&
      url.hash === ""
    );
  } catch {
    return false;
  }
}

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null && !Array.isArray(value);
}
