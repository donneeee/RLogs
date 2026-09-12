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

The output is a sanitized JSON receipt containing exact artifact hashes,
sampling counts, policy assertions, and deduplicated lifecycle transitions.
It contains no PID, raw pointer/module address, or filesystem path. Its
timestamped filename is created beside the package and never overwritten.

## Interpretation limits

A successful receipt proves only that a stable, class-validated read chain was
observed while the user exercised the normal UI. Polling is non-atomic and can
miss short-lived transitions. The canary does not prove general target
convergence, placement, server acceptance, or the contents of ephemeral
`UseSkillParam`; it deliberately does not test a confirmation click. Any
programmatic placement remains outside this tool and outside the enabled
product surface.
