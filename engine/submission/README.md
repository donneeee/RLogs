# RLogs submission

This crate defines the client-side contract for RLogs' built-in uploader.
Transport, authentication, UI, and the leaderboard service are deliberately
outside it.

The state machine enforces these invariants:

- live and post-run uploads reference chunks produced by the local `.rlog`
  writer;
- chunk order, offsets, lengths, and SHA-256 digests are explicit;
- live sessions may add chunks while uploading;
- post-run sessions must already be sealed before uploading;
- retries ask for unacknowledged chunks instead of rebuilding the log;
- finalization requires every chunk to be acknowledged;
- the server receipt must identify the sealed local artifact;
- no client API accepts or declares a ranked score.

The eventual HTTP client will translate this state into resumable requests.
The server will replay the artifact and return report and verification
information through a separate server-owned result model.
