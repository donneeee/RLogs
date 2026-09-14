# Private native Automarkers adapter boundary

Status: design-only and disabled. This boundary performs no process access,
memory access, native invocation, input injection, or packet transmission. It
must not be included in a public release while it depends on unsupported
in-process execution.

## Why an adapter is still required

The exact global Steam build `25247556` contains a game-owned placement route:
set the saved world position with `EntityAttrExtensions.SetIndicatorPos`, then
invoke `ZSkillInputMgr.FirePlaySkillByIndicator` for skills `1101` through
`1106`. The game then constructs fresh action, session, authentication, and
transport state through its normal `World.UseSlot` path. That avoids raw packet
replay and does not depend on the player position, cursor, marker menu, or a
manually placed marker.

The route is not externally callable. The audited client exposes no sanctioned
plug-in or IPC entry, and a raw memory write of XYZ does not dispatch a marker.
Therefore a no-menu implementation needs code executing inside the game process
on its IL2CPP main thread. How that code gets there remains unresolved and is
anti-cheat-sensitive.

## Component split

1. **Desktop coordinator (out of process).** Resolves the current scene/family,
   leader status, exact preset and commitment, and current connection epoch. It
   installs the existing source-unbound interceptor before requesting a carrier.
2. **Private authenticated channel.** Uses the existing per-launch capability,
   direction-separated HMAC keys, session ID, monotonic counters, exact target
   bits, and one-attempt state machine. Peer ownership must bind the adapter to
   the exact selected game PID; loopback reachability alone is insufficient.
3. **Minimal in-process adapter.** Accepts only place/cancel messages for marker
   numbers 1–6. It has no arbitrary address/read/write/call API and no raw socket
   or packet API. It revalidates all gates, schedules exactly once on the game
   main thread, obtains fresh live objects, sets the indicator position, calls
   the reviewed indicator skill method, and emits authenticated receipts.
4. **Existing confirmation path.** The desktop advances to the next marker only
   after the dispatch returned success and the matching transport ACK, RPC
   Return, authoritative self MarkerAdd, and transport-obligation retirement are
   all observed for the same attempt and current scene/connection generation.

No adapter receipt is server confirmation. A timeout, scene transition,
leadership change, process restart, connection epoch change, object-generation
change, cancellation race, or ambiguous acknowledgement terminates the whole
preset load.

## Exact-build and privilege boundary

The only reviewed identity is global Steam build `25247556`:

- process: `BPSR_STEAM.exe`
- executable: 808,496 bytes, SHA-256
  `90537dbd0e4c9d4b3ed2af06aa8bf7ffe7219fc94bb2f6963673f7b340288588`
- `GameAssembly.dll`: 218,074,672 bytes, SHA-256
  `4a079aec0a3e51a8023aa86bbd152e12068907b65b9aafb355d20bb6d6c41fe3`

Build number alone is insufficient. Every identity and loaded-module hash must
match before any adapter bootstrap, and must be rechecked after launch. Epic,
SEA, Asia, Taiwan, future Steam builds, compatibility packs, or renamed
executables are unsupported even if their packet grammar appears compatible.

The adapter and desktop should run at the game's existing integrity level. rLogs
must not request administrator privileges merely to make placement work. An
integrity mismatch or inability to authenticate the game-owned peer is a hard
failure, not a reason to elevate. The presence of ACE components is a risk fact,
not evidence that an operation is allowed or safe. No anti-cheat bypass,
concealment, evasion, or service/driver modification belongs in this design.

## Fail-closed gates

All gates are required again immediately before queueing and inside the
main-thread callback:

- private, unpublished experimental mode and current-launch authorization;
- exact deployment/channel/build/process/executable/GameAssembly identity;
- authenticated peer owned by the current selected game process;
- proven supported in-process entry and attached IL2CPP thread;
- proven exactly-once main-thread scheduling and exception containment;
- current dungeon scene/map/family and matching saved preset family;
- current local player is the party leader;
- live entity/component/skill-manager chain is class-valid, mutually linked,
  generation-stable, and freshly reacquired rather than cached;
- input state, skill availability/cooldown, target bounds, and replacement rules
  are eligible;
- atomic protection against normal `ZIndicatorMgr` position changes;
- no other marker attempt is in flight;
- source-unbound interception is already armed for the bit-exact target; and
- fresh ACK, return, and authoritative MarkerAdd observers are ready.

The pure policy in
`apps/desktop/src/automarker_native_adapter_policy.rs` enumerates these gates.
Passing it authorizes only a future adapter preflight, never placement by itself.
The currently unresolved in-process entry, IL2CPP attachment/scheduler,
eligibility, and atomicity proofs intentionally keep activation disabled.

## Required user authorization

Before implementing or running any private in-process canary, the user must
explicitly authorize all of the following in the current task:

> Proceed with a private, opt-in, global Steam build 25247556 in-process
> Automarkers adapter. I understand that it executes unsupported code inside a
> protected game process and may crash the game or cause account action. Do not
> publish or release it.

That authorization is narrow. It does not authorize an anti-cheat bypass, raw
packet synthesis/replay, arbitrary process-memory writes, general-purpose native
calls, another client/build, public distribution, or release. A separate runtime
confirmation must be bound to a random current-launch ID and the current risk
notice revision; it defaults off and expires on game or rLogs restart. Any build
or risk-notice change invalidates it.

## Remaining proof before implementation

The repository still lacks a supported in-process entry, a safely attached
IL2CPP caller, a valid and rooted one-shot delegate/trampoline, exactly-once
main-thread scheduling, exception containment, and runtime proof for leader,
eligibility, atomic position staging, return semantics, pacing, replacement,
clear, reconnect, and scene-transition behavior. Until those are closed, the
production `UnavailableNativeCarrierTrigger` remains the only valid trigger.
