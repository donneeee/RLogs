-- Packet-replayed, solo three-minute target-dummy results. The projection
-- stays in R2 while D1 carries only sortable leaderboard and audit fields.

CREATE TABLE training_dummy_results (
    result_id TEXT PRIMARY KEY,
    upload_id TEXT NOT NULL UNIQUE REFERENCES upload_sessions(upload_id) ON DELETE RESTRICT,
    visibility TEXT NOT NULL CHECK (visibility IN ('private', 'unlisted', 'public')),
    verification_tier TEXT NOT NULL CHECK (verification_tier IN ('replayed', 'corroborated', 'ranked')),
    verifier_release TEXT NOT NULL,
    submitter_id TEXT NOT NULL REFERENCES accounts(submitter_id) ON DELETE RESTRICT,
    character_id TEXT NOT NULL,
    display_name TEXT,
    deployment_id TEXT NOT NULL,
    region_id TEXT NOT NULL,
    realm_id TEXT,
    world_id TEXT,
    season_id INTEGER NOT NULL CHECK (season_id > 0),
    class_id INTEGER NOT NULL,
    specialization_id INTEGER NOT NULL,
    target_monster_id INTEGER NOT NULL CHECK (target_monster_id IN (115, 122)),
    duration_micros INTEGER NOT NULL CHECK (duration_micros = 180000000),
    total_damage INTEGER NOT NULL CHECK (total_damage > 0),
    dps REAL NOT NULL CHECK (dps > 0),
    artifact_sha256 TEXT NOT NULL CHECK (length(artifact_sha256) = 64),
    projection_sha256 TEXT NOT NULL CHECK (length(projection_sha256) = 64),
    projection_object_key TEXT NOT NULL UNIQUE,
    created_unix_millis INTEGER NOT NULL,
    verified_unix_millis INTEGER NOT NULL,
    published_unix_millis INTEGER
) STRICT;

CREATE INDEX training_dummy_results_leaderboard
ON training_dummy_results (
    season_id, region_id, class_id, specialization_id, dps DESC,
    verified_unix_millis, result_id
)
WHERE visibility = 'public';

CREATE INDEX training_dummy_results_by_character
ON training_dummy_results (
    character_id, season_id, dps DESC, verified_unix_millis
);
