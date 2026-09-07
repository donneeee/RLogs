-- Materialize one compact public listing per replayed run so catalog reads do
-- not need to download full immutable report projections from R2.

CREATE TABLE report_runs (
    report_id TEXT NOT NULL REFERENCES reports(report_id) ON DELETE CASCADE,
    run_index INTEGER NOT NULL CHECK (run_index >= 0),
    run_group_id TEXT NOT NULL,
    catalog_entry_json TEXT NOT NULL CHECK (json_valid(catalog_entry_json)),
    created_unix_millis INTEGER NOT NULL,
    PRIMARY KEY (report_id, run_index)
) STRICT;

CREATE INDEX report_runs_by_created
ON report_runs (created_unix_millis DESC, report_id, run_index);

CREATE INDEX report_runs_by_group
ON report_runs (run_group_id, created_unix_millis DESC);
