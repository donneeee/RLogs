# GitHub Pages site with a Cloudflare-hosted rLogs backend

## Non-negotiable ownership boundary

No public rLogs service may depend on a user's or developer's PC. The desktop
application captures, seals, verifies locally, and uploads evidence. It is not
the website, API, database, artifact store, or public verification server.

GitHub is the source-control host and serves the static website through GitHub
Pages. Cloudflare owns the API, database, private objects, and verification
runtime:

```text
GitHub website repository push
  -> required tests and build
  -> GitHub Pages static website

GitHub application repository push
  -> required tests and build
  -> Cloudflare backend deployment

rLogs desktop
  -> Cloudflare API
       -> upload coordinator
       -> durable artifact storage
       -> hosted Rust verifier/replay worker
       -> metadata database
  -> immutable receipt

Browser
  -> GitHub Pages site
  -> Cloudflare API
```

## Required production components

1. **Site:** GitHub Actions builds the website repository and GitHub Pages
   serves the generated static site. The site contains no submission data,
   database credentials, server secrets, or authoritative verification state.
2. **API edge:** a Cloudflare Worker owns routing, CORS, authentication,
   request limits, idempotency, and stable public URLs. It never proxies to a
   workstation or a `trycloudflare.com` hostname.
3. **Metadata:** a dedicated rLogs D1 database stores accounts, device-token
   hashes, immutable UID claims, upload sessions, report indexes, visibility,
   profiles, likes, reconciliation status, and audit records.
4. **Artifacts:** a private rLogs R2 bucket stores resumable upload parts,
   sealed `.rlog` evidence, profile images, and generated projections. Objects
   are not made public directly; authorized Worker routes serve reviewed data.
5. **Verification compute:** the repository's pinned Rust verifier runs in a
   Cloudflare-hosted container or equivalent hosted build worker. Every job is
   tied to an exact verifier version, game build, protocol-pack digest, input
   digest, and output digest. No browser-side result becomes authoritative.
6. **Coordination:** Durable Objects and/or Queues serialize upload
   finalization and verification jobs. Retries are idempotent and cannot create
   duplicate reports or double-apply ownership.
7. **Secrets:** Discord OAuth, token peppers, and deployment credentials live
   only in Cloudflare/GitHub secret stores. They are never committed or sent to
   the desktop plug-in UI.
8. **Operations:** hosted health checks cover API, database, artifact storage,
   queue age, verifier failures, and deployment version. Alerts distinguish a
   client capture failure from a hosted-service failure.

## Submission lifecycle

1. The desktop seals an immutable artifact and persists its local queue entry.
2. The API creates or resumes an upload keyed by the artifact digest.
3. Parts are written to private object storage and acknowledged by digest.
4. Finalization checks the complete object digest and enqueues verification.
5. Hosted verification replays the artifact with the pinned build contract,
   validates privacy and conservation invariants, and writes an immutable
   projection plus audit receipt.
6. Public or private indexes are updated transactionally only after successful
   verification. Automatic-publication consent never bypasses verification.
7. Repeating any request returns the same upload/report state.

## Deployment gates

Backend production deployment is blocked unless all of the following are true:

- the site and API revisions identify the same compatible contract version;
- D1 migrations are applied and backward-compatible with the active clients;
- artifact storage and verification queues pass an end-to-end canary upload;
- no production configuration contains loopback or `trycloudflare.com` URLs;
- the public health endpoint proves hosted dependencies rather than merely
  returning a process-level success response;
- rollback preserves immutable artifacts, claims, and report receipts.

The repository workflow `.github/workflows/deploy-cloudflare.yml` is the
production deployment path. It tests and deploys the private Worker first,
deploys the Pages gateway second, and then runs
`tools/smoke-test-cloudflare-production.mjs` against the public hostname. The
Worker health response includes the deployed Git commit, so a successful
workflow proves that traffic reached the requested revision rather than an
older healthy deployment.

Automatic deploys require two GitHub Actions repository secrets,
`CLOUDFLARE_ACCOUNT_ID` and `CLOUDFLARE_API_TOKEN`, plus the repository variable
`CLOUDFLARE_DEPLOY_ENABLED=true`. Without that explicit variable a push safely
skips production deployment; a maintainer can still invoke the workflow
manually, where missing credentials fail before any deployment starts. The API
token should be scoped only to the rLogs Workers and Pages resources needed by
Wrangler.

## Current migration state

As of 2026-09-06, `rlogs-submissions` is a Cloudflare Pages API gateway with an
authoritative service binding to the private `rlogs-backend` Worker. Production
contains no loopback or `trycloudflare.com` origin. The migration-managed
`rlogs-production` D1 database is bound as `RLOGS_DB`; its health marker and 16
production metadata tables must be present before the service reports healthy.
Cloudflare KV temporarily retains the digest-audited public read model and
migrated profile/loadout/photo assets while records are moved into D1 and R2.
A SQLite-backed Durable Object owns Discord OAuth, new web sessions and app
tokens, account updates, parse visibility, photo likes, and serialized profile
publication. Profile publication reproduces the Rust package digest and
device-bound HMAC contracts before updating a UID claim.

Parse upload creation, chunk ingestion, and finalization remain fail-closed with
HTTP 503 in production. The Worker now implements the authenticated resumable
ingress contract behind `RLOGS_PARSE_UPLOADS_ENABLED=true`: it validates the
sealed manifest, binds the session to the authenticated app token, stores each
digest-addressed client chunk in private R2, persists lifecycle metadata in D1,
and resumes by returning only missing chunks. Client chunks are separate R2
objects rather than R2 multipart parts because the installed desktop contract
uses 4 MiB chunks while non-final R2 multipart parts require at least 5 MiB.

Finalization is deliberately asymmetric: it queues the pinned verifier and
returns no successful receipt merely because the verifier answered HTTP 200.
The edge returns an accepted report only after the hosted verifier has replayed
the artifact and committed both the accepted upload state and report metadata
to shared D1. Thus an incomplete or compromised wake-up response cannot publish
unverified client data. The promotion flag is absent from production and health
continues to report `parse_uploads: false` until the flag, R2 binding, verifier
binding, D1 schema, and public-read model are all present and an end-to-end
canary passes.

R2 is not enabled on the current account, so the Worker cannot yet retain the
`.rlog` chunks. The account is also on Workers Free, whose ordinary Worker CPU
allowance is not a safe execution budget for replaying a multi-megabyte sealed
log. Enabling R2 and Workers Paid/Containers (or approving and validating an
equivalent hosted verifier) is therefore the remaining account-level
prerequisite. The local receiver remains a development fixture and migration
source only, never a production dependency.

The verifier capacity decision is backed by the repository's read-only
`--benchmark-replay` command, which invokes the same two-pass attribution and
encounter reconstruction used by finalization. On the 2026-09-06 Windows
release build, a 16,590,766-byte sealed artifact containing 448,546 canonical
events produced one schema-12 report (1,625,658 JSON bytes) in 8,407 ms. A
3,799,658-byte artifact containing 96,641 events produced one report (471,252
JSON bytes) in 2,331 ms. A 39,998,055-byte interrupted artifact containing
32,254 events replayed in 662 ms and then failed closed with `NoCompletedRun`.
These figures are local reference measurements, not a Cloudflare runtime
benchmark; paid hosted compute still requires an end-to-end WASM or Container
measurement before write routes can be enabled.
