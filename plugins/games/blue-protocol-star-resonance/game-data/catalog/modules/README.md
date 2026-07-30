# Modules

Reviewed module definitions are organized by readable module type:

```text
modules/
  attack/
  support/
  guard/
    <config-id>-<localized-slug>.json

module-effects/
  <effect-id>-<localized-slug>.json

module-types/
  <type-id>-<localized-slug>.json

module-slots/
  <slot-id>-slot.json

module-link-effects/
  <link-value>-link-value.json
```

The 12 module records own their config ID, category, effect-library relation,
initialization ID, localized display name, and reviewed item icon when the
current item table supplies one. The 21 effect records own all seven current
threshold rows (`0, 1, 4, 8, 12, 16, 20` link points), exact effect configs,
fight values, official text, and icons. The 121 link-effect records preserve
the exact total-link curve used by the optimizer. Type and slot records keep
their display names and empty-slot/tab visuals.

Player-owned instance IDs, upgrade history, and equipped slots do not belong in
this static catalog; they arrive through the typed character profile.

The exact-build source tables are `ModTable`, `ModTypeTable`, `ModHoleTable`,
`ModInitializationTable`, `ModEffectTable`, `ModEffectLibTable`,
`ModLinkEffectTable`, and `AssessModuleTable`. The exact weights in
`ModInitializationTable` are retained in external evidence, but its three roll
dimensions stay unnamed until current behavior proves them. `AssessModuleTable`
configures the generic assessment screen and is not treated as an optimizer
formula.
