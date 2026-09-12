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

The armed canary emits one six-pixel relative horizontal mouse move, waits for
the game's normal indicator smoothing, observes the position, emits the inverse
only while the game remains foreground, and finally sends Escape to cancel. It
never emits a mouse button, Enter, Normal Attack, or placement confirmation.
Loss of focus prevents further input. Its receipt contains no PID,
account/session identity, raw address, or filesystem path.

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
.\run-bpsr-automarker-lifecycle-probe.ps1 -ArmReversibleNudge
```

Do not click during the approximately one-second canary. A passing receipt
requires the indicator to move, approximately return, and become inactive after
Escape. A failed-closed receipt is not placement authority.

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
