-- Production metadata for the Cloudflare-hosted rLogs service.
-- Large immutable .rlog artifacts and rendered photo bytes belong in private
-- R2 objects; this database stores their identities, lifecycle, and audit trail.

CREATE TABLE service_metadata (
    component TEXT PRIMARY KEY,
    schema_version INTEGER NOT NULL CHECK (schema_version > 0),
    installed_unix_millis INTEGER NOT NULL CHECK (installed_unix_millis > 0)
) STRICT;

INSERT INTO service_metadata (component, schema_version, installed_unix_millis)
VALUES ('production-metadata', 1, CAST(unixepoch('now') AS INTEGER) * 1000);

CREATE TABLE accounts (
    submitter_id TEXT PRIMARY KEY,
    account_id TEXT NOT NULL UNIQUE,
    username TEXT NOT NULL UNIQUE,
    discord_user_hash TEXT NOT NULL UNIQUE,
    discord_username TEXT NOT NULL,
    discord_global_name TEXT,
    discord_avatar_url TEXT,
    created_unix_millis INTEGER NOT NULL,
    updated_unix_millis INTEGER NOT NULL,
    CHECK (updated_unix_millis >= created_unix_millis)
) STRICT;

CREATE TABLE device_tokens (
    token_hash TEXT PRIMARY KEY,
    submitter_id TEXT NOT NULL REFERENCES accounts(submitter_id) ON DELETE CASCADE,
    device_id TEXT NOT NULL UNIQUE,
    created_unix_millis INTEGER NOT NULL,
    revoked_unix_millis INTEGER
) STRICT;

CREATE INDEX device_tokens_by_submitter
ON device_tokens (submitter_id, created_unix_millis DESC);

CREATE TABLE uid_claims (
    game_id TEXT NOT NULL,
    character_id TEXT NOT NULL,
    profile_id TEXT NOT NULL UNIQUE,
    submitter_id TEXT NOT NULL REFERENCES accounts(submitter_id) ON DELETE RESTRICT,
    deployment_id TEXT NOT NULL,
    region_id TEXT NOT NULL,
    realm_id TEXT,
    claimed_unix_millis INTEGER NOT NULL,
    PRIMARY KEY (game_id, character_id)
) STRICT;

CREATE INDEX uid_claims_by_submitter
ON uid_claims (submitter_id, claimed_unix_millis DESC);

CREATE TABLE profiles (
    profile_id TEXT PRIMARY KEY,
    game_id TEXT NOT NULL,
    character_id TEXT NOT NULL,
    submitter_id TEXT NOT NULL REFERENCES accounts(submitter_id) ON DELETE RESTRICT,
    current_package_id TEXT NOT NULL,
    source_client_build TEXT NOT NULL,
    deployment_id TEXT NOT NULL,
    region_id TEXT NOT NULL,
    realm_id TEXT,
    public_projection_json TEXT NOT NULL CHECK (json_valid(public_projection_json)),
    created_unix_millis INTEGER NOT NULL,
    updated_unix_millis INTEGER NOT NULL,
    UNIQUE (game_id, character_id),
    CHECK (updated_unix_millis >= created_unix_millis)
) STRICT;

CREATE INDEX profiles_by_updated
ON profiles (updated_unix_millis DESC, profile_id);

CREATE TABLE profile_loadouts (
    profile_id TEXT NOT NULL REFERENCES profiles(profile_id) ON DELETE CASCADE,
    project_id INTEGER NOT NULL CHECK (project_id > 0),
    package_id TEXT NOT NULL,
    display_name TEXT,
    projection_json TEXT NOT NULL CHECK (json_valid(projection_json)),
    updated_unix_millis INTEGER NOT NULL,
    PRIMARY KEY (profile_id, project_id)
) STRICT;

CREATE TABLE photo_assets (
    profile_id TEXT NOT NULL REFERENCES profiles(profile_id) ON DELETE CASCADE,
    photo_id INTEGER NOT NULL CHECK (photo_id > 0),
    object_key TEXT NOT NULL UNIQUE,
    sha256 TEXT NOT NULL CHECK (length(sha256) = 64),
    byte_length INTEGER NOT NULL CHECK (byte_length > 0),
    media_type TEXT NOT NULL,
    uploaded_unix_millis INTEGER NOT NULL,
    PRIMARY KEY (profile_id, photo_id)
) STRICT;

CREATE TABLE photo_likes (
    profile_id TEXT NOT NULL,
    photo_id INTEGER NOT NULL,
    submitter_id TEXT NOT NULL REFERENCES accounts(submitter_id) ON DELETE CASCADE,
    liked_unix_millis INTEGER NOT NULL,
    PRIMARY KEY (profile_id, photo_id, submitter_id),
    FOREIGN KEY (profile_id, photo_id) REFERENCES photo_assets(profile_id, photo_id) ON DELETE CASCADE
) STRICT;

CREATE INDEX photo_likes_by_photo
ON photo_likes (profile_id, photo_id, liked_unix_millis DESC);

CREATE TABLE upload_sessions (
    upload_id TEXT PRIMARY KEY,
    artifact_sha256 TEXT NOT NULL UNIQUE CHECK (length(artifact_sha256) = 64),
    submitter_id TEXT NOT NULL REFERENCES accounts(submitter_id) ON DELETE RESTRICT,
    device_id TEXT NOT NULL,
    state TEXT NOT NULL CHECK (state IN ('receiving', 'assembled', 'queued', 'verifying', 'accepted', 'rejected')),
    byte_length INTEGER NOT NULL CHECK (byte_length > 0),
    chunk_size INTEGER NOT NULL CHECK (chunk_size > 0),
    chunk_count INTEGER NOT NULL CHECK (chunk_count > 0),
    manifest_json TEXT NOT NULL CHECK (json_valid(manifest_json)),
    artifact_object_key TEXT,
    rejection_code TEXT,
    rejection_detail TEXT,
    created_unix_millis INTEGER NOT NULL,
    updated_unix_millis INTEGER NOT NULL,
    finalized_unix_millis INTEGER,
    CHECK (updated_unix_millis >= created_unix_millis)
) STRICT;

CREATE INDEX upload_sessions_by_state
ON upload_sessions (state, updated_unix_millis, upload_id);

CREATE INDEX upload_sessions_by_submitter
ON upload_sessions (submitter_id, created_unix_millis DESC);

CREATE TABLE upload_chunks (
    upload_id TEXT NOT NULL REFERENCES upload_sessions(upload_id) ON DELETE CASCADE,
    sequence INTEGER NOT NULL CHECK (sequence >= 0),
    sha256 TEXT NOT NULL CHECK (length(sha256) = 64),
    byte_length INTEGER NOT NULL CHECK (byte_length > 0),
    object_key TEXT NOT NULL UNIQUE,
    acknowledged_unix_millis INTEGER NOT NULL,
    PRIMARY KEY (upload_id, sequence)
) STRICT;

CREATE TABLE verification_jobs (
    upload_id TEXT PRIMARY KEY REFERENCES upload_sessions(upload_id) ON DELETE CASCADE,
    state TEXT NOT NULL CHECK (state IN ('queued', 'running', 'accepted', 'rejected', 'retryable_failure')),
    attempt_count INTEGER NOT NULL DEFAULT 0 CHECK (attempt_count >= 0),
    verifier_release TEXT NOT NULL,
    game_build TEXT NOT NULL,
    protocol_pack_digest TEXT NOT NULL,
    input_sha256 TEXT NOT NULL CHECK (length(input_sha256) = 64),
    output_sha256 TEXT,
    lease_owner TEXT,
    lease_expires_unix_millis INTEGER,
    next_attempt_unix_millis INTEGER,
    created_unix_millis INTEGER NOT NULL,
    updated_unix_millis INTEGER NOT NULL,
    completed_unix_millis INTEGER,
    failure_code TEXT,
    failure_detail TEXT
) STRICT;

CREATE INDEX verification_jobs_ready
ON verification_jobs (state, next_attempt_unix_millis, created_unix_millis);

CREATE TABLE reports (
    report_id TEXT PRIMARY KEY,
    upload_id TEXT NOT NULL UNIQUE REFERENCES upload_sessions(upload_id) ON DELETE RESTRICT,
    run_group_id TEXT NOT NULL,
    visibility TEXT NOT NULL CHECK (visibility IN ('private', 'unlisted', 'public')),
    verification_tier TEXT NOT NULL CHECK (verification_tier IN ('replayed', 'corroborated', 'ranked')),
    verifier_release TEXT NOT NULL,
    game_build TEXT NOT NULL,
    protocol_pack_digest TEXT NOT NULL,
    artifact_sha256 TEXT NOT NULL CHECK (length(artifact_sha256) = 64),
    projection_sha256 TEXT NOT NULL CHECK (length(projection_sha256) = 64),
    projection_object_key TEXT NOT NULL UNIQUE,
    verified_unix_millis INTEGER NOT NULL,
    published_unix_millis INTEGER
) STRICT;

CREATE INDEX reports_by_run_group
ON reports (run_group_id, verified_unix_millis DESC);

CREATE INDEX reports_public
ON reports (visibility, published_unix_millis DESC, report_id);

CREATE TABLE report_memberships (
    report_id TEXT NOT NULL REFERENCES reports(report_id) ON DELETE CASCADE,
    game_id TEXT NOT NULL,
    character_id TEXT NOT NULL,
    actor_id TEXT,
    player_name TEXT,
    PRIMARY KEY (report_id, game_id, character_id)
) STRICT;

CREATE INDEX report_memberships_by_character
ON report_memberships (game_id, character_id, report_id);

CREATE TABLE reconciliations (
    run_group_id TEXT PRIMARY KEY,
    state TEXT NOT NULL CHECK (state IN ('single_vantage', 'pending', 'reconciled', 'conflicted')),
    projection_json TEXT NOT NULL CHECK (json_valid(projection_json)),
    updated_unix_millis INTEGER NOT NULL
) STRICT;

CREATE TABLE audit_events (
    sequence INTEGER PRIMARY KEY AUTOINCREMENT,
    event_id TEXT NOT NULL UNIQUE,
    event_kind TEXT NOT NULL,
    subject_kind TEXT NOT NULL,
    subject_id TEXT NOT NULL,
    actor_submitter_id TEXT,
    occurred_unix_millis INTEGER NOT NULL,
    previous_event_sha256 TEXT,
    event_sha256 TEXT NOT NULL UNIQUE CHECK (length(event_sha256) = 64),
    payload_json TEXT NOT NULL CHECK (json_valid(payload_json))
) STRICT;

CREATE INDEX audit_events_by_subject
ON audit_events (subject_kind, subject_id, sequence);
