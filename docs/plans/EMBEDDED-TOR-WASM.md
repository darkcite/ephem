# Plan: Tor built into the WASM app (no companion), for iOS and desktop

| Field | Value |
|---|---|
| Status | **The only Tor path** (the owner decided on 2026-09-28: no native programs, SPEC P10). Normative summary in SPEC §28. **G1 passed** (compile level, spike E1); G2–G4 open |
| Relates to | SPEC v0.5 §28 (Tor mode), [`TOR-AND-IP-PRIVACY.md`](TOR-AND-IP-PRIVACY.md), [`PUBLIC-CHANNELS-IPFS.md`](PUBLIC-CHANNELS-IPFS.md) |
| Constraint | Free, with no registration. It may use Tor Project infrastructure (the Snowflake broker, Snowflake bridges) and volunteer Snowflake proxies |
| Date | 2026-09-28 |

---

## 1. Verdict

**Technically possible, but hard, and with one limit that cannot be removed on iOS.**

- A browser **cannot open TCP connections**, and a normal Tor client needs them to reach Tor relays. The only transport a web page *can* use to enter the Tor network today is **Snowflake**: WebRTC to a volunteer proxy, which forwards to the Tor Project's Snowflake bridge. WebRTC is exactly what browsers do well, iOS Safari included.
- On top of Snowflake, a full Tor client has to run **inside WASM**: link TLS, circuits, directory, onion-service client **and** host. The natural base is **arti**, the Tor Project's Rust implementation. But `wasm32-unknown-unknown` is **not an official arti target**, so a maintained fork or patch set is expected.
- **The limit that cannot be removed:** on iOS the app only runs in the foreground. When the PWA goes to the background, Snowflake's WebRTC connection and all circuits are suspended, so **your onion service is unreachable while the app is in the background**. Contacts can reach you only while your app is open. There is no workaround in a PWA.

## 2. Architecture

```
┌────────────────────────── PWA (tor.html, own CSP) ──────────────────────────┐
│ core (sans-IO) ── Transport::Tor ──▶ tor-wasm (arti fork, wasm32)            │
│                                     ├─ runtime: wasm-bindgen-futures + timers │
│                                     ├─ TLS: rustls (RustCrypto provider)      │
│                                     ├─ dir cache: IndexedDB (public data)     │
│                                     ├─ keys: in-memory keystore only          │
│                                     └─ PT: snowflake-wasm (Rust)              │
│                                          ├─ broker rendezvous: HTTPS fetch    │
│                                          ├─ RTCPeerConnection to a volunteer  │
│                                          │  proxy (web-sys)                   │
│                                          └─ Turbotunnel: KCP + smux over the  │
│                                             DataChannel                       │
└─────────────────────────────────────┬───────────────────────────────────────┘
                                      │ WebRTC (UDP)
                             volunteer Snowflake proxy
                                      │ WebSocket
                             Snowflake bridge (Tor Project) ──▶ Tor network ──▶ peer's onion
```

| Component | What must be built | Existing base | Risk |
|---|---|---|---|
| Async runtime for arti | Implement arti's runtime traits (spawn, sleep, time, TCP/TLS provider → PT streams) over `wasm-bindgen-futures` and JS timers | arti's runtime abstraction (`tor-rtcompat`) is designed to be swappable | Medium |
| Time, randomness, filesystem | Patch uses of `std::time::SystemTime` (which panics on wasm32), `std::fs`, and threads | `web-time`, `getrandom` (wasm_js) | **Resolved (E1):** upstream already handles wasm32 (`coarsetime` uses `performance.now()`, and state is in memory on wasm), so no fork is needed |
| Directory store | arti builds without SQLite on wasm; we add persistence of the cache to IndexedDB for warm starts | arti has storage traits | Medium |
| Link TLS | rustls on wasm32 with a pure-Rust crypto provider | rustls with a RustCrypto provider | Medium |
| Snowflake client | Broker rendezvous, WebRTC to the proxy, and **Turbotunnel (KCP + smux)** in Rust; plugged in through arti's `AbstractPtMgr` / `ChanMgr::set_pt_mgr` (no fork) | The reference client is Go; the broker sends CORS `*` (source); Rust `kcp` exists (28 releases); smux must be written (small) | **High** |
| Onion-service host | Hosting from a browser tab: intro points, rendezvous, in-memory keys | `tor-hsservice` in arti | Medium–High |
| Binary size | arti plus a Snowflake client and rustls | — | **Expected several MB**, loaded lazily **only** by Tor sessions (`tor.html`) |

## 3. Behaviour and limits

| Topic | Desktop (embedded) | iOS (embedded) |
|---|---|---|
| Bootstrap | First time: fetch the consensus and microdescriptors (a few MB) through Snowflake, which takes tens of seconds. Later, reuse the directory cached in IndexedDB | Same. Every return to the foreground re-attaches, which takes seconds |
| Onion hosting (being reachable) | While the tab is open | **Only while the app is in the foreground** |
| One-way invite (SPEC §28.4) | ✓ | ✓, but the inviter must keep the app open until the peer dials |
| Contacts reconnect | ✓ while open | ✓ only while both apps are in the foreground |
| Rooms over Tor | ✓ | Possible, but each member must stay in the foreground; practically weak |
| Latency | Snowflake adds a hop and its own jitter: expect 0.5–2 s per message | Same |
| Censored networks | The broker is reached by direct HTTPS. **Domain fronting is impossible from a browser**, because `fetch` cannot override the `Host` header. The AMP-cache rendezvous needs CORS support *(spike E2)*. Where the broker is blocked, embedded Tor fails, and there is no fallback (P10) | Same, with no fallback |

**Privacy notes:**

- The Snowflake proxy (a volunteer) sees your IP and that you use Snowflake, but not your destination. This is the same as a guard or bridge.
- The broker sees your IP when you rendezvous.
- The TLS SNI `snowflake-broker.torproject.net` shows network observers that you use Tor.
- **Bridges on by default** (your decision) is automatic here: Snowflake *is* a bridge.

## 4. Changes to the SPEC if this succeeds

- **§28.1:** Tor mode also becomes available on **iOS** and on desktop **without** the companion.
- **§28.5 no-mixing guard, restated:** in a Tor session, `RTCPeerConnection`s may be created **only by the Snowflake transport, to Snowflake proxies**, using Snowflake's STUN list. **Never to a chat peer.** `core` still emits no `CreatePc` for peers.
- **Separate entry page:** `tor.html`, with a CSP that allows only `'self'` plus the Snowflake broker and rendezvous origins. `index.html` (direct mode) keeps `connect-src 'self'` plus the companion's loopback port.
- **There is no companion** (SPEC P10). Consequences:
  - no always-on hosting while the tab is closed;
  - no obfs4 or WebTunnel for networks where the Snowflake broker is blocked;
  - no path that avoids the extra Snowflake hop.

## 5. Research plan with gates

| Step | Work | Gate (must pass to continue) |
|---|---|---|
| E1 | Build the arti client crates for `wasm32-unknown-unknown` with a stub runtime; list every blocker (time, fs, threads, SQLite, TLS) | **G1: PASSED 2026-09-28.** All 16 crates and the full feature set build, upstream already has wasm stubs, and there is no fork. TLS uses ring. See `../spikes/RESULTS-2026-09-28.md` |
| E2 | Snowflake from the browser: call the broker `/client` from a `github.io` origin (CORS), direct and through the AMP cache; open a DataChannel to a proxy on desktop **and iOS Safari** | **G2:** the rendezvous and DataChannel work on both, without domain fronting |
| E3 | Turbotunnel (KCP + smux) in Rust over the DataChannel, talking to the real Snowflake bridge | Byte stream to the bridge is stable for 10 minutes |
| E4 | arti over the E3 stream: bootstrap, a 3-hop circuit, and connecting to a known onion service | **G3:** time to bootstrap ≤ 60 s cold and ≤ 10 s warm; an onion connects on desktop and iOS |
| E5 | Onion-service **hosting** from a tab, with keys held in memory | A second client reaches it; publishing the descriptor completes in ≤ 60 s |
| E6 | Size and performance: WASM size (compressed), memory on iOS, battery during 10 minutes of chat | **G4:** ≤ 5 MB compressed, lazily loaded; no iOS memory kills |
| E7 | Hardening: fuzz the Snowflake and Turbotunnel parsers, review the fork's diff against upstream arti, plan for tracking upstream | Security review signed off |

- **If G2 fails:** embedded Tor is not viable today, so **there is no Tor mode**. Users get IP privacy from a VPN or WARP (SPEC §29), and public channels with a hidden owner are not offered.
- **If G3 passes but E5 fails:** ship **client-only** embedded Tor. iOS and desktop tabs can **dial** onions (a companion host, or a channel), but cannot host. One-way invites from an iPhone then do not work, and the iPhone user dials instead.

## 6. Effort and maintenance

- This is the largest item in the whole project, bigger than MVP-1. Most of the cost is in E1 (the fork), E3 (a Snowflake client in Rust) and E5.
- **Ongoing cost:** following arti releases and Tor protocol changes. Security fixes in Tor must be merged into the fork **quickly**.
- **Recommendation:** run E1 and E2 as early spikes, because they are cheap and decide feasibility. Commit to E3–E7 only after G1 and G2 pass.
