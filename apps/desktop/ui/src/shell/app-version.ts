const GITHUB_RELEASES_ROOT = "https://github.com/donneeee/RLogs/releases";

export function releaseNotesUrl(version: string): string {
  const normalized = version.trim().replace(/^v/i, "");
  return /^\d+\.\d+\.\d+(?:[-+][0-9A-Za-z.-]+)?$/.test(normalized)
    ? `${GITHUB_RELEASES_ROOT}/tag/v${encodeURIComponent(normalized)}`
    : GITHUB_RELEASES_ROOT;
}

export function displayVersion(version: string): string {
  const normalized = version.trim().replace(/^v/i, "");
  return normalized === "" ? "Release notes" : `v${normalized}`;
}
