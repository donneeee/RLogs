# Native desktop application

This is the thin Tauri 2 application layer for rLogs. Capture, decoding,
canonical events, plug-in execution, profiles, submissions, and combat
calculations remain in the reusable Rust crates.

From the repository root:

```powershell
npm --prefix apps/desktop/ui run build
cargo run -p rlogs-app
```

The normal application opens its own native window. The older
`rlogs-desktop-host` binary remains available as an explicit browser-based
developer and diagnostic entrypoint.

The native application resolves data and plug-ins from the repository checkout
in development, from `RLOGS_INSTALL_ROOT` when explicitly overridden, and from
Tauri's installed resource directory in release builds. The NSIS bundle uses a
per-user writable installation by default, so `runtime-data/` is created beside
the installed resources under the user's local application-data tree.

On Windows the desktop opens installed Npcap directly and discovers the
process-matched `\\Device\\NPF_{GUID}` interface on first run. Wireshark and
`dumpcap.exe` are not required; a saved dumpcap path is only a compatibility
fallback. The installer does not redistribute the Npcap driver itself. A
mismatched installed `wpcap.dll`/`Packet.dll` pair cannot block application
startup: rLogs preflights the required `Packet.dll` export, reports the Npcap
repair requirement in Settings, and keeps the rest of the application
available. Npcap's free license does not grant redistribution rights.
