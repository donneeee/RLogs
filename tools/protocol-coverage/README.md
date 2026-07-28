# Protocol coverage

Print every route observed in an opt-in RLogs protocol research journal:

```text
cargo run -p rlogs-protocol-coverage -- <capture.jsonl>
```

Machine-readable output:

```text
cargo run -p rlogs-protocol-coverage -- --json <capture.jsonl>
```

The command validates record sequence and monotonic time while streaming the
file. It does not retain raw packet payloads in memory.
