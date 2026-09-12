# Automarker internal game-owned route audit

This is a read-only static design audit for the exact global Steam build `25247556`. It does not implement or authorize process access, memory writes, method calls, input injection, or packet synthesis. The machine-readable proof is
`plugins/games/blue-protocol-star-resonance/research/game-file-inventory/global/steam-25247556/ground-marker-internal-game-owned-route-design-proof.v1.json`.

## Result

The exact build contains a coherent game-owned route:

1. `EntityAttrExtensions.SetIndicatorPos(livePlayerEntity, targetWorldPosition)`
2. `ZSkillInputMgr.FirePlaySkillByIndicator(markerSkillId)`
3. the ordinary player/character skill path
4. `World.UseSlot`, with fresh action, session, authentication, and transport state created by the game

That is a static call-graph result, not a safe invocation contract. Native placement must remain disabled until the unresolved scheduler, object-lifetime, gating, and acknowledgement requirements are proven at runtime.

## Exact ABI evidence

`SetIndicatorPos` is at RVA `0x53E86A0`. Its generated signature is:

```c
void Panda_ZGame_EntityAttrExtensions__SetIndicatorPos(
    Panda_ZGame_ZEntity_o* entity,
    UnityEngine_Vector3_o position,
    const MethodInfo* method);
```

On Windows x64 the entry moves `RCX` (the entity) to a preserved register and treats `RDX` as an address. The body reads eight bytes at `[RDX]` and four at `[RDX+8]`. The exact generated `Vector3` layout is three consecutive `float32` fields (`x`, `y`, `z`), totaling 12 bytes. The hidden `MethodInfo*` remains part of the generated contract in `R8`; absence of an obvious read in this body is not permission to omit it.

`FirePlaySkillByIndicator` is at RVA `0x52E09E0`. Its generated signature is:

```c
bool Panda_ZGame_ZSkillInputMgr__FirePlaySkillByIndicator(
    Panda_ZGame_ZSkillInputMgr_o* self,
    int32_t skillID,
    const MethodInfo* method);
```

The entry preserves `RCX` as `self` and `EDX` as the skill id. It calls `TryGetSkillControlData` (`0x52BE550`); lookup failure returns false. Success calls `FirePlaySkillEvent` (`0x52E0AB0`) at callsite `0x52E0A83` with `clearSlot=true`, `isIndicator=true`, and `isAIPress=false`, then propagates the boolean result.

## Required live objects

The manager cannot be fabricated or cached across lifecycle changes. The exact current chain is `ZEntityMgr -> playerEnt_ (+0x18) -> skillInputComp_ (+0x128) -> skillInputMgr_ (+0x28)`. The manager's `comp_` field at `+0x10` must point back to the same current `PlayerSkillInputComp`. The entity, component, manager, scene, and root generation must be class-valid and identical immediately before and after the operation.

The indicator component is reached through the player's PureComponents storage and `_value105` at `+0x9C4`. That layout is useful as validation evidence only. Directly writing the three floats would neither dispatch a skill nor produce server authority or a party-visible marker, and could be overwritten by normal indicator code.

## Main-thread requirement

`UniTask.Post(Action, PlayerLoopTiming)` at RVA `0x670EB30`, with `Update = 8`, is the least-complex named one-shot scheduler candidate in this build. It is not proven safe. A future implementation would still need a supported in-process entry, an IL2CPP-attached caller, a correctly constructed and GC-rooted `System.Action`, an exact trampoline, class/MethodInfo initialization, exception containment, cancellation, and exactly-once execution. All lifecycle and authority gates must run again inside the callback.

`UpdateManager.AddUpdate` (`0x4097270`) is not simpler: it additionally needs a live `MonoBehaviour`, a correctly typed float callback delegate, registration ownership, stable rooting, and guaranteed removal.

## Gates and acknowledgement

The normal game UI exposes scene-mask skills only in a dungeon and only when the current character matches `TeamInfo.baseInfo.leaderId`. An internal route bypasses that UI, so both before queueing and inside the callback it must fail closed unless the exact build, dungeon scene/family/map, party leadership, marker slot-to-skill mapping, live object generation, input state, and target rules all still match.

Only one marker may be in flight. In the six-marker normal-UI trace, every request had a transport acknowledgement, an empty `World.UseSlot` return, and a matching self inbound marker-add notification. Request-to-return times ranged from 38.297 ms to 91.077 ms. This single trace does not establish a rate limit. A future operation should advance only after both the game dispatch reports success and a matching self marker-add is observed for the same scene/session, skill, and XYZ. A transport acknowledgement or empty return alone is insufficient.

## Why packets are not a shortcut

The normal request contains dynamic slot, skill UUID, begin time, session sequence, authenticated/encrypted attribute data, call id, frame sequence, position, and transport ordering state. Captured constants are stale and bypass current client eligibility and lifecycle logic. The game-owned route is valuable precisely because it lets the current client construct those fields through its ordinary path.

## Unresolved before any canary

- supported in-process entry and IL2CPP thread attachment
- safe delegate construction, rooting, cancellation, and exception handling
- atomicity against ordinary input and `ZIndicatorMgr` position updates
- authoritative queries for leader, scene transition, input blocking, cooldown, replacement, and clear state
- runtime meaning of the boolean return
- failure, replacement, clear, reconnect, transition, and stale-object captures
- server pacing beyond one successful manual trace

Any seasonal build invalidates the RVAs and layouts until the full proof is regenerated and revalidated.
