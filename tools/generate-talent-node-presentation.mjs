#!/usr/bin/env node

import { existsSync, mkdirSync, readFileSync, readdirSync, writeFileSync } from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const source = path.join(root, "plugins/games/blue-protocol-star-resonance/game-data/catalog/talents");
const output = path.join(root, "plugins/games/blue-protocol-star-resonance/game-data/runtime/talent-node-presentation.v1.json");
const nodes = {};

for (const file of jsonFiles(source)) {
  const talent = JSON.parse(readFileSync(file, "utf8"));
  if (talent?.kind !== "talent" || !Number.isInteger(talent.id)) continue;
  for (const node of talent.attributes?.tree_nodes ?? []) {
    if (!Number.isInteger(node?.node_id)) continue;
    if (nodes[node.node_id]) throw new Error(`duplicate talent node ${node.node_id}`);
    nodes[node.node_id] = {
      talent_id: talent.id,
      talent_level: talent.attributes?.talent_level ?? null,
      profession_id: node.profession_id ?? talent.attributes?.profession_id ?? null,
      specialization_id: node.specialization_id ?? null,
    };
  }
}

mkdirSync(path.dirname(output), { recursive: true });
writeFileSync(output, `${JSON.stringify({ schema_version: 1, nodes })}\n`);
console.log(`wrote ${output} with ${Object.keys(nodes).length} talent nodes`);

function jsonFiles(directory) {
  if (!existsSync(directory)) return [];
  return readdirSync(directory, { withFileTypes: true }).flatMap((entry) => {
    const file = path.join(directory, entry.name);
    if (entry.isDirectory()) return jsonFiles(file);
    return entry.isFile() && entry.name.endsWith(".json") ? [file] : [];
  });
}
