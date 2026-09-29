<!-- SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0 -->
<!-- Copyright 2026 Anton (darkcite) -->
# Ephem

Ephemeral, serverless, end-to-end encrypted chat that runs entirely in the browser (Rust → WebAssembly). No account, no server, no history.

- **Chats and rooms** (up to 16 people), up to 16 of them open at once in one tab: replies, edits, deletes, reactions, read receipts, self-destructing messages.
- **Direct mode** (`app/`): WebRTC DataChannel between the browsers, Noise KK; two codes, one each way, by QR or link; never a relay.
- **Tor mode** (`app/tor.html`): the Tor client (arti) runs inside the page and reaches Tor through Snowflake; chats go onion service to onion service with Noise IK, so nobody sees anyone's IP address. One code, no answer; saved contacts reconnect without a code. Optional Snowflake **bridge lines** for networks where the defaults are blocked.
- **Contacts and contact cards**, kept only in an encrypted key file (Argon2id); several saved identities per device; identity transfer between devices (direct mode).
- **Public channels**: the owner posts, anyone with the link reads, through Tor. A channel is IPFS data (CIDv1, DAG-CBOR, CAR, IPNS records) served on an onion service from the owner's tab, verified by every reader; followers can mirror it. In the app's **Following** and **My channels** tabs.

- **Try it:** `https://darkcite.github.io/ephem/` (landing) → `/app/` (direct) or `/app/tor.html` (Tor)
- **Design, spec, plan and checkpoint results:** [`docs/P2P-CHAT.md`](docs/P2P-CHAT.md)

## Layout

| Path | What |
|---|---|
| `index.html`, `site.css` | Landing page |
| `app/` | The web app: `index.html` (direct) and `tor.html` (Tor, generated), `app.js` (chats, shell), `channels.js` (Following, My channels), `bridges.js`, `slots.js`, `ui.js`; `app/pkg/` is the built WASM (committed so GitHub Pages can serve it) |
| `crates/proto` | `no_std`, allocation-free wire formats: codes, cards, SDP template, frames |
| `crates/crypto` | Identity, Noise KK/IK, in-place transport cipher, SAS, key file, contacts |
| `crates/core` | Sans-IO session and room state machines (natively tested) |
| `crates/snowflake` | Snowflake client (Turbotunnel, KCP, smux), sans-IO |
| `crates/tor` | arti in the page over Snowflake, onion services, bridge lines |
| `crates/channel` | Public channels: CID, strict DAG-CBOR, CAR, IPNS V2, signed manifest and posts, the onion gateway (sans-IO) |
| `crates/channel-web` | Channels in the browser (owner, reader, mirrors); part of the Tor build |
| `crates/wasm` | The browser adapter: WebRTC, Tor streams, several chats per tab, QR encode/scan. Two builds: direct (`pkg/ephem*`, 232 KB gzip) and Tor (`pkg/ephem_tor*`, 2.2 MB gzip; the direct page loads it only for its channel tabs) |
| `vendor/` | arti 0.46 with four small wasm patches (see `vendor/README.md`) |
| `tools/` | `stamp.py` (integrity hashes, CSPs, `tor.html`, service-worker version; run by `build.sh`), `make_icons.py` |
| `checks/` | Checkpoint suite (`checks/run_all.sh`), end-to-end tests, and the offline Tor lab (`checks/tor-lab/`) |

## Build and test

```sh
rustup target add wasm32-unknown-unknown
cargo install wasm-bindgen-cli --version 0.2.129
brew install llvm                  # macOS only: a clang that can build C for wasm32 (Apple's cannot)
./build.sh                         # → app/pkg/, then stamps app/index.html, app/tor.html and app/sw.js
cargo test --workspace             # native unit tests
cd checks && npm install && cd ..  # once
./checks/run_all.sh                # the whole suite; ONLY=app or ONLY=tor for a part
```

Serve the repository root with any static server (for example `python3 -m http.server`) and open `/app/` or `/app/tor.html`.

The Tor tests run in an offline lab (a private Tor network with the Go Snowflake broker, proxy and server; `checks/tor-lab/lab.sh up`) and, unless `NET=0`, on the real Tor network: `ONLY=tor ./checks/run_all.sh`.

After editing `app/*.js` or `app/*.css`, run `python3 tools/stamp.py` (or `./build.sh`): the pages pin their hashes, so unstamped edits are refused by the browser.
