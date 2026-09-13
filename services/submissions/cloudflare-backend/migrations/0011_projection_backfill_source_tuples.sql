-- Admit only the three historical projection tuples reviewed for replay into
-- the current schema-17/projection-12/timeline-8 producer. Runtime validation
-- independently checks the exact projection and timeline revision for the
-- requested source schema. Publication remains paused in the verifier.
--
-- Rebuild the complete deferred-FK chain as one unit. Production may contain
-- audit rows at every level, including operator-authored rollback requests.

PRAGMA defer_foreign_keys = ON;

DROP INDEX projection_backfill_rollbacks_ready;
DROP INDEX projection_backfill_batches_ready;
DROP INDEX projection_backfill_jobs_by_batch;

ALTER TABLE projection_backfill_rollbacks RENAME TO projection_backfill_rollbacks_schema12_only;
ALTER TABLE report_projection_versions RENAME TO report_projection_versions_schema12_only;
ALTER TABLE projection_backfill_jobs RENAME TO projection_backfill_jobs_schema12_only;
ALTER TABLE projection_backfill_batches RENAME TO projection_backfill_batches_schema12_only;

CREATE TABLE projection_backfill_batches (
    batch_id TEXT PRIMARY KEY,
    requested_by TEXT NOT NULL,
    workflow_run_url TEXT NOT NULL,
    target_verifier_release TEXT NOT NULL,
    source_schema_version INTEGER NOT NULL CHECK (source_schema_version IN (12, 15, 17)),
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
    source_indexes_sha256 TEXT CHECK (source_indexes_sha256 IS NULL OR length(source_indexes_sha256) = 64),
    source_indexes_object_key TEXT,
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

INSERT INTO projection_backfill_batches
SELECT * FROM projection_backfill_batches_schema12_only;

INSERT INTO projection_backfill_jobs
SELECT * FROM projection_backfill_jobs_schema12_only;

INSERT INTO report_projection_versions
SELECT * FROM report_projection_versions_schema12_only;

INSERT INTO projection_backfill_rollbacks
SELECT * FROM projection_backfill_rollbacks_schema12_only;

DROP TABLE projection_backfill_rollbacks_schema12_only;
DROP TABLE report_projection_versions_schema12_only;
DROP TABLE projection_backfill_jobs_schema12_only;
DROP TABLE projection_backfill_batches_schema12_only;

CREATE INDEX projection_backfill_batches_ready
ON projection_backfill_batches (state, created_unix_millis, batch_id);

CREATE INDEX projection_backfill_jobs_by_batch
ON projection_backfill_jobs (batch_id, state, report_id);

CREATE INDEX projection_backfill_rollbacks_ready
ON projection_backfill_rollbacks (state, created_unix_millis, rollback_id);
