# Plan: Permanent public channels, with the owner hidden behind Tor and IPFS-format content

| Field | Value |
|---|---|
| Status | **Plan v3 (WASM-only).** The principle is in SPEC §27. It **depends on Tor mode (SPEC §28) passing gates G2–G4**, and it is not offered before then |
| Relates to | [`SPEC.md`](../spec/SPEC.md) v0.6 §27, §28; [`EMBEDDED-TOR-WASM.md`](EMBEDDED-TOR-WASM.md); [`../spikes/RESULTS-2026-09-28.md`](../spikes/RESULTS-2026-09-28.md) |
| Constraint | Free, with no registration. **The owner's IP address and chat identity are never revealed by the system.** **Only our WASM app is built** (SPEC P10) |
| Date | 2026-09-28 |
| Replaces | v2 (hosted by a native companion, which was removed) and v1 (the owner's Kubo announced the channel on the public DHT, which revealed the owner's IP) |

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
| D7 | **No native programs.** Hosting happens in the owner's browser tab |

## 2. Design

- **The data is IPFS-native:** CIDs, dag-cbor blocks, CAR files and IPNS V2 signed records, all verified in Rust.
- **The owner's browser tab serves it as a Tor onion service,** using the embedded Tor client (SPEC §28).
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

## 3. Identity separation (D4)

- Channel signing key = `HKDF(seed, "p2pchat/channel/" ‖ u32 index)` (Ed25519). The IPNS name is that key.
- Channel onion key = `HKDF(seed, "p2pchat/channel-onion/" ‖ u32 index)`. It is a **different onion address** from the owner's chat onion.
- HKDF is one-way. Nothing in a channel can be linked to the owner's `PeerId`, chat onion, nickname or contacts. The manifest has no owner field: only a `title` and an `about` text the owner writes.
- **Caveats shown when the channel is created:** writing style and posting times can identify you. Anyone who obtains the key file can link the channel to your chat identity, so the UI recommends a **separate identity** just for the channel (SPEC §7.2).

## 4. Availability (the honest limit)

| Host | When the channel is online |
|---|---|
| The owner's tab | While it is open on the desktop. Leaving a tab open is possible; whether a hidden tab keeps working is spike E8 |
| Followers' mirror tabs (§7.1) | While any mirror tab is open |
| A follower's own Kubo (§7.2, third-party and optional) | While their node runs. **Their** IP is public, never the owner's |

**No always-on host exists without native software (P10).** A channel whose owner and mirrors are all offline cannot be read, except from a follower's Kubo mirror or from gateway caches.

## 5. Data and publishing

### 5.1 Blocks (dag-cbor)

- **Root**: manifest, head page, post count, last update time.
- **Manifest**: title, about, `channel_pk`, created, an optional signed mirror list, and a signature.
- **Page**: up to 64 posts, and a link to the previous page.
- **Post**: `seq`, timestamp, body (≤ 4 KiB), `reply_to`, `deleted` flag, and a signature.

Deleting a post rewrites it with `deleted = true` and an empty body. Older copies may survive on mirrors, and the UI says so before the first post.

### 5.2 Posting (the owner's desktop tab, Tor session)

1. Sign the post, then rebuild the head page and the root.
2. Write the changed blocks and the new IPNS V2 record (`sequence = count`, `validity = now + 30 days`, `ttl = 60 s`) to OPFS.
3. Serve them on the onion address at once.
4. Optionally publish the record through a **Tor exit stream** to `https://delegated-ipfs.dev/routing/v1/ipns/<name>` (`PUT`, CORS `*`, spike C-P4). This makes the name resolvable on the public IPFS network **without revealing the owner**. It only matters if some IPFS mirror holds the content.

### 5.3 Republishing (D3)

- The onion serves the latest record directly, so the owner never has to republish.
- The record is valid for 30 days, so mirrors and Kubo followers can republish the owner's signed record (`ipfs name put`, spike C-P4) without the owner's key.
- The owner's app re-signs on every post, and whenever it opens and the record is older than 7 days.

## 6. Reading

### 6.1 Channel link

```
https://<owner>.github.io/p2p-chat/channel.html#c=<ipns-name>&o=<channel-onion>[&m=<mirror-onion>…]
```

- The IPNS name is the channel's identity. The onion addresses are **hints**, because everything is verified against the IPNS key.
- The manifest can carry a **signed mirror list**.

### 6.2 Readers by platform

| Reader | Path | Reader's IP |
|---|---|---|
| Desktop or iOS, Tor session | Embedded Tor → owner or mirror onion | Hidden |
| Any browser without Tor | Public IPFS gateways, **only if** a follower mirrors the channel to IPFS with their own Kubo | Visible to the gateway (never to the owner) |

### 6.3 Default gateways (D6)

- `trustless-gateway.link`, `ipfs.io` and `dweb.link`, used in order with a 4 s timeout; the reader keeps the highest valid record.
- The gateway software defaults to CORS `*` and supports trustless CAR and IPNS-record responses (spike C-P1, from source).

## 7. Followers and mirrors

### 7.1 Onion mirror in the browser (keeps the follower hidden)

- **Mirror this channel** copies and verifies the channel into the follower's OPFS (`mirrors/<name>/`).
- While the follower's Tor-session tab is open, it serves the channel on the follower's own mirror onion. Every 10 minutes it checks the owner's onion for a newer record, and updates only when the new record verifies with a **higher** sequence.

### 7.2 IPFS mirror (optional; the follower's own third-party software)

- A follower who runs **Kubo themselves** (not built or required by us, P10) can press **Also mirror to public IPFS**.
- The app shows the two Kubo commands to run:
  - `ipfs dag import channel.car` (the app downloads the CAR);
  - `ipfs name put <record>`.
- The app itself never talks to Kubo.
- The UI states: "Your IP will be visible as a host of this channel."

## 8. Security

| Threat | Mitigation |
|---|---|
| Finding the owner's IP | The owner is reachable only as an onion service, and any clearnet publishing goes through a Tor exit |
| Linking the channel to the chat identity | Separate HKDF keys, a separate onion, no owner field, and a dedicated identity recommended |
| Forged or changed posts | CID checks, the post signature, and the signed IPNS record |
| An old version served | IPNS sequence high-water marks, and several sources tried |
| Someone else posts | Impossible: only the channel key can sign |
| Losing the channel data | OPFS with `persist()`, the optional real-folder mirror, and CAR export |

## 9. Phases (after TOR-1 passes G2–G4)

| Phase | Scope |
|---|---|
| CH-1 | `channel` crate: keys, IPNS V2, dag-cbor, CAR, verification |
| CH-2 | Onion hosting of the read-only gateway subset from a tab; OPFS store; CAR export and import |
| CH-3 | Owner UI (desktop): create a channel, post, delete, the warnings; optional IPNS PUT via a Tor exit |
| CH-4 | Reader UI: Tor session, and public gateways for IPFS-mirrored channels |
| CH-5 | Mirrors: browser onion mirrors, the signed mirror list, Kubo mirror instructions |

## 10. Spikes

| ID | Question | Status |
|---|---|---|
| C-P1 | Gateways serve CAR and IPNS records with CORS | 🔬 Confirmed from source (boxo defaults to CORS `*`); ⏳ live |
| C-P2 | A tab hosting 2 onion services at once (the chat onion and one channel onion) | ⏳ (after TOR-1) |
| C-P3 | Time to load a channel with 1 000 posts over an onion | ⏳ |
| C-P4 | Republishing a signed IPNS record without the key | ✅ Confirmed from source: Kubo `name put`; `delegated-ipfs.dev` `PUT` with CORS |
| C-P5 | OPFS quota and eviction with `persist()` on each browser | ⏳ |
| E8 | A hidden desktop tab with an open DataChannel keeps its timers running | ⏳ |
