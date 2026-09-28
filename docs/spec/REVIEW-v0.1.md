# Review of "P2P Ephemeral Chat — Full Rust/WASM Specification" v0.1

| Field    | Value                                                |
|----------|------------------------------------------------------|
| Reviewed | v0.1 (Architecture / Protocol Draft)                 |
| Output   | [`SPEC.md`](SPEC.md) v0.2 (all decisions below are applied there) |
| Date     | 2026-09-28                                           |
| Status   | Open questions pending owner answers (see §4)        |

This document records **why** v0.2 differs from v0.1. Every point is one of:

- **REJECT**: the v0.1 claim is technically wrong or cannot be built in a browser. It was replaced.
- **AMEND**: the direction is right, but the mechanism was missing, too optimistic or underspecified.
- **CONFIRM**: correct as written. It was kept, sometimes made more precise.

Items marked *(spike)* depend on browser behaviour and must be checked by a validation spike before they are relied on (see SPEC §24).

---

## 1. Rejected points

### R1 — "Mode A — Single QR" (§14, §16) cannot work with browser WebRTC

v0.1 says the joiner (Bob) may connect "without sending anything back". That is impossible with `RTCPeerConnection`, for three independent reasons:

1. **DTLS authentication.** Alice's browser checks the peer's DTLS certificate against the `a=fingerprint` of the **remote description**. Alice has no remote description until she applies Bob's answer. The DTLS handshake is never allowed to complete, so the connection never reaches `connected`.
2. **ICE credentials.** Alice's ICE agent needs Bob's `ice-ufrag` and `ice-pwd` to send authenticated connectivity checks (the STUN USERNAME/MESSAGE-INTEGRITY attributes). Without them she cannot finish the check handshake in both directions.
3. **No pre-shared-key workaround.** In theory Alice could choose Bob's credentials ahead of time and put them in the QR. But `RTCCertificate` has no import API: `generateCertificate()` always makes a fresh key pair, so Bob cannot use a certificate that Alice chose. Munging the fingerprint therefore makes DTLS fail.

**Decision:** the two-way exchange (offer → answer) is the **only** bootstrap mode. The work goes into making the exchange as cheap as possible instead:

- a binary invite of about 190 bytes rather than an SDP of 1–2 KB (see R4);
- in person: two scans (Alice's QR, then Bob's QR);
- remote: link out, link or code back;
- MVP-3 adds peer-relayed signalling, so after the first contact, new room members need no extra out-of-band exchange with anyone except one existing member.

### R2 — "A new QR should not normally be necessary" after a Wi-Fi change (§33, §82)

An ICE restart is an offer/answer exchange, and that needs a **signalling channel**. Here the only signalling channel after bootstrap is the DataChannel itself, and it runs over the path that just died. When a device's only network changes, every link it has drops at the same moment. Nothing is left to carry the restart.

**Decision:** replace this with an explicit **recovery ladder** (SPEC §13):

| Tier | Mechanism | Works when |
|---|---|---|
| T0 | The browser switches to an already-validated backup pair, or continual gathering finds a peer-reflexive path. No signalling. | A second interface was gathered at connect time, or the peer that did not move is directly reachable *(spike)* |
| T1 | ICE restart signalled over the **still-alive** DataChannel | The network is degrading but not yet dead (`online` or `connection.change` fires first, or the path is `disconnected` but not yet `failed`) |
| T2 | ICE restart signalled **through another room member** (E2E-encrypted, so the member only forwards ciphertext) | Groups, where the break is partial |
| T3 | **Resume code**: a new short out-of-band exchange. The same static keys rebind the session automatically; room state, sequence numbers and membership are kept. | Always |

The honest claim is: *"a new exchange is not needed while at least one path survives"*.

### R3 — `chat://join/<payload>` (§13, §66)

A PWA cannot register a custom scheme. The phone camera app ignores `chat://`. `registerProtocolHandler` and manifest `protocol_handlers` only accept `web+*` schemes and are Chromium-only.

**Decision:** use `https://<host>/<base>/#i=<base64url>`. The **URL fragment** is never sent to the host, so GitHub Pages never sees or logs the invite. The app strips it straight away with `history.replaceState` so it does not stay in browser history.

### R4 — "CBOR + optional compression + base64url" for the invite (§13, §62, §63)

- An offer is mostly high-entropy data (credentials, a fingerprint, keys). Compression does nothing useful on it.
- CBOR spends bytes on keys and tags for no benefit.
- Most of an SDP is **boilerplate that can be rebuilt** from a template.

**Decision:** send only the fields that vary: ufrag, pwd, DTLS SHA-256 fingerprint, the static public key, and packed candidates. Use a **fixed-layout little-endian binary** format that can be parsed without copying, and **rebuild the SDP from a template** on the receiver. This is legal, because a *remote* description can be any valid SDP. Result: about 190 bytes, which is about 256 base64url characters and a QR of about version 12-M. Multi-frame QR (§63) becomes unnecessary and is **removed from scope**.

### R5 — Candidate reuse from "other WebRTC connections" in the browser (§25, §29)

No browser API exposes candidates from other peer connections, and certainly not from other origins. Inside this app, reusing a stale candidate adds nothing that ICE does not already do. **Removed.**

### R6 — Flood-forwarding in a full mesh (§43)

In a full mesh every member already gets each message directly. Flooding multiplies traffic by about N and adds no delivery guarantee. Worse, a peer that forwards another peer's traffic **is a relay**, which contradicts P8 ("never silently introduce a relay").

**Decision:**

- no forwarding by default;
- deduplicate with a fixed per-sender high-water mark;
- forwarding is allowed only as an **explicit, visible** "relayed via X" gap-fill for pairs that cannot connect directly (open question Q6).

### R7 — Retrying the *initial* connection from `ICE_CONNECTING` (§30)

Before the first connection there is no in-band channel. If ICE fails, there is nothing new to try with the same descriptions.

**Decision:** an initial ICE failure leads to `FAILED(E_NO_DIRECT_PATH)` and an offer to make a new invite. `RETRYING` exists only after a connection has been established.

---

## 2. Amended points

### A1 — STUN is "optional" (§4.2, §27): in practice it is required for anything off-LAN

Current Chrome, Safari and Firefox replace **host candidates with mDNS names** (`<uuid>.local`). These names only resolve on the same link. Without STUN, two peers on different networks have **no usable candidate at all**. That includes public IPv6 hosts, because their global address is hidden behind mDNS too.

In practice the IPv6 direct path (§26, which is correct in principle) is only found through a **dual-stack STUN server**, which reports the global v6 address as `srflx`.

**Also:** the third-party STUN operator learns both peers' public IPs and when they connect. This is a metadata disclosure that the privacy claims must name. How STUN is configured is open question Q2.

### A2 — Camera permission changes what the peer can see *(spike)*

Chromium stops hiding host IPs with mDNS once a page has been granted camera or microphone permission. The **in-app QR scanner needs camera permission**, so after the first scan the invite may contain real LAN IPv4 and IPv6 addresses. This is good for same-LAN connectivity and bad for privacy.

**Decision:** the invite builder sends only the candidate classes the user's privacy mode allows (SPEC §9.4), whatever the browser happens to expose.

### A3 — "TURN explicitly disabled" (§28): enforcement must also cover the remote side

Leaving TURN out of our own `iceServers` stops relay candidates on **our** side only. A peer could still announce `relay` candidates from **its own** TURN server, and our traffic would then flow through that server.

**Decision:**

1. Drop every remote candidate whose type is `relay` before it is applied.
2. After connecting, check with `getStats()` that neither side of the selected pair is `relay`. If one is, close with `E_RELAY_REJECTED`.
3. State the limit honestly: candidate types are **self-declared**. A malicious peer can hide a relay or VPN behind a `srflx` or `host` label. We guarantee that *we* add no relay. We cannot guarantee that the peer does not route through one.

### A4 — Application E2E over DTLS (§20, §21): keep it, but base it on out-of-band authentication

In a 1:1 session, DTLS is already end-to-end, and the out-of-band exchange of the fingerprints already authenticates it. The application layer earns its place through three things:

- (a) **identity that survives reconnection**: each new `RTCPeerConnection` gets a new DTLS certificate, but the static key stays the same;
- (b) group semantics (MLS);
- (c) independence from the transport.

**Decision for 1:1:** use **Noise KK** (`Noise_KK_25519_ChaChaPoly_BLAKE2s`). Both static keys are already known from the invite and the answer, so it takes 1 round trip. The prologue is `invite ‖ answer`, which binds the Noise session to the same out-of-band data that pins the DTLS fingerprints.

**Security then comes down to the integrity of the out-of-band channel.** A code shared through a messenger can be swapped by the messenger (a man-in-the-middle attack). So v0.2 adds a **short authentication string (SAS)**, derived from the handshake hash, for users to compare when the exchange was not in person.

### A5 — MLS for groups (§46): right choice, but serverless ordering is a real problem

MLS assumes a delivery service that **orders Commits**. A mesh has no such service, so two concurrent commits fork the group.

**Decision (proposed):**

- a **single committer**, the room owner;
- other members send Proposals only;
- the owner's successor is chosen deterministically (the lowest connected leaf index);
- the case of a network split producing two owners is documented as a limit.

Open question Q4.

### A6 — Identity (§8–§10): make it smaller and more precise

- `PeerId` = the 32-byte X25519 static public key itself. No hash or second key is needed for 1:1. A signature key is added only when MLS arrives.
- `anon_7F2A91` has 24 bits. It is a display handle, and collisions are easy to find, so it must never be shown as proof of identity. The SAS is the proof.

### A7 — Invite expiry (§13, §64, §65): enforce it where the state lives

- Only Alice's `RTCPeerConnection` can accept the answer, so **Alice enforces** the expiry, using her own clock.
- Bob's check is advisory only, with ±120 s tolerance for clock skew.
- An offer belongs to exactly one peer connection, so **every invite is single-use by construction**. §65 ("invalidate after the first connection") is automatically true.

### A8 — Envelope and message IDs (§36–§39): keep the metadata out of plaintext

A 32-byte sender ID plus a timestamp in every frame header is wasteful and gives away metadata.

**Decision:**

- The outer header is 12 bytes: version, type, flags, sequence number. It is authenticated as associated data (AAD).
- Everything else, including the inner message type, is **inside the ciphertext**.
- The replay check in 1:1 is the Noise nonce, which must equal the expected counter. The DataChannel is ordered and reliable, so any gap is fatal.
- Resending after a reconnect is deduplicated by comparing a per-sender `chat_seq` against a high-water mark. There is **no growing "seen" set**, which also meets the zero-allocation rule.

### A9 — Data model (§58): no `Vec`/`String` in protocol types

This follows the Rust doctrine. Use fixed arrays with explicit counts (`[CandidateBin; 8]` plus `n_candidates: u8`). Protocol types are `#[repr(C)]` and `Copy`, and they borrow `&[u8]` views instead of owning buffers.

The field `bootstrap: Vec<u8>` had no defined meaning and is **removed**.

### A10 — WASM API and the `P2pTransport` trait (§59, §60)

The v0.1 trait is synchronous and the API has no `self`, but every browser WebRTC call is async.

**Decision:** a **sans-IO core**. `core` is a pure, deterministic state machine: it takes `(now, Input)` and returns `Action`s through a fixed-capacity sink. It has no `wasm-bindgen` dependency, so it builds and tests natively, including replay and fuzz tests. The `wasm` crate is the only I/O adapter: `web-sys` WebRTC plus event wiring. The JS surface is just the bootstrap.

### A11 — Copies at the JS↔WASM boundary (user networking rule)

Zero-copy is not fully possible across the WASM boundary. Every remaining copy is documented in SPEC §11.6:

- receive: 1 unavoidable copy, from the `ArrayBuffer` into linear memory;
- send: 0 copies on our side, because `send()` is given a view of linear memory (the browser then copies internally);
- UTF-8↔UTF-16 conversion at the UI edge.

### A12 — The code host is part of the trust base (§51, §52)

"No backend" does not mean "no trusted party". Whoever controls GitHub Pages, or the repository, serves the code that holds the keys.

**Decision:**

- a strict CSP set in a `<meta>` tag (Pages cannot set headers);
- the service worker pins a build version and asks the user before updating, showing the new build hash;
- reproducible builds, with hashes published in Releases;
- no third-party scripts.

This is added to the threat model and to the list of claims we must not make.

### A13 — Wallet authentication (§10): hidden infrastructure and privacy costs

- On mobile, a wallet is usually reached through **WalletConnect, which needs the WalletConnect relay server**. That is a runtime third-party dependency.
- An injected provider only exists in desktop extensions and in the wallets' own in-app browsers.
- **ERC-1271** smart-contract wallets need an RPC call (`eth_call`) to verify. A plain EOA signature can be checked offline with `ecrecover`.
- A wallet address is a **permanent identifier that links sessions**. It must only ever be sent inside the encrypted channel, never in an invite.

Open question Q5.

### A14 — Network-change detection (§31)

The reliable signals are `iceconnectionstatechange` (`disconnected` after about 5 s, `failed` after about 30 s), `online`/`offline`, and `navigator.connection.onchange` (Chromium only). Consent freshness (RFC 7675) already checks the path about every 5 s. The application PING (§74) is kept for application-level liveness only, such as detecting a frozen tab, and its default interval is 15 s.

### A15 — Answer delivery for remote exchange (UX gap)

If Bob sends his answer as a link and Alice taps it, it opens in a **new tab**. That tab does not hold Alice's `RTCPeerConnection`.

**Decision:** the new tab forwards the answer to the tab that owns the connection through a `BroadcastChannel`, then closes. On iOS, a standalone PWA and a Safari tab do not share a storage partition, so paste or scan inside the app is the reliable path there *(spike)*.

### A16 — iOS and mobile backgrounding during the exchange (§54)

To share an invite remotely, Alice has to switch to a messenger. iOS may suspend the PWA and drop the pending peer connection. This is recorded as an **MVP-1 risk** with a spike (S6). Mitigations: the system share sheet (which keeps the app in the foreground), a short TTL, and regenerating the invite if it went stale.

### A17 — Maximum message size and backpressure (§72, §73)

- 16 KiB is the largest DataChannel message that works reliably across browsers.
- The chat plaintext limit is 4 KiB.
- The send queue is a fixed ring. It stops draining at a `bufferedAmount` high-water mark of 256 KiB and resumes on `bufferedamountlow` at 64 KiB.
- A full queue returns `E_BACKPRESSURE` to the UI. It never grows the queue.

### A18 — HFT doctrine vs. the browser (user rules)

Several rules cannot apply inside a browser sandbox:

- no CPU affinity or NUMA control;
- no AVX2 (WASM has `simd128`);
- no raw sockets;
- no threads, because GitHub Pages cannot send the COOP/COEP headers that `SharedArrayBuffer` needs.

SPEC §22 maps each rule to its browser equivalent or marks it not applicable, so nobody tries to "implement" them.

---

## 3. Confirmed points (kept)

| v0.1 § | Point | Note |
|---|---|---|
| P1–P8 | Design principles | P9 was added: *security never exceeds the integrity of the out-of-band channel and of the code host* |
| §3, §6, §61 | Rust owns protocol, state and crypto; JS is only an adapter | Strengthened by the sans-IO core (A10) |
| §5, §86 | Forbidden infrastructure; no custom DTLS, NAT traversal or crypto | Kept verbatim |
| §11 | 16-byte random `RoomId` | |
| §17 | The out-of-band answer path (QR, clipboard, share, AirDrop) | Now the only mode (R1) |
| §19 | Transport stack ICE → DTLS → SCTP → DataChannel | Correct |
| §22–§24 | Confidentiality yes; IP anonymity and DPI-invisibility **not** guaranteed; no obfuscation | Excellent and honest. Kept verbatim |
| §26 | IPv6 as first-class | With the STUN caveat (A1) |
| §35 | Identity ≠ transport | This is the core invariant |
| §40, §67, §68 | RAM only, no mailbox, offline means lost | Plus key zeroization and a note that the browser may still swap RAM to disk |
| §41 | Logging redaction rules | Plus: release builds contain no log calls at all (compile-time) |
| §42 | Full mesh with a cap on room size | The cap value is Q3 |
| §45 | Identity ≠ reachability | Correct and important |
| §47, §48 | New joiners get no history; leaving starts a new epoch | Comes with MLS |
| §49, §50, §89 | Security model and allowed/forbidden claims | Extended (A1, A3, A12) |
| §55–§57 | Diagnostics and a direct-only indicator | Specified from the `getStats()` fields |
| §69, §70 | Stable error codes; version field on every object | Codes are now numeric `u16`, and new codes were added |
| §85 | Staged MVPs | Order kept; content adjusted (resume code moves into MVP-1) |

---

## 4. Open questions for the owner

| # | Question | v0.2 default until answered |
|---|---|---|
| Q1 | Main exchange scenario: **in person** (two scans) or **remote** (links through a messenger)? This decides whether SAS verification is optional or required. | Both are supported. SAS is prompted for, but not required, when the exchange was remote. |
| Q2 | **STUN policy.** Which servers (Google, Cloudflare, your own)? On by default, or "LAN-only" by default with STUN opt-in? | On by default, with a user-editable list and a "LAN-only" privacy mode |
| Q3 | **Maximum room size** for MVP-3 | 8 (28 links) |
| Q4 | **Room authority.** Does only the owner admit and remove members, or may any member invite? What happens when the owner leaves? | The owner is the single MLS committer; the lowest connected leaf index takes over |
| Q5 | **Wallet.** Which chains (EVM/SIWE, Solana, others)? Is the WalletConnect relay acceptable on mobile? Are ERC-1271 smart wallets needed (which means RPC)? | EVM EOA only, SIWE (EIP-4361), no WalletConnect, no RPC |
| Q6 | **Peer forwarding** when two members cannot connect directly: allow it as an explicit "relayed via X" link, or never? | Never in MVP-3 |
| Q7 | **Hosting.** Custom domain, or `user.github.io/p2p-chat/`? Is IPFS mirroring wanted? | `github.io` project path; no IPFS |
| Q8 | **Browser support matrix.** Is iOS Safari required (this affects A15, A16, and the QR scanner fallback)? | Latest 2 versions of Chrome, Edge, Firefox and Safari, desktop and mobile |
