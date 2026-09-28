# P2P Ephemeral Chat — Rust/WASM Specification

| Field          | Value |
|----------------|-------|
| Version        | 0.6 |
| Status         | Architecture / Protocol Draft. Owner decisions up to v0.6 applied (§25). **Only our WASM app is built: no native companion programs** (§2 P10). Spike results: [`../spikes/RESULTS-2026-09-28.md`](../spikes/RESULTS-2026-09-28.md) |
| Supersedes     | v0.5 (companion removed; Tor and hidden channel owners are WASM-only; spike results applied), v0.4, v0.3, v0.2, v0.1 (rationale in [`REVIEW-v0.1.md`](REVIEW-v0.1.md)) |
| Deployment     | GitHub Pages, project site `https://<owner>.github.io/p2p-chat/` |
| Runtime        | Browser PWA |
| Implementation | Rust (edition 2024) → `wasm32-unknown-unknown` |
| Transport      | **Direct mode:** WebRTC DataChannel (SCTP / DTLS / ICE / UDP). **Tor mode** (opt-in, research-gated): a Tor client built into the WASM app, reaching Tor through Snowflake (§28) |
| Signalling     | Two-way out-of-band exchange (QR, link, paste, share). **No signalling server** |
| Relay          | None in direct mode: TURN is disabled locally and relay candidates are rejected from the peer. Tor mode routes through the volunteer Tor network, by explicit user choice only (§28) |
| STUN           | Public, free, no registration: Google and Cloudflare by default; the list is user-editable (§9.3). Needed in practice for any connection that is not on the same LAN |
| Persistence    | No messages, ever. Identity keys and the contacts list are saved only if the user chooses to, and only encrypted (§7.3) |
| Targets        | Desktop Chrome, Edge, Firefox and Safari; iOS Safari, both as a tab and as an installed PWA (§17.5) |

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
| P2 | **Direct communication.** In direct mode, chat traffic goes only between peers. **Tor mode** (§28) is an explicit, user-selected anonymity transport that routes through volunteer Tor relays by design. It is never an automatic fallback. |
| P3 | **Out-of-band rendezvous.** One invite/answer round trip over QR, link, paste or share replaces a signalling server. |
| P4 | **Identity ≠ network location.** Identity is a static key, never an IP, port, candidate or connection. |
| P5 | **Network paths are disposable.** Paths are rebuilt; identity, room and sequence numbers survive. |
| P6 | **Encryption is mandatory and layered.** DTLS for transport, plus application E2E. |
| P7 | **No history.** Private chats are RAM only, and keys are zeroized when the session ends. This applies to **every private chat, with no exception**. The only exception in the whole system is a separate, opt-in feature: **public channels** (§27), which are public, permanent publications and never contain private-chat data. |
| P8 | **Failure is explicit.** No silent relay, whether a server or a peer. If there is no direct path, the application says so. There is never a silent switch between direct mode and Tor mode, in either direction. |
| P9 | **Trust is explicit.** Security is never stronger than (a) the integrity of the out-of-band channel and (b) the code served by the static host. The UI and documentation MUST say so. |
| P10 | **Only our WASM.** The project builds and ships only the static web app (HTML, JS glue, WASM). No native companion, helper, daemon, extension or app-store build. A feature that cannot be done inside the browser sandbox is not built. Users MAY run third-party software on their own (a VPN, WARP, Kubo), but the app never requires it. |

## 3. Layers

```
+------------------------------------------------+
| 5 Application   rooms, members, messages, UI   |  Rust (core)       + JS (DOM only)
| 4 Crypto        Noise KK (1:1), MLS (groups),  |  Rust (crypto)
|                 SAS, identity key file         |
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
| STUN (default list in §9.3, user-editable) | Discover server-reflexive (srflx) and global IPv6 candidates | The public IP and port of each peer, and when they connect | **No** |
| Tor network (Tor mode only, §28): volunteer relays; optional bridges (obfs4, Snowflake, WebTunnel) | Anonymous onion-to-onion transport | The guard relay (or bridge) sees your IP but not your destination. No relay sees both ends. The Snowflake broker sees that you use Snowflake | Carries **encrypted** chat by design (Noise inside Tor) |

### 4.3 Forbidden

Application backend, WebSocket or HTTP signalling, TURN, chat relay, message database, Redis, Kafka, IPFS message storage, a central presence service, central authentication, analytics or telemetry, remote logging, and third-party scripts. These bans apply **fully and without exception to private chats (1:1 and rooms)**.

- IPFS MAY mirror the **static assets**.
- **The only exception** is the separate, opt-in **public channels** feature (§27). Its **public** posts use the IPFS data format, are served by the owner **only over a Tor onion service**, and may be mirrored by followers (optionally to public IPFS). Private-chat data (messages, keys, codes, membership) MUST NOT enter it. In code this is enforced by crate and page isolation: the `channel` crate has no API that accepts chat data, and it runs only on `channel.html`, which has its own CSP.

## 5. Trust base

| Party | What we trust it for | Mitigation |
|---|---|---|
| Out-of-band channel (in person, messenger, …) | Integrity of the invite and answer: they carry the DTLS fingerprints and static keys | SAS comparison (§10.4). In-person QR scanning is strongest. |
| Static host / repository owner | Serving honest code | CSP, SRI, a service worker that pins the version and asks before updating (§17), reproducible builds with published hashes |
| Browser and OS | Everything | Out of scope (§21) |
| STUN operator | Nothing about security. It learns metadata only | Configurable list, and a LAN-only mode |
| Embedded Tor client (Tor mode only, §28) | Correct Tor protocol behaviour, built from upstream arti with a small patch set | Our code; reviewed and fuzzed like the rest of the WASM (§28.9) |

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
│   │   ├── identity.rs             # 32-byte seed → X25519 static + Ed25519 signing keys, PeerId, display handle
│   │   ├── keyfile.rs              # encrypted identity file v2: seed + contacts + TLV sections (§7.3)
│   │   ├── contacts.rs             # fixed-capacity contact table (256), verified keys (§7.5)
│   │   ├── noise.rs                # Noise_KK session wrapper (snow), in-place encrypt/decrypt
│   │   ├── sas.rs                  # short authentication string from handshake hash
│   │   └── mls.rs                  # (MVP-3) openmls group wrapper
│   ├── core/                       # pure deterministic state machines, no wasm-bindgen
│   │   ├── session.rs              # per-peer connection FSM (§12)
│   │   ├── recovery.rs             # recovery ladder T0–T3 (§13)
│   │   ├── room.rs                 # membership, dedup high-water marks (§14)
│   │   ├── sendq.rs                # fixed ring send queue + backpressure (§11.5)
│   │   ├── messages.rs             # message table: TTL, edit, delete, replies, reactions, ticks (§11.7)
│   │   ├── transport.rs            # Transport = Direct(WebRTC) | Tor(embedded), mode guard (§28.5)
│   │   └── io.rs                   # Input / Action enums, ActionSink (fixed capacity)
│   └── wasm/                       # the ONLY crate touching the browser
│       ├── lib.rs                  # #[wasm_bindgen] ChatApp facade
│       ├── rtc.rs                  # web-sys RTCPeerConnection / RTCDataChannel adapter
│       ├── stats.rs                # getStats → Diagnostics
│       ├── qr.rs                   # qrcode (encode) + BarcodeDetector / rqrr (decode)
│       ├── share.rs                # Web Share, clipboard, BroadcastChannel hand-off
│       ├── keystore.rs             # file download/upload + IndexedDB slots (≤ 8) + Web Locks (§7.2)
│       └── tor/                    # embedded Tor (§28): arti runtime shim, Snowflake transport, IndexedDB dir cache
│                                   # built as a separate lazily-loaded WASM module (tor_bg.wasm), used only by tor.html
├── web/                            # static assets (§4.1)
├── tests/                          # native replay/fuzz of proto + core
└── docs/spec/                      # this document
```

### 6.2 Core contract

```rust
pub enum Input<'a> {
    Tick,                                        // timer (driven by adapter, e.g. every 250 ms)
    UserNewIdentity, UserImportIdentity { blob: &'a [u8], pass: &'a [u8] },
    UserCreateInvite, UserApplyCode(&'a [u8], CodeSource), // decoded bytes; source = Scan | Link | Paste (§10.4)
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

- An identity is a **32-byte seed**. Two keys are derived from it with domain-separated HKDF-BLAKE2s:
  - `HKDF(seed, "p2pchat/x25519")` → the X25519 static key, used by Noise (§10.1);
  - `HKDF(seed, "p2pchat/ed25519")` → the Ed25519 signing key, used for MLS credentials (MVP-3). It is sent to peers inside the Noise channel, so it is bound to the `PeerId`.
  - `HKDF(seed, "p2pchat/onion")` → the Ed25519 **onion service key** (Tor mode, §28). It is stable for saved identities, and temporary for temporary ones.
- `PeerId` is the X25519 public key itself. It is not hashed.
- While the app runs, the seed and the derived secret keys live only in wasm linear memory. They are zeroized on sign-out and on `pagehide`.
- **Display handle:** `anon_` plus the first 6 hex digits of `BLAKE2s(PeerId)`. It is **not authentication**. Nicknames are free text that users choose, and are only ever shown inside the encrypted channel.
- Rule: `PeerId ≠ IP ≠ port ≠ candidate ≠ RTCPeerConnection ≠ DTLS certificate`.

### 7.2 Sign-in ("login") and several identities

The app opens on a sign-in screen:

| Choice | What happens | Privacy |
|---|---|---|
| **New temporary identity** (default) | A fresh seed from the CSPRNG. Nothing is saved; the identity is gone when the tab closes | Sessions cannot be linked to each other |
| **New identity + save** | A fresh seed. The user picks a **label** (for example "Work"), a passphrase, and how to save the encrypted key file (§7.3) | The same `PeerId` in every session: peers can recognise you and link your sessions. The UI MUST say so |
| **Use saved identity** | The user picks one of the identities remembered on this device, or loads a key file, then enters its passphrase | As above |
| **Receive identity from another device** | Identity transfer over P2P (§7.6) | As above |

- **Several identities on one device.** Up to **8** remembered identities, each in its own IndexedDB slot. The sign-in list shows each slot's label and display handle. Any number of extra key files can be kept outside the app.
- **One identity per tab.** Each tab runs its own WASM instance with at most one active identity. Different tabs may use different identities at the same time. Switching identity means signing out (keys zeroized) and signing in again.
- **The same identity in two tabs is refused.** The tab takes the Web Lock `p2pchat-id-<first 16 hex digits of BLAKE2s(PeerId)>`. If the lock is already held, sign-in fails with `E_DUPLICATE_SESSION` ("This identity is already open in another tab").
- There are no accounts and no server. "Login" only means unlocking a key the user holds.

### 7.3 Encrypted identity key file (v2)

**Outer layout** (little-endian):

| Size | Field |
|---|---|
| 4 | magic `"P2PK"` |
| 1 | `ver` = 2 (a v1 file is read and upgraded when it is next saved) |
| 1 | `kdf` = 1 (Argon2id) |
| 4 | KDF parameters: `u16 m_mib` (default 19), `u8 t` (default 2), `u8 p` (default 1) |
| 16 | `salt` |
| 24 | `nonce` (new for every save) |
| 1 + n | `label` (≤ 32 B UTF-8). Plaintext so the sign-in list can show it; authenticated as AAD. It MUST NOT be secret, and the UI says so |
| 4 | `ct_len` (≤ 65 536 + 16) |
| ct_len | `ciphertext` = XChaCha20-Poly1305(body), with every outer byte above as AAD |

**Encrypted body**:

| Size | Field |
|---|---|
| 32 | `seed` |
| 1 + n | nickname (≤ 32 B) |
| … | TLV sections: `u8 type`, `u16 len`, value |

| TLV | Section | Value |
|---|---|---|
| 0x01 | CONTACTS | `u16 count` (≤ 256), then one entry per contact (§7.5) |
| 0x02 | — | Reserved (was the companion token in v0.5) |
| 0x03 | KUBO | `u8 len` + RPC token (only for followers who mirror to IPFS, public channels plan §7.2) |
| 0x04 | CARD | `card_secret [u8; 16]`, `expires_at u32` (0 = never): the secret in your current contact card (§7.5) |
| other | — | Kept unchanged on re-save, so newer app versions can add sections |

- Key: Argon2id(passphrase, salt), 32 bytes. After sign-in the derived key stays in wasm memory, so the app can **re-save** after the contacts change without asking again. It is zeroized on sign-out. The Argon2id defaults are the OWASP minimum.
- **Save and load options:**
  1. **Download** it as `p2pchat-<label>.p2pkey`, and load it back with a file picker. This is the reliable backup on every target.
  2. **Copy** it as base64url text. It is about 170 characters with no contacts, and much longer with contacts, so the file is recommended then.
  3. **Remember on this device**: keep the same encrypted blob in an IndexedDB slot. The passphrase is still needed each time; it is never stored. **Limit:** Safari deletes storage written by scripts after 7 days without a visit, for sites used in a Safari tab (not for installed PWAs).
- **Backups can go stale.** After contacts change, the remembered slot is updated automatically, but a downloaded file cannot be. The UI shows "Backup out of date — download again" until the user does.
- Each passphrase attempt (Argon2id) allocates about 19 MiB. This happens during sign-in, **before** the zero-allocation phase starts (§22).
- Wrong passphrase or damaged file: `E_KEYFILE_INVALID`. There is no recovery: a lost file or passphrase means a lost identity.

### 7.4 Wallet identity: deferred

Wallet sign-in is removed from the MVP plan. When it comes back, the design in REVIEW A13 applies: the wallet signs a binding to the `PeerId`, the binding is only sent inside the encrypted channel, and WalletConnect is excluded.

### 7.5 Contacts (saved identities only)

- A fixed-capacity table of up to **256** contacts. It exists in RAM while signed in, and on disk only inside the encrypted key-file body (TLV 0x01). It is **disabled for temporary identities**.
- **Entry layout:**

  | Size | Field |
  |---|---|
  | 32 | `peer_id` |
  | 1 | `flags`: bit0 `verified` (the SAS was compared), bit1 has `onion_pk`, bit2 has `sign_pk` |
  | 0 / 32 | `onion_pk` (Tor mode, §28) |
  | 0 / 32 | `sign_pk` (MLS credential) |
  | 4 | `added_at` (u32, Unix seconds) |
  | 1 + n | local nickname (≤ 32 B), chosen by the user, not by the peer |

- **Adding a contact:** after a connection, the user taps "Save contact". The flag `verified` is set only if the SAS was confirmed in that session, or later by comparing the SAS again. **No last-seen time and no message data are ever stored.**
- **What contacts give you:**
  - peers are shown by their local nickname instead of an anonymous handle, plus a ✔ badge when verified;
  - **SAS skipped:** when the peer's static key matches a *verified* contact, no SAS prompt is needed, because the key is already pinned;
  - **impersonation warning:** when a new peer calls itself by a verified contact's nickname but has a **different key**, the UI warns "This is not the Alice you verified";
  - **reconnecting without a QR** in Tor mode, through the stored `onion_pk` (§28.6).
- **Privacy:** a contacts list records who you talk to. It is only as safe as the key file's passphrase (§21).

**Contact cards** (saved identities only)

- A contact card is a QR code or link (`#k=`) that lets someone add you **without chatting first**. In Tor mode it also lets them **dial you later** (§28.4). It works like Tox's "nospam".
- **Layout** (`kind = 6 CONTACT_CARD`, about 89 B plus the nickname):

  | Size | Field |
  |---|---|
  | 4 | header (§8.3) |
  | 32 | `peer_id` |
  | 32 | `onion_pk` |
  | 16 | `card_secret` |
  | 4 | `expires_at` |
  | 1 + n | suggested nickname (≤ 32 B) |

- **Adding from a card** creates an **unverified** contact. The first connection still prompts the SAS, which then sets `verified`.
- **Revoking:** "Reset my contact card" generates a new `card_secret` (TLV 0x04). Every card shared before stops working for first contact. Contacts already added are not affected.
- **Default expiry:** 30 days, or "never".
- **Direct mode:** a card only pins the key and nickname. A connection still needs the invite and answer exchange (§8), because there is no rendezvous without signalling.

### 7.6 Moving an identity to another device (P2P, no cloud)

This is Telegram's "log in with a QR code", done without a server:

1. On the new device, choose "Receive identity from another device". It shows an invite with `flags.bit2 TRANSFER` set.
2. The old device, signed in with that identity, scans or pastes it. The two connect with the normal flow (direct mode, or Tor mode in §28).
3. **The SAS is mandatory**, even when both codes were scanned in person, because the whole identity is at stake.
4. After the SAS is confirmed on both sides, the old device sends its **encrypted key-file blob** (§7.3) in IDENTITY_CHUNK records (§11.2). The blob is still encrypted with the passphrase.
5. The new device asks for the passphrase. It decrypts the blob and offers to remember it in a slot, and to download a backup.
6. The old device then offers "Keep this identity here" (**the default**) or "Remove it from this device".

- There is **no sync afterwards**: each device holds its own copy.
- Two devices using the same identity **at the same time** in the same 1:1 or room are refused. The second session gets `E_DUPLICATE_SESSION` from the peer or the room owner, who sees two live sessions with one `PeerId`.

## 8. Rendezvous

### 8.1 The exchange is always two-way

Browser WebRTC cannot finish DTLS or ICE without the remote description (REVIEW R1). Every new pairwise link therefore needs exactly one **invite → answer** exchange. The first link is done out of band. Later links inside a room are signalled through the owner (§14.4).

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

**iOS:** `BarcodeDetector` is not available in Safari, so the in-app scanner decodes QR codes with the Rust `rqrr` crate from camera frames. A link or a QR scanned with the iOS Camera app always opens in a **Safari tab**, never in the installed PWA. Users of the installed PWA must therefore scan with the in-app scanner or paste the code. The UI MUST say this.

### 8.3 Binary layout (little-endian, `#[repr(C)]`-compatible, parsed in place)

**Common header (4 bytes)**

| Off | Size | Field | Notes |
|---|---|---|---|
| 0 | 1 | `ver` | Protocol major version = `1` |
| 1 | 1 | `kind` | `1` = INVITE, `2` = ANSWER, `3` = RESUME_INVITE, `4` = RESUME_ANSWER, `5` = TOR_INVITE (§28.4), `6` = CONTACT_CARD (§7.5) |
| 2 | 1 | `flags` | bit0 `LAN_ONLY`, bit1 `GROUP` (MVP-3), bit2 `TRANSFER` (identity transfer, §7.6), bit3 `OBSERVER` (room invite for a read-only member, §14.2). Other bits are reserved and MUST be 0 |
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

- Nothing in these messages is secret in the key sense. There are no private keys. The messages are still **sensitive bootstrap material**: whoever answers first wins the link (§8.6).
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
- *Optional optimization*: uppercase Base32 in QR alphanumeric mode saves about one QR version. It needs a URL that is valid in uppercase. GitHub Pages project paths are case-sensitive, so this needs a custom domain. It is not planned.

### 8.6 Lifetime, single use and races

- `expires_at = now + TTL`. The TTL defaults to 5 min and can be set from 1 to 30 min.
- **Alice enforces** the expiry: she rejects an answer after `expires_at` with `E_EXPIRED_INVITE` and closes the connection. Bob's check is advisory, with ±120 s tolerance for clock skew.
- An offer belongs to exactly one `RTCPeerConnection`, so **each invite is single-use by construction**. A second answer for a consumed `invite_id` gets `E_INVITE_CONSUMED`. To invite N people, Alice makes N invites (in a room, each joiner still gets an invite from the owner, §14.4).
- Race: if a third party answers first, Alice connects to them. The SAS (§10.4) and the display of who is connected catch this.

### 8.7 URL and fragment hygiene

- Codes travel **only in the URL fragment**, which browsers never send to the host.
- On load, the app reads the fragment, runs `history.replaceState(null, '', '<base>/')`, and passes the bytes to the core.
- If a tab opened from a link is not the owning tab, it posts the code on `BroadcastChannel('p2pchat-codes')`. It closes itself if the owning tab acknowledges within 500 ms. Otherwise it offers paste or scan.
- **iOS:** a Safari tab and the installed PWA have separate storage, so `BroadcastChannel` cannot connect them. When there is no acknowledgement, the tab offers "Copy code", and the user pastes it into the app.
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
- **Default STUN list.** These are public servers: free, no account, no key. They have no SLA, may rate-limit, and each one learns the user's public IP.

  | Server | Operator | Default |
  |---|---|---|
  | `stun:stun.l.google.com:19302` | Google | yes |
  | `stun:stun.cloudflare.com:3478` | Cloudflare | yes |
  | `stun:stun1.l.google.com:19302` … `stun4` | Google | no (optional) |
  | `stun:global.stun.twilio.com:3478` | Twilio | no (optional) |

- The default has **two** servers from two operators. That covers one operator being down while keeping gathering fast: Chromium warns that five or more servers slow down discovery, and every extra server is one more operator that sees the user's IP.
- The list is editable in settings, with a limit of 4 entries. Only `stun:` URLs are accepted; `turn:` and `turns:` are rejected (§9.2).
- Whether each default server has an IPv6 (AAAA) address is checked in spike S8. A dual-stack server is needed for the direct IPv6 path.
- **LAN-only mode**: no STUN, only mDNS host candidates, and the `LAN_ONLY` flag is set in the invite.

### 9.4 Candidate privacy modes

| Mode | Candidates placed in the invite or answer |
|---|---|
| `lan-only` | host-mDNS only |
| `default` | host-mDNS, srflx-v4, srflx-v6 |
| `max-connectivity` | adds raw host-v4 and host-v6 when the browser exposes them. After camera permission is granted for the QR scanner, Chromium may expose them *(spike S4)* |

An extra **Drop IPv6** toggle, off by default, removes every IPv6 candidate in any mode. It is the fix offered by the IPv6-bypass warning (§29.2).

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
- Noise gives: (a) identity that continues across new `RTCPeerConnection`s and DTLS certificates; (b) authentication that does not depend on the transport, for owner-relayed signalling (§14.4) and future transports; (c) a clear, pinned root for the SAS.
- For chat, the cost of double encryption is negligible.

### 10.3 Groups (MVP-3)

MLS (RFC 9420) via `openmls`, compiled for `wasm32`. See §14.4 for ordering.

### 10.4 Short authentication string (SAS)

- `s = BLAKE2s("p2pchat-sas" ‖ handshake_hash)`. It is shown two ways:
  - **6 decimal digits** = `u32::from_le_bytes([s[0], s[1], s[2], 0]) % 10^6` (24 bits, negligible bias);
  - **4 emoji**, one per byte of `s[3..7]`, from a fixed 256-entry table.
- It is shown on both devices after the handshake.
- Both exchange scenarios are supported. The core decides the SAS policy from where the code came from (`CodeSource` in §6.2):

  | How the code arrived (either side) | SAS policy |
  |---|---|
  | Both codes scanned with the in-app camera | Optional. The SAS is shown, but the link is treated as in person |
  | At least one code opened from a link or pasted | **Prompted.** A full-width banner asks the users to compare the SAS on another channel, such as a voice call. The peer keeps an "unverified" badge until someone taps "Codes match" |

- "Codes don't match" closes the link with `E_SAS_REJECTED` and shows a warning that the exchange may have been intercepted.
- **Verified contacts skip the SAS:** if the peer's static key matches a contact marked `verified` (§7.5), no prompt is shown.
- **Always mandatory** for identity transfer (§7.6).
- **Tor mode:** prompted for every non-contact, because the one-way invite gives Alice no out-of-band proof of Bob's key (§28.4).

### 10.5 Primitives and randomness

- Use RustCrypto crates only: `x25519-dalek`, `ed25519-dalek`, `chacha20poly1305` (including XChaCha20), `blake2`, `hkdf`, `argon2`, `zeroize`. **No hand-written cryptographic primitives.**
- The CSPRNG is `getrandom` with the `wasm_js` backend, which calls `crypto.getRandomValues`.
- WebCrypto is **not** on the protocol path: it is async-only and would split state between JS and Rust.

## 11. Wire protocol (inside the DataChannel or Tor stream)

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
| 0x01 | HELLO | ver_min u8, ver_max u8, caps u32 bitset, max_msg u16, sign_pk [u8; 32] (Ed25519, §7.1), nick_len u8, nick | MVP-1 |
| 0x02 | CHAT | `chat_seq u64`; then `ttl_s u32` if `rflags.bit1`; then `reply_sender u8`, `reply_seq u64` if `rflags.bit2`; then UTF-8 text (≤ 4 096 B) | MVP-1 |
| 0x03 | ACK | `chat_seq u64`, cumulative: **delivered** | MVP-1 |
| 0x04 | PING / 0x05 PONG | t_ms u64 | MVP-1 |
| 0x06 | GOODBYE | ErrorCode u16 | MVP-1 |
| 0x07 | REKEY | – | MVP-1 |
| 0x08 | TYPING | `u8` state (0 = stopped, 1 = typing) | MVP-1 |
| 0x09 | READ | `chat_seq u64`, cumulative: **read** | MVP-1 |
| 0x0A | EDIT | `target_seq u64`, new UTF-8 text (≤ 4 096 B). Own messages only | MVP-1 |
| 0x0B | DELETE | `target_sender u8`, `target_seq u64`. Own messages; the room owner may delete any (§11.7) | MVP-1 |
| 0x0C | REACT | `target_sender u8`, `target_seq u64`, `len u8` + emoji (0–32 B UTF-8; 0 = remove) | MVP-2 |
| 0x40 | IDENTITY_CHUNK | `idx u16`, `total u16`, up to 12 KiB of the encrypted key-file blob. Only valid on a `TRANSFER` link after the SAS is confirmed (§7.6) | MVP-2 |
| 0x10 | SIGNAL_OFFER / 0x11 SIGNAL_ANSWER | target conn / peer, AnswerBin-shaped body | MVP-2 (T1), MVP-3 (§14.4) |
| 0x30… | ROOM_* / MLS_* | defined in MVP-3 | MVP-3 |

Unknown `rtype` values are ignored if `rflags.bit0` (IGNORABLE) is set. Otherwise they cause `E_PROTOCOL_MISMATCH`.

### 11.3 Sequencing and deduplication

- The transport replay check is the Noise nonce (§10.1). There is no set of seen IDs.
- `chat_seq` is a u64 per sender that **never resets** for the whole session, including across reconnects.
- A receiver keeps `last_chat_seq[peer_idx]`. A CHAT with `chat_seq ≤ last` is a duplicate (a resend after a reconnect) and is dropped.
- **Pending queue** (as in qTox's "will send when online"):
  - The sender keeps every CHAT that has not been ACKed, including messages typed while the peer is `DEGRADED`, `RECOVERING` or `SUSPENDED`, in a fixed ring of **256** entries per link.
  - In rooms, each ring entry carries a 16-bit mask of the members that have not yet ACKed it.
  - Entries are **resent automatically** after T0–T3 recovery, or when a Tor contact comes back (§28.6).
  - EDIT and DELETE of a pending message rewrite its ring slot in place, so the peer only ever receives the final version.
  - If the ring is full, `UserSend` fails with `E_BACKPRESSURE` ("Too many messages waiting for Bob").
  - The queue lives **in this tab's RAM only**. Closing the tab drops it, and the UI says so ("Pending messages exist only in this tab").
- `MessageId` = `(PeerIdx u8, chat_seq u64)`. `PeerIdx` is an index into the room's member table, which is fixed-size.

### 11.4 Capabilities and versioning

- Every code and frame carries `ver`.
- If majors differ, the connection is closed with `E_PROTOCOL_MISMATCH`.
- HELLO carries `ver_min..ver_max` and a capability bitset: bit0 group, bit1 resume, bit2 in-band restart, bit3 **sends read receipts**, bit4 **sends typing**, bit5 Tor, and so on. The session uses the intersection of the protocol bits. Bits 3 and 4 are the peer's privacy settings (§11.7).

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

### 11.7 Message features: semantics

All of these exist only in RAM, reach only peers who are connected (or who come back while the message is still pending), and are gone when the session ends.

**Status ticks**

| Tick | Meaning | Source |
|---|---|---|
| 🕓 | **Pending**: in the queue, the peer is not connected (§11.3) | Local |
| ✓ | **Delivered** | ACK |
| ✓✓ | **Read** | READ. In a room: "read by k/N" in the message details, and ✓✓ once every current member has read it |

**Read receipts and typing** are on by default in 1:1 and off in rooms. They are **reciprocal**: if you turn off read receipts, you stop sending READ and you also stop seeing others' ✓✓. The same applies to typing.

- **READ** is sent when the message is on screen and the page is visible, coalesced to at most one per second.
- **TYPING** is sent at most once every 3 s while typing. The receiver clears the indicator after 6 s without a refresh, or on `0`.

**Replies**

- A CHAT with `rflags.bit2` quotes `(reply_sender, reply_seq)`.
- The receiver renders a quote from its own RAM copy. If that message is unknown, expired or deleted, it shows "Message unavailable".
- Only the reference is sent, never the quoted text.

**Edit**

- Only your own messages can be edited. **There is no time limit**: messages only exist for the session anyway, and a limit would add clock-skew problems between peers without adding any privacy.
- The EDIT record carries the full new text. Receivers replace the text and show "edited". There is no edit history.

**Delete**

- **Delete for everyone:** the sender can delete their own messages. In a room, the **owner** can delete any member's message (moderation).
- Receivers overwrite the text slot with zeros and show "Message deleted".
- **Delete for me:** local only.

**Reactions**

- Each member has at most one reaction per message. The latest one wins, and an empty emoji removes it.
- An emoji is 1–32 UTF-8 bytes. Multi-codepoint emoji are allowed, and anything that renders as more than one grapheme is rejected.

**Self-destruct timer** (`rflags.bit1`)

- `ttl_s` ∈ {5, 30, 60, 300, 3 600, 86 400} seconds, chosen per chat and applied to every message sent while it is set.
- **Who sets it:** in 1:1, **either person**. In a room, **only the owner**. A change is shown to everyone as a notice in the chat ("Alice set messages to disappear after 1 min"). The setting is sent as a CHAT with `rflags.bit3 SETTING` and an empty body.
- **Recipient:** the countdown starts when the message is first on screen, which is when READ would be sent.
- **Sender:** the countdown starts when READ arrives. If the peer has read receipts off (HELLO bit3 = 0), it starts at ACK.
- On expiry, both sides overwrite the text slot with zeros, remove it from the DOM, and any reply quoting it shows "Message unavailable". A pending message does not start its countdown until it is delivered.
- **Limit (shown in the UI):** a recipient can still screenshot or copy the message. Nothing can prevent that.

**Rooms and observers** (§14.2)

- Observers are **strictly read-only**: every receiver drops CHAT, EDIT, DELETE, TYPING and REACT from an observer (`E_NOT_PERMITTED`).
- An observer sends only protocol records: ACK, READ (if read receipts are on), PING/PONG and GOODBYE.

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
| **T2** | ICE restart relayed **through the room owner**, end-to-end encrypted (§14.4) | Peer-relayed | Group, with a partial break; the owner is still linked to both sides | MVP-3 |
| **T3** | **Resume code**: RESUME_INVITE / RESUME_ANSWER exchanged out of band. Noise KK with the **same static keys** rebinds the session automatically (no room join, `chat_seq` continues, unacknowledged messages are resent) | Out of band | Always | MVP-1 |

- Glare during T1 or T2 (both sides restart at once) is handled with the *perfect negotiation* pattern. The **polite** peer is the one whose `PeerId` is lexicographically greater.
- UI copy:
  - T0–T2: "Reconnecting…"
  - T3: "Direct path lost — share a reconnect code."
  - The words "backup server" MUST NOT appear.

## 14. Groups (MVP-3)

### 14.1 Topology

- Full mesh, **up to 16 members**: 120 links in the room, 15 per member.
- Each link is an independent pairwise session as in §8–§13.
- Cost of the cap on each device: 15 `RTCPeerConnection`s, 15 × (64-frame send ring + 256-message resend ring) of preallocated memory, and 15 DTLS/ICE keepalive streams. This is acceptable for chat. It is checked on iOS Safari in spike S9.

### 14.2 Room authority: the owner

- The room creator is the **owner**, and the only authority:
  - only the owner creates invites and admits members;
  - only the owner removes members;
  - the owner is the **only MLS committer** (§14.5). Members send Proposals, such as "I am leaving".
- There is **no succession**. When the owner leaves, the room is **disposed** (§14.6).
- **Roles:** `owner` (exactly one), `member` (reads and writes), and **`observer`** (read-only).
  - Only the owner assigns roles: an `OBSERVER` room invite (flag bit3), or a later role change.
  - The role is stored in the MLS group context (a GroupContext extension changed only by owner commits), so every member knows every role.
  - Roles are **enforced by every receiver** (§11.7): there is no central point that could filter.
  - Observers are full mesh members for networking. **They see the other members' IP addresses, and the other members see theirs** (§29.2).

### 14.3 Propagation

- The sender sends each message **directly** to every connected member. There is **no flooding and no forwarding** (REVIEW R6). Forwarding is deferred and is not part of any current phase.
- Deduplication uses `last_chat_seq[leaf_idx]`, a fixed array of 16.
- A member without a direct link to the sender does not receive the message. The UI shows that link as missing ("no direct path to Carol").

### 14.4 Joining (introductions by the owner)

1. A new member M does the out-of-band exchange with the **owner** O (§8).
2. O commits an MLS Add for M and sends the Welcome to M. O sends M's `PeerId` and signing key to every other member Y over the existing encrypted links.
3. For each Y, M and Y exchange SIGNAL_OFFER/ANSWER **through O**. The body is sealed M↔Y with a key from the MLS exporter secret, so O forwards only ciphertext. This is signalling only, never chat.
4. The M↔Y link comes up directly. If ICE fails, that pair stays unlinked, and both sides show it.

### 14.5 Group key management

- MLS (RFC 9420). The credential is the Ed25519 signing key from §7.1, bound to the `PeerId` by the Noise session.
- With a single committer there are no forked epochs, even without a delivery service.
- A new joiner cannot read earlier epochs. A leave or removal leads to a Commit and a new epoch.

### 14.6 Disposal

The room is disposed when either of these happens:

- the owner leaves on purpose (GOODBYE to every member), or closes the room;
- no member has had a link to the owner for longer than the owner grace period (10 min, the same as `SUSPENDED` in §12). While the owner is unreachable, members can keep chatting on their existing links, but nobody can join or be removed.

On disposal, every member's core:

1. shows "Room closed by owner" or "Owner unreachable — room closed", with `E_ROOM_DISPOSED`;
2. closes all links in the room;
3. zeroizes the room's MLS and Noise state.

There is no takeover. To continue, someone creates a new room and becomes its owner.

## 15. Wallet authentication (deferred)

Wallet authentication is not in any current phase (§7.4). Identity comes only from user-held keys (§7.2).

## 16. Persistence, logging and memory hygiene

- **Never written:** messages, keys or codes to localStorage, IndexedDB, Cache Storage, a service-worker cache, the clipboard (beyond the moment of sharing, §8.7), IPFS, a blockchain, a server or analytics.
- **Settings MAY be stored** in localStorage: STUN list, privacy mode, TTL, nickname. They are not secret.
- **The identity key MAY be stored** only in its encrypted form (§7.3), and only if the user chooses it. Nothing else is persisted.
- All secrets live in wasm linear memory and are zeroized on `CLOSED` and on `pagehide`.
- **Limit:** chat text shown in the DOM exists as JS strings that cannot be zeroized, and the OS may swap RAM to disk. We do not claim otherwise.
- **Logging:** only when `#[cfg(debug_assertions)]`. Release builds contain **no log calls at all**, removed at compile time. Debug logs may contain `PeerIdx`, states, candidate *types*, RTT, byte counts and error codes. They never contain plaintext, keys, passphrases, codes, signatures or IP addresses.

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

- `connect-src 'self'` stops `fetch` or XHR from sending data anywhere. Tor sessions run on a separate page, `tor.html`, with its own CSP (§28.6).
- **Limit:** browsers do not reliably enforce the CSP `webrtc` directive, so peer connections cannot be restricted by CSP.

### 17.4 Mobile lifecycle

- A connection may survive, be suspended, or be killed in the background. A killed connection goes to `SUSPENDED` / T3.
- Messages sent while a peer is fully offline are lost: there is no mailbox, and the UI says so ("Messages are delivered only while peers are connected").
- **Risk (spike S6):** iOS may kill the pending peer connection while Alice switches to a messenger to share an invite. Mitigations: use the system share sheet, and regenerate the invite automatically when the app returns to the foreground if the connection has died.

### 17.5 iOS Safari (a required target)

**Distribution: PWA only.** There is no App Store (or TestFlight) app, and none is planned. On iOS the app is either used in a Safari tab or installed with Share → Add to Home Screen, from Safari or, from iOS 16.4, from other browsers. Consequences:

- No App Store review, and no Apple developer account is needed.
- No native background modes (VoIP push, background networking), so connections pause when the app goes to the background (see *Backgrounding* below).
- No push notifications. Web Push on iOS needs a server to send the pushes, which P1 forbids.
- Every iOS browser uses WebKit (outside the EU), so "iOS Safari" covers every browser on iOS.


| Topic | Rule |
|---|---|
| QR scanning | `rqrr` in WASM from `getUserMedia` frames (there is no `BarcodeDetector`) |
| Links | Always open in a Safari tab, not the PWA (§8.2). The PWA flow uses the in-app scanner or paste |
| Tab ↔ PWA | Separate storage partitions, so there is no `BroadcastChannel` hand-off (§8.7) |
| Backgrounding | Safari suspends the page soon after it goes to the background. Sharing uses the share sheet, which keeps the app in the foreground. **Reading** the answer in a messenger puts the app in the background, which is the S6 risk. **Spike S6 gates the remote flow on iOS**: if the pending connection does not survive about 60 s in the background, the remote flow on iOS is changed so that the iOS user is always the **answerer**. The answerer can rebuild its peer connection from the invite it still holds, and the offerer, typically on desktop, keeps waiting |
| Storage | IndexedDB may be deleted after 7 days in a Safari tab (§7.3). Key files are the backup |
| WASM | `simd128` is supported from Safari 16.4. The target is the current and previous major iOS versions |

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
- The UI never shows raw IP addresses unless "show addresses" is turned on in the diagnostics view. The only exception is the **"What your peer sees"** panel (§29.2), which shows **your own** visible addresses so that you can check your VPN.
- In Tor mode the first line reads **`VIA TOR · IP hidden`**, and there are no path or relay lines.

## 19. Error codes (stable, `u16`)

| Code | Name | Meaning |
|---|---|---|
| 0x0001 | E_INVALID_INVITE | The code failed to parse or validate |
| 0x0002 | E_EXPIRED_INVITE | Alice received the answer after `expires_at` |
| 0x0003 | E_INVITE_CONSUMED | A second answer for the same invite |
| 0x0004 | E_ANSWER_MISMATCH | The answer's `invite_id` is unknown or belongs to another connection |
| 0x0010 | E_INVALID_ROOM | |
| 0x0011 | E_ROOM_FULL | The room already has 16 members |
| 0x0012 | E_ROOM_DISPOSED | The owner left or was unreachable past the grace period (§14.6) |
| 0x0013 | E_NOT_OWNER | A non-owner tried an owner-only action |
| 0x0020 | E_AUTH_FAILED | Static key mismatch, or an invalid signing-key binding |
| 0x0021 | E_CRYPTO_FAILED | Noise or MLS failure, nonce gap, or bad tag |
| 0x0022 | E_SAS_REJECTED | The user marked the SAS as a mismatch |
| 0x0023 | E_DUPLICATE_SESSION | The identity is already open in another tab (Web Lock), or already live in this 1:1 or room from another device |
| 0x0024 | E_NOT_A_CONTACT | Tor mode: an incoming onion stream from a key that is neither a contact nor holding a live invite |
| 0x0030 | E_ICE_FAILED | |
| 0x0031 | E_NO_DIRECT_PATH | No non-relay pair succeeded |
| 0x0032 | E_RELAY_REJECTED | A relay candidate was selected or offered |
| 0x0033 | E_CONNECTION_TIMEOUT | |
| 0x0034 | E_NETWORK_CHANGED | |
| 0x0035 | E_PEER_OFFLINE | |
| 0x0036 | E_TOR_UNAVAILABLE | Tor mode: the embedded Tor client cannot bootstrap (Snowflake broker or proxies unreachable) or cannot reach the onion. There is **no fallback** to direct mode |
| 0x0037 | — | Reserved (was E_COMPANION_AUTH in v0.5) |
| 0x0040 | E_PROTOCOL_MISMATCH | |
| 0x0041 | E_MESSAGE_TOO_LARGE | |
| 0x0042 | E_BACKPRESSURE | Send queue or pending ring is full |
| 0x0043 | E_NOT_PERMITTED | The action is not allowed for this role or sender (an observer sending, editing someone else's message) |
| 0x0050 | E_BROWSER_UNSUPPORTED | No `RTCPeerConnection`, WASM or `crypto.getRandomValues` |
| 0x0060 | E_KEYFILE_INVALID | Wrong passphrase, or a damaged or unsupported key file |

## 20. Limits (defaults)

| Item | Value |
|---|---|
| Invite TTL | 5 min (1–30) |
| Candidates per code | ≤ 8 |
| Chat text | ≤ 4 096 B UTF-8 |
| Frame | ≤ 16 384 B |
| Send queue | 64 frames per connection |
| Pending / resend ring | 256 messages per link |
| Contacts | 256 |
| Remembered identities per device | 8 (IndexedDB slots) |
| Key-file body | ≤ 64 KiB |
| Self-destruct values | 5 s, 30 s, 1 min, 5 min, 1 h, 1 day |
| Reaction emoji | ≤ 32 B, one grapheme |
| Typing refresh / expiry | 3 s / 6 s |
| Tor frame | the same 16 KiB limit, with a `u16` length prefix |
| Room size | 16 members (120 links) |
| Owner grace (room disposal) | 10 min |
| STUN servers | 2 by default, at most 4 |
| Argon2id (key file) | m = 19 MiB, t = 2, p = 1 |
| App ping interval | 15 s idle |
| Suspended grace | 10 min |
| Rekey | 2^20 messages or 10 min |

## 21. Security model

**Protected:**

- message confidentiality and integrity against network observers and against the static host;
- peer authentication, pinned to the out-of-band exchange and optionally SAS-verified;
- room membership (MLS, owner-controlled, MVP-3);
- a saved identity key and **contacts list** at rest (Argon2id + XChaCha20-Poly1305; as strong as the passphrase);
- in **Tor mode**: your IP address, hidden from the peer and from network observers (§28);
- no central storage or relay;
- no traffic replay across sessions.

**Not protected:**

- peer IP anonymity **in direct mode** (the peer, and the STUN operator, see your public IP). Use a VPN or Tor mode (§29);
- traffic analysis and DPI detection (WebRTC is easy to fingerprint; no obfuscation is attempted);
- a compromised browser, extension, OS or device;
- a malicious static host serving altered code (mitigated, not eliminated: §17.2);
- a swapped code on a remote out-of-band channel when the SAS is not compared;
- a peer secretly routing its own side through a relay or VPN;
- delivery to offline peers;
- unlinkability between sessions **when a saved identity is reused** (the same `PeerId` every time);
- the room surviving its owner.

**Attackers considered:**

| Attacker | Capability | Mitigation |
|---|---|---|
| Network observer | IPs, timing, sizes, volume | DTLS + Noise hide the contents. Metadata is accepted as exposed |
| Out-of-band channel MITM (messenger) | Swaps invite and answer | SAS comparison on another channel |
| Malicious peer | Fake identity, injection, replay, joining, abuse of membership, relay candidates | Keys pinned by the invite, AEAD with strict nonces, owner-only admission and MLS commits, relay filtering |
| Thief of a key file | Offline passphrase guessing; this also exposes the contacts list | Argon2id. The UI enforces a minimum passphrase strength |
| Snowflake proxy / broker (Tor mode) | Sees your IP and that you use Snowflake; a malicious proxy can drop traffic | Tor's own encryption and circuit verification inside WASM; proxy rotation (Turbotunnel keeps the session across proxies) |
| Static host | Serves altered code | SRI, a service worker that pins the version and asks before updating, reproducible builds |
| STUN operator | Learns IPs and timing | Configurable list, LAN-only mode |
| Browser extension or device | Full access | Out of scope |

**Allowed claims:**

- "Messages are encrypted in transit (DTLS) and end-to-end at the application layer."
- "Chat traffic travels directly between peers; the app uses no relay and rejects relay paths."
- "There is no server-side or app-side chat history."
- "Your identity is independent of your network connection."
- "Authenticity depends on how you exchanged codes. Compare the safety code when you did not meet in person."
- "In direct mode your peer can see your IP address. The app can use your VPN or Tor mode to hide it."
- "In Tor mode, your IP address is hidden from your peer and from network observers."

**Forbidden claims:**

- "anonymous"
- "untraceable"
- "invisible to DPI"
- "your IP is hidden" (except in Tor mode, and never for direct mode)
- "works behind every NAT or firewall"
- "no one but the peers is involved" (STUN and the code host are involved)

## 22. Rust implementation rules (browser mapping of the HFT doctrine)

| Rule | Application here |
|---|---|
| No allocation after init | Rust core: all buffers (RX/TX slots, send rings, resend rings, member tables sized for 16) are allocated once in `ChatApp::new`, after sign-in, and linear memory never grows afterwards. Argon2id's working memory during sign-in (§7.3) is part of the init phase. Allocations inside `web-sys` and `wasm-bindgen-futures` at the JS boundary happen on the control plane (signalling and stats) and are **documented exceptions** |
| POD `#[repr(C)]`, `Copy`, no heap fields in protocol types | Yes (§7.1, §8.3, §11.1) |
| No `dyn`; generics | Yes. The `core` ↔ `wasm` boundary is the `Input` / `Action` enums, not trait objects |
| No iterators or bounds checks in hot loops | Applies to frame parse, encrypt/decrypt and dedup (index loops, `get_unchecked` behind `debug_assert!`) |
| Fail fast; `panic = "abort"` | Yes. A protocol violation closes the connection with an error code. An internal invariant violation aborts the WASM instance, and the UI shows "internal error — reload" |
| No logging in release | Yes (§16) |
| Zero-copy networking | §11.6 lists the unavoidable copies |
| SIMD | `-C target-feature=+simd128`, supported by all target browsers. **AVX2 is not available in WASM** |
| Thread pinning, NUMA, lock-free SPSC across threads | **Not applicable.** WASM here is single-threaded: Pages cannot send COOP/COEP headers, so there is no `SharedArrayBuffer`. The design is single-writer by construction |
| Raw sockets, io_uring, RDMA, kernel bypass | **Not applicable** in a browser sandbox. Nothing native is built (P10) |
| crossbeam bounded channels | **Not applicable** (single thread). Fixed rings in `core` instead |
| Build profile | `lto = "fat"`, `codegen-units = 1`, `panic = "abort"`; `wasm-opt -O3` (or `-Oz` if binary size wins, decided by spike S5) |

## 23. Delivery plan

| Phase | Scope |
|---|---|
| **MVP-1** | Sign-in (temporary identity, or saved identity with an encrypted key file); 1:1 in direct mode; two-way exchange by QR, link and paste; binary codes with SDP reconstruction; STUN defaults, privacy modes and **Drop IPv6**; relay prohibition; Noise KK; SAS policy; T3 resume code; **pending queue and ticks; replies, edit, delete, self-destruct timers; typing and read receipts**; **IP disclosure features (§29.2)**; basic diagnostics; CSP, SRI and a version-pinned service worker; desktop browsers and iOS Safari |
| **MVP-2** | T1 in-band ICE restart and `NetChanged` handling; full diagnostics; **contacts; several identities (IndexedDB slots, Web Locks); identity transfer over P2P; reactions** |
| **MVP-3** | Owner-controlled rooms of up to 16 members, with **observer role** and owner moderation (delete); introductions by the owner; MLS with the owner as single committer; room disposal; T2 recovery through the owner |
| **TOR-1** | `p2pchat-companion`: embedded arti client and onion service, loopback WebSocket bridge, token and Origin check, built-in bridge lists; reproducible releases for Linux, macOS and Windows |
| **TOR-2** | Tor mode in the PWA for 1:1: transport guard, TOR_INVITE (one-way), Noise IK, stream framing, "VIA TOR" UI |
| **TOR-3** | Reconnecting contacts through stable onion addresses, with no QR |
| **TOR-4** | Rooms over Tor (the owner shares members' onion addresses; members dial each other directly) |
| **CARDS** | Contact cards and card secrets; card-based Tor dial (with TOR-3) |
| **CH-1…CH-5** | Public channels, Tor-only owner (plan v2 §9) |
| **TOR-E (research)** | Tor built into WASM through Snowflake, for iOS and desktop without the companion. Gates G1–G4 in `EMBEDDED-TOR-WASM.md` |
| **Deferred** | Wallet authentication; peer forwarding of chat; file transfer; voice and video; rooms larger than 16 |

## 24. Validation spikes (must finish before the design they gate is frozen)

| ID | Question | Gates |
|---|---|---|
| S1 | Does the Appendix A template SDP work as a remote description in Chrome, Firefox and Safari, desktop and mobile, in all 9 offerer/answerer combinations? | §8 |
| S2 | Measured code sizes (ufrag and pwd lengths, candidate counts) per browser | §8.5 |
| S3 | T0 behaviour per browser: continual gathering, backup pairs, recovery through a peer-reflexive path | §13 |
| S4 | Does camera permission disable mDNS obfuscation, and does that persist after the camera track stops? | §9.4 |
| S5 | Size of `snow`, and later `openmls`, on `wasm32`; `-O3` vs `-Oz` | §22 |
| S6 | iOS lifetime of a pending `RTCPeerConnection` while in the background during sharing. **Gates the remote flow on iOS** | §17.4, §17.5 |
| S7 | Link hand-off on desktop browsers (`BroadcastChannel`). On iOS it is already known not to work between a tab and the PWA; the paste fallback is the design | §8.7 |
| S8 | IPv6 reachability (AAAA records) of the default STUN servers, and srflx-v6 gathering on each target | §9.3 |
| S9 | 15 simultaneous peer connections on iOS Safari: memory, keepalive and battery | §14.1 |
| TS1 | Embedded arti: stable onion-service hosting with a key supplied at runtime and kept **only in RAM**; onion-to-onion time to first message and RTT | §28 |
| TS2 | Can the PWA reach `ws://127.0.0.1:47431` from `https://<owner>.github.io` in Chrome and Edge (Local Network Access prompt), Firefox, and desktop Safari? (Shared with P2 of the channels plan) | §28.3 |
| TS3 | WebRTC through Cloudflare WARP and 2–3 common VPNs, on desktop and iOS: does the srflx address show the VPN's exit? | §29.1 |
| TS4 | How often does IPv6 bypass a v4-only VPN in practice (the §29.2 warning)? | §29.2 |

## 25. Owner decisions

### 25.1 Decisions in v0.3 (open questions from v0.2)

| # | Topic | Decision | Where |
|---|---|---|---|
| Q1 | Exchange scenario | Both in person and remote. The SAS policy follows the code source | §8.2, §10.4 |
| Q2 | STUN | Google and Cloudflare by default, public and free; the list is editable | §9.3 |
| Q3 | Room cap | 16 members | §14.1, §20 |
| Q4 | Room authority | Only the owner admits members. When the owner leaves, the room is disposed | §14.2, §14.6 |
| Q5 | Wallet | Deferred. Users generate keys at sign-in and may save them encrypted for reuse | §7.2–§7.4 |
| Q6 | Peer forwarding | Deferred | §14.3 |
| Q7 | Hosting | GitHub Pages project site | Header, §4.1 |
| Q8 | Browsers | Desktop Chrome, Edge, Firefox, Safari, plus iOS Safari | §17.5 |

### 25.2 Decisions in v0.4

| Topic | Decision | Where |
|---|---|---|
| Messaging | Replies, edit, delete, self-destruct timers, reactions, typing indicators, read ticks, pending queue | §11.2, §11.3, §11.7 |
| Identities | Contacts in the encrypted key file; several identities per device; identity transfer over P2P | §7.2, §7.3, §7.5, §7.6 |
| Rooms | Observer (read-only) role | §14.2 |
| Tor | A second, separate transport through a local companion, with all features that fit the architecture | §28 |
| IP privacy | VPN or Cloudflare WARP, Tor mode, LAN-only mode; three disclosure features in MVP-1 | §29 |

### 25.3 Decisions in v0.5

| # | Topic | Decision | Where |
|---|---|---|---|
| QN2 | Tor mode scope | Chosen per signed-in session | §28.2 |
| QN3 | Typing and read receipts | On in 1:1, off in rooms, reciprocal | §11.7 |
| QN4 | Moderation | The room owner can delete any message | §11.7 |
| QN5 | Observers | Strictly read-only (no reactions, no typing) | §11.7 |
| QN6 | Self-destruct control | Either person in 1:1; only the owner in rooms; changes shown to all | §11.7 |
| QN7 | Tor bridges | On by default (Snowflake first) | §28.3 |
| QN8 | After identity transfer | The old device keeps it by default | §7.6 |
| QN9 | Contact cards | Yes, with a revocable card secret | §7.5, §28.4 |
| QN10 | Edit and delete time limit | None (session-scoped) | §11.7 |
| QN11 | Remembered identities | 8 per device | §7.2 |
| QN12 | Public channels | Desktop-only publishing; the owner is always hidden (Tor-only onion); no link to the chat identity; 4 KiB posts; republishing and gateways as designed in plan v2 | §27 |
| — | Tor without the companion | Research track with gates (Snowflake + arti in WASM) | §28.1, EMBEDDED-TOR-WASM.md |

### 25.4 Open question

- **QN1:** code signing for `p2pchat-companion`. Either ship unsigned, with instructions for getting past macOS Gatekeeper and Windows SmartScreen, or pay for signing (Apple Developer about $99 a year, plus a Windows code-signing certificate).

## 26. Out of scope

- A custom NAT traversal, DTLS, WebRTC or cryptographic algorithm.
- Wallet authentication and peer forwarding (deferred, §23).
- A DHT.
- Blockchain or IPFS message storage.
- Server-side signalling.
- TURN fallback.
- Persistent history.
- Multi-frame QR.
- Single-QR bootstrap in **direct mode** (impossible, REVIEW R1). Tor mode supports it (§28.4).
- Traffic obfuscation in direct mode. Tor bridges in Tor mode are the only exception.

## 27. Public channels (opt-in exception to P7 and §4.3)

- A public channel is a permanent, **public** broadcast feed. Only its owner can post, and everyone else can only read.
- Posts are limited to 4 KiB of text.
- It is a **publication, not a chat**, and is fully separate from private chats: its own page (`channel.html`), its own CSP, its own crate, and its own keys. Private-chat data MUST NOT flow into a channel.
- **The owner is always hidden:**
  - The owner publishes **only through a Tor onion service**, run by the companion on a desktop. The owner never takes part in the public IPFS network, so the owner's IP is never exposed.
  - The channel's keys and onion address are derived one-way and are **not linked** to the owner's chat identity. The owner is known only if they say so in the channel.
- **Content format:** IPFS-native (CIDs, dag-cbor, CAR, IPNS V2 records), verified in Rust. Followers may mirror a channel over their own onion, or, accepting that their own IP becomes visible, to public IPFS. That is what lets readers without Tor read it through public gateways.
- Details and phases: [`../plans/PUBLIC-CHANNELS-IPFS.md`](../plans/PUBLIC-CHANNELS-IPFS.md) (plan v2).

## 28. Tor mode (a second, separate transport)

### 28.1 Why a separate transport

- WebRTC data runs over UDP, and Tor carries only TCP streams. Tor Browser and Onion Browser also disable WebRTC entirely. So Tor mode is **not** "WebRTC over Tor": it is a different transport, under the **same** protocol above it (Noise, records, rooms, contacts, messaging features).
- **Availability with the companion:** desktop Chrome, Edge and Firefox, plus desktop Safari if spike TS2 passes.
- **Without the companion (research track):** Tor **built into the WASM app** through Snowflake, for iOS and desktop. It is feasible in principle, with gates and limits: on iOS the onion is reachable **only while the app is in the foreground**. See [`../plans/EMBEDDED-TOR-WASM.md`](../plans/EMBEDDED-TOR-WASM.md). Until gate G3 passes, iOS users get IP privacy from a VPN or WARP (§29.1).

### 28.2 Mode selection and isolation

- The connection mode is chosen **per signed-in session**, on the sign-in screen: **Direct** (the default) or **Tor**.
- It cannot be changed without signing out, so a single session never mixes the two.

### 28.3 The companion (`p2pchat-companion`)

```
 PWA (core, Noise, UI)  ──ws://127.0.0.1:47431──▶  p2pchat-companion
                                                     ├─ embedded arti (Tor client + onion service)
                                                     ├─ optional bridges: obfs4, Snowflake, WebTunnel (built-in lists)
                                                     └─ byte pipes only: it never sees plaintext
```

- A native single binary for Linux, macOS and Windows, built from `companion/` in this repository. Releases are reproducible, their hashes are published, and it embeds arti, so no separate Tor install is needed.
- **Local security:**
  - it listens on `127.0.0.1:47431` only;
  - it rejects any `Origin` other than the app's;
  - the first WebSocket message must carry the 32-byte **token**. The companion prints a pairing code once, and the PWA stores the token in the key file (TLV 0x02; temporary identities re-enter it each session).
- **Protocol between the PWA and the companion:**
  - one **control** WebSocket, carrying `HOST(onion_secret)`, `DIAL(onion_pk)` and incoming-stream notifications;
  - one WebSocket per Tor stream, carrying raw bytes.
- The onion secret key is sent at session start and kept **in RAM only** by the companion *(spike TS1)*. It is dropped when the control socket closes.
- **Bridges are on by default.** Snowflake is first, then obfs4 and WebTunnel from the built-in lists. The companion bundles the pluggable-transport binaries (lyrebird and snowflake-client). Settings offer "Connect to Tor directly" for faster bootstrap where Tor is not blocked.

### 28.4 One-way invite (single QR) and handshake

**TOR_INVITE** (`kind = 5`), about 104 B:

| Size | Field |
|---|---|
| 4 | header (§8.3) |
| 16 | `invite_id` |
| 16 | `room_id` |
| 32 | `static_pk` (X25519) |
| 32 | `onion_pk` (Ed25519, the v3 onion address) |
| 4 | `expires_at` |

The virtual port is fixed. There are no ICE candidates, fingerprints or answer.

- **Single QR:** Bob dials Alice's onion address straight from the invite. **No answer code is needed.** This is the one-way bootstrap that v0.1 wanted, and it becomes possible under Tor.
- **Handshake:** `Noise_IK_25519_ChaChaPoly_BLAKE2s`. The initiator is the dialler (Bob, or a contact reconnecting). The prologue is the invite bytes, or `"p2pchat/contact"` for contact reconnects. The first handshake payload carries `invite_id` (16 B) and the initiator's `onion_pk`.
- **Who is accepted:** an incoming stream is accepted only in one of these cases:
  1. its static key is in **contacts**;
  2. the payload names a **live, unused** `invite_id`;
  3. the payload carries the current, unexpired **`card_secret`** (§7.5). The user is then asked "Bob (from your contact card) wants to connect", and the key is added as a contact if they accept.

  Everything else is closed with `E_NOT_A_CONTACT` before any application data. This blocks spam, which was Tox's "nospam" problem.
- For a card-based dial, the first handshake payload carries `card_secret` (16 B) in place of `invite_id`.
- **SAS:** prompted for every non-contact (§10.4), because Alice has no out-of-band proof of Bob's key.

### 28.5 Transport rules

- **Framing:** each frame is `u16 len` followed by an outer frame (§11.1). The same 16 KiB limit applies, and the same records apply (§11.2).
- **No-mixing guard:** in a Tor session, `core` refuses to emit `CreatePc`, the `wasm` crate refuses to construct an `RTCPeerConnection`, and no STUN server is contacted. Any attempt aborts, because it would be a bug that leaks the IP.
- **No fallback:** if Tor fails, the session fails with `E_TOR_UNAVAILABLE`. The app never quietly switches to direct.
- **Network changes need no recovery ladder:** an onion address does not depend on the network. After a drop, the side that dialled simply dials again with backoff (§12), the Noise session re-handshakes, and the pending queue is resent.
- **Latency:** 0.3–1.5 s per message (6 hops onion to onion). Text only; voice and video are out of scope for Tor.

### 28.6 Contacts reconnect without a QR (TOR-3)

- A contact entry with `onion_pk` (§7.5) enables **"Connect"** from the contacts list. It dials the stored onion address directly, from any network, at any time the contact is online in Tor mode.
- The onion address comes from the identity seed, so it is stable for saved identities and survives moving to another device (§7.6).
- While signed in in Tor mode, the PWA keeps the onion service up so contacts can reach you. The UI shows "Reachable by contacts via Tor". It stops when the tab closes.

### 28.7 Rooms over Tor (TOR-4)

- The same owner model (§14), with each member hosting an onion service.
- The owner sends each new member's `onion_pk` to the others, and members dial each other **directly**. No signalling relay is needed, which is simpler than WebRTC's §14.4.
- The member cap is 16 (15 Tor circuits per member).

### 28.8 What Tor mode hides, and what it does not

- **Hides:** your IP from peers and from network observers. With bridges, it also hides the fact that you use Tor.
- **Does not hide:**
  - what you type, including nicknames;
  - timing correlation by a global observer;
  - that sessions are linked when a **saved** identity is reused, because its onion address stays the same;
  - anything the companion's host machine can see.

## 29. IP-address privacy

### 29.1 Supported options

| Option | Hides your IP from the peer | Hides it from your ISP / network | Cost / registration | Desktop | iOS |
|---|---|---|---|---|---|
| **LAN-only mode** (§9.4) | The peer sees only a LAN address, and both must be on the same network | ✓ (traffic stays local) | free | ✓ | ✓ |
| **Your own VPN with UDP support**, full tunnel, set up outside the app. **Cloudflare WARP** (1.1.1.1 app) is free and needs no account | ✓ (the peer sees the VPN exit) | ✓ (the ISP sees that a VPN is used) | depends on the VPN; WARP is free | ✓ | ✓ |
| **Tor mode** (§28) | ✓ | ✓ (and with bridges, Tor use too) | free | ✓ | ✗ |

**Not options (the UI MUST say so when relevant):**

- **iCloud Private Relay:** it does not cover WebRTC UDP.
- **Tor Browser / Onion Browser:** they disable WebRTC, so direct mode cannot run there.
- **TURN:** forbidden (P8).

### 29.2 Disclosure features (MVP-1, direct mode)

1. **"What your peer sees" panel.** In the diagnostics, and one tap away from the chat header, the app lists **your own** srflx addresses: the IPv4 and IPv6 addresses the peer will see, taken from the gathered candidates and the selected pair in `getStats`. Text: *"Your peer can see this IP address. If you use a VPN, it should be the VPN's address, not your home address."*
2. **IPv6-bypass warning.** A browser cannot tell which interface belongs to a VPN, so the rule is:
   - Whenever gathering produced **both** a srflx-v4 and a srflx-v6, show a banner: *"Your peer can see two addresses: an IPv4 one and an IPv6 one. If you use a VPN that covers only IPv4, your IPv6 address is your real one."*
   - The banner offers **Drop IPv6** (§9.4), which applies from the next code.
   - The user can mark "I use a VPN" in settings. The banner is then shown on **every** session with both families, not just the first.
3. **Room exposure warning** (MVP-3):
   - When a code with `GROUP` set is opened: *"This is a room invite. The owner and every member (up to 16) will see your IP address."*
   - When the owner's room state arrives, before the mesh links start: *"Connect to N other members? Each of them will see your IP address."* The user can cancel, or continue.
   - The same text is shown to observers.

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

1. Sign in: a temporary identity, or unlock a saved one.
2. Create the room (`RoomId`).
3. Create the peer connection and the negotiated DataChannel.
4. `createOffer` and `setLocalDescription`.
5. Gather (≤ 3 s) and filter by privacy mode.
6. Build the InviteBin and show it as a QR or link.
7. Wait for the answer, until the TTL runs out.

**Join (Bob):**

1. Scan or open the link. The fragment is read and stripped.
2. Parse and validate: version, lengths, advisory expiry.
3. Sign in, if not already signed in.
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
