# Plan: Permanent public channels, with the owner hidden behind Tor and IPFS-format content

| Field | Value |
|---|---|
| Status | **Plan v2, owner decisions applied.** The principle is in SPEC §27. The details here become normative once spikes C-P1…C-P5 pass |
| Relates to | [`SPEC.md`](../spec/SPEC.md) v0.5 §27, §28; [`EMBEDDED-TOR-WASM.md`](EMBEDDED-TOR-WASM.md) |
| Constraint | Free, with no registration. **The owner's IP address and chat identity are never revealed by the system** |
| Date | 2026-09-28 |
| Replaces | Plan v1 (the owner's Kubo announced the channel on the public IPFS DHT, which revealed the owner's IP) |

---

## 1. Goal and owner decisions

| # | Decision |
|---|---|
| D1 | Publishing is **desktop only**, and that is acceptable |
| D2 | The **owner's IP is always hidden**, so channels are published **only over Tor** |
| D3 | Republishing: whatever works best (§5.3) |
| D4 | The channel is **not linked** to the owner's chat identity. The owner is known only if they choose to say so in a post |
| D5 | Posts are limited to **4 KiB** of text |
| D6 | Gateways: whatever works best (§6.3) |

## 2. Why v1 could not hide the owner

An IPFS node (Kubo) announces its own IP address in the public DHT as a *provider* of the content it hosts. Kubo cannot run its libp2p networking over Tor in a supported way. Anyone who looks up the channel's content would find the owner's address. **So the owner must never take part in the public IPFS network.**

## 3. Design v2: IPFS format, Tor transport

- **The data stays IPFS-native.** It uses CIDs, dag-cbor blocks, CAR files and IPNS V2 signed records, so any IPFS tool can verify it, and anyone can mirror it to IPFS.
- **The owner serves it only as a Tor onion service.**

```
 OWNER (desktop, hidden)                                  READERS
 ┌───────────────────────────────┐                        ┌────────────────────────────────┐
 │ PWA channel.html              │                        │ PWA channel.html#c=<ipns-name>  │
 │  Rust: sign posts, build CAR, │                        │  Rust: verify record, CIDs and  │
 │  sign the IPNS V2 record      │                        │  post signatures                │
 └───────────┬───────────────────┘                        └───────┬──────────────┬──────────┘
             │ loopback, token-scoped                              │ Tor           │ HTTPS (optional)
             v                                                    v               v
 ┌───────────────────────────────┐   onion service    ┌──────────────────┐  ┌──────────────────────┐
 │ p2pchat-companion             │ ◀────────────────▶ │ reader's Tor:    │  │ public IPFS gateways │
 │  channel store (a folder on   │   (Tor network)    │ companion, or    │  │ (only if a follower  │
 │  disk = the pinning folder)   │                    │ embedded Tor     │  │ mirrored to IPFS)    │
 │  trustless-gateway API        │                    └──────────────────┘  └──────────────────────┘
 │  served on <channel>.onion    │
 └───────────────────────────────┘
```

1. **Owner.** The companion (SPEC §28.3) keeps the channel as a **folder on the owner's disk**, `<data>/channels/<name>/`: blocks and the latest signed record. This is the pinning folder. The companion serves the folder on a **dedicated onion address** as a small read-only subset of the **IPFS trustless-gateway API**:
   - `GET /ipfs/<cid>?format=car` (and `format=raw`);
   - `GET /ipns/<name>?format=ipns-record`.
2. **Readers** use exactly the same verifying client as for a public gateway (§6). The difference is that the "gateway" is `http://<channel-onion>.onion`, reached through Tor.
3. **Mirrors** (followers) keep a copy and serve it too (§7). A mirror can be another onion (the follower's IP stays hidden) or, if the follower chooses to, the public IPFS network (the follower's IP is exposed, never the owner's).

## 4. Identity separation (D4)

- Channel signing key = `HKDF(seed, "p2pchat/channel/" ‖ u32 index)` (Ed25519). The **IPNS name** is that key.
- Channel onion key = `HKDF(seed, "p2pchat/channel-onion/" ‖ u32 index)`. It is a **different onion address** from the owner's chat onion (SPEC §28.6).
- HKDF is one-way. Nothing in a channel (keys, onion address, manifest) can be linked to the owner's `PeerId`, chat onion, nickname or contacts.
- The `owner_peer` field is **removed** from the manifest. The manifest has a `title` and an `about` text that the owner writes; if the owner wants to be known, they say so there.
- **Operational caveats**, which the UI states when the channel is created:
  - writing style and posting times can identify you;
  - the channel is owned by the same key file as your chat identity, so anyone who obtains that file can link the two. The owner MAY use a **separate identity** just for the channel, as several identities are supported (SPEC §7.2).

## 5. Data and publishing

### 5.1 Blocks (dag-cbor)

These are the same as v1, without `owner_peer`:

- **Root**: manifest, head page, post count, last update time.
- **Manifest**: title, about, `channel_pk`, created, and a signature.
- **Page**: up to 64 posts, and a link to the previous page.
- **Post**: `seq`, timestamp, body (≤ 4 KiB), `reply_to`, `deleted` flag, and a signature.

Deleting a post rewrites it with `deleted = true` and an empty body. Older copies may survive on mirrors, and the UI says so before the first post.

### 5.2 Posting (owner, desktop)

1. The PWA signs the post and rebuilds the head page and the root.
2. It builds a CAR file and signs a new IPNS V2 record: `sequence = count`, `validity = now + 30 days`, `ttl = 60 s`.
3. It sends both over loopback to the companion (`PUT /channels/<name>`, token-scoped). The companion stores them in the channel folder and serves them at once on the onion address.
4. Nothing touches the public IPFS network, so the owner's IP is never exposed.

### 5.3 Republishing (D3: the chosen design)

- **There is no DHT record to expire on the owner's side.** The onion serves the latest record directly, so the owner never has to republish.
- The IPNS record is valid for **30 days**, and the PWA re-signs it on every post and whenever the owner opens the app and the record is older than 7 days.
- This long validity lets **any mirror republish the owner's signed record** to the IPFS DHT (§7.2) without the owner's key. Anyone may republish a valid signed IPNS record.
- The private key never leaves the owner's PWA. The companion only ever holds signed data.

### 5.4 Availability

- The channel is reachable while the **owner's companion is running**. It runs in the background on desktop and can start at login.
- It is also reachable from **any mirror** that is online.
- The channel link lists the owner's onion plus known mirrors (§6.1), and readers try them in order.

## 6. Reading

### 6.1 Channel link

```
https://<owner>.github.io/p2p-chat/channel.html#c=<ipns-name>&o=<channel-onion>[&m=<mirror-onion>…]
```

- The IPNS name is the channel's identity. The onion addresses are only **hints**, because every byte is verified against the IPNS key.
- The owner can publish a **signed mirror list** in the manifest, so readers learn new mirrors automatically.

### 6.2 Readers by platform

| Reader | Reaches the channel through | Reader's IP |
|---|---|---|
| Desktop with the companion | Tor (companion) → onion | Hidden |
| Desktop or iOS with **embedded Tor** (research track, EMBEDDED-TOR-WASM.md) | Tor (in WASM, Snowflake) → onion | Hidden |
| iOS or desktop **without Tor** | Public IPFS gateways, **only if** some follower mirrored the channel to IPFS | Visible to the gateway (not to the owner) |

**Consequence:** until embedded Tor passes its gates, **an iPhone can read a channel only when a follower has mirrored it to public IPFS.** The owner's IP is hidden in every case.

### 6.3 Default gateways (D6: the chosen design)

- For readers without Tor, and only for channels that are mirrored to IPFS: `trustless-gateway.link`, `ipfs.io` and `dweb.link`. All are free, need no account, and support trustless CAR and IPNS-record responses (spike C-P1).
- They are used in order with a 4 s timeout, and the reader keeps the valid record with the highest sequence.
- They are untrusted, because everything is verified in Rust.

## 7. Followers and mirrors

### 7.1 Onion mirror (recommended: keeps the follower hidden)

- A follower with the companion presses **Mirror this channel**.
- Their companion fetches and verifies the channel over Tor, stores it in its own folder `<data>/mirrors/<name>/`, and serves it on the follower's own mirror onion address.
- It checks the owner's onion for a newer record every 10 minutes while running, and updates only when the new record verifies with a **higher** sequence.

### 7.2 IPFS mirror (optional: exposes the follower, never the owner)

- A follower who runs Kubo and accepts that their IP will be visible can press **Also mirror to public IPFS**.
- Their PWA imports the CAR into their Kubo (`dag/import`, pinned in MFS at `/p2p-chat/mirrors/<name>/`) and republishes the owner's signed IPNS record (`routing/put`).
- This makes the channel readable through public gateways, for example on an iPhone without Tor.
- The UI states: "Your IP will be visible as a host of this channel."

## 8. Security

| Threat | Mitigation |
|---|---|
| Finding the owner's IP | The owner never takes part in IPFS or any direct connection. The only way in is the Tor onion service |
| Linking the channel to the owner's chat identity | Separate keys (HKDF), a separate onion, no `owner_peer` field, and the option of a dedicated identity |
| Forged or changed posts | CID checks, the post signature, and the signed IPNS record |
| A mirror or gateway serves an old version | IPNS sequence high-water marks, and trying the owner's onion and several mirrors |
| Someone else posts | They cannot: only the channel key can sign |
| A compromised page talks to the companion | Token-scoped loopback API; the channel endpoints only accept signed data |
| Traffic correlation against a global observer | Tor's usual limits apply (SPEC §28.8) |

## 9. Phases

| Phase | Scope |
|---|---|
| C1 | `channel` crate: keys, IPNS V2, dag-cbor, CAR, verification (the same as v1) |
| C2 | Companion: channel folder store, read-only trustless-gateway API on the channel onion, loopback publishing endpoint |
| C3 | Owner UI (desktop): create a channel, post, delete, the anonymity warnings |
| C4 | Reader UI: through the companion (desktop), through public gateways (mirrored channels), and through embedded Tor once it is ready |
| C5 | Followers: onion mirrors, the optional IPFS mirror, the signed mirror list |

## 10. Spikes

| ID | Question |
|---|---|
| C-P1 | Do the public gateways serve CAR and IPNS-record responses with CORS to `github.io`, including on iOS? |
| C-P2 | Embedded arti in the companion hosting 2 onion services (the chat onion and one channel onion) at once |
| C-P3 | Time to fetch a channel with 1 000 posts over an onion: head page and the older pages |
| C-P4 | Does Kubo's `routing/put` accept an IPNS record signed elsewhere? (For IPFS mirrors) |
| C-P5 | Does the companion use little enough resources to run at login and all day? |
