# BPSR marker action correlation

`bpsr-marker-action-correlation.mjs` is a read-only, offline triage tool for a
completed `capture-marker-audit.ps1` session. It narrows a timestamped marker
placement to client-to-server protocol-journal candidates, then lists
subsequent same-connection server returns and inbound changes.

It does not transmit, inject, replay, or construct game traffic. Its output is
correlation evidence, not proof that a candidate route places a marker.

```powershell
node tools/bpsr-marker-action-correlation.mjs analyze `
  --actions C:\private\capture.marker-actions.json `
  --session C:\private\capture.marker-session.json `
  --journal C:\private\capture.protocol.jsonl `
  --output C:\private\capture.marker-correlation.json

node tools/bpsr-marker-action-correlation.mjs verify `
  --report C:\private\capture.marker-correlation.json
```

The analyzer fails closed when the capture ID, scene, build, protocol-pack
digest, action-plan hash, session-bound action-ledger hash/count, timestamp
ordering, journal extent, or journal sequence is inconsistent. When the
session manifest binds a `protocol_journal` artifact, that hash is checked as
well. Raw carry-forward captures must also carry the same non-authoritative
pack source/captured-build stamp in the session and journal; a missing or
mismatched stamp is rejected. Use `--actions-sha256`, `--session-sha256`, or `--journal-sha256` to bind
copies against hashes retained outside the files.

For every marker, the report uses the immediately preceding idle interval as
a baseline (three seconds by default), removes exact duplicate records, and
prefers decoded children over same-coordinate frame containers. Candidate
scores describe only baseline novelty, action-window specificity, and nearby
inbound correlation. A journal gap touching either the baseline or response
window is retained and marks that window `gap-observed`; the score is reduced,
and `correlation_is_protocol_proof` remains `false` in every case.

Run the hermetic fixture coverage with:

```powershell
node tools/bpsr-marker-action-correlation.mjs self-test
```
