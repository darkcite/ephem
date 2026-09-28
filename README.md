# Ephem

Ephemeral, serverless, end-to-end encrypted 1:1 chat that runs entirely in the browser (Rust → WebAssembly, WebRTC DataChannel, Noise KK). Two codes, one each way, by QR or link; no account, no server, no history.

- **Try it:** `https://darkcite.github.io/ephem/` (landing) → `/app/`
- **Design, spec, plan and checkpoint results:** [`docs/P2P-CHAT.md`](docs/P2P-CHAT.md)

## Layout

| Path | What |
|---|---|
| `index.html`, `site.css` | Landing page |
| `app/` | The web app; `app/pkg/` is the built WASM (committed so GitHub Pages can serve it) |
| `crates/proto` | `no_std`, allocation-free wire formats: codes, SDP template, frames |
| `crates/crypto` | Identity, Noise KK handshake, in-place transport cipher, SAS |
| `crates/core` | Sans-IO session state machine (natively tested) |
| `crates/wasm` | The only crate touching the browser (web-sys WebRTC adapter, QR) |
| `checks/` | Checkpoint suite (`checks/run_all.sh`) and the app end-to-end test |

## Build and test

```sh
rustup target add wasm32-unknown-unknown
cargo install wasm-bindgen-cli --version 0.2.129
./build.sh                         # → app/pkg/
cargo test --workspace             # native unit tests
cd checks && npm install && cd ..  # once
E2E_BROWSER=chrome node checks/e2e_app.mjs   # two browsers chat end to end
```

Serve the repository root with any static server (for example `python3 -m http.server`) and open `/app/`.
