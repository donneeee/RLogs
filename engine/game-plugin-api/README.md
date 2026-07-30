# rLogs game plug-in API

This crate defines the manifest boundary for trusted, native game integrations.
Core remains the sole capture owner. A selected game plug-in may receive only
the process-filtered, reconstructed game streams handed to it, then frame,
decrypt when required, decompress, and decode that game's protocol. It may
also provide game-data catalogs and project privacy-reviewed website payloads.

This is intentionally separate from the ordinary community add-on API. A game
plug-in has access to sensitive raw transport evidence and must be bundled or
explicitly trusted by the host. It cannot create an independent capture path,
and credential/account-authentication decoding remains prohibited.
