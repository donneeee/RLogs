# Protocol coverage

```text
rlogs-protocol-coverage [--json] [--pack <pack.json>] <capture.jsonl>
```

Without a pack, the tool reports every observed fragment and route. With a
pack, it additionally separates known and unknown packets, allowed decoders,
opaque research traffic, prohibited routes, and observed feature domains such
as entities, maps, dungeons, chat, character profiles, skills, and combat.

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
