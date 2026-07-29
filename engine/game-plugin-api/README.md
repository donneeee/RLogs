# rLogs game plug-in API

This crate defines the manifest boundary for trusted, native game integrations.
Game plug-ins may receive reconstructed game streams, decode game protocols,
provide game-data catalogs, and project privacy-reviewed website payloads.

This is intentionally separate from the ordinary community add-on API. A game
plug-in has access to sensitive raw transport evidence and must be bundled or
explicitly trusted by the host.
