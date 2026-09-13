# resonance-logs-cn waymarker audit

Status: static source audit

Audited upstream: `tmp-rdps-audit/upstream-resonance-logs-cn`

Upstream commit: `bd71d2dfd3c7289e6398c4cd042f4357d4f35721`

Audit date: 2026-09-13

## Conclusion

The audited resonance-logs-cn source passively observes party waymarkers from captured game traffic and renders them on its minimap. It does **not** provide evidence of named marker presets, automatic party-visible placement, packet injection, game-memory writes, UI input automation, or in-process game calls.

Map visualization and party-visible placement are separate capabilities. The former is implemented; the latter was not found.

## Passive observation path

- `src-tauri/src/packets/packet_capture.rs:56-60` opens WinDivert with a TCP capture filter and the sniff flag. `:72-79` receives captured packets. This is an observation path, not a transmit path.
- `src-tauri/src/live/protocol/decoder.rs:116-129` accepts world notifications only in the server-to-client direction and world calls only in the client-to-server direction.
- Server snapshots and deltas feed passive-skill starts and ends at `decoder.rs:348-350` and `decoder.rs:426-430`.
- `decoder.rs:671-700` recognizes marker skill IDs `1101..1106`, retains the passive instance ID, and reads the target position. `decoder.rs:703-727` decodes end notifications, which contain the instance ID rather than the complete start descriptor.
- The generated schema records the underlying fields in `src-tauri/src/blueprotobuf-lib/src/blueprotobuf_package.rs:251-284`: actor, passive instance, skill ID, target position, and end-instance IDs.
- `src-tauri/src/live/runtime/entity_context.rs:1458-1508` retains start state so a later end event can recover the marker identity and position.

## Minimap projection path

- `src-tauri/src/live/projections/minimap/mod.rs:246-269` adds and removes active markers by passive instance ID.
- `minimap/mod.rs:271-278` clears marker state when the scene changes.
- `minimap/mod.rs:414-419` rejects IDs outside marker numbers 1 through 6.
- `src-tauri/src/live/ipc/models.rs:680-704` defines markers as minimap-rendering data containing marker number, skill ID, and optional X/Z coordinates.
- `src/routes/minimap-overlay/minimap-canvas.svelte:485-525` draws the observed markers as colored digits at their projected positions.
- `src/lib/i18n/messages/en-US.ts:1822-1824` explicitly describes the feature as drawing markers on the map only.
- Scene adapters translate world coordinates into arena-local coordinates. For example, `src/routes/minimap-overlay/scenes/s3-tina-mindrealm/index.ts:38-50` applies coordinate translation without distance-filtering the marker.

This pipeline can display a marker that the server has already reported. It cannot cause a marker to exist in the game or make one visible to the party.

## Outbound schema clue, not placement proof

`src-tauri/src/live/protocol/decoder.rs:1037-1064` passively decodes the client-to-server `USE_SLOT` world call (`0x3d002`) into a local skill-request observation. The generated `UseSkillParam` schema at `src-tauri/src/blueprotobuf-lib/src/blueprotobuf_package.rs:5534-5556` includes target and current position fields, nested through `UseSlotRequest` at `:5559-5565`.

This is useful when classifying controlled captures. It does not establish that waymarker placement uses this route, and the audited project does not encode or transmit such a request.

## Reusable architecture

The following observation-side design is suitable to carry into rLogs:

- Strictly map skill IDs `1101..1106` to marker numbers `1..6`.
- Key active marker state by passive instance ID, not player UUID or local character position.
- Preserve the start descriptor so instance-only end packets can remove the correct marker.
- Clear observed marker state on an authoritative scene transition.
- Keep world-to-scene coordinate conversion in scene adapters.
- Do not distance-filter markers; a marker may be observed far from the local character.
- Treat authoritative server-to-client marker observations as confirmation after any future placement attempt.
- Keep marker observation independent from DPS, encounter clocks, and run synchronization.

## Negative findings and trust boundary

Targeted searches across `src` and `src-tauri/src` found no automarker or waymarker placement API, named marker-preset storage, packet-send API, `ReadProcessMemory`, `WriteProcessMemory`, `CreateRemoteThread`, `SendInput`, `mouse_event`, or `keybd_event` use.

The only matching `OpenProcess` call is in `src-tauri/src/packets/game_connections.rs:185,303`, using query-only access to identify the executable associated with TCP endpoints (`:195-240`, `:302-315`). It is not game-state memory access.

The retained upstream tree contains no application executable to contradict the source audit; its only matching native binary is the capture dependency `src-tauri/WinDivert.dll`.

Database and timeline objects also named “markers” are unrelated. `src-tauri/src/live/marker_skills.rs:1-6` identifies those as key-skill annotations on the DPS timeline, not world waymarkers.

This research note does **not** authorize packet injection, process-memory writes, or other active manipulation. Any placement transport must remain disabled until its exact game-owned request route, framing and sequencing requirements, server acceptance behavior, and safe validation boundary are independently demonstrated.
