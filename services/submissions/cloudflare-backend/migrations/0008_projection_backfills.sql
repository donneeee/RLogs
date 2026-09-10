-- Operator-created, bounded re-verification batches for upgrading immutable
-- hosted parse projections.  A request is inert until the private verifier
-- Worker claims it; no browser-facing route can create one.

CREATE TABLE projection_backfill_batches (
    batch_id TEXT PRIMARY KEY,
    requested_by TEXT NOT NULL,
    workflow_run_url TEXT NOT NULL,
    target_verifier_release TEXT NOT NULL,
    source_schema_version INTEGER NOT NULL CHECK (source_schema_version = 12),
    target_schema_version INTEGER NOT NULL CHECK (target_schema_version = 15),
    maximum_reports INTEGER NOT NULL CHECK (maximum_reports BETWEEN 1 AND 25),
    dry_run INTEGER NOT NULL CHECK (dry_run IN (0, 1)),
    after_report_id TEXT,
    cursor_report_id TEXT,
    state TEXT NOT NULL CHECK (state IN ('pending', 'running', 'completed', 'retryable_failure', 'rejected')),
    lease_token TEXT,
    inspected_count INTEGER NOT NULL DEFAULT 0 CHECK (inspected_count BETWEEN 0 AND 25),
    eligible_count INTEGER NOT NULL DEFAULT 0 CHECK (eligible_count BETWEEN 0 AND 25),
    published_count INTEGER NOT NULL DEFAULT 0 CHECK (published_count BETWEEN 0 AND 25),
    skipped_count INTEGER NOT NULL DEFAULT 0 CHECK (skipped_count BETWEEN 0 AND 25),
    consecutive_retry_count INTEGER NOT NULL DEFAULT 0 CHECK (consecutive_retry_count BETWEEN 0 AND 3),
    failure_code TEXT,
    failure_detail TEXT,
    created_unix_millis INTEGER NOT NULL,
    updated_unix_millis INTEGER NOT NULL,
    completed_unix_millis INTEGER
) STRICT;

CREATE INDEX projection_backfill_batches_ready
ON projection_backfill_batches (state, created_unix_millis, batch_id);

CREATE TABLE projection_backfill_jobs (
    job_id TEXT PRIMARY KEY,
    batch_id TEXT NOT NULL REFERENCES projection_backfill_batches(batch_id) ON DELETE RESTRICT,
    report_id TEXT NOT NULL REFERENCES reports(report_id) ON DELETE RESTRICT,
    upload_id TEXT NOT NULL,
    artifact_sha256 TEXT NOT NULL CHECK (length(artifact_sha256) = 64),
    source_projection_sha256 TEXT NOT NULL CHECK (length(source_projection_sha256) = 64),
    source_projection_object_key TEXT NOT NULL,
    target_verifier_release TEXT NOT NULL,
    state TEXT NOT NULL CHECK (state IN ('planned', 'running', 'published', 'skipped', 'retryable_failure', 'rejected', 'superseded')),
    candidate_projection_sha256 TEXT,
    candidate_projection_object_key TEXT,
    candidate_membership_sha256 TEXT,
    candidate_membership_object_key TEXT,
    failure_code TEXT,
    failure_detail TEXT,
    created_unix_millis INTEGER NOT NULL,
    updated_unix_millis INTEGER NOT NULL,
    completed_unix_millis INTEGER,
    UNIQUE (batch_id, report_id)
) STRICT;

CREATE INDEX projection_backfill_jobs_by_batch
ON projection_backfill_jobs (batch_id, state, report_id);

-- Every pointer ever made current remains addressable.  The original
-- projection is registered in the same transaction that advances the pointer.
CREATE TABLE report_projection_versions (
    report_id TEXT NOT NULL REFERENCES reports(report_id) ON DELETE RESTRICT,
    projection_sha256 TEXT NOT NULL CHECK (length(projection_sha256) = 64),
    projection_object_key TEXT NOT NULL UNIQUE,
    schema_version INTEGER NOT NULL CHECK (schema_version > 0),
    verifier_release TEXT NOT NULL,
    artifact_sha256 TEXT NOT NULL CHECK (length(artifact_sha256) = 64),
    backfill_job_id TEXT REFERENCES projection_backfill_jobs(job_id) ON DELETE RESTRICT,
    created_unix_millis INTEGER NOT NULL,
    PRIMARY KEY (report_id, projection_sha256)
) STRICT;
