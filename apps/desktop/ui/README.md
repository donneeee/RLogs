# rLogs desktop shell UI

This is a framework-free TypeScript shell. It is intentionally separate from
Plugin Lab and from any specific game plug-in.

```powershell
npm install
npm run dev
```

Open the local Vite address. The development adapter loads three safe example
workspaces by default:

- Combat Meter demonstrates a one-surface plug-in with no redundant tab bar.
- Profile Sync demonstrates real Profile, Sync, and Options tabs.
- Log Library demonstrates another multi-tab workspace.

Profile Sync also receives a `Modules` tab contributed by a separate sample
add-on. The ADD-ON badge makes that ownership visible. The shell stores only
navigation preferences in browser storage. In the native Windows build, the
pairing controls validate the app token with the submission service and hand
it to the host for transactional storage in Windows Credential Manager.

Use the `Preview blank shell` button or open `/?empty=1` to verify that rLogs
Core works with zero UI plug-ins enabled.

For the real localhost runtime, build the UI and start the Rust host from the
repository root:

```powershell
npm run build
cd ..\..\..
cargo run -p rlogs-desktop-host
```

Open `http://127.0.0.1:7419`. Debug builds expose **Session Tools** under
**Settings** for safe replay, private-capture processing, manual capture
controls, sealed-session review, and canonical-event diagnostics. Vite removes
those surfaces from production UI assets, and the release host also removes
their plug-in and routes. The same adapter publishes separate Log Uploader and
BPSR Profile Sync settings.
Profile Sync can build bounded local packages from the last sealed log,
summarize current packages, and lazily inspect exact JSON without external
network activity.

## Host boundary

`DesktopHostAdapter` is the only environment seam. The development adapter
renders safe fixtures and the local-host adapter talks to the real Rust
runtime. A packaged desktop adapter will:

1. receive already validated and aggregated workspace descriptors from
   `rlogs-plugin-host`;
2. persist ordering and active tabs in host settings;
3. mount exactly one isolated package surface for the active tab;
4. broker typed capabilities instead of exposing engine internals;
5. store pairing secrets in the operating system credential vault.

The packaged Windows adapter implements all five boundaries. Vite-only preview
mode keeps account controls inert because it has no native credential vault.
