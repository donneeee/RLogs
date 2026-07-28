# Capture inspect

Validate and summarize an offline pcap or pcapng without printing packet
payloads:

```text
cargo run -p rlogs-capture-inspect -- capture.pcapng
cargo run -p rlogs-capture-inspect -- --json capture.pcapng
```

This is a capture-boundary diagnostic. It does not reconstruct TCP streams or
decode game packets yet.

