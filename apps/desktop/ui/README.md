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
navigation preferences in browser storage; the pairing controls are inert
until a native credential-vault adapter exists.

Use the `Preview blank shell` button or open `/?empty=1` to verify that rLogs
Core works with zero UI plug-ins enabled.

For the real localhost runtime, build the UI and start the Rust host from the
repository root:

```powershell
npm run build
cd ..\..\..
cargo run -p rlogs-desktop-host
```

Open `http://127.0.0.1:7419`. The Session Recorder workspace appears only when
the Rust API is present. It can run the safe replay, process an existing
private capture, or start/stop Windows process-owned live capture. The same
adapter publishes separate Log Uploader and BPSR Profile Sync workspaces.
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
