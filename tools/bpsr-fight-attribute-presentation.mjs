#!/usr/bin/env node

import { createHash } from "node:crypto";
import { readFileSync, writeFileSync } from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const repoRoot = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const sourcePath = path.join(repoRoot, "Excels/FightAttrTable.json");
const manifestPath = path.join(
  repoRoot,
  "plugins/games/blue-protocol-star-resonance/game-data/runtime/rdps-formula-runtime.v1.json",
);
const outputPath = path.join(
  repoRoot,
  "plugins/games/blue-protocol-star-resonance/game-data/runtime/fight-attribute-presentation.v1.json",
);
const mode = process.argv[2] ?? "verify";

if (mode !== "generate" && mode !== "verify") {
  throw new Error("usage: node tools/bpsr-fight-attribute-presentation.mjs [generate|verify]");
}

const sourceBytes = readFileSync(sourcePath);
const rows = Object.values(JSON.parse(sourceBytes.toString("utf8")));
const manifest = JSON.parse(readFileSync(manifestPath, "utf8"));
const components = [
  ["final", "AttrFinal"],
  ["total", "AttrTotal"],
  ["add", "AttrAdd"],
  ["extra_add", "AttrExAdd"],
  ["percent", "AttrPer"],
  ["extra_percent", "AttrExPer"],
];
const attributes = [];
const seen = new Set();

for (const row of rows) {
  if (!Number.isSafeInteger(row.AttrFinal) || row.AttrFinal <= 0) continue;
  if (typeof row.OfficialName !== "string" || row.OfficialName.trim() === "") {
    throw new Error(`Fight attribute family ${row.AttrFinal} has no official name`);
  }
  for (const [component, field] of components) {
    const attributeId = row[field];
    if (!Number.isSafeInteger(attributeId) || attributeId <= 0) continue;
    if (seen.has(attributeId)) {
      throw new Error(`duplicate Fight Attribute member ${attributeId}`);
    }
    seen.add(attributeId);
    attributes.push({
      attribute_id: attributeId,
      family_id: row.AttrFinal,
      component,
      name: row.OfficialName.trim(),
      description:
        typeof row.AttrDes === "string" && row.AttrDes.trim() !== ""
          ? row.AttrDes.trim()
          : null,
      number_type: Number.isSafeInteger(row.AttrNumType) ? row.AttrNumType : 0,
      format_type: attributeId % 10,
      icon:
        typeof row.Icon === "string" && row.Icon.trim() !== ""
          ? row.Icon.trim()
          : null,
      displayable: row.OfficialName.trim() !== "AttrLevel",
    });
  }
}

attributes.sort((left, right) => left.attribute_id - right.attribute_id);
const catalog = {
  schema_version: 1,
  game_build: String(manifest.game_build),
  locale: "en-US",
  source: "Exact-build BPSR Global Steam FightAttrTable",
  source_sha256: createHash("sha256").update(sourceBytes).digest("hex"),
  attributes,
};
const encoded = `${JSON.stringify(catalog)}\n`;

if (attributes.length !== 906) {
  throw new Error(`expected 906 exact Fight Attribute members, found ${attributes.length}`);
}
if (mode === "generate") {
  writeFileSync(outputPath, encoded);
  console.log(`wrote ${outputPath}`);
} else {
  const current = readFileSync(outputPath, "utf8");
  if (current !== encoded) {
    throw new Error(
      "fight-attribute presentation catalog is stale; run this tool with generate",
    );
  }
}

console.log(
  `verified ${attributes.length} exact Fight Attribute members for build ${catalog.game_build}`,
);
