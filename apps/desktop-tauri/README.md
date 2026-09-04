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
fallback. The installer does not redistribute the Npcap driver itself. An
incompatible `Packet.dll` loaded by another component cannot block application
startup: rLogs validates the complete Packet API, pins the trusted sibling DLL
before loading `wpcap.dll`, reports any conflict in Settings, and keeps the rest
of the application available. Npcap's free license does not grant
redistribution rights.

Tagged releases remain drafts until the generated NSIS package passes an
isolated silent-install smoke test on the Windows release runner. The gate
installs into a new runner-temporary directory and verifies the installed
Cargo package executable is a non-empty Windows PE with matching product/file version
metadata, and that the package includes a non-empty uninstaller. Only then does
the workflow record the installer checksum and publish the release.
