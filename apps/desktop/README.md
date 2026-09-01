# Desktop runtime

The Windows and Linux desktop host will manage capture, replay, plug-ins,
overlays, local logs, and website device pairing.

The reusable desktop runtime is shared by the native Tauri application and an
explicit loopback-only browser diagnostics mode.

For the normal native application, from the repository root:

```powershell
npm --prefix apps/desktop/ui run build
cargo run -p rlogs-app
```

For browser diagnostics:

```powershell
cd apps\desktop\ui
npm install
npm run build

cd ..\..\..
cargo run -p rlogs-desktop-host
```

Open `http://127.0.0.1:7419`. The host refuses non-loopback bind addresses.
Debug builds include a developer-only Session Recorder workspace that is not
bundled, cataloged, or routable in the public installer. It provides:

- a sanitized `.rlog` replay through the real bounded Combat Meter plug-in;
- deterministic whole-run, segment, attempt, pause, and quality projections
  through the built-in Encounter Recorder;
- a sealed-log Run Report showing run, mobbing, boss, pull, retry, winning
  attempt, pause, gap, and leaderboard-disposition evidence without
  recalculating it in the browser;
- a sealed-log Event Viewer with exact raw canonical IDs, topic/kind/ID
  filtering, bounded forward-only pages, provenance details, and exact
  canonical JSON copy;
- background PCAP/PCAPNG processing through exact connection filtering, TCP
  reconstruction, the selected BPSR protocol pack, canonical decoding, sealed
  `.rlog` output, and Combat Meter;
- automatic Windows process-owned monitoring using the saved network device;
- continuous in-memory TCP reconstruction and BPSR decoding for the lifetime
  of the game process, including newly opened game sockets;
- build-aware Steam pack selection from the running executable and
  `appmanifest_3681810.acf`: exact promoted packs are preferred, followed by
  exact static candidates and then the nearest compatible pack;
- visible provisional operation for an unpromoted or compatible pack. History,
  overlays, submissions, and best-effort rDPS remain active, while runtime
  status identifies both the observed and source builds and warns that changed
  routes may affect results;
- non-blocking rDPS capability preflight: missing evidence families are named
  in runtime status, but monitoring continues with all available decoders and
  retains the undecoded records needed to repair the pack;
- a private zero-extra-payload-copy JSONL journal for every provisional run,
  retaining framed `World`/`WorldNtf` records and transport gaps under
  `private-research/live-journals/`; login and account services are excluded;
- packet-authoritative dungeon persistence: entry opens a run `.rlog`,
  completion seals it in a background worker, and monitoring immediately
  continues for the next entry;
- conserved incremental rDPS in the live overlay, followed automatically by a
  serialized two-pass replay of every completed sealed run. History keeps the
  provisional subtotal visible and labels it as queued, then atomically swaps
  in the exact projection after remote-factor learning and conservation checks;
- cooperative restart, which drains the current decoder and marks an open run
  incomplete without turning a completed run into a leaderboard candidate.

Event Viewer verifies and caches the current sealed artifact before exposing
rows. Each filter owns one resumable stream cursor, reads at most 50,000 source
events or 100 milliseconds of scanning per request, returns at most 200 rows or
8 MiB, and replaces rather than accumulates browser table pages. Values that
may exceed JavaScript's exact integer range—including entity UUIDs, ability
IDs, and amounts—cross the host API as decimal strings.

Run Report uses that same verified sealed artifact and replays the built-in
Encounter Recorder on demand. The API returns its typed reducer snapshot,
bounded by the plug-in runtime's event and 16 MiB output limits. The UI
validates the complete contract, rejects contradictory eligibility evidence,
and presents both a human-readable segmented report and exact JSON suitable
for comparing with future server replay. Unknown BPSR meanings remain raw IDs
or unresolved labels.

Every completed real capture also prepares—but does not upload—a deterministic
resumable artifact manifest and persists a local draft under
`runtime-data/submissions/queue/`. Each draft is named by the exact artifact
SHA-256 and is atomically published as a separate bounded JSON file. The
sanitized reference replay is explicitly excluded. New drafts use the Log
Uploader's configured default visibility; the disabled-by-default setting is
unlisted.

The Last Session tab shows the exact file digest, canonical-content digest,
byte length, chunk count, and local queue result. The separate Log Uploader
workspace can rescan and inspect local drafts, artifact presence/length,
region/build metadata, and isolated invalid-file diagnostics.

The same tab can recover a previously completed `.rlog` when its original PCAP
is unavailable. Import canonicalizes the selected path and performs the full
bounded seal, EOF, file-hash, and chunk-hash pass before adding a draft.
Sanitized reference fixtures cannot pass the required immutable protocol-pack
digest. Import does not copy the `.rlog`.

Each draft also has a **Re-verify exact artifact** action. It re-reads the
entire file and compares every immutable queue identity. The displayed
verification time is diagnostic only and is not persisted as permission to
upload; a future uploader must invoke the same verification immediately before
transport. Only one full import/re-verification job runs at a time, while the
bounded localhost request workers keep other controls responsive.

Log Uploader and BPSR Profile Sync have independent, atomically persisted
opt-in policies and both default off. Log Uploader's local dry run invokes the
real upload state machine, forces and restores a mid-upload restart, validates
chunk acknowledgements and the final receipt, reports zero external requests,
and leaves the real queue and artifact unchanged. BPSR Profile Sync projects
only personal character observations from a live process-owned parse, merges
partial updates, binds the exact session seal and package to the authenticated
device token, and atomically stores a current review package under
`runtime-data/profile-sync/packages/<game>/<deployment>/<region>/<server>/<UID>/`.
Reference replay, offline processing, imported `.rlog` files, copied history,
and packages bound to another device cannot claim a UID.
The UI shows bounded summaries and loads exact JSON only when requested.
Remote transport remains opt-in. The public app connects only to the fixed
rLogs service and never displays its infrastructure hostname. The uploader
keeps a device bearer token in process memory and sends it only to that
validated service.

Debug contributor builds may override the service through
`GET/POST /api/submissions/connection` or `RLOGS_SUBMISSION_API_URL`. The public
build ignores endpoint overrides. The native host persists only the validated
endpoint URL in `runtime-data/settings/submission-connection.v1.json`;
on Windows, the bearer token is stored under the
`rLogs/submission-device-token/v1` target in Windows Credential Manager. API
responses expose only whether a credential exists and never return its value.
`POST /api/submissions/connection/disconnect` removes the credential and the
saved endpoint. HTTPS is mandatory except for loopback development receivers.
When Log Uploader and automatic combat logs are enabled, a dedicated bounded
worker sends one draft at a time without blocking packet capture. A verified
server receipt advances the durable queue entry to `submitted`; failures leave
the draft intact for retry.

The host also scans `plugins/installed/` at startup. Each child folder must
contain a valid `plugin.toml`. Newly discovered packages are disabled until the
user enables them in Plug-in Manager, and the desired state is stored in
`runtime-data/settings/plugin-enablement.v1.json`. Rescanning reports invalid
manifests, missing dependencies, dependency cycles, and blocked workspace
contributions without preventing unrelated valid packages from loading.

Installed browser and native-developer packages can publish validated local
HTML surfaces. Native-developer packages must explicitly request the unsafe
native-execution capability and remain disabled until the user enables them.
The host then supervises the validated package entrypoint as a hidden child
process, supplies package-scoped data/asset paths, records output privately,
and stops it when the package is disabled or rLogs exits. Sandboxed WASM and
ordinary external-process adapters remain future runtime boundaries.

Process-owned monitoring starts automatically while BPSR is running and the
saved adapter is available. Pre-entry traffic may update bounded in-memory
network/protocol state, but it is not written into dungeon logs. Credentials,
account-authentication payloads, private chat, and prohibited routes remain
excluded by the compiled game privacy policy; no broad or pre-entry PCAP is
created by continuous mode.

`ui/` is the framework-free shell prototype. It deliberately contains only
host responsibilities:

- draggable plug-in workspaces in the left navigation;
- real, keyboard-accessible tabs for the selected plug-in on the right;
- one mounted tab surface at a time;
- blank, loading, and failed-plug-in states;
- host-owned Plug-in Manager and Settings destinations.

Plug-ins publish workspace and tab descriptors through `rlogs-plugin-api`.
They own their tab content. The shell does not import first-party feature
implementations, so the same core can host parsers for games other than Blue
Protocol: Star Resonance.

When Vite is run without the Rust host, the development adapter supplies safe
sample descriptors and can render a true zero-plug-in state. When served by
the Rust host, the UI automatically selects the local runtime adapter and shows
only the packages actually discovered on disk. Website pairing is still an
unconnected Profile Sync option; real secrets will be generated by the native
host and stored in the operating system credential vault, not in browser
storage or plug-in folders.
