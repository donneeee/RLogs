-- Indexed, packet-observed profile rankings. The nested profile field named
-- dungeon_id is the Master tier; activity_id is the actual seasonal dungeon.

CREATE TABLE profile_season_rankings (
    profile_id TEXT NOT NULL REFERENCES profiles(profile_id) ON DELETE CASCADE,
    season_id INTEGER NOT NULL CHECK (season_id > 0),
    master_score INTEGER NOT NULL CHECK (master_score >= 0),
    observed_unix_millis INTEGER NOT NULL,
    PRIMARY KEY (profile_id, season_id)
) STRICT;

CREATE INDEX profile_season_rankings_by_season_score
ON profile_season_rankings (season_id, master_score DESC, observed_unix_millis, profile_id);

CREATE TABLE profile_dungeon_records (
    profile_id TEXT NOT NULL REFERENCES profiles(profile_id) ON DELETE CASCADE,
    season_id INTEGER NOT NULL CHECK (season_id > 0),
    activity_id INTEGER NOT NULL CHECK (activity_id > 0),
    tier INTEGER NOT NULL CHECK (tier BETWEEN 1 AND 20),
    score INTEGER CHECK (score >= 0),
    pass_time_seconds INTEGER NOT NULL CHECK (pass_time_seconds > 0),
    completion_count INTEGER CHECK (completion_count >= 0),
    observed_unix_millis INTEGER NOT NULL,
    PRIMARY KEY (profile_id, season_id, activity_id, tier)
) STRICT;

CREATE INDEX profile_dungeon_records_by_time
ON profile_dungeon_records (
    season_id, activity_id, tier, pass_time_seconds, observed_unix_millis, profile_id
);
