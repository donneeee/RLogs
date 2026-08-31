# Built-in desktop plug-ins

Each child folder is a complete manifest package. Workspace and Settings
placement comes from `plugin.toml`; rLogs Core does not keep a feature list.

The small HTML files are validated package entrypoints. During the current
native-shell phase, trusted built-in surfaces are mounted by the desktop host.
They can move to the sandboxed browser/component runtime without changing their
manifest-owned navigation contract.
