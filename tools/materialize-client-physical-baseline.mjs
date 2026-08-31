#!/usr/bin/env node

import { mkdir, readdir, readFile, writeFile } from "node:fs/promises";
import { extname, join } from "node:path";

function parseArgs(argv) {
  const values = new Map();
  for (let index = 0; index < argv.length; index += 2) {
    const key = argv[index];
    const value = argv[index + 1];
    if (!key?.startsWith("--") || value === undefined) {
      throw new Error(`invalid argument near ${key ?? "<end>"}`);
    }
    values.set(key.slice(2), value);
  }
  for (const required of ["baseline", "diff", "output"]) {
    if (!values.has(required)) throw new Error(`missing --${required}`);
  }
  return Object.fromEntries(values);
}

function normalizedHash(value) {
  return String(value ?? "").replace(/^sha256:/i, "").toLowerCase();
}

function assertEqual(actual, expected, message) {
  if (actual !== expected) {
    throw new Error(`${message}: expected ${expected}, received ${actual}`);
  }
}

function baselineFile(change, previous) {
  return {
    relative_path: change.relative_path,
    bytes: change.new_bytes,
    modified_utc: previous?.modified_utc ?? "1970-01-01T00:00:00.000Z",
    stable_during_scan: true,
    extension: previous?.extension ?? extname(change.relative_path).toLowerCase(),
    signature: previous?.signature ?? "binary_or_text",
    sha256: `sha256:${normalizedHash(change.new_sha256)}`,
  };
}

async function main() {
  const options = parseArgs(process.argv.slice(2));
  const existingOutput = await readdir(options.output).catch((error) => {
    if (error.code === "ENOENT") return [];
    throw error;
  });
  if (existingOutput.length > 0) {
    throw new Error(`refusing to overwrite non-empty output: ${options.output}`);
  }

  const familyFiles = (await readdir(options.baseline, { withFileTypes: true }))
    .filter((entry) => entry.isFile() && entry.name.endsWith(".json"))
    .sort((left, right) => left.name.localeCompare(right.name));
  const entries = new Map();
  for (const familyFile of familyFiles) {
    const family = familyFile.name.slice(0, -5);
    const files = JSON.parse(await readFile(join(options.baseline, familyFile.name), "utf8"));
    for (const file of files) {
      if (!file.stable_during_scan) {
        throw new Error(`unstable baseline entry: ${file.relative_path}`);
      }
      if (entries.has(file.relative_path)) {
        throw new Error(`duplicate baseline path: ${file.relative_path}`);
      }
      entries.set(file.relative_path, { family, file });
    }
  }

  const diff = JSON.parse(await readFile(options.diff, "utf8"));
  assertEqual(entries.size, diff.summary.baseline_files, "baseline file count mismatch");
  if (diff.unstable.length > 0) {
    throw new Error("cannot materialize a baseline containing unstable changes");
  }

  for (const change of diff.removed) {
    const previous = entries.get(change.relative_path);
    if (!previous) throw new Error(`removed path missing from baseline: ${change.relative_path}`);
    assertEqual(previous.file.bytes, change.old_bytes, `old byte count mismatch for ${change.relative_path}`);
    assertEqual(
      normalizedHash(previous.file.sha256),
      normalizedHash(change.old_sha256),
      `old hash mismatch for ${change.relative_path}`,
    );
    entries.delete(change.relative_path);
  }

  for (const change of diff.changed) {
    const previous = entries.get(change.relative_path);
    if (!previous) throw new Error(`changed path missing from baseline: ${change.relative_path}`);
    assertEqual(previous.file.bytes, change.old_bytes, `old byte count mismatch for ${change.relative_path}`);
    assertEqual(
      normalizedHash(previous.file.sha256),
      normalizedHash(change.old_sha256),
      `old hash mismatch for ${change.relative_path}`,
    );
    if (!change.stable_during_scan) throw new Error(`changed path was unstable: ${change.relative_path}`);
    entries.set(change.relative_path, {
      family: change.family,
      file: baselineFile(change, previous.file),
    });
  }

  for (const change of diff.added) {
    if (entries.has(change.relative_path)) {
      throw new Error(`added path already exists: ${change.relative_path}`);
    }
    if (!change.stable_during_scan) throw new Error(`added path was unstable: ${change.relative_path}`);
    entries.set(change.relative_path, { family: change.family, file: baselineFile(change) });
  }

  assertEqual(entries.size, diff.summary.current_files, "materialized file count mismatch");
  const families = new Map();
  for (const { family, file } of entries.values()) {
    if (!families.has(family)) families.set(family, []);
    families.get(family).push(file);
  }

  await mkdir(options.output, { recursive: true });
  for (const [family, files] of [...families].sort(([left], [right]) => left.localeCompare(right))) {
    files.sort((left, right) => left.relative_path.localeCompare(right.relative_path));
    await writeFile(join(options.output, `${family}.json`), `${JSON.stringify(files, null, 2)}\n`);
  }

  process.stdout.write(`${JSON.stringify({
    baseline_build_id: diff.baseline_build_id,
    materialized_build_id: diff.build_id,
    files: entries.size,
    families: families.size,
    output: options.output,
  }, null, 2)}\n`);
}

main().catch((error) => {
  process.stderr.write(`${error.stack ?? error}\n`);
  process.exitCode = 1;
});
