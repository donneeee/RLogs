-- Operator-authored inverse transactions for a published projection backfill.
-- Requests name both exact pointers; scheduled verifier work may never choose a
-- report, projection, or rollback target on its own.

ALTER TABLE projection_backfill_jobs ADD COLUMN source_indexes_sha256 TEXT
  CHECK (source_indexes_sha256 IS NULL OR length(source_indexes_sha256) = 64);
ALTER TABLE projection_backfill_jobs ADD COLUMN source_indexes_object_key TEXT;

CREATE TABLE projection_backfill_rollbacks (
    rollback_id TEXT PRIMARY KEY CHECK (
      length(rollback_id) = 36 AND substr(rollback_id, 1, 4) = 'bfr_'
    ),
    requested_by TEXT NOT NULL,
    workflow_run_url TEXT NOT NULL,
    job_id TEXT NOT NULL UNIQUE REFERENCES projection_backfill_jobs(job_id) ON DELETE RESTRICT,
    expected_candidate_projection_sha256 TEXT NOT NULL CHECK (length(expected_candidate_projection_sha256) = 64),
    expected_candidate_projection_object_key TEXT NOT NULL,
    target_source_projection_sha256 TEXT NOT NULL CHECK (length(target_source_projection_sha256) = 64),
    target_source_projection_object_key TEXT NOT NULL,
    state TEXT NOT NULL CHECK (state IN (
      'pending', 'running', 'committing', 'restored', 'retryable_failure', 'rejected', 'superseded'
    )),
    lease_token TEXT,
    attempt_count INTEGER NOT NULL DEFAULT 0 CHECK (attempt_count BETWEEN 0 AND 3),
    failure_code TEXT,
    failure_detail TEXT,
    created_unix_millis INTEGER NOT NULL,
    updated_unix_millis INTEGER NOT NULL,
    completed_unix_millis INTEGER
) STRICT;

CREATE INDEX projection_backfill_rollbacks_ready
ON projection_backfill_rollbacks (state, created_unix_millis, rollback_id);
