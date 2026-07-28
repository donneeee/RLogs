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
- character name and stable character identifier;
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

Unknown routes are opaque local research data until reviewed. They are not
automatically decoded, exposed to ordinary plugins, or placed in `.rlog`
submissions.

Website submissions use a typed allowlist. Raw packet journals are never the
submission artifact.
