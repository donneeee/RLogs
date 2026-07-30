# rLogs Plugin Lab

Plugin Lab is the first lightweight UI for developing and testing the rLogs
extension model. It reads the live install layout and can stream checked,
sanitized `.rlog` fixtures through the bounded replay runtime.

```text
cargo run -p rlogs-plugin-lab
```

Then open `http://127.0.0.1:7418`.

The UI currently shows:

- ordinary installed, built-in, and example plug-ins;
- trusted game integrations;
- Core, ordinary plug-in API, and game plug-in API versions;
- capabilities, subscriptions, imports, exports, and asset storage;
- deterministic dependency and before/Core/after hook ordering;
- discovered replay fixtures and sealed-log verification;
- event delivery, execution cost, diagnostics, and published snapshots from the
  bundled combat timeline plug-in;
- the current-build BPSR module catalog and CN 0.2.0-compatible module
  optimizer, with safe demo data or pasted `modules.inventory` profile data;
- malformed manifests, missing resources, incompatible imports, and ordering
  failures.

The executable adapter currently accepts only first-party native replay
plug-ins linked into Plugin Lab. Directory-installed community packages are
inspection-only. A future sandboxed component adapter will use the same
capability, subscription, limit, and output contracts before community code is
allowed to execute.

Use `--root <folder>` to inspect another rLogs install tree or
`--bind <ip:port>` to change the local address. The default bind is loopback
only, and non-loopback binds are rejected because the API exposes local install
diagnostics.

The optimizer API is also loopback-only in Plugin Lab. Its request accepts
module inventory data, target/excluded attributes, minimum thresholds, and
4/5-module search settings. It does not accept or inspect account, login,
password, or token data. Full inventories use bounded deterministic beam search
by default; smaller inventories and parity checks use exact enumeration.
