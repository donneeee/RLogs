# Desktop application

The Windows and Linux desktop host will manage capture, replay, plug-ins,
overlays, local logs, and website device pairing.

The current host is deliberately a loopback-only web application. It exercises
the same Rust runtime that a packaged desktop window will use later:

```powershell
cd apps\desktop\ui
npm install
npm run build

cd ..\..\..
cargo run -p rlogs-desktop-host
```

Open `http://127.0.0.1:7419`. The host refuses non-loopback bind addresses.
Its Session Recorder workspace provides:

- a sanitized `.rlog` replay through the real bounded Combat Meter plug-in;
- deterministic whole-run, segment, attempt, pause, and quality projections
  through the built-in Encounter Recorder;
- a sealed-log Event Viewer with exact raw canonical IDs, topic/kind/ID
  filtering, bounded forward-only pages, provenance details, and exact
  canonical JSON copy;
- background PCAP/PCAPNG processing through exact connection filtering, TCP
  reconstruction, the selected BPSR protocol pack, canonical decoding, sealed
  `.rlog` output, and Combat Meter;
- Windows process-owned live capture with automatic BPSR process and dumpcap
  interface discovery where available;
- cooperative Stop, which terminates only the private dumpcap ingress, drains
  owned frames, atomically publishes private capture evidence, then decodes and
  seals the canonical log.

Event Viewer verifies and caches the current sealed artifact before exposing
rows. Each filter owns one resumable stream cursor, reads at most 50,000 source
events or 100 milliseconds of scanning per request, returns at most 200 rows or
8 MiB, and replaces rather than accumulates browser table pages. Values that
may exceed JavaScript's exact integer range—including entity UUIDs, ability
IDs, and amounts—cross the host API as decimal strings.

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
only personal character observations from a fully verified sealed log, merges
partial updates, and atomically stores a current review package under
`runtime-data/profile-sync/packages/<game>/<deployment>/<region>/<server>/<UID>/`.
The UI shows bounded summaries and loads exact JSON only when requested.
Authentication, remote endpoints, and external transport remain disconnected.

The host also scans `plugins/installed/` at startup. Each child folder must
contain a valid `plugin.toml`. Newly discovered packages are disabled until the
user enables them in Plug-in Manager, and the desired state is stored in
`runtime-data/settings/plugin-enablement.v1.json`. Rescanning reports invalid
manifests, missing dependencies, dependency cycles, and blocked workspace
contributions without preventing unrelated valid packages from loading.

At this stage, installed packages publish validated metadata and workspace
descriptors only. The host does not execute their WASM, scripts, overlays, or
external processes until the sandbox/runtime boundary is implemented.

Live capture never starts automatically. Start it after entering the world if
login traffic should not even be observed by the bounded ingress. Credentials,
account-authentication payloads, private chat, and prohibited routes remain
excluded from canonical output regardless.

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
