# Character profiles

Consent-aware character snapshot display and website profile synchronization.

The desktop now publishes a separate **BPSR Profile Sync** workspace with its
own disabled-by-default consent and automatic-sync settings. It is deliberately
independent from Log Uploader consent: enabling combat-log submission must not
permit profile submission, or vice versa.

When enabled, the plug-in can replay a sealed `.rlog`, select only
`personal_gameplay` BPSR profile observations, merge partial character updates,
and write one current review package per character identity. Public social
lookups for other characters are excluded. The folder path is human-readable
by game, deployment, region, realm/world, and public character UID.

Packages can be inspected as exact JSON in the UI. Each package carries its
source session, build, protocol-pack identity, canonical log digest,
observation count, last event sequence, safe relative website endpoint, and
privacy-reviewed payload. Device pairing, authentication, and external
transport are not wired yet. This feature must never collect passwords, login
tokens, credentials, or private account containers.
