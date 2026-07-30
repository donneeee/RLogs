# Combat timeline

The first executable bundled rLogs analysis plug-in. It consumes only
privacy-reviewed canonical events from sealed `.rlog` replay and produces a
versioned combat snapshot containing:

- encounter and active-combat duration;
- attributed damage, HP loss, shield loss, healing, overheal, and shielding;
- casts, hits, critical hits, deaths, and revives;
- position sample counts and raw path distance;
- actor and ability breakdowns;
- data-gap count and replay provenance.

Its displayed DPS uses attributed damage observed during explicit active combat
windows. rDPS is intentionally not guessed here; support redistribution belongs
to the separate versioned attribution plug-in.
