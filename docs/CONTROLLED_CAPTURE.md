# Controlled Global capture

This workflow is for private protocol research on the exact installed Global
Steam build. It does not capture the login flow and it never produces a
website-submission file.

## Before capturing

1. Start the game normally and finish logging in. For a stable-flow capture,
   enter the world first. For a process-aware world-load capture, stop at the
   authenticated server/character selection screen.
2. Close applications that generate unnecessary traffic.
3. Decide on one short scenario. Do not combine every test into one capture.
4. Do not open direct messages, account settings, payment screens, or login
   screens during capture.

## Capture one scenario

From the RLogs repository:

```powershell
$research = Join-Path $env:LOCALAPPDATA 'RLogs\private-research'
.\tools\windows\capture-global-steam.ps1 `
  -OutputDirectory $research `
  -CaptureId 'profile-baseline-001'
```

The helper resolves the running `BPSR_STEAM` process, snapshots only its exact
established TCP endpoint pairs, maps them to the active Npcap interface, and
passes that allowlist to `dumpcap`. It refuses to overwrite existing files.
Press Ctrl+C after the planned scenario.

For a timed capture that stops automatically, add `-DurationSeconds 90`.

For an authenticated character-selection-to-world transition, the local TCP
socket may change during loading. `-FollowProcessConnections` can follow a
replacement local port only while the remote game-server endpoint stays the
same:

```powershell
.\tools\windows\capture-global-steam.ps1 `
  -OutputDirectory $research `
  -CaptureId 'world-load-profile-002' `
  -DurationSeconds 120 `
  -FollowProcessConnections `
  -SeedConnectionsPath (Join-Path $research 'world-load-profile-001.connections.json')
```

This mode does not broaden capture to the entire adapter. Its private pcap may
contain another process only if that process connects to the same exact server
endpoint during the short capture. The process-owned connection allowlist is
applied before TCP reconstruction, so unrelated packets never reach protocol
framing or decoding. Optional seed evidence adds only endpoints previously
observed on connections owned by the game process; it does not copy those stale
connection tuples into the new allowlist.

Controlled retries on the current Global build proved that entering the world
can rotate both the remote address and port. A BPF filter cannot be widened
after `dumpcap` starts, so this helper cannot capture that complete transition.
Do not repeat the world-load test with progressively broader endpoint seeds.

Use the process-aware backend for transitions that can rotate servers:

```powershell
.\tools\windows\capture-global-steam-process-aware.ps1 `
  -OutputDirectory $research `
  -CaptureId 'world-load-process-001' `
  -DurationSeconds 180
```

This helper continuously reads the Windows TCP ownership tables for the
already-authenticated `BPSR_STEAM` process. Dumpcap streams TCP frames only to
RLogs memory; it is never given a broad capture-file path. RLogs retains a
short, bounded pending window for the first-SYN ownership race and writes only
frames whose exact bidirectional four-tuple is confirmed as game-owned.
Unattributed traffic is discarded before TCP reconstruction, protocol decoding,
or persistence. The capture stops on its bounded timer.

## Build the private journal

```powershell
$research = Join-Path $env:LOCALAPPDATA 'RLogs\private-research'
cargo run -p rlogs-protocol-journal -- `
  --private-research `
  --pack '.\protocol-packs\global\steam-24252055\pack.json' `
  --connections (Join-Path $research 'profile-baseline-001.connections.json') `
  --capture-id 'profile-baseline-001' `
  (Join-Path $research 'profile-baseline-001.pcapng') `
  (Join-Path $research 'profile-baseline-001.jsonl')
```

The JSONL contains opaque payload evidence. It remains local-only and must not
be committed, uploaded, or shared.

## Measure route coverage

```powershell
$research = Join-Path $env:LOCALAPPDATA 'RLogs\private-research'
cargo run -p rlogs-protocol-coverage -- `
  --pack '.\protocol-packs\global\steam-24252055\pack.json' `
  (Join-Path $research 'profile-baseline-001.jsonl')
```

Coverage output contains route IDs and counts, not packet payloads.

## Initial scenario set

Use separate captures in this order:

1. `profile-baseline`: stand still, then open the character/profile screen;
2. `world-load-profile`: while already authenticated, capture a
   character-selection-to-world transition to look for the complete
   `SyncContainerData` snapshot;
3. `equipment-change`: swap one known gear item and swap it back;
4. `skill-talent-change`: change one skill or talent and restore it;
5. `zone-change`: move between two known maps;
6. `combat-basic`: basic attacks, class skills, healing, and support buffs;
7. `death-revive`: controlled death and revival;
8. `dungeon-run`: a complete run, captured after its server connection is
   established.

Every mapping promoted from these captures needs the exact build, scenario,
route tuple, privacy classification, sanitized fixture, and expected canonical
events.

The first `profile-baseline` observation did not resend the complete character
snapshot. It produced one `SyncContainerDirtyData` update whose dirty tree
included `char_base` and current-build field `104`. The route stays opaque
because `char_base` can contain both allowed character fields and prohibited
account identifiers.

The endpoint-filtered attempts did not observe the full snapshot and proved
that world entry can rotate servers. The process-aware
`world-load-process-001` retry then followed three exact process-owned
connections and observed the complete `SyncContainerData` snapshot with zero
interface drops. Its sanitized result is recorded under the exact-build
protocol pack; raw capture and journal files remain local-only.

Do not repeat the world-load scenario for this build. The next controlled
scenario is `equipment-change`: swap one known gear item, wait for the dirty
update, then restore the original item.
