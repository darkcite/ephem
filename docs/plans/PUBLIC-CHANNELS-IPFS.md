# Plan: Permanent public channels on IPFS (owner-only posting, read-only for others)

| Field | Value |
|---|---|
| Status | **Proposal**. Nothing here is part of SPEC v0.3 until the owner approves it (see §12) |
| Relates to | [`SPEC.md`](../spec/SPEC.md) v0.3 |
| Constraint | Every IPFS component must be **free and need no registration**: no paid pinning, no accounts, no API keys |
| Date | 2026-09-28 |

---

## 1. Goal

A user can optionally create a **public channel**:

- It has a stable link and is **permanent**: it stays available as long as someone pins it.
- **Only the owner can post.** Everyone else can only read.
- The content is stored on **IPFS** and pinned from a folder on the **owner's own device**.
- There is no server, no paid pinning service and no registration.

## 2. Is it possible? Yes, with three hard constraints

| # | Constraint | Why | Consequence |
|---|---|---|---|
| C1 | **The owner must publish from a desktop running a local IPFS node**: Kubo, or IPFS Desktop, which bundles Kubo. Both are free, open source and need no account | A browser tab cannot be a reliable IPFS host. It stops serving when the tab closes, and browser nodes cannot accept incoming connections, so public gateways cannot fetch from them. iOS cannot run Kubo at all | The owner publishes from desktop Chrome, Edge or Firefox (desktop Safari to be checked in spike P2). **Readers can use any target, iPhone included** |
| C2 | **Availability = the owner's machine uptime, plus followers who pin** | "Free + no registration" excludes every pinning service (Pinata, Filebase, web3.storage and others all need accounts) | When the owner's computer is off and no follower pins the channel, new readers cannot load it. Followers who run Kubo can pin a channel to keep it available (Phase C4) |
| C3 | **Public data is permanent and public** | Anyone can copy content-addressed data. Gateways cache it. Followers pin it | "Delete" only removes a post from the latest version of the channel. Old copies may survive. The UI MUST say so before the first post |

## 3. How this fits the existing principles

Public channels are **publications, not chats**. They are a separate feature with their own screen and their own page. Two parts of the spec must be amended if this plan is approved:

- **P7 (no history)** and **§4.3 (IPFS message storage forbidden)** stay fully in force for **private chats**. The amendment adds one explicit, opt-in exception: *"Public channels (§X) are permanent public publications on IPFS; no private-chat data may ever enter them."*
- **P1 (no application backend)** still holds:
  - the owner's Kubo is software the user runs on their own device;
  - public gateways are optional third-party infrastructure, like STUN;
  - they are untrusted, because every byte is checked against its hash and signature (§6).

## 4. Architecture

```
 OWNER (desktop)                                         READERS (any target, incl. iPhone PWA)
 ┌──────────────────────────────┐                        ┌──────────────────────────────────┐
 │ PWA  channel.html            │                        │ PWA  channel.html#c=<ipns-name>  │
 │  Rust/WASM `channel` crate:  │                        │  Rust/WASM `channel` crate:      │
 │   build post → dag-cbor      │                        │   fetch IPNS record + CAR        │
 │   sign (Ed25519, owner key)  │                        │   verify record sig + seq        │
 │   compute CIDs, build CAR    │                        │   verify CIDs + post signatures  │
 │   sign IPNS record           │                        │   render, read-only              │
 └──────────┬───────────────────┘                        └───────────────┬──────────────────┘
            │ HTTP 127.0.0.1:5001 (Kubo RPC,                              │ HTTPS (trustless mode:
            │ token-scoped, CORS for app origin)                          │  ?format=car, ipns-record)
            v                                                             v
 ┌──────────────────────────────┐    libp2p / DHT     ┌────────────────────────────────────────┐
 │ Kubo (IPFS Desktop)          │ ◀──────────────────▶│ Public gateways (free, no account):    │
 │  MFS folder /p2p-chat/<name> │    bitswap           │ trustless-gateway.link, ipfs.io,       │
 │  = the channel folder        │                      │ dweb.link. Untrusted: content          │
 │  pins, provides to DHT,      │                      │ verified in Rust                       │
 │  republishes IPNS            │                      └────────────────────────────────────────┘
 └──────────────────────────────┘
            ▲
            │ optional: followers' Kubo pins the same CIDs (Phase C4)
```

Design choices:

1. **The channel is a folder.** The channel lives in Kubo's MFS (Mutable File System) at `/p2p-chat/channels/<name>/`. It shows up in IPFS Desktop's *Files* view and is pinned for as long as it stays there. This is the "pinning folder on the user's device". Optionally, on Chromium, the PWA also mirrors it to a real disk folder using the File System Access API.
2. **Rust does all the cryptography and content addressing.** Kubo is used only for storage, networking, pinning and publishing. The owner's private key never leaves the PWA, unless the owner chooses automatic republishing (§6.3, option B).
3. **Readers never trust a gateway.** Gateways are used in *trustless* mode: they return the raw CAR or IPNS-record bytes, and the Rust code verifies every hash and signature. A malicious gateway can refuse to answer or serve stale data. It cannot forge or change posts.

## 5. Data model

### 5.1 Identity and the channel address

- A channel needs a **saved identity** (SPEC §7.2). A temporary identity cannot own a permanent channel.
- Channel signing key: `HKDF(seed, "p2pchat/channel/" ‖ u32 channel_index)` → an Ed25519 key pair.
- **Channel address** = its **IPNS name**: the libp2p peer ID of that Ed25519 public key (identity multihash), written as base36 CIDv1. For example: `k51qzi5uqu5d…`.
- Link: `https://<owner>.github.io/p2p-chat/channel.html#c=<ipns-name>`. It also works as a QR code. The address is also valid at `https://dweb.link/ipns/<ipns-name>`.
- Because the key is derived from the identity seed, the owner can **restore channel ownership on any desktop** from the key file (SPEC §7.3).

### 5.2 Blocks (dag-cbor, deterministic encoding)

Why dag-cbor, when the rest of the spec uses fixed binary layouts:

- signatures need a deterministic encoding, which dag-cbor guarantees;
- IPFS tools and gateways can read, explore and validate it with no custom code;
- this is **cold-path** code (a human posts rarely), so the zero-allocation rule does not apply. Parsing on the reader side is still done over borrowed `&[u8]`.

```
Root  (the target of the IPNS name; rewritten on every post)
{ v: 1, kind: "p2pchat/channel",
  manifest: CID<Manifest>, head: CID<Page>, count: u64, updated: u64 }

Manifest  (changes rarely)
{ v: 1, title: str(≤64), about: str(≤512), owner_peer: bytes(32) /*PeerId, optional*/,
  channel_pk: bytes(32), created: u64, sig: bytes(64) /*Ed25519 over the block without sig*/ }

Page  (up to 64 posts; the head page is rewritten, full pages are sealed and immutable)
{ v: 1, index: u64, prev: CID<Page> | null, posts: [Post; ≤64] }

Post
{ seq: u64 /*strictly increasing, never reused*/, ts: u64, body: str(≤4096 B UTF-8),
  reply_to: u64 | null, deleted: bool, sig: bytes(64) /*Ed25519 over (channel_pk, seq, ts, body, reply_to, deleted)*/ }
```

- **Two requests to open a channel:**
  1. fetch the IPNS record;
  2. fetch a CAR with `dag-scope=all` bounded to the root, the manifest and the head page (≤ about 300 KiB).
- Older pages load one CAR request each, as the reader scrolls.
- **Deleting** (C3) rewrites the post in the head page, or in a sealed page by re-sealing it, with `deleted: true` and an empty body. Old CIDs keep the original content, and the UI states this.
- **Attachments are out of scope** here. A future phase could add UnixFS files ≤ 1 MiB linked from posts.

### 5.3 IPNS record

- IPNS V2 record, signed in Rust with the channel key.
- `sequence` = the root's `count`, so it always increases.
- `validity` = now + 7 days. `ttl` = 60 s, which tells gateways and resolvers to refresh often.
- **Rollback protection:** a reader who follows a channel stores the highest `sequence` it has seen in localStorage. This is not secret, and follows are opt-in. A record with a lower sequence is rejected as stale, and the reader then tries the next gateway.

## 6. Owner flow (publisher)

### 6.1 One-time setup (a guided wizard in the PWA)

1. Install **IPFS Desktop** (or Kubo) and start it. Both are free and need no account.
2. The wizard shows the exact config command to copy (run once), then restart Kubo:
   ```sh
   ipfs config --json API.HTTPHeaders.Access-Control-Allow-Origin '["https://<owner>.github.io"]'
   ipfs config --json API.HTTPHeaders.Access-Control-Allow-Methods '["POST"]'
   ipfs config --json API.Authorizations '{"p2pchat":{"AuthSecret":"bearer:<random-token-shown-by-wizard>","AllowedPaths":["/api/v0/dag/import","/api/v0/files","/api/v0/routing/put","/api/v0/routing/provide","/api/v0/name/publish","/api/v0/key/import","/api/v0/id"]}}'
   ```
   - **Scoped token (Kubo RPC authorization):** the PWA can reach only the RPC paths it needs, never the whole RPC (for example `config`, `shutdown` or `pin rm` on other data).
   - This matters because the origin `https://<owner>.github.io` is **shared by every Pages site under that account**.
3. The token is stored inside the owner's encrypted key file (SPEC §7.3, a new optional field), so it is only available after the owner signs in.
4. The browser asks for **Local Network Access** permission (Chromium) the first time the page connects to `127.0.0.1`. The user allows it once.

### 6.2 Posting

1. The owner writes a post, and the Rust code signs it.
2. The head page is rebuilt (or a full page is sealed and a new one started), then the root.
3. The changed blocks are packed into a **CAR** in wasm memory.
4. `POST /api/v0/dag/import` (the blocks are stored and pinned).
5. `POST /api/v0/files/…` links the new root into `/p2p-chat/channels/<name>/` (MFS keeps it pinned).
6. `POST /api/v0/routing/provide` announces the new root and head page on the DHT.
7. The PWA signs the IPNS record and sends `POST /api/v0/routing/put /ipns/<name>` to publish it (spike P3 confirms that Kubo accepts records signed elsewhere).
8. The UI shows "Published · seen by N providers" (from `routing/findprovs`, best effort).

### 6.3 Keeping the channel alive (IPNS republishing)

Records on the DHT expire after about 48 h and must be published again.

| Option | How | Trade-off |
|---|---|---|
| **A (default)** | The PWA re-signs and re-publishes when the owner opens the app, plus a reminder when the record is more than 36 h old | The key never leaves the PWA. The channel name stops resolving on the DHT if the owner does not open the app for more than about 2 days. The content stays reachable by CID, and gateways may keep a cached copy |
| **B (opt-in)** | `key/import` the channel key into Kubo's keystore. Kubo then republishes by itself, every 4 h by default | Fully automatic, but the channel key is stored **unencrypted** in Kubo's keystore on disk. The UI warns about this, and the owner can revoke it with `ipfs key rm` |

### 6.4 Owner exposure (MUST be shown before the channel is created)

- Kubo announces itself as the provider of the channel on the public DHT. **The owner's home IP address becomes publicly linked to the channel.** Avoiding this needs a VPS or a paid pinning service, both of which the "free, no registration" rule excludes.
- Everything posted is public and practically permanent (C3).

## 7. Reader flow (read-only, all targets including iPhone)

1. Open `channel.html#c=<ipns-name>` from a link or QR code. The fragment is stripped from the URL, as in SPEC §8.7.
2. **Resolve the name.** Request `GET https://<gw>/ipns/<name>?format=ipns-record` from the gateways in order, then:
   - check the record's signature against the key in the name;
   - check that it has not expired;
   - check that its sequence is not lower than the last one seen (rollback check).
3. **Fetch the content.** Request `GET https://<gw>/ipfs/<root>?format=car&dag-scope=…`. Rust walks the CAR and checks that every block's multihash matches its CID, then checks the manifest signature and every post signature.
4. Render the posts. There is no compose box, because the channel is read-only by design and a post would fail signature checks anyway.
5. **Readers with a local Kubo** (optional): the page uses `http://127.0.0.1:8080` (the local gateway) first, which reaches the channel without any third party.

**Default gateway list** (free, no account, CORS-enabled; the list is editable; checked in spike P1):

| Gateway | Operator | Default |
|---|---|---|
| `https://trustless-gateway.link` | IPFS Foundation / Shipyard | yes |
| `https://ipfs.io` | IPFS Foundation / Shipyard | yes |
| `https://dweb.link` | IPFS Foundation / Shipyard | yes |

- Requests go to the gateways in sequence with a 4 s timeout each. When a follow is active, the reader keeps the valid response with the **highest** IPNS sequence.
- **Reader privacy:** the gateway sees the reader's IP address and which channel they read. Readers who run a local Kubo avoid this.

## 8. Follower pinning ("help keep this channel alive")

- A reader who runs Kubo can press **Pin this channel**. It uses the same setup wizard, with a scoped token for `/api/v0/pin/add`, `/api/v0/files` and `/api/v0/name/resolve`.
- The PWA copies the channel's root into the follower's MFS at `/p2p-chat/following/<name>/`. That folder is the follower's pinning folder.
- The PWA re-pins when it sees a newer IPNS sequence, but only while it is open.
- **Automatic following** without the PWA open would need a background process, which a browser cannot provide. The wizard can print a copy-paste `cron` or Task Scheduler one-liner for followers who want it: `ipfs name resolve` followed by `ipfs pin add`, which is enough because the data is signed.

## 9. Security

| Threat | Mitigation |
|---|---|
| A gateway forges or changes posts | CIDs are checked in Rust, and every post carries an Ed25519 signature from the channel key |
| A gateway serves an old version (rollback or withholding) | The IPNS sequence check, a stored high-water mark for followed channels, and trying several gateways |
| Someone other than the owner posts | They cannot: only the holder of the channel key can sign posts or IPNS records |
| A compromised web page takes over Kubo | The token-scoped RPC paths (§6.1) and a CORS allowlist. The token only exists after sign-in |
| Another Pages site on the same `github.io` account reaches Kubo | It has no token, so every call is rejected |
| The channel key is stolen | Option A keeps it only in the passphrase-encrypted key file. Rotation means creating a new channel and posting a signed "moved to" notice in the old one |
| Private-chat data leaks into a channel | Separate page (`channel.html`) with its own CSP, and the chat page's CSP keeps `connect-src 'self'` (SPEC §17.3). The channel crate has no API that accepts chat data |

**CSP of `channel.html`**

```
default-src 'none'; script-src 'self' 'wasm-unsafe-eval'; style-src 'self'; img-src 'self' data:;
connect-src 'self' https://trustless-gateway.link https://ipfs.io https://dweb.link
            http://127.0.0.1:5001 http://127.0.0.1:8080;
base-uri 'none'; form-action 'none'; frame-ancestors 'none'
```

`frame-ancestors` is ignored when set in a `<meta>` tag. It is listed so it is not forgotten if the site ever moves to a host that can set headers.

## 10. Implementation phases

| Phase | Scope | Targets |
|---|---|---|
| **P (spikes)** | See §11 | — |
| **C1: `channel` crate** | Seed → channel key; IPNS name; dag-cbor codec for Root, Manifest, Page and Post; CID computation (`cid`, `multihash` with sha2-256); CAR v1 reader and writer; IPNS V2 record signing and verification; native tests with fixtures cross-checked against Kubo output | Native + wasm32 |
| **C2: Reader** | `channel.html`; gateway client; rollback high-water mark; paginated rendering; follow list in localStorage (names and high-water marks only) | All, including iPhone |
| **C3: Publisher** | Setup wizard; Kubo RPC client (dag/import, files, routing/put and provide); posting and deleting; option A republishing; an exposure warning that must be accepted; key-file token field | Desktop Chrome, Edge, Firefox (Safari after spike P2) |
| **C4: Follower pinning** | "Pin this channel" through the reader's local Kubo; re-pin while open; the printed cron one-liner | Desktop |
| **C5: Later (optional)** | Option B republishing; attachments (UnixFS ≤ 1 MiB); unlisted channels (content encrypted with a key kept in the link fragment); publishing from iPhone, where the phone signs the post and sends it over the P2P chat link to the owner's own desktop, which does the Kubo step; direct reads from the owner's Kubo over WebRTC-direct (rust-libp2p `webrtc-websys`), which removes the gateways | — |

## 11. Validation spikes (before C1 is frozen)

| ID | Question |
|---|---|
| P1 | Do `trustless-gateway.link`, `ipfs.io` and `dweb.link` serve `?format=car` and `?format=ipns-record` with CORS to a `github.io` origin, from desktop browsers **and iOS Safari**? What is the real latency for a freshly published name? |
| P2 | Can a page on `https://<owner>.github.io` call `http://127.0.0.1:5001` in Chrome and Edge (Local Network Access prompt), Firefox, and **desktop Safari** (mixed-content rules for loopback)? |
| P3 | Does Kubo's `routing/put` accept an IPNS record signed outside Kubo, for a key it does not hold, and propagate it to the DHT? |
| P4 | Is `API.Authorizations` with `AllowedPaths` enough to confine the token to the paths listed in §6.1, on the current Kubo release? |
| P5 | How long after `routing/provide` does a new root become fetchable through each gateway when the owner is behind a home NAT (AutoNAT, hole punching, public relays)? |
| P6 | How much do `cid`, `multihash`, the dag-cbor codec and the CAR code add to the wasm binary? |

## 12. Decisions needed from the owner

| # | Question | Proposed default |
|---|---|---|
| D1 | Is it acceptable that **only a desktop running IPFS Desktop or Kubo can publish** (iPhone can read, and can publish later through C5)? | Yes |
| D2 | Is it acceptable that the owner's **IP address is publicly visible** as the channel's provider on the DHT? | Yes, with the warning in §6.4 |
| D3 | IPNS republishing: A (key stays in the PWA) or B (Kubo holds the key and republishes by itself)? | A by default, B as an opt-in |
| D4 | Should the channel show a link to the owner's chat `PeerId` (`owner_peer` in the manifest)? | No. It links the public channel to the private chat identity |
| D5 | Maximum post size | 4 KiB of text, the same as chat |
| D6 | Are the three default gateways (§7) acceptable? | Yes |
