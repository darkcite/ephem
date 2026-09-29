<!-- SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0 -->
<!-- Copyright 2026 Anton (darkcite) -->
# Ephem: design, specification and plan

**Ephem** is the product name of this ephemeral, backend-free, peer-to-peer messenger (decided 2026-09-28). The repository is `darkcite/ephem` (renamed from `p2p-chat`); the site is `https://<owner>.github.io/ephem/`.

| Field | Value |
|---|---|
| Product | **Ephem** |
| Document | The **single** project document. It replaces the earlier SPEC, REVIEW, plans and spike notes (all merged here on 2026-09-28) |
| Spec level | v1.2 (see the decision log, §25) |
| Status | MVP-1…MVP-3, **Tor mode (TOR-1…TOR-4, also on the real Tor network and laptop ↔ iPhone)**, **contact cards**, **public channels (CH-1…CH-5)**, **Tor bridge lines (BR-1…BR-3)** and **one app with Chats, Following and My channels, up to 16 chats per tab (UI-1…UI-6)** built and tested end to end (§24.2, Appendix F). Open: iOS memory under Tor (G4), live runs of v1.2, C-P5 on each browser |
| Deployment | GitHub Pages, project site `https://<owner>.github.io/ephem/` |
| Runtime | Browser PWA. **Only our WASM app is built: no native programs (P10)** |
| Implementation | Rust (edition 2024) → `wasm32-unknown-unknown` |
| Transport | **Direct mode:** WebRTC DataChannel (SCTP / DTLS / ICE / UDP). **Tor mode** (opt-in, gated): a Tor client built into the WASM app, reaching Tor through Snowflake (§28) |
| Signalling | Two-way out-of-band exchange (QR, link, paste, share) in direct mode; one-way invite in Tor mode. **No signalling server** |
| Relay | None in direct mode (TURN disabled, relay candidates rejected). Tor mode routes through the volunteer Tor network, by explicit user choice only |
| STUN | Public, free, no registration: Google and Cloudflare by default (both dual-stack); user-editable (§9.3) |
| Persistence | No messages, ever. Identity keys and contacts only if the user saves them, and only encrypted (§7.3) |
| Targets | Desktop Chrome, Edge, Firefox and Safari; iOS Safari, as a tab and as an installed PWA (§17.5) |
| Checkpoints | `checks/run_all.sh` (§24) |

Keywords **MUST**, **MUST NOT**, **SHOULD** and **MAY** are used as defined in RFC 2119.

## How to read this document

| Part | Sections | Content |
|---|---|---|
| 0 | Status at a glance | Where the project stands and what comes next |
| I | §1–§3 | Summary, principles, layers |
| II | §4–§22, §26–§29 | Normative specification: infrastructure, architecture, identity, rendezvous, WebRTC, crypto, wire protocol, state machines, groups, PWA, security, Tor mode, IP privacy, public channels |
| III | §23–§25 | **Plan**: phases, gates, checkpoints with results, decision log |
| IV | Appendices A–F | SDP template, end-to-end flows, embedded-Tor engineering, public-channel design, features considered and rejected, **v1.2: Tor bridges and the three-tab app (proposal and as built)** |

## Status at a glance (2026-09-29)

| Area | State |
|---|---|
| Design | Complete up to MVP-3 (rooms by owner-signed state, v0.9), plus Tor mode and public channels |
| Proven | SDP rebuild from ~150–165-byte codes connects in **every tested pair: Chrome, Safari, Firefox and iPhone, both directions across engines**; camera permission exposes the real IP in Chrome and iPhone Safari but **not Firefox**; a pending offer survives **182 s** in the iOS background; the protocol crates fit in 291 KB gzip **with no C code**; Argon2id 34 ms (t = 4, Apple Silicon); **arti (Tor) builds for WASM on Linux and macOS (G1)**; **live Snowflake rendezvous + DataChannel from Chrome, Safari, Firefox and iPhone (G2)**; browsers publish an IPNS record and read it back via `trustless-gateway.link` |
| Built | **MVP-1 feature-complete, 41/41 end-to-end checks in Chromium** (APP-E2E, §24.2): landing page; `/app/` PWA; temporary or saved identity (encrypted key file, one identity per tab); invite → answer by QR (camera scanner in wasm), link or paste, hand-off between tabs; Noise KK; SAS policy; 1:1 chat with pending queue and ticks 🕓 ✓ ✓✓, typing, reply, edit, delete, self-destruct timers; T3 reconnect codes; diagnostics with relay rejection; "what your peer sees" panel and IPv6 warning; version-pinned service worker, SRI, offline start. 188 KB gzip wasm. 33 native unit tests |
| Built (MVP-2) | **MVP-2 feature-complete, 18/18 end-to-end checks** (APP-E2E-MVP2, §24.2): up to 8 remembered identities (IndexedDB) with sign-in from the list; nicknames; contacts in the key file (verified by SAS, SAS skipped next time, impersonation warning, backup-out-of-date notice); reactions; identity transfer to another device over P2P; in-band ICE restart T1 (perfect negotiation, triggered by a stuck path, a network change or by hand); full diagnostics. 40 native unit tests |
| Built (MVP-3) | **Rooms, 18/18 end-to-end checks with four browsers** (APP-E2E-ROOM, §24.2): owner-controlled rooms of up to 16 with member and **observer** roles; the owner-signed room state (Ed25519) verified by every member; introductions through the owner with **sealed** signalling (the owner forwards what it cannot read); full mesh; one sequence number per sender on every link; sender labels, replies across members, delivery "✓ k/N"; owner moderation and room timer; removal, leaving, disposal; **T2** (lost member links come back by themselves through the owner). 225 KB gzip wasm. 46 native unit tests |
| Built (Tor) | **Tor mode, in the offline lab** (`checks/tor-lab/`: private Tor network + the real Go Snowflake broker, proxy and server): Snowflake client in Rust (Turbotunnel, KCP, smux; 10 MB interop with the Go server, fuzzed); arti in the page over Snowflake; **onion service hosted from a tab**; `app/tor.html` with the Tor build of the app. **1:1 chat over Tor 18/18** (APP-E2E-TOR: one-way TOR_INVITE, Noise IK, same SAS, chat, receipts, stream loss → redial, contacts connect with no code) and **rooms over Tor 9/9** (APP-E2E-TOR-ROOM: owner + 2 members, introductions onion to onion, redial). Bootstrap 5.8 s in the lab; Tor build 2.08 MB gzip |
| Built (cards, channels) | **Contact cards** (§7.5): QR/`#k=` card, add from a card (in Contacts, or any code field), reset, first Tor chat through a card with the host's accept prompt (APP-E2E-CARDS 9/9, APP-E2E-TOR-CARDS 6/6 ×4). **Public channels** (§27, App. D): `crates/channel` (CIDv1, strict DAG-CBOR, CAR, IPNS V2; the js-ipns reference validator accepts our records); owner, onion gateway from the tab, reader over Tor or a public gateway, mirrors, IPNS publish through a Tor exit: APP-E2E-TOR-CHANNEL 19/19 in the lab, C-P3 1 000 posts read in 0.4 s |
| Built (v1.2, Appendix F) | **Tor bridge lines** (F.2): Snowflake lines in the Tor Browser format, every unusable line refused with its reason, kept in the key file, `#b=` share links (APP-E2E-TOR-BRIDGES 9/9). **Several chats per tab** (F.3.2): up to 16 chats and rooms, one onion for all (APP-E2E-MULTI 12/12, APP-E2E-TOR-MULTI 6/6). **One app, three tabs** (F.3): Chats, Following, My channels; channels share the chats' Tor client and identity; the direct page loads the Tor build only for its channel tabs. Direct build 232 KB gzip, Tor build (chats + channels) 2.16 MB |
| Next | 1. Owner's live runs: `ONLY=tor ./checks/run_all.sh` includes cards, channels, bridges and several chats; iPhone memory under Tor (G4); C-P5 (OPFS quota) per browser. 2. Then merge to `main`, public repo and Pages (§23.1a: once everything planned is implemented, owner decision 2026-09-29). 3. Device runs of the room flow (S9). Camera/QR work after the merge (owner decision) |
| Blocked on devices | S3, S6, S7, S9 (phones, real networks) |

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
| 4 Crypto        Noise KK (every link), owner-  |  Rust (crypto)
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
<base>/index.html            (CSP and integrity hashes in <meta>, stamped by tools/stamp.py; §17.2, §17.3)
<base>/app.js                (DOM glue; registers the SW; fetches the wasm with its pinned hash)
<base>/app.css
<base>/pkg/ephem.js          (wasm-bindgen glue)
<base>/pkg/ephem_bg.wasm
<base>/manifest.webmanifest
<base>/sw.js
<base>/icons/*
```

### 4.2 Optional third-party infrastructure

| Service | Purpose | What it learns | Carries chat? |
|---|---|---|---|
| STUN (default list in §9.3, user-editable) | Discover server-reflexive (srflx) and global IPv6 candidates | The public IP and port of each peer, and when they connect | **No** |
| Tor network (Tor mode only, §28): Snowflake broker and bridge (Tor Project), volunteer Snowflake proxies, volunteer relays | Anonymous onion-to-onion transport, entered through Snowflake | The Snowflake proxy and broker see your IP and that you use Snowflake, but not your destination. No relay sees both ends | Carries **encrypted** chat by design (Noise inside Tor) |

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
ephem/
├── Cargo.toml                      # workspace; release: lto="fat", codegen-units=1, panic="abort", opt-level="s"|3
├── crates/
│   ├── proto/                      # #![no_std] — zero-copy codecs, no alloc
│   │   ├── code.rs                 # invite / answer codes (§8.3) encode/decode over &[u8]
│   │   ├── candidate.rs            # CandidateBin (§8.4) + SDP a=candidate line render/parse
│   │   ├── sdp.rs                  # template-based SDP reconstruction (Appendix A)
│   │   ├── frame.rs                # outer frame header + inner records (§11)
│   │   ├── b64url.rs               # base64url codec into caller-provided buffers
│   │   └── error.rs                # ErrorCode (u16, §19)
│   ├── crypto/
│   │   ├── identity.rs             # 32-byte seed → X25519 static + Ed25519 signing keys, PeerId, display handle
│   │   ├── keyfile.rs              # encrypted identity file v2: seed + contacts + TLV sections (§7.3)
│   │   ├── contacts.rs             # fixed-capacity contact table (256), verified keys (§7.5)
│   │   ├── noise.rs                # Noise_KK handshake (snow); transport over the raw split keys: in-place ChaCha20-Poly1305, header as AAD
│   │   ├── sas.rs                  # short authentication string from handshake hash
│   │   └── seal.rs                 # sealed boxes between two static keys: relayed room signalling (§14.4)
│   ├── core/                       # pure deterministic state machines, no wasm-bindgen
│   │   ├── session.rs              # per-peer connection FSM (§12)
│   │   ├── recovery.rs             # recovery ladder T0–T3 (§13)
│   │   ├── room.rs                 # owner-signed room state, roles, relayed-signal codec (§14)
│   │   ├── sendq.rs                # fixed ring send queue + backpressure (§11.5)
│   │   ├── messages.rs             # message table: TTL, edit, delete, replies, reactions, ticks (§11.7)
│   │   ├── transport.rs            # Transport = Direct(WebRTC) | Tor(embedded), mode guard (§28.5)
│   │   └── io.rs                   # Input / Action enums, ActionSink (fixed capacity)
│   ├── wasm/                       # the ONLY crate touching the browser
│   │   ├── lib.rs                  # #[wasm_bindgen] App facade, events to JS, identity save/load, up to 16 chats per tab (App. F.3.2)
│   │   ├── rtc.rs                  # web-sys RTCPeerConnection / RTCDataChannel adapter, getStats path check
│   │   ├── qr.rs                   # qrcode (encode) + rqrr (decode; BarcodeDetector is used from JS when present)
│   │   ├── room.rs                 # rooms: owner-signed state, introductions, T2 (§14)
│   │   ├── tor.rs                  # Tor build only (feature `tor`): chat links over Tor streams, onion service (§28)
│   │   └── (key files, contacts, transfer in lib.rs; IndexedDB slots ≤ 8 in app/slots.js: it stores only the encrypted key file)
│   ├── channel/                    # public channels, sans-IO: CIDv1, DAG-CBOR, CAR, IPNS V2, signed blocks, gateway (§27)
│   ├── channel-web/                # channels in the browser: owner, onion gateway, reader, mirrors (part of the Tor build)
│   ├── snowflake/                  # sans-IO Snowflake client: Turbotunnel, encapsulation, KCP, smux (§28.3)
│   └── tor/                        # arti in the page: runtime shim, Snowflake carrier, bridge-as-TCP, TLS, bridge lines (§28.3, App. F.2)
├── vendor/                         # four arti 0.46 crates with small wasm-only patches (vendor/README.md)
├── app/                            # static web app (§4.1): index.html (+ generated tor.html), app.js, channels.js, bridges.js, slots.js, ui.js, app.css, sw.js, manifest, icons, pkg/
├── index.html, site.css            # landing page (§23.1a)
├── tools/                          # stamp.py (integrity, §17.2), make_icons.py
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
  - `HKDF(seed, "p2pchat/ed25519")` → the Ed25519 signing key: a room owner signs the room state with it (§14.2), and contacts keep it. It is sent to peers inside the Noise channel, so it is bound to the `PeerId`.
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
| 4 | KDF parameters: `u16 m_mib` (default 19), `u8 t` (default **4**), `u8 p` (default 1) |
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
| 0x03 | — | Reserved (was a Kubo RPC token in v0.5; the app never talks to Kubo, P10) |
| 0x04 | CARD | `card_secret [u8; 16]`, `expires_at u32` (0 = never): the secret in your current contact card (§7.5) |
| 0x05 | TOR_BRIDGES | UTF-8 Snowflake bridge lines (Appendix F.2), optionally led by the comment line `# ephem: also use the Tor Project Snowflake` |
| 0x06 | FOLLOWS | UTF-8 JSON, the channels followed (Appendix F.3.3, decision D2): `[{"n": IPNS name, "o": [onions], "s": highest record sequence seen, "t": title, "seen": newest post seq read, "m": mirrored}]` |
| other | — | Kept unchanged on re-save, so newer app versions can add sections |

- Key: Argon2id(passphrase, salt), 32 bytes. After sign-in the derived key stays in wasm memory, so the app can **re-save** after the contacts change without asking again. It is zeroized on sign-out. Defaults: m = 19 MiB (the OWASP minimum) and t = 4, which is about 100 ms in desktop WASM (spike S5b, where t = 2 took 50 ms).
- **Save and load options:**
  1. **Download** it as `ephem-<label>.p2pkey`, and load it back with a file picker. This is the reliable backup on every target.
  2. **Copy** it as base64url text. It is about 170 characters with no contacts, and much longer with contacts, so the file is recommended then.
  3. **Remember on this device**: keep the same encrypted blob in an IndexedDB slot. The passphrase is still needed each time; it is never stored. **Limit:** Safari deletes storage written by scripts after 7 days without a visit, for sites used in a Safari tab (not for installed PWAs).
- **Backups can go stale.** After contacts change, the remembered slot is updated automatically, but a downloaded file cannot be. The UI shows "Backup out of date — download again" until the user does.
- Each passphrase attempt (Argon2id) allocates about 19 MiB. This happens during sign-in, **before** the zero-allocation phase starts (§22).
- Wrong passphrase or damaged file: `E_KEYFILE_INVALID`. There is no recovery: a lost file or passphrase means a lost identity.

### 7.4 Wallet identity: deferred

Wallet sign-in is removed from the MVP plan. When it comes back, the design in §7.4 applies: the wallet signs a binding to the `PeerId`, the binding is only sent inside the encrypted channel, and WalletConnect is excluded.

### 7.5 Contacts (saved identities only)

- A fixed-capacity table of up to **256** contacts. It exists in RAM while signed in, and on disk only inside the encrypted key-file body (TLV 0x01). It is **disabled for temporary identities**.
- **Entry layout:**

  | Size | Field |
  |---|---|
  | 32 | `peer_id` |
  | 1 | `flags`: bit0 `verified` (the SAS was compared), bit1 has `onion_pk`, bit2 has `sign_pk` |
  | 0 / 32 | `onion_pk` (Tor mode, §28) |
  | 0 / 32 | `sign_pk` (Ed25519, §7.1) |
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
- **As built:** "My contact card" (saved identities) shows the card as a QR code and a `#k=` link; the first card is created when it is first shown (30 days, or "never" for a new card), and the secret lives in TLV 0x04. A contact added from a card keeps the card's secret (entry flag bit3 `FROM_CARD`, 16 bytes) until its first Tor connection went through; later dials are plain contact dials. Adding needs a saved identity, refuses an expired card and our own card. An existing contact keeps its name and verification (it only gains the onion key).

### 7.6 Moving an identity to another device (P2P, no cloud)

This is Telegram's "log in with a QR code", done without a server:

1. On the new device, choose "Receive identity from another device". It shows an invite with `flags.bit2 TRANSFER` set.
2. The old device, signed in with that identity, scans or pastes it. The two connect with the normal flow (direct mode, or Tor mode in §28).
3. **The SAS is mandatory**, even when both codes were scanned in person, because the whole identity is at stake.
4. After the SAS is confirmed on both sides, the old device sends its **encrypted key-file blob** (§7.3) in IDENTITY_CHUNK records (§11.2). The blob is still encrypted with the passphrase.
   - Protocol: the receiver (the device that made the TRANSFER invite) sends IDENTITY_READY (0x41) when its user confirms the SAS; the sender sends the IDENTITY_CHUNKs only after its own confirmation **and** IDENTITY_READY. Chunks on any other link, or before that, are `E_NOT_PERMITTED`. A transfer link carries no chat.
5. The new device asks for the passphrase. It decrypts the blob and offers to remember it in a slot, and to download a backup.
6. The old device then offers "Keep this identity here" (**the default**) or "Remove it from this device".

- There is **no sync afterwards**: each device holds its own copy.
- Two devices using the same identity **at the same time** in the same 1:1 or room are refused. The second session gets `E_DUPLICATE_SESSION` from the peer or the room owner, who sees two live sessions with one `PeerId`.

## 8. Rendezvous

### 8.1 The exchange is always two-way

Browser WebRTC cannot finish DTLS or ICE without the remote description (§25.1 R1). Every new pairwise link therefore needs exactly one **invite → answer** exchange. The first link is done out of band. Later links inside a room are signalled through the owner (§14.4).

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
| Full link `https://<host>/ephem/#i=` + payload | ≈ 290 characters, which is a QR of about version 12-M (byte mode) |

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
- **Transport:** Noise CipherState semantics. snow runs only the handshake; its raw split keys (`risky-raw-split`) then drive `chacha20poly1305` directly, because snow's transport API cannot take associated data and §11.1 authenticates the frame header. Nonce encoding (4 zero bytes ‖ LE counter) and `REKEY` are exactly Noise's, so the wire is still standard Noise ChaChaPoly. Encryption and decryption are in place in the frame buffer. The nonce is an implicit 64-bit counter. The DataChannel is ordered and reliable, so the receiver expects exactly `n+1`. Anything else causes `E_CRYPTO_FAILED` and closes the connection.
- **Rekey:** `REKEY` (Noise `rekey()`) every 2^20 messages or 10 minutes, whichever comes first.

### 10.2 Why two layers

- DTLS protects the transport.
- Noise gives: (a) identity that continues across new `RTCPeerConnection`s and DTLS certificates; (b) authentication that does not depend on the transport, for owner-relayed signalling (§14.4) and future transports; (c) a clear, pinned root for the SAS.
- For chat, the cost of double encryption is negligible.

### 10.3 Groups (MVP-3)

No group key (owner decision v0.9, replacing MLS). A room is a full mesh of the pairwise Noise KK links of §10.1, and **every message is encrypted separately on each link**. What makes the set of links a room is the **owner-signed room state** (§14.2): room id, version and the member table (index, role, `PeerId`, Ed25519 key), signed with Ed25519 over `"ephem-room-state/1" ‖ state`. Members verify it with the signing key the owner sent in HELLO on their own Noise link, which is bound to the owner's pinned static key.

Signalling between two members travels through the owner, **sealed** (§14.4): ChaCha20-Poly1305 under `HKDF-BLAKE2s(X25519(static_a, static_b))`, with a random nonce and `room_id ‖ from ‖ to` as associated data. The owner forwards the box and can neither read nor alter it.

Why not MLS: with one committer and no delivery service MLS adds a group key, epochs and a 100 KB+ dependency, while each message must still be sent on every link (no forwarding, §14.3). Pairwise Noise already gives per-link forward secrecy and rekeying; removing a member is just closing its links, after which it receives nothing (post-compromise security by construction).

### 10.4 Short authentication string (SAS)

- `s = BLAKE2s("p2pchat-sas" ‖ handshake_hash)`. It is shown two ways:
  - **6 decimal digits** = `u32::from_le_bytes([s[0], s[1], s[2], 0]) % 10^6` (24 bits, negligible bias);
  - **4 emoji**, one per byte of `s[3..7]`, from a fixed 256-entry table: the contiguous Unicode block U+1F400..U+1F4FF (animals and objects), entry `b` = `U+1F400 + b` followed by U+FE0F (emoji presentation).
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
| 1 | 1 | `ftype`: `1` NOISE_HS, `2` NOISE_TRANSPORT (rooms use the same frames, §10.3) |
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
| 0x04 | PING / 0x05 PONG | t_ms u64; PING adds `hidden u8` (1 = the sender's page is hidden, §12) | MVP-1 |
| 0x06 | GOODBYE | ErrorCode u16 | MVP-1 |
| 0x07 | REKEY | – | MVP-1 |
| 0x08 | TYPING | `u8` state (0 = stopped, 1 = typing) | MVP-1 |
| 0x09 | READ | `chat_seq u64`, cumulative: **read** | MVP-1 |
| 0x0A | EDIT | `target_seq u64`, new UTF-8 text (≤ 4 096 B). Own messages only | MVP-1 |
| 0x0B | DELETE | `target_sender u8`, `target_seq u64`. Own messages; the room owner may delete any (§11.7) | MVP-1 |
| 0x0C | REACT | `target_sender u8`, `target_seq u64`, `len u8` + emoji (0–32 B UTF-8; 0 = remove) | MVP-2 |
| 0x40 | IDENTITY_CHUNK | `idx u16`, `total u16`, up to 12 KiB of the encrypted key-file blob. Only valid on a `TRANSFER` link after the SAS is confirmed (§7.6) | MVP-2 |
| 0x41 | IDENTITY_READY | empty: the receiving device's user confirmed the SAS (§7.6) | MVP-2 |
| 0x10 | SIGNAL_OFFER / 0x11 SIGNAL_ANSWER | `n_cand u8` + ICE body (ufrag, pwd, DTLS fingerprint, candidates; the §8.3 layout without ids): in-band ICE restart of this link | MVP-2 (T1) |
| 0x30 | ROOM_STATE | the owner-signed room state: `room_id [16]`, `version u32`, `n u8`, n × (`idx u8`, `role u8`, `peer_id [32]`, `sign_pk [32]`), `sig [64]`. Owner → member only; strict decoding (§14.2) | MVP-3 |
| 0x31 | ROOM_SIGNAL | `from u8`, `to u8`, sealed box (§10.3) of a §8.3 code (invite, answer, resume invite, resume answer) for the `from`→`to` member link. Member → owner → member; the owner checks that `from` is the member on the link it came in on | MVP-3 |
| 0x32 | ROOM_LEAVE | empty: the member is leaving (the owner removes it and re-signs) | MVP-3 |

Room records are accepted only on links made from a `GROUP` invite. Unknown `rtype` values are ignored if `rflags.bit0` (IGNORABLE) is set. Otherwise they cause `E_PROTOCOL_MISMATCH`.

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
- HELLO carries `ver_min..ver_max` and a capability bitset: bit0 group, bit1 resume, bit2 in-band restart, bit3 **sends read receipts**, bit4 **sends typing**, bit5 Tor, bit8 **the sender scanned the peer's code in the app** (SAS policy, §10.4), and so on. The session uses the intersection of the protocol bits. Bits 3 and 4 are the peer's privacy settings (§11.7).

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
| Room control (setup path) | ROOM_STATE / ROOM_SIGNAL / ROOM_LEAVE body → room inbox | **1 copy** | The room layer handles a record after the core call that produced it returns (it touches other links); these are admissions, introductions and reconnects, never chat |
| Room control (setup path) | Relayed code → sealed box → ROOM_SIGNAL body | **2 copies** (≤ 400 B) | Sealing writes a new buffer; the owner forwards the body as received. Once per introduction or T2 attempt |
| Room chat | One message to N members | N encryptions | Inherent to pairwise links (§10.3): each link encrypts in place from the same text slot; no plaintext copy |

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

- `AWAITING_ANSWER` times out at `expires_at` with `E_EXPIRED_INVITE`. There is **no retry before the first connection** (§25.1 R7).
- **Liveness:** ICE consent freshness (RFC 7675) is the authority on the path. The application PING is sent every 15 s when idle, and 2 missed PONGs lead to `DEGRADED`. This catches frozen or backgrounded tabs.
- **Throttled background tabs (E8):** browsers delay JS timers in hidden tabs (Safari: gaps of about 100 s were measured), while ICE consent and SCTP keep-alives run in the browser's network stack and are not throttled. So: (a) the PING interval is best-effort; (b) a peer's missed PONGs lead to `DEGRADED` only after **120 s**, not 30 s, unless ICE itself reports `disconnected`; (c) PING carries a `hidden` flag (`document.visibilityState`), so the other side shows "in background" instead of "connection problem".
- **Retry backoff** in `RECOVERING`: 1 s, 2 s, 4 s, 8 s, capped at 15 s, with ±20 % jitter.

## 13. Network migration: the recovery ladder

Honest baseline: when a device's **only** network changes, all of its links drop at once. No in-band channel survives to carry an ICE restart.

| Tier | Mechanism | Signalling | Preconditions | Phase |
|---|---|---|---|---|
| **T0** | The ICE agent switches to an already-validated backup pair, or continual gathering finds a peer-reflexive path | None | A second interface was gathered at connect time, or the stationary peer is directly reachable *(spike S3)* | Free (browser behaviour) |
| **T1** | ICE restart. SIGNAL_OFFER/ANSWER travel over the **still-open** DataChannel, encrypted by Noise. The re-offer is rendered with the Appendix A template, the `o=` version increased and the DTLS roles of the connection kept (the path's answerer stays `active`, so an answer from the other side is rendered `passive`) | In-band | The path is `disconnected` for 2 s, the browser reports a network change (`online`, `navigator.connection`), or the user taps "Restart ICE" in the diagnostics | MVP-2 |
| **T2** | A **resume code pair relayed through the room owner**, sealed end to end (§14.4): the member with the greater `PeerId` sends a RESUME_INVITE as ROOM_SIGNAL, the other answers the same way; Noise KK rebinds as in T3. Automatic, retried every 15 s | Peer-relayed | Room, a member-to-member link is `SUSPENDED` while both members' links to the owner are up | MVP-3 |
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

### 14.2 Room authority: the owner-signed state

- The room creator is the **owner**, member index **0**, and the only authority:
  - only the owner creates invites and admits members;
  - only the owner removes members;
  - only the owner signs the **room state** (§10.3, ROOM_STATE §11.2). Every admission, removal or departure produces a new state with a higher `version`, sent to every member.
- A member verifies each state strictly: the signature with the owner's HELLO key, index 0 = the owner of its own link (key and `PeerId`), indices unique and < 16, known roles, no trailing bytes, the room id of its invite, and a higher version than the last. A state without the member means it was removed.
- There is **no succession**. When the owner leaves, the room is **disposed** (§14.6).
- **Roles:** `owner` (exactly one), `member` (reads and writes), and **`observer`** (read-only).
  - Only the owner assigns roles, by the kind of invite (`OBSERVER` flag bit3). Changing a role later is not part of MVP-3.
  - The role is in the signed state, so every member knows every role.
  - Roles are **enforced by every receiver** (§11.7): observers' CHAT, EDIT, DELETE, TYPING and REACT are dropped (but acknowledged); only the owner may delete someone else's message or set the timer.
  - Observers are full mesh members for networking. **They see the other members' IP addresses, and the other members see theirs** (§29.2).
- **Admission:** the owner's GROUP invite is answered as in §8. Once the joiner's HELLO (with its signing key) arrives, the owner assigns the lowest free index, fixes the link's indices and roles, and sends the new state to everyone. A second tab of an identity already in the room is refused (`E_DUPLICATE_SESSION`); a 17th person gets `E_ROOM_FULL`.
- Members verify the owner by the SAS of their link to the owner; the owner sees every member's SAS in its member list. Trust in the member table is trust in the owner: a malicious owner can admit anyone, but cannot forge a member's key (links are Noise KK with the key named in the state).

### 14.3 Propagation

- The sender sends each message **directly** to every connected member. There is **no flooding and no forwarding** (§25.1 R6). Forwarding is deferred and is not part of any current phase.
- Messages are identified by `(sender index, chat_seq)`. Each sender numbers its messages once for the room and uses **the same `chat_seq` on every link**; a link opened later starts after the current number, and a receiver accepts gaps. Replies, edits, deletes and reactions name `(sender, seq)`, so they work across members. Deduplication is per link (each sender's messages arrive only on its own link).
- Delivery is shown as "✓ k/N" from the per-link cumulative ACKs. Read receipts and typing are off in rooms.
- The room timer is set by the owner; members stamp their messages with it on every link.
- A member without a direct link to the sender does not receive the message. The UI shows that link as missing ("no direct path").

### 14.4 Joining (introductions by the owner)

1. A new member M does the out-of-band exchange with the **owner** O (§8), from a GROUP invite. The app warns before answering that every member will see M's IP address (§29.2).
2. O admits M (§14.2) and sends the new signed state to everyone.
3. M is asked "Connect directly to N other members?" (a member who joins alone is not asked again later). Of every pair M, Y the side with the **greater `PeerId` offers**: it builds a normal invite code for the M↔Y link, seals it to the other's static key with `room_id ‖ from ‖ to` as associated data, and sends it as ROOM_SIGNAL to O, who forwards it to the addressee. The answer comes back the same way. Each side checks that the code's static key is the one the signed state names for that member. This is signalling only, never chat.
4. The M↔Y link comes up directly (Noise KK). If ICE fails, that pair stays unlinked, both sides show "no direct path", and the introduction is retried every 15 s.
5. T2 (§13) uses the same relay for resume codes.

What the owner learns: that M and Y are being introduced and the size and timing of their codes, not the codes (candidates, fingerprints) themselves. Sealed signalling has no forward secrecy of its own (static-static key); it carries only ICE parameters, and the chat keys come from the fresh Noise KK handshake.

### 14.5 Group key management

- None (v0.9): each link has its own Noise KK keys, rekeyed per §10.1. Nothing is encrypted to "the room".
- A newcomer receives only messages sent after its links come up; a removed or departed member's links are closed, so it receives nothing more.
- Removal: the owner sends the removed member the state without it, closes its link (GOODBYE), and sends the new state to everyone else, who close their links to it.

### 14.6 Disposal

The room is disposed when either of these happens:

- the owner leaves on purpose (GOODBYE to every member), or closes the room;
- a member's link to the owner closes: a GOODBYE, or `SUSPENDED` for longer than the grace period (10 min, §12). While the owner is unreachable, members keep chatting on their existing links and can reconnect to the owner with a T3 code, but nobody can join or be removed.

On disposal, every member's core:

1. shows "The owner closed the room" (`E_ROOM_DISPOSED`), or "You were removed from the room";
2. closes all links in the room;
3. drops the room state, the Noise states and all messages.

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

- `index.html` is the root of trust, pinned by the service worker. `tools/stamp.py` (run by `./build.sh`) writes into it: SRI on `app.css` and `app.js`; an **import map with `integrity`** for every JS module (`app.js`, `pkg/ephem.js`), whose own SHA-256 is allowed in the CSP `script-src`; and the SHA-384 of `ephem_bg.wasm`, which `app.js` passes to `fetch(…, { integrity })`. A tampered wasm or `app.js` is refused (APP-E2E). Browsers without import-map integrity still check `app.js` and the wasm.
- The build id (first 12 hex digits of SHA-256 over the stamped hashes) is the service-worker cache version and is shown in the app footer.
- Builds are reproducible (pinned toolchain, `--remap-path-prefix`). The hash of each build is published in GitHub Releases and shown in the app's About screen.

### 17.3 CSP (set in a `<meta>` tag, because GitHub Pages cannot set headers)

```
default-src 'none'; script-src 'self' 'wasm-unsafe-eval'; style-src 'self';
img-src 'self' data: blob:; connect-src 'self' https:; worker-src 'self';
manifest-src 'self'; media-src 'self' blob:; base-uri 'none'; form-action 'none'
```

- `connect-src 'self' https:` (v1.2, decision D5): a page fetches only a Snowflake broker (Tor mode, and the direct page's channel tabs, which load the Tor build) and, on request, a public IPFS gateway; the broker may be the user's own (bridge lines, Appendix F.2). WebRTC, which carries chats and Snowflake proxies, is outside `connect-src` anyway (below), so a stricter list protected little; `script-src` stays hash-pinned with no inline scripts, which is the real barrier.
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
| 0x0021 | E_CRYPTO_FAILED | Noise failure, nonce gap, or bad tag |
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
| 0x0044 | E_TOO_MANY_CHATS | The tab already holds 16 chats and rooms (Appendix F.3.2) |
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
| Argon2id (key file) | m = 19 MiB, t = 4, p = 1 |
| App ping interval | 15 s idle |
| Suspended grace | 10 min |
| Rekey | 2^20 messages or 10 min |

## 21. Security model

**Protected:**

- message confidentiality and integrity against network observers and against the static host;
- peer authentication, pinned to the out-of-band exchange and optionally SAS-verified;
- room membership (owner-signed state, verified by every member, MVP-3), and member-to-member signalling relayed by the owner (sealed, §14.4);
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
| Malicious peer | Fake identity, injection, replay, joining, abuse of membership, relay candidates | Keys pinned by the invite, AEAD with strict nonces, owner-only admission and signed room state, relay filtering |
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

## 23. Plan

### 23.1 Phases

| Phase | Scope | Depends on |
|---|---|---|
| **MVP-1** | Sign-in (temporary identity, or saved identity with an encrypted key file); 1:1 in direct mode; two-way exchange by QR, link and paste; binary codes with SDP reconstruction; STUN defaults, privacy modes and **Drop IPv6**; relay prohibition; Noise KK; SAS policy; T3 resume code; **pending queue and ticks; replies, edit, delete, self-destruct timers; typing and read receipts**; **IP disclosure features (§29.2)**; basic diagnostics; CSP, SRI and a version-pinned service worker; desktop browsers and iOS Safari; **a static landing page** (what the messenger is, how it works, privacy claims as allowed by §21, and a **Start** link to the app) | Checkpoints S1, S2, S4 on all engines |
| **MVP-2** | T1 in-band ICE restart and `NetChanged` handling; full diagnostics; **contacts; several identities (IndexedDB slots, Web Locks); identity transfer over P2P; reactions** | MVP-1; S3 |
| **MVP-3** | Owner-controlled rooms of up to 16 members, with **observer role** and owner moderation (delete); introductions by the owner with sealed signalling; owner-signed room state (v0.9, no MLS); room disposal; T2 recovery through the owner | MVP-2; S9 |
| **CARDS** | Contact cards and card secrets | MVP-2. **Built**: APP-E2E-CARDS 8/8, APP-E2E-TOR-CARDS (lab) |
| **TOR-1** | Embedded Tor in WASM (Appendix C steps E2–E7): runtime shim, Snowflake transport in Rust (KCP + smux), IndexedDB directory cache, onion hosting from a tab | Gates G2–G4. **Built (lab)** |
| **TOR-2** | Tor mode for 1:1: transport guard, TOR_INVITE (one-way), Noise IK, stream framing, "VIA TOR" UI | TOR-1. **Built (lab)** |
| **TOR-3** | Contacts reconnect through stable onion addresses (no QR); card-based Tor dial | TOR-2, CARDS. **Built** |
| **TOR-4** | Rooms over Tor | TOR-3, MVP-3. **Built (lab)** |
| **CH-1…CH-5** | Public channels with a Tor-only owner, hosted from the owner's tab (Appendix D) | TOR-1, E8. **Built (lab)**: APP-E2E-TOR-CHANNEL 19/19 |
| **BR-1…BR-3** | Tor bridge lines (Appendix F.2) | TOR-1. **Built (lab)**: APP-E2E-TOR-BRIDGES 9/9 |
| **UI-1…UI-6** | Several chats per tab; one app with Chats, Following and My channels (Appendix F.3) | CH-5. **Built**: APP-E2E-MULTI 12/12, APP-E2E-TOR-MULTI 6/6 (lab) |
| **Deferred** | Wallet authentication; peer forwarding of chat; file transfer; voice and video; rooms larger than 16 | — |

### 23.1a Release and site layout

- **The repository stays private, and all work stays on the development branch, until Tor mode is fully implemented** (owner decision 2026-09-28, superseding "after MVP-1" and "after MVP-3"); then it is merged to `main`, made public and GitHub Pages is switched on (Settings → Pages → Deploy from a branch, repository root; `.nojekyll` is already present).
- **Pages layout** (project site `https://<owner>.github.io/ephem/`):

  | Path | Content |
  |---|---|
  | `/` | **Landing page** (static HTML, no scripts needed): **Ephem**: what the messenger is, how a chat starts (two codes, in person or by link), what is and is not protected (§21), supported browsers, and a prominent **Start** link to `/app/` |
  | `/app/` | The PWA: `index.html`, `app.js` (chats and the shell), `channels.js` (Following, My channels), `bridges.js`, `slots.js`, `ui.js`, `app.css`, `sw.js`, `manifest.webmanifest`, `icons/`, `pkg/ephem.js` + `pkg/ephem_bg.wasm` (built and stamped by `./build.sh`, committed) (§4.1); the channel tabs load the Tor build on first use |
  | `/app/tor.html` | Tor-mode entry (§28.2, §28.6): generated from `index.html` by `tools/stamp.py`, loads `pkg/ephem_tor.js` + `pkg/ephem_tor_bg.wasm` (chats and channels). Channel links: `tor.html#c=<name>&o=<onion>[,<mirror>…]` |
  | `/app/channel.html` | Old channel links (v1.1): a redirect into the app (`redirect.js`) |
  | `/checks/web/` | The checkpoint page (§24) |
  | `/docs/P2P-CHAT.md` | This document |

- The landing page follows the same CSP rules (§17.3), uses no third-party fonts, scripts or analytics, and is part of the MVP-1 definition of done.

### 23.1b Plan: CARDS and public channels (started 2026-09-29)

The owner's decision: implement CARDS and CH-1…CH-5, then merge (§23.1a). Each step ends with its proof, a commit and a status line in §24.2.

| # | Work | Proof | |
|---|---|---|---|
| K-1 | `proto::card`: CONTACT_CARD (kind 6) codec, strict; `#k=` links | unit tests (round trip, truncation, bad nickname) | ✅
| K-2 | Key file TLV 0x04 CARD (secret, expiry) kept with the contacts; contact entries gain flag bit3 + the 16-byte `card_secret` of the card they were added from (used for the first Tor dial only) | crypto tests (round trip, reset) | ✅
| K-3 | Core: contact dial carries a `card_secret` instead of the zero `invite_id`; the host accepts it only if it equals its current, unexpired secret, from any key (case 3 of §28.4) | core test (card accept, wrong/expired secret refused) | ✅
| K-4 | Adapter + UI: "My contact card" (QR/link, expiry 30 days or never, reset), "Add from card" (unverified contact, suggested nickname), Tor "Connect" to a card contact, the host's prompt "X (from your contact card) wants to connect" (accept = contact saved, decline = closed); direct mode: the card only pins key and nickname | e2e direct (add from card, impersonation warning, SAS then ✔) and Tor lab (first dial via card, prompt, later contact dials) | ✅
| C-1 | `crates/channel` (sans-IO): channel keys (§D.3), dag-cbor subset, CIDv1 (sha2-256), CAR v1 read/write, IPNS V2 record create/verify (protobuf + dag-cbor data + Ed25519 V2 signature), manifest/page/post blocks and their signatures | unit tests; IPNS record verified against a record produced by the Go reference (boxo) as a test vector; CAR read back | ✅
| C-2 | Channel store in OPFS (`channels/<name>/`, `persist()`), CAR export/import; an HTTP/1.1 read-only gateway subset (`/ipfs/<cid>?format=car|raw`, `/ipns/<name>?format=ipns-record`) served on the channel onion from the tab (a second onion service: C-P2) | e2e in the lab: a second browser fetches and verifies the channel over the onion | ✅
| C-3 | `channel.html` (own CSP, own Tor build `ephem_channel`): owner creates a channel (warnings of §D.3), posts, deletes; optional IPNS PUT to `delegated-ipfs.dev` through a Tor exit | e2e lab: create, post 3, delete 1, record sequence grows | ✅
| C-4 | Reader: `channel.html#c=…&o=…` over Tor; without Tor, through `trustless-gateway.link` for IPFS-mirrored channels; high-water mark of the record sequence | e2e lab reader; C-P3 (1 000 posts) timing | ✅
| C-5 | Mirrors: "Mirror this channel" into OPFS and serve it on the follower's mirror onion, refresh every 10 min (higher sequence only); signed mirror list in the manifest; Kubo instructions + CAR/record download | e2e lab: owner offline, reader reads from the mirror | ✅

### 23.2 Order of work

```
now ──▶ checks/run_all.sh on a laptop ──▶ MVP-1 ──▶ MVP-2 ──▶ MVP-3
              │                                     │
              └──▶ G2 live ──▶ TOR-1 (E3–E7) ──▶ TOR-2 ──▶ TOR-3 ──▶ TOR-4
                                    │
                                    └──▶ CH-1…CH-5 (needs E8)
```

MVP-1 does not depend on Tor. The Tor track runs in parallel and is dropped cleanly if a gate fails (§23.3).

### 23.3 Gates

| Gate | Criterion | Status | If it fails |
|---|---|---|---|
| G1 | arti builds for `wasm32` without forking its core | ✅ Passed at compile level (E1) | — |
| G2 | Snowflake broker rendezvous and a DataChannel to a proxy work from a browser, on desktop **and** iOS Safari | ✅ **Passed** (2026-09-28): Chrome 153 and Safari 26.5 on macOS; **iPhone Safari (iOS 18.7)**: rendezvous 0.5–0.7 s, DataChannel open 1.4–2.2 s, through both the CDN URL and the direct broker | No Tor mode; no public channels; IP privacy via VPN/WARP only |
| G3 | Tor bootstrap ≤ 60 s cold, ≤ 10 s warm; an onion connects on desktop and iOS | ✅ **Lab, Chromium:** bootstrap 5.5–17 s, a tab's onion reached in 1–12 s. **Live network (2026-09-28, macOS, Chrome, 18/18):** ✅ **warm 7.4 s**; ❌ cold 283 s in one tab (the other was up far sooner): directory downloads stalled when a Snowflake proxy died, arti's 10 s read timeout failed them and marked the single bridge down for 2.5–11 min. Fixed afterwards (both Tor Project bridges, stall detection in 4 s): ✅ **cold 47 s** in the second live run (18/18; both bridges used, no bridge marked down). First onion dial 55–57 s (one attempt timed out, now 30 s), later ones 2–13 s, messages ~0.8 s. ✅ **Laptop ↔ iPhone chats and rooms over Tor work** (owner, 2026-09-29). ⏳ iOS memory | Same as G2 |
| G4 | Tor build ≤ 5 MB compressed; no iOS memory kills; onion hosting from a tab works | ✅ **Size: 2.08 MB gzip** (5.49 MB raw, before `wasm-opt`). ✅ **Onion hosting from a tab** (lab). ⏳ iOS memory | Client-only Tor (dial, no hosting): no one-way invites from Tor hosts in tabs, no channels |

## 24. Checkpoints

### 24.1 How to run

```sh
SAFARI=1 ./checks/run_all.sh home-wifi        # installed Chrome + your real Safari, incl. Chrome↔Safari (recommended on macOS)
./checks/run_all.sh home-wifi                 # installed Chrome only (arti build is the slow part)
BROWSERS=chrome,firefox,webkit ./checks/run_all.sh all-engines   # adds Firefox (downloaded once)
./checks/run_all.sh warp-on                   # again with Cloudflare WARP / your VPN on (TS3)
E8=1 ONLY=browser ./checks/run_all.sh e8     # only the browser checks + the 7-minute hidden-tab test (visible Chrome window)
IPHONE=1 ONLY=iphone ./checks/run_all.sh iphone   # iPhone via a free Cloudflare quick tunnel + QR code (works while the repo is private)
E8REAL=1 ONLY=e8real ./checks/run_all.sh e8   # E8 in your real Chrome, Safari and Firefox at once (macOS): each opens in E8 mode;
                                               # switch to another tab in each for ≥ 6 min, then come back
BROWSERS=chrome,firefox ONLY=browser ./checks/run_all.sh firefox   # Firefox (brew install node@22; picked up automatically)
NET=0 SKIP_ARTI=1 ./checks/run_all.sh quick   # offline, fast
./build.sh && ONLY=tor ./checks/run_all.sh tor-live   # Tor mode on the real Tor network (T-10): 1:1, contacts, rooms
```

- **Needs:** Node ≥ 20, Python ≥ 3.9, Rust (rustup). For E1 only: an LLVM clang with the WebAssembly backend (Linux `clang`; macOS `brew install llvm`, because Apple's clang has none). Runs on macOS or Linux; on Windows, use WSL2.
- **`SAFARI=1`** also runs the page in your real Safari, **including cross-engine S1** (Chrome ↔ Safari in both directions, exchanging only the minimal fields through a local mailbox).
- **`ONLY=`** limits a run to some sections: `app, tor, stun, gateways, browser, safari, iphone, sizes, argon2, arti`. `ONLY=tor` runs the Tor-mode e2e tests on the real Tor network (and in the offline lab when `checks/tor-lab/lab.sh up` is running). `ONLY=app` runs the MVP-1 native tests and the end-to-end chat test (`checks/e2e_app.mjs`) in your installed Chrome.
- **Two real devices while the repo is private:** `node checks/serve_app.mjs` serves the landing page and `/app/` from the laptop through a free Cloudflare quick tunnel (`brew install cloudflared`) and prints the HTTPS address and a QR code. Only the static files use the tunnel; the chat is direct WebRTC. Use it for S3, S10 (two devices) and the iPhone app.
- **Building the app:** `./build.sh` (needs the `wasm32-unknown-unknown` target and `wasm-bindgen-cli` 0.2.129; uses `wasm-opt` if present) writes `app/pkg/`. Native tests: `cargo test --workspace`.
- **iPhone while the repo is private:** `IPHONE=1` serves `checks/web/` from the laptop through a free Cloudflare quick tunnel (`brew install cloudflared`, no account), prints a QR code, and collects the results automatically. `S6_SECONDS` sets the S6 target (default 120 s).
- **iPhone once the repo is public (G2, S6, S4):** open **https://darkcite.github.io/ephem/checks/web/** in Safari. Tap **1** (automatic checks), **2** (S6: leave the app for about 60 s, then come back), **3** (S4: allow the camera), then **Share** or **Copy** the results. The page is `checks/web/` served by GitHub Pages from this branch (repo root, with `.nojekyll`). Refresh the test IPNS record in `checks/web/config.json` with `node checks/make_web_config.mjs`.
- **Output:** `checks/out/<timestamp>-<label>/REPORT.md`, plus raw JSON and logs.

### 24.2 Checkpoint list and results

Legend: ✅ passed · ⚠️ caveat · ❌ failed · 🔬 established from source code · ⏳ open · 🤖 automated in `checks/run_all.sh` · ✋ manual.

| ID | Question | Result so far (container, 2026-09-28) | Run | Gates |
|---|---|---|---|---|
| S1 | Does the Appendix A template SDP connect, in every offerer × answerer browser pair? | ✅ **All tested pairs, both directions:** Chrome 153 ↔ Chrome, Chrome ↔ Firefox 142, Firefox ↔ Firefox, Safari 26.5 ↔ Chrome; Safari and iPhone Safari (iOS 18.7) in one tab. Raw-IP and mDNS candidates. The template's fixed `max-message-size:262144` is accepted by Firefox, which itself advertises 1 073 741 823 | 🤖 | §8 |
| S2 | Real code sizes per browser | ✅ Chrome, Safari and iPhone: ufrag 4, pwd 24 → invite **153 B** (mDNS). **Firefox 142: ufrag 8, pwd 32 → invite 165 B** (§8.5 budget assumed this worst case). All: `actpass`/`active`, mid 0, sctp-port 5000. Raw SDP 584–738 B | 🤖 | §8.5 |
| APP-E2E | Does the MVP-1 app work end to end in a real browser? | ✅ **Laptop (macOS arm64, installed Chrome, WARP on), 41/41, 2026-09-28:** real STUN; invite **212 B** (mDNS + srflx-v4 + srflx-v6); the "what your peer sees" panel lists **only WARP egress addresses** (104.28.163.34, 2a09:bac5:…), which is TS3 confirmed inside the app; the selected path was `prflx ↔ host`, RTT 2 ms. ✅ **Chromium (container), 41/41** (`checks/e2e_app.mjs`, two browsers): landing → app; **identity** saved as key file, wrong passphrase refused, signed back in, same identity in a second tab refused (Web Lock); invite 141 B (2 raw host candidates; no STUN reachable), "what your peer sees" panel; Bob **scans the invite with a camera** (fake camera playing the real QR; rqrr in wasm, the iOS path); answer link opened in a **new tab** is handed to the inviting tab (S7 on one device); Noise KK, **identical SAS**; chat both ways (Unicode, 4 096 B), ✓ then ✓✓, typing, reply with quote, edit, delete for everyone, **self-destruct 5 s** removes the message on both sides; path diagnostics via getStats (no relay, addresses hidden by default); **simulated network loss → message queued 🕓 → reconnect codes (T3) → delivered and ✓**; leave; invalid code; **offline start from the service worker; a new build waits and activates only on consent; tampered wasm and tampered `app.js` refused (SRI)**; no CSP violations or page errors. Native: 33 unit tests (proto, crypto incl. key file, core incl. two sessions over paths, tamper, replay, resume binding; QR render → decode) | 🤖 (`ONLY=app`) | §7, §8, §10–§13, §17, §29 |
| APP-E2E-MVP2 | Do the MVP-2 features work end to end? | ✅ **Chromium (container), 18/18** (`checks/e2e_mvp2.mjs`): identity saved and remembered in a slot, page reloaded, signed in from the list (nickname kept in the key file); peer nickname shown as self-chosen; contact saved after the SAS → "Bobby ✔", backup flagged out of date; reaction round trip; diagnostics (Noise epoch, rekey timer, ICE/DTLS/channel state); **in-band ICE restart with the chat continuing**; next chat with the verified contact skips the SAS prompt; **identity transfer to a new browser profile**: transfer invite → answer → equal SAS → both confirm → encrypted key file sent → unlocked on the new device with the same handle, label, nickname and contacts; the old device keeps it by default; no CSP violations | 🤖 (`ONLY=app`) | §7, §10.4, §11.7, §13, §18 |
| APP-E2E-ROOM | Do rooms work end to end? | ✅ **Chromium (container), 18/18, four browsers** (`checks/e2e_room.mjs`): owner creates a room; member B joins (owner sees B's SAS, equal to B's); member C joins, is asked before connecting, and is **introduced to B through the owner** (sealed ROOM_SIGNAL); observer D joins; full mesh of 4, all links direct; **B's member links dropped → back by T2 without user action**; observer read-only (no composer); a message reaches all three others with its sender label; "✓ 3/3"; C replies quoting B, seen correctly by the owner; owner deletes C's message for everyone (incl. C); owner sets the timer, members cannot; C removed (C told; others' lists updated); B leaves (others updated); owner closes the room (D sees "Room closed"); no CSP violations. Native: signed-state admit/verify/forgery/tamper, sealed-signal privacy and integrity, 3-party introduction via the owner, roles and moderation | 🤖 (`ONLY=app`) | §10.3, §11.2, §13, §14 |
| QR-CAM | Does the in-app scanner read a QR filmed from a screen? | ❌ → ✅ **iPhone PWA (2026-09-28): camera worked but nothing decoded.** Cause reproduced natively: rqrr (a quirc port) keeps at most 251 flood-filled regions, and blur + noise + uneven light along module edges produce enough speckle to evict the finder patterns (it decoded only 15 of 45 synthetic camera frames, failing even on some crisp ones). Fix: 3×3 denoise → local adaptive threshold → 3×3 majority filter before rqrr, plus a native-resolution centre crop on alternate frames and `BarcodeDetector` only when it lists `qr_code`: 34 of 45 decode, all at ≥ 5 px per module but one very low-contrast case. The e2e fake camera now plays a blurred, noisy, low-contrast QR. ✅ **Re-tested on the iPhone PWA (2026-09-28): scans the laptop's QR** | 🤖 + ✋ iPhone | §8.2, §17.5 |
| S3 | Path switch without signalling (T0) | ⏳ direct mode. **Tor mode ✅ (2026-09-29, laptop ↔ iPhone):** the iPhone switched Wi-Fi → mobile data; the chat reconnected by itself with no code ("Reconnected through Tor", pending messages delivered). The laptop saw the drop at once, the iPhone only after 3–4 min (arti/Snowflake timers) → fixed: a Tor link silent for 45 s is treated as lost and redialled on fresh circuits (APP-E2E-TOR: paused peer noticed in 45 s, back 6 s after resuming). ✅ **Re-tested on the devices: reconnects very fast now** | ✋ two devices | §13, §28.5 |
| S4 | Does camera permission disable mDNS obfuscation? | ✅ **Chrome 153 and iPhone Safari: yes** (raw IP after permission; stays after the camera stops). **Firefox 142: no** (mDNS even with the camera live). Safari without permission: mDNS. So the invite builder's own filtering (§9.4) is mandatory: two of three engines leak the LAN IP after the QR scanner is used | 🤖 / iPhone page | §9.4 |
| S5 | WASM sizes of the protocol crates | ✅ gzip (Linux and macOS agree within 1 %): Noise 38 KB; key-file crypto 131 KB; QR 33 KB; all three 176 KB; with openmls 291 KB. **The whole protocol stack builds with no C compiler** (verified with `CC=/bin/false`, and on macOS with Apple clang) once `snow` is used **without** its `std` feature, because `snow`'s `std` silently enables `ring` (C). Also: `hkdf::SimpleHkdf` for BLAKE2s | 🤖 | §22 |
| S5b | Argon2id cost in WASM | ✅ x86 server: m 19 MiB t 4 = 61 ms. **Apple Silicon (V8): t 2 = 21 ms, t 4 = 32 ms; m 64 MiB t 3 = 98 ms.** Memory 21 MiB (85 MiB at 64 MiB), never shrinks. The spec uses m 19 MiB, t 4 | 🤖 | §7.3 |
| S6 | iOS: does a pending connection survive the background? | ✅ **iPhone (iOS 18.7): pending offers survived 53 s and 182 s in the background** (have-local-offer, gathering complete), then connected and delivered a message. **The §17.5 fallback (iOS users always answer) is not needed**; invite TTL guidance: ≥ 3 min in the background is fine | iPhone page | §17.5 |
| S7 | Answer-link hand-off between tabs | ✅ same browser profile (APP-E2E). ⏳ iOS: Safari tab → installed PWA cannot use `BroadcastChannel` (§8.7 copy fallback) | 🤖 / ✋ iPhone | §8.7 |
| S8 | Default STUN servers dual-stack; srflx gathering | ✅ DNS: Google and Cloudflare have A + AAAA; Twilio A only. ✅ Live (macOS, home Wi-Fi): all three answer over IPv4 and both browsers get the same srflx-v4. That network has **no IPv6**, so srflx-v6 is untested | 🤖 | §9.3 |
| S9 | 15 connections on iOS | ⏳ | ✋ iPhone | §14.1 |
| TS3 | WebRTC through WARP or a VPN shows the VPN exit | ✅ **WARP connected (macOS): peers see only Cloudflare WARP egress addresses**: UDP `104.28.163.34` (and IPv6 `2a09:bac1:…`, which WARP adds), HTTPS `104.28.214.151`; the ISP address `171.97.169.36` never appears. WARP uses different egress addresses per protocol, so the check tests the address *range* (104.28.0.0/16, 2a09:bac0::/29). **Over WARP, Snowflake (E2), the IPFS gateway (C-P1) and browser IPNS publishing (C-P4) all pass.** ⚠️ Under WARP, the host-only S1 fails (two mDNS host candidates, Wi-Fi and the WARP interface; neither connects), so **LAN-only mode does not work with WARP**; the same-machine `S1 via srflx only` also fails, which is inconclusive (it needs WARP to hairpin). **Design consequence confirmed:** users cannot tell whether their VPN covers WebRTC, so the "what your peer sees" panel (§29.2) is essential | 🤖 | §29.1 |
| TS4 | IPv6 bypassing a v4-only VPN | ⏳ | 🤖 | §29.2 |
| S10 | NAT mapping type (IPv4): does one local socket keep the same public port toward different servers? | ✅ Measured (macOS, 2026-09-28, one socket, two servers). **Home ISP router: endpoint-independent (cone)**, same port `55298` for both, so direct P2P from home normally works. **Cloudflare WARP: address-dependent (symmetric)**, `50104` vs `49370`. **Consequence:** WARP hides the IP (TS3 ✅) but direct connections through it succeed only if the other peer is easy to reach (public IPv6, or a cone NAT that is not port-restricted). WARP ↔ WARP and WARP ↔ strict mobile carrier NAT are expected to fail with `E_NO_DIRECT_PATH`, because there is no relay (P8). **Tor mode does not depend on NAT type** (onion services only make outbound connections), so it is the robust IP-hiding option. The UI suggests it on `E_NO_DIRECT_PATH`, explicitly and never automatically. **Confirmed with MVP-1 (2026-09-28, laptop + iPhone via `checks/serve_app.mjs`):** without WARP they connect and chat; **with WARP on the laptop and the iPhone on home Wi-Fi they do not** (`E_NO_DIRECT_PATH`), as predicted for symmetric ↔ port-restricted NAT. **Across networks: laptop on home Wi-Fi ↔ iPhone on mobile data works, with and without WARP on the laptop.** So WARP fails only against a port-restricted home router; the carrier side is reachable | 🤖 | §9, §28, §29.1 |
| E1 | arti on `wasm32` (**G1**) | ✅ **Linux and macOS (arm64)**: arti 0.46.0, 16/16 crates, plus `arti-client` with onion client and service, ephemeral keystore, bridges, PT, rustls. Upstream has wasm stubs; `coarsetime` uses `performance.now()`. TLS: **ring** with `wasm32_unknown_unknown_js`. **Without the `compression` feature** (zstd/xz), **ring is the only C code**; it needs an LLVM clang with the WebAssembly backend (Linux clang; macOS `brew install llvm`). Extension point for Snowflake: `AbstractPtMgr` / `ChanMgr::set_pt_mgr` | 🤖 | §28 |
| E2 | Snowflake from a browser (**G2**) | ✅ **Live:** Chrome 153, Safari 26.5, **Firefox 142** (macOS) and iPhone Safari (iOS 18.7) get a proxy answer from the broker and open a DataChannel to a volunteer proxy, through **both** the CDN URL (`1098762253.rsc.cdn77.org`, no domain fronting needed) and `snowflake-broker.torproject.net`. Rendezvous 0.7–4.8 s; DataChannel open 2–6 s after start; sometimes the first tries report no proxy available, so retry. Protocol: `POST /client`, body `1.0\n{"offer": <JSON SDP>, "nat": "unknown", "fingerprint": <bridge fp>}`; stack: WebRTC → encapsulation → KCP → smux. ⏳ iPhone | 🤖 | §28 |
| E3 | Turbotunnel (KCP + smux) against the real Snowflake server | ✅ **Native interop** (`crates/snowflake/tests/interop.rs`): our client ↔ Go snowflake server v2.14.1 (as a proxy talks to it) ↔ echo, **10 MB both ways in 0.68 s, with a proxy switch mid-transfer** keeping the stream. ✅ Browser: lab broker → DataChannel → Go proxy → server. ⏳ 10 min against the real bridge | 🤖 lab | G3 |
| E4 | arti in the page over Snowflake: bootstrap, dial an onion | ✅ **Lab, Chromium** (`checks/tor-lab/e2e_tor.mjs`): bootstrap 5.5 s, lab onion echo 9.6 s; native twin (`crates/tor/tests/lab_native.rs`) 2.3 s. arti 0.46 upstream with four small wasm-only patches (`vendor/README.md`) | 🤖 lab | G3 |
| E5 | Onion hosting from a tab | ✅ **Lab:** a tab hosts an onion service (key from the seed); another browser, with its own arti and Snowflake, reaches it and they talk both ways (33 s first time, including descriptor publication) | 🤖 lab | G4 |
| E6 | Size, memory, warm start | ✅ Tor build **2.08 MB gzip** (direct build 232 KB). ✅ Warm start: the directory snapshot is kept in IndexedDB and a reload loads it ("Loaded a good directory from cache"; lab snapshot 18 KB, a real one is a few MB). ⏳ iOS memory and battery (device); warm-start time on the real network | 🤖 / ✋ iPhone | G4 |
| E7 | Fuzzing and review of our Tor-side parsers | ✅ `crates/snowflake/tests/fuzz.rs`: encapsulation, KCP, smux and the whole session take random and mutated server traffic (bit flips, inserts, drops, splices, extreme length fields) without panics, failing only with their typed errors; deterministic seeds, `FUZZ_ITERS` for longer runs. TOR_INVITE decoding is strict (exact length). Review items: §28.4 accept rules, transport guard (§28.5) | 🤖 | Sign-off |
| APP-E2E-TOR | Does 1:1 chat work over Tor, end to end? | ✅ **Lab, Chromium, 19/19** (`checks/tor-lab/e2e_tor_app.mjs`, two browsers each with its own arti; every rendezvous first tries a dead broker, exercising the fallback): mode routing (`#t=` → `tor.html`, `#i=` → direct page); Alice's onion up; **one 104-byte TOR_INVITE, no answer**; Bob dials, Noise IK, **same SAS**; messages both ways (50–120 ms in the lab), ✓; **stream lost on either side → the dialler redials, queued message delivered**; both save each other as contacts → later **Bob connects with no code** (after Alice signed out and in: her onion service follows the identity); **warm start**: the directory snapshot is in IndexedDB and a reload loads it; no CSP violations | 🤖 lab | §28 |
| APP-E2E-TOR-LIVE | The same 1:1 test on the real Tor network | ✅ **Third run: 5 proxy switches in the whole run** (58 before the stall-rule fix). ✅ **18/18 twice**; second run (after the bridge and stall fixes): cold 47 s, warm 7.6 s, redials 2.4 s, contact reconnect 2.6 s, snapshot 21.7 MB gzip. ✅ Laptop ↔ iPhone 1:1 and rooms over Tor by hand. First run (2026-09-28, owner's MacBook, installed Chrome, `LIVE=1`): real broker, Snowflake bridge and Tor network; Bob reached Alice's onion (55 s, after one timed-out attempt retried with a fresh descriptor), same SAS, messages ~0.8 s each way, both redials, contact reconnect 12.7 s, **warm start 7.4 s**. Cold bootstrap slow (see G3); real directory snapshot 40 MB as JSON, now stored gzip-compressed | 🤖 `ONLY=tor` | §28 |
| APP-E2E-CARDS | Do contact cards work (direct mode)? | ✅ **Chromium, 9/9** (`checks/e2e_cards.mjs`): card only for saved identities; QR + `#k=` link (30 days); a temporary identity cannot add; the Contacts field refuses what is not a card, then adds from a card; Bob adds Alice from the card (unverified, his own name for her); they chat by invite and answer, Bob sees his contact's name, the SAS is still asked, then ✔; reset makes a new card and flags the backup | 🤖 | §7.5 |
| APP-E2E-TOR-CARDS | Do cards open a first Tor chat? | ✅ **Lab, 6/6** (`checks/tor-lab/e2e_tor_cards.mjs`): Bob and Carol add Alice from her card; Bob's Connect carries the card's secret; Alice is asked (0.9 s) and sees no chat before accepting; accepted → contacts, messages both ways; after Alice resets her card Carol's old card opens nothing; Bob reconnects as a plain contact with no prompt | 🤖 lab | §28.4 |
| APP-E2E-TOR-ROOM | Do rooms work over Tor? | ✅ **Lab, Chromium, 9/9** (`checks/tor-lab/e2e_tor_room.mjs`, three browsers): owner invites B and C with TOR_INVITEs (no answer box); **B and C are introduced by the owner and connect onion to onion**; no IP-exposure prompt; a member's message reaches everyone; a member's links dropped → redialled, queued message delivered | 🤖 lab | §28.7 |
| E8 | Does a hidden desktop tab with an open DataChannel keep its timers? | ✅ **Chrome 153: yes** (604 s hidden, max gap 2.0 s). ✅ **Firefox 142: yes** (611 s hidden, max gap 1.5 s). ❌ **Safari 26.5: no**, confirmed twice (198 s hidden → 103 s gap; 604 s hidden → **150 s gap**). **Consequences:** (1) Tor mode and channel hosting (§27, §28) stay reachable from a background tab in Chrome and Firefox, but **not in Safari**, where the UI says "keep this tab visible" and recommends Chrome or Firefox for channel owners; (2) app-level PING timing tolerates throttled peers (§12) | 🤖 (E8=1) / E8REAL=1 | §12, §27, §28.3 |
| C-P1 | Gateways: trustless CAR with CORS | ✅ `trustless-gateway.link`: CAR served to Chrome, Safari and iPhone (119 874 B), CORS `*`. **Resolved:** `ipfs.io` and `dweb.link` answer trustless requests with a **301 redirect to `trustless-gateway.link` that has no CORS header**, so browsers refuse it. They are aliases, not independent gateways. **Default list: `trustless-gateway.link` only** (one operator: availability risk, §D.6.3) | 🤖 | App. D |
| APP-E2E-TOR-CHANNEL | Do public channels work end to end? | ✅ **Lab, 19/19** (`checks/tor-lab/e2e_tor_channel.mjs`, five browsers, v1.2 app): in Tor mode the owner's **My channels** uses the chat identity (no second sign-in, D3); warnings must be acknowledged; a channel, 3 posts, 1 deleted, online on its own onion (~5 s); **a second channel, both online at once** (UI-5); a reader verifies it over Tor (newest first, deleted shown) and **follows** it; a new post arrives when the Following tab refreshes; the second channel reads too; the reader mirrors the first on a second onion; the owner signs the mirror list, the reader gets version 7; **C-P3**: 1 000 more posts, read in 0.4 s; **IPNS record published through a Tor exit** (HTTPS PUT to a stand-in routing server with its own CA, body = the record); after a reload both channels come back from the store and go online again; backup exports; Kubo instructions and downloads; **without Tor**, read through a (stand-in) public gateway after the IP warning; **a direct-mode page loads the Tor build only when its Following tab is used** (no request before), then reads the channel (UI-3); **owner offline: a new reader gets it from the mirror**; no CSP violations | 🤖 lab | §27, App. D, F.3 |
| APP-E2E-TOR-BRIDGES | Do the user's own bridge lines work? | ✅ **Lab, 9/9** (`checks/tor-lab/e2e_tor_bridges.mjs`): a `#b=` link on the direct page opens Tor mode; with “my own bridges” chosen Tor waits for them; an obfs4 line is refused with its reason and Tor does not start; the lab Snowflake pasted as bridge lines starts Tor (onion hosted in ~6 s); “Share my bridges” gives a `#b=` link with the lines; the lines move into the key file on save, a reload waits and the sign-in starts Tor with them; a `#b=` link fills another user's setting without applying it; a chat runs through the pasted bridges | 🤖 lab | App. F.2 |
| APP-E2E-MULTI | Several chats in one tab (direct)? | ✅ **Chromium, 12/12** (`checks/e2e_multi.mjs`): Alice holds chats with Bob and Carol at once; each keeps its own messages, receipts and draft; a message in the background chat raises its unread count (row and tab), cleared on opening; Bob leaving ends only his chat; tabs, tab panels and the live log carry ARIA roles; a 390 px phone window shows the list first, a pane full screen with Back, no horizontal scrolling | 🤖 | App. F.3 |
| APP-E2E-TOR-MULTI | Several chats over one onion? | ✅ **Lab, 6/6** (`checks/tor-lab/e2e_tor_multi.mjs`): two Tor invites of one onion reach their own chats; a redial in Bob's chat leaves Carol's untouched; a contact dials while Alice is in another chat: a new chat appears in her list (Bob's stays on screen) and opens on a tap | 🤖 lab | App. F.3.2 |
| CH-INTEROP | Are our channel formats what IPFS software expects? | ✅ **6/6** (`checks/channel_interop.mjs`): the name is a libp2p-key CIDv1 (`k51…`); **the js-ipns reference validator accepts our records** (V1 + V2 fields); the record points at the CAR's root; every block hashes to its CID and decodes as strict DAG-CBOR in `cborg` | 🤖 | App. D |
| C-P2, C-P3 | Two onions in one tab; 1 000 posts | ✅ (App. D.10) | 🤖 lab | App. D |
| C-P5 | OPFS quota and eviction with `persist()` | ⏳ per browser, on devices | ✋ | App. D |
| C-P4 | Republishing signed IPNS records; browser PUT | ✅ **Live:** Chrome, Safari and **iPhone** `PUT` a signed record to `delegated-ipfs.dev` (200), and `trustless-gateway.link` serves back the **identical 397-byte record**. Kubo `name put` also accepts third-party records (source) | 🤖 | App. D |

**The offline Tor lab** (`checks/tor-lab/lab.sh up`, Appendix C.5) stands in for the Tor network; the live-network runs are T-10.

**The real Tor network from a container without UDP** (`checks/tor-lab/lab.sh relay`, 2026-09-29): cloud containers reach the Internet only through an HTTPS proxy, so WebRTC to real Snowflake volunteers is impossible there. The relay mode runs the Snowflake broker and four proxies locally (WebRTC stays on localhost) and has them relay to the Tor Project's Snowflake bridge (`2B280B23…`, `wss://snowflake.torproject.net/`) over WebSocket/TLS through the proxy. Everything after that hop is the real Tor network: consensus, onion services, HSDirs, exits. `lab.sh relay-verify`: a system `tor` bootstrapped in 30 s and check.torproject.org answered `IsTor: true`. `RELAY=1` runs the Tor e2e tests and spikes that way (lab-only steps skipped, as with `LIVE=1`). It needs `snowflake.torproject.net` allowed by the container's network policy; it does not test the Snowflake volunteers' WebRTC leg (the laptop runs, `LIVE=1`, do).

**Container limits (why some checks are still open):** no outbound UDP, no IPv6, and HTTPS only to an allow-list (crates.io and the Go proxy). The Snowflake broker, IPFS gateways and STUN were unreachable, and only Chromium was installed.

## 25. Decision log

### 25.1 Rejected claims of the original draft (v0.1)

| # | v0.1 claim | Why rejected | Replacement |
|---|---|---|---|
| R1 | Single-QR connection with no reply | The browser needs the peer's DTLS fingerprint and ICE credentials (from the answer) before it can connect, and it cannot import a pre-chosen certificate | Two-way exchange of ~150–190 B codes (§8). One-way invites exist only in Tor mode (§28.4) |
| R2 | No new QR after a network change | An ICE restart needs signalling, and the only channel dies with the path | Recovery ladder T0–T3 with a resume code (§13) |
| R3 | `chat://join/…` links | A PWA cannot register custom schemes | `https://…/#i=…`, using the fragment (§8.7) |
| R4 | CBOR + compression for invites | High-entropy data does not compress; SDP is mostly rebuildable boilerplate | Fixed binary layout + SDP template (§8.3, Appendix A) |
| R5 | Reusing candidates from other connections | No browser API exposes them | Removed |
| R6 | Flood-forwarding in a mesh | Multiplies traffic and turns peers into relays (P8) | Direct fan-out only; forwarding deferred |
| R7 | Retrying the first connection | Nothing to retry without a channel | Explicit failure with a new invite (§12) |

The v0.1 points that were **confirmed** are kept throughout: principles P1–P9, Rust owning the state, identity ≠ transport, no history, honest privacy claims (§21), full mesh with a cap, and MLS for groups (replaced by the owner-signed room state in v0.9, §10.3).

### 25.2 Owner decisions

| Version | Decision | Where |
|---|---|---|
| v0.3 | Both in-person and remote exchange; the SAS policy follows the code source | §8.2, §10.4 |
| v0.3 | STUN: Google + Cloudflare by default, editable | §9.3 |
| v0.3 | Rooms up to 16; only the owner admits; the owner leaving disposes the room | §14 |
| v0.3 | Wallet deferred; users generate keys at sign-in and may save them encrypted | §7 |
| v0.3 | Peer forwarding deferred; GitHub Pages hosting; desktop browsers + iOS Safari | §14.3, §17.5 |
| v0.3 | iOS is PWA-only (no App Store) | §17.5 |
| v0.3 | Public channels as the single opt-in exception to "no history" | §27, P7, §4.3 |
| v0.4 | Replies, edit, delete, self-destruct, reactions, typing, read ticks, pending queue | §11 |
| v0.4 | Contacts in the key file; several identities; identity transfer over P2P; observer role | §7, §14.2 |
| v0.4 | Tor as a second transport; IP privacy via LAN-only, VPN/WARP, Tor; three disclosure features in MVP-1 | §28, §29 |
| v0.5 | Tor per session; read/typing on in 1:1, off in rooms, reciprocal; the owner may delete any message; observers strictly read-only; self-destruct set by either person (1:1) or the owner (rooms) | §28.2, §11.7 |
| v0.5 | Tor bridges on by default; identity transfer keeps the old copy; contact cards; no edit/delete time limit; 8 identities per device | §28.3, §7 |
| v0.5 | Public channels: desktop-only publishing, owner always hidden via Tor, no link to the chat identity, 4 KiB posts | §27 |
| v0.6 | **No native programs** (P10): Tor is built into WASM; channel owners host from a browser tab | §2, §27, §28 |
| v0.6 | Argon2id t = 4 | §7.3 |
| v0.7 | Transport cipher: snow for the handshake only; raw split keys + in-place ChaCha20-Poly1305 with the header as AAD (same wire as Noise) | §10.1 |
| v0.7 | SAS emoji table = U+1F400..U+1F4FF; PING carries `hidden` | §10.4, §11.2 |
| v0.7 | HELLO caps bit8 SCANNED drives the SAS policy (the core cannot otherwise know how the peer got our code); codes shorter than any valid code are `E_INVALID_INVITE`, not a version mismatch | §10.4, §11.4, §8.3 |
| v0.7 | Integrity via stamped `index.html` (import-map integrity + CSP hash + wasm fetch integrity) instead of a separate `boot.js`; key files are named `ephem-<label>.p2pkey` | §17.2, §7.3 |
| v0.7 | Visual style shared with the owner's other projects (darkcite/trading-engine-multivenue dashboard): dark panels, one monospace face, uppercase accent section titles, status chips; dark-only | §23.1a |
| v0.8 | MVP-2: IDENTITY_READY (0x41) orders the transfer; SIGNAL body = §8.3 ICE layout; T1 by perfect negotiation with DTLS roles kept; remembered identities in IndexedDB hold only the encrypted key file | §7.6, §11.2, §13 |
| v0.9 | **Rooms by owner-signed state instead of MLS**: pairwise Noise links carry every message; the owner signs the member table (Ed25519) and every member verifies it; member-to-member signalling relayed by the owner, sealed with the static-static X25519 key; the greater `PeerId` offers; T2 = resume codes through the owner; record types ROOM_STATE 0x30, ROOM_SIGNAL 0x31, ROOM_LEAVE 0x32; one `chat_seq` per sender on every link | §10.3, §11.2, §13, §14 |
| v1.0 | **Tor mode as a second build of the same app**, not a separate `tor_bg.wasm` beside it: `crates/wasm` feature `tor` → `app/pkg/ephem_tor_bg.wasm` (adapter + arti + Snowflake), loaded only by `tor.html`; the direct build contains no Tor code and never downloads it (the service worker caches the Tor files on first use only) | §28.2, §17 |
| v1.0 | **The Snowflake bridge is a "TCP" address**: arti dials the bridge line's placeholder address and our runtime returns the Snowflake stream for exactly that address and refuses every other one (the transport guard of §28.5 inside Tor), so no pluggable-transport manager is needed; a warm pool of Snowflake proxies absorbs arti's fixed 5 s channel timeout | App. C.5 |
| v1.0 | Onion key = HKDF(seed, "p2pchat/onion-ed25519"), separate from the signing key; the service is re-hosted when the identity changes. Contact dials: prologue `p2pchat/contact`, zero `invite_id`, accepted only from a contact's key and only while the tab has no chat. Room introductions carry sealed TOR_INVITEs; the offering member hosts, the other dials; no T2 over Tor (the dialler redials) | §28.4, §28.7 |
| v1.0 | After the first live run: **both Snowflake bridges** (snowflake-01 `2B28…`, snowflake-02 `8838…`, placeholder addresses 192.0.2.3/192.0.2.4, one warm-proxy pool of 2 each); a proxy is replaced after **4 s without data while ours is unacknowledged** (20 s when idle), before arti's 10 s directory read timeout; directory snapshots are gzip-compressed JSON (the real directory is ~40 MB of text) | §28.3, App. C.5 |
| v1.0 | After the second live run: a new Snowflake proxy **resends everything in flight at once** and restarts KCP's backoff and dead-link count (`Kcp::new_path`); without it a backed-off timer left the new proxy silent, the 4 s stall rule replaced it too, and proxies churned every few seconds. Dial attempts time out after 30 s (live dials take 2–13 s) | App. C.5 |
| v1.0 | Tor links: **45 s without anything from the peer = lost** (peers PING every 15 s when idle); the dialler redials on fresh circuits at once. The Snowflake stall rule is timed from our send, not from the last byte received (the old rule replaced healthy idle proxies at every keep-alive: 58 switches in one live run) | §28.5, App. C.5 |
| v1.2 | Owner approved Appendix F with D1–D6 as recommended (2026-09-29): no chat history (P7 unchanged); the follow list in the key file (TLV 0x06); channels owned by the chat identity by default; one chat mode per tab; `connect-src https:`; 16 chats per tab. As built: several chats by *loading* one chat at a time in the adapter (a few swapped words) rather than threading a chat index through every path; owned channels are found in the store, so no key-file list of them (TLV 0x07 was not needed); the `#b=` link carries the lines as base64url text (no binary kind); the channel code is part of the Tor build and the separate channel build is gone | App. F |
| v1.1 | **Contact cards:** the card's secret travels in the Noise IK payload where a contact dial sends a zero `invite_id`; a contact added from a card keeps the secret (entry flag bit3) only until its first connection; the host shows no chat content before its user accepts. **Channels:** own page and build; IPNS `sequence` = revision (grows on every change); HTTP/1.1 on onion port 80; a reader accepts a response once `Content-Length` bytes arrived (arti ends streams with reason MISC); stable per-browser mirror onions; one channel per identity for now | §7.5, §28.4, §27, App. D |
| v1.0 | Streams naming nothing of ours are dropped unanswered (no `E_NOT_A_CONTACT` is sent: nothing is told to an unknown dialler) | §28.4 |
| v0.7 | "Simulate network loss" in the diagnostics drops the path without GOODBYE, so users (and the e2e test) can exercise T3; in a room (v0.9) it drops the member's links to other members, exercising T2 | §13, §18 |

### 25.3 Open questions

None.

## 26. Out of scope

- A custom NAT traversal, DTLS, WebRTC or cryptographic algorithm.
- Wallet authentication and peer forwarding (deferred, §23).
- A DHT.
- Blockchain or IPFS message storage.
- Server-side signalling.
- TURN fallback.
- Persistent history.
- Multi-frame QR.
- Single-QR bootstrap in **direct mode** (impossible, §25.1 R1). Tor mode supports it (§28.4).
- Traffic obfuscation in direct mode. Tor bridges in Tor mode are the only exception.

## 27. Public channels (opt-in exception to P7 and §4.3)

- A public channel is a permanent, **public** broadcast feed. Only its owner can post, and everyone else can only read.
- Posts are limited to 4 KiB of text.
- It is a **publication, not a chat**, and is fully separate from private chats: its own page (`channel.html`), its own CSP, its own crate, and its own keys. Private-chat data MUST NOT flow into a channel.
- **The owner is always hidden:**
  - The owner publishes **only through a Tor onion service hosted by the embedded Tor client in the owner's desktop browser tab** (§28). The owner never takes part in the public IPFS network, so the owner's IP is never exposed.
  - **Availability:** the channel is online while the owner's tab (or a follower's mirror tab) is open. There is no always-on host, because nothing native is built (P10).
  - **This depends on Tor mode passing its gates** (§28.9). Until then, public channels cannot be offered with a hidden owner, so they are not offered at all.
  - The channel's keys and onion address are derived one-way and are **not linked** to the owner's chat identity. The owner is known only if they say so in the channel.
- **Content format:** IPFS-native (CIDs, dag-cbor, CAR, IPNS V2 records), verified in Rust. Followers may mirror a channel over their own onion, or, accepting that their own IP becomes visible, to public IPFS. That is what lets readers without Tor read it through public gateways.
- Details and phases: Appendix D.
- **As built (v1.1):** `app/channel.html` + `channel.js` with its own wasm and CSP, one channel per identity. Details under each part of Appendix D ("As built").
- **As built (v1.2, Appendix F.3.3):** channels are two tabs of the app, **Following** and **My channels** (`app/channels.js`). The channel code (`crates/channel-web`) is part of the Tor build, so in Tor mode chats and channels share one Tor client (a shared `TorSlot`) and the chat identity owns the channels (`App::bind_channels`; channel keys stay derived and unlinkable in public; a separate identity on request). The direct page loads the Tor build only when a channel tab is first used, with its own Tor client and a one-time passphrase to own channels. An identity owns up to 16 channels (indices 0–15), found by their stored files, each online on its own onion while the tab is open; serving is idempotent per channel and always serves its latest version. The follow list lives in the key file (TLV 0x06) and is refreshed when the tab opens and every 10 minutes, four channels at a time, with new-post counts. `channel.html` only redirects old links (`#c=…`) into the app.

## 28. Tor mode (a second, separate transport, built into the WASM app)

### 28.1 Why a separate transport, and why embedded

- WebRTC data runs over UDP, and Tor carries only TCP streams. Tor Browser and Onion Browser also disable WebRTC entirely. So Tor mode is **not** "WebRTC over Tor": it is a different transport, under the **same** protocol above it (Noise, records, rooms, contacts, messaging features).
- **P10 (only our WASM):** the Tor client is built **into the web app**. There is no native helper.
- **How a browser reaches Tor:** a web page cannot open TCP connections to Tor relays. The only transport it can use is **Snowflake**: WebRTC to a volunteer proxy, which relays to the Tor Project's Snowflake bridge. Tor mode therefore always runs **through bridges**, which matches the "bridges on by default" decision.
- **Targets:** desktop Chrome, Edge, Firefox and Safari, **and iOS Safari**.
- **Status: built, tested in the offline lab.** Tor mode ships once gates G3–G4 (§28.9) also pass on the live Tor network. G1 (arti builds for `wasm32-unknown-unknown`) and G2 (Snowflake from browsers, desktop and iOS) passed on 2026-09-28; TOR-1…TOR-4 pass end to end in the lab (§24.2 APP-E2E-TOR, APP-E2E-TOR-ROOM).

### 28.2 Mode selection and isolation

- The connection mode is chosen **per signed-in session**, on the sign-in screen: **Direct** (the default) or **Tor**.
- It cannot be changed without signing out, so a single session never mixes the two.
- **A Tor session runs on its own page, `tor.html`,** which loads the **Tor build** of the app, `pkg/ephem_tor_bg.wasm` (the same adapter compiled with the cargo feature `tor`: arti and Snowflake inside, the WebRTC chat transport refused). Direct sessions never download the Tor code; the service worker caches the Tor files only when `tor.html` is used.
- `tor.html` is generated from `index.html` (`tools/stamp.py`): the same page with `data-mode="tor"`, its own CSP (§28.6) and the Tor build. A `#t=` link opened on the direct page moves to `tor.html`, and a direct code opened on `tor.html` moves to the direct page, before anything else runs.

### 28.3 Embedded Tor client

```
tor.html ── core (sans-IO, Noise, rooms) ── Tor streams (crates/wasm feature `tor`)
              │
              └─ ephem_tor_bg.wasm
                   ├─ arti-client 0.46+ (upstream crates; features: onion-service-client/-service,
                   │   ephemeral-keystore, bridge-client, pt-client, keymgr, rustls. NOT compression: no zstd/xz C code)
                   ├─ runtime shim: spawn_local, setTimeout timers, inline "blocking", TCP/UDP = unsupported
                   ├─ TLS: rustls + ring (wasm32_unknown_unknown_js)
                   ├─ state: in memory for the session (vendored patches); the public directory (consensus,
                   │   authority certificates, microdescriptors) is snapshotted to IndexedDB for warm starts
                   ├─ keys: arti ephemeral keystore (the onion key is derived from the seed, §7.1, and kept in RAM only)
                   └─ bridge "TCP" stream = Snowflake in Rust (no PT manager, App. C.5):
                        broker fetch (CORS *) → RTCPeerConnection to a proxy (web-sys)
                        → Turbotunnel → KCP → smux → bridge byte stream
```

- **Bridge lines:** the Snowflake bridge lines (both of the Tor Project's Snowflake bridges) and broker URLs are built in, as in Tor Browser, and updated with app releases.
- **Rendezvous:** direct HTTPS to the broker; when it cannot be reached, the broker's **CDN URL** (`https://1098762253.rsc.cdn77.org/`, which E2 showed working from browsers without domain fronting). The AMP-cache route is not implemented.
- **Domain fronting is not possible from a browser**, because `fetch` cannot override the `Host` header. Where the broker is blocked, Tor mode fails with `E_TOR_UNAVAILABLE` and there is no other route (P10).
- **Background tabs:** the tab keeps an open `RTCDataChannel` to the Snowflake proxy. Whether that exempts it from Chromium's aggressive timer throttling of hidden tabs is checked in spike E8. On iOS, everything pauses when the app goes to the background (§28.8).

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

- **Single QR:** Bob dials Alice's onion address straight from the invite. **No answer code is needed.** Alice's tab must be open, and on iOS in the foreground, until Bob connects.
- **Handshake:** `Noise_IK_25519_ChaChaPoly_BLAKE2s`. The initiator is the dialler (Bob, or a contact reconnecting). The prologue is the invite bytes, or `"p2pchat/contact"` for contact reconnects. The first handshake payload carries `invite_id` (16 B), or a `card_secret` for card-based dials, plus the initiator's `onion_pk`.
- **Who is accepted:** an incoming stream is accepted only in one of these cases:
  1. its static key is in **contacts**;
  2. the payload names a **live, unused** `invite_id`;
  3. the payload carries the current, unexpired **`card_secret`** (§7.5). The user is then asked "Bob (from your contact card) wants to connect", and the key is added as a contact if they accept.

  Everything else is dropped before any application data, unanswered (nothing is told to an unknown dialler). This blocks spam, which was Tox's "nospam" problem.
- **As built:** all three cases. Contact and card streams are accepted only while the tab has no chat open (a tab holds one chat or one room). Case 3: the stream carries the card's secret where a contact dial carries a zero `invite_id`; after the handshake the host shows "… (from your contact card) wants to connect" with **no chat content** until the user accepts (accept = saved as an unverified contact, decline = closed). Every incoming stream is offered to the tab's waiting chats first: its first frame (IK message 1) is decrypted with each chat's prologue until one accepts it. A link to a room member accepts only the key the signed room state names. The virtual port is 1.
- **SAS:** prompted for every non-contact (§10.4), because Alice has no out-of-band proof of Bob's key.

### 28.5 Transport rules

- **Framing:** each frame is `u16 len` followed by an outer frame (§11.1). The same 16 KiB limit applies, and the same records apply (§11.2).
- **No-mixing guard:** in a Tor session, `core` never emits `CreatePc` for a **chat peer**, and no STUN server from §9.3 is contacted.
  - The **only** `RTCPeerConnection`s allowed are the Snowflake transport's own, to Snowflake proxies, using Snowflake's STUN list. They carry only Tor traffic.
  - Any other attempt aborts, because it would be a bug that leaks the IP.
- **No fallback:** if Tor fails, the session fails with `E_TOR_UNAVAILABLE`. The app never quietly switches to direct.
- **Network changes need no recovery ladder:** an onion address does not depend on the network. After a drop, the side that dialled simply dials again with backoff (§12). Turbotunnel also keeps the Tor session alive across a change of Snowflake proxy. The Noise session re-handshakes, and the pending queue is resent.
- **Latency:** 0.5–2 s per message (Snowflake hop plus 6 hops onion to onion). Text only; voice and video are out of scope for Tor.

### 28.6 CSP of `tor.html`

```
default-src 'none'; script-src 'self' 'wasm-unsafe-eval' '<import-map hash>'; style-src 'self';
img-src 'self' data: blob:; connect-src 'self' https:;
worker-src 'self'; manifest-src 'self'; media-src 'self' blob:; base-uri 'none'; form-action 'none'
```

- Written by `tools/stamp.py`, as for `index.html` (§17.2).

- `fetch` reaches only a Snowflake broker: the Tor Project's (directly or through its CDN URL) or the user's own from their bridge lines (Appendix F.2), hence `https:` (v1.2, decision D5; before, the two brokers were listed). Snowflake's WebRTC is not governed by CSP (§17.3 limit).
- **Everything else goes through Tor**, including the channel owner's IPNS publishing (§27, which uses a Tor exit).

### 28.7 Contacts reconnect without a QR, and rooms over Tor

- **Contacts:** a contact entry with `onion_pk` (§7.5) enables **"Connect"**. It dials the stored onion address directly, from any network, whenever the contact has a Tor session open.
  - The onion address comes from the identity seed, so it is stable for saved identities and survives moving to another device (§7.6).
  - While signed in in Tor mode, the tab keeps the onion service up. The UI shows "Reachable by contacts via Tor while this tab is open".
- **Rooms:** the same owner model (§14), with each member hosting an onion service. The owner invites with TOR_INVITEs; members are introduced through the owner as in direct mode, but the sealed code is a TOR_INVITE (it carries the offering member's `onion_pk`), so the other member dials that onion **directly**. Nothing is answered, and nobody is asked about IP exposure (§29.2 does not apply). A lost member link is redialled by its dialler (no T2). The cap is 16.
- **As built:** a contact saved after a Tor chat stores its `onion_pk`; "Connect" in the contact list dials it (prologue `p2pchat/contact`, zero `invite_id`). The onion service follows the signed-in identity (re-hosted on sign-in and sign-out).

### 28.8 What Tor mode hides, and its limits

- **Hides:** your IP from peers and from network observers.
  - The Snowflake proxy and broker see your IP and that you use Snowflake, but not whom you talk to.
  - The TLS SNI of the broker shows that you use Tor.
- **Does not hide:**
  - what you type, including nicknames;
  - timing correlation by a global observer;
  - that sessions are linked when a **saved** identity is reused, because its onion address stays the same.
- **iOS:** reachable **only while the app is in the foreground**. In the background, Snowflake and all circuits pause. On return, Tor re-attaches (warm start from the IndexedDB cache) and contacts can dial again.
- **Desktop Safari:** hidden tabs are throttled heavily (E8: timer gaps of 103–150 s), so Tor circuits and Snowflake keep-alives in a background Safari tab are unreliable. The UI shows "Keep this tab visible to stay reachable via Tor" in Safari. **Chrome and Firefox are not throttled** with an open DataChannel (E8: max gap ≤ 2 s over 10 min), so background tabs there stay reachable.
- **Censored networks:** where the broker and the AMP cache are blocked, Tor mode is unavailable (no domain fronting in browsers, and nothing native, P10).

### 28.9 Gates (details in Appendix C)

| Gate | Criterion | Status |
|---|---|---|
| G1 | arti builds for `wasm32` without forking its core | ✅ Passed at compile level (spike E1, 2026-09-28) |
| G2 | Broker rendezvous and a DataChannel to a Snowflake proxy work from `github.io`, on desktop **and** iOS Safari | ✅ Passed: desktop Chrome and Safari, and iPhone Safari from `darkcite.github.io` (live, 2026-09-28) |
| G3 | Bootstrap ≤ 60 s cold and ≤ 10 s warm; an onion connects on desktop and iOS | ✅ lab (Chromium); ⏳ live network, iOS (§23.3) |
| G4 | Tor build ≤ 5 MB compressed, lazily loaded; no iOS memory kills; onion hosting from a tab works | ✅ 2.08 MB gzip, loaded only by `tor.html`; ✅ hosting from a tab (lab); ⏳ iOS memory |

## 29. IP-address privacy

### 29.1 Supported options

| Option | Hides your IP from the peer | Hides it from your ISP / network | Cost / registration | Desktop | iOS |
|---|---|---|---|---|---|
| **LAN-only mode** (§9.4) | The peer sees only a LAN address, and both must be on the same network | ✓ (traffic stays local) | free | ✓ | ✓ |
| **Your own VPN with UDP support**, full tunnel, set up outside the app. **Cloudflare WARP** (1.1.1.1 app) is free and needs no account | ✓ (the peer sees the VPN exit) | ✓ (the ISP sees that a VPN is used) | depends on the VPN; WARP is free | ✓ | ✓ |
| **Tor mode** (§28, once its gates pass) | ✓ | ✓ (the network sees Snowflake/Tor use, not the destination) | free | ✓ | ✓ (foreground only) |

**Connectivity cost of VPNs (measured, S10):** Cloudflare WARP applies a **symmetric NAT**. It hides the IP, but direct connections through it only work when the other peer is easy to reach. WARP ↔ WARP, or WARP ↔ strict mobile carrier NAT, usually fails with `E_NO_DIRECT_PATH`. The app MUST explain this when a direct connection fails while a VPN is detected (a WARP egress range, or HTTPS and UDP addresses that differ), and SHOULD offer **Tor mode** (§28), which works regardless of NAT type.

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

---

## Appendix C: Embedded Tor, engineering detail

### C.1 Architecture

```
┌──────────────────────────── tor.html (own CSP, §28.6) ────────────────────────────┐
│ core (sans-IO) ── Tor streams ──▶ ephem_tor_bg.wasm (Tor build of the app)        │
│                                     ├─ arti-client 0.46+ (upstream, no fork)       │
│                                     ├─ runtime shim: spawn_local, setTimeout,      │
│                                     │  inline blocking, TCP/UDP = unsupported      │
│                                     ├─ TLS: rustls + ring (wasm32_unknown_unknown_js)│
│                                     ├─ state: in memory; dir snapshot → IndexedDB  │
│                                     ├─ keys: ephemeral keystore (onion key in RAM) │
│                                     └─ bridge "TCP" stream = Snowflake (Rust)      │
│                                          broker fetch → RTCPeerConnection to proxy │
│                                          → encapsulation → KCP → smux              │
└─────────────────────────────────────┬─────────────────────────────────────────────┘
                                      │ WebRTC (UDP)
                             volunteer Snowflake proxy
                                      │ WebSocket
                             Snowflake bridge (Tor Project) ──▶ Tor network ──▶ peer's onion
```

### C.2 Work items

| Component | What we build | Base | Risk |
|---|---|---|---|
| Runtime shim | `tor_rtcompat::Runtime`: Spawn, SleepProvider, CoarseTimeProvider, Blocking, NetStreamProvider/UdpProvider (unsupported), TlsProvider | arti's swappable runtime traits; `coarsetime` is already wasm-aware | Medium |
| `!Send` browser objects | The Snowflake transport runs in a local task, bridged to arti through `Send` channels | `futures::channel::mpsc` | Medium |
| Directory cache | Persist the consensus, microdescriptors and guards to IndexedDB (public data) for warm starts | Built: `MemoryStore` (vendored `tor-dirmgr` patch) with JSON snapshots (`cache_export` / `cache_import`); the page keeps the snapshot in IndexedDB (`ephem-tor`), saved when Tor is ready and every 30 min, imported before the next start. arti re-validates it on load. Guards are not kept (the guard is the Snowflake bridge) | Medium |
| Snowflake client | Broker rendezvous (§24.2 E2 format), WebRTC to the proxy, encapsulation, **KCP** (Rust `kcp` crate), **smux v2** (our own, small) | Reference Go client; broker CORS `*` | **High** |
| Onion hosting | `tor-hsservice` from a tab | Compiles for wasm (E1) | Medium–High |
| Size | Lazily loaded Tor build | 2.08 MB gzip | ✅ G4 size |

### C.3 Behaviour and limits

| Topic | Desktop | iOS |
|---|---|---|
| Bootstrap | Cold: a few MB of directory through Snowflake, tens of seconds. Warm: from IndexedDB | Same; re-attaches on every return to the foreground |
| Being reachable (onion hosting) | While the tab is open (hidden tab: E8) | **Foreground only** |
| One-way invite | ✓ | ✓, but the inviter must stay in the foreground until the peer dials |
| Latency | 0.5–2 s per message | Same |
| Censored networks | Broker via direct HTTPS or the CDN URL; **no domain fronting** in browsers; no fallback (P10) | Same |

**Privacy:** the Snowflake proxy and broker see your IP and that you use Snowflake, but not your destination. The broker's TLS SNI shows Tor use unless the CDN URL is used.

### C.4 Research steps

| Step | Work | Gate |
|---|---|---|
| E1 | arti for wasm32 | **G1 ✅** |
| E2 | Live broker rendezvous and a DataChannel to a proxy, desktop and iOS | **G2 ✅** (desktop and iPhone) |
| E3 | Turbotunnel (KCP + smux) to the real bridge; stable for 10 min | — |
| E4 | arti over E3: bootstrap, circuit, dial a known onion | **G3** |
| E5 | Onion hosting from a tab | — |
| E6 | Size, iOS memory, battery | **G4** |
| E7 | Fuzz the Snowflake/Turbotunnel parsers; security review | Sign-off |

**Effort:** this is the largest item in the project. The ongoing cost is tracking arti releases and Tor protocol changes, and applying Tor security fixes quickly.

### C.5 Implementation plan (TOR-1 … TOR-4, started 2026-09-28)

**Key simplification: the bridge is a "TCP" address.** arti needs no pluggable-transport manager. The Snowflake bridge line's placeholder address (`192.0.2.3:80`, as in Tor Browser) is configured as an ordinary bridge, and our runtime's `NetStreamProvider::connect` returns the **Snowflake stream** for exactly that address, and refuses every other address (`E_TOR_UNAVAILABLE`; this is also the transport guard of §28.5, since the Tor code can open no other socket). arti then runs TLS (rustls) and the Tor link handshake over that stream, exactly as over TCP. The snowflake server on the bridge side forwards the stream to the bridge's ORPort unchanged.

**Wire stack of the Snowflake stream** (ported from the reference Go client v2.9.2 with kcp-go v5.6.8 and smux v1.5.24; interop-tested against snowflake v2.14.1 with kcp-go v5.6.24 and smux v1.5.56):

| Layer | Format | Our implementation |
|---|---|---|
| WebRTC DataChannel to a proxy | proxy relays bytes ↔ WebSocket to the snowflake server | web-sys `RTCPeerConnection` (as in E2) |
| Turbotunnel | per DataChannel: `Token 12 93 60 5d 27 81 75 f5` ‖ `ClientID [8]` (random per session), then encapsulated packets; a new DataChannel continues the same session | `crates/snowflake` |
| Encapsulation | chunks: `dcxxxxxx [cyyyyyyy [0zzzzzzz]]` length prefix (d = data/padding), ≤ 3 prefix bytes | `crates/snowflake::encap` |
| KCP | ikcp segments, little-endian `conv u32, cmd u8 (81 PUSH, 82 ACK, 83 WASK, 84 WINS), frg u8, wnd u16, ts u32, sn u32, una u32, len u32`; no crypto, no FEC; stream mode; window 65 535; congestion control off; MTU 1400 | port of kcp-go's `kcp.go`, sans-IO, fixed-capacity rings |
| smux v2 | `ver u8=2, cmd u8 (0 SYN, 1 FIN, 2 PSH, 3 NOP, 4 UPD), len u16, sid u32`; UPD = `consumed u32, window u32` flow control; client stream ids odd from 1; keep-alive NOP | `crates/snowflake::smux`, one stream |
| Tor link protocol | TLS 1.2/1.3 + cells, run by arti | arti (rustls) |

**Crates and pages:**

- `crates/snowflake` (sans-IO, no wasm-bindgen): encapsulation, KCP, smux, Turbotunnel session (redial on a new DataChannel keeps the KCP/smux session). Native tests + interop tests against the Go server.
- `crates/tor` (a library of the Tor build; `checks/tor-lab/build_web.sh` also builds it alone, with the `js-api` feature, for the lab page): browser runtime for arti (`CompoundRuntime`: spawn via `spawn_local`, timers via `setTimeout`, `web-time` clock, blocking work inline, UDP/listen unsupported, rustls + ring TLS), the Snowflake transport (broker rendezvous by `fetch`, `RTCPeerConnection` to proxies), arti-client, onion service hosting, and the Tor transport for `core`.
- `vendor/` (arti 0.46.0): **four small wasm-only patches**, listed in `vendor/README.md` and marked "Ephem patch": `arti-client` (in-memory state manager instead of `unimplemented!()`), `tor-persist` (in-memory state directory), `tor-hsservice` (ephemeral replay logs), `tor-dirmgr` (in-memory directory store instead of SQLite). Native builds compile the upstream paths. arti's core crates stay unmodified (G1). Each patch is dropped when upstream covers wasm.
- `crates/wasm` feature `tor` → `app/pkg/ephem_tor_bg.wasm`: the app adapter with chat links over Tor streams (`src/tor.rs`), used by `app/tor.html` (generated, own CSP, §28.6) with the same `app.js` in Tor mode.

**Offline Tor lab** (`checks/tor-lab/`, no Internet needed, because this container cannot reach torproject.org): chutney builds a private Tor network (directory authorities, relays, exits with `TestingTorNetwork`), plus one **bridge** whose ORPort is fed by the real **Go snowflake server**; the real Go **broker** and a **standalone proxy** run on localhost. arti is configured with the lab's authorities and the lab bridge. This exercises every layer end to end (browser included) exactly as on the real network; only the broker URL, bridge line and authorities differ. Live-network runs happen on the owner's laptop (`ONLY=tor ./checks/run_all.sh`: the same e2e tests with `LIVE=1`, the real broker, bridge and Tor network, the page unchanged).

**Steps:**

| # | Work | Proof | Status |
|---|---|---|---|
| T-1 | Lab: chutney network + Go broker, proxy, server | a native Tor client (system `tor`) bootstraps through the lab | ✅ |
| T-2 (E3) | `crates/snowflake`: encapsulation, KCP, smux, Turbotunnel | unit tests; **native interop**: our client ↔ Go snowflake server (WebSocket, as a proxy would) ↔ TCP echo, 10 MB both ways; proxy switch mid-transfer keeps the stream | ✅ |
| T-3 (E3) | Browser Snowflake: broker rendezvous, DataChannel, redial | Chromium ↔ lab broker/proxy/server ↔ echo | ✅ |
| T-4 (E4) | arti runtime shim + vendored patch; bootstrap over Snowflake; dial an onion (lab onion service) | **G3** in the lab: bootstrap time cold/warm, onion echo round trip, in Chromium | ✅ |
| T-5 (E5) | Onion service hosted from the tab (key from the seed, §7.1); directory cache in IndexedDB | two Chromium tabs reach each other's onion | ✅ (warm start: "Loaded a good directory from cache") |
| T-6 (TOR-2) | `tor.html`; TOR_INVITE (kind 5); Noise IK over the onion stream (u16 framing); accept rules (§28.4); SAS; the same chat features | e2e: 1:1 chat over Tor in the lab | ✅ 16/16 |
| T-7 (TOR-3) | Contacts with `onion_pk`: "Connect" without a QR | e2e reconnect of saved contacts | ✅ |
| T-8 (TOR-4) | Rooms over Tor: introductions carry sealed TOR_INVITEs (with `onion_pk`); members dial each other | e2e room of 3 over Tor | ✅ 9/9 |
| T-9 (E6, E7) | Size (G4), memory; fuzz the Snowflake parsers; review | numbers in §24.2 | ✅ size, fuzzing; ⏳ iOS memory |
| T-10 | Live network on the owner's laptop and iPhone | G3/G4 on the real Tor network | ⏳ |

## Appendix D: Public channels, detailed design

Normative summary: §27. **This depends on Tor mode passing gates G2–G4.**

### D.1 Goal and owner decisions

| # | Decision |
|---|---|
| D1 | Publishing is **desktop only**, and that is acceptable |
| D2 | The **owner's IP is always hidden**, so channels are published **only over Tor** |
| D3 | Republishing: whatever works best (D.5.3) |
| D4 | The channel is **not linked** to the owner's chat identity. The owner is known only if they choose to say so in a post |
| D5 | Posts are limited to **4 KiB** of text |
| D6 | Gateways: whatever works best (D.6.3) |
| D7 | **No native programs.** Hosting happens in the owner's browser tab |

### D.2 Design

- **The data is IPFS-native:** CIDs, dag-cbor blocks, CAR files and IPNS V2 signed records, all verified in Rust.
- **The owner's browser tab serves it as a Tor onion service,** using the embedded Tor client (§28).
- The owner never takes part in the public IPFS network, and never makes a direct connection.

```
 OWNER (desktop browser tab, hidden)                           READERS
 ┌──────────────────────────────────────┐                     ┌───────────────────────────────────┐
 │ channel.html (Tor session)           │                     │ channel.html#c=<ipns-name>&o=…    │
 │  Rust: sign posts, build CAR,        │                     │  Rust: verify record, CIDs and    │
 │  sign the IPNS V2 record             │                     │  post signatures                  │
 │  store: OPFS folder = pinning folder │                     └──────┬────────────────┬───────────┘
 │  onion service (embedded Tor):       │                            │ Tor (embedded) │ HTTPS (optional)
 │  read-only trustless-gateway API     │ ◀──── Tor network ─────────┘                v
 │  IPNS PUT to delegated-ipfs.dev      │ ── via Tor exit ──▶ delegated routing   public IPFS gateways
 │  (optional)                          │                                        (only if a follower
 └──────────────────────────────────────┘                                         mirrors to IPFS with
                                                                                  their own Kubo)
```

1. **The channel folder (the pinning folder).** The channel lives in the browser's **Origin Private File System (OPFS)**, at `channels/<name>/` (blocks, the latest signed record, and the manifest). OPFS works in every target browser, including Safari. The app calls `navigator.storage.persist()` so the browser does not evict it.
   - **Optional mirror to a real folder** on the owner's disk through the File System Access API (Chromium desktop).
   - **"Export channel as CAR"** downloads a full backup, and "Import CAR" restores it on another machine.
2. **Serving.** While the owner's tab is open, the embedded Tor client hosts the channel's **dedicated onion address**. It serves a read-only subset of the **IPFS trustless-gateway API**:
   - `GET /ipfs/<cid>?format=car` (and `format=raw`);
   - `GET /ipns/<name>?format=ipns-record`.
3. **Readers** use the same verifying client as for a public gateway. The "gateway" is the owner's `.onion` (or a mirror's), reached through the reader's embedded Tor.

**As built:** the owner's store is OPFS `channels/<name>/{channel.car, record.bin}` (IndexedDB where OPFS files cannot be written), rewritten after every change; `persist()` is requested when a channel is created. "Export backup" downloads both files; "restore from a backup" reopens them (the record's root must be the CAR's). The gateway is plain HTTP/1.1 on port 80 of the onion, one request per stream (`Connection: close`), served by `crates/channel::gateway` (sans-IO, unit-tested): `ipns-record`, `car` (the DAG under any held CID) and `raw`; everything else 404/405/406/431. A tab runs several onion services side by side (`Tor::launch`: the channel's, each mirror's).

### D.3 Identity separation (D4)

- Channel signing key = `HKDF(seed, "p2pchat/channel/" ‖ u32 index)` (Ed25519). The IPNS name is that key.
- Channel onion key = `HKDF(seed, "p2pchat/channel-onion/" ‖ u32 index)`. It is a **different onion address** from the owner's chat onion.
- HKDF is one-way. Nothing in a channel can be linked to the owner's `PeerId`, chat onion, nickname or contacts. The manifest has no owner field: only a `title` and an `about` text the owner writes.
- **Caveats shown when the channel is created:** writing style and posting times can identify you. Anyone who obtains the key file can link the channel to your chat identity, so the UI recommends a **separate identity** just for the channel (§7.2).

### D.4 Availability (the honest limit)

| Host | When the channel is online |
|---|---|
| The owner's tab | While it is open on the desktop. Leaving a tab open is possible; whether a hidden tab keeps working is spike E8 |
| Followers' mirror tabs (D.7.1) | While any mirror tab is open |
| A follower's own Kubo (D.7.2, third-party and optional) | While their node runs. **Their** IP is public, never the owner's |

**No always-on host exists without native software (P10).** A channel whose owner and mirrors are all offline cannot be read, except from a follower's Kubo mirror or from gateway caches.

### D.5 Data and publishing

#### D.5.1 Blocks (dag-cbor)

- **Root**: manifest, head page, post count, last update time.
- **Manifest**: title, about, `channel_pk`, created, an optional signed mirror list, and a signature.
- **Page**: up to 64 posts, and a link to the previous page.
- **Post**: `seq`, timestamp, body (≤ 4 KiB), `reply_to`, `deleted` flag, and a signature.

Deleting a post rewrites it with `deleted = true` and an empty body. Older copies may survive on mirrors, and the UI says so before the first post.

**As built:** manifest `{v: 1, title, about, pk, created, mirrors, sig}`, post `{seq, ts, body, reply (0 = none), deleted, sig}`, page `{posts: [≤ 64 posts, oldest first], prev}`, root `{manifest, head, count, updated}`; signatures are Ed25519 over `"ephem-channel-manifest:" | "ephem-channel-post:" ‖ dag-cbor(the map without sig)`. DAG-CBOR is canonical (keys by length, then bytes) and decoding is strict (non-canonical input is refused), so hashes and signatures always refer to the same bytes. A post is signed once and cached (1 000 posts build in ~1 s in the browser).

#### D.5.2 Posting (the owner's desktop tab, Tor session)

1. Sign the post, then rebuild the head page and the root.
2. Write the changed blocks and the new IPNS V2 record (`sequence` = the next revision: it grows on **every** change, deletions and mirror lists included, `validity = now + 30 days`, `ttl = 60 s`) to OPFS.
3. Serve them on the onion address at once.
4. Optionally publish the record through a **Tor exit stream** to `https://delegated-ipfs.dev/routing/v1/ipns/<name>` (`PUT`, CORS `*`, spike C-P4). This makes the name resolvable on the public IPFS network **without revealing the owner**. It only matters if some IPFS mirror holds the content.

#### D.5.3 Republishing (D3)

- The onion serves the latest record directly, so the owner never has to republish.
- The record is valid for 30 days, so mirrors and Kubo followers can republish the owner's signed record (`ipfs name put`, spike C-P4) without the owner's key.
- The owner's app re-signs on every post, and whenever it opens and the record is older than 7 days.

### D.6 Reading

#### D.6.1 Channel link

```
https://<owner>.github.io/ephem/channel.html#c=<ipns-name>&o=<channel-onion>[&m=<mirror-onion>…]
```

- The IPNS name is the channel's identity. The onion addresses are **hints**, because everything is verified against the IPNS key.
- The manifest can carry a **signed mirror list**.

#### D.6.2 Readers by platform

| Reader | Path | Reader's IP |
|---|---|---|
| Desktop or iOS, Tor session | Embedded Tor → owner or mirror onion | Hidden |
| Any browser without Tor | Public IPFS gateways, **only if** a follower mirrors the channel to IPFS with their own Kubo | Visible to the gateway (never to the owner) |

#### D.6.3 Default gateways (D6)

- `trustless-gateway.link` (the only public trustless gateway that works from browsers; `ipfs.io` and `dweb.link` redirect to it without CORS, spike C-P1). Timeout 4 s. The list is editable, so a follower's own gateway can be added.
- **Single-operator risk:** if that gateway is down, readers without Tor cannot read mirrored channels. Readers with Tor are unaffected, because they read from onion hosts and mirrors directly.
- The gateway software defaults to CORS `*` and supports trustless CAR and IPNS-record responses (spike C-P1, from source).

### D.7 Followers and mirrors

#### D.7.1 Onion mirror in the browser (keeps the follower hidden)

- **Mirror this channel** copies and verifies the channel into the follower's OPFS (`mirrors/<name>/`).
- While the follower's Tor-session tab is open, it serves the channel on the follower's own mirror onion. Every 10 minutes it checks the owner's onion for a newer record, and updates only when the new record verifies with a **higher** sequence.
- **As built:** the mirror's onion key is a random seed kept in the follower's browser per channel, so the address is stable across visits (and can be in the owner's signed mirror list); the mirrored CAR and record are kept in OPFS `mirrors/<name>/` and served again on the next visit. Readers try every address of the link at once (owner and mirrors) and take the first valid answer, with retries on fresh circuits; they keep a high-water mark of the record sequence per channel (browser storage) and refuse older versions.

#### D.7.2 IPFS mirror (optional; the follower's own third-party software)

- A follower who runs **Kubo themselves** (not built or required by us, P10) can press **Also mirror to public IPFS**.
- The app shows the two Kubo commands to run:
  - `ipfs dag import channel.car` (the app downloads the CAR);
  - `ipfs name put <record>`.
- The app itself never talks to Kubo.
- The UI states: "Your IP will be visible as a host of this channel."

### D.8 Security

| Threat | Mitigation |
|---|---|
| Finding the owner's IP | The owner is reachable only as an onion service, and any clearnet publishing goes through a Tor exit |
| Linking the channel to the chat identity | Separate HKDF keys, a separate onion, no owner field, and a dedicated identity recommended |
| Forged or changed posts | CID checks, the post signature, and the signed IPNS record |
| An old version served | IPNS sequence high-water marks, and several sources tried |
| Someone else posts | Impossible: only the channel key can sign |
| Losing the channel data | OPFS with `persist()`, the optional real-folder mirror, and CAR export |

### D.9 Phases (after TOR-1 passes G2–G4)

| Phase | Scope |
|---|---|
| CH-1 | `channel` crate: keys, IPNS V2, dag-cbor, CAR, verification |
| CH-2 | Onion hosting of the read-only gateway subset from a tab; OPFS store; CAR export and import |
| CH-3 | Owner UI (desktop): create a channel, post, delete, the warnings; optional IPNS PUT via a Tor exit |
| CH-4 | Reader UI: Tor session, and public gateways for IPFS-mirrored channels |
| CH-5 | Mirrors: browser onion mirrors, the signed mirror list, Kubo mirror instructions |

### D.10 Spikes

| ID | Question | Status |
|---|---|---|
| C-P1 | Gateways serve CAR and IPNS records with CORS | ✅ `trustless-gateway.link` (live: Chrome, Safari, iPhone). `ipfs.io`/`dweb.link` only redirect there without CORS |
| C-P2 | A tab hosting several onion services at once (the chat onion and channel onions) | ✅ v1.2: one Tor-mode tab runs its chat onion, two channel onions and mirror onions side by side (`Tor::launch`), exercised in APP-E2E-TOR-CHANNEL |
| C-P3 | Time to load a channel with 1 000 posts over an onion | ✅ **Lab: 0.4 s** to fetch, verify and render 1 003 posts (CAR 120 KB); the owner builds 1 000 posts in 1.1–1.3 s |
| C-P4 | Republishing a signed IPNS record without the key | ✅ Live: browser `PUT` to `delegated-ipfs.dev`, read back byte-identical from `trustless-gateway.link` |
| C-P5 | OPFS quota and eviction with `persist()` on each browser | ⏳ |
| E8 | A hidden desktop tab with an open DataChannel keeps its timers running | ✅ Chrome and Firefox; ❌ Safari (§24.2 E8) |

### D.11 One identity on several devices: the vault (V-1…V-3 built; spikes V-P1/V-P2 and V-4 open)

**Problem (owner report, 2026-09-29).** A channel lives in the storage of the browser that created it (OPFS/IndexedDB, D.5). The same identity signed in on another device, in a private window, in the iPhone Home Screen app instead of Safari (iOS keeps their storage apart), or in Safari after 7 days without a visit (ITP may evict storage), finds no channel. Today the only move is the manual backup (Export → Import).

**Goal.** A device that has only the identity (key file + passphrase) finds its channels and can keep posting, **even when every other device of this identity and every mirror is off**, with no server of ours and no companion program (P10).

**The honest limit first.** IPFS stores nothing by itself: bytes survive only where some node keeps them. With all our devices and mirrors off, three places remain, each with a different reach:

| Where | Holds | Survives with everything off | Cost |
|---|---|---|---|
| **The key file** (a TLV section, like the follow list) | A snapshot as of the last save/export | For ever (it is the user's file) | Stale: only as fresh as the last export |
| **An IPNS record on the public DHT** (the vault record, V.2) | ≤ ~6 KiB, encrypted: the channel list, heads, mirrors, the writer lease | **~36–48 h** after the last republish (DHT nodes drop records older than their record age; to be measured, spike V-P2) | Free, anonymous (published through a Tor exit, as D.5.2) |
| **An IPFS storage provider** the user chooses (optional, V.4) | The whole channel (CARs), plain: the channel is public anyway | For as long as the account lasts | An account with a third party; the upload goes through a Tor exit |

Everything else follows from this table: the vault makes the *state* follow the identity for free; the *content* follows it for free only while some holder is online (another device, a mirror, a provider), and otherwise the new device **continues the channel without its history** and back-fills it later.

#### D.11.1 Keys (all derived, nothing new to keep)

- `vault_sign = HKDF(seed, "p2pchat/vault-sign")` → Ed25519; its IPNS name is the vault name. One-way like D.3: the name links to nothing (not the chat `PeerId`, onion, or any channel).
- `vault_key = HKDF(seed, "p2pchat/vault-key")` → 32-byte XChaCha20-Poly1305 key (crate `chacha20poly1305`, already a dependency of `crates/crypto`).
- Two channels of one identity are linked **only inside the ciphertext**; the vault name is never given to followers or mirrors, so they cannot tie channels together.

#### D.11.2 The vault record

- An **IPNS V2-only** record (no V1 fields: they would duplicate the value and halve the space) under the vault name; `sequence` grows on every change; `validity = now + 30 days`; `ttl = 60 s`.
- Its value is `/ipfs/<CIDv1 raw, identity multihash, base58btc>`: the ciphertext **inlined in the CID**, so no block has to be fetched from anyone; the device decodes it from the record itself.
- Ciphertext = `nonce(24) ‖ XChaCha20-Poly1305(vault_key, AAD = "ephem-vault-v1" ‖ sequence, plaintext)`; the plaintext is padded to the next of 1, 2, 4 or 6 KiB so its size does not reveal what changed.
- Plaintext (TLV, as the key file): version; **writer lease** `{device id (random, per browser), until}`; per owned channel `{index, title, head page CID, root CID, count, record sequence, updated, mirrors[]}` (≈ 250 B each: 16 channels ≈ 4 KiB); the follow list only if it still fits (it also lives in the key file); optional storage-provider settings (V.4), token included (it is encrypted).
- Size budget: IPNS records are limited to **10 KiB** (IPNS spec; `ipns::MAX_RECORD`); the base58 CID text expands the ciphertext ×1.37, so **≈ 6 KiB of plaintext**. Spike V-P1 confirms `delegated-ipfs.dev` accepts and serves a record of that size with an identity-CID value.

#### D.11.3 Flows

1. **Publish.** Every device of the identity, once Tor is up, `PUT`s the vault record through a Tor exit to `https://delegated-ipfs.dev/routing/v1/ipns/<vault name>` (the D.5.2 path, spike C-P4 ✅) on each change, and **re-signs and republishes it every 12 h** while it runs, which keeps it inside the DHT's record age. The channel records themselves are republished as today (D.5.3), by the owner and by any mirror.
2. **Sign in on a new device.** Derive the vault name, `GET` the record (routing API or `trustless-gateway.link/ipns/<name>?format=ipns-record`), verify the IPNS signature (V2 data), decrypt. No vault found (never published, or older than the DHT keeps): fall back to the **key file's snapshot**, then to probing indices 0–15 (D.3 channel names) for their channel records on the routing API.
3. **Fetch the content** of each channel, first source wins, all verified by CID and signatures as a reader does (D.6): the channel's own onion (another device hosting it), the mirrors listed in the vault, the storage provider / gateway caches (`trustless-gateway.link`, CAR by root CID).
4. **Nothing answers: continue without history.** The device knows the root and head-page CIDs, the count, the manifest fields and the sequence from the vault. It rebuilds and re-signs the manifest, starts a **new head page whose `prev` is the old head page's CID** (D.5.1), and posts on top: `sequence` and `count` continue, and every reader sees one chain. The UI says "N older posts are not on this device; they appear when your other device, a mirror or your storage provider is online". Back-fill is automatic: when the old head page becomes fetchable, it is verified against the CID already linked and stored.
5. **One writer at a time (the lease).** Two devices posting at once would fork the chain (the higher `sequence` wins on IPNS, the other device's posts drop out), and two tabs hosting the same channel onion compete for its descriptor. So the vault carries a lease: the device that hosts/posts writes `{its id, now + 15 min}` and renews it while it runs. Another device that finds a live lease reads the channel (as a follower would) and offers **"Take over here"**; taking over writes a new lease, and the old device, on its next renewal, sees it lost the lease, stops serving the channel onion and says so. An expired lease (the other device is off) is taken silently. Before every post the writer re-reads the channel's latest record, so a stale device never overwrites a newer version.

#### D.11.4 Optional: keep the whole channel online with a storage provider

- Settings → "Keep a copy with a storage provider": the user pastes an API token of an IPFS pinning/storage service that accepts **CAR uploads over HTTPS with CORS** (candidates to test in spike V-P3: Pinata, Filebase, Storacha). The owner's tab uploads each new CAR (the diff since the last upload) through a Tor exit.
- Effect: the channel is readable **by everyone** from `trustless-gateway.link` while all devices and mirrors are off, and a new device gets the full history (step 3).
- Costs, shown before enabling: a third party (its IP is the Tor exit's); the **account** may identify the user if it was opened with a real email or payment, which links that person to the channel; free tiers have size limits. The token lives only in the vault and the key file, encrypted.

#### D.11.5 Security notes

- The vault is public ciphertext: anyone can fetch it; only the seed opens it. The vault name, the channel names and the channel onions are all independent HKDF outputs, so an observer cannot group them; the DHT and the routing API see only Tor exits.
- A key-file thief gets the vault too (as they get the channels today, D.3): the identity is the root secret either way.
- A replayed old vault record is refused by `sequence` once a newer one was seen on this device; on a brand-new device the worst case is a stale state, and step 5 still re-reads each channel's own (independently signed) latest record before writing.

#### D.11.6 As built (V-1…V-3)

- **Rust** (`crates/channel/src/vault.rs`): `Vault { lease {device (16 bytes), until}, entries [{index, title, about, created, mirrors, head, count, last_seq, record_seq}] }` as dag-cbor `{v: 1, dev, until, ch: […]}` (short keys), padded to 512/1 024/2 048/4 096/5 632 bytes (`about` texts, then mirror lists, are dropped if that is what it takes to fit), sealed with XChaCha20-Poly1305 (AAD `"ephem-vault-v1" ‖ sequence`, a random 24-byte nonce per record) and signed as an IPNS V2-only record (`ipns::create_v2`). Keys: `Identity::vault_seeds` (`"p2pchat/vault-sign"`, `"p2pchat/vault-key"`).
- **Continuing without history** (`Channel::resume`, `backfill`): new pages chain onto the old head; `count` and `seq` continue. `read_dag` stops at the first page it does not hold and reports the posts before it as `missing` (the view's JSON, the owner's note, the plain page's footer) instead of failing; `seq`s must run 1, 2, … without gaps, and a chain cannot claim more or fewer posts than it links. A resumed chain leaves a partly filled page, so the loop guard is "more pages than posts".
- **Wasm** (`ChannelApp`): `vault_name`, `vault_fetch`, `vault_publish(host, root, device, until)`, `vault`, `resume`, `backfill`, `release` (stop hosting: serve loops end when their onion service is dropped), `channel_onion` (a channel's onion address, the same on every device). HTTPS through a Tor exit does any method and returns the body (`Content-Length`, chunked, or to the end).
- **Page** (`app/channels.js`): a random device id per browser (localStorage). Once Tor is up and an identity is signed in: fetch the vault; a live lease of another device → this tab stops hosting (**"written by your other device"**, **Take over here**); otherwise it writes: each vault channel missing here is read from its onion and mirrors (90 s), else resumed without history; the vault is published with a lease of **15 min**, renewed every 5 min (after checking nobody took over; a lost lease stops hosting and says so), and 2 s after every change. Missing older posts are back-filled from mirrors at each renewal.
- **Lab** (`checks/tor-lab/e2e_tor_vault.mjs`, lease 30 s, 8/8): A publishes (a 1 056-byte, opaque record) → B signs in: listed as written by the other device → B takes over (the newest version read from the channel's onion in < 1 s) → A notices at its next renewal → A and B close → C waits out B's lease, finds no host, continues without the 2 older posts (the sequence continues), posts `seq` 3 and serves it.
- **Found on the way:** since the plain page (D.2), the owner's tab re-verified the whole channel on every post to rebuild that page (1 000 posts: 58 s instead of 1.1 s). The owner's page now comes from the state it just signed (`Hosted::owned`): 1 000 posts in 1.1 s again (C-P3).

#### D.11.7 Phases and spikes

| ID | Scope / question | Done when | Status |
|---|---|---|---|
| V-P1 (spike) | `delegated-ipfs.dev` accepts and returns a V2-only record of ~9.5 KiB with an identity-CID value; `trustless-gateway.link` returns it | Live PUT/GET byte-identical, as C-P4 | ✅ Live 2026-09-29 (`NODE_USE_ENV_PROXY=1 node checks/vault_spike.mjs`): a 9 248-byte V2-only record with a 5 672-byte inline identity CID accepted (`PUT` 200) and returned byte-identical by the routing API and by `trustless-gateway.link` |
| V-P2 (spike) | How long the DHT keeps the record without republishing | Measured over 72 h (resolve every hour) | ⏳ running: record `k51qzi5uqu5dgvhn5btbh9q1hrrcgbdwz00a4qwwdd0w5pz9syqsu5jpjfisq5` published 2026-09-29 16:22 UTC, never republished; `CHECK=<name> node checks/vault_spike.mjs` |
| V-1 | `channel::vault`: keys, dag-cbor, padding, seal/open, V2-only record | Unit tests | ✅ tamper (every 97th byte), wrong key and name, the largest vault ≤ 10 KiB |
| V-2 | Publish on change and on every lease renewal; restore on sign-in; fetch from the channel's onion and mirrors | Lab E2E | ✅ `e2e_tor_vault.mjs` |
| V-3 | Continue without history; back-fill; the writer lease and "Take over here" | Lab E2E | ✅ `e2e_tor_vault.mjs` (back-fill: unit tests; in the app from mirrors) |
| — | Key-file snapshot (the fallback once the DHT forgot the vault) | | ⏳ not built: the vault is the only source for now |
| V-P3 (spike) → V-4 | Storage providers with CORS CAR upload; the optional setting | Live: upload through Tor, read back from the gateway | ⏳ |

### D.12 Boards: a 4chan-like channel type (proposed)

A second channel type, next to the channels above (which stay as they are): boards → threads → replies, anyone with the link posts anonymously through Tor, proof of work instead of captchas, the owner's tab numbers, moderates, signs and serves. The full design is Appendix G, in [`docs/BOARDS.md`](BOARDS.md).

## Appendix E: Features considered from Tox/qTox and Telegram

**Rule:** a feature is adopted only if it works with **no application server**. It may use the peers' own devices and free, no-registration third-party services.

### E.1 Adopted

| Source | Feature | Here |
|---|---|---|
| Tox | Identity = key pair; passphrase-encrypted profile | §7.1, §7.3 |
| Tox | Friend list of verified keys; "nospam" | Contacts and contact cards (§7.5) |
| Tox (NGC) | Group roles | Owner, member, observer (§14.2) |
| qTox | "Will send when online" | Pending queue (§11.3) |
| Telegram | Secret-chat emoji key visualisation | SAS emoji (§10.4) |
| Telegram | Self-destruct timers, reply, edit, delete, reactions, typing, read ticks | §11.7 |
| Telegram | QR login to link a device | Identity transfer over P2P (§7.6) |
| Telegram | Several accounts | Several identities (§7.2) |
| Telegram | Channels (owner posts, others read) | Public channels (§27, Appendix D) |

### E.2 Rejected

| Feature | Needs | Why not |
|---|---|---|
| Tox DHT discovery | UDP sockets or a DHT in the browser, plus bootstrap nodes | Browsers cannot; Tor onion addresses give contact reconnect instead (§28.7) |
| Tox TCP relays, TURN | A relay | P8 |
| Cloud history, sync, offline mailbox | A server | P7, §4.3 |
| Usernames, global search, channel view counters | A directory or counter service | P1 |
| Push notifications, bots | A server | P1 |
| Automatic link previews | Fetching the link | Reveals the IP to third parties |
| Channel comments | Write access for readers | Breaks read-only; "message the owner" through a normal invite instead |


## Appendix F: v1.2: Tor bridges, and one app with Chats, Following and My channels

**Status: approved by the owner with D1–D6 as recommended (2026-09-29), and built** (F.5: as built, and what differs from this proposal). F.1–F.4 are the proposal as approved.

### F.1 Where we start from

| Fact (as built in v1.1) | Consequence for this proposal |
|---|---|
| One page holds **one** conversation: the adapter's `Inner` has one session **or** one room, and the event meta carries no chat id | Several chats at once is a **core change** (F.3.2), not only a UI change |
| Three wasm builds: direct `ephem` (234 KB gzip), Tor `ephem_tor` (2.13 MB), channels `ephem_channel` (2.05 MB). The two big ones each contain arti | Chats and channels in one Tor page would run **two Tor clients** unless the builds merge (F.3.3) |
| Mode is per page: `index.html` direct, `tor.html` Tor, `channel.html` always Tor | The "this tab never shows your IP" promise of a Tor tab is simple to state and to test |
| Tor is reached only through the Tor Project's Snowflake (two brokers, two bridges, fixed STUN list in `app.js`); the brokers are in the page's CSP `connect-src` | A custom broker is **blocked by the CSP** today (F.2.4) |
| P7: no chat history, ever | A Telegram-like chat **list** is possible; a message **history** is not (F.4, D1) |

### F.2 Tor bridges (optional setting)

#### F.2.1 What a browser page can and cannot use

A page has no raw TCP or UDP. It has `fetch`, WebSocket, WebTransport and WebRTC. So of Tor Browser's bridge types:

| Bridge type (Tor Browser line) | In our page | Why |
|---|---|---|
| `snowflake …` with its own `url=` (broker), `fingerprint=`, `ice=` | **Yes** | Everything it needs is `fetch` + WebRTC, which we already run |
| `snowflake … ampcache=…` (AMP cache rendezvous) | **Spike** (BR-4) | A `GET` through `cdn.ampproject.org`; works only if the AMP cache answers cross-origin reads (CORS). Unknown until tried |
| `snowflake … front=…` / `fronts=` (domain fronting) | Ignored, with a note | A browser cannot send a `Host` different from the URL. The URL itself is still used (as we do with the CDN77 URL) |
| `snowflake … utls-imitate=…` | Ignored | The browser's own TLS is used; it already looks like a browser |
| `webtunnel …` | **No** | The server expects raw bytes after the HTTP upgrade; a browser WebSocket always adds WebSocket framing. Making it work needs a server-side change (a companion program, excluded by P10) |
| `obfs4`, `meek_lite`, `conjure`, `dnstt`, plain `IP:ORPort` | **No** | Raw TCP/UDP, or a forged `Host` header |

**What this means in practice:** "bridges" in Ephem are **alternative Snowflake setups**:
- another broker URL, for example a CDN name not blocked where the user is;
- another STUN list, for where Google's STUN is blocked;
- other Snowflake bridge fingerprints;
- or a **self-hosted Snowflake stack**, the Tor Project's own broker, proxy and server, the same as our lab. Running it is the operator's choice; we build nothing for it (P10).

Lines of a type the page cannot use are **rejected with the reason**, never silently dropped.

#### F.2.2 Behaviour

- **Settings → Tor connection**:
  - "Automatic (Tor Project Snowflake)", the default and today's behaviour;
  - "Custom bridges": paste one or more bridge lines, the Tor Browser format, one per line.
- **Parsing:** lines are parsed in Rust (sans-IO, in `crates/tor`: `bridge::parse(&str) -> Result<BridgeLine, BridgeError>`, no allocation per field, only slices into the pasted text). The result is the existing `SnowflakeParams { brokers, fingerprints, ice, nat }`. Several lines give several brokers and bridges. The existing `WarmPool` per bridge and the broker fallback already handle more than one.
- **Validation before use:**
  - fingerprint: 40 hex characters;
  - broker: `https:` only;
  - ICE: `stun:` only, since TURN is rejected by P8;
  - at least one usable line.
  Each rejected line shows its reason.
- **Falling back:**
  - "Also use the default Snowflake if my bridges fail" is **off** by default. Someone who chose bridges may not want to touch the default broker.
  - The Tor status line always says which set is in use.
- **Storage:** bridge lines for a private bridge are semi-secret (they reveal the bridge).
  - Signed in: they go in the **encrypted key file**, as a new TLV `0x05 TOR_BRIDGES`. Unknown TLVs are already kept by older versions (§7.3), so this is backward compatible.
  - Temporary identity: they are kept in RAM only, and re-pasted after a reload, with a clear note.
- **Sharing:** "Share these bridges" gives a `#b=` link / QR (kind 7). Opening it pre-fills the setting and never applies it silently.
- **Directory snapshot:** the IndexedDB snapshot (§28) is independent of the bridge choice and stays as is.

#### F.2.3 Plan

| # | Deliverable | Checkpoint |
|---|---|---|
| BR-1 | `bridge::parse` (+ `to_line`), unit tests from Tor Browser's shipped lines, fuzz target | Every line type in F.2.1 is accepted or rejected with the right reason; fuzz 10⁶ runs clean |
| BR-2 | Settings UI, TLV `0x05`, `#b=` kind 7, status line | APP-E2E-TOR-BRIDGES in the lab: the lab's broker and bridges pasted as lines → bootstrap and chat; a line with only a dead broker → the reasoned error; the default path unchanged |
| BR-3 | CSP change (F.4, D5) | CSP check in `checks/`: the Tor page loads, a custom broker is reachable, nothing else widened |
| BR-4 (spike) | AMP cache rendezvous from a page | Live: a CORS answer from `cdn.ampproject.org` or not. Adopt only if yes |

#### F.2.4 The CSP question

`connect-src` lists exactly two brokers today. A pasted broker URL is blocked by the browser. There are three ways to allow it:
1. **`connect-src 'self' https:` on the Tor pages** (recommended):
   - WebRTC, STUN and the Snowflake proxies are not governed by `connect-src` anyway. A script able to misuse `connect-src` could already exfiltrate over WebRTC.
   - `script-src` stays hash-pinned with no inline scripts, so script injection is the real barrier, and it is unchanged.
2. **A fixed allowlist** of known brokers and CDN names, extended by us in releases. This is safer on paper, but a user in a censored place cannot add the name that works for them.
3. **Custom brokers only on a self-hosted copy of the app** with its own CSP. That is correct, but useless to most people.

### F.3 One app, three tabs (Telegram-like)

#### F.3.1 The layout

```
┌─────────── desktop (≥ 900 px) ───────────┐    ┌──── phone ────┐
│ ☰ Ephem · alice · Tor ●      [+ New]     │    │ Chats      [+]│
├──────────────┬───────────────────────────┤    │ ● Bob      2  │
│ Chats (3)    │ Bob  · via Tor · 🔒 SAS ✓ │    │ ○ Carol       │
│ ● Bob     2  │                           │    │ ● Room: ops   │
│ ○ Carol      │  messages…                │    │               │
│ ● Room: ops  │                           │    │               │
│──────────────│                           │    │               │
│ Following    │                           │    ├───────────────┤
│ My channels  │ [ message…         ] Send │    │ 💬  📰  ✎      │
└──────────────┴───────────────────────────┘    └───────────────┘
```

Three tabs: **Chats**, **Following** (channels you read), and **My channels** (channels you own and post to). On a phone they are a bottom tab bar, and a row opens the conversation full screen with Back. On a desktop the list and the open pane sit side by side, in a single page and a single identity.

| Tab | Rows | Row shows | Open pane | "+" |
|---|---|---|---|---|
| **Chats** | Live chats and rooms of this tab, then saved contacts | Nick, identicon from the peer key, online dot, "via Tor"/"direct", last message preview (**RAM only**), unread count | Today's chat or room view, unchanged in behaviour (SAS, receipts, reply, edit, timers, members) | Invite · Create room · Add a card · Paste/scan a code |
| **Following** | Channels you follow | Title, newest post preview, new-post count, where it was read from (owner/mirror/gateway), "mirroring" badge | Today's reader view: posts, Refresh, Mirror on/off, Kubo, gateway | Paste/scan a channel link |
| **My channels** | Channels you own | Title, "online through Tor"/"offline", post count, mirrors | Today's owner view: composer, link/QR, mirrors, backup, publish to IPFS | Create a channel · Restore from backup |

A counter on each tab shows the unread total, from this tab's RAM and the follow list's high-water marks. Notifications: while the tab is open, a system notification says only "New message" or "New post in <channel title>". It never shows message text, which would go to the OS notification store and break P7.

#### F.3.2 Several chats at once (core change)

- **Adapter:**
  - `Inner` gets a **fixed-capacity table of conversations** (`MAX_CHATS = 16`, preallocated at start, no allocation afterwards). Each slot is a 1:1 session or a room, with its links, pending queue and sequence state.
  - Scratch buffers (`rx`, `text`, the meta slot) stay **shared and single**. The tab is single-threaded and frames are processed one at a time, so per-chat buffers would only add memory: 16 × 16 KB for nothing.
  - Every API call that acts on a chat takes a `chat: u8`. Every event's meta gains a `CHAT` byte.
  - Invites and cards map to a chat by `invite_id` / card secret, in a small fixed table.
- **Routing, direct mode:** one `RTCPeerConnection` per 1:1 chat, as now, times N. Rooms keep their mesh. Browser limits (Chrome has hundreds of peer connections) are far above 16 chats + 16 room members.
- **Routing, Tor mode:** one onion for all 1:1 chats, as today. An incoming stream is matched to its chat by the Noise IK static key: `contact_host` already tries sessions in turn and becomes a key lookup. Each room member link is its own stream. **Zero-copy is unchanged:** a Tor cell payload is read into the shared `rx` buffer once, then decrypted in place, as now (the one existing copy, arti's `DataStream` → `rx`, stays documented in §22).
- **Safety in a multi-chat UI:**
  - the chat header always shows the peer's nick, transport and SAS state;
  - the composer's draft is per chat (RAM);
  - switching chats never carries a draft over;
  - removing a chat or leaving a room zeroizes its slot at once.
- **Tests:**
  - native: two sessions in one `Inner`, frames interleaved, each delivered to its own chat, and a slot reused after zeroize;
  - e2e: Alice chats with Bob and Carol at the same time (direct and Tor), with messages, receipts and a redial in one chat that does not disturb the other.

#### F.3.3 Chats and channels in one Tor client

- **Builds go from three to two:**
  - `ephem-channel-web` is folded into the Tor build of `ephem-wasm` (feature `tor`), so chats and channels share **one** arti client, one Snowflake pool and one directory snapshot. That saves about 2 MB of download and one Tor client's memory: the iOS concern (G4) is exactly this.
  - The direct build stays small (no Tor).
- **In direct mode**, "Following" and "My channels" load the Tor build **lazily**, the first time one of them is opened, and use it **only for channels**. Chats in that page stay direct.
  - The page then shows two mode pills: "Chats: direct" and "Channels: Tor". No direct chat ever goes through Tor, and no channel request ever goes out directly (except the explicit "Read without Tor" gateway button).
- **Old links keep working:** `channel.html#c=…` becomes a small redirect page into the app's Following tab with that link pre-filled. `channel.html` without a fragment opens My channels.
- **Following list:** per identity, one entry per followed channel: IPNS name, onions, high-water sequence number, "mirror it" flag. It is stored per decision D2. Refresh:
  - on open, every 10 minutes while the tab is open, and on demand;
  - rounds in parallel, as the reader does now;
  - capped at 4 channels at a time, so Tor circuits aren't flooded.
- **My channels:** several owned channels per identity (`channel_seeds(index)` already supports this), each served on its own onion while the tab is open (`Tor::launch` already runs several). The list of owned indices is kept per decision D3.

#### F.3.4 Plan

| # | Deliverable | Checkpoint |
|---|---|---|
| UI-1 | Multi-chat core: conversation table, `chat` in API and events, Tor key routing | Native tests (F.3.2); the existing e2e suites unchanged (one chat is slot 0) |
| UI-2 | App shell: tabs, list + pane, responsive, "+" sheet, per-chat drafts and unread counts, notifications without content | APP-E2E-MULTI: A with B and C at once, direct and Tor; phone viewport run; axe accessibility check clean |
| UI-3 | One Tor build with channels; lazy channel tabs in direct mode; `channel.html` redirect | APP-E2E-TOR-CHANNEL passes inside the app; direct page never loads the Tor build until a channel tab opens (network check) |
| UI-4 | Following: follow list, refresh scheduler, new-post counts, mirror toggle | e2e: follow 3 channels, owner posts, counts update, one channel offline → read from its mirror |
| UI-5 | My channels: several owned channels, each online on its onion | e2e: 2 owned channels served at once, both readable |
| UI-6 | Polish: identicons, empty states, keyboard shortcuts, strings in one table (ready for translation) | Visual check on desktop Chrome/Safari/Firefox and iPhone |

**Order:** BR-1…BR-3 (small, independent), then UI-1 → UI-2 → UI-3 → UI-4 → UI-5 → UI-6. UI-1 is the risky one: it touches the core every flow uses. It gets the full regression suite before anything is built on it.

### F.4 Decisions for the owner

| # | Question | Options | Recommendation |
|---|---|---|---|
| D1 | Chat history in the Chats tab | (a) **keep P7**: the list shows contacts and live chats, and messages vanish when the tab closes; (b) optional encrypted local history | **(a)**. History changes the threat model (a seized device reveals chats) and the product promise |
| D2 | Where the follow list lives | (a) **in the encrypted key file** (TLV `0x06 FOLLOWS`); (b) plain in the browser (OPFS/localStorage) | **(a)** when signed in. It reveals what you read. A temporary identity keeps it in RAM |
| D3 | Which identity owns channels | (a) **the chat identity** (channel keys are derived with HKDF and are unlinkable in public; they are linkable only from the key file); (b) a separate publisher identity, signed in inside My channels | **(a) by default, with (b) offered** ("use a separate identity for channels") next to today's warning |
| D4 | Direct and Tor chats in one tab | (a) **one mode per tab for chats** (channels always Tor); (b) mixed, chosen per chat | **(a)**. "This tab never shows your IP" stays a one-line promise; mixed mode makes a leak a mis-click away |
| D5 | CSP for custom brokers | F.2.4 options 1–3 | **Option 1** (`https:` on the Tor pages) |
| D6 | Concurrent chats per tab | 8 / **16** / 32 | **16**. Memory and UI stay sane; a room already counts as one chat |

### F.5 As built (2026-09-29)

| Part | As built | Proof |
|---|---|---|
| BR-1 | `crates/tor::bridge::parse`: lines → brokers, bridges (up to 4, at placeholder addresses 192.0.2.3–6) and STUN, each the union in order; `snowflake` only; obfs4, meek, WebTunnel, conjure and plain bridges refused with their reason; `front=`/`utls`/`ampcache=`/TURN noted and ignored; `http://` only to loopback (the lab) | unit tests from Tor Browser's lines; mutation fuzz, 10⁶ runs clean |
| BR-2 | Settings → Tor connection (automatic, or my own bridges + optional fallback); TLV 0x05 (RAM for a temporary identity, moved into the key file when it is saved); a per-browser flag makes the next start wait for sign-in; `#b=` links fill the setting only; the built-in setup is itself bridge lines (`bridges.js`) | APP-E2E-TOR-BRIDGES 9/9 |
| BR-3 | `connect-src 'self' https:` on every app page (D5) | all suites: no CSP violation |
| BR-4 | AMP cache rendezvous: not done (spike; `ampcache=` lines are refused unless they also have a `url=` broker) | — |
| UI-1 | Adapter: one chat loaded in `Inner`, up to 15 parked (shells preallocated, empty chats recycled); every entry point loads its chat first (`focus`, or `find` by link id); events carry the chat id (meta byte 17); an incoming Tor stream is offered to every chat, a contact or card dial gets a new chat; `E_TOO_MANY_CHATS` | native tests (park, load, recycle, 16-chat limit); every earlier suite unchanged |
| UI-2 | Shell: tabs and lists beside the pane (≥ 900 px), or the list then the pane with Back and a bottom tab bar (phones); one clone of the chat views per chat, only the one on screen attached; unread counts per chat and on the tab; opt-in notifications without message text; an incoming chat takes the screen only from the home or an ended chat | APP-E2E-MULTI 12/12, APP-E2E-TOR-MULTI 6/6 |
| UI-3 | The channel code is part of the Tor build; one Tor client per Tor page; the direct page loads the Tor build (SRI-pinned) only for its channel tabs; `channel.html` redirects | APP-E2E-TOR-CHANNEL 19/19 (network check included) |
| UI-4 | Following: follow list (TLV 0x06), refresh on tab open and every 10 min (4 at a time), new-post counts, mirrors resumed after a reload | APP-E2E-TOR-CHANNEL |
| UI-5 | My channels: up to 16 per identity, found in the store, each online on its own onion | APP-E2E-TOR-CHANNEL (two at once) |
| UI-6 | Identicon avatars (a colour and a letter from the name, not an identity), keyboard shortcuts (Alt+1/2/3 tabs, Alt+↑/↓ chats, Escape back on phones), empty states. **Not done:** moving every UI string into one table for translation (error messages already are) | visual check; APP-E2E-MULTI (ARIA) |

**Open:** live runs of bridges, several chats and the channel tabs on the real Tor network and on the iPhone; BR-4; the string table for translation.

### F.6 After the owner's first v1.2 runs (2026-09-29)

| Report | Change |
|---|---|
| iPhone Home Screen app scrolled as a whole | The app is a fixed full-screen frame (`100dvh`, no page scroll, no overscroll bounce); only the tab lists and the pane scroll; the status bar area is kept clear (`black-translucent`, safe-area insets). Checked at 390 px (APP-E2E-MULTI) |
| The bottom tab bar disappeared on phones when a pane opened | The tab bar is a row of its own in the frame, always visible |
| Settings and identity management mixed into New chat | A fourth tab, **Settings**: Tor status and bridges, identity (save, sign in, transfer, contact card), contacts, connection and privacy, About (protection limits, license, build). New chat keeps invite, room and "Got a code?" |
| My channels asked to sign in again after switching tabs | Two causes: every return to Chats re-bound the chat identity to the channels, which dropped a separate channel sign-in (now `bind` never replaces an explicit `sign_in`, and the page re-binds only when the identity really changed); and the owned list was emptied while being rescanned (now swapped in whole) |
| A reader on the iPhone could not reach a channel the laptop showed "online" | "Online" came right after launch, before arti had published the onion's descriptor. Readers whose clock or consensus picks the HSDir ring without the descriptor fail ("Failed to obtain hidden service circuit"). The page now shows arti's state: until it reports the service fully published (both rings), chats and channels say "Tor is still publishing your address: some peers/readers may not reach you for a minute or two" (`Service::reach`, `App::tor_reach`, `ChannelApp::reach`). Followers also add the mirrors the owner signed to the addresses they read from |
| "Got a code?" should be reachable from everywhere | A **⌗ Code** button in the header, on every tab: it opens "Got a code?" (paste or scan an invite, answer, reconnect code, contact card, channel or bridge link) as a sheet over the current view; Escape or Close dismisses it |
| Channels readable without Ephem (owner's request) | Every channel onion serves `GET /` as a plain web page for Tor Browser (`crates/channel::page`): title, about, the 200 newest posts (replies, deletions), the version; no scripts (`Content-Security-Policy: default-src 'none'; style-src 'unsafe-inline'`), no referrer. The owner's page says the onion address vouches for it (its key belongs to the channel); a mirror's page says it is a mirror that checked the signatures, and that its address vouches for the mirror only. My channels shows the `http://…onion/` address. Lab: fetched through a C Tor client (APP-E2E-TOR-CHANNEL 21/21) |
| Back from Settings on a phone led to a stray list | On phones Settings opens straight to its page with no Back; Chats, Following and My channels open on their lists, a row opens its page and Back returns to the list (APP-E2E-MULTI 18/18) |
| Signing in "did nothing" (laptop and iPhone) | Could not be reproduced in the lab; the likely cause: its refusal (wrong passphrase, identity open in another tab, or chats still open) was shown at the top of a scrolled Settings page. Errors now stay pinned in view (tap to dismiss); "chats still open" says how many and that they must be closed; a sign-in confirms itself in the status chip |
| "Tor ready" not always shown | It was set only if the New chat page was on screen when Tor came up; now whenever no chat is on screen |
| Both devices stayed "disconnected" after a while | Only the dialler redials (§28.5); the host waits, and both said "Reconnecting". Now: the host says it waits for its peer to dial back; the dialler shows each attempt and the last error (ev::REDIAL); it redials at once, on fresh circuits, when the page comes back to the foreground, when the network returns, and on **Reconnect now** (`App::tor_redial_now`) instead of waiting out a backoff of up to 16 s (a background tab may run its timers only once a minute) |
| Typing on the iPhone: the page slid up under the status bar, the header disappeared, the tab bar floated above the keyboard with an empty gap under the composer | The frame follows `visualViewport`: its height is the visible area above the keyboard (`--vvh`), the page is held at the top, and the tab bar is hidden while a field has the keyboard (`body.kbd`), so the header stays and the composer sits right on the keyboard. The chat fills the pane (flex columns down to the composer; a percentage min-height did not pass through `.chatroot`), which removes the gap. The composer's 14 px font had overridden the 16 px touch-screen rule, so iOS zoomed in on focus; now 16 px. Check: `checks/e2e_keyboard.mjs` |
| On the iPhone the New chat / Follow a channel / New channel button was at the top | On phones it is in the middle of the screen while the list is empty, otherwise a floating button centred at the bottom, above the tab bar (within thumb reach). The contact avatars' styles, lost in the Settings-tab change, are back |
| Censorship: a network that blocks the default STUN servers | Settings → Connection: **own STUN servers** (direct mode; up to 4 `stun:host[:port]`, never `turn:`, kept in localStorage, empty = the default list; `App::set_stun`, `rtc::parse_stun`); new links use them. The other hard-coded endpoints and what bypasses each: the Snowflake broker → bridge lines with another `url=` (F.2); Tor relays, directories and the IPFS routing host are reached through Tor; no bypass for a network that blocks WebRTC/UDP altogether (P10) |
| First runs on the real Tor network from the build container (`RELAY=1`, 2026-09-29) | app 20/20, channels 16/16, multi-chat 6/6, rooms 9/9, cards 6/6, vault 8/8 (twice). Three bugs the lab could not show, fixed: (1) **routing answers are cached by URL** (a newer vault record stayed invisible ~5 min on delegated-ipfs.dev): vault reads send `Cache-Control: no-cache` and a fresh `?fresh=` query, and are then current at once (also on trustless-gateway.link); (2) **a renewal can race a takeover** with the same sequence and the routing service keeps one: on a tie the network's record now wins, and a device that took over re-claims for one lease (checking every 15 s) while any other writer steps down; (3) **a card request can arrive before the dialler's HELLO**: the contact was saved under the handle and never got the nickname; now it is saved without a name and filled in when HELLO arrives. Also: for an unknown name delegated-ipfs.dev answers **200 with the text "routing: not found"**, now read as "no vault" |
| Contacts were spread over Settings, the Chats list (Tor only), the identity card and the Code sheet | One place, per `docs/CONTACTS-UX.md` (owner's choices C1–C5): contacts are rows of the Chats list in both modes (Connect / Invite; one row per person); a person pane with inline rename, remove with undo, and a **contact fingerprint** from both keys to verify without a chat; **＋ New → Add a contact** (paste/scan a card with its suggested name, share yours); no browser dialogs for contacts; Settings keeps only "Reset my contact card" |
| A channel created on the laptop did not show in an incognito window with the same identity | The new device listed nothing until Tor was up, the vault was read and **every** channel restored one after another (up to 90 s each), with nothing on screen; and it found nothing at all if the other device had never published a vault. Now: a line under My channels says what it is doing; vault channels are listed at once as "restoring…" and restored side by side; without a vault, the identity's first 4 channel onions are asked directly (a device that still serves them answers); when nothing is found it says why and what to do |
| Notifications inside the app | In-app notices (top of the screen, 6 s, at most 3, one per source with a count): a message in a chat that is not on screen (or while on another tab), a chat that connected, a card holder who wants to connect, new posts in a followed channel. A tap opens the source. Never the message text, only who and how many (as the system notifications, P7). `e2e_multi` and `e2e_tor_channel` check them |
| iPhone Home Screen app: the update banner under the status bar, a gap above the header, the header on two lines, a strip under the tab bar | The banner takes the status-bar inset and the header below it then does not; the header stays on one line (the status chip shortens, the handle hides under 480 px); the frame follows `visualViewport` only while the keyboard is up. The strip under the tab bar stayed: in the Home Screen app with the translucent status bar, iOS computes `100dvh` as the screen **minus the status bar** (≈ 59 pt); the frame is now pinned to the four edges (`position: fixed; inset: 0`) instead of `height: 100dvh` |
| License | PolyForm Noncommercial 1.0.0 (LICENSE, NOTICE); SPDX headers on every source file, stamped before each commit (`tools/spdx.py`, `.githooks/pre-commit`, a Claude Code hook) |
