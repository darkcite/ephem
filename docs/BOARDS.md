<!-- SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0 -->
<!-- Copyright 2026 Anton (darkcite) -->
# Appendix G: Boards (a 4chan-like channel type), proposed, not built

**Status: proposal (2026-09-29), nothing here is built or tested.** It extends Appendix D of `docs/P2P-CHAT.md` and follows every principle there, P10 most of all. Unknowns are spikes (G.16). The existing channel type (§27, Appendix D: one owner posts, followers read) **stays exactly as it is**: its blocks, record, gateway and UI are not changed by this proposal.

## G.1 Goal and constraints

| # | Constraint | Consequence for boards |
|---|---|---|
| G1 | Browser tab only, no servers of ours, no companion (P1, P10) | The board is hosted by the **owner's tab** as an onion service, exactly like a channel (D.2). Nothing is always on |
| G2 | The owner is hidden (D2) | Posts reach the host only **over Tor**; the host never joins the public IPFS network |
| G3 | Anyone with the link may post, with no account | Anti-spam cannot use IPs (an onion service sees none) or accounts: it uses **proof of work**, caps and moderation (G.8, G.9) |
| G4 | Content is IPFS-native and verified in Rust (D.2) | Reuse `crates/channel`'s `cbor`, `cid`, `car`, `ipns`, `time`; a new crate `crates/board` holds the model. `channel::channel` is untouched |
| G5 | HFT taste (§22) | Fixed pools and budgets set when the board is served; every input refused **before** anything grows beyond a fixed slot; single writer (the host tab) |
| G6 | P7 | A board is a publication, not a chat: permanent and public, like a channel. No private-chat data flows into it |

**What a board is:** boards → threads → replies. Anyone holding the board link and running the app can start a thread or reply; the owner (and janitors) moderate; the owner's tab orders, signs and serves the result.

## G.2 What "4chan-like" means here

| Feature | Decision | Notes |
|---|---|---|
| Boards → threads → replies | **Yes** | One board per onion; a thread is an OP plus replies |
| Anonymous posting, no accounts | **Yes** | Each post is signed by an **ephemeral poster key** (G.4); "Anonymous" is the default name |
| Post numbers | **Yes**, board-wide, monotonic `no` (u64), assigned by the host | Never reused, also across devices (G.13) |
| Quote links `>>123`, backlinks | **Yes**, computed by readers from the body | No host-side parsing; `>>>` cross-board links: **no** in v1 (one board per link) |
| Greentext (`>` at line start) | **Yes**, render-only | No other markup; bodies are plain text, escaped everywhere |
| Poster IDs per thread | **Owner flag** `ids`, **off by default** (B5) | On: one random key per thread per tab, shown as an 8-character ID. Off: a fresh key per post (G.4) |
| Tripcodes | **Yes, as signed keys** ("trip keys") | Unforgeable, unlike 4chan's hashed passwords (G.4) |
| Capcodes (owner, janitor) | **Yes** | The post is signed by the board key or a listed janitor key |
| Bump order, bump limit, sage | **Yes** | A reply bumps its thread unless `sage` or the thread is past the bump limit |
| Pages, pruning, archive | **Yes** | Threads beyond the page count, or beyond the byte budget, are pruned oldest-bump first; a text-only archive (G.5.3) |
| Sticky, locked | **Yes** | Owner/janitor actions |
| Catalog view | **Yes** | One block with every live thread (G.5.1) |
| Images | **Yes, off by default**, owner opt-in with a warning | JPEG only, re-encoded in the poster's tab, thumbnails made by the host (G.7) |
| Video, animated GIF, audio, files | **No** | Parsing and moderation risk; size over Tor |
| Captcha | **No** | Solvable by paid services, needs JS image generation, and hurts Tor users most. PoW instead (G.8) |
| Flags, country, IP-based anything | **No** | There is no IP |
| Post editing | **No** | As on 4chan: delete and repost |

## G.3 Roles and topology

```
 POSTER (any tab with the app, Tor)       OWNER (desktop tab, hidden)                 READERS
 ┌───────────────────────────┐            ┌───────────────────────────────────┐      ┌────────────────────┐
 │ compose, re-encode image  │ GET /pow   │ board onion (embedded Tor)        │      │ app (Following)    │
 │ sign with poster key      │──────────▶ │  /pow  → seed, efforts            │◀─────│ Tor Browser (no JS)│
 │ solve Equi-X PoW          │ POST       │  /submit → validate → accept      │ GET  │ IPFS gateway, only │
 │                           │──/submit──▶│  rebuild blocks, sign IPNS record │      │ if a Kubo mirror   │
 └───────────────────────────┘ ◀─ 200 no ─│  serve /ipfs, /ipns, / (HTML)     │      └────────────────────┘
                                          │  OPFS: boards/<name>/             │──▶ mirrors (D.7), read-only
 JANITOR (tab, janitor key) ── signed mod action ─▶ /submit                   │
                                          └───────────────────────────────────┘
```

- **Only the host writes.** It is the single writer: it assigns numbers and time, applies moderation, and signs the snapshot (the IPNS record). Everyone else submits.
- **Mirrors are read-only** (G.10). Posting needs the owner's tab.

## G.4 Keys and identities

| Key | Derivation | Held | Purpose |
|---|---|---|---|
| Board signing key | `HKDF(seed, "p2pchat/board/" ‖ u32 index)` → Ed25519; the IPNS name | Owner | Signs the record and capcode posts |
| Board onion key | `HKDF(seed, "p2pchat/board-onion/" ‖ u32 index)` | Owner | A dedicated onion, unlinked from the chat onion and from channels (as D.3) |
| PoW secret | `HKDF(seed, "p2pchat/board-pow/" ‖ u32 index)` | Owner (every device of the identity) | Epoch seeds (G.8); derived so a second device accepts the same seeds (G.13) |
| Poster key, IDs on | Random Ed25519 per (board, thread) per tab, RAM only | Poster | Signs each post; the thread ID is `base32(BLAKE2b-40("ephem-board-id" ‖ board ‖ thread ‖ pk))` |
| Poster key, IDs off | Random Ed25519 **per post**, RAM only | Poster | Same, but no two posts share a key |
| Trip key (optional) | `HKDF(seed, "p2pchat/board-trip/" ‖ board name ‖ label)` | Poster, signed in | A stable, unforgeable name on **one** board: shown `!` + 10 base32 chars of its hash. Different boards give unrelated keys |
| Janitor key | `HKDF(seed, "p2pchat/board-janitor/" ‖ board name)` | Janitor | Listed in the manifest with rights (G.9) |

- Board indices 0–3: **at most 4 boards per identity** (each costs far more than a channel: RAM, CPU, OPFS). The index space is separate from channels' 0–15.
- The poster's chat identity is never used, except to derive a trip key the poster chose. A temporary identity can post but cannot have a trip.
- Poster keys die with the tab. Self-delete (G.9) works only while the tab that posted is open.
- **What a poster signature proves, honestly:** a mirror cannot forge or alter a post (and neither can a gateway). The host **can** invent posts under fresh keys, indistinguishable from real anonymous posts; it cannot post as an existing trip key or as an existing thread ID. A board is exactly as honest as its owner.

## G.5 Data model

### G.5.1 Blocks (strict dag-cbor, canonical, as D.5.1)

Two kinds of reference, on purpose: **pin links** (CBOR tag 42, followed by `dag-scope=all`) and **view refs** (CID bytes, not followed). This keeps the whole board pinnable from the root, while a thread or catalog CAR stays small.

```
IPNS record ──▶ root {v:1, kind:"board", manifest⁴², catalog⁴², threads⁴², media⁴², log⁴², archive⁴², next_no, rev, updated}
  manifest  {v, kind:"board", title, about, rules, pk, created, mirrors[≤8], janitors[≤8], flags, limits, sig}
  catalog   {t: [{no, thread (ref), bump, r, i, sub, ex, thumb⁴²|null, st, lk}] ≤ MAX_THREADS, bump order}
  threads   {t: [thread⁴² …]}                      (pin index only)
  thread    {no, sub, chunks: [chunk⁴² …] ≤ 8, r, i, st, lk}
  chunk     {p: [post …] ≤ 64, oldest first}
  post      {no, ts, s, sig, thumb⁴²|null, img (ref)|null, w, h, cap}   or tombstone {no, ts, del, by}
  s (signed by the poster) {b: board name, t: thread no (0 = new), k: pk, sub, body, img: CID bytes|null, sage, e: epoch}
  media     {m: [full image⁴² …]}                  (pin index for full images, raw blocks)
  log       {a: [{ts, act, no, by, why}] ≤ 1 024, newest last}
  archive   {t: [{no, sub, ex, pruned, thread (ref, text only)}] ≤ 256}
```

- `s` is signed as `"ephem-board-post:" ‖ dag-cbor(s)` by `s.k`. The host adds `no`, `ts`, the thumbnail, dimensions and `cap` (0 anon, 1 owner, 2 janitor n). The host's own signature is the IPNS record over the root, which fixes order, numbers, times and moderation for every block by CID.
- `ex` is the first 140 characters of the OP body; `r`/`i` are reply and image counts; `st`/`lk` sticky and locked.
- A reader detects the type from the root: a channel root has no `kind`; a board root has `kind: "board"`. Channel code refuses a board root as `Invalid`, which is correct.

### G.5.2 Limits (defaults; the owner may tighten, never loosen past the maximum)

| Item | Default | Maximum | Why |
|---|---|---|---|
| Body | 2 000 B UTF-8 | 2 000 B | 4chan's size; keeps a chunk ≤ ~170 KiB |
| Subject | 100 B | 100 B | |
| Threads per board | 100 (10 pages × 10) | 150 | Catalog block ≤ 128 KiB |
| Replies per thread | bump limit 300, hard cap 500 (then locked) | 500 | 8 chunks of 64 |
| Images per thread | 150 | 150 | |
| Full image | ≤ 512 KiB JPEG, ≤ 2 048 px long side | same | Under the 1 MiB IPFS block limit; ~5–20 s over Snowflake (B-P5) |
| Thumbnail | ≤ 16 KiB, ≤ 250 px, made by the host | same | |
| Board text in RAM | 64 MiB | 128 MiB | The host keeps text blocks in RAM, images on OPFS |
| Board store (OPFS) | 512 MiB | 2 GiB, and the browser quota (C-P5) | **The byte budget prunes before the thread count on a busy board** |
| Submit request | ≤ 520 KiB | same | Header + text + one image |
| Concurrent submit streams | 8 | 8 | Fixed slot pool (G.6.2) |
| Accepted posts, board-wide | 120 / min | 600 / min | Above it: 503 and higher effort |
| Janitors / bans / filters / reports | 8 / 1 024 keys / 64 × 64 B / 256 | same | Rings: oldest dropped |
| Archive | 256 threads, 7 days, text only | same | |

### G.5.3 Chunking, deletion and CID stability

- A thread's posts fill chunks of 64 in order. **A full chunk never changes** unless a post in it is deleted, so its CID is stable and readers cache it by CID; a refresh fetches the thread block and the last chunk only.
- A new reply rewrites: the last chunk, the thread block, the catalog, the `threads` index and the root. A new image adds itself to `media`. That is ≤ 6 small blocks per post plus the image.
- **Deletion** replaces the post with a tombstone `{no, ts, del, by}` (`del`: 1 owner, 2 janitor, 3 poster, 4 filter; `by`: janitor index). The poster's signed `s`, the thumbnail and the image leave the DAG, and the host deletes their blocks from its store. The chunk's CID changes, and so do the blocks above it. Numbers are never reused, so `>>123` to a deleted post shows "(deleted)".
- **Pruning** removes a thread from the catalog and the indexes and deletes its blocks; with the archive on, a text-only copy of the thread (images and thumbnails dropped) is linked from `archive` for 7 days.
- Publishing is **coalesced**: accepted posts wait in a fixed ring of 64 and are built and signed together at most once a second (and at once when the ring is full). IPNS `sequence` grows per publish; `validity = now + 30 days`, `ttl = 60 s` as D.5.2. The optional `PUT` to `delegated-ipfs.dev` through a Tor exit runs at most every 10 minutes.
- **Store:** the channel store rewrites one CAR per change (D.2, as built); at board sizes that is untenable. A board keeps **one OPFS file per block** (`boards/<name>/b/<cid>`) plus `record.bin`, and deletes files that the new root no longer reaches. "Export backup" still writes one CAR (`dag-scope=all` from the root).

## G.6 Submitting a post

### G.6.1 Protocol: HTTP/1.1 over the board onion

**Decision: `POST /submit` to the onion's gateway**, the same HTTP/1.1 subset the channel gateway already speaks (`crates/channel::gateway`), extended in `crates/board::gateway`.

**Rejected: a Noise-authenticated stream.** The onion already authenticates the host (by its address) and encrypts end to end; the poster has no stable key to authenticate with anyway. Noise would add a handshake round trip over Tor for nothing.

1. `GET /pow` → `{epoch u32, seed [32], effort_thread u32, effort_reply u32, effort_image u32, effort_report u32, paused bool}` (a fixed 60-byte body, not CBOR).
2. The poster builds `s`, signs it, solves the PoW (G.8) in wasm in **Web Workers** (one per core, up to 4, each on its own nonces: B-P1 found one Equi-X attempt takes 64–131 ms and cannot be interrupted, so slicing on the page is impossible), with a progress bar and Cancel. The workers load the same pinned wasm (SRI) and receive only the challenge; nothing else leaves the page.
3. `POST /submit` with `Content-Type: application/vnd.ephem.board-submit` and `Content-Length`. Answer: `200 {no, rev}` once the post is in a published snapshot (≤ ~1 s), or an error status plus a stable `u16` code (G.6.3).

**Submit body (little-endian, parsed in place):**

| Off | Size | Field | Notes |
|---|---|---|---|
| 0 | 4 | `magic` | `EPB1` |
| 4 | 1 | `kind` | 1 thread, 2 reply, 3 self-delete, 4 report, 5 janitor action, 6 capcode post |
| 5 | 1 | `flags` | bit0 `sage`, bit1 `image`; others MUST be 0 |
| 6 | 2 | `text_len` | dag-cbor `s` (or the action map), ≤ 2 400 |
| 8 | 4 | `img_len` | 0 or ≤ 524 288 |
| 12 | 4 | `epoch` | Must be the current or previous epoch |
| 16 | 4 | `effort` | ≥ the effort required for `kind`/`flags` |
| 20 | 16 | `nonce` | PoW nonce |
| 36 | 16 | `solution` | Equi-X solution |
| 52 | 64 | `sig` | Ed25519 by `s.k` (or the janitor/board key) over `"ephem-board-post:" ‖ s` |
| 116 | var | `s`, then image bytes | Nothing after them |

### G.6.2 Host pipeline (refuse early, allocate nothing per request)

At serve time the host preallocates **8 submit slots of 520 KiB + 8 KiB head** (≈ 4.2 MiB), 32 read-stream slots, the replay set and the publish ring. Then, per stream:

| Step | Check | On failure |
|---|---|---|
| 0 | A free slot exists (else the stream is closed at once) | `503`, close |
| 1 | Head ≤ 8 KiB, `POST /submit`, `Content-Length` ≤ 520 KiB, 30 s total deadline | `431`/`413`/`408`, close |
| 2 | Fixed 116-byte header: magic, kind, flags, lengths add up to `Content-Length`, epoch current/previous, effort ≥ required, board not paused | `400`/`409`, close **before reading the body** |
| 3 | Read the body into the slot (one copy, G.14.2) | close |
| 4 | Replay set: `BLAKE2b(challenge ‖ solution)` not seen in the last 2 epochs | `409` |
| 5 | Equi-X verify, and `BLAKE2b-32(challenge ‖ solution) × effort ≤ 2³² − 1` | `403`, `E_BOARD_POW` |
| 6 | Strict dag-cbor decode of `s` (limits, `b` = this board, `t` a live unlocked thread or 0, `e` = header epoch, `img` = CID of the image bytes) | `400` |
| 7 | Ed25519 verify; key not banned; word filters (fixed Aho–Corasick automaton, linear time) | `403` |
| 8 | Image: JPEG segment whitelist and dimensions in Rust, then the browser's decoder, then the host's thumbnail (G.7) | `415` |
| 9 | Duplicate body in this thread's last 64 posts; rate caps (G.8) | `409`/`429` |
| 10 | Into the publish ring; answer after the next signed snapshot | `503` if the ring is full |

The PoW is checked before the signature because Equi-X verification is cheaper than an Ed25519 check in wasm (spike B-P1 measures both), and both before any decode.

**Change needed in the gateway loop:** today's channel `serve_loop` spawns one task per incoming stream with no cap and grows a `Vec` for the head. Boards need a **bounded** loop: a fixed stream pool, fixed head buffers, per-stream deadlines, and keep-alive of up to 32 sequential GETs per stream (each Tor stream costs a round trip to open). The channel gateway MAY adopt it later; that is not part of this proposal.

### G.6.3 Error codes (additions to §19)

`0x0070 E_BOARD_POW` (bad or insufficient work, or a stale epoch) · `0x0071 E_BOARD_BUSY` (no slot, ring full, or the posts-per-minute cap) · `0x0072 E_BOARD_REFUSED` (banned key, filter, locked or pruned thread, duplicate) · `0x0073 E_BOARD_PAUSED` (posting paused by the owner) · `0x0074 E_BOARD_IMAGE` (not a clean JPEG within limits) · `0x0075 E_BOARD_OFFLINE` (the host onion cannot be reached; reading may still work from mirrors).

## G.7 Images

| Question | Decision | Rejected, and why |
|---|---|---|
| Allowed at all | Owner opt-in per board, **off by default**, behind the warning of G.9.3 | On by default: the legal risk lands on the owner |
| Input formats | Anything the poster's browser decodes (`createImageBitmap`) | Parsing formats in Rust: attack surface we do not need |
| Output format | **Baseline or progressive JPEG only**, quality 0.82, long side ≤ 2 048 px | WebP: Safari's canvas cannot encode it. PNG: too large. Animated GIF: dropped (first frame only, with a notice) |
| Metadata stripping | Decode in the browser → RGBA pixels → **encode in Rust** (a small pure-Rust JPEG encoder) → only SOI, APP0 (JFIF), DQT, SOF0/SOF2, DHT, DRI, SOS, EOI | Canvas `toBlob`: strips EXIF too, but its bytes reveal the browser engine (libjpeg-turbo vs Apple's encoder). Our encoder gives the same bytes on every engine for the same pixels |
| Anti-fingerprinting noise | Firefox RFP, Brave and Safari's advanced protection may add noise to pixel readback: accepted (the image changes slightly) | Refusing such browsers |
| Host verification | Rust checks the segment whitelist and SOF dimensions (a bounded, fuzzed parser); any APPn other than APP0, any COM, trailing bytes → refused. Then the host's own browser decoder must decode it | Trusting the poster's claim of "no metadata" |
| Thumbnails | **Made by the host** from the decoded image (≤ 250 px, same encoder) | Poster-made thumbnails: the host cannot cheaply prove the thumbnail matches the image, and a benign thumbnail over an illegal image is exactly the abuse moderators must not miss |
| Reader side | The same whitelist check before handing bytes to the browser decoder; images shown only through `blob:` URLs of verified bytes; per board, a reader setting "show images", **off** until the reader turns it on | Auto-loading full images |

Decoding happens only in the browser's own sandboxed image decoder, in every role; our wasm never decodes image data, it only parses JPEG segment headers.

## G.8 Anti-spam without IPs

| Layer | Mechanism | Honest limit |
|---|---|---|
| Proof of work | **Equi-X** (the puzzle of Tor's onion-service PoW v1), via arti's pure-Rust `equix` crate. Challenge = `"ephem-board-pow-v1" ‖ board name ‖ seed(epoch) ‖ kind ‖ thread ‖ BLAKE2b(s ‖ image) ‖ nonce ‖ effort`. Valid when `BLAKE2b-32(challenge ‖ solution) × effort ≤ 2³² − 1`: the expected solve count is `effort` | Buys cost, not identity. A botnet or a GPU farm still posts; Equi-X is CPU-oriented, which narrows the GPU advantage. In wasm, hashx runs interpreted (no JIT): speed is spike B-P1 |
| Freshness | `seed(epoch) = BLAKE2b-keyed(pow secret, epoch)`, epoch = 10 min; the current and previous epochs are accepted. The work is bound to the post's content and thread, so it cannot be precomputed ahead of ~20 min or reused | — |
| Efforts | Owner sets a base; defaults: reply ×1, image ×4, new thread ×8, report ×½. Target **~10 s** median for a reply on a recent phone (B6; calibrated after B-P1) | Slow phones pay more |
| Adaptive effort | Doubles when accepted posts pass 50 % of the per-minute cap, or the slot pool is > 50 % busy for 30 s; halves after 10 calm minutes; capped by the owner's maximum. `GET /pow` always gives the current value | Legitimate posters wait longer during a flood |
| Caps | Board-wide posts per minute; 8 submit slots; per-thread duplicate check | — |
| Per circuit | If `tor-hsservice` exposes which rendezvous circuit a stream came on (spike B-P3): ≤ 4 streams and ≤ 1 submit per 10 s per circuit | New circuits are cheap for a client; this only slows naive floods |
| Onion-service PoW (Tor's own, at the introduction point) | Wanted: it protects the **tab** from introduction floods, which app-level PoW cannot (each introduction makes the host build a rendezvous circuit over Snowflake). arti's service-side support in our vendored version is unknown: **spike B-P2** | Until then, a determined flood of introductions can take a board offline. Readers then use mirrors |
| Panic switches | Owner: pause posting (read-only), threads only by owner, pre-moderation (posts held until approved) | — |

## G.9 Moderation

### G.9.1 Actions and who may do them

| Action | Owner | Janitor (if granted) | Poster (own post, same tab) |
|---|---|---|---|
| Delete post / delete image only | ✓ | ✓ | ✓ (self-delete, `kind` 3, low effort) |
| Lock, sticky, prune thread | ✓ | lock only | — |
| Ban poster key (optional public reason "USER WAS BANNED FOR THIS POST") | ✓ | ✓ | — |
| Word filters (reject or replace), efforts, pause, pre-moderation | ✓ | — | — |
| Add/remove janitors, edit rules | ✓ | — | — |
| Approve held posts, read reports | ✓ | ✓ | — |

- **Janitors** send their public janitor key to the owner (a `#j=` link or a paste); the owner lists it in the signed manifest as `{pk, rights (bitmask), until}`. A janitor acts by submitting a `kind` 5 map `{act, no, why}` signed by that key, with no PoW. The host applies it and appends it to the public **mod log**. Janitors act only while the owner's tab is online: the owner signs every state.
- **Bans are weak, and the UI says so.** Poster keys are ephemeral, so a ban stops one key: a new tab or a new thread gets a new key. A ban does bind a trip key for good. The real brakes are PoW effort, pre-moderation and pausing.
- **Filters** stay in the owner's store, not in the manifest, so spammers cannot read them.
- **Reports** (`kind` 4: `{no, reason}` with a small PoW) go to a ring of 256 in the owner's tab, shown in the owner view.

### G.9.2 Illegal content (CSAM and the like)

- **The owner is the publisher.** The owner's tab signs and serves everything it accepts. Readers, mirrors and the law see the board as the owner's publication.
- **No automatic scanning.** Hash lists for known abuse material (PhotoDNA, NCMEC) are not available to a browser app without a server of ours (P1). The host has only its moderators' eyes.
- **Deletion is real only on the host.** Delete-and-republish removes the blocks from the owner's store and from every later snapshot. Browser mirrors replace their copy on their next refresh (≤ 10 min, D.7.1) and keep only blocks reachable from the latest root (a board-mirror requirement). **Kubo mirrors, gateway caches and readers' caches may keep older copies** as long as they choose.
- Defaults that follow: images **off**; with images on, **pre-moderation of image posts** is offered first; readers' "show images" is off by default.

### G.9.3 Warnings shown before creating a board (one checkbox, as channels do)

1. "You publish whatever your board accepts. You are responsible for it where you live."
2. "Anyone with the link can post. Spam and illegal content will arrive; only you and your janitors can remove it, and only while your tab is online."
3. "Deleting removes a post from your board. Copies made by mirrors, IPFS nodes and readers before that may survive."
4. "Images greatly raise the risk. They are off; turn them on only if you will moderate."
5. The channel warnings of D.3 (writing style, posting times; a separate identity is recommended).

## G.10 Availability and mirrors

| Option | Posting while the owner's tab is off | Decision |
|---|---|---|
| (a) Posting pauses; reading continues from mirrors | No. The app says "The board's host is offline: reading from a mirror; posting resumes when the host is back" | **v1** |
| (b) Mirrors queue signed submissions and hand them to the owner later | Queued, not visible until the owner accepts them | **Later (BD-8).** Mirrors cannot check PoW against the owner's secret seeds, so each mirror needs its own seed and effort, and the owner re-checks every queued post: a flood of queued work lands on the owner at once |
| (c) Multi-writer (a merged log or CRDT) | Yes | **Rejected for boards as designed.** Numbers, order, bumping and moderation all need one writer; a merge would let any writer fork the board or undo a deletion |

- Mirrors are D.7 mirrors of a board root: "Mirror this board" copies the latest DAG **without full images** by default (thumbnails only; "include full images" is a mirror option with a size shown), and serves it read-only, including the plain HTML page (G.11.2). Their `/submit` answers `E_BOARD_OFFLINE` with the owner's onion as a hint.
- **The honest limit:** a board is **writable only while its owner's tab is open**, and readable while that tab or any mirror is open (D.4). A board that everyone forgets goes offline.

## G.11 Reading paths

### G.11.1 The app

- Following lists boards beside channels. A reader fetches the record, the catalog CAR (catalog + OP thumbnails), and per thread the thread block plus missing chunks (each chunk CAR carries its thumbnails). Full images are fetched one by one (`format=raw`) on tap. Every block is checked by CID, every post by its poster signature, the root by the record (D.6).
- Refresh: open threads every 30 s while on screen; the catalog on open and every 10 min with the follow list (F.3.3, 4 at a time). **Watched threads** (≤ 32) live in RAM, or in the key file as TLV `0x07 WATCHED` when signed in (0x07 is unused so far). Followed boards join the follow list (TLV `0x06`) with `"k": "board"`.

### G.11.2 Tor Browser (plain page, no JS)

- The onion serves `/` (catalog, 10 pages), `/t/<no>` (a thread), `/th/<cid>.jpg` (thumbnails) and, with images on, `/im/<cid>.jpg`, all built by `crates/board::page`. CSP: `default-src 'none'; img-src 'self'; style-src 'unsafe-inline'; form-action 'none'`, `X-Content-Type-Options: nosniff`, no referrer. Images are served as `image/jpeg` only after the host's whitelist check.
- **Posting from the plain page: no.** The PoW needs JavaScript, which Tor Browser's Safest level disables. A no-JS form would need either no PoW (a spam door into the owner's tab) or a captcha (rejected, G.2). The page says: "Posting needs the Ephem app."

### G.11.3 Public IPFS gateways

Only when some follower runs a Kubo mirror (D.7.2): the root's pin links reach every live block, full images included (`media`), so `ipfs dag import` of the exported CAR pins the whole board. The app reads it read-only through `trustless-gateway.link` ("Read without Tor", D.6.3). Posting always goes over Tor to the owner.

## G.12 UI placement

- **My channels → "+"**: "Create a channel" (unchanged) or "Create a board" (title, about, rules, images on/off, IDs on/off, efforts, the G.9.3 checkbox). The tab keeps its name; boards show a ▦ badge.
- **Following**: boards and channels in one list; a board row shows title, new threads and new replies in watched threads, and where it was read from (owner/mirror/gateway).
- **Board view**: catalog grid (thumbnail or text card, subject, R/I counts, sticky/locked marks; sort by bump, new, replies; local text search), then a thread view (No., ID, trip, capcode, greentext, `>>` hover previews on desktop and tap-to-jump on phones, backlinks, "(deleted)").
- **Reply box**: subject (new thread only), body with a byte counter, image picker showing the re-encoded preview, its size and "metadata removed", sage, an optional "send after a random 10–120 s delay" (G.14.1), then **Post** → "Proof of work… ~N s" (progress, Cancel) → "Sending over Tor" → "Posted as No. N".
- **Owner view** (in My channels): online state and `reach` (F.6), the queue of held posts, reports, mod log, bans, filters, efforts, janitors, pause posting, backup.
- In direct mode the board views load the Tor build lazily, like channel tabs (F.3.3).

## G.13 Several devices (ties to D.11)

- The vault (D.11.2) gains a board entry: `{kind: board, index, title, root, next_no, rev, updated, mirrors}` (≈ 200 B). Boards update the vault at most every 10 min, not on every post.
- **The writer lease (D.11.3 step 5) is mandatory for boards.** Two tabs hosting one board onion would split submissions between two writers and fork the numbers. A device without the lease reads the board as a follower and offers "Take over here".
- The PoW secret is derived from the seed, so after a takeover, posts solved against the old device's seeds still verify. The replay set is not shared; the per-thread duplicate check covers that window.
- **Numbers never collide after a takeover:** the new writer reads the latest board record from any source first; if none answers, it continues from `vault.next_no + cap_per_min × minutes since vault.updated`, which is above any number the old writer could have assigned.
- Continuing without history (D.11.3 step 4) for a board means an empty catalog with numbers continuing; the old threads come back only if a mirror or the other device serves them. A board prunes anyway, so this costs less than for a channel.

## G.14 Security and privacy

### G.14.1 Threats

| Threat | Mitigation | What remains |
|---|---|---|
| Finding a poster's IP | Tor only (the Tor build); no poster data in clear | Tor's limits (§28.8) |
| Linking a poster's posts | Per-thread or per-post random keys; trips only on request; poster keys are never derived from the identity | Writing style; the host sees which posts came on one circuit (keep-alive), and times them |
| Timing correlation (watching the poster's network and the board) | Optional random send delay; the image upload is a large, visible burst on the poster's link | A global observer still wins against Tor |
| Finding the owner | Onion-only host, as D.3/D.8; separate HKDF keys; separate identity recommended | The board is online exactly when the owner is: moderation times and uptime are public |
| Malicious host | Cannot forge posts under existing keys, cannot move a post to another board or thread (`s.b`, `s.t` are signed) | Can drop, delay, reorder, delete, and invent "anonymous" posts. Can show different states to different readers: readers seeing two roots at one `sequence` flag a fork |
| Malicious mirror or gateway | CIDs, the record, poster signatures; high-water marks (D.8) | Can withhold, or serve an older state until the record expires |
| Malicious poster: oversized, malformed, no PoW | G.6.2: header and lengths checked before the body is read; everything lands in a fixed slot; strict decoders; `panic = "abort"` never reachable from input (fuzzed) | — |
| Malicious image (decoder exploits) | Only the browsers' sandboxed decoders see pixels; our Rust parses segment headers only (fuzzed); re-encoding by the poster, whitelist by host and reader | A browser decoder bug is the browser's; readers' "show images" is off by default |
| Host exhaustion | Slot pools, deadlines, byte budget, adaptive effort, pause | Introduction floods until B-P2 |
| Illegal content | G.9.2 | Copies made before deletion |

### G.14.2 Memory copies (as §11.6)

| Path | Step | Copy? | Justification |
|---|---|---|---|
| Submit RX | arti `DataStream` → preallocated submit slot | **1 copy** | Unavoidable: the stream yields into a buffer we own (the same copy as for chats, F.3.2) |
| Submit RX | header, PoW, signature, CBOR, JPEG segments | 0 | Views over the slot |
| Submit RX | image → browser decoder (`Blob` from a view) | **1 copy** (browser) | Unavoidable: a `Blob` owns its bytes |
| Host store | slot → OPFS file | **1 copy** (browser) | `write` from a wasm memory view |
| Host store | text of accepted posts → block bytes | **1 copy** | The block is new bytes (dag-cbor with `no`, `ts`, `thumb`); ≤ 2.4 KiB per post |
| Serve | OPFS image → response slot | **1 copy** | OPFS reads return a new `ArrayBuffer`; wasm cannot address it (as §11.6). Only for `format=raw` of full images |
| Poster TX | canvas RGBA → wasm scratch (≤ 16 MiB, allocated when the image picker opens, freed after) | **1 copy** | Setup path, a documented allocation exception (§22) |
| Poster TX | JPEG out → submit slot → `write` | 0 | Encoded straight into the slot |

## G.15 Decisions (the owner, 2026-09-29)

| # | Question | Decision |
|---|---|---|
| B1 | Images | Supported, **off by default**; the owner turns them on after a warning (JPEG only, re-encoded in the poster's tab, thumbnails by the host) |
| B2 | Posting while the owner is offline | **Pauses**; reading continues from mirrors. Mirror queues (G.10 b) later; several writers rejected |
| B3 | No-JS posting from Tor Browser | **No**: Tor Browser reads the plain page; posting needs Ephem (G.11.2) |
| B4 | Boards per identity, default size | **4 boards**; **10 pages × 15 threads** (150 live threads), bump limit 300, oldest-bumped pruned, text archive kept |
| B5 | Poster IDs | Owner flag, **off by default**: a fresh key per post unless the owner turns IDs on |
| B6 | Proof-of-work cost | Target **~10 s median for a reply on a recent phone** (was ~3 s); threads ×8 as before; adaptive effort on top during floods |
| B7 | Moderation in v1 | **Owner only**: delete, ban a poster or trip key, lock, sticky, pause posting. Janitors, word filters, reports and pre-moderation move to a later phase (BD-5b) |
| B8 | Tripcodes | **Yes, signed trip keys** (G.4) |

## G.17 Optional public discovery (proposed; the owner's request of 2026-09-30)

**Goal.** An owner may make a board, or one thread, findable by people who do not have its link, with **no directory, index or link built into the app** (no server of ours, no hard-coded list; P10). Off by default, per board and per thread.

**Principle: discovery spreads through what people already follow.** A reader who knows no board finds nothing; one who knows one board can reach every board that chose to be discoverable and is connected to it. This is how webrings and early Usenet grew, and it keeps each hop an explicit, signed choice of an owner.

### G.17.1 What an owner publishes

- A **discovery card** in the board's signed manifest (G.5): `disc: {tags: [≤ 4 short words], blurb: ≤ 140 chars, nsfw: bool, since}` and, per discoverable thread, `{no, title}` in the catalog. No card → the board is invisible to discovery, and the app never lists it.
- A **"see also" list** in the same manifest: up to 16 other boards' names (+ onions) the owner vouches for, each with its own tags as the owner saw them. This is the only way one board points to another; it is signed, so a mirror cannot add entries.
- Served at `/disc` by the board's gateway (the card and the list, a few hundred bytes), so a reader fetches it without the whole board.

### G.17.2 How a reader discovers (the **Discover** view, Following tab)

1. **Seeds**: the boards and channels the reader already follows, plus links received from contacts. Nothing else; an empty follow list means an empty Discover view with the sentence "Follow a board to see the boards it recommends".
2. **Crawl**: over Tor, fetch `/disc` of each seed, then of the boards in their "see also" lists, breadth first, **depth ≤ 3**, ≤ 100 boards, at most 4 fetches in flight, only while the tab is visible and at most once an hour. Each card is verified (the manifest signature; the onion is the board's own or a signed mirror).
3. **Show**: cards grouped by tag, with "recommended by <board>" (the path it was found through), the blurb, `nsfw` hidden unless the reader turns it on. Opening a card = reading the board as usual.
4. **Cache**: the verified cards in the reader's storage for a day; nothing leaves the device (the crawl reveals to each board's host only that *someone over Tor* read `/disc`).

### G.17.3 Optional: public rendezvous on the IPFS network (spike B-P8)

For boards that want to be found by strangers with no shared seed:
- A **topic key** is derived from a tag (`HKDF("ephem-disc-v1" ‖ tag)`), known to everyone. An IPNS record under a publicly known key can be overwritten by anyone, so a single record per topic cannot work.
- Instead the board announces itself as a **provider** of a topic CID (`CID(raw, sha256("ephem-disc-v1:" ‖ tag ‖ week))`) on the public DHT, through the IPFS routing API from a Tor exit, as channels publish today. A reader asks the routing API for the providers of that CID and gets peer records; each one must then prove it is a board (its `/disc` card, signed) or is dropped.
- Unknowns: whether `delegated-ipfs.dev` accepts provider announcements over HTTP (the v1 API's `PUT /providers` is not generally served), and how easily the topic is flooded. **Only if B-P8 shows it works, and behind its own switch.** The IPFS routing host itself is already configurable (P10 forbids a hard-coded *directory*; the routing service is infrastructure, as for channels).

### G.17.4 Abuse and safety

| Risk | Answer |
|---|---|
| Illegal or abusive boards spread through "see also" | Each hop is a signed choice of an owner the reader already trusts; the path is shown ("found via A → B"); a reader can **block** a board, which also stops the crawl through it; `nsfw` off by default |
| Spam boards pointing at each other | Depth ≤ 3 and ≤ 100 boards; a reader-side score: boards reached by several independent paths first |
| Shared blocklists | A board may publish a signed **block list** (names); a reader can subscribe to the lists of boards they follow; no global list exists |
| Linking an owner's boards | "See also" is public by design; an owner who wants two boards unlinked does not list one in the other (the UI says so) |
| Readers' privacy | Everything over Tor; no query leaves the device except `/disc` fetches of boards already in the graph |

### G.17.5 Phases

| ID | Scope | Done when |
|---|---|---|
| BD-9 | Discovery card, "see also" list, `/disc`, per-thread flag, owner UI | Lab E2E: an owner turns discovery on; `/disc` serves a signed card |
| BD-10 | Reader crawl (depth 3, caps, cache), Discover view, block, nsfw | Lab E2E: three boards A → B → C; a reader following A sees B and C with their paths; blocking B hides C |
| B-P8 (spike) → BD-11 | DHT rendezvous by topic (provider records through the routing API) | Live: a provider announcement for a topic CID is accepted and found again, or the idea is dropped |

## G.16 Phases and spikes

| ID | Scope / question | Done when |
|---|---|---|
| B-P1 (spike) | `equix` for wasm32: builds without a JIT; solve and verify times in Chrome, Firefox, Safari and on an iPhone; Ed25519 verify time for comparison | ✅ Chromium (2026-09-29, `checks/spikes/RESULTS-B-P1-B-P2.md`): equix 0.7.0 builds for wasm32 (interpreted hashx); **88.6 ms per attempt, 2.16 solutions per attempt → 24.4 solutions/s per core**; verify ≈ 0.1 ms; 4 Web Workers 3.7×. Effort E costs E solutions on average: median ≈ 28 ms × E per core here, so **~10 s (B6) ≈ E 350 on this 2.1 GHz Xeon core, ≈ 90–175 on a phone 2–4× slower** (estimate). A native JIT attacker on 4 cores is ~37× one browser core. ⏳ Firefox, Safari, iPhone |
| B-P2 (spike) | Tor onion-service PoW (hs-pow v1) and introduction DoS limits in our vendored arti, service and client side | ✅ answered: arti 0.46.0 implements both sides behind the experimental `hs-pow-full` feature (descriptor pow-params, INTRODUCE2 check with replay log, effort-ordered queue, prop 362 effort updates; `enable_pow` config), and it compiles for wasm32, **but** the service runs its loops through `spawn_blocking` + `reenter_block_on` (panics in our page runtime, even with `enable_pow = false`) and the client solves on `std::thread::spawn`. Enabling it needs patches to vendored `tor-hsservice` and `tor-hsclient` (async loops; solve in steps or a Worker). Until then, introduction floods can take a board offline (G.8) |
| B-P3 (spike) | Does `tor-hsservice` tell which rendezvous circuit a stream came on? | Yes → per-circuit caps (G.8); no → dropped |
| B-P4 (spike) | Pixel readback of a 2 048 px image (memory on iPhone; noise under Firefox RFP, Brave, Safari); pure-Rust JPEG encode time in wasm | A 12 MP photo re-encoded under 2 s on an iPhone, peak memory measured |
| B-P5 (spike) | A 512 KiB `POST` to an onion from a tab over Snowflake: time and failure rate (lab and live) | ✅ **Lab** (`checks/tor-lab/spike_upload.mjs`, 2026-09-29; each tab its own arti over Snowflake, a new stream per upload, 5 each, 15/15 ok): stream open 20 ms median (395 ms for the first, with the descriptor and rendezvous); send + answer 4 KiB 64 ms, 128 KiB 156 ms (p95 232), 512 KiB 357 ms (p95 394), ≈ 1.4 MiB/s. The lab has no latency: an upper bound. **Real Tor network** (`RELAY=1`, the local-relay Snowflake leg, 15/15 ok): host online in 19.7 s; stream open 460–516 ms median (6.4 s p95 for the first, with the descriptor fetch and rendezvous); send + answer 4 KiB 546 ms, 128 KiB 642 ms (p95 1 564), **512 KiB 1 607 ms (p95 1 992)**, ≈ 320 KiB/s. The 512 KiB image cap stands; a text post costs ~1 s over the wire plus the PoW. ⏳ With real Snowflake volunteers (`LIVE=1`, laptop) |
| B-P6 (spike) | Host throughput in a tab: posts/s verified, built, signed and written; OPFS with thousands of block files (with C-P5) | ≥ 10 posts/s sustained, or the caps lowered to what is measured |
| B-P7 (spike) | Tor Browser Safest level: JPEG thumbnails through `img-src 'self'` | Shown, or the plain page goes text-only |
| BD-1 | `crates/board` model: blocks, limits, strict decode, chunking, tombstones, pruning, build and verify; unit tests, fuzzed decoders | Native tests: 100 threads × 500 replies built and verified; chunk CIDs stable across unrelated replies |
| BD-2 | Submit format, sans-IO host pipeline (G.6.2), Equi-X PoW, epochs, replay set, filters, caps | Native tests for every refusal row; fuzzed submit parser (10⁶ runs clean); no allocation after serve start (counting allocator) |
| BD-3 | Board gateway: bounded stream pool, `/pow`, `/submit`, keep-alive GETs; OPFS per-block store; owner and poster in the Tor build | Lab E2E: owner + 2 posters, a thread with replies, sage, bump order, a pruned thread |
| BD-4 | Images: poster re-encode (after B-P4), host whitelist + decode + thumbnail, `media`, reader "show images" | Lab E2E: a photo with EXIF/GPS posts; the served bytes carry no APP1; a JPEG with a COM segment is refused |
| BD-5 | Moderation, owner only (B7): deletes, lock, sticky, bans, trips, owner capcode, mod log, pause | Lab E2E: the owner deletes and bans; readers see the tombstone and the log entry |
| BD-5b | Later (B7): janitors, word filters, reports, pre-moderation | Lab E2E: a janitor deletes; a filtered post is refused |
| BD-6 | Reader UI (catalog, thread, reply box, watched threads), Following integration, the plain HTML page, board mirrors | Lab E2E: owner offline → read from a mirror, posting says `E_BOARD_OFFLINE`; plain page fetched through a C Tor client |
| BD-7 | Several devices, after V-1…V-3 | Lab E2E: take over on B, numbers continue without a gap collision |
| BD-8 | Later: mirror submit queues (G.10 b) | A separate proposal |

**Order:** B-P1, B-P2 and B-P5 first (they decide whether boards are practical at all), then BD-1 → BD-2 → BD-3; images (B-P4, BD-4) only after text boards pass BD-3 and BD-5.
