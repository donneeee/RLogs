# rLogs plug-in host

This crate discovers self-contained packages under the configured installed
plug-ins folder. It validates `plugin.toml`, keeps every entrypoint inside its
package root, and confines external resources to host-derived
`assets/rlogs/plugins/<plugin-folder>/` or
`assets/rlogs/shared/<provider-plugin-folder>/` namespaces. It resolves required
plug-in dependencies, publishes named read-only resources without copying
them, and produces deterministic before/after operation plans.

Interactive packages can publish their own workspace or contribute a
namespaced tab to a declared dependency's workspace. The host resolves every
surface from its owner package, aggregates enabled contributions in a stable
order, and drops optional contributions when their target is absent.

The host contract is independent from any desktop UI. A desktop build may scan
an application-data `plugins/` folder while repository development scans
`plugins/installed/`.

The development desktop host now uses this crate for startup discovery,
dependency resolution, persisted desired enablement, diagnostics, and safe
workspace publication. Executable entrypoints remain declarative until the
sandboxed component and authenticated external-process adapters are ready.
