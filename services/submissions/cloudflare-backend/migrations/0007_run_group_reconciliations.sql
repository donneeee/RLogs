-- Immutable, content-addressed multi-POV reconciliation publications.  The
-- Worker records the exact source set before replay and only advances the
-- current pointer while every source is still the current public projection.

CREATE TABLE reconciliation_jobs (
    job_id TEXT PRIMARY KEY,
    run_group_id TEXT NOT NULL,
    source_set_sha256 TEXT NOT NULL CHECK (length(source_set_sha256) = 64),
    state TEXT NOT NULL CHECK (state IN ('running', 'published', 'retryable_failure', 'rejected', 'superseded')),
    attempt_count INTEGER NOT NULL DEFAULT 0 CHECK (attempt_count >= 0),
    lease_token TEXT NOT NULL,
    failure_code TEXT,
    failure_detail TEXT,
    created_unix_millis INTEGER NOT NULL,
    updated_unix_millis INTEGER NOT NULL,
    completed_unix_millis INTEGER,
    UNIQUE (run_group_id, source_set_sha256)
) STRICT;

CREATE INDEX reconciliation_jobs_ready
ON reconciliation_jobs (state, updated_unix_millis, run_group_id);

CREATE TABLE reconciliation_job_sources (
    job_id TEXT NOT NULL REFERENCES reconciliation_jobs(job_id) ON DELETE CASCADE,
    report_id TEXT NOT NULL REFERENCES reports(report_id) ON DELETE RESTRICT,
    run_index INTEGER NOT NULL CHECK (run_index >= 0),
    projection_sha256 TEXT NOT NULL CHECK (length(projection_sha256) = 64),
    projection_object_key TEXT NOT NULL,
    PRIMARY KEY (job_id, report_id),
    UNIQUE (job_id, report_id, run_index)
) STRICT;

CREATE TABLE reconciliation_versions (
    reconciliation_id TEXT PRIMARY KEY,
    run_group_id TEXT NOT NULL,
    source_set_sha256 TEXT NOT NULL CHECK (length(source_set_sha256) = 64),
    projection_sha256 TEXT NOT NULL CHECK (length(projection_sha256) = 64),
    projection_object_key TEXT NOT NULL UNIQUE,
    source_count INTEGER NOT NULL CHECK (source_count BETWEEN 2 AND 64),
    reconciliation_status TEXT NOT NULL,
    created_unix_millis INTEGER NOT NULL,
    UNIQUE (run_group_id, source_set_sha256)
) STRICT;

CREATE INDEX reconciliation_versions_by_group
ON reconciliation_versions (run_group_id, created_unix_millis DESC);

CREATE TABLE reconciliation_current (
    run_group_id TEXT PRIMARY KEY,
    reconciliation_id TEXT NOT NULL REFERENCES reconciliation_versions(reconciliation_id) ON DELETE RESTRICT,
    source_set_sha256 TEXT NOT NULL CHECK (length(source_set_sha256) = 64),
    updated_unix_millis INTEGER NOT NULL
) STRICT;
