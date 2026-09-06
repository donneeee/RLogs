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

## Current migration state

As of 2026-09-06, `rlogs-submissions` is a Cloudflare Pages API gateway with an
authoritative service binding to the private `rlogs-backend` Worker. Production
contains no loopback or `trycloudflare.com` origin. Cloudflare KV contains the
digest-audited public read model, profile/loadout/photo assets, account hashes,
UID claims, memberships, projections, and reconciliation records migrated from
the former receiver. A SQLite-backed Durable Object owns Discord OAuth, new web
sessions and app tokens, account updates, parse visibility, and serialized
profile publication. Profile publication reproduces the Rust package digest
and device-bound HMAC contracts before updating a UID claim.

Parse upload creation, chunk ingestion, and finalization remain fail-closed with
HTTP 503. They must not be enabled until a hosted pinned Rust replay succeeds
end-to-end. The current Cloudflare account is on Workers Free; Containers are a
Workers Paid feature, and R2 has not been enabled. Enabling those account
capabilities (or approving an equivalent hosted verifier) is therefore a
deployment prerequisite for the remaining parse-write path. The local receiver
remains a development fixture and migration source only, never a production
dependency.
