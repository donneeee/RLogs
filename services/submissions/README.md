# Submission service

This service is the first website backend for rLogs. It owns:

- resumable, digest-verified `.rlog` ingestion;
- private content-addressed artifact storage;
- deterministic server replay into a compact public parse projection;
- link-addressed public or unlisted parses;
- uploader provenance kept separate from every character in a parse;
- exact-instance run groups that retain every contributing report instead of
  silently choosing or merging one observer's timeline;
- Discord-backed website accounts and one-time desktop app-token issuance;
- immutable first-owner UID claims from sealed personal profile evidence;
- current per-character profile and module publication;
- a public browse catalog whose region, activity, scene, difficulty, and run
  state filters are derived from accepted reports.

It does **not** implement rankings or leaderboard scores.
The raw artifact is never served by a public endpoint. Public projections are
server-produced and retain exact client-build and protocol-pack provenance.

Only the desktop's separately sealed privacy export is accepted. PCAPs,
connection evidence, link/IP/TCP headers, IP and MAC addresses, chat, and
account/contact/authentication fields are excluded. The receiver replays every
artifact through the same privacy validator and rejects an outdated or unknown
privacy-policy digest before writing accepted state or creating a GitHub
archive job. Character names/UIDs and gameplay/profile evidence remain because
they are required for run correlation and rDPS research.

The application does not persist or emit a contributor's transport IP address.
The HTTPS platform still processes it transiently to deliver the connection,
as every Internet host must, but it is not part of the rLogs evidence package,
catalog, projection, or private GitHub archive.

## Run locally

```powershell
$env:RLOGS_SUBMISSION_DATA = "C:\path\to\rlogs-submission-data"
cargo run -p rlogs-submission-service
```

The default listen address is `127.0.0.1:8787`. Set
`RLOGS_AUTH_INTROSPECTION_URL` to the private pairing provider's complete
introspection endpoint. The receiver forwards the bearer token for validation
but never stores it. Read endpoints stay public. `RLOGS_PUBLIC_SITE_URL`
controls the share URL returned in receipts.

The process refuses to listen outside loopback without authentication. A
disposable, deliberately unauthenticated test can override that guard with
`RLOGS_ALLOW_UNAUTHENTICATED_INGEST=1`; production deployments must not use
that override. `RLOGS_INGEST_KEY` remains a temporary single-developer fallback
and cannot be combined with introspection.

An external introspection provider remains supported for deployments that
already have an identity service. It returns a submitter ID and device ID; the
receiver never stores the forwarded bearer token. The built-in Discord flow
below provides the same identity contract for this deployment.

## Accounts, app tokens, and UID claims

The website authentication flow intentionally follows the original 2025
Resonance Logs boundary: a user signs in with Discord, receives a short-lived
website session, and explicitly generates a desktop app token. Plaintext web
and app tokens are returned only at issuance and only peppered SHA-256 digests
are persisted. The desktop stores its app token in Windows Credential Manager.

Configure all four values together before starting the receiver:

```powershell
$env:RLOGS_DISCORD_CLIENT_ID = "Discord application client ID"
$env:RLOGS_DISCORD_CLIENT_SECRET = "Discord application client secret"
$env:RLOGS_PUBLIC_API_URL = "https://rlogs-submissions.example.workers.dev"
$env:RLOGS_DISCORD_CALLBACK_URL = "https://rlogs-app.github.io/account/"
$env:RLOGS_AUTH_TOKEN_PEPPER = "at least 32 random characters"
```

Set the Discord application's redirect URI to `RLOGS_DISCORD_CALLBACK_URL`.
When it is omitted, the receiver uses the legacy stable API origin followed by
`/v1/auth/discord/callback`. The website callback keeps the internal API
hostname out of the browser's address bar and completes the exchange through
the constrained gateway. OAuth state and login codes are bounded, single-use
records. Browser sessions expire after 30 days. App tokens are independent per
device and never appear in a public profile.

The first authenticated `current.profile.json` for an exact
deployment/region/realm-or-world/character-ID tuple claims that UID only when
its HMAC-SHA256 proof matches the publishing device token and exact live
process-owned capture seal. Replays, offline processing, imports, shared logs,
unbound packages, and packages copied from another device are rejected. A
different account receives HTTP 409 and cannot replace an existing claim. The
same owner can publish only a newer live-proven package. The public profile
retains the complete privacy-reviewed BPSR envelope, including module inventory
and equipped-slot state under that character ID.

## Private GitHub research archive

The receiver can copy every accepted evidence package into a private GitHub
repository after validation. GitHub is a secondary research archive, not the
ingest endpoint: contributors authenticate only to the receiver and never
receive the repository credential.

```powershell
$env:RLOGS_GITHUB_ARCHIVE_REPOSITORY = "owner/private-evidence-repository"
$env:RLOGS_GITHUB_ARCHIVE_TOKEN = "server-only-fine-grained-token"
cargo run -p rlogs-submission-service
```

Use a fine-grained token restricted to that single private repository with
Contents write access. Keep the token in the receiver host's secret store; do
not place it in a plug-in, `.env` file committed to Git, projection, or tester
instructions.

Each sealed artifact gets one prerelease tagged with its full SHA-256 digest.
The release contains a server-produced projection, a digest-named evidence
manifest, and one or more digest-named binary artifact parts. Parts default to
512 MiB so large captures do not exceed GitHub's per-asset limit or need to be
loaded into RAM. `RLOGS_GITHUB_ARCHIVE_PART_BYTES` can set a value from 8 MiB
through 1 GiB. `RLOGS_GITHUB_API_URL` exists only for GitHub Enterprise or
loopback tests and otherwise defaults to `https://api.github.com/`.

Archiving is asynchronous and idempotent. A failed attempt leaves its job in
`archive-outbox/` and is retried without invalidating the already accepted
report. An `archive-receipts/` record is written only after every expected
release asset is present with the correct byte length. Starting the receiver
also reconciles older accepted projections into the outbox, so enabling the
archive later does not require hand-authored jobs.

An operator can drain the archive without opening the HTTP listener:

```powershell
cargo run -p rlogs-submission-service -- --archive-once
```

This is useful for deployment jobs and for verifying the private archive
credential before enabling the background worker.

An operator can also apply the receiver's exact privacy and integrity checks to
an existing sealed artifact without starting the HTTP listener:

```powershell
cargo run -p rlogs-submission-service -- --audit-artifact C:\path\to\artifact.rlog
```

For an end-to-end ingest test, an operator can produce the same separately
sealed privacy export and resumable upload manifest as the desktop client. The
output paths must not already exist, which prevents an accidental overwrite:

```powershell
cargo run -p rlogs-submission-service -- --prepare-submission `
  C:\path\to\completed-source.rlog `
  C:\path\to\private-upload-artifact.rlog `
  C:\path\to\upload-manifest.json
```

The resulting manifest defaults to unlisted visibility. This command prepares
local files only; it does not transmit them or expose a GitHub credential.

## Run correlation

Reports from different observers are grouped only when their canonical run
identity contains the same exact game instance ID in the same deployment,
region, scene, and client build. The game build prevents a historically reused
instance number from collapsing unrelated runs. The protocol-pack digest is
deliberately not part of game-run identity: two RLogs versions can record the
same server instance. A protocol-pack mismatch is instead exposed as a joint
replay blocker, so the website can show that the uploads are the same run
without mixing decoder evidence that has not been proven compatible.
Optional metadata such as world visibility is deliberately excluded because it
can differ between observers. If the exact instance ID is missing, the report
gets an artifact-local group and is never timestamp-matched or guessed into
another run.

The catalog exposes every contributing report ID, contribution count, and
distinct submitter count. Source artifacts and projections remain independent,
so disagreements can be compared and resolved as evidence rather than erased.
Catalog schema 6 derives participant coverage from the private membership index
that was independently rebuilt from each sealed artifact. Public projections
may redact remote character IDs; reconciliation schema 10 therefore uses those
private IDs for set membership and coverage math without adding redacted remote
UIDs to the public character manifest.

### Cross-vantage rDPS reconciliation

An exact-instance run group is also the server boundary for reconciling the
same fight recorded by different local players. A player who is remote in one
artifact can supply a privacy-reviewed local character-profile witness in a
second artifact. The catalog records how many distinct run participants have
such local witnesses and reports whether the group is still single-vantage,
has cross-vantage evidence available, or has a completed reconciliation.

Joint replay follows these rules:

1. Partition an exact-instance group by client build and protocol-pack digest.
   Evidence never crosses either boundary.
2. Select one sealed artifact as the canonical combat-event spine. Other
   artifacts are evidence witnesses; their duplicate casts, damage, healing,
   and status events are never added to the spine.
3. Match participants by stable game character ID and retain the supplying
   report ID, artifact digest, canonical event sequence, and observation time
   for every imported fact.
4. Prefer exact event-local evidence from the spine, then exact local evidence
   from another observer of the same run, then formula-bounded inference. A
   missing field is never converted to zero.
5. Reject or surface conflicting exact witnesses instead of averaging them.
   Time-scoped loadout or attribute changes must be ordered before they can
   affect a counterfactual.
6. Run one conserved attribution replay over the canonical spine. Ordinary
   damage remains unchanged, each marginal is transferred once, and every
   result records whether it is exact, cross-vantage exact, or inferred.
7. Rebuild the derived reconciliation when a new observer report arrives,
   retaining the prior version and all input provenance.

The receiver implements exact-instance grouping, local-profile witness
inventory, sealed-witness verification, and conserved joint replay. Only
`personal_gameplay` profile observations qualify as local witnesses;
public/social profile observations are excluded. Each run keeps the latest
qualifying snapshot at or before its start and every qualifying in-run change,
committed by artifact digest, event sequence, observation time, and
profile-payload digest.

After that personal profile proves the local character, the same run also
commits that character's exact `EntityAttributes` and `TemporaryAttributes`
events. State replay begins at the latest authoritative pre-run snapshot,
retains every later delta through the run end, and records server game-time
when the packet carries it. Secondary artifacts still contribute no damage,
healing, cast, cooldown, or status events to the canonical combat spine.

Life Wave is the narrow exception at the evidence layer, not at the combat
spine. For the proven local recipient only, the receiver commits an exact
five-second child-status row (`2302421`, origin `1:2302420`) together with every
positive-healing row for that recipient on the same wire packet. The sealed
artifact must map both target and healer entity UUIDs back to stable character
IDs. During joint replay those paired rows become a projector-only trigger
timeline; they are never sent to the ordinary healing or status reducers. One
healer proves unique trigger ownership, multiple simultaneous healers remain
ambiguous, and a missing pair produces no inferred owner.

Stat Resonance is a second projector-only state exception. A secondary
artifact may contribute the proven local recipient's exact status lifecycle
only when the same wire packet also carries that recipient's authoritative
`EntityAttributes` attack family and identifies one stable external provider.
Conflicting or unresolved same-target status rows reject the witness. Joint
replay learns the exact single attack-family marginal from those paired
transitions and applies it only while the canonical spine's own status
lifecycle is active. The imported status and attributes never enter ordinary
combat reducers, so no damage, status, or state row is counted twice.

The verified personal profile itself is also a state witness. A pre-run
profile is inserted immediately after the canonical run entry; an in-run
profile is inserted only before a canonical event with a strictly later server
game-time. This exposes exact personal loadout/module state (including Life
Wave level 5 versus 6) to the same projector without importing a second copy of
combat results. Missing server time on an in-run profile is a reconciliation
blocker, not permission to apply the snapshot at run start.

`GET /v1/run-groups/{run_group_id}/reconciliation` returns the derived,
versioned evidence and replay product. It identifies the canonical event
spine, every contributing report, per-character local witness coverage, and
any multi-report snapshot set that still requires temporal ordering. The
manifest reports both total and server-time-alignable state-witness counts.
Before replay, the receiver reopens every selected sealed artifact and verifies
the committed event sequence, local-profile sensitivity and identity, state
kind, actor/entity identity, related healer identity when present, update kind,
observation time, server time, and payload digest. A mismatch blocks
reconciliation.

Ready state is remapped by stable character identity onto the canonical
runtime entity and inserted after that run's entry boundary. In-run changes
are admitted only before a canonical event with a strictly later
packet-authored server time; an equal timestamp remains unordered and cannot
affect that event. One two-pass BPSR attribution replay then emits reconciled
participants and an exact conservation receipt. The service marks
`attribution_replay_completed` true and the status `reconciled` only when the
ordinary per-actor damage map is unchanged, total contribution given equals
total contribution received, and total party rDPS damage equals raw party
damage. Source parses remain immutable beside this derived result.

Reconciliation schema 7 also carries an audit-only Swift Vortex candidate
report when effect `2110060` appears. A magnitude receipt requires the exact
current deployment/build/protocol identity, a complete Haste/normal-action-
speed/guide-action-speed baseline, one exact 10-second one-stack status
instance followed by an unconfounded three-lane positive delta, and an exact
symmetric three-lane delta when that same instance is removed. Four matching
paired receipts across at least two providers and two recipients satisfy only
the magnitude review gate. The report is kept outside participants and
conservation and always serializes `production_attribution_enabled: false`, so
neither receipts nor consensus can silently promote the candidate or change
rDPS.

Before the desktop stores a newly pasted app token, it calls
`GET /v1/auth/device` with that bearer token. The endpoint is read-only and
returns only the pseudonymous submitter and device IDs. Invalid, revoked, or
non-device credentials are rejected before Windows Credential Manager is
updated, so a local UID package cannot appear connected and then fail only at
its first claim attempt.

State replay readiness is independent from report count. A pre-run baseline
can be placed at the canonical run start without synchronizing two local
monotonic clocks. Every in-run state change requires packet-authored game-time;
otherwise the manifest names the character and missing-time count as a blocker.
Coverage is reported as distinct local-vantage characters over distinct run
participants and is explicitly partial or complete.

Multiple artifacts from the same local character are reported as
`multiple_reports_no_additional_vantage`; they do not satisfy the
`cross_vantage_evidence_available` gate. That status requires exact local
profile witnesses for at least two distinct run participants.

## Connect the desktop client

Configure the native desktop host, then restart it:

```powershell
$env:RLOGS_SUBMISSION_API_URL = "https://receiver.example.com"
$env:RLOGS_SUBMISSION_DEVICE_TOKEN = "the-token-shown-during-device-pairing"
```

The Log Uploader queue sends only missing chunks and finalizes after the
receiver verifies the complete artifact digest. Remote endpoints must use
HTTPS; plain HTTP is accepted only for loopback development.

## Container deployment

Build from the repository root so Cargo can see the complete workspace:

```powershell
docker build -f services/submissions/Dockerfile -t rlogs-submissions .
docker run --rm -p 8787:8787 -v rlogs-submissions:/data `
  -e RLOGS_AUTH_INTROSPECTION_URL="https://pairing.internal/v1/auth/introspect" `
  -e RLOGS_PUBLIC_SITE_URL="https://donneeee.github.io/rlogs-website/" `
  rlogs-submissions
```

The container listens on `0.0.0.0:8787`, stores all mutable state below
`/data`, runs as an unprivileged user, and exposes `/health` for hosting
platform probes. A persistent volume is required: ephemeral container storage
would discard private artifacts and the rebuildable catalog on redeploy.

## Small-sample Cloudflare gateway

GitHub Pages cannot safely receive uploads: a static page has nowhere to run
the replay service, and embedding a GitHub token would give every visitor the
archive credential. For a small invited test, GitHub remains the private
evidence archive while the independent Pages project in `cloudflare-gateway/`
provides a stable, rLogs-namespaced HTTPS API origin in front of this receiver.
It does not share Aniipedia's public Worker namespace or branding.

The gateway is deliberately a narrow streaming reverse proxy. It exposes only
the documented health, ingest, parse, and reconciliation routes; it cannot
serve the private artifact store. The receiver still owns authentication,
privacy validation, deterministic replay, persistence, and GitHub archiving.
The receiver host must remain online during this initial test phase.

Start a Cloudflare quick tunnel to the loopback receiver and copy its HTTPS
origin. Then create the independent Pages project, store that changing origin
as a project secret rather than committing it, and deploy the gateway:

```powershell
cloudflared tunnel --url http://127.0.0.1:8787 --no-autoupdate
Set-Location services/submissions/cloudflare-gateway
npm.cmd install
npx.cmd wrangler pages project create rlogs-submissions --production-branch main
"https://generated-name.trycloudflare.com" | npx.cmd wrangler pages secret put ORIGIN_BASE_URL --project-name rlogs-submissions
npm.cmd run deploy:pages
```

Set the `RLOGS_API_BASE_URL` GitHub Actions repository variable in
`donneeee/rlogs-website` to the deployed `rlogs-submissions.pages.dev` origin and rerun its
Pages workflow. The desktop submission URL is that same origin. Quick tunnels
have no uptime guarantee; replace the origin with a named tunnel or a hosted
receiver when testing expands beyond the initial invited sample.

## Storage layout

```text
artifacts/sha256/<prefix>/<digest>.rlog  private immutable source artifacts
projections/<report-id>.json            public-safe server projections
reconciliations/<run-group-id>.json      derived cross-vantage evidence manifests
accounts/                                private account, session, and token-hash records
profiles/<profile-id>/claim.json         private immutable UID owner mapping
profiles/<profile-id>/public.json        public current character profile and modules
profiles/catalog.v1.json                 rebuildable public profile catalog
uploads/<upload-id>/                     resumable temporary chunks
archive-outbox/<report-id>.json           retryable private archive jobs
archive-receipts/<report-id>.json         completed private archive receipts
catalog.v1.json                           compact rebuildable discovery index
```

The filesystem artifact boundary is intentionally narrow so a deployment can
replace it with S3/R2-compatible object storage without changing the public
API or website. The catalog is derived data and can be rebuilt from projection
files; dungeon and season filter values are never maintained by hand.
