# Automarker read-only runtime probe

`rlogs-bpsr-automarker-lifecycle-probe` is a separate research executable for
one question: can the exact build-`25247556` client expose the normal indicator
press/release and position lifecycle through an out-of-process read-only
observation?

The probe is compiled against the reviewed executable and `GameAssembly.dll`
sizes and SHA-256 digests. It also requires Steam app `3681810`, build
`25247556`, a unique `BPSR_STEAM` process, and exact running-module paths. It
opens the process with only `PROCESS_QUERY_INFORMATION | PROCESS_VM_READ`.

It follows one compile-time allowlisted singleton chain. The MethodInfo's
declaring-class generic context must resolve to the same inflated class as the
companion TypeInfo slot before static fields are read. Encoded/uninitialized
slots fail closed and are never initialized. The probe validates every managed
object by IL2CPP class name and namespace, checks pointer alignment and
committed/readable region bounds, and reads only the skill input slot/press
fields and `IndicatorPos` XYZ. Roots bracket two identical state reads; a
changed root or mixed state is discarded. The observer performs no
scan, injection, debugger attach, thread suspension, function invocation,
memory write, packet operation, mouse click, or placement action. `Place`
remains disabled.

The executable also contains a separately armed, bounded input canary. Default
invocation remains read-only. The canary accepts only the literal compiled mode
token exposed by the launcher, requires the reviewed game window to be in the
foreground, and requires the user to have manually selected exact Marker 1. It
class-validates `ZIndicatorMgr`, requires skill `1101`, slot `201`, PC point
mode, release eligibility, the exact 18-metre range, and a settled finite
position before emitting any input.

The armed calibration emits four six-pixel relative moves in the exact order
`+X, -X, +Y, -Y`. Each move is paired with its subsequent settled observation,
including a monotonic relative timestamp, position, current velocity, stability
gap, and measured stability delta. Before every input it revalidates foreground
ownership, the exact Marker 1 lifecycle, and the original class-validated root
context. Any change fails the run and prevents further movement. Escape is sent
at the end only while the same Marker 1 context remains active in the
foreground game. It never emits a mouse button, Enter, Normal Attack, or
placement confirmation. Its receipt contains no PID, account/session identity,
raw address, or filesystem path.

Receipt schema v6 retains a sanitized diagnostic even when armed preflight is
rejected before input. It reports acquisition/class/coherence status, whether
the allowlisted roots stayed unchanged, each exact Marker 1 field gate, the
safe observed scalar flags and IDs, indicator parameters and range, input-slot
state, current velocity, camera/enter state, and the measured settle evidence.
It does not relax any gate: a failed field or settle check still returns before
`SendInput`. Default read-only mode already records the same raw lifecycle
scalars as ordinary transition events when Marker 1 remains active long enough
to be sampled; v4 makes the armed failure self-diagnosing in one receipt.

The first v3 live rejection exposed no failed field. Exact-build disassembly of
`ZIndicatorMgr.buildIndicatorData` subsequently corrected one preflight
interpretation: the marker table array `[1, 18]` supplies indicator type `1`
and maximum distance `18`; missing elements 2 and 3 default `Param1` and
`Param2` to `1`. Therefore the reviewed Marker 1 scalar expectation is
`Type=1`, `MaxDistance=18`, `Param1=1`, `Param2=1`. The v4 diagnostic retains
separate observed values and pass/fail results for all four fields.

The process boundary is permanently read-only for anti-cheat safety. The probe
requests only `PROCESS_QUERY_INFORMATION | PROCESS_VM_READ`; it has no remote
write/all-access right, remote allocation or thread creation, DLL injection,
internal game-function invocation, or packet synthesis surface. The armed
calibration is limited to ordinary foreground Windows mouse movement and Escape
after all read-only gates pass. Source and receipt-policy tests enforce this
boundary.

The desktop capture path also exposes a diagnostic-only local-player position
from legitimate outbound `World.UseSlot` requests on exact build `25247556`.
It strictly verifies the reviewed route and pack, the authenticated gameplay
envelope, required action identity, finite XYZ values, and the game-owned
`sessionSequence`. Only XYZ, sequence, capture time, and a host receipt time are
published. The observation is bound to the active capture session and an
already packet-observed scene/map, rejects sequence or capture-time regression,
and is erased on session or scene/map change. It is intentionally not wired to
the planner, any input path, or a distance policy. It exists only as evidence
for the game-owned request reconstruction boundary: ordinary skill traffic is
activity-driven and cannot guarantee a fresh pre-pull position, while a marker
request arrives only after the click it could not safely authorize.

## Package and run

The delivery package contains the native probe, a launcher, and this README;
it does not require Cargo or a repository checkout. Start the reviewed Steam
client, enter a safe scene, and run:

```powershell
.\run-bpsr-automarker-lifecycle-probe.ps1
```

The launcher auto-discovers one exact Steam installation. If discovery is
ambiguous, supply both `-InstallRoot '<install>\bpsr'` and
`-SteamManifest '<steamapps>\appmanifest_3681810.acf'`. Optional
`-DurationMs` and `-IntervalMs` parameters control the bounded polling window.
Invoke a normal ground-targeted skill manually during that window.

After a successful read-only lifecycle receipt, run the bounded canary only
while safely stationary in a dungeon as leader. Open Team, open the marker
palette, manually select Marker 1, keep the game focused, and run:

```powershell
.\run-bpsr-automarker-lifecycle-probe.ps1 -ArmReversibleCalibration
```

After launching it, use the visible five-second countdown to return focus to
the game. Do not click during the approximately three-second calibration. A
passing receipt requires both mouse axes to excite the indicator, the symmetric
sequence to approximately return, and the indicator to become inactive after
Escape. The old `-ArmReversibleNudge` switch is intentionally rejected rather
than treated as an alias. A failed-closed receipt is not placement authority.

Schema v5 adds a separate one-step planner canary. Keep rLogs running so its
read-only Mechanics Map API exposes a current, non-stale packet-observed local
player position and Automarkers scene-family context. Supply an intended saved
world coordinate explicitly:

```powershell
.\run-bpsr-automarker-lifecycle-probe.ps1 `
  -ArmSinglePlannerStep `
  -TargetX <saved-x> -TargetY <saved-y> -TargetZ <saved-z> `
  -RLogsBaseUrl 'http://127.0.0.1:54221'
```

No target coordinate is built into the executable. The canary requires
matching build, session, scene, map, and
activity family plus a finite target within 18 metres of the live player. It
also requires the Mechanics Map revision and observation clock to advance,
rather than accepting a merely cached `stale=false` entity. It
repeats the calibration, asks the pure planner for exactly one integer move
bounded to four pixels, requires strict distance reduction and an
actual/predicted improvement ratio of at least `0.20`, then applies the exact
inverse and requires return within `0.002 m`. It never falls back to a supplied
player origin or guessed memory offset.

After a planner movement is emitted, rollback no longer depends on subsequent
map-API availability or measurement success. If foreground ownership and the
same read-only Marker 1 roots/context can be re-established, the exact inverse
is attempted. Otherwise the canary emits Escape only while the game remains
foreground and records `rollback_not_safe`; it never substitutes a click or
placement action.

Schema v6 also adds the separately armed v10 closed-loop aiming canary:

```powershell
.\run-bpsr-automarker-lifecycle-probe.ps1 `
  -ArmClosedLoopAim `
  -TargetX <saved-x> -TargetY <saved-y> -TargetZ <saved-z> `
  -RLogsBaseUrl 'http://127.0.0.1:54221'
```

Marker 1 selection remains manual. Do not touch the mouse after the countdown.
The canary installs a bounded low-level mouse observer and accepts ownership
only when every emitted relative move produces exactly one matching injected
move tagged by this process and no untagged or foreign movement. If that
observer cannot start, no planner movement is emitted.

After fresh calibration it retains at most four planner moves, each at most
four pixels and no more than sixteen cumulative pixels. Every step requires a
fresh advancing Mechanics Map observation and unchanged build, session, local
actor, scene, map, dungeon family, roots, and Marker 1 state. Arrival means at
most `0.075 m` from the supplied target. Whether it arrives or fails, all
emitted moves are inverted in exact reverse order using only foreground and
read-only root/Marker 1 safety gates. Map API failure cannot suppress rollback.
The canary then requires return within `0.01 m` and cancels with Escape. It has
no mouse-button input and cannot place a marker.

The v11 launcher can resolve the same closed-loop target from an existing local
rLogs preset, avoiding coordinate copy/paste without changing the native v10
canary or adding placement authority:

```powershell
.\run-bpsr-automarker-lifecycle-probe.ps1 `
  -ArmClosedLoopAim `
  -PresetId 'preset-000000000001-0000'
```

The Automarkers UI displays preset names and marker counts, but not the backing
preset ID. The v13 launcher therefore also supports an exact preset name:

```powershell
.\run-bpsr-automarker-lifecycle-probe.ps1 `
  -ArmClosedLoopAim `
  -PresetName 'Boss opening'
```

Names are ordinal and case-sensitive and must resolve exactly once inside the
active family. Zero matches or duplicate exact names fail closed. Deterministic
automation can continue to use `-PresetId`. To discover IDs without exposing
coordinates or capture identity, run:

```powershell
.\run-bpsr-automarker-lifecycle-probe.ps1 -ListPresets
```

The sanitized table contains only preset name, ID, active family, and sorted
marker numbers. It contains no XYZ, session, deployment, or protocol digest.

`-PresetId`, `-PresetName`, and explicit `-TargetX/-TargetY/-TargetZ` are
mutually exclusive.
Preset mode fetches only `/api/automarkers/presets` over literal
`http://127.0.0.1:<port>`, rejects redirects and proxies, requires schema v4,
exact build `25247556`, a complete active scene/map/family context, a matching
preset family, and exactly one finite Marker 1 point. Only that point's XYZ is
passed to the existing closed-loop token. Marker selection remains manual and
there is still no click or placement action.

When `-RLogsBaseUrl` is omitted, the launcher examines only TCP listeners bound
to `127.0.0.1` and owned by one of the two audited desktop executable names:
the Cargo/development name `rlogs-app.exe` or the installed Tauri product name
`rLogs.exe`. It proceeds only if exactly one of those listeners returns the
expected preset schema; otherwise pass the loopback URL explicitly. A failed
discovery reports sanitized listener, recognized-owner, endpoint-rejection, and
unresolved-owner counts without process IDs or paths; endpoint rejections retain
a safe category such as `http-runtime-unavailable`, `connect-or-timeout`, or
`schema-rejected`. The launcher explicitly loads `System.Net.Http` before using
its client types so this route also works in Windows PowerShell 5.1; failure to
load that framework assembly is reported directly. It never scans or connects
to a non-loopback address. Run
`-SelfTest` for the no-process launcher checks, or `-DryRun` for the packaged
no-process/no-input receipt.

Preset and explicit XYZ targets share one Windows PowerShell 5.1-safe argument
formatter. It converts the supplied value directly to a finite `Double` and
uses invariant round-trip text; it does not dereference `Nullable[Double].Value`
because Windows PowerShell 5.1 unwraps populated nullable values.

## Operator-confirmed Marker 1 placement evidence

Receipt schema v7 extends v6 with the optional sanitized operator-placement
evidence block; read-only and earlier armed modes retain their prior meaning.

The next separately armed evidence mode retains manual placement authority:

```powershell
.\run-bpsr-automarker-lifecycle-probe.ps1 `
  -ArmOperatorPlacement `
  -PresetName 'Boss opening'
```

`-PresetId` is also accepted; raw XYZ is deliberately rejected for this mode.
Start the command first. After the preset resolves, use the dedicated ten-second
preparation countdown to return to the game, open the marker menu, select Marker
1, and leave its reticle active. Do not move the mouse after selecting it. The
canary performs the same bounded calibration and closed-loop aim, then waits up
to eight seconds for exactly one physical left click while the game is
foreground. Click only after the Marker 1 reticle visibly stops moving. It never
synthesizes a mouse button event. Other armed modes retain their five-second
focus countdown.

Success requires all of the following on the exact active build and capture
identity: one human click and no injected/other click or mouse movement; a
strictly newer verified outbound Marker 1 request; then a strictly newer
authoritative inbound Marker 1 whose capture-clock timestamp follows that
request and whose position is within `0.075 m` of the preset target. Session,
deployment, protocol-pack digest, scene, map, activity family, local actor,
foreground, read-only roots, and Marker 1 lifecycle gates remain fail-closed.

Before the human click, any failure reverses emitted aim deltas in exact reverse
order when the existing safety gates permit, then cancels with Escape. After a
human click, aim deltas are never replayed because the marker UI may already be
closed and those movements could affect the gameplay camera; every post-click
failure emits Escape only if the game is still foreground. The receipt records
the human placement attempt separately from `programmaticClickEmitted: false`
and contains no PID, pointers, account, session, deployment, or digest values.

The output is a sanitized JSON receipt containing exact artifact hashes,
sampling counts, policy assertions, and deduplicated lifecycle transitions.
It contains no PID, raw pointer/module address, or filesystem path. Its
timestamped filename is created beside the package and never overwritten.

The v15 launcher validates the newly written schema-v7 receipt after the
native process returns. It requires the exact producer, game/build/app,
requested mode, duration, interval, identity, policy, summary, and canary
envelopes and rejects stale, malformed, missing, or unexpected critical
fields. Every armed invocation now succeeds only when the sanitized top-level
`canary.outcome` is exactly `passed` **and** its mode-specific nested proof is
internally consistent. Calibration requires the four transitions plus Escape,
return, and cancellation evidence; a planner step requires its strict
improvement and inverse-return proof; closed loop requires bounded arrival and
complete safe rollback; operator placement requires a single observed human
click, no programmatic/injected/other click, a newer outbound followed by a
newer inbound observation within `0.075 m`, continuous context, and no timeout.
Any other outcome returns nonzero while retaining the receipt for diagnosis.
The launcher prints only that bounded sanitized outcome. Ordinary read-only
observation keeps its prior success behavior after the same receipt-integrity
checks.

Failed planner receipts retain a bounded static `live_context_failure_reason`
when the read-only mechanics-map origin gate fails (for example,
`mechanics-map-not-fresh` or `missing-local-actor`). The launcher validates this
as a lowercase reason token; the field cannot contain a URL, path, response
body, process identity, or captured identity value. The mechanics-map feed also
retains its existing bounded publication behavior; unrelated high-rate combat
events are not promoted into map rerenders merely to advance a clock. A stale
local entity remains a hard failure for the 18 m player-origin requirement.

## Read-only native-dispatch preflight

Receipt schema v8 added a separate, non-activating preflight for the exact-build
game-owned dispatch candidate. It does not use the manual reticle planner and
does not apply the planner's 18-metre range gate:

```powershell
.\run-bpsr-automarker-lifecycle-probe.ps1 `
  -NativeDispatchPreflight `
  -PresetName 'Boss opening'
```

`-PresetId` is also accepted. The launcher resolves exactly one saved preset
inside the active scene family and passes only its opaque ID and the literal
loopback rLogs URL to the native observer. The observer independently fetches
the schema-v4 preset projection and requires exact build `25247556`, the active
scene and map, the same activity family, and one finite non-zero Marker 1 point.
It never accepts explicit XYZ for this mode. Schema v9 extends that exact
envelope with the scheduler code and live scheduler-state gate below; the
launcher rejects older v8 receipts rather than interpreting a missing gate.

The observer verifies the running executable, Steam manifest, and
`GameAssembly.dll`; double-reads and class-validates the reviewed
`PlayerEnt -> PlayerSkillInputComp -> ZSkillInputMgr` chain; requires an idle
indicator lifecycle; and hashes the loaded exact method bodies for
`EntityAttrExtensions.SetIndicatorPos` and
`ZSkillInputMgr.FirePlaySkillByIndicator`. It separately hashes the four exact
scheduler regions for `UniTask.Post`, `PlayerLoopHelper.AddContinuation`,
`ContinuationQueue.Enqueue`, and `ContinuationQueue.RunCore`; regions larger
than 512 bytes are read in fixed 512-byte-or-smaller chunks. The receipt contains only reviewed
RVAs, hashes, booleans, bounded reason tokens, preset ID, scene/map/family, and
the saved Marker 1 coordinate. It contains no runtime address, process ID,
account/session identity, endpoint, or filesystem path.

The exact-build dungeon-stage query is now resolved without calling game code.
The observer class-validates the independent `StageMgr` singleton and its
current `StageDungeon`, requires `ESwitchState.ENone` and
`EStageType.Dungeon`, then repeats the full chain and requires identical roots
and values. The bounded proof is retained in
`automarker-read-only-dungeon-stage-proof.v1.json`. Other dungeon-like stage
classes remain rejected until their normal marker eligibility is separately
proven.

The marker-skill query is likewise resolved without invoking a lookup method.
The observer follows the already class-validated current
`PlayerSkillInputComp`, class-validates `dataMgr_` at `+0x20` as
`SkillControlDataMgr`, then reproduces the first-match enumeration of
`skillContinuousDict_` at `+0x40` before reading `skillControlDatas_` at
`+0x18`. If the first active continuous entry whose `BeginContinuousSkillId`
equals 1101 exists, its dictionary key becomes the effective lookup ID;
otherwise the effective ID remains 1101. A native-valid empty continuous
dictionary, including null storage with zero count/free-count, is accepted as
the no-remap case. Exact-build
disassembly proves that `ZSkillInputMgr.FirePlaySkillByIndicator` receives the
skill ID directly and calls `SkillControlDataMgr.TryGetSkillControlData`; this
route does not consult `skillSlotDict_` at `+0x28`. That slot dictionary belongs
to the separate `TryGetSkillDataBySlotId` UI lookup and is intentionally not an
activation gate for direct dispatch. The read-only gate requires the exact
reviewed generic dictionary TypeInfo, the exact `int[]` bucket TypeInfo, a
rank-one 24-byte entry array, bounded coherent counts, a null/default integer
comparer, and native-equivalent bucket-chain reachability for the effective
skill ID. Bucket and collision indexes are bounded and cycles are rejected. It
then requires the resolved object to have the exact reviewed `SkillControlData`
TypeInfo and a `skillId_` equal to the effective lookup ID. The entire query is
repeated across the stability interval and must retain identical manager,
dictionary objects, arrays, capacities, counts, versions, remap result,
validated structure fingerprints, and resolved-object identity.
Each individual dictionary scan also bookends its mutable header/version and
array lengths, so a torn scan is rejected before the outer stability read.
The sanitized receipt exposes only the bounded gate result documented in
`automarker-read-only-marker-skill-proof.v1.json`; it never exposes addresses
or dictionary contents.

When that gate fails, the receipt identifies only the fixed chain stage: root
chain, `SkillControlDataMgr`, continuous dictionary/remap state, control-data dictionary shape/type/storage,
Marker 1 control-data lookup, exact class identity, or skill identity. A
distinct fixed token reports when the two bounded reads fail at different
stages. These diagnostics contain no pointers, container counts, dictionary
keys or values, character identity, process identity, or other live runtime
values; they still grant no placement or invocation authority. Receipt schema
11 records this method-equivalent direct-route contract.

The party-leader query is also resolved read-only. Exact-build disassembly of
`PlayerTeamLeaderCondition.Check` proves that leadership is the current
`PlayerEnt.CharId` at `+0xD8` matching the first member of local attribute
`151` (`ETeammateList`). The observer follows that same entity's
`ZAttrCollection` at `+0x48`, reproduces the bounded native
`ZAttrCacheSlim.TryGet` lookup for local key `0x80000097`, validates the exact
generic attribute/list and array TypeInfo identities, and requires a positive
current and first-member CharId match. It repeats the complete chain and
rejects absent parties, nonleaders, duplicate keys, invalid types, torn cache
state, or any changed root/container/value. The receipt exposes only a bounded
gate result; it never exposes either CharId. The retained static proof is
`automarker-read-only-party-leader-proof.v1.json`.

The main-thread scheduler query is a separate read-only gate. It validates the
exact `PlayerLoopHelper` TypeInfo and static-fields pointer, requires a positive
recorded `mainThreadId` and nonnull Unity synchronization context, validates a
bounded one-dimensional reference `yielders` array containing index 8, and
class-validates that entry as a `ContinuationQueue` whose timing is `Update`
(8). Both `System.Action[]` queue buffers must have the exact reviewed TypeInfo;
their lengths are capped at 16,384 and each nonnegative count must fit its
buffer. The observer repeats the complete sample across the stability interval
and accepts it only when every pointer, scalar, buffer length, and count is
identical. The sanitized receipt exposes only the four reviewed code hashes and
one bounded `main_thread_scheduler_gate`; no thread ID, object address, queue
count, or queued callback is retained.

This preflight deliberately remains blocked even when every currently
resolvable gate passes because no sanctioned in-process entry or ABI-valid
managed callback exists for the selected one-shot Unity main-thread queue. The
separate `main_thread_bridge_gate` therefore remains false even when the new
scheduler-state gate passes. The marker-skill, party-leader, and scheduler gates are now live read-only results
rather than unresolved placeholders. The mode never calls either native method, creates or
schedules a delegate, emits input, requests
write/debug/thread rights, modifies memory, or observes/sends packets. It is
evidence for the next implementation boundary, not placement authority.

The statically reviewed direct sequence avoids one specific overwrite in the
normal UI route: `ZIndicatorMgr.FireSkill` copies its reticle position before
normal dispatch, whereas direct `FirePlaySkillByIndicator` does not call
`ZIndicatorMgr.FireSkill`, `SetIndicatorPos`, `ResetIndicatorPos`, or
`SetSelectPoint`. That bypasses reticle selection as the coordinate source; it
does not prove that downstream client or server validation will accept a saved
point farther than 18 metres. A later activating canary must require both a
true game-owned dispatch result and a matching authoritative inbound marker
notification before distant placement can be claimed.

## Interpretation limits

A successful receipt proves only that a stable, class-validated read chain was
observed while the user exercised the normal UI. Polling is non-atomic and can
miss short-lived transitions. The canary does not prove general target
convergence, placement, server acceptance, or the contents of ephemeral
`UseSkillParam`; it deliberately does not test a confirmation click. Any
programmatic placement remains outside this tool and outside the enabled
product surface.
