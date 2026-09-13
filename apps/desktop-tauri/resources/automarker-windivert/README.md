# Automarker WinDivert runtime

This directory contains the reviewed WinDivert 2.2.2 x64 runtime used by the
private, read-only Automarker connection-readiness worker.

- Upstream release commit: `1789526ecfb9ff5397c94f9f54c1a3dc2fb60440`
- `WinDivert.dll` SHA-256: `c1e060ee19444a259b2162f8af0f3fe8c4428a1c6f694dce20de194ac8d7d9a2`
- `WinDivert64.sys` SHA-256: `8da085332782708d8767bcace5327a6ec7283c17cfb85e40b03cd2323a90ddc2`

The desktop verifies these exact hashes and the Windows driver signature before
opening even the passive sniff-only handle. An elevated desktop may install or
start this pinned driver through a temporary false-filter, read-only handle;
the retained readiness observer remains `SNIFF | RECV_ONLY`. Active packet
interception, mutation, sending, and Automarker placement remain disabled until
their separate lifecycle is fully verified.
