# Plugins

rLogs has two explicit extension trust levels:

- `games/` contains trusted native game integrations. They receive
  reconstructed transport streams and own game framing, packet decoding,
  mappings, profiles, and website payload projection.
- `builtin/` and future community add-ons use the ordinary public API. They see
  privacy-reviewed canonical events and never receive raw packets or
  undocumented decoder access.

Adding another game means adding another folder under `games/`; it must not add
game names, opcodes, or profile fields to Core.

## Where users install plug-ins

Each community plug-in is one directory package placed under
[`installed/`](installed/PUT_PLUGINS_HERE.md):

```text
plugins/installed/<plugin-name>/
  plugin.toml
  bin/ or web/
  resources/

assets/rlogs/plugins/<plugin-name>/...
assets/rlogs/shared/<plugin-name>/...
```

The small TOML file declares identity, compatibility, permissions, imports,
exports, resource storage, and operation order. Functionality and small
declarative data belong to the surrounding folder; large private assets may
live in the first external namespace. Assets intentionally reused by other
plug-ins live in the provider-owned shared namespace. rLogs does not treat a
loose JSON document as a plug-in. Files, archives, or binaries placed directly
in `installed/` are ignored.

Named shared resources retain one provider whether their files live in the
package or in `assets/rlogs/shared/<provider>/`. Game integrations use
`assets/<game-id>/shared/`. A plug-in can import the BPSR
catalog, UID mappings, localization, or icons without shipping another copy.
Imports declare schema compatibility, and the host exposes read-only access
inside the exported path.

Operation hooks are explicit and deterministic. A plug-in chooses a stage,
`before_core` or `after_core`, a priority, and optional plug-in IDs that it must
run before or after. Cycles disable the conflicting plan. The
[`bpsr-uid-aliases`](examples/bpsr-uid-aliases/) example runs after normal
localization, changing only the presented label while retaining canonical IDs.

Localization add-ons are a data-only extension type. They carry no code or
capabilities and may contain official game text plus rLogs UI text for exactly
one locale. The built-in `en-US` package defines the default layout under
`plugins/builtin/localization/`. Game text is namespaced by game plug-in ID so
one locale add-on can support multiple installed games without collisions.

The first executable host path is deliberately narrower than the final public
model: it runs only native plug-ins that are linked into RLogs itself. Community
packages are discovered and validated but are not executed yet. The future
sandboxed component adapter will enforce the same public capabilities,
subscriptions, limits, and versioned output schemas.
