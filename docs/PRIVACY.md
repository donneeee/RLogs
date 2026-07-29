# Privacy boundary

RLogs may decode data needed for combat analysis, dungeon reconstruction,
world context, and character profiles. Character data is not treated as
private account authentication data, but it is still subject to explicit
publication and upload controls.

## Allowed reviewed domains

- combat and encounter events;
- player and enemy actions;
- deaths, revives, casts, effects, positions, and movement;
- party, world, scene, map, and region context;
- character name and stable in-game character UID/identifier, including
  teammate UIDs already exposed as public gameplay identity;
- public guild identity and display data, including guild ID/name, badge,
  displayed character role/rank, and other fields visible on game profiles;
- class, level, build, equipment, progression, stats, and cosmetics;
- fields required to create or refresh a consented website character profile.
- public, system, and party chat for separately permissioned local plugins
  only; chat text is excluded from leaderboard submissions.

## Prohibited domains

- passwords or password-encryption material;
- login credentials;
- authentication, refresh, or session tokens;
- email addresses and private account-security data;
- payment or billing data;
- direct/private communications.

Guild chat, applications, invitations, moderation/member-management state,
permissions, and private guild records are not implied by the public guild
identity allowance. Public guild photos and descriptions are a separate
user-generated-content surface and require their own upload and image-safety
review.

An in-game character UID is allowed character identity, not an account secret.
RLogs keys it with deployment, region, and world/server context so equal-looking
values from different services cannot be merged accidentally. It must never be
substituted with or inferred from a platform account ID, publisher account ID,
open ID, login identifier, or session identifier; those remain prohibited.

Unknown routes are opaque local research data until reviewed. They are not
automatically decoded, exposed to ordinary plugins, or placed in `.rlog`
submissions.

Website submissions use a typed game-owned profile projected into Core's
game-neutral envelope. Core recursively rejects credential/account field names
and accepts only a safe relative endpoint; the host alone owns the configured
website origin and authentication. Raw packet journals are never the
submission artifact.

Game-file research follows the same boundary. Checked-in inventories may
contain relative client paths, digests, public game identifiers, table shapes,
asset identities, and aggregate relationship evidence. Raw client payloads,
absolute install paths, extraction tooling, arbitrary executable strings, and
all prohibited authentication/account material remain outside the repository.
