# Plugin API

This crate defines the public, versioned contract shared by bundled and
community plug-ins.

A plug-in is a directory package. Its small `plugin.toml` requests
capabilities, declares its entrypoint, dependencies, shared resource
imports/exports, and ordered hooks. Code and small declarative data remain in
that folder. A resource export may instead select a host-controlled external
asset namespace:

```text
assets/<plugin-folder>/...
assets/shared/<provider-plugin-folder>/...
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

Chat is also separate. Public/system/party chat can only be delivered locally
to a plugin granted `local_chat_read`; it is not part of ordinary event access
or leaderboard submissions. Direct/private-message routes remain prohibited.

The initial runtime implementation will use these contracts for WebAssembly,
browser overlays, and authenticated external-process IPC.
