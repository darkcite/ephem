<!-- SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0 -->
<!-- Copyright 2026 Anton (darkcite) -->
# Appendix G: Boards (a 4chan-like channel type), proposed, not built

**Status: proposal, revision 2 (2026-10-03), nothing here is built.** Revision 1 (2026-09-29/30) was reviewed by two independent reviewers (security/protocol and feasibility/UX), who then cross-checked each other against the code; every finding and how this revision answers it is in **G.19**. The owner settled the four points they still disagreed on (G.15, R1–R4).

It extends Appendix D of `docs/P2P-CHAT.md` and follows every principle there, P10 most of all. Unknowns are spikes (G.16). The existing channel type (§27, Appendix D: one owner posts, followers read) **stays exactly as it is**: its blocks, record, gateway and UI are not changed by this proposal.

**What changed in revision 2, in one paragraph:** text-only boards come first (v1); images, helpers, discovery crawl and scale work move to later versions. The PoW is bound to the post key and solved while the user types, checked from the header before any body byte is read. A new bounded accept loop in `crates/tor` sits under everything. Records carry a time-based sequence, a 72 h validity with a labelled stale mode, and a signed deletion list. Taking a board to another device is explicit, fenced and keeps moderation state. Capacity claims are cut to what has been measured ("low thousands of Tor readers"), and the honest weak points (one desktop tab over one volunteer proxy; PoW is a speed bump, not a wall) are stated up front.

## G.1 Goal and constraints

| # | Constraint | Consequence for boards |
|---|---|---|
| G1 | Browser tab only, no servers of ours, no companion (P1, P10) | The board is hosted by the **owner's tab** as an onion service, like a channel (D.2). Nothing is always on |
| G2 | The owner is hidden (D2) | Posts reach the host only **over Tor**; the host never joins the public IPFS network |
| G3 | Anyone with the link may post, with no account | Anti-spam cannot use IPs (an onion service sees none) or accounts: it uses **proof of work, budgets, and moderation switches** (G.8, G.9) |
| G4 | Content is IPFS-native and verified in Rust (D.2) | Reuse `crates/channel`'s `cbor`, `cid`, `car`, `ipns`, `time`, `varint`; a new crate `crates/board` holds the model. `channel::channel` is untouched (its `build()` rebuilds every block per change: fine for a channel, not for 75 000 posts) |
| G5 | HFT taste (§22) | Bounded everything **from the Tor layer up** (G.6.2); every input refused before it can grow past a fixed slot; single writer |
| G6 | P7 | A board is a publication, not a chat: permanent and public, like a channel. No private-chat data flows into it |
| G7 | Hosting needs a desktop browser (D1, E8) | v1 hosts from **desktop Chrome or Firefox** only; Safari hosts with a warning (hidden tabs throttled); phones read and post, never host |

**What a board is:** boards → threads → replies. Anyone holding the board link and running the app can start a thread or reply; the owner moderates; the owner's tab orders, signs and serves the result.

**The honest weak point, first.** In v1 a board takes posts only while one desktop tab is online over one volunteer Snowflake proxy. A typical owner's duty cycle is perhaps 30–60 %; reading continues from mirrors when they exist. Whether a tab stays reachable for hours on the live network is not yet measured (**B-P11 gates BD-3**). The real availability fix, mirrors as the front door, is v1.5 (BD-8).

## G.2 What "4chan-like" means here

| Feature | Decision | Notes |
|---|---|---|
| Boards → threads → replies | **Yes** | One board per board key; a thread is an OP plus replies |
| Anonymous posting, no accounts | **Yes** | Each post is signed by an **ephemeral poster key** (G.4); "Anonymous" is the default name |
| Post numbers | **Yes**, board-wide, monotonic `no` (u64), assigned by the host | Never reused, also across devices (G.13) |
| Quote links `>>123`, backlinks | **Yes**, computed by readers from the body | `>>>` cross-board links: **no** in v1 |
| Greentext (`>` at line start) | **Yes**, render-only | Bodies are plain text, escaped everywhere |
| Poster IDs per thread | **Owner flag** `ids`, **off by default** (B5); IDs-on is built after v1 | On: one random key per thread per tab, shown as an 8-character ID |
| Tripcodes | **Yes, as signed keys** ("trip keys") | Unforgeable; shown with 16 characters (G.4) |
| Capcodes | **Owner** in v1; janitors later (BD-5b) | Signed by the board key or a listed janitor key |
| Bump order, bump limit, sage | **Yes** | A reply bumps its thread unless `sage` or past the bump limit |
| Pages, pruning, archive | **Yes** | Oldest-bump pruned, with **prune protection** (G.8); a text-only archive, pinned (G.5) |
| Sticky, locked | **Yes** | Owner actions |
| Catalog view | **Yes** | Built by readers from 10 catalog buckets (G.5.1) |
| Images | **Not in v1.** v2: off by default, owner opt-in, pre-moderated, decoded only in our wasm (G.7) | JPEG only |
| Video, animated GIF, audio, files | **No** | |
| Captcha | **No** | Solvable by paid services, hurts Tor users most |
| Flags, country, IP-based anything | **No** | There is no IP |
| Post editing | **No** | Delete and repost |

## G.3 Roles and topology

```
 POSTER (any tab with the app, Tor)        OWNER: dedicated desktop tab, own Tor client      READERS
 ┌────────────────────────────┐            ┌───────────────────────────────────────┐        ┌────────────────────┐
 │ reply box opens → GET /pow │ GET /pow   │ board onion                           │        │ app (Following)    │
 │ solve while typing (Worker)│──────────▶ │  bounded accept loop (crates/tor)     │◀───────│ Tor Browser (no JS)│
 │ sign s; header k,n,h,PoW   │ POST       │  /pow  → epoch, seed, efforts         │  GET   │ mirrors (D.7),     │
 │ fresh circuit per submit   │──/submit──▶│  /submit → header PoW → body → accept │        │ read-only          │
 └────────────────────────────┘ ◀─ 200 no ─│  publish ≤ 1/s; serve /ipfs /ipns /   │──────▶ └────────────────────┘
                                           │  OPFS: boards/<name>/b/<cid>          │  pull ≤ 1/10 s
                                           └───────────────────────────────────────┘
```

- **Only the host writes.** It assigns numbers and time, applies moderation and signs the snapshot (the IPNS record). Everyone else submits.
- **The host runs in the main app tab (R6)**, beside the owner's chat onion and channels. One shared Snowflake session makes every onion in the tab go down and up together, which links them (A-M5); the board's warning says so, and an owner who needs them unlinked hosts the board from another browser profile.
- **Mirrors are read-only in v1** (G.10). In v1.5 they become the front door and the writer onion becomes private (BD-8).

## G.4 Keys and identities

| Key | Derivation | Held | Purpose |
|---|---|---|---|
| Board signing key | `HKDF(seed, "p2pchat/board/" ‖ u32 index)` → Ed25519; the IPNS name | Owner | Signs the record, owner actions and capcode posts |
| Board onion key | `HKDF(seed, "p2pchat/board-onion/" ‖ u32 index)` | Owner | A dedicated onion, unlinked from the chat onion and from channels |
| Writer onion key (v1.5) | `HKDF(seed, "p2pchat/board-write/" ‖ u32 index ‖ u32 n)` | Owner | The restricted-discovery writer onion of BD-8; `n` rotates it |
| PoW secret | `HKDF(seed, "p2pchat/board-pow/" ‖ u32 index)` | Owner (every device) | Epoch seeds (G.8); a second device accepts the same seeds |
| Owner-state key | `HKDF(seed, "p2pchat/board-own/" ‖ u32 index)` | Owner | Encrypts the moderation state block (G.5.1) |
| Poster key, IDs off | Random Ed25519 **per post**, RAM only | Poster | Signs one post |
| Poster key, IDs on | Random Ed25519 per (board, thread) per tab | Poster | The thread ID is `base32(BLAKE2b-40("ephem-board-id" ‖ board ‖ thread ‖ pk))` |
| Trip key (optional) | `HKDF(seed, "p2pchat/board-trip/" ‖ board name ‖ label)` | Poster, signed in | A stable name on one board, shown `!` + **16** base32 characters (80 bits; 10 characters were GPU-grindable, A-m2) and the full key on tap |
| Janitor key (BD-5b) | `HKDF(seed, "p2pchat/board-janitor/" ‖ board name)` | Janitor | Listed in the manifest with rights |

**Signing domains are separate per kind** (A-M10): `"ephem-board-post-v1:"` (post), `"ephem-board-act-v1:"` (owner or janitor action), `"ephem-board-del-v1:"` (self-delete), `"ephem-board-report-v1:"` (report), `"ephem-board-manifest-v1:"`. A signature for one kind never verifies as another.

- Board indices 0–3: **at most 4 boards per identity**, and one per hosting tab.
- The poster's chat identity is never used, except to derive a trip key the poster chose. **A trip signature removes deniability:** a seized key file proves authorship of every post under that trip (the UI says so when a trip is first used).
- Poster keys die with the tab.
- **What a signature proves, honestly:** a mirror or gateway cannot forge or alter a post. The host **can** invent posts under fresh keys, indistinguishable from real anonymous posts; it cannot post as an existing trip key or thread ID. The host-added fields `no`, `ts`, `cap` (and in v2 `thumb`, `w`, `h`) are signed only by the host's record, not by the poster. A board is exactly as honest as its owner.

## G.5 Data model

### G.5.1 Blocks (strict dag-cbor, canonical, as D.5.1)

Two kinds of reference, on purpose: **pin links** (CBOR tag 42, followed for pinning, GC and `dag-scope=all`) and **view refs** (CID bytes, never followed). **The GC rule:** a block is kept exactly when it is reachable from the current root **by pin links**; everything else is deleted, on the host, on mirrors and (in v2) on helpers.

```
IPNS record ──▶ root {v:1, kind:"board", manifest⁴², cat:[bucket⁴² ×10], threads⁴², archive⁴², arch_threads⁴²,
                      dels⁴², modlog⁴², own⁴², ev: null, next_no, updated}
  manifest      {v, kind:"board", title, about, rules, pk, created, mirrors[≤8], see_also[≤16], flags, limits, sig}
  bucket i      {t: [{no, thread (ref), bump, r, sub, ex, st, lk}]}      threads with no mod 10 = i
  threads       {t: [thread⁴² …]}                                       pin index of live threads
  thread        {no, sub, chunks: [chunk⁴² …] ≤ 8, r, st, lk}
  chunk         {p: [post …] ≤ 64, oldest first}
  post          {no, ts, s, cap}   or tombstone {no, ts, del, by}
  s (signed)    {b: board name, t: thread no (0 = new), k: pk, n: [16] nonce, sub, body, sage, e: epoch}
  archive       {t: [{no, sub, ex, pruned, thread (ref)}] ≤ 256}
  arch_threads  {t: [thread⁴² …]}                                       pin index of archived (text-only) threads
  dels          {d: [{h: BLAKE2b-256 of the deleted s, at}] ≤ 4 096, 30 days}   signed by the record
  modlog        {a: [{ts, act, no, why}] ≤ 1 024, newest last}
  own           XChaCha20-Poly1305 ciphertext, padded to 4 KiB steps   bans, filters, efforts, held queue
  ev            reserved for the v2 event log (always null in v1)
```

- **Catalog in 10 buckets by `no mod 10`, sorted by readers.** A reply rewrites exactly one bucket (its thread's), not a 128 KiB catalog. Reviewer A suggested splitting by page; that would not work, because a bump moves a thread to page 1 and shifts every page above its old position by one entry. Buckets by number never shift; readers sort the ≤ 150 entries by `bump` locally (microseconds) and build the 10 pages.
- `ex` is the first **140 bytes** of the OP body, cut on a UTF-8 boundary; `r` the reply count; `st`/`lk` sticky and locked.
- `s` is signed as `"ephem-board-post-v1:" ‖ dag-cbor(s)` by `s.k`. The host adds `no`, `ts` (**rounded to the minute** in the published post, A-M5: exact times reveal the tab's clock skew, which links the owner's boards; exact times stay host-local) and `cap` (0 anon, 1 owner, 2 janitor n).
- **`dels`, the deletion list** (A-M4): when a post is deleted the host appends the hash of its `s`. A reader or mirror that has seen a deletion list refuses to render or serve any post whose `s` hashes to a listed value, **even from an older root** that a stale mirror or a Sybil serves. Entries live 30 days (longer than any record's validity).
- **`own`, the owner-state block** (B-M6): bans, filters, efforts and the held (pre-moderation) queue, encrypted with the owner-state key and padded. It travels with the board, so a takeover on another device keeps every ban and filter. Others see only its padded size.
- A reader detects the type from the root: a channel root has no `kind`; a board root has `kind: "board"`. Channel code refuses a board root as `Invalid`.

**Reader verification rules** (B-m6, written as `verify` rules with tests in BD-1): `s.b` equals the board; `s.t` equals the containing thread (or 0 for an OP whose `no` is the thread's); `no` strictly increasing within a thread; every chunk but the last holds exactly 64 posts; `r` = posts − 1; `next_no` > every `no`. Together they give the channel crate's property that a chain cannot lie about its count: withheld chunks show as missing, not as a shorter thread.

### G.5.2 Limits (defaults; the owner may tighten, never loosen past the maximum)

| Item | Default | Maximum | Why |
|---|---|---|---|
| Body | 2 000 B UTF-8 | 2 000 B | 4chan's size; a chunk ≤ ~170 KiB |
| Subject | 100 B | 100 B | |
| Threads per board | **150 (10 pages × 15, B4)** | 150 | Each bucket ≤ ~15 entries (~5–13 KiB) |
| Replies per thread | bump limit 300, hard cap 500 (then locked) | 500 | 8 chunks of 64 |
| New threads, board-wide | **1 per 2 min**, tightening automatically (G.8) | owner setting | A-B3: thread creation is the pruning weapon |
| Prune protection | a thread younger than 30 min or bumped in the last 10 min is never pruned by a new thread; the new thread is refused instead | owner setting | A-B3 |
| Accepted posts, board-wide | 120 / min | 600 / min | Above it: effort-ordered intake (G.8) |
| Board text in RAM | 64 MiB per board, **128 MiB per hosting tab in all** | same | Pruned by RAM bytes as well as by thread count (B-M11); wasm memory never shrinks |
| Board store (OPFS) | 512 MiB | 2 GiB, and the browser quota (C-P5) | |
| Submit request | ≤ 4 KiB in v1 (text); ≤ 520 KiB in v2 (one image) | same | |
| Rendezvous in flight / stream ring / submit slots | 4 / 32 / 8 (writer) | same | G.6.2 |
| Archive | 256 threads, 7 days, text only, pinned | same | |
| Record validity | **72 h** (R1) | 72 h | G.5.3 |

### G.5.3 Chunking, deletion, records and the store

- A thread's posts fill chunks of 64 in order. **A full chunk never changes** unless a post in it is deleted, so its CID is stable; a refresh fetches the thread block and the last chunk.
- A reply rewrites: the last chunk, the thread block, one catalog bucket, the `threads` index and the root, about **25–40 KiB** in all for a busy thread (A-B2 counted ~68 KiB with the single catalog).
- **Deletion** replaces the post with a tombstone `{no, ts, del, by}` (`del`: 1 owner, 2 janitor, 3 poster, 4 filter) and appends to `dels`. The chunk's CID changes, and so do the blocks above it. Numbers are never reused. **Deleting an OP prunes its thread** (4chan behaviour, B-m5).
- **Pruning** removes a thread from its bucket and `threads`; with the archive on, a text-only copy is put in `archive` and pinned in `arch_threads` for 7 days (B1: in revision 1 the archive was referenced but not pinned, so GC deleted it).
- **Publishing is coalesced:** accepted posts wait in a fixed ring and are built and signed together **at most once a second, and at most once every 5 s while the post cap is more than half used** (A-B2).
- **The IPNS record:**
  - `sequence = max(last + 1, unix_ms)` (**B2/A-M11**). This is monotonic across devices with no coordination, so a takeover never publishes below a reader's high-water mark. Readers refuse a record whose `sequence` is more than 1 h ahead of their own clock (a `2⁶⁴−1` record would otherwise pin every reader forever).
  - `validity = now + 72 h` (**R1**), `ttl = 60 s`. The writer republishes at least every 24 h even when nothing changed.
  - **Stale mode:** after expiry a reader may still show the last verified state, labelled "Board not updated since <date>; the host is offline", with reading only. A board never silently disappears after a weekend offline, and a pre-deletion snapshot cannot be revalidated for a month.
  - Mirrors re-`PUT` only unexpired records; they cannot extend validity.
  - The optional `PUT` to `delegated-ipfs.dev` through a Tor exit runs at most every 10 minutes. V-P2 measured 13–86 h of DHT lifetime without republishing, which the 24 h republish covers.
- **Store:** one OPFS file per block (`boards/<name>/b/<cid>`) plus `record.bin`, written from a Worker with `SyncAccessHandle` (main-thread `createWritable` costs milliseconds per file; B-P6 measures both). After each publish, files no longer reachable by pin links are deleted.
- **The gateway never builds a whole-board CAR** (B-M10): `format=car` is refused (406) for the root, `threads` and `arch_threads`; it is served for a bucket, a thread (with its chunks) or a chunk. **Export backup** streams block by block into a file (File System Access, or Blob parts), never one `Vec`.

## G.6 Submitting a post

### G.6.1 Protocol: HTTP/1.1 over the board onion

**Decision: `POST /submit`**, the same HTTP/1.1 subset the channel gateway speaks, extended in `crates/board::gateway`, with `Connection: close` (keep-alive is cut from v1, B-m14: arti already reuses the rendezvous circuit for new streams). A Noise-authenticated stream is rejected: the onion authenticates the host and encrypts end to end; the poster has no stable key to authenticate with.

1. **When the reply box opens** (not when Post is pressed): `GET /pow` → a fixed body `{epoch u32, seed [32], effort_thread u32, effort_reply u32, effort_report u32, min_thread u32, min_reply u32, paused u8, threads_open u8}` (58 bytes, little-endian; `min_*` are the lowest efforts advertised in the current and previous epoch, see the grace rule in G.8).
2. The poster's tab draws a fresh poster key `k` (IDs off) and a random 16-byte nonce `n`, and **solves the PoW while the user types** (G.8), in 1–4 Web Workers, under the Screen Wake Lock. Most replies are then posted with no wait at all.
3. On Post: build `s` with `k`, `n`, sign it, and send `POST /submit` with `Content-Type: application/vnd.ephem.board-submit` and `Content-Length`, **on a new Tor isolation group** (A-M9: otherwise one circuit carries several "unlinkable" posts; the extra 3–6 s of rendezvous overlap with typing). `GET /pow` and reads use another group.
4. Answer: `200` with a fixed body `{no u64, seq u64}` once the post is in a published snapshot (≤ ~1–5 s), or an error status plus a stable `u16` code (G.6.3).
5. **Idempotent resubmit** (B-m9): a retry of the same submit after a dropped stream (same solution, same `h`) returns the original `{no, seq}`, not a refusal. The client also confirms by finding its own `s.k` in the next thread fetch, and turns "Posted as No. N" into "✓ seen on the board".

**Submit format (little-endian, parsed in place). Everything needed to refuse is in the header, before any body byte is read** (A-M2):

| Off | Size | Field | Notes |
|---|---|---|---|
| 0 | 4 | `magic` | `EPB2` |
| 4 | 1 | `kind` | 1 thread, 2 reply, 3 self-delete, 4 report, 5 owner/janitor action, 6 capcode post |
| 5 | 1 | `flags` | bit0 `sage`; others MUST be 0 in v1 (bit1 `image` in v2) |
| 6 | 2 | `text_len` | dag-cbor `s` (or the action map), ≤ 2 400 |
| 8 | 4 | `img_len` | 0 in v1 |
| 12 | 4 | `epoch` | current or previous |
| 16 | 4 | `effort` | ≥ the grace minimum for `kind` (G.8) |
| 20 | 8 | `thread` | 0 for a new thread |
| 28 | 32 | `k` | the signing key (poster, owner or janitor) |
| 60 | 16 | `n` | the per-post nonce, also inside `s` |
| 76 | 32 | `h` | `BLAKE2b-256(s ‖ image)` |
| 108 | 16 | `solution` | Equi-X solution |
| 124 | 64 | `sig` | Ed25519 by `k` over the kind's signing prefix ‖ `s` |
| 188 | var | `s`, then image bytes (v2) | nothing after them |

### G.6.2 The front door: bounded from the Tor layer up

**Below HTTP: a new accept loop in `crates/tor` (BD-0, before boards).** Today `Tor::launch` uses arti's `handle_rend_requests` (`vendor/tor-hsservice/src/helpers.rs:15`, `flat_map_unordered(None, …)`: unbounded concurrent rendezvous circuit builds) and queues every stream in an unbounded `VecDeque` (`crates/tor/src/web/mod.rs:171-174`). An introduction flood therefore becomes unbounded circuit builds over Snowflake whatever the gateway does (A-M3, B-M4). The replacement:

| Layer | Rule | On excess |
|---|---|---|
| Rendezvous | ≤ K `RendRequest::accept` in flight (writer K = 4, mirror K = 16) | `RendRequest::reject()` at once (`req.rs:246`), no queue |
| Per circuit | Each accepted rendezvous yields its own `Stream<StreamRequest>` (`req.rs:209`): that is the circuit's identity, so **spike B-P3 is answered without a spike**. ≤ 4 concurrent streams, ≤ 1 `/submit` per 10 s | `StreamRequest::shutdown_circuit()` |
| Stream queue | A fixed ring of 32 | `StreamRequest::reject` |
| arti config | `max_concurrent_streams_per_circuit = 8` (backstop); a **moderate** `rate_limit_at_intro` (rate 10, burst 50 per intro point) | The intro points drop the excess. Honest limit: this bucket drops legitimate and attacker introductions alike; it protects the tab, not availability (`vendor/tor-hsservice/src/config.rs:46-56`) |

The channel gateway and the chat onion adopt the same loop (they share `Tor::launch`).

**Above it, the host pipeline.** At serve time the host preallocates 8 submit slots (4 KiB + 8 KiB head in v1), the replay set, the idempotency ring and the publish ring. Per stream:

| Step | Check | On failure |
|---|---|---|
| 0 | A free slot (else close at once) | `503`, close |
| 1 | Head ≤ 8 KiB, `POST /submit`, `Content-Length` = 188 + `text_len` + `img_len`, a 10 s total deadline, and **≥ 16 KiB/s after the first 2 s** (slow-loris) | `431`/`413`/`408`, close |
| 2 | The 188-byte header: magic, kind, flags, lengths, epoch current/previous, effort ≥ the grace minimum, board not paused, new threads open (`kind` 1) | `400`/`409`, close **before reading the body** |
| 3 | **Replay set:** `(challenge, solution)` not seen in 2 epochs. If seen with the same `h` and already accepted → answer the original `{no, seq}` (idempotent) | `409` |
| 4 | **Equi-X verify** from the header alone (≈ 0.1 ms) and `BLAKE2b-32(challenge ‖ solution) × effort ≤ 2³² − 1`; on success the solution enters the replay set **now** | `403`, `E_BOARD_POW`, close |
| 5 | Read the body into the slot (one copy, G.14.2); `BLAKE2b-256(body) = h` | close |
| 6 | Strict dag-cbor decode of `s` with a **borrowing decoder** (`cbor::Value::decode` allocates a tree, B-BD-2); `s.b` = this board, `s.t` = header `thread` (a live, unlocked thread or 0), `s.k` = `k`, `s.n` = `n`, `s.e` = `epoch` | `400` |
| 7 | Ed25519 verify under the kind's prefix; key not banned; word filters (BD-5b) | `403` |
| 8 | Duplicate body in this thread's last 64 posts; the thread budget and prune protection (G.8) | `409`/`429` |
| 9 | Into the publish ring (effort-ordered when contended, G.8); answer after the next signed snapshot | `503` if the ring is full |

**The no-allocation rule** covers steps 0–8 (the refusal path). An accepted post's block bytes are new bytes, written into an arena sized by the RAM budget (B-M11).

### G.6.3 Error codes (additions to §19)

`0x0070 E_BOARD_POW` (bad or insufficient work, or a stale epoch) · `0x0071 E_BOARD_BUSY` (no slot, ring full, the posts cap, or new threads closed by the budget) · `0x0072 E_BOARD_REFUSED` (banned key, filter, locked or pruned thread, duplicate) · `0x0073 E_BOARD_PAUSED` (paused, or trips-only and no trip) · `0x0074 E_BOARD_IMAGE` (v2) · `0x0075 E_BOARD_OFFLINE` (the host cannot be reached; reading may still work from mirrors).

## G.7 Images (v2, not in v1)

Revision 1 had the host decode every attacker image in its own browser's native decoder, in the one renderer that holds the owner's IP (through the Snowflake WebRTC connection) and every key (**B3/A-M7**). Revision 2:

| Question | Decision |
|---|---|
| When | **v2**, after text boards pass v1. Off by default per board; turning it on needs the G.9.3 warning **and** pre-moderation of image posts (B-M8) |
| Poster side | Decode in the browser (`createImageBitmap`) → RGBA → **our pure-Rust JPEG encoder** in a terminable Worker (B-m7: wasm memory never shrinks; the worker is killed after the post) → only SOI, APP0, DQT, SOF0/SOF2, DHT, DRI, SOS, EOI. Same bytes on every engine for the same pixels |
| Host side | Rust segment whitelist (fuzzed), then **decode with a memory-safe pure-Rust decoder (`zune-jpeg`) in a terminable Worker**, with dimension caps; the thumbnail is made from those pixels. **The browser's native decoder never sees an unmoderated image in the owner's tab.** The pre-moderation view shows the host-made thumbnail; the full image only on an explicit click (the owner's own legal exposure) |
| Reader side | The same whitelist; images shown only through `blob:` URLs of verified bytes; "show images" **off** per board until the reader turns it on |
| Thumbnails | **View refs** in posts and catalog, pinned through a per-thread media index that thread and chunk CARs do **not** follow (B-M7: in revision 1 a reader with images off still downloaded every thumbnail). Fetched `format=raw` only when the reader shows images. Mirrors are **text only** by default |
| Anti-fingerprinting noise | Brave's farbling is seeded per session, so noise in two images from one session correlates and links "IDs off" posts (A-M8). B-P4 measures it; prefer an unfarbled readback path (WebCodecs `ImageDecoder` → `VideoFrame.copyTo`) where it exists; otherwise add fresh per-image noise and downscale, and warn |
| Byte budget | When the store budget is hit, full images are pruned first; thumbnails and text stay (A-B3) |
| Video, GIF, audio | No |

## G.8 Anti-spam without IPs

**The honest framing first.** PoW is a speed bump against hobbyists, not a wall. The measured attacker edge (B-P1) is **9.2× per core** against desktop wasm (a native JIT does 226 solutions/s per core, our wasm 24.4) and **~18–37× per core against a phone**. One rented server fills any cap a phone can afford. The real defences against a determined spammer are the **budgets and switches** below, and the owner's moderation.

| Layer | Mechanism | Honest limit |
|---|---|---|
| Proof of work | **Equi-X** via arti's pure-Rust `equix` (interpreted hashx in wasm). Challenge = `"ephem-board-pow-v2" ‖ board name ‖ seed(epoch) ‖ kind ‖ thread ‖ k ‖ n ‖ effort`. Valid when `BLAKE2b-32(challenge ‖ solution) × effort ≤ 2³² − 1` | Buys cost, not identity |
| **Bound to the post key, not the content** (B-M3) | Solved while typing. Safe because: `n` is a fresh random per-post nonce **signed inside `s`** (with IDs on, `k` is shared within a thread); `k` and `n` are in the header, so the PoW is checked before the body; the solution enters the replay set at that check; the epoch seed limits precomputation to ~20 min. Reusing a solution needs the poster's private key to sign a new `s`. After a takeover the replay set is lost; a replay of the same signed `s` hits the per-thread duplicate check | — |
| Freshness | `seed(epoch) = BLAKE2b-keyed(pow secret, epoch)`, epoch = 10 min; current and previous accepted | — |
| Efforts | Owner sets a base; defaults reply ×1, new thread ×8, report ×½. Target **~10 s median for a reply on a mid-range phone with 4 workers, measured** (B-P1b; default base **700**, see the B-P1b row), and the UI shows the **p95** too (Equi-X solve times have p95 ≈ 3 × mean; k-of-n sub-puzzles are an option B-P1b measures to narrow it) | Slow phones pay more |
| **Effort grace** (B-M2) | A submit is accepted at ≥ the **lowest** effort advertised in the current or previous epoch, so an effort raised mid-solve never voids finished work | A flood is slowed one epoch later |
| **Effort-ordered intake** (A-B3) | When submissions contend for the publish ring, the highest effort per post wins (as Tor's prop 327); a refused client may retry with more effort. The board degrades to the highest payers, not to whoever arrives first | Rich attackers still win slots |
| **Thread budget and prune protection** (A-B3) | ≤ 1 new thread per 2 min board-wide (owner setting), tightening automatically when the budget is used up for 10 min; young or recently bumped threads are never pruned by new ones; new threads are refused instead | Revision 1 let a 4-core laptop prune all 150 threads in ~4 min |
| **Automatic panic modes** | When the posts cap is saturated for 10 min, or the thread budget for 30 min, the board switches itself to **trips-only** or **new threads closed** (owner's choice of which), shows it in `/pow`, and notifies the owner | — |
| Caps | Board-wide posts per minute; per-circuit ≤ 1 submit per 10 s (G.6.2); per-thread duplicate check | New circuits are cheap for a client |
| Onion-service PoW (Tor's own) | Wanted; blocked in the browser until two vendored-arti patches (B-P2). Until then, introduction floods are met by the accept loop and `rate_limit_at_intro` (G.6.2), and in v1.5 by the restricted-discovery writer (BD-8) | A determined introduction flood can take a v1 board offline for writing; readers use mirrors |
| Switches the owner sets | Pause posting; new threads by owner only; **trips-only**; replies only from trips the owner approved; pre-moderation (posts held until approved) | — |

## G.9 Moderation

### G.9.1 Actions and who may do them

| Action | Owner (v1) | Janitor (BD-5b) | Poster (own post, same tab) |
|---|---|---|---|
| Delete post | ✓ | ✓ | ✓ (self-delete, kind 3, signs `{b, no, k}` under its own prefix, low effort) |
| **Mass delete:** all posts since T; all in a thread; all with this ID (IDs on) | ✓ | — | — |
| Lock, sticky, prune thread | ✓ | lock only | — |
| Ban a **trip key**, or a poster key in an IDs-on thread | ✓ | ✓ | — |
| Pause, new threads closed, trips-only, approved trips, **pre-moderation** | ✓ | — | — |
| Word filters, efforts, janitors, rules | ✓ (filters and janitors in BD-5b) | — | — |
| Approve held posts, read reports | ✓ | ✓ | — |

- **Owner actions are local in v1** (the owner's own tab applies them), so they have no replay surface. **Janitor actions (BD-5b)** are kind 5 maps `{b, e: epoch, n: 16 B, no, act, why}` under `"ephem-board-act-v1:"`, and the host keeps a replay set for them (A-M10).
- **Bans are weak, and the UI says so.** With IDs off (the default) every post has a fresh key, so "ban this key" is shown only for trips and IDs-on threads (B-M9). The brakes are the budgets, the switches, pre-moderation and mass delete.
- **Undo:** an owner delete waits 5 s before the publish batch, with Undo (CONTACTS-UX conventions; no browser dialogs).
- **Filters and bans live in the encrypted `own` block** (G.5.1), unreadable to spammers, carried across devices.
- **Reports** (kind 4, a small PoW) go to a ring of 256, shown in the owner view.

### G.9.2 Illegal content (CSAM and the like)

- **The owner is the publisher.** The owner's tab signs and serves everything it accepts.
- **No automatic scanning.** Known-abuse hash lists are not available to a browser app without a server of ours (P1).
- **Deletion:** removes the blocks from the owner's store and from every later snapshot, and puts the post's hash on the signed **deletion list**, which mirrors and readers enforce even against older roots (G.5.1). With the 72 h validity, a pre-deletion snapshot stops verifying within 3 days at most. **Kubo nodes, gateway caches and readers' caches may still keep older copies.**
- v1 is text only. In v2, images are off by default and need pre-moderation; readers' "show images" is off by default; mirrors are text only by default.

### G.9.3 Warnings shown before creating a board (one checkbox, as channels do)

1. "You publish whatever your board accepts. You are responsible for it where you live."
2. "Anyone with the link can post. Spam and illegal content will arrive; only you can remove it, and only while your hosting tab is online."
3. "Deleting removes a post from your board and tells mirrors and readers to drop it. Copies made by others before that may survive."
4. "Host the board in its own tab: hosting it beside your chat lets anyone who can reach both tell they are the same person." (The app enforces the separate tab.)
5. "Your hosting tab's IP is visible to the volunteer Snowflake proxy it uses, and anyone can make it send traffic by posting. For stronger protection use bridges you trust (Settings → Tor bridges)." (A-M5)
6. (v2, images on) "Images greatly raise the risk. Turn them on only if you will pre-moderate."
7. The channel warnings of D.3 (writing style, posting times; a separate identity is recommended).

## G.10 Availability and mirrors

| Option | Posting while the owner's tab is off | Decision |
|---|---|---|
| (a) Posting pauses; reading continues from mirrors | No. "The board's host is offline since 21:40: reading from a mirror; posting resumes when the host is back" | **v1** |
| (b) **Mirrors are the front door** (BD-8): posters submit to a mirror; mirrors check the PoW from the header (they can: `/pow` serves the epoch seed, A-M1), run the same bounded loop, and forward batches to the writer over one long-lived stream; the writer re-verifies and keeps the authoritative replay set. The writer onion uses arti's **restricted discovery** (`vendor/tor-hsservice/src/config/restricted_discovery.rs`), authorised only for the signed mirrors' keys, so outsiders cannot even find its introduction points | Queued at the mirror until the writer is back | **v1.5 (R4)**, after spike **B-P12**: the feature is in arti's experimental set and our wasm keystore path for client keys is untested; it also makes posting need a mirror online |
| (c) Multi-writer (merged log or CRDT) | Yes | **Rejected.** Numbers, order, bumping and moderation need one writer |

- Mirrors are D.7 mirrors of a board root: "Mirror this board" copies the text DAG reachable by pin links, applies the deletion list, re-`PUT`s only unexpired records, pulls **at most every 10 s** (not 1 Hz: 8 mirrors polling every second exceed the writer's link, A-B2) and serves read-only, including the plain HTML page. Their `/submit` answers `E_BOARD_OFFLINE` with the owner's onion as a hint (until BD-8).
- **The honest limit:** in v1 a board is **writable only while its owner's hosting tab is open**, and readable while that tab or any mirror is open.

## G.11 Reading paths

### G.11.1 The app

- Following lists boards beside channels (follow entries gain `"k": "board"`; `saveFollows` keeps it, and an app that does not know a kind shows "update the app" instead of trying it as a channel, B-m4). A reader fetches **the index** in one request, `GET /ipns/<name>?format=ephem-board` → `u16 len ‖ record ‖ CAR(root; manifest, 10 buckets, threads, archive, arch_threads, dels, modlog)` (as built, BD-3: the record and the blocks it names always come from the same version, and one Tor round trip replaces eleven), then per open thread `GET /ipfs/<thread>?format=car` (the thread block and its chunks). Every block is checked by CID, every post by its poster signature and the deletion list, the root by the record.
- **Response caps per request** (A-m3): record ≤ 10 KiB, block ≤ 1 MiB, a bucket or thread CAR ≤ 1.5 MiB. The channel gateway's 64 MiB `MAX_RESPONSE` does not apply to boards.
- **Cold start:** the catalog is shown at once from the last verified copy (CIDs in IndexedDB) and refreshed behind it (B-UX-3).
- Refresh: an open thread every 30 s while on screen; the catalog on open and every 10 min with the follow list. **Watched threads** (≤ 32) in RAM, or in the key file as TLV `0x07 WATCHED` when signed in.
- "Updated N min ago" from `root.updated` is always visible; expired records show the stale label (G.5.3).
- **Board links** use their own fragment, `#B=<name>&o=<onion>&m=<mirror>…` (`#b=` is taken by bridge links, B-m3).

### G.11.2 Tor Browser (plain page, no JS)

- The onion serves `/` (catalog, 10 pages) and `/t/<no>` (a thread), built by `crates/board::page`. CSP `default-src 'none'; style-src 'unsafe-inline'; form-action 'none'` (v2 adds `img-src 'self'` for thumbnails), `X-Content-Type-Options: nosniff`, no referrer.
- **Posting from the plain page: no** (B3). The page says: "Posting needs the Ephem app."

### G.11.3 Public IPFS gateways

Only through followers' Kubo nodes (a manual `ipfs dag import`, D.7.2) or, later, a pinning-service push (V-4). A live board changes every second, so a manual Kubo copy goes stale within minutes: this path is for archives and quiet boards, not for scale (A-B1). Posting always goes over Tor.

## G.12 UI placement

- **My channels → "+"**: "Create a channel" (unchanged) or "Create a board" (title, about, rules, IDs on/off, efforts, the G.9.3 checkbox). Boards show a ▦ badge.
- **"Host this board"** opens a dedicated hosting tab with its own Tor client (G.3); the main app tab reads the board like any follower. The hosting tab shows uptime, "last reachable from outside" and reach (F.6), honestly.
- **Following**: boards and channels in one list; a board row shows title, new threads, new replies in watched threads, where it was read from (owner/mirror), and "host last seen".
- **Board view**: catalog grid (subject, reply count, sticky/locked marks; sort by bump, new, replies; local search), then a thread view (No., trip, capcode, greentext, `>>` previews, backlinks, "(deleted)").
- **Reply box**: the PoW starts when it opens ("Getting ready to post…" → "Ready"), with an honest estimate (median and p95). Subject (new thread only), body with a byte counter, sage, then **Post** → "Sending over Tor" → "Posted as No. N" → "✓ seen on the board". Before composing, a line says whether the host is online ("Host offline since 21:40: you can write now, it is sent when the host is back"). **Drafts are kept** and retried automatically after `E_BOARD_OFFLINE` or a dropped stream (idempotent, G.6.1).
- **Owner view** (in the hosting tab): held posts, reports, mod log, mass delete, switches (pause, threads closed, trips-only, approved trips, pre-moderation), efforts, the automatic panic mode, backup. A notice when the posts cap or the thread budget is hit.
- In direct mode the board views load the Tor build lazily, like channel tabs (F.3.3).

## G.13 Several devices (ties to D.11)

Revision 1 relied on the identity-wide vault lease, which is taken silently once expired. Closing a laptop for 15 minutes and opening the app on a phone would have made the phone the board host, then iOS would suspend it (B-M5); two writers could fork the numbers (B-M6). Revision 2:

1. **Vault v2, and old apps stop writing first.** Board entries need vault `v = 2`: `{kind: board, index, title, root, next_no, seq, updated, mirrors, lease: {dev, until}}`. Today `Vault::decode` refuses `v ≠ 1` and `Vault::encode` writes only the v1 fields (`crates/channel/src/vault.rs:119-139`), so an old app would see "no vault" and publish a fresh v1 over a v2 one. **Before boards ship, the app learns to never overwrite a vault it cannot decode** (it goes read-only for the vault and says "update the app on this device"). Service-worker updates wait for user consent (§17.1), so mixed versions are normal.
2. **A lease per board**, separate from the channels' lease, so moving channels never moves boards.
3. **A per-device "can host boards" flag**: off on phones, in the PWA and in Safari (E8); on in desktop Chrome and Firefox.
4. **No silent board takeover.** A host-capable device may take a board lease automatically only if it has expired by **≥ 2 renewal periods** (the routing service caches answers by URL for ~5 min, A-m7); otherwise it offers "Host this board here", an explicit action.
5. **Fencing.** Before each publish, at most every 30 s, the writer resolves the board record from its mirrors and its own onion's peers; on a `sequence` above its own signed by the board key from another device, it stops serving at once and says why.
6. **Numbers never collide:** `next_no = max(record.next_no from every source, vault.next_no + cap_per_min × minutes(lease.until − vault.updated))`. The bound is the **end of the old lease** (the old writer could not post after it), not "now" (B2: "now" jumped ~172 800 after one night offline).
7. **Sequence:** time-based (G.5.3), so the new writer is never below a reader's high-water mark.
8. **Moderation state** comes from the `own` block of the latest root (G.5.1).
9. Continuing without history (D.11.3 step 4) means an empty catalog with numbers continuing; old threads come back if a mirror or the other device serves them.

## G.14 Security and privacy

### G.14.1 Threats

| Threat | Mitigation | What remains |
|---|---|---|
| Finding a poster's IP | Tor only; no poster data in clear | Tor's limits (§28.8) |
| Linking a poster's posts | Per-post keys (IDs off); trips only on request; **a fresh isolation group per submit** (no shared circuit); `ts` rounded to the minute | Writing style; exact timing of arrivals at the host |
| Timing correlation | Optional random send delay (later) | A global observer still wins against Tor |
| **Finding the owner** | Onion-only host; separate HKDF keys; **a dedicated hosting tab and Tor client** (no shared Snowflake churn with the chat onion); `ts` rounded; v1.5: the writer behind restricted discovery, so only mirrors can make it send traffic | **Snowflake proxies are unvetted volunteers who see the host's IP**, and anyone can make the v1 host send traffic by posting: a proxy operator who also posts can look for the matching bursts. Owners who need more should use bridges they trust (F.2). Uptime and moderation times are public |
| Malicious host | Cannot forge posts under existing keys or move a post (`s.b`, `s.t` signed) | Can drop, delay, reorder, delete and invent anonymous posts; can show different states to different readers (no gossip detects it) |
| Malicious mirror, Sybil copy, rollback | CIDs, record, poster signatures, high-water marks; **72 h validity**; the **deletion list** enforced against older roots; `sequence` > now + 1 h refused | Withholding; serving the newest state up to 72 h late |
| Malicious poster: oversized, malformed, no PoW, slow | Bounded accept loop; header-first PoW; minimum transfer rate; strict, fuzzed decoders; `panic = "abort"` never reachable from input | — |
| Malicious image (v2) | Decoded only in our memory-safe Rust decoder in a terminable Worker on the host; readers' "show images" off | A browser decoder bug for readers who turned images on |
| Host exhaustion | Bounded accept loop, slots, deadlines, RAM and byte budgets, effort-ordered intake, thread budget, panic modes | Introduction floods (G.8) until v1.5 or the Tor PoW patches |
| Board key compromise | — | No key rotation yet: a signed **successor statement** ("this board moved to key K′") is a later design item (A-M11) |
| Illegal content | G.9.2 | Copies made before deletion |

### G.14.2 Memory copies (as §11.6)

| Path | Step | Copy? | Justification |
|---|---|---|---|
| Submit RX | arti `DataStream` → preallocated submit slot | **1 copy** | Unavoidable: the stream yields into a buffer we own (as for chats, F.3.2) |
| Submit RX | header, replay, PoW, `h`, signature, CBOR | 0 | Views over the slot; the borrowing decoder (G.6.2 step 6) |
| Host store | text of accepted posts → block bytes | **1 copy** | The block is new bytes (dag-cbor with `no`, `ts`); ≤ 2.4 KiB per post, into the arena |
| Host store | block → OPFS file (Worker, `SyncAccessHandle`) | **2 copies** (as built, BD-3) | wasm memory → a JS `Uint8Array` (`BoardApp.delta`), then *transferred* (not copied) to the store Worker, which writes it; each block once, when it is new (`Board::build_into` skips unchanged full chunks and archived threads) |
| Serve | held block → response | **1 copy** (as built, BD-3) | Blocks are served from wasm memory (the store is for restarts); a response is head + body built once (`gateway::Served`); the index is built once per version and shared (`Rc`) |
| Serve | refusal, answer, `/pow` | 0 | Written into a 256-byte stack buffer (`gateway::SHORT`) |
| Reopen | OPFS → JS → wasm | **2 copies** | Setup path: the store Worker reads each file, wasm copies it in (`BoardApp.open`) |
| v2 poster TX | canvas RGBA → encoder Worker's memory | **1 copy** | Setup path, a documented exception (§22); the Worker is terminated after the post |
| v2 host | image bytes → decoder Worker; pixels → thumbnail | **1 copy** each | Isolation is the point: the decoder runs in a separate, killable instance |

## G.15 Decisions

**The owner, 2026-09-29 (revision 1):**

| # | Question | Decision |
|---|---|---|
| B1 | Images | Supported, **off by default** (revision 2: from v2, pre-moderated, decoded in our wasm) |
| B2 | Posting while the owner is offline | **Pauses** in v1; mirrors as the front door in v1.5 (BD-8, R4) |
| B3 | No-JS posting from Tor Browser | **No** |
| B4 | Boards per identity, default size | **4 boards**; **10 pages × 15 threads** (150 live threads), bump limit 300, text archive kept |
| B5 | Poster IDs | Owner flag, **off by default** |
| B6 | Proof-of-work cost | **~10 s median for a reply on a phone** (revision 2: measured with 4 workers, solved while typing, p95 shown) |
| B7 | Moderation in v1 | **Owner only** (revision 2 adds mass delete, pre-moderation, trips-only and the panic modes to v1; janitors, filters and reports stay in BD-5b) |
| B8 | Tripcodes | **Yes, signed trip keys** (shown with 16 characters) |

**The owner, 2026-10-03 (the four points the reviewers still disagreed on):**

| # | Question | Reviewer A | Reviewer B | Decision |
|---|---|---|---|---|
| R1 | Record validity | 48 h | 72 h | **72 h**, with a labelled stale mode (a board does not vanish after a weekend offline) |
| R2 | G.18 | Keep the DoS rows | Cut it all | **Keep the DoS rows and the configuration, drop the scale claims** (G.18 rewritten) |
| R3 | Per-circuit limits in v1 | Keep | Cut | **Keep**: they come free with the bounded accept loop (G.6.2) |
| R4 | Restricted-discovery writer | The BD-8 design | v1.5 after a spike | **v1.5, after spike B-P12** |

**The owner, 2026-10-03 (before implementation starts):**

| # | Question | Decision |
|---|---|---|
| R5 | Where boards appear | **Inside the existing tabs**: Following and My channels, marked ▦ (4 tabs stay on phones) |
| R6 | Where the owner's tab hosts a board | **In the main app tab** (not a dedicated hosting tab). Consequence, stated in the UI and in G.9.3: the chat onion and the board onion share one Tor client, so they go up and down together and an observer probing both can link them (A-M5). A separate identity for the board does not remove this; hosting from another browser profile does |
| R7 | Live spikes vs building | **Build now; the live spikes (phone PoW timing B-P1b, 8–24 h hosting soak B-P11) run in parallel** on the owner's devices; efforts are calibrated from their numbers before v1 ships |
| R8 | Automatic panic mode | **Close new threads** (replies keep working) when the thread budget or the posts cap stays saturated; the owner is notified and reopens |


## G.16 Versions, phases and spikes

### G.16.1 Order

| Version | Contents | Effort (one engineer, this repo's pace) |
|---|---|---|
| **Step 0, gating** | Spikes B-P1b, B-P6, B-P11; **BD-0** (the bounded accept loop and the vault write guard) | ~1.5–2 weeks |
| **v1: text boards** | BD-1 → BD-2 → BD-3 → BD-5 → BD-6 → BD-7 | ~8–11 weeks |
| **v1.5** | B-P12 → BD-8 (mirrors as the front door, restricted-discovery writer, writer onion rotation) | ~3–4 weeks |
| **v2** | B-P4 → BD-4 (images); BD-5b (janitors, filters, reports); BD-12 (helpers, off by default, text only); BD-10 (discovery crawl); BD-14 (event log, only if B-P6/B-P9 show snapshots are the limit) | later |
| Parked | B-P8/BD-11 (DHT rendezvous), B-P9 swarm scale, B-P10 | only if usage justifies it |

**Riskiest phase: BD-3**, the first time a tab accepts uploads from strangers: a new store (one OPFS file per block with GC), Workers (the app has none today), and the first "done" that depends on real-network behaviour the lab cannot show (B-P11).

### G.16.2 Spikes

| ID | Question | Status / done when |
|---|---|---|
| B-P1 | `equix` in wasm32: solve and verify times | ✅ Chromium (2026-09-29, `checks/spikes/RESULTS-B-P1-B-P2.md`): interpreted hashx, **88.6 ms per attempt, 2.16 solutions per attempt → 24.4 solutions/s per core**; verify ≈ 0.1 ms; 4 Workers 3.7×. A native JIT does 903 solutions/s on 4 cores: **9.2× per core** (37× was 4 native cores against one wasm core) |
| **B-P1b** | The same on a mid-range Android, an iPhone, Firefox and Safari, **with 4 workers**; the p95; k-of-n sub-puzzles (k = 4) to narrow the p95; Wake Lock behaviour | ◐ **iPhone measured** (2026-10-03, iOS 18.7, Safari 26.5.2, 4 cores): 45.4 solutions/s on one core, 96.8 with 4 Workers (2.13×). Effort 350: reply median 2.5 s, p95 10.8 s; thread (2 800) median 20 s, p95 87 s. A 10 s median reply on this iPhone is effort ≈ 1 400. Container, headless Chromium: 15.3 per core, 64.7 with 4 Workers. **Default base effort set to 700** (thread ×8 = 5 600): ~10 s median on a phone half as fast as this iPhone; on the iPhone a reply is 5 s median (p95 22 s), a thread 40 s (p95 2.9 min). Still to measure: a mid-range Android and Firefox (page: `https://darkcite.github.io/ephem/checks/spikes/equix_bench/phone.html`); the defaults move if the Android is far from half the iPhone |
| B-P2 | Tor onion-service PoW in our arti | ✅ answered: in arti 0.46.0 behind `hs-pow-full`; compiles for wasm32 but uses `spawn_blocking` + `reenter_block_on` (panics in our runtime) and solves on `std::thread::spawn`. Needs patches to vendored `tor-hsservice` and `tor-hsclient` |
| B-P3 | Which circuit did a stream come on? | ✅ **answered from the code, no spike** (both reviewers): `RendRequest::accept()` yields a per-circuit `Stream<StreamRequest>` (`vendor/tor-hsservice/src/req.rs:209`); only the helper flattens it. BD-0 uses it |
| B-P4 | Pixel readback and the pure-Rust encoder/decoder (v2): iPhone memory, noise correlation under Brave/Firefox RFP/Safari | A 12 MP photo re-encoded under 2 s on an iPhone; noise correlation measured (G.7) |
| B-P5 | A 512 KiB upload to an onion from a tab | ✅ Lab: 512 KiB in 357 ms. `RELAY=1` (local Snowflake broker and proxies relaying to the Tor Project bridge, **not volunteer proxies**, n = 5 per size, so "p95" is the max of 5): stream open ~0.5 s, 512 KiB in 1.6 s, ≈ 320 KiB/s. **An upper bound, not a per-tab capacity.** ⏳ `LIVE=1` with real volunteers |
| **B-P6** | Host throughput: posts/s verified, built, signed and written; OPFS with thousands of block files (Worker `SyncAccessHandle` vs main-thread `createWritable`); startup time with 1 200+ files | ≥ 10 posts/s sustained in CPU, or the caps lowered to what is measured |
| B-P7 | Tor Browser Safest: thumbnails through `img-src 'self'` (v2) | Shown, or the plain page stays text-only |
| B-P9 | Swarm reads (parked) | Only with LIVE Snowflake, the 30 s thread refresh and real rendezvous costs (G.18) |
| B-P10 | `rate_limit_at_intro` under an introduction flood | Folded into BD-0's done-when |
| **B-P11** | **An 8–24 h LIVE hosting soak** from desktop Chrome: share of time the onion is reachable from a second network, proxy switches, memory | **Gates BD-3.** Reachable ≥ 90 % of 24 h, or v1 says plainly what owners should expect |
| **B-P12** | Restricted discovery in our wasm arti: a service authorised for 2 client keys; a client with the key in the ephemeral keystore connects, one without cannot find the intro points | Works in the lab and live, or BD-8 keeps a public writer onion with the accept loop only |

### G.16.3 Phases

| ID | Scope | Done when |
|---|---|---|
| **BD-0** | `crates/tor`: the bounded accept loop (G.6.2: K rendezvous, per-circuit rules, fixed stream ring, `rate_limit_at_intro`, `max_concurrent_streams_per_circuit`), used by chat, channel and board onions. App: never overwrite a vault it cannot decode | Lab: a flood of 100 introductions/s leaves the tab responsive and memory flat; legitimate reads succeed via mirrors; unit test: an unknown vault version is never rewritten |
| BD-1 | `crates/board` model: blocks (G.5.1), buckets, pin indexes incl. `arch_threads`, `dels`, `own`, `ev` reserved, limits, strict decode, chunking, tombstones, pruning with protection, the reader rules, time-based sequence and the future-sequence check; fuzzed decoders | Native tests: 150 threads × 500 replies built and verified; chunk CIDs stable across unrelated replies; GC keeps exactly the pin-reachable set (archive included); a deleted post refused from an older root |
| BD-2 | Submit format (G.6.1), sans-IO pipeline (G.6.2), Equi-X bound to `k`/`n`, epochs, grace, replay and idempotency rings, effort-ordered intake, thread budget, panic modes | Native tests for every refusal row; fuzzed parser (10⁶ runs); no allocation on the refusal path (counting allocator); a raised effort does not void a solved post |
| BD-3 | Board gateway (GET buckets/threads/chunks, `/pow`, `/submit`, no whole-board CAR), per-block OPFS store with GC (Worker), PoW Workers (the equix wasm compiled on the main thread with `fetch(…, {integrity})` and the `WebAssembly.Module` posted to a same-origin worker the SW caches; `worker-src 'self'`), the dedicated hosting tab | Lab E2E: owner + 2 posters, a thread with replies, sage, bump order, a pruned thread, a retried submit answered idempotently; **and B-P11 passed** |
| BD-5 | Owner moderation: deletes with undo, mass delete, lock, sticky, prune, trip bans, capcode, mod log, pause, threads closed, trips-only, approved trips, pre-moderation, panic modes, `own` block | Lab E2E: the owner mass-deletes; readers see tombstones; a stale mirror's old root no longer shows the deleted post |
| BD-6 | Reader UI (catalog from buckets, thread, reply box solving while typing, drafts, confirmation), cached catalog, Following (`k: board`), `#B=` links, the plain page, text-only board mirrors (pull ≤ 1/10 s, deletion list, unexpired re-`PUT`), "see also" links shown on the board page | Lab E2E: owner offline → read from a mirror, posting says `E_BOARD_OFFLINE` and keeps the draft; plain page through a C Tor client; stale mode after expiry |
| BD-7 | Several devices (G.13): vault v2, per-board lease, host-capable flag, no silent takeover, fencing, `next_no` bound, `own` restore | Lab E2E: take over on B explicitly; numbers continue; bans survive; a phone never becomes host; two writers → one stops |
| BD-8 (v1.5) | Mirrors as the front door; restricted-discovery writer (after B-P12); writer onion rotation sent to mirrors encrypted, never in the public manifest | Lab E2E: posts via a mirror; the writer unreachable without a mirror key; a rotation under a flood |
| BD-4 (v2) | Images (G.7) | Lab E2E: EXIF/GPS stripped; a COM segment refused; images-off readers fetch no thumbnail bytes |
| BD-5b (v2) | Janitors (actions with epoch and nonce), word filters, reports | Lab E2E: a janitor deletes; a replayed action is refused |
| BD-9/10, BD-12, BD-14 (v2) | Discovery card and crawl (G.17), helpers (G.18), event log | Separate designs when their time comes |

## G.17 Optional public discovery (v2; the owner's request of 2026-09-30)

**Goal.** An owner may make a board, or one thread, findable by people who do not have its link, with **no directory, index or link built into the app** (P10). Off by default.

**v1 part (BD-6):** the manifest's signed **"see also" list** (up to 16 other boards the owner vouches for) is shown on the board page as plain links. That is discovery without hardcoded links at a fraction of the cost (B-UX-5). The rest of this section is v2.

### G.17.1 What an owner publishes

- A **discovery card** in the signed manifest: `disc: {tags: [≤ 4 short words], blurb: ≤ 140 B, nsfw: bool, since}`; per discoverable thread `{no, title}` in a separate `disc_threads` block (not the catalog buckets, whose size it would push up, B-m2).
- The **"see also" list** (above).
- Served at `/disc` (a few hundred bytes).

### G.17.2 How a reader discovers (the **Discover** view, Following tab)

1. **Seeds**: the boards and channels the reader already follows, plus links received from contacts. An empty follow list gives an empty Discover view: "Follow a board to see the boards it recommends".
2. **Crawl**: over Tor, `/disc` of each seed, then of their "see also" boards, breadth first, depth ≤ 3, ≤ 100 boards, ≤ 4 fetches in flight, only while visible, at most hourly; **from mirrors first** (tab-hosted boards are often offline: 100 boards at a 6 s p95 first connect, plus dead onions, B-UX-5); each card verified.
3. **Show**: cards grouped by tag, "found via A → B", `nsfw` hidden unless turned on.
4. **Cache**: a day, on the device only.

### G.17.3 Optional: public rendezvous on the IPFS network (parked, spike B-P8)

A board announces itself as a provider of `CID(raw, sha256("ephem-disc-v1:" ‖ tag ‖ week))` through the routing API from a Tor exit; readers ask for providers and keep only those that prove a signed `/disc` card. Unknowns: whether `delegated-ipfs.dev` accepts provider announcements, and how easily a topic is flooded. Only if B-P8 shows it works, behind its own switch.

### G.17.4 Abuse and safety

| Risk | Answer |
|---|---|
| Illegal or abusive boards spread through "see also" | Each hop is a signed choice of an owner the reader already trusts; the path is shown; a reader can **block** a board, which also stops the crawl through it; `nsfw` off by default |
| Spam boards pointing at each other | Depth ≤ 3, ≤ 100 boards; boards reached by several independent paths first |
| Shared blocklists | A board may publish a signed block list; readers may subscribe to those of boards they follow; no global list |
| Linking an owner's boards | "See also" is public by design; the UI says so |
| Readers' privacy | Everything over Tor; only `/disc` fetches of boards already in the graph |

## G.18 Scale and denial of service

**Revision 1 claimed 100 000 readers served by ~12 helper tabs. Both reviewers showed that was wrong by 5–12×** (G.19). This section now keeps the denial-of-service defences (R2) and states only what is supported.

### G.18.1 What a board can serve (honest)

| Quantity | Revision 1 | Corrected (A-B1, A-B2, with revision 2's buckets) |
|---|---|---|
| Bytes per catalog refresh | 20 KiB | the changed buckets, ~5–40 KiB (one 45–128 KiB block in revision 1) |
| Open-thread refresh | every 10 min | **every 30 s** while on screen (G.11.1); a thread block plus last chunk up to ~170 KiB |
| 100 000 readers (10 % active) | 3.4 MB/s, 12 helpers | **≈ 17–29 MB/s with revision 1's catalog; still ≈ 10–20 MB/s with buckets; 50–150 helper tabs**, plus ~5 rendezvous/s per helper that nobody has measured |
| Writer to 8 mirrors at 1 post/s | not counted | **~550 KiB/s with revision 1** (above the ~320 KiB/s best case); ≈ 25–40 KiB per publish × 8 mirrors pulling ≤ 1/10 s in revision 2 |

**What v1 claims:** **"low thousands of Tor readers per board"**, from the writer and a few mirrors, until a LIVE measurement (B-P9) shows more. Larger audiences need a path that does not exist yet: an optional pinning-service push (V-4: a CAR diff through a Tor exit every few minutes) feeding `trustless-gateway.link`, so gateway scale is someone else's CDN. Tier-2 reader helpers (v2, BD-12) are **off by default and text only** (A-M6: helpers on by default would make every reader re-serve content it never displayed, on an onion exposed to the correlation attack of G.14.1).

### G.18.2 Writes

- One writer verifies, orders and signs everything; B-P6 measures the CPU ceiling (target ≥ 10 posts/s). The wire is the real limit: publishing coalesces to ≤ 1/s, or ≤ 1/5 s under load (G.5.3); mirrors pull ≤ 1/10 s; in v1.5 the writer pushes to 1–2 mirrors that relay to the rest.
- **Beyond one writer:** split the board (topics, languages); one board stays one writer.
- If B-P6 shows snapshots are the limit, the v2 **event log** (`ev` in the root, reserved in v1): signed, hash-chained segments of events (~0.5 KiB per post) plus a catalog checkpoint every ~5 min; readers derive the catalog. Per-change cost drops ~100×.

### G.18.3 Denial of service, attack by attack

| Attack | Defence | Status |
|---|---|---|
| **Introduction flood** on the board onion | The bounded accept loop (≤ K rendezvous, reject the rest) and a moderate `rate_limit_at_intro` (G.6.2); v1.5: the writer behind **restricted discovery**, so outsiders cannot find its introduction points, with rotation sent to mirrors encrypted; Tor's own onion-service PoW once B-P2's patches land | Loop and config: BD-0. Restricted discovery: B-P12/BD-8. Tor PoW: needs patches |
| Introduction flood on a mirror | Same loop (K = 16); readers try several mirrors and drop a flooded one | BD-0 |
| Stream or request flood through one circuit | ≤ 4 concurrent streams and ≤ 1 submit per 10 s per circuit, else the circuit is shut down; `max_concurrent_streams_per_circuit = 8` backstop | BD-0 (R3) |
| **Slow-loris** on submit slots | 10 s deadline, ≥ 16 KiB/s after 2 s, header checked first | BD-2/BD-3 |
| **Zero-work uploads** | PoW verified from the header before any body byte; the solution enters the replay set at that point | BD-2 |
| **Post flood** | PoW (speed bump), effort-ordered intake, posts cap, per-thread duplicates | BD-2 |
| **Thread-pruning flood** | Thread budget, prune protection, automatic "new threads closed" | BD-2 |
| Read flood (bandwidth) | Mirrors; response caps per request; v2 helpers with upload caps | BD-6 |
| Poisoning, rollback, freeze | CIDs, signatures, high-water marks, 72 h validity, deletion list, future-sequence refusal | BD-1 |
| Whole-board CAR requests (memory) | `format=car` refused for root and pin indexes | BD-3 |
| Fake helpers (Sybil, v2) | Readers verify everything; a source serving junk or stale-beyond-others data is dropped; ≥ 1 signed mirror always among the sources | BD-12 |
| Snowflake exhaustion | Outside our control; own bridge lines (F.2) | Exists |

### G.18.4 What this cannot promise

- A board whose writer is offline takes no posts (B2; v1.5 queues them at mirrors).
- A determined introduction flood can stop a v1 board from taking posts; readers continue from mirrors.
- A rented server can always out-solve phones; the board then runs on budgets, switches and moderation.
- The host's IP is protected from its Snowflake proxy by Tor's design only as far as that proxy is honest (G.14.1).

## G.19 Review findings (2026-10-03)

Two reviewers read revision 1 and the code it builds on, then each checked the other's report against the code. **A** = security, protocol, abuse and scale; **B** = feasibility, implementation, UX and operations. Severity: **blocker**, **major**, minor. Every finding below is answered in revision 2 except where "open" is stated.

### G.19.1 Blockers

| ID | Finding | Evidence | Resolution |
|---|---|---|---|
| B-B1 | Archived threads were referenced, not pinned: the host's GC deleted its own archive, and mirrors and Kubo lost it | G.5.1 refs are not followed; `Hosted::dag` follows only tag-42 links (`crates/channel/src/gateway.rs:59-75`) | `arch_threads` pin index; GC rule stated (G.5.1, G.5.3) |
| B-B2 / A-M11 | After a takeover the new writer published a lower IPNS `sequence` (the vault lags by up to ~600 publishes), so every reader refused it as a rollback; `next_no` bound by "now" over-jumped | `channel.rs:404-410`; G.13 rev 1 | `sequence = max(last + 1, unix_ms)`; `next_no` bounded by the old lease's end; future-sequence refusal (G.5.3, G.13) |
| B-B3 / A-M7 | The owner's tab decoded every attacker image in the browser's native decoder, the renderer that holds the owner's IP | G.6.2 step 8, G.7 rev 1 | Images moved to v2; host decodes with `zune-jpeg` in a terminable Worker; the native decoder never sees an unmoderated image (G.7) |
| A-B1 | G.18 capacity inputs were wrong: 17–29 MB/s and 56–150 helpers, not 3.4 MB/s and 12; rendezvous per second, not bytes, is the unmeasured limit; tier 3 does not exist for a live board | G.11.1 30 s refresh; 45–128 KiB catalog; D.7.2 is a manual import | Scale claims removed; "low thousands" until B-P9 LIVE; pinning push named as the large-audience path (G.18.1) |
| A-B2 | Write amplification: 8 mirrors pulling ~68 KiB at 1 Hz exceeded the writer's link at 1 post/s | G.5.3, G.18.1 rev 1 | Catalog buckets; mirrors pull ≤ 1/10 s; publish ≤ 1/5 s under load; event log reserved for v2 (G.5, G.10, G.18.2) |
| A-B3 | PoW economics: a 4-core laptop pruned all 150 threads in ~4 min; one VM core saturated the cap and adaptive effort then punished phones | B-P1 numbers; G.8 rev 1 | Thread budget, prune protection, effort-ordered intake, automatic panic modes; PoW described as a speed bump (G.8) |

### G.19.2 Major

| ID | Finding | Resolution |
|---|---|---|
| A-M1 / B-M12 | "The writer onion is not public" contradicted posters and Tor Browser using it; BD-8 was both "later" and "promoted"; mirrors *can* check PoW (`/pow` serves the seed) | v1: public onion with the accept loop; v1.5: restricted-discovery writer, mirrors as the front door (R4, G.10) |
| A-M2 | PoW was checked after reading a 520 KiB body; the replay set was filled late, so one valid header could be reused; 8 slots × 30 s were open to slow-loris | Header carries `k`, `n`, `h`; PoW and replay first; ≥ 16 KiB/s; 10 s deadline (G.6) |
| A-M3 / B-M4 | No bound below HTTP: unbounded rendezvous accept (`helpers.rs:15`) and stream queue (`mod.rs:171-174`) | BD-0 accept loop in `crates/tor` (G.6.2) |
| A-M4 | 30-day validity let Sybil copies serve pre-deletion snapshots for a month; freeze attacks | 72 h validity (R1), stale mode, deletion list, mirrors re-`PUT` only unexpired records (G.5.3, G.9.2) |
| A-M5 | Owner deanonymisation: Snowflake proxies see the IP, anyone can make the host send bursts; one shared Tor client links chat and board onions; `ts` leaks clock skew | Dedicated hosting tab and Tor client; `ts` rounded; warnings; restricted discovery in v1.5 (G.3, G.9.3, G.14.1). **Open:** traffic padding between writer and mirrors (BD-8) |
| A-M6 | Helpers on by default re-served thumbnails readers never saw; battery and metered checks don't exist outside Chromium | Helpers v2, off by default, text only (G.18.1) |
| A-M8 | Anti-fingerprinting noise correlates across images from one session | B-P4 measures it; G.7 |
| A-M9 | Circuit reuse let the host link one poster's posts | A fresh isolation group per submit (G.6.1) |
| A-M10 | Janitor actions replayable; one signing prefix for every kind | Per-kind prefixes; epoch and nonce in actions (G.4, G.9.1) |
| B-M1 | One VPS fills the cap; phone threads ~80 s median before any attack | Measured phone calibration with p95 (B-P1b); trips-only and approved-trips switches; pre-moderation in v1 |
| B-M2 | A raised effort voided finished work | Effort grace (G.8) |
| B-M3 | Content-bound PoW forced the wait after Post | Bound to `k` and `n`, solved while typing (G.8) |
| B-M5 | A phone could silently become the board host | Per-board lease, host-capable flag, no silent takeover (G.13) |
| B-M6 | Split-brain writers; bans lost on takeover; old apps re-encode the vault without board entries | Fencing; `own` block; vault v2 with the write guard shipped first (G.13, BD-0) |
| B-M7 | "Images off" still downloaded thumbnails | Thumbnails as view refs with a separate media index (G.7) |
| B-M8 | Pre-moderation promised for images but scheduled after them | Images require pre-moderation; pre-moderation in v1 (BD-5) |
| B-M9 | No mass tools; "ban" a no-op with IDs off | Mass delete; ban shown only for trips and IDs-on (G.9.1) |
| B-M10 | One `format=car` request for the root built the whole board in RAM | Refused for root and pin indexes; streamed export (G.5.3) |
| B-M11 | Board text could exceed the RAM maximum; "no allocation" contradicted itself | Per-tab RAM budget, prune by RAM bytes; the rule scoped to the refusal path (G.5.2, G.6.2) |
| B-M13 | Hosting availability over live Snowflake unmeasured | B-P11 gates BD-3 (G.16) |

### G.19.3 Minor

| ID | Finding | Resolution |
|---|---|---|
| A-m1 / B-m1, m12, m13 | Inconsistent defaults (100 vs 150 threads), `/pow` body size, section order | Fixed: 150; `/pow` 58 bytes specified; sections renumbered (G.16 before G.17) |
| A-m2 | 10-character trips grindable; trips remove deniability | 16 characters; a warning (G.4) |
| A-m3 | 64 MiB response before any CID check | Per-request caps (G.11.1) |
| A-m4 | DHT lifetime 13–86 h; mirror re-`PUT` unspecified | 24 h republish; mirrors re-`PUT` unexpired records (G.5.3) |
| A-m5 / B-m8 | Workers cannot use SRI | Main-thread `fetch(…, {integrity})` → `WebAssembly.Module` → worker; `worker-src 'self'` (BD-3) |
| A-m6 | RAM per identity | Per-tab cap (G.5.2) |
| A-m7 | Lease split-brain through cached routing answers | Expired by ≥ 2 periods before an automatic takeover; fencing (G.13) |
| B-m2 | `ex` in characters could overflow the catalog | 140 bytes; discovery titles in their own block |
| B-m3 | `#b=` taken by bridges | `#B=` (G.11.1) |
| B-m4 | Follow entries lost `k` | Kept; unknown kinds say "update the app" |
| B-m5 | OP deletion unspecified | Prunes the thread |
| B-m6 | Reader rules implicit | Listed (G.5.1) |
| B-m7 | Encoder memory never freed in the main instance | Terminable Workers (G.7) |
| B-m9 | Retries after a dropped stream were refused | Idempotent resubmit (G.6.1) |
| B-m10 | One `media` block for every image | Per-thread media index (G.7) |
| B-m11 | Vault update cadence stated two ways | Irrelevant with the time-based sequence |
| B-m14 | Keep-alive not worth it | Cut from v1 |
| B-m15 | Host-asserted fields not listed | Listed (G.4) |

### G.19.4 Conflicts the reviewers settled together

| Question | Settled |
|---|---|
| The PoW attacker's edge (A: ~9×, B: 37×) | Both right in different units: **9.2× per core** against desktop wasm, 37× for 4 native cores against one wasm core, **~18–37× per core against a phone**. The doc uses per core |
| Is B-P3 needed? (B: no; A: the accept path is unbounded) | One fix: our own accept loop gives both the bound and the per-circuit identity; no spike (G.6.2) |
| Is a PoW bound to the post key safe? | Yes, with a signed per-post nonce, `k` and `n` in the header, the replay insert at the header check, and the epoch seed in the challenge (G.8) |
| v1 scope | B's text-only cut, plus the cheap-now, costly-later items from A: header-first PoW, thread budget and panic modes, short validity, a dedicated hosting tab, fresh isolation per submit, per-kind signatures, response caps, and the reserved `ev` field |

### G.19.5 What both reviewers said to keep

The single writer and the rejection of multi-writer; reuse of the verified IPFS stack; 64-post chunks with stable CIDs; tombstones that keep numbers; refuse-before-reading with a fixed header and stable error codes; Equi-X with keyed epoch seeds; the honest-limit sections and the "what a signature proves" box; images off by default with an engine-independent encoder; read-only Tor Browser pages with a strict CSP; separate HKDF keys and onions per board; owner-only moderation in v1 with a public mod log.

### G.19.6 Corrections to the reviews

- **Catalog split by page** (A-B2 fix c, B's round 2) would not keep a bump to ≤ 2 blocks: a bump moves a thread to page 1 and shifts every page above its old position. Revision 2 uses 10 buckets by `no mod 10`, sorted by readers (G.5.1).
