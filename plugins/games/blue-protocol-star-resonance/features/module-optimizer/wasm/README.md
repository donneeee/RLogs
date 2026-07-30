# Browser adapter

This crate packages the existing `rlogs-bpsr-module-optimizer` engine for a
browser. It does not reimplement scoring in JavaScript.

At build time, `build.rs` reads the reviewed exact-build catalog from the BPSR
game plug-in and embeds a compact runtime catalog. The exported WebAssembly
functions use JSON strings:

- `optimizer_catalog_json()` returns the public attribute catalog used by the UI;
- `optimize_json(request)` accepts the native `OptimizeRequest` contract and
  returns the native `OptimizeResponse` contract.

Module instance IDs remain strings across the WebAssembly boundary. No packet
capture, login, account, password, token, or network-submission code is linked
into this adapter.

Build a browser package with:

```text
rustup target add wasm32-unknown-unknown
cargo build --release --target wasm32-unknown-unknown \
  -p rlogs-bpsr-module-optimizer-wasm
wasm-bindgen --target web --out-dir <site-output-directory> \
  target/wasm32-unknown-unknown/release/rlogs_bpsr_module_optimizer_wasm.wasm
```
