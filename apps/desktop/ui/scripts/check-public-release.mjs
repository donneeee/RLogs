import { readdir, readFile } from "node:fs/promises";
import { join } from "node:path";
import { fileURLToPath } from "node:url";

const distRoot = fileURLToPath(new URL("../dist/", import.meta.url));
const forbidden = [
  "rlogs-submissions.pages.dev",
  "Receiver HTTPS URL",
];
const requiredDeveloperGateText = [
  "Developer mode",
  "Event Inspector is disabled",
  "rlogs:developer-mode-changed",
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
let bundledText = "";
for (const path of await publicAssets(distRoot)) {
  const content = await readFile(path, "utf8");
  bundledText += content;
  for (const value of forbidden) {
    if (content.includes(value)) {
      leaks.push(`${value} in ${path}`);
    }
  }
}

if (leaks.length > 0) {
  throw new Error(`Public desktop build contains developer-only details:\n${leaks.join("\n")}`);
}

const missingGateText = requiredDeveloperGateText.filter(
  (value) => !bundledText.includes(value),
);
if (missingGateText.length > 0) {
  throw new Error(
    `Public desktop build is missing Developer mode safeguards:\n${missingGateText.join("\n")}`,
  );
}

console.log("Public desktop release contains no internal service hostname and includes the Developer mode safeguards.");
