# P2P Ephemeral Chat — Rust/WASM Specification

| Field          | Value |
|----------------|-------|
| Version        | 0.2 |
| Status         | Architecture / Protocol Draft. Open questions in §25 |
| Supersedes     | v0.1. The rationale for every change is in [`REVIEW-v0.1.md`](REVIEW-v0.1.md) |
| Deployment     | Static host (GitHub Pages or equivalent) |
| Runtime        | Browser PWA |
| Implementation | Rust (edition 2024) → `wasm32-unknown-unknown` |
| Transport      | WebRTC DataChannel (SCTP / DTLS / ICE / UDP) |
| Signalling     | Two-way out-of-band exchange (QR, link, paste, share). **No signalling server** |
| Relay          | None. TURN is disabled locally and relay candidates are rejected from the peer |
| STUN           | Third-party, user-configurable. Needed in practice for any connection that is not on the same LAN (§9.3) |
| Persistence    | None (RAM only) |

Keywords **MUST**, **MUST NOT**, **SHOULD** and **MAY** are used as defined in RFC 2119.

---

## 1. Summary

A backend-free, ephemeral, end-to-end-encrypted peer-to-peer chat. It is shipped as static files and runs entirely in the browser. Two peers meet through **one out-of-band round trip**: an *invite* goes out and an *answer* comes back. Each is about 190 bytes and fits in one QR code or a short link. After that, all traffic flows over a direct WebRTC DataChannel protected by two layers:

- DTLS, pinned by the fingerprints exchanged out of band;
- an application Noise session, bound to the same out-of-band data.

There is no server, relay, database or history. Network paths can be thrown away and rebuilt. Identities (static keys) last for the whole session.

```
             STATIC HOST (code only, never sees traffic or invite fragments)
                              |
                              v
       +--------------------------------------------+
       |  Browser PWA: Rust/WASM core + thin JS boot |
       +--------------------------------------------+
            |  invite  (QR / link)  ------->  |
   Alice    |                                 |    Bob
            |  <-------  answer (QR / link)   |
            |                                 |
            |<====== WebRTC DataChannel =====>|   direct only
                 DTLS (fingerprint-pinned)
                 + Noise KK (key-pinned)
```

## 2. Design principles

| # | Principle |
|---|---|
| P1 | **No application backend.** No server that the application controls is needed at runtime. |
| P2 | **Direct communication.** Chat traffic goes only between peers. |
| P3 | **Out-of-band rendezvous.** One invite/answer round trip over QR, link, paste or share replaces a signalling server. |
| P4 | **Identity ≠ network location.** Identity is a static key, never an IP, port, candidate or connection. |
| P5 | **Network paths are disposable.** Paths are rebuilt; identity, room and sequence numbers survive. |
| P6 | **Encryption is mandatory and layered.** DTLS for transport, plus application E2E. |
| P7 | **No history.** RAM only, and keys are zeroized when the session ends. |
| P8 | **Failure is explicit.** No silent relay, whether a server or a peer. If there is no direct path, the application says so. |
| P9 | **Trust is explicit.** Security is never stronger than (a) the integrity of the out-of-band channel and (b) the code served by the static host. The UI and documentation MUST say so. |

## 3. Layers

```
+------------------------------------------------+
| 5 Application   rooms, members, messages, UI   |  Rust (core)       + JS (DOM only)
| 4 Crypto        Noise KK (1:1), MLS (groups),  |  Rust (crypto)
|                 SAS, wallet binding            |
| 3 P2P protocol  frames, sequencing, recovery,  |  Rust (proto, core)
|                 peer-relayed signalling        |
| 2 WebRTC        PC, DataChannel, ICE, stats    |  Browser, driven by Rust (wasm via web-sys)
| 1 Browser       camera, share, clipboard,      |  Browser
|                 BroadcastChannel, SW           |
+------------------------------------------------+
```

Rust owns layers 3–5 and **all** state: rooms, peers, crypto, sequencing, membership and connection state machines. JS never holds the authoritative copy of any state.

## 4. Infrastructure

### 4.1 Required

Only a static host is required:

```
<base>/index.html          (CSP in <meta>, see §17.3)
<base>/boot.js             (loads WASM, registers the SW; nothing else)
<base>/app_bg.wasm
<base>/app.js              (wasm-bindgen glue)
<base>/style.css
<base>/manifest.webmanifest
<base>/sw.js
<base>/icons/*
```

### 4.2 Optional third-party infrastructure

| Service | Purpose | What it learns | Carries chat? |
|---|---|---|---|
| STUN (user-configurable list) | Discover server-reflexive (srflx) and global IPv6 candidates | The public IP and port of each peer, and when they connect | **No** |
| EVM RPC (MVP-2, only if ERC-1271 is enabled, Q5) | Verify smart-contract wallet signatures | The wallet address being checked | **No** |

### 4.3 Forbidden

Application backend, WebSocket or HTTP signalling, TURN, chat relay, message database, Redis, Kafka, IPFS message storage, a central presence service, central authentication, analytics or telemetry, remote logging, and third-party scripts. IPFS MAY mirror the **static assets** only.

## 5. Trust base

| Party | What we trust it for | Mitigation |
|---|---|---|
| Out-of-band channel (in person, messenger, …) | Integrity of the invite and answer: they carry the DTLS fingerprints and static keys | SAS comparison (§10.4). In-person QR scanning is strongest. |
| Static host / repository owner | Serving honest code | CSP, SRI, a service worker that pins the version and asks before updating (§17), reproducible builds with published hashes |
| Browser and OS | Everything | Out of scope (§21) |
| STUN operator | Nothing about security. It learns metadata only | Configurable list, and a LAN-only mode |

## 6. Architecture: sans-IO core

### 6.1 Workspace

```
p2p-chat/
├── Cargo.toml                      # workspace; release: lto="fat", codegen-units=1, panic="abort", opt-level="s"|3
├── crates/
│   ├── proto/                      # #![no_std] — zero-copy codecs, no alloc
│   │   ├── invite.rs               # InviteBin / AnswerBin (§8.3) encode/decode over &[u8]
│   │   ├── candidate.rs            # CandidateBin (§8.4) + SDP a=candidate line render/parse
│   │   ├── sdp.rs                  # template-based SDP reconstruction (Appendix A)
│   │   ├── frame.rs                # outer frame header + inner records (§11)
│   │   ├── b64url.rs               # base64url codec into caller-provided buffers
│   │   └── error.rs                # ErrorCode (u16, §19)
│   ├── crypto/
│   │   ├── identity.rs             # X25519 static key, PeerId, display handle
│   │   ├── noise.rs                # Noise_KK session wrapper (snow), in-place encrypt/decrypt
│   │   ├── sas.rs                  # short authentication string from handshake hash
│   │   ├── wallet.rs               # (MVP-2) SIWE/EIP-191 binding + k256 ecrecover
│   │   └── mls.rs                  # (MVP-3) openmls group wrapper
│   ├── core/                       # pure deterministic state machines, no wasm-bindgen
│   │   ├── session.rs              # per-peer connection FSM (§12)
│   │   ├── recovery.rs             # recovery ladder T0–T3 (§13)
│   │   ├── room.rs                 # membership, dedup high-water marks (§14)
│   │   ├── sendq.rs                # fixed ring send queue + backpressure (§11.5)
│   │   └── io.rs                   # Input / Action enums, ActionSink (fixed capacity)
│   └── wasm/                       # the ONLY crate touching the browser
│       ├── lib.rs                  # #[wasm_bindgen] ChatApp facade
│       ├── rtc.rs                  # web-sys RTCPeerConnection / RTCDataChannel adapter
│       ├── stats.rs                # getStats → Diagnostics
│       ├── qr.rs                   # qrcode (encode) + BarcodeDetector / rqrr (decode)
│       ├── share.rs                # Web Share, clipboard, BroadcastChannel hand-off
│       └── wallet.rs               # (MVP-2) EIP-1193 provider adapter
├── web/                            # static assets (§4.1)
├── tests/                          # native replay/fuzz of proto + core
└── docs/spec/                      # this document
```

### 6.2 Core contract

```rust
pub enum Input<'a> {
    Tick,                                        // timer (driven by adapter, e.g. every 250 ms)
    UserCreateInvite, UserApplyCode(&'a [u8]),   // decoded invite/answer/resume bytes
    UserSend(&'a [u8]),                          // UTF-8 already in wasm rx/tx buffer
    RtcLocalDescription { conn: ConnId, ufrag: &'a [u8], pwd: &'a [u8], fp: &'a [u8; 32] },
    RtcLocalCandidate  { conn: ConnId, cand: CandidateBin },
    RtcGatheringComplete(ConnId),
    RtcIceState(ConnId, IceState), RtcChannelOpen(ConnId), RtcChannelClosed(ConnId),
    RtcFrame(ConnId, &'a mut [u8]),              // in-place decrypt
    RtcBufferedLow(ConnId),
    RtcSelectedPair(ConnId, PairInfo),           // from getStats
    NetChanged,
}

pub enum Action {
    CreatePc(ConnId, PcConfigId), SetRemote(ConnId, SdpRef), CreateAnswer(ConnId),
    RestartIce(ConnId), ClosePc(ConnId, ErrorCode),
    SendFrame(ConnId, BufRef),                   // BufRef = (offset,len) into linear memory
    ShowCode(CodeKind, BufRef), ShowSas([u8; 7]),
    Deliver(PeerIdx, BufRef), State(ConnId, SessionState), Error(ErrorCode),
    ArmTimer(u32 /*ms*/),
}

impl Core {
    pub fn handle(&mut self, now_ms: u64, input: Input<'_>, out: &mut ActionSink); // no alloc
}
```

- `ActionSink` is a fixed-capacity array (for example 32 slots). If it overflows, that is a bug, caught by `debug_assert!` and an abort.
- `core` builds and runs natively, so tests replay recorded input traces deterministically.

## 7. Identity

### 7.1 Model

```rust
#[repr(C)] #[derive(Copy, Clone)] pub struct PeerId(pub [u8; 32]);   // == X25519 static public key
#[repr(C)] #[derive(Copy, Clone)] pub struct RoomId(pub [u8; 16]);   // CSPRNG
#[repr(C)] #[derive(Copy, Clone)] pub struct InviteId(pub [u8; 16]); // CSPRNG
```

- The static key pair is generated when the app starts, stays in wasm memory only (never written to storage), and is zeroized on exit (`zeroize`).
- `PeerId` is the public key itself. It is not hashed, and there is no second key in MVP-1/2.
- **Display handle:** `anon_` plus the first 6 hex digits of `BLAKE2s(PeerId)`. It is **not authentication**. Nicknames are free text that users choose, and are only ever shown inside the encrypted channel.
- Rule: `PeerId ≠ IP ≠ port ≠ candidate ≠ RTCPeerConnection ≠ DTLS certificate`.

### 7.2 Wallet binding (MVP-2)

- The wallet signs a SIWE (EIP-4361) message. The statement binds `PeerId`, `RoomId` and an expiry, and `nonce = BLAKE2s(RoomId ‖ PeerId ‖ InviteId)[..16]` in hex.
- The binding is sent **only inside the Noise channel**. It is never placed in an invite.
- EOA signatures are verified offline (EIP-191 plus `k256` ecrecover). ERC-1271 needs RPC and is optional (Q5).
- The wallet key never signs network traffic. The static key stays the session key.
- The UI MUST warn that a wallet address links sessions together.

## 8. Rendezvous

### 8.1 The exchange is always two-way

Browser WebRTC cannot finish DTLS or ICE without the remote description (REVIEW R1). Every new pairwise link therefore needs exactly one **invite → answer** exchange. The first link is done out of band. Later links inside a room can be relayed through a peer (§14.3).

```
Alice (offerer)                                   Bob (answerer)
  create PC + negotiated DC(id=0)
  createOffer / setLocalDescription
  gather ICE (≤ 3 s, §9.5)
  InviteBin ──── QR / link / paste / share ──────▶ parse, validate
                                                  rebuild offer SDP (App. A)
                                                  create PC, setRemote, createAnswer
                                                  gather ICE (≤ 3 s)
  parse, verify invite_id, not expired ◀── QR / link / paste ── AnswerBin
  rebuild answer SDP, setRemote
  ───────────── ICE checks → DTLS (fingerprints pinned) → SCTP → DC open ─────────────
  Noise KK handshake (prologue = invite ‖ answer)
  SAS shown on both sides → CONNECTED
```

### 8.2 Carriers

| Scenario | Invite | Answer |
|---|---|---|
| In person | Alice shows a QR, Bob scans it in the app | Bob shows a QR, Alice scans it in the app |
| Remote | Link sent through the system share sheet or a messenger | Bob shares a link back. Alice taps it (the new tab forwards it to the owning tab through `BroadcastChannel`, §8.7) or pastes it |
| Same device (testing) | Clipboard | Clipboard |

### 8.3 Binary layout (little-endian, `#[repr(C)]`-compatible, parsed in place)

**Common header (4 bytes)**

| Off | Size | Field | Notes |
|---|---|---|---|
| 0 | 1 | `ver` | Protocol major version = `1` |
| 1 | 1 | `kind` | `1` = INVITE, `2` = ANSWER, `3` = RESUME_INVITE, `4` = RESUME_ANSWER |
| 2 | 1 | `flags` | bit0 `LAN_ONLY`, bit1 `WALLET_REQUIRED` (MVP-2), bit2 `GROUP` (MVP-3). Other bits MUST be 0 |
| 3 | 1 | `n_cand` | 0..=8 |

**INVITE / RESUME_INVITE body**

| Size | Field |
|---|---|
| 16 | `invite_id` |
| 16 | `room_id` |
| 32 | `static_pk` (X25519) |
| 4 | `expires_at` (u32, Unix seconds) |
| 1 + n | `ufrag_len` (4..=32), `ufrag` (ICE chars) |
| 1 + n | `pwd_len` (22..=32), `pwd` |
| 32 | `dtls_fp` (SHA-256 of the DTLS certificate) |
| var | `n_cand` × `CandidateBin` (§8.4) |

**ANSWER / RESUME_ANSWER body:** `invite_id` (echoed back, 16), `static_pk` (32), `ufrag`, `pwd`, `dtls_fp`, candidates. There is no room ID and no expiry.

Rules:

- Nothing in these messages is secret in the key sense. There are no private keys and no wallet data. The messages are still **sensitive bootstrap material**: whoever answers first wins the link (§8.6).
- The invite is not signed. A self-signature adds nothing, because authenticity comes from the out-of-band channel and the SAS.
- Parsers MUST reject any length outside the ranges above, unknown `ver`, unknown `kind`, non-zero reserved flag bits, and trailing bytes (`E_INVALID_INVITE`).

### 8.4 CandidateBin

Only UDP candidates with component 1 are carried.

| Size | Field |
|---|---|
| 1 | `tag`: `0` host-v4, `1` host-v6, `2` host-mDNS, `3` srflx-v4, `4` srflx-v6 |
| 4 / 16 / 16 | address (IPv4, IPv6, or the mDNS UUID as 16 raw bytes) |
| 2 | `port` |

- Sizes: 7 bytes (v4) or 19 bytes (v6 or mDNS).
- The receiver rebuilds each `a=candidate` line. It derives the foundation from `tag`, computes the priority with the RFC 8445 formula (type preference host 126, srflx 100), and uses `raddr 0.0.0.0 rport 0`.
- `relay`, TCP and `prflx` candidates are **never** encoded.

### 8.5 Encoding and size

| Stage | Bytes |
|---|---|
| Header + ids + key + expiry | 4 + 16 + 16 + 32 + 4 = 72 |
| ufrag + pwd (Firefox worst case, 8 + 32) | 2 + 40 = 42 |
| DTLS fingerprint | 32 |
| Candidates (1 mDNS + 1 srflx-v4 + 1 srflx-v6) | 45 |
| **Total** | **≈ 191 B** |
| base64url, no padding | ≈ 255 characters |
| Full link `https://<host>/p2p-chat/#i=` + payload | ≈ 290 characters, which is a QR of about version 12-M (byte mode) |

- **No compression**: the payload is high-entropy.
- **No multi-frame QR**: the worst case (8 IPv6 candidates) is about 300 B, which is about v15-M and still scans easily.
- Fragment keys: `#i=` invite, `#a=` answer, `#r=` resume invite, `#q=` resume answer.
- *Optional optimization*: uppercase Base32 in QR alphanumeric mode saves about one QR version. It needs a URL that is valid in uppercase, which means a custom domain (Q7). It is not in the MVP.

### 8.6 Lifetime, single use and races

- `expires_at = now + TTL`. The TTL defaults to 5 min and can be set from 1 to 30 min.
- **Alice enforces** the expiry: she rejects an answer after `expires_at` with `E_EXPIRED_INVITE` and closes the connection. Bob's check is advisory, with ±120 s tolerance for clock skew.
- An offer belongs to exactly one `RTCPeerConnection`, so **each invite is single-use by construction**. A second answer for a consumed `invite_id` gets `E_INVITE_CONSUMED`. To invite N people, Alice makes N invites (or uses §14.3).
- Race: if a third party answers first, Alice connects to them. The SAS (§10.4) and the display of who is connected catch this.

### 8.7 URL and fragment hygiene

- Codes travel **only in the URL fragment**, which browsers never send to the host.
- On load, the app reads the fragment, runs `history.replaceState(null, '', '<base>/')`, and passes the bytes to the core.
- If a tab opened from a link is not the owning tab, it posts the code on `BroadcastChannel('p2pchat-codes')`. It closes itself if the owning tab acknowledges within 500 ms. Otherwise it offers paste or scan.
- A clipboard write is followed by a best-effort clear of the clipboard 60 s later.

## 9. WebRTC configuration

### 9.1 RTCPeerConnection

```js
{
  iceServers: [{ urls: STUN_LIST }],   // never contains turn: / turns:, asserted in Rust before the call
  iceTransportPolicy: 'all',
  bundlePolicy: 'max-bundle',
  rtcpMuxPolicy: 'require',
  iceCandidatePoolSize: 0,
  certificates: [ await RTCPeerConnection.generateCertificate({ name: 'ECDSA', namedCurve: 'P-256' }) ]
}
```

- **DataChannel:** `createDataChannel('c', { negotiated: true, id: 0, ordered: true })`. It is reliable and ordered, and it needs no in-band DCEP round trip.
- `binaryType = 'arraybuffer'`.
- A fresh certificate is made for every connection. Continuity of identity comes from the Noise static keys, not from DTLS.

### 9.2 Relay prohibition (defence in depth)

1. `iceServers` MUST NOT contain TURN URLs. Rust validates this and aborts if it finds one.
2. Remote candidates of type `relay`, and any candidate over TCP, are **dropped before they are applied**.
3. After `connected`, and again after every selected-pair change, `getStats()` is read. If `candidateType` is `relay` on either side of the selected pair, the connection is closed with `E_RELAY_REJECTED`.
4. **Limit:** candidate types are self-declared, so a peer can route through its own relay or VPN behind a `host` or `srflx` label. We guarantee that *we* introduce no relay, not that the peer's path is relay-free.

### 9.3 STUN and mDNS reality

- Browsers hide host IPs behind mDNS (`<uuid>.local`). Without STUN, connectivity works **only on the same local link**, and that includes public IPv6 hosts.
- A direct path over IPv6 across networks needs a **dual-stack STUN server** (one with an AAAA record). The srflx-v6 candidate it returns is the global address.
- Default STUN list: see Q2. The list is editable in settings.
- **LAN-only mode**: no STUN, only mDNS host candidates, and the `LAN_ONLY` flag is set in the invite.

### 9.4 Candidate privacy modes

| Mode | Candidates placed in the invite or answer |
|---|---|
| `lan-only` | host-mDNS only |
| `default` | host-mDNS, srflx-v4, srflx-v6 |
| `max-connectivity` | adds raw host-v4 and host-v6 when the browser exposes them. After camera permission is granted for the QR scanner, Chromium may expose them *(spike S4)* |

The builder filters by mode **regardless of what the browser exposes**.

### 9.5 Gathering for a code

- Codes are sent without trickle ICE: we wait for `icegatheringstate === 'complete'`, or for 1 500 ms after the first srflx candidate, with a hard cap of 3 000 ms.
- Candidates that arrive late are ignored for the code. They may still be used later through T1 (§13).

## 10. Application cryptography

### 10.1 1:1 session: Noise KK

- Protocol: `Noise_KK_25519_ChaChaPoly_BLAKE2s`, using the `snow` crate with RustCrypto backends.
- KK fits because both static keys are already known from the invite and the answer. The handshake is 1 round trip:
  - `→ e, es, ss`
  - `← e, ee, se`
- **Prologue** = `"p2pchat/1" ‖ invite_bytes ‖ answer_bytes`. These are exactly the bytes that were exchanged, which binds the session to the same out-of-band data that pins both DTLS fingerprints.
- **Initiator** = the offerer (Alice). On resume (§13 T3), the initiator is whoever made the resume invite.
- **Transport:** Noise CipherState. The nonce is an implicit 64-bit counter. The DataChannel is ordered and reliable, so the receiver expects exactly `n+1`. Anything else causes `E_CRYPTO_FAILED` and closes the connection.
- **Rekey:** `REKEY` (Noise `rekey()`) every 2^20 messages or 10 minutes, whichever comes first.

### 10.2 Why two layers

- DTLS protects the transport.
- Noise gives: (a) identity that continues across new `RTCPeerConnection`s and DTLS certificates; (b) authentication that does not depend on the transport, for peer-relayed signalling (§14.3) and future transports; (c) a clear, pinned root for the SAS.
- For chat, the cost of double encryption is negligible.

### 10.3 Groups (MVP-3)

MLS (RFC 9420) via `openmls`, compiled for `wasm32`. See §14.4 for ordering.

### 10.4 Short authentication string (SAS)

- `s = BLAKE2s("p2pchat-sas" ‖ handshake_hash)`. It is shown two ways:
  - **6 decimal digits** = `u32::from_le_bytes([s[0], s[1], s[2], 0]) % 10^6` (24 bits, negligible bias);
  - **4 emoji**, one per byte of `s[3..7]`, from a fixed 256-entry table.
- It is shown on both devices after the handshake.
- **Remote exchange:** the UI prompts the users to compare the SAS on a *different* channel, such as a voice call. A user can mark the peer as **verified**. Unverified peers carry a persistent badge.
- **In-person QR:** the SAS is optional, because the out-of-band channel is physically authenticated.

### 10.5 Primitives and randomness

- Use RustCrypto crates only: `x25519-dalek`, `chacha20poly1305`, `blake2`, `zeroize`, `k256` (MVP-2). **No hand-written cryptographic primitives.**
- The CSPRNG is `getrandom` with the `wasm_js` backend, which calls `crypto.getRandomValues`.
- WebCrypto is **not** on the protocol path: it is async-only and would split state between JS and Rust.

## 11. Wire protocol (inside the DataChannel)

### 11.1 Outer frame (12-byte header, authenticated as AAD)

| Off | Size | Field |
|---|---|---|
| 0 | 1 | `ver` = 1 |
| 1 | 1 | `ftype`: `1` NOISE_HS, `2` NOISE_TRANSPORT, `3` MLS (MVP-3) |
| 2 | 2 | `flags` (reserved, must be 0) |
| 4 | 8 | `seq`: the transport counter. It must equal the Noise nonce and is carried for diagnostics and assertions |
| 12 | … | ciphertext, plus a 16-byte Poly1305 tag |

- DataChannel messages keep their boundaries, so there is no length field.
- Frame size limit: **16 384 B**.

### 11.2 Inner records (plaintext, inside the ciphertext)

| Off | Size | Field |
|---|---|---|
| 0 | 1 | `rtype` |
| 1 | 1 | `rflags` |
| 2 | 2 | `len` |
| 4 | len | body |

| `rtype` | Name | Body | Phase |
|---|---|---|---|
| 0x01 | HELLO | ver_min u8, ver_max u8, caps u32 bitset, max_msg u16, nick_len u8, nick | MVP-1 |
| 0x02 | CHAT | chat_seq u64, UTF-8 text (≤ 4 096 B) | MVP-1 |
| 0x03 | ACK | chat_seq u64 (cumulative) | MVP-1 |
| 0x04 | PING / 0x05 PONG | t_ms u64 | MVP-1 |
| 0x06 | GOODBYE | ErrorCode u16 | MVP-1 |
| 0x07 | REKEY | – | MVP-1 |
| 0x10 | SIGNAL_OFFER / 0x11 SIGNAL_ANSWER | target conn / peer, AnswerBin-shaped body | MVP-2 (T1), MVP-3 (§14.3) |
| 0x20 | WALLET_BIND | SIWE message + 65-byte signature | MVP-2 |
| 0x30… | ROOM_* / MLS_* | defined in MVP-3 | MVP-3 |

Unknown `rtype` values are ignored if `rflags.bit0` (IGNORABLE) is set. Otherwise they cause `E_PROTOCOL_MISMATCH`.

### 11.3 Sequencing and deduplication

- The transport replay check is the Noise nonce (§10.1). There is no set of seen IDs.
- `chat_seq` is a u64 per sender that **never resets** for the whole session, including across reconnects.
- A receiver keeps `last_chat_seq[peer_idx]`. A CHAT with `chat_seq ≤ last` is a duplicate (a resend after a reconnect) and is dropped.
- The sender keeps unacknowledged CHATs in a fixed ring of 256 entries and **resends** them after T1, T2 or T3 recovery.
- `MessageId` = `(PeerIdx u8, chat_seq u64)`. `PeerIdx` is an index into the room's member table, which is fixed-size.

### 11.4 Capabilities and versioning

- Every code and frame carries `ver`.
- If majors differ, the connection is closed with `E_PROTOCOL_MISMATCH`.
- HELLO carries `ver_min..ver_max` and a capability bitset: bit0 wallet, bit1 group, bit2 resume, and so on. The session uses the intersection.

### 11.5 Backpressure

- Each connection has a fixed ring send queue of 64 frames × 16 KiB, preallocated in linear memory.
- `bufferedAmountLowThreshold` = 64 KiB. Draining stops while `bufferedAmount` > 256 KiB and resumes on `bufferedamountlow`.
- If the queue is full, `UserSend` returns `E_BACKPRESSURE`. The queue **never grows**.
- Files are out of scope. A future transfer protocol will use its own DataChannel with chunking.

### 11.6 Memory copies (zero-copy policy and documented exceptions)

| Direction | Step | Copy? | Justification |
|---|---|---|---|
| RX | SCTP → JS `ArrayBuffer` | Browser-internal | Not under our control |
| RX | `ArrayBuffer` → preallocated wasm RX slot | **1 copy** | Unavoidable: WASM cannot address a JS `ArrayBuffer` |
| RX | Decrypt | 0 | In place (`decrypt_in_place_detached`) |
| RX | Parse records | 0 | `&[u8]` views over the slot |
| RX | UTF-8 → DOM text | **1 transcode** | Unavoidable: the DOM uses UTF-16 JS strings (`TextDecoder` over a wasm memory view) |
| TX | JS string → wasm TX slot | **1 transcode** | Unavoidable: `TextEncoder.encodeInto` writes directly into a wasm memory view |
| TX | Encrypt | 0 | In place |
| TX | `send(Uint8Array view of wasm memory)` | 0 on our side | The browser copies into SCTP internally. The view is created right before `send`, and memory never grows after init, so the view stays valid |

## 12. Connection state machine (per pairwise link)

```
Offerer:  NEW → GATHERING → INVITE_READY → AWAITING_ANSWER ─┐
Answerer: NEW → CODE_PARSED → GATHERING → ANSWER_READY ─────┤
                                                             v
                      CONNECTING (ICE + DTLS) ──fail──▶ FAILED(E_NO_DIRECT_PATH | E_ICE_FAILED)
                                                             │
                                                             v
                      CHANNEL_OPEN → HANDSHAKING (Noise KK) ──fail──▶ FAILED(E_CRYPTO_FAILED)
                                                             │
                                                             v
                                                        CONNECTED  (SAS shown; verified flag optional)
                                                             │ ice 'disconnected' | NetChanged | ping miss
                                                             v
                                                        DEGRADED ──T0 success──▶ CONNECTED
                                                             │
                                                             v
                                                        RECOVERING(T1|T2) ──success──▶ CONNECTED (new PC, Noise re-handshake, resend)
                                                             │ exhausted
                                                             v
                                                        SUSPENDED (room state kept, grace 10 min) ──resume code (T3)──▶ CONNECTING
                                                             │ grace expired | user leaves
                                                             v
                                                        CLOSED (keys zeroized)
```

- `AWAITING_ANSWER` times out at `expires_at` with `E_EXPIRED_INVITE`. There is **no retry before the first connection** (REVIEW R7).
- **Liveness:** ICE consent freshness (RFC 7675) is the authority on the path. The application PING is sent every 15 s when idle, and 2 missed PONGs lead to `DEGRADED`. This catches frozen or backgrounded tabs.
- **Retry backoff** in `RECOVERING`: 1 s, 2 s, 4 s, 8 s, capped at 15 s, with ±20 % jitter.

## 13. Network migration: the recovery ladder

Honest baseline: when a device's **only** network changes, all of its links drop at once. No in-band channel survives to carry an ICE restart.

| Tier | Mechanism | Signalling | Preconditions | Phase |
|---|---|---|---|---|
| **T0** | The ICE agent switches to an already-validated backup pair, or continual gathering finds a peer-reflexive path | None | A second interface was gathered at connect time, or the stationary peer is directly reachable *(spike S3)* | Free (browser behaviour) |
| **T1** | ICE restart. SIGNAL_OFFER/ANSWER travel over the **still-open** DataChannel, encrypted by Noise | In-band | The path is `disconnected` but not yet `failed`, or `NetChanged` fired before the path died | MVP-2 |
| **T2** | ICE restart relayed **through another room member**, end-to-end encrypted (§14.3) | Peer-relayed | Group, with a partial break | MVP-3 |
| **T3** | **Resume code**: RESUME_INVITE / RESUME_ANSWER exchanged out of band. Noise KK with the **same static keys** rebinds the session automatically (no room join, `chat_seq` continues, unacknowledged messages are resent) | Out of band | Always | MVP-1 |

- Glare during T1 or T2 (both sides restart at once) is handled with the *perfect negotiation* pattern. The **polite** peer is the one whose `PeerId` is lexicographically greater.
- UI copy:
  - T0–T2: "Reconnecting…"
  - T3: "Direct path lost — share a reconnect code."
  - The words "backup server" MUST NOT appear.

## 14. Groups (MVP-3)

### 14.1 Topology

- Full mesh. The member cap is Q3 (default 8, which means 28 links).
- Each link is an independent pairwise session as in §8–§13.

### 14.2 Propagation

- The sender sends each message **directly** to every connected member. There is **no flooding and no forwarding** (REVIEW R6).
- Deduplication uses `last_chat_seq[leaf_idx]`, a fixed array of length equal to the cap.
- A member without a direct link to the sender does not receive the message. The UI shows that link as missing (Q6).

### 14.3 Peer-relayed signalling (introductions)

1. A new member M joins through an out-of-band exchange with any member X.
2. X sends M's `PeerId` and the room state to the other members Y over the existing encrypted links. X vouches for M through an MLS Add, committed by the owner.
3. For each Y, M and Y exchange SIGNAL_OFFER/ANSWER **through X**. The body is sealed M↔Y with the MLS exporter secret, so X forwards only ciphertext.
4. The M↔Y link comes up directly. If ICE fails, that pair stays unlinked. **Chat is never relayed** unless Q6 allows it.

### 14.4 Group key management

- MLS (RFC 9420). The credential is `PeerId` plus the optional wallet binding.
- **Single committer:** the room owner (the creator). Other members send Proposals only, and the owner commits them. This avoids forked epochs without a delivery service.
- **Owner succession:** if the owner is gone for longer than the grace period, the connected member with the lowest leaf index takes over. **Limit:** a network split can produce two owners; the MVP resolves this by requiring a re-invite.
- A new joiner cannot read earlier epochs. Leaving or removal leads to a Commit and a new epoch.

## 15. Wallet authentication (MVP-2)

- Provider: EIP-1193 injected provider, which exists in desktop extensions and in wallets' own in-app browsers. WalletConnect is excluded (it relies on a relay server, Q5).
- Flow:
  1. `personal_sign(SIWE message, §7.2)`.
  2. Send WALLET_BIND in-band.
  3. The peer checks it with `k256` ecrecover: the address matches, the domain equals the app origin, and the nonce and expiry are valid.
- The UI shows "wallet-verified: 0xAB…CD" next to the peer. The wallet address is never logged.

## 16. Persistence, logging and memory hygiene

- **Never written:** messages, keys or codes to localStorage, IndexedDB, Cache Storage, a service-worker cache, the clipboard (beyond the moment of sharing, §8.7), IPFS, a blockchain, a server or analytics.
- **Settings MAY be stored** in localStorage: STUN list, privacy mode, TTL, nickname. They are not secret.
- All secrets live in wasm linear memory and are zeroized on `CLOSED` and on `pagehide`.
- **Limit:** chat text shown in the DOM exists as JS strings that cannot be zeroized, and the OS may swap RAM to disk. We do not claim otherwise.
- **Logging:** only when `#[cfg(debug_assertions)]`. Release builds contain **no log calls at all**, removed at compile time. Debug logs may contain `PeerIdx`, states, candidate *types*, RTT, byte counts and error codes. They never contain plaintext, keys, codes, signatures or IP addresses.

## 17. PWA, service worker and CSP

### 17.1 Service worker

- It caches **only** the static assets listed in §4.1.
- It never handles, stores or forwards messages or codes.
- Updates:
  1. A new `sw.js` installs and **waits**.
  2. The UI shows the new build hash and asks the user before activating it.
  3. It activates only on user consent.

### 17.2 Integrity

- `boot.js` loads `app.js` and `app_bg.wasm` with SRI hashes.
- Builds are reproducible (pinned toolchain, `--remap-path-prefix`). The hash of each build is published in GitHub Releases and shown in the app's About screen.

### 17.3 CSP (set in a `<meta>` tag, because GitHub Pages cannot set headers)

```
default-src 'none'; script-src 'self' 'wasm-unsafe-eval'; style-src 'self';
img-src 'self' data: blob:; connect-src 'self'; worker-src 'self';
manifest-src 'self'; media-src 'self' blob:; base-uri 'none'; form-action 'none'
```

- `connect-src 'self'` stops `fetch` or XHR from sending data anywhere. MVP-2 adds the RPC origin only if Q5 needs it.
- **Limit:** browsers do not reliably enforce the CSP `webrtc` directive, so peer connections cannot be restricted by CSP.

### 17.4 Mobile lifecycle

- A connection may survive, be suspended, or be killed in the background. A killed connection goes to `SUSPENDED` / T3.
- Messages sent while a peer is fully offline are lost: there is no mailbox, and the UI says so ("Messages are delivered only while peers are connected").
- **Risk (spike S6):** iOS may kill the pending peer connection while Alice switches to a messenger to share an invite. Mitigations: use the system share sheet, and regenerate the invite automatically when the app returns to the foreground if the connection has died.

## 18. Diagnostics UI

All of the following come from `getStats()` (selected candidate pair, local and remote candidate, transport) and from core state:

```
DIRECT P2P  ● verified (SAS 482 913)
Path        IPv6 · srflx ↔ host        Relay: NO (checked 12:04:31)
RTT         21 ms (ICE)  · 23 ms (app) Loss: n/a
Bytes       ↑ 18.2 KB  ↓ 22.9 KB       Buffered: 0 B
Transport   healthy (ICE connected, DTLS connected, DC open)
Crypto      healthy (Noise KK, epoch 0, rekey in 7:12)
Peer        authenticated (static key pinned by invite)
Members     4 / 8  (links 5 / 6)
```

- If the direct path fails: **DIRECT CONNECTION UNAVAILABLE**, the error code, and "share a reconnect code".
- The UI never shows raw IP addresses unless "show addresses" is turned on in the diagnostics view.

## 19. Error codes (stable, `u16`)

| Code | Name | Meaning |
|---|---|---|
| 0x0001 | E_INVALID_INVITE | The code failed to parse or validate |
| 0x0002 | E_EXPIRED_INVITE | Alice received the answer after `expires_at` |
| 0x0003 | E_INVITE_CONSUMED | A second answer for the same invite |
| 0x0004 | E_ANSWER_MISMATCH | The answer's `invite_id` is unknown or belongs to another connection |
| 0x0010 | E_INVALID_ROOM | |
| 0x0011 | E_ROOM_FULL | |
| 0x0020 | E_AUTH_FAILED | Wallet binding invalid, or static key mismatch |
| 0x0021 | E_CRYPTO_FAILED | Noise or MLS failure, nonce gap, or bad tag |
| 0x0022 | E_SAS_REJECTED | The user marked the SAS as a mismatch |
| 0x0030 | E_ICE_FAILED | |
| 0x0031 | E_NO_DIRECT_PATH | No non-relay pair succeeded |
| 0x0032 | E_RELAY_REJECTED | A relay candidate was selected or offered |
| 0x0033 | E_CONNECTION_TIMEOUT | |
| 0x0034 | E_NETWORK_CHANGED | |
| 0x0035 | E_PEER_OFFLINE | |
| 0x0040 | E_PROTOCOL_MISMATCH | |
| 0x0041 | E_MESSAGE_TOO_LARGE | |
| 0x0042 | E_BACKPRESSURE | |
| 0x0050 | E_BROWSER_UNSUPPORTED | No `RTCPeerConnection`, WASM or `crypto.getRandomValues` |

## 20. Limits (defaults)

| Item | Value |
|---|---|
| Invite TTL | 5 min (1–30) |
| Candidates per code | ≤ 8 |
| Chat text | ≤ 4 096 B UTF-8 |
| Frame | ≤ 16 384 B |
| Send queue | 64 frames per connection |
| Unacknowledged resend ring | 256 messages per peer |
| Room size | 8 (Q3) |
| App ping interval | 15 s idle |
| Suspended grace | 10 min |
| Rekey | 2^20 messages or 10 min |

## 21. Security model

**Protected:**

- message confidentiality and integrity against network observers and against the static host;
- peer authentication, pinned to the out-of-band exchange and optionally SAS-verified;
- room membership (MLS, MVP-3);
- no central storage or relay;
- no traffic replay across sessions.

**Not protected:**

- peer IP anonymity (the peer, and the STUN operator, see your public IP);
- traffic analysis and DPI detection (WebRTC is easy to fingerprint; no obfuscation is attempted);
- a compromised browser, extension, OS or device;
- a malicious static host serving altered code (mitigated, not eliminated: §17.2);
- a swapped code on a remote out-of-band channel when the SAS is not compared;
- a peer secretly routing its own side through a relay or VPN;
- delivery to offline peers.

**Attackers considered:**

| Attacker | Capability | Mitigation |
|---|---|---|
| Network observer | IPs, timing, sizes, volume | DTLS + Noise hide the contents. Metadata is accepted as exposed |
| Out-of-band channel MITM (messenger) | Swaps invite and answer | SAS comparison on another channel |
| Malicious peer | Fake identity, injection, replay, joining, abuse of membership, relay candidates | Keys pinned by the invite, AEAD with strict nonces, owner-committed MLS, relay filtering |
| Static host | Serves altered code | SRI, a service worker that pins the version and asks before updating, reproducible builds |
| STUN operator | Learns IPs and timing | Configurable list, LAN-only mode |
| Browser extension or device | Full access | Out of scope |

**Allowed claims:**

- "Messages are encrypted in transit (DTLS) and end-to-end at the application layer."
- "Chat traffic travels directly between peers; the app uses no relay and rejects relay paths."
- "There is no server-side or app-side chat history."
- "Your identity is independent of your network connection."
- "Authenticity depends on how you exchanged codes. Compare the safety code when you did not meet in person."

**Forbidden claims:**

- "anonymous"
- "untraceable"
- "invisible to DPI"
- "your IP is hidden"
- "works behind every NAT or firewall"
- "no one but the peers is involved" (STUN and the code host are involved)

## 22. Rust implementation rules (browser mapping of the HFT doctrine)

| Rule | Application here |
|---|---|
| No allocation after init | Rust core: all buffers (RX/TX slots, send rings, resend rings, member tables) are allocated once in `ChatApp::new`, and linear memory never grows afterwards. Allocations inside `web-sys` and `wasm-bindgen-futures` at the JS boundary happen on the control plane (signalling and stats) and are **documented exceptions** |
| POD `#[repr(C)]`, `Copy`, no heap fields in protocol types | Yes (§7.1, §8.3, §11.1) |
| No `dyn`; generics | Yes. The `core` ↔ `wasm` boundary is the `Input` / `Action` enums, not trait objects |
| No iterators or bounds checks in hot loops | Applies to frame parse, encrypt/decrypt and dedup (index loops, `get_unchecked` behind `debug_assert!`) |
| Fail fast; `panic = "abort"` | Yes. A protocol violation closes the connection with an error code. An internal invariant violation aborts the WASM instance, and the UI shows "internal error — reload" |
| No logging in release | Yes (§16) |
| Zero-copy networking | §11.6 lists the unavoidable copies |
| SIMD | `-C target-feature=+simd128`, supported by all target browsers. **AVX2 is not available in WASM** |
| Thread pinning, NUMA, lock-free SPSC across threads | **Not applicable.** WASM here is single-threaded: Pages cannot send COOP/COEP headers, so there is no `SharedArrayBuffer`. The design is single-writer by construction |
| Raw sockets, io_uring, RDMA, kernel bypass | **Not applicable** in a browser sandbox |
| crossbeam bounded channels | **Not applicable** (single thread). Fixed rings in `core` instead |
| Build profile | `lto = "fat"`, `codegen-units = 1`, `panic = "abort"`; `wasm-opt -O3` (or `-Oz` if binary size wins, decided by spike S5) |

## 23. Delivery plan

| Phase | Scope |
|---|---|
| **MVP-1** | 1:1; two-way exchange by QR, link and paste; binary codes with SDP reconstruction; configurable STUN and privacy modes; relay prohibition; Noise KK; SAS; T3 resume code; RAM only; basic diagnostics; CSP, SRI and a version-pinned service worker |
| **MVP-2** | T1 in-band ICE restart and `NetChanged` handling; full diagnostics; wallet binding (EVM EOA, SIWE) |
| **MVP-3** | Rooms up to the cap; peer-relayed signalling; MLS with a single committer; T2 recovery |
| **Later** | File transfer (separate channel, chunking), voice and video, larger rooms and alternative topologies |

## 24. Validation spikes (must finish before the design they gate is frozen)

| ID | Question | Gates |
|---|---|---|
| S1 | Does the Appendix A template SDP work as a remote description in Chrome, Firefox and Safari, desktop and mobile, in all 9 offerer/answerer combinations? | §8 |
| S2 | Measured code sizes (ufrag and pwd lengths, candidate counts) per browser | §8.5 |
| S3 | T0 behaviour per browser: continual gathering, backup pairs, recovery through a peer-reflexive path | §13 |
| S4 | Does camera permission disable mDNS obfuscation, and does that persist after the camera track stops? | §9.4 |
| S5 | Size of `snow`, and later `openmls`, on `wasm32`; `-O3` vs `-Oz` | §22 |
| S6 | iOS lifetime of a pending `RTCPeerConnection` while in the background during sharing | §17.4 |
| S7 | `BroadcastChannel` hand-off between a Safari tab and an installed PWA | §8.7 |

## 25. Open questions

Recorded with their current defaults in [`REVIEW-v0.1.md` §4](REVIEW-v0.1.md#4-open-questions-for-the-owner): Q1 exchange scenario and whether the SAS is required, Q2 STUN policy, Q3 room cap, Q4 room authority, Q5 wallet scope, Q6 peer forwarding, Q7 hosting and domain, Q8 browser matrix.

## 26. Out of scope

- A custom NAT traversal, DTLS, WebRTC or cryptographic algorithm.
- A DHT.
- Blockchain or IPFS message storage.
- Server-side signalling.
- TURN fallback.
- Persistent history.
- Multi-frame QR.
- Single-QR bootstrap (impossible, REVIEW R1).
- Traffic obfuscation.

---

## Appendix A — SDP reconstruction template

The receiver renders the remote description from a `&'static str` template, filling fields into a preallocated buffer. `{role}` is `actpass` for an offer and `active` for an answer.

```
v=0
o=- {sess_id_u64} 2 IN IP4 127.0.0.1
s=-
t=0 0
a=group:BUNDLE 0
a=extmap-allow-mixed
a=msid-semantic: WMS
m=application 9 UDP/DTLS/SCTP webrtc-datachannel
c=IN IP4 0.0.0.0
a=ice-ufrag:{ufrag}
a=ice-pwd:{pwd}
a=ice-options:trickle
a=fingerprint:sha-256 {fp_hex_colon}
a=setup:{role}
a=mid:0
a=sctp-port:5000
a=max-message-size:262144
{candidate_lines}
a=end-of-candidates
```

Candidate line: `a=candidate:{foundation} 1 udp {priority} {addr} {port} typ {host|srflx}[ raddr 0.0.0.0 rport 0]`

- An mDNS `addr` is rendered as the lowercase hyphenated UUID followed by `.local`.
- `sess_id_u64` is taken from the first 8 bytes of `invite_id`.

## Appendix B — End-to-end flows

**Create and invite (Alice):**

1. The app starts and generates the static key.
2. Create the room (`RoomId`).
3. Create the peer connection and the negotiated DataChannel.
4. `createOffer` and `setLocalDescription`.
5. Gather (≤ 3 s) and filter by privacy mode.
6. Build the InviteBin and show it as a QR or link.
7. Wait for the answer, until the TTL runs out.

**Join (Bob):**

1. Scan or open the link. The fragment is read and stripped.
2. Parse and validate: version, lengths, advisory expiry.
3. Generate the static key.
4. Create the peer connection and the negotiated DataChannel.
5. Rebuild the offer SDP, then `setRemoteDescription` and `createAnswer`.
6. Gather and filter.
7. Build the AnswerBin and show it as a QR or link.

**Complete:**

1. Alice applies the answer: checks `invite_id`, checks the TTL, rebuilds the answer SDP and calls `setRemoteDescription`.
2. ICE checks run, then DTLS with pinned fingerprints, then the DataChannel opens.
3. Noise KK handshake.
4. The relay check runs through `getStats`.
5. HELLO is exchanged.
6. The SAS is shown, and the link is **CONNECTED**.

**Wi-Fi change:**

1. The link is `CONNECTED`, then `disconnected` or `NetChanged`, and moves to `DEGRADED`.
2. T0 is tried. If the DataChannel is still alive, T1 (MVP-2). In a group, T2 (MVP-3).
3. Otherwise the link moves to `SUSPENDED`, and a resume code is shared (T3).
4. The link returns to `CONNECTED` with the same `PeerId`, `RoomId` and `chat_seq`, and unacknowledged messages are resent.
