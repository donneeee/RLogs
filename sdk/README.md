# Plug-in SDKs

The Rust SDK is available in [`sdk/rust`](rust). It re-exports the public
canonical-event, manifest, sealed-log, and replay contracts and adds a fixture
harness for plug-in compatibility tests.

The harness checks all of the following before a fixture test can pass:

- the package manifest is valid and exactly matches the compiled plug-in id,
  name, version, capabilities, and subscriptions;
- the `.rlog` seal, build/pack identity, event count, timestamps, and digest
  match the values pinned by the test;
- subscribed events and emitted snapshot schemas match their expected counts;
- two fresh plug-in instances produce identical normalized reports.

An empty suite is an error. When output-schema expectations are present,
unexpected snapshot schemas are also errors. Diagnostics remain available in
the report but are not treated as versioned snapshot outputs.

Run the checked-in example from the repository root:

```powershell
cargo run -p rlogs-plugin-sdk --example reference_fixture
```

For a plug-in, place sanitized `.rlog` fixtures under its test data, construct a
`FixtureCase` for every behavior the package supports, and invoke
`run_fixture_suite` from a normal Rust test. Keep every identity field and the
`content_sha256` expectation populated so replacing a fixture requires an
intentional test update. The example is a complete minimal implementation.

This SDK executes a Rust `ReplayPlugin` adapter against canonical events. It
does not grant capture, decoder, account, filesystem, network, or UI access and
does not weaken the production host's permission checks. A passing SDK test is
a deterministic compatibility check, not a substitute for the production
sandbox.

TypeScript and language-neutral local-host clients remain future work. The IPC
contract is intended to support Python, C#, Go, and other languages without
moving packet capture or protocol decoding into those runtimes.
