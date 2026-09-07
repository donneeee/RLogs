-- Mirror the authenticated account's future-parse publication consent in D1
-- so hosted verification can make the visibility decision transactionally.
-- The Auth Durable Object remains the authority and refreshes this value on
-- every authenticated desktop upload request.

ALTER TABLE accounts
ADD COLUMN publish_verified_parses INTEGER NOT NULL DEFAULT 0
CHECK (publish_verified_parses IN (0, 1));
