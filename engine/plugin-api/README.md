# Plugin API

This crate defines the public, versioned contract shared by bundled and
community plug-ins.

A plug-in is a directory package. Its small `plugin.toml` requests
capabilities, declares its entrypoint, dependencies, shared resource
imports/exports, and ordered hooks. Code and small declarative data remain in
that folder. A resource export may instead select a host-controlled external
asset namespace:

```text
assets/rlogs/plugins/<plugin-folder>/...
assets/rlogs/shared/<provider-plugin-folder>/...
```

The host derives the folder name. A manifest selects `plugin_assets` or
`shared_assets` and a safe relative path; it cannot claim another provider's
namespace.
The host decides which capabilities are granted. Normal plug-ins subscribe to
canonical event topics and cannot access raw protocol
evidence. Raw protocol research and unrestricted native execution are separate
developer-mode capabilities.

Data-only plug-ins can publish tables and register host-provided transforms
without executable code. Shared resources retain one owner and are imported by
owner ID, name, schema ID, and minimum version instead of being copied.
Operation hooks declare a stage, phase, priority, and optional before/after
relationships.

Interactive plug-ins can request `ui_workspace_publish` and declare one
top-level desktop workspace. The workspace becomes one draggable item in the
left navigation. Its packaged browser surfaces become real tabs on the right:

```toml
capabilities = ["events_read", "scoped_storage", "ui_workspace_publish"]

[workspace]
icon = "ui/profile.svg"
default_order = 20

[[workspace.tabs]]
id = "profile"
label = "Profile"
entrypoint = "ui/profile.html"

[[workspace.tabs]]
id = "sync"
label = "Sync"
entrypoint = "ui/sync.html"

[[workspace.tabs]]
id = "options"
label = "Options"
entrypoint = "ui/options.html"
kind = "options"
```

The host validates every icon and tab entrypoint as a package-relative path.
It owns workspace selection, user drag order, tab selection, containment, and
permissions. The plug-in owns the tab labels and surfaces. A single-tab
workspace does not need to show a redundant tab bar. A plug-in may declare at
most one `options` tab.

An add-on can contribute a tab to another plug-in's workspace without copying
or changing the target:

```toml
capabilities = ["ui_workspace_publish"]

[[dependencies]]
plugin_id = "app.rlogs.character-profiles"
optional = true

[[workspace_tab_contributions]]
target_plugin_id = "app.rlogs.character-profiles"
id = "modules"
label = "Modules"
entrypoint = "ui/profile-modules.html"
default_order = 200
```

The target must be an explicit dependency. The host resolves the surface from
the contributing package, namespaces the tab ID by contributor, and shows it
only while both plug-ins are enabled and compatible. Disabling the add-on
removes its tab without modifying or leaving data in the target package.
Options-kind tabs remain grouped at the end of the target's tab list.

Chat is also separate. Public/system/party chat can only be delivered locally
to a plugin granted `local_chat_read`; it is not part of ordinary event access
or leaderboard submissions. Direct/private-message routes remain prohibited.

The initial runtime implementation will use these contracts for WebAssembly,
browser overlays, and authenticated external-process IPC.
