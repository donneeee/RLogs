-- Preserve the immutable schema-15 backfill audit while admitting an explicit
-- schema-17 request. The verifier independently pins the exact current
-- projection/timeline tuple; publishing remains paused until an operator
-- intentionally enables it.
--
-- All three tables are rebuilt as one deferred-FK unit because batches are
-- referenced by jobs and jobs are referenced by projection versions. Replacing
-- only the parent fails as soon as production contains an audit row.

PRAGMA defer_foreign_keys = ON;

DROP INDEX projection_backfill_batches_ready;
DROP INDEX projection_backfill_jobs_by_batch;

ALTER TABLE report_projection_versions RENAME TO report_projection_versions_schema15;
ALTER TABLE projection_backfill_jobs RENAME TO projection_backfill_jobs_schema15;
ALTER TABLE projection_backfill_batches RENAME TO projection_backfill_batches_schema15;

CREATE TABLE projection_backfill_batches (
    batch_id TEXT PRIMARY KEY,
    requested_by TEXT NOT NULL,
    workflow_run_url TEXT NOT NULL,
    target_verifier_release TEXT NOT NULL,
    source_schema_version INTEGER NOT NULL CHECK (source_schema_version = 12),
    target_schema_version INTEGER NOT NULL CHECK (target_schema_version IN (15, 17)),
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

INSERT INTO projection_backfill_batches
SELECT * FROM projection_backfill_batches_schema15;

INSERT INTO projection_backfill_jobs
SELECT * FROM projection_backfill_jobs_schema15;

INSERT INTO report_projection_versions
SELECT * FROM report_projection_versions_schema15;

DROP TABLE report_projection_versions_schema15;
DROP TABLE projection_backfill_jobs_schema15;
DROP TABLE projection_backfill_batches_schema15;

CREATE INDEX projection_backfill_batches_ready
ON projection_backfill_batches (state, created_unix_millis, batch_id);

CREATE INDEX projection_backfill_jobs_by_batch
ON projection_backfill_jobs (batch_id, state, report_id);
