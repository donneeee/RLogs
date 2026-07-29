# rLogs Plugin Lab

Plugin Lab is the first lightweight UI for developing and testing the rLogs
extension model. It reads the live install layout and does not execute plug-in
code.

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
- malformed manifests, missing resources, incompatible imports, and ordering
  failures.

Use `--root <folder>` to inspect another rLogs install tree or
`--bind <ip:port>` to change the local address. The default bind is loopback
only, and non-loopback binds are rejected because the API exposes local install
diagnostics.
