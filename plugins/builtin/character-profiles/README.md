# Character profiles

Consent-aware character snapshot display and website profile synchronization.

The desktop now publishes a separate **Account & Profiles** settings tab with its
own disabled-by-default consent and automatic-sync settings. It is deliberately
independent from Log Uploader consent: enabling combat-log submission must not
permit profile submission, or vice versa.

When enabled, the plug-in selects only `personal_gameplay` BPSR profile
observations from a live process-owned parse, merges partial character updates,
and writes one current review package per character identity. Reference replay,
offline processing, imported `.rlog` files, and copied history are ineligible
for UID claims. Public social lookups for other characters are excluded. The
folder path is human-readable by game, deployment, region, realm/world, and
public character UID.

Packages can be inspected as exact JSON in the UI. Each package carries its
source session, build, protocol-pack identity, canonical log digest,
observation count, last event sequence, safe relative website endpoint, and
privacy-reviewed payload. Its HMAC-SHA256 proof binds the package contents and
exact live session seal to the authenticated device token; the receiver
recomputes that proof before accepting a claim. Device pairing, authentication, and external
transport use the shared **rLogs account connection** shown in this tab. The
desktop validates a per-device app token before Windows Credential Manager
stores it. Publishing the first device-bound live personal package claims that exact
region-scoped UID for the authenticated account; later packages from the same
account update its profile and per-character modules. This feature must never
collect passwords, Discord login tokens, or private account containers.
