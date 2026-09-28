# Spike scripts

Results: [`../RESULTS-2026-09-28.md`](../RESULTS-2026-09-28.md).

| Directory | Spikes | How to run |
|---|---|---|
| `webrtc/` | S1, S2, S4 | `npm i playwright-core@1.56`, then `node run.js` (S1/S2) and `node s4.js` (S4). Set the Chromium path in both scripts. `page.js` is the in-browser code: it extracts the minimal fields and rebuilds the remote SDP from the SPEC Appendix A template |
| `wasm-sizes/` | S5 | `RUSTFLAGS='--cfg getrandom_backend="wasm_js"' cargo build --release --target wasm32-unknown-unknown --features noise,keyfile,qr[,mls]`, then gzip the `.wasm` |
| `argon2-timing/` | S5b | `cargo build --release --target wasm32-unknown-unknown`, then load the `.wasm` in Node and call `kdf(m_kib, t)` |
| `arti-wasm32/` | E1 | `./check.sh` in an empty directory. For the full feature set, see the results file |
