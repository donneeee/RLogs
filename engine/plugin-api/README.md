# Plugin API

This crate defines the public, versioned contract shared by bundled and
community plugins.

A manifest requests capabilities; the host decides which are granted. Normal
plugins subscribe to canonical event topics and cannot access raw protocol
evidence. Raw protocol research and unrestricted native execution are separate
developer-mode capabilities.

Chat is also separate. Public/system/party chat can only be delivered locally
to a plugin granted `local_chat_read`; it is not part of ordinary event access
or leaderboard submissions. Direct/private-message routes remain prohibited.

The initial runtime implementation will use these contracts for WebAssembly,
browser overlays, and authenticated external-process IPC.
