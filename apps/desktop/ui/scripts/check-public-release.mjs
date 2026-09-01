import { readdir, readFile } from "node:fs/promises";
import { join } from "node:path";
import { fileURLToPath } from "node:url";

const distRoot = fileURLToPath(new URL("../dist/", import.meta.url));
const forbidden = [
  "rlogs-submissions.pages.dev",
  "Receiver HTTPS URL",
  "Session Recorder",
  "app.rlogs.session-recorder",
  "/api/runtime/live/events/wait",
  "/api/runtime/events/page",
  "/api/runtime/run-report",
  "/api/runtime/reference-replay",
  "/api/runtime/offline",
  "/api/runtime/live/start",
  "/api/runtime/live/stop",
];

async function publicAssets(directory) {
  const entries = await readdir(directory, { withFileTypes: true });
  const files = [];
  for (const entry of entries) {
    const path = join(directory, entry.name);
    if (entry.isDirectory()) {
      files.push(...(await publicAssets(path)));
    } else if (!entry.name.endsWith(".map")) {
      files.push(path);
    }
  }
  return files;
}

const leaks = [];
for (const path of await publicAssets(distRoot)) {
  const content = await readFile(path, "utf8");
  for (const value of forbidden) {
    if (content.includes(value)) {
      leaks.push(`${value} in ${path}`);
    }
  }
}

if (leaks.length > 0) {
  throw new Error(`Public desktop build contains developer-only details:\n${leaks.join("\n")}`);
}

console.log("Public desktop release surface contains no internal service hostname or Session Recorder controls.");
