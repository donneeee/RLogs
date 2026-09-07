-- Leaderboards are public materialized projections and must also cover
-- restored legacy profiles that predate the authenticated D1 identity mirror.

ALTER TABLE profile_season_rankings RENAME TO profile_season_rankings_v1;
DROP INDEX profile_season_rankings_by_season_score;

CREATE TABLE profile_season_rankings (
    profile_id TEXT NOT NULL,
    character_id TEXT NOT NULL,
    display_name TEXT,
    deployment_id TEXT NOT NULL,
    region_id TEXT NOT NULL,
    realm_id TEXT,
    season_id INTEGER NOT NULL CHECK (season_id > 0),
    master_score INTEGER NOT NULL CHECK (master_score >= 0),
    observed_unix_millis INTEGER NOT NULL,
    PRIMARY KEY (profile_id, season_id)
) STRICT;

INSERT INTO profile_season_rankings (
    profile_id, character_id, display_name, deployment_id, region_id, realm_id,
    season_id, master_score, observed_unix_millis
)
SELECT old.profile_id, p.character_id,
    json_extract(p.public_projection_json, '$.display_name'),
    p.deployment_id, p.region_id, p.realm_id,
    old.season_id, old.master_score, old.observed_unix_millis
FROM profile_season_rankings_v1 old
JOIN profiles p ON p.profile_id=old.profile_id;

DROP TABLE profile_season_rankings_v1;

CREATE INDEX profile_season_rankings_by_season_score
ON profile_season_rankings (
    season_id, region_id, master_score DESC, observed_unix_millis, profile_id
);

ALTER TABLE profile_dungeon_records RENAME TO profile_dungeon_records_v1;
DROP INDEX profile_dungeon_records_by_time;

CREATE TABLE profile_dungeon_records (
    profile_id TEXT NOT NULL,
    character_id TEXT NOT NULL,
    display_name TEXT,
    deployment_id TEXT NOT NULL,
    region_id TEXT NOT NULL,
    realm_id TEXT,
    season_id INTEGER NOT NULL CHECK (season_id > 0),
    activity_id INTEGER NOT NULL CHECK (activity_id > 0),
    tier INTEGER NOT NULL CHECK (tier BETWEEN 1 AND 20),
    score INTEGER CHECK (score >= 0),
    pass_time_seconds INTEGER NOT NULL CHECK (pass_time_seconds > 0),
    completion_count INTEGER CHECK (completion_count >= 0),
    observed_unix_millis INTEGER NOT NULL,
    PRIMARY KEY (profile_id, season_id, activity_id, tier)
) STRICT;

INSERT INTO profile_dungeon_records (
    profile_id, character_id, display_name, deployment_id, region_id, realm_id,
    season_id, activity_id, tier, score, pass_time_seconds, completion_count,
    observed_unix_millis
)
SELECT old.profile_id, p.character_id,
    json_extract(p.public_projection_json, '$.display_name'),
    p.deployment_id, p.region_id, p.realm_id,
    old.season_id, old.activity_id, old.tier, old.score, old.pass_time_seconds,
    old.completion_count, old.observed_unix_millis
FROM profile_dungeon_records_v1 old
JOIN profiles p ON p.profile_id=old.profile_id;

DROP TABLE profile_dungeon_records_v1;

CREATE INDEX profile_dungeon_records_by_time
ON profile_dungeon_records (
    season_id, region_id, activity_id, tier, pass_time_seconds,
    observed_unix_millis, profile_id
);
