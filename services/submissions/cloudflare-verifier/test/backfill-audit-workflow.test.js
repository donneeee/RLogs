import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import test from "node:test";

const workflowUrl = new URL(
  "../../../../.github/workflows/inspect-projection-backfills.yml",
  import.meta.url,
);

test("projection backfill audit workflow stays manual, bounded, and read-only", async () => {
  const workflow = await readFile(workflowUrl, "utf8");

  assert.match(workflow, /on:\s*\n\s+workflow_dispatch:/u);
  assert.doesNotMatch(workflow, /\n\s+(?:push|pull_request|schedule):/u);
  assert.match(workflow, /LIMIT 5/u);
  assert.match(workflow, /LIMIT 1/u);
  assert.match(workflow, /LIMIT 80/u);
  assert.match(workflow, /\^bf_\[0-9\]\{1,20\}_\[0-9\]\{1,4\}\$/u);

  const commands = [...workflow.matchAll(/--command "\$(BATCH_SQL|JOB_SQL)"/gu)];
  assert.deepEqual(commands.map((match) => match[1]), ["BATCH_SQL", "JOB_SQL"]);
  assert.doesNotMatch(workflow, /\b(?:INSERT|UPDATE|DELETE|REPLACE|DROP|ALTER|CREATE)\b/u);
  assert.doesNotMatch(workflow, /(?:failure_detail|requested_by|workflow_run_url|report_id|upload_id|artifact_sha256|object_key)/u);
  assert.doesNotMatch(workflow, /(?:wrangler deploy|db:migrate|migrations apply|pages deploy)/u);
});
