# RLogs submission

This crate defines game-neutral client-side contracts for log uploads and
privacy-reviewed website payloads. Authentication, UI, concrete HTTP clients,
and the leaderboard service remain outside it.

Game plug-ins may construct a `WebsitePayloadRequest` using a safe relative
endpoint. Core validates its game/schema identity, routing fields, size, JSON
shape, and prohibited credential/account field names. Only the host combines
that request with a user-configured website base URL and authentication, so a
game plug-in cannot redirect credentials to another host.

The state machine enforces these invariants:

- post-run artifacts are accepted only after the `.rlog` header, canonical
  sequence, event limits, integrity seal, and end-of-file are verified;
- verification, the exact full-file digest, and deterministic per-chunk
  digests are produced in one bounded streaming pass;
- live and post-run uploads reference chunks from the exact local `.rlog`
  byte layout;
- chunk order, offsets, lengths, and SHA-256 digests are explicit;
- live sessions may add chunks while uploading;
- post-run sessions must already be sealed before uploading;
- retries ask for unacknowledged chunks instead of rebuilding the log;
- finalization requires every chunk to be acknowledged;
- the server receipt must identify the sealed local artifact;
- no client API accepts or declares a ranked score.

The default post-run chunk size is 4 MiB, memory per chunk is capped at
16 MiB, and the default artifact limit is 16 GiB. The full-file SHA-256
identifies the uploaded bytes; the separate canonical-content SHA-256 from the
`.rlog` seal identifies the event stream. Both are exposed for diagnostics.

`QueuedSubmission` is the validated, game-neutral persistence contract for a
verified post-run artifact. The desktop stores one entry per exact full-file
SHA-256 instead of maintaining one growing mutable index. Its local artifact
path is host-only state and is not part of `UploadManifest`. Deserialization
rechecks the schema, post-run state, digest identity, chunk offsets/lengths,
artifact length, and path bounds before an entry can be treated as a draft.
The current trusted game pack owns its route-level privacy classifications, so
the draft records that exact immutable pack digest as both the protocol-pack
and privacy-policy digest. A separately versioned privacy artifact can replace
the latter without changing the queue format.

Before transport, `QueuedSubmission::verify_artifact` compares a newly
stream-verified artifact against the queued full-file digest,
canonical-content digest, byte length, deterministic chunk manifest, log
format, capture session, region, client build, and protocol-pack digest. A
previous verification result is never sufficient proof for a later upload
because the local file can change afterward.

The eventual HTTP client will translate this state into resumable requests.
The server will replay the artifact and return report and verification
information through a separate server-owned result model.

`MockSubmissionReceiver` is the bounded game-neutral test counterpart to that
future service. It validates the upload manifest and exact chunk bytes,
persists only validated acknowledgement state, supports idempotent retries, and
returns a deterministic replayed receipt only after every chunk is present.
The desktop dry run serializes and restores both sides mid-upload, proving
restart recovery without authentication or external network access.
