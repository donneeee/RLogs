# Tools

Top-level developer tools are game-neutral. Tools that understand a concrete
game protocol live under that trusted game plug-in.

Tools must preserve unknown evidence and respect the same privacy
classification policy as the runtime.

- `capture-inspect`: validates and summarizes pcap/pcapng without displaying
  payloads.
- `process-capture`: consumes live dumpcap frames through memory, attributes
  exact TCP four-tuples to a Windows process, and persists only confirmed
  process-owned frames.
- `plugin-inspect`: validates the `plugins/installed/` folder and prints
  discovered packages, shared imports/exports, load order, and operation hooks.
- `reference-reconcile`: compares sanitized observed routes with
  exact-revision, lineage-aware route-name manifests and preserves conflicts.

The BPSR-specific `profile-audit`, `protocol-journal`, and
`protocol-coverage` tools live at
`plugins/games/blue-protocol-star-resonance/tools/`.
