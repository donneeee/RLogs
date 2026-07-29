# Plug-in folder inspector

Validate and display the packages rLogs would discover:

```text
cargo run -p rlogs-plugin-inspect
```

The default folder is `plugins/installed/`. Pass one alternate folder to
inspect examples or a staged application-data directory:

```text
cargo run -p rlogs-plugin-inspect -- plugins/examples
```
