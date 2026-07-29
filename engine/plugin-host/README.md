# rLogs plug-in host

This crate discovers self-contained packages under the configured installed
plug-ins folder. It validates `plugin.toml`, keeps every entrypoint inside its
package root, and confines external resources to host-derived
`assets/<plugin-folder>/` or
`assets/shared/<provider-plugin-folder>/` namespaces. It resolves required
plug-in dependencies, publishes named read-only resources without copying
them, and produces deterministic before/after operation plans.

The host contract is independent from any desktop UI. A desktop build may scan
an application-data `plugins/` folder while repository development scans
`plugins/installed/`.
