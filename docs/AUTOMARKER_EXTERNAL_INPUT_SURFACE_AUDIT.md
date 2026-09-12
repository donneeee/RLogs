# Automarker external input surface audit

This read-only audit answers a narrow question for global Steam build `25247556`: does the production client already expose a normal keyboard, mouse, controller, chat/console, accessibility, deep-link, or launcher action that can directly select waymarker 1–6 and place it without moving the targeting reticle or traversing the marker UI?

The answer is **no route was found**. No input was generated, no process was accessed, and no runtime behavior was changed.

## Exact-build result

The `Panda.ZInput.InputActionIds` registry contains 171 named action constants spanning IDs 1–187. It includes generic mouse axes/buttons, Confirm, Cancel, Chat, and OpenChat. It contains no marker-, waymark-, flag-, indicator-, punctuate-, or scene-mask-named action and no direct numbered marker action.

The exact current-build Lua census instead finds the ordinary marker flow in `main_copy_punctuate_view.lua`:

1. open/use the dungeon marker view;
2. choose one of the six entries sourced by `GetSceneMaskSkillList`;
3. call `PlayerInputController.FlagSkill(info.id, true)`;
4. drive `Skill_Horizontal` and `Skill_Vertical` through the touch/pointer controller;
5. release with `FlagSkill(info.id, false)`.

Slots 201–206 correlate with skills 1101–1106, but neither `FlagSkill` call accepts a world position. Keyboard, mouse, and controller can operate this UI, yet that is menu/pointer/reticle traversal rather than a direct action. It cannot restore arbitrary saved XYZ without also solving the camera/reticle projection problem.

## Other surfaces

- Gamepad support is implemented through UI-pointer adapters. No numbered marker controller action exists in the exact action registry.
- No marker-specific accessibility binding appears in the registry or marker-view path.
- Chat/OpenChat actions only enter or navigate chat. No marker chat command was found.
- The bundled generic debug-console framework has exactly two `ConsoleMethod` registrations in the dump: `scene.loadasync` and `common.framerate`. Neither is a marker command, and framework presence does not prove the production console UI is enabled.
- Generic command-line parameters and token deep links exist, but no marker consumer, command-line switch, URI, or public launcher contract was found.
- The official Steam product page exposes normal game launch and product features, not a marker automation hook: <https://store.steampowered.com/app/3681810/Blue_Protocol_Star_Resonance/>. This is corroborating evidence only; a store page is not a complete controls manual.

## Consequence

Ordinary foreground input is useful only for reproducing the same manual UI workflow. It does not provide the requested seamless saved-position route. A direct external-input implementation should not be added on this evidence.

The machine-readable evidence is `plugins/games/blue-protocol-star-resonance/research/game-file-inventory/global/steam-25247556/ground-marker-external-input-surface-audit.v1.json`.
