<!-- SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0 -->
<!-- Copyright 2026 Anton (darkcite) -->
# Security audit 2026-10-03: Tor, Snowflake, channels, vault

## Status after the fixes

| ID | Severity | Status (2026-10-03) |
|---|---|---|
| H-1 | high | **Fixed**: CBOR arrays/maps reserve at most 16 items up front; CAR blocks over 1 MiB are refused. A 1 MiB bomb now peaks at ~53 MiB (values actually parsed) instead of 491–839 MiB. Test: `crates/channel/tests/cbor_bomb.rs` |
| H-2 | high | **Fixed**: `Tor::launch` uses a bounded accept loop (8 rendezvous being built, 32 live circuits, 8 new streams per circuit per 10 s, 8 open per circuit, a 32-stream queue, `rate_limit_at_intro` 10/s burst 50); the channel gateway serves at most 16 requests at once with a 60 s deadline including the write; the root CAR is built once per version and shared; a chat stream must send its first frame within 30 s. Tor's own onion PoW stays unavailable (B-P2) |
| M-1 – M-5, M-7 | medium | Open |
| M-6 | medium | **Fixed**: the HTTP parser moved to `gateway::parse_any_response`, bounds-checked, CRLF required; test `any_response_never_panics` |
| L-1, L-2 | low | Open |
| I-2 | info | **Fixed**: `Cid::read` uses `checked_add` |
| I-1, I-3 | info | Open |


## Scope and method

**Slice audited** (two other auditors cover the rest of the app): `crates/tor` (`web/mod.rs`,
`web/carrier.rs`, `web/rt.rs`, `bridge.rs`, `net.rs`, `stream.rs`, `tls.rs`, `config.rs`, onion
hosting, stream isolation), `crates/snowflake` (encap, KCP, smux, session), `crates/channel`
(cbor, cid, car, ipns, channel, gateway, page, vault, time, varint), `crates/channel-web`
(`lib.rs`: owner, serve loop, reader, mirrors, vault fetch/publish; `https.rs`; `json.rs`), and
`app/channels.js` where it handles records, blocks, mirrors and leases. Where the onion-hosting
path continues into `crates/wasm/src/tor.rs` (chat onion accept) it is cross-referenced only.

**Read first:** docs/P2P-CHAT.md §21, §28, §29, Appendix C, Appendix D (with D.11), Appendix F.2,
`vendor/README.md` and the vendored `tor-hsservice` config defaults.

**Ran** (scratch crate outside the repo, `…/scratchpad/audit-tor/poc`, release build, counting
allocator):

| Run | Result |
|---|---|
| `cargo test -p ephem-channel -p ephem-snowflake` | all pass (incl. Go-server interop) |
| `FUZZ_ITERS=2000000 cargo test --release -p ephem-snowflake --test fuzz` | clean (all four targets) |
| `FUZZ_ITERS=1000000 cargo test --release -p ephem-tor --no-default-features --test fuzz_bridge` | clean |
| Own mutation fuzzer over `cbor::Value::decode`, `car::read`, `ipns::verify`, `vault::open`, `gateway::respond/parse_response/complete`, `Cid::parse`, `parse_rfc3339`, `Channel::load` (1.5 M iterations) | no panic |
| Proofs `f1`, `f1b`, `f2`, `f3`, `f4`, `f9` (below) | each confirms its finding |

**Could not run:** `cargo audit` / `cargo deny` (not installed); the browser build and the
offline Tor lab (`checks/tor-lab/`), so memory exhaustion of a real tab was not driven end to
end: H-2 is shown by the per-request cost and a code trace, not by crashing a tab. No live
network was touched.

## Summary table

| ID | Severity | Area | Title | Status |
|---|---|---|---|---|
| H-1 | **high** | channel / cbor | DAG-CBOR decoder pre-allocates by declared length: 1 MiB block → 839 MiB; a crafted channel link crashes any reader's tab | confirmed (f1, f1b) |
| H-2 | **high** | tor / channel-web | Onion services have no admission limits: unbounded accept queue and tasks, no write timeout, a full CAR built per request | confirmed by trace + f3; live exhaustion unconfirmed |
| M-1 | medium | channel-web / vault | Switching identity in a tab keeps the old identity's onions, mirrors and vault; the new identity's vault absorbs the old channel list and can later chain a new channel onto the old one | confirmed by trace |
| M-2 | medium | vault / channels.js | Restore and takeover accept an older version of one's own channel; posts are not preceded by a re-read; the vault's high-water mark is not kept | confirmed by trace |
| M-3 | medium | channel-web reader | Readers take the first valid answer, not the newest: a stale or malicious mirror holds readers on an old version for up to 30 days | confirmed by trace |
| M-4 | medium | channel-web mirrors | Blocks a CAR carries beyond the channel are kept and re-served (mirror and owner onions), and a mirror's root is the CAR header's | confirmed (f2) |
| M-5 | medium | tor isolation | All HTTPS-through-exit requests share one isolation group: vault and channel publishes are linkable at the routing service | confirmed by trace (arti source) |
| M-6 | medium | channel-web / https | Chunked-body parser indexes past the buffer: panic, and `panic = "abort"` kills the tab | confirmed (f4) |
| M-7 | medium | design | Chat onion and channel onions hosted by one tab share uptime: contacts can correlate them | unconfirmed (no measurement) |
| L-1 | low | channels.js / reader | Onion hints and signed mirror entries are not checked to be `.onion`; a follower's address list grows without bound | confirmed by trace |
| L-2 | low | channel / cid | Base36 decoding is quadratic: a 100 KB name blocks 3.5 s (native) | confirmed (f9) |
| I-1 | info | tor / bridge | `broker()` accepts `http://localhost:1@other.host/` and loopback `http` in production | confirmed by trace (unexploitable: CSP, fetch) |
| I-2 | info | channel / cid | `Cid::read` adds `start + len` unchecked (debug-build panic) | confirmed by trace |
| I-3 | info | tor / config | Lab-only `TorNet::new` interpolates unvalidated fingerprints into TOML | confirmed by trace (lab API only) |

## Findings

### H-1 (high): DAG-CBOR decoding amplifies memory ~840×; one crafted block crashes a tab

**Description.** `Reader::value` allocates `Vec::with_capacity(n)` for arrays and maps, where
`n` is the declared length, bounded only by "bytes left in the block" (`len`). A `Value` is 32 B
and a map entry 56 B, and the bound is applied independently at every nesting level (up to
`MAX_DEPTH = 16`). A block of 15 nested maps, each claiming as many entries as bytes remain,
holds all fifteen reservations at once before decoding fails.

**Evidence.** `crates/channel/src/cbor.rs:211-215` (`len`: `n <= remaining`), `:235` and
`:243` (`with_capacity(n)`), `:163` (depth 16). Blocks are decoded on verified-CID paths:
`channel.rs:397` (`read_dag`, root and manifest), `:359` (pages), `gateway.rs:266`
(`Hosted::dag`, any block held), `ipns.rs:151`, `vault.rs:250`.

**Reproduction** (`poc/src/tests.rs`):
- `f1_cbor_decode_amplification`: a 1 048 576-byte block → peak **839 MiB** allocated (×839).
- `f1b_reader_of_a_crafted_channel`: anyone creates a key, signs an IPNS record whose value is
  the bomb's CID, ships the bomb in the CAR; `channel::verify` peaks at **839 MiB**.
- `f2` (M-4): the same bomb inside a mirrored CAR; one 140-byte `GET /ipfs/<bomb>?format=car`
  to the mirror → 839 MiB.

**Impact.** wasm32 memory is capped at 4 GiB (much less on iOS); a 3–5 MiB block exceeds it and
the allocation failure aborts the module. In Tor mode the channels share the chat page's wasm
instance (`App::bind_channels`), so the crash also drops every chat, the chat onion and every
hosted channel. Attack paths: (a) publish a channel and share its link: every reader who opens
it (and every follower at each 10-minute refresh) crashes; (b) put the bomb in a CAR served to a
follower who mirrors (M-4), then request it from the mirror's onion: anyone can crash the mirror
at will, repeatedly, without user action.

**Fix.** Never trust a declared length for allocation: `Vec::with_capacity(n.min(16))` and let
`push` grow (growth is then bounded by items actually parsed). Add a per-block size cap where
blocks enter (`car::read`, `Hosted`): channel blocks are ≤ ~280 KiB (64 posts × 4 KiB + sigs),
so `MAX_BLOCK = 512 KiB` fails fast; and optionally an item budget in `Reader` (total values ≤
bytes). Add the bomb as a regression test in `cbor.rs`.

### H-2 (high): onion services accept unbounded work

**Description.** For every onion the tab hosts (chat, each channel, each mirror):

1. `Tor::launch` accepts every rendezvous stream and pushes it into an unbounded
   `VecDeque` (`crates/tor/src/web/mod.rs:168-177`).
2. The service config is arti's default (`:159-162`): `max_concurrent_streams_per_circuit =
   65535`, no `rate_limit_at_intro`, `enable_pow = false`
   (`vendor/tor-hsservice/src/config.rs:56,65-70`).
3. `serve_loop` spawns one task per stream with no cap (`crates/channel-web/src/lib.rs:789-799`).
4. `serve_one` bounds only the request-head read (30 s); the response is built in full
   (`gateway::respond`, `lib.rs:824`) and then `write_all` has **no timeout** (`:826-828`). A
   client that never reads stalls the write once Tor's stream window fills, so the response
   buffer lives as long as the stream.
5. `GET /ipfs/<root>?format=car` rebuilds the whole channel CAR per request
   (`gateway.rs:343-349`: `dag()` clones every block, `car::write` copies again).
6. Chat onion (cross-reference, `crates/wasm/src/tor.rs:444-460`): each incoming stream task
   pre-allocates `RX_CAP` (≈ 20 KiB) and waits for its first frame with no timeout.

**Reproduction.** `f3_car_per_request_amplification`: a 1 000-post channel (2 KiB posts); one
140-byte request → 2 062 KiB response, 6 188 KiB peak per request. With item 4, 1 000 parked
streams on a single rendezvous circuit hold ~2 GiB. Live exhaustion was not driven (no lab).

**Impact.** Channel onion addresses are public by design (links, the plain page, mirror lists),
and the chat onion is known to contacts and invite holders. Anyone can exhaust the owner's or
mirror's tab memory at a cost of a few hundred bytes per stream; the tab aborts (see H-1 for what
that takes down). This is a remote DoS of hosting, chats included.

**Fix** (bounded buffers, fail fast):
- `OnionServiceConfigBuilder::max_concurrent_streams_per_circuit(8)` (a reader needs 2; a chat
  needs 1) and `enable_pow(true)` where the vendored build supports it.
- A bounded accept queue (e.g. 32; drop the stream beyond it) and a per-service cap on live
  `serve_one` tasks (an `Rc<Cell<u16>>` counter; refuse at 16).
- One deadline around the whole of `serve_one`, write included (`with_timeout` over head read +
  write, e.g. 60 s).
- Cache the root CAR once per version in `Hosted` (it is already rebuilt only on change for the
  page) and serve `&[u8]` slices; do not clone blocks per request.
- Chat side: a first-frame timeout and a lazily grown buffer in `incoming`.

### M-1 (medium): an identity switch leaks the previous identity's channels into the new one

**Description.** `ChannelApp::bind` (identity change in Tor mode) and `ChannelApp::sign_in`
(the "separate identity for channels" flow) clear only `st.own`. They keep:
- `st.served`: the previous identity's channel onions stay online (`lib.rs:115-131`, `:155-165`);
- `st.mirrors`: the previous identity's mirror onions stay online;
- `st.vault` and `st.vault_seq` of the previous identity.

The page does not call `release()` on these paths (`app/channels.js:67-80`, `:654-666`; it
only resets its own `vault` variable).

Then, for the new identity B:
- `vault_fetch` replaces the vault only if `seq >= st.vault_seq` (`lib.rs:467`), a comparison
  across two identities' sequences; if B has none yet it returns early and keeps A's;
- `vault_publish` merges `st.vault.entries` (A's channels: titles, about, mirrors, head CIDs,
  counts) into B's vault and seals it under B's key with A's sequence + 1 (`lib.rs:489-498`);
- on B's next sign-in elsewhere, `syncVault` lists those entries, `restore` finds no host for
  B's channel at that index and calls `resume` (`lib.rs:527-536`, `channels.js:871-890`): B's
  channel `index` is created with **A's title, about, mirrors and `base.head` = A's head-page
  CID**, and served publicly on B's channel onion.

**Impact.** D.3 tells users to use a separate identity so a channel cannot be tied to their
chat identity. After a switch in one tab: (1) A's and B's onions are co-hosted (shared uptime);
(2) B's vault, and anyone holding B's key file, learns A's channel list; (3) in the resume path
B's public channel chains onto A's page CIDs and copies A's title, which links the two
identities' channels publicly. Requires the user to switch identity in a tab, a normal flow.

**Fix.** Treat an identity change as a full reset in Rust: in `bind` (when `!same`) and
`sign_in`, clear `own`, `served`, `mirrors` (or keep mirrors only if the follow list is
identity-independent), `vault = Vault::default()`, `vault_seq = 0`. Add a unit test that
`vault_publish` after `sign_in(B)` contains only B's entries.

### M-2 (medium): restore and takeover can roll an owner back to an older version

**Description.**
- `restore(e, fresh)` reads its own channel with `min_seq = local`, which is `0` when the device
  holds no copy (`channels.js:876`), not the sequence the vault records (`Entry::record_seq`,
  which `vault_json` does not even expose, `lib.rs:665-690`).
- `ChannelApp::read` races the channel onion and all mirrors and keeps the **first** valid answer
  (`lib.rs:577-583`, M-3). Mirrors refresh every 10 min, so a stale mirror is normal, and a
  malicious one is easy (any follower the owner lists).
- `ch.open` then adopts that version and its sequence (`channels.js:879-881`, `lib.rs:242-263`).
- Posting does not re-read the latest record (`change`, `channels.js:604-615`), although
  D.11.3 step 5 says "Before every post the writer re-reads the channel's latest record".
- The vault's own high-water mark is in memory only (`State::vault_seq`, `lib.rs:82`): every
  page load accepts any validly signed vault record (valid 30 days), although D.11.5 says an old
  one "is refused by sequence once a newer one was seen on this device".

**Impact.** The owner's new device, or a takeover, continues from an older version: posts and
deletions made after it are dropped from the chain for good, and the next records carry lower
sequences than followers' high-water marks, so followers see nothing new until the sequence
catches up. Integrity loss without key compromise; also reachable with no attacker.

**Fix.** Pass the vault's `record_seq` as the minimum in `restore` (expose it in `vault_json`);
in `read`, collect all answers of a round and keep the highest sequence (M-3); before `post`/
`delete`, fetch the channel's own record from its onion and refuse to write below it; persist
the highest vault sequence per vault name (localStorage is enough: it only raises a floor).

### M-3 (medium): readers keep the first valid version, not the newest

**Description.** `select_ok` returns the first source that delivers a valid state with
`sequence >= min_seq` (`crates/channel-web/src/lib.rs:577-583`). Nothing compares answers.

**Impact.** A mirror that answers faster (a stale one, or one that deliberately serves an
older record still inside its 30-day validity) holds new readers and followers on an old version
indefinitely: new posts and deletions are hidden. D.8 lists "several sources tried" as the
mitigation; the code tries them but does not prefer the newest. Followers' high-water marks
only stop going backwards, not being held back.

**Fix.** Within a round, wait for all sources (each bounded by the existing timeout) or for the
owner's own onion, and keep the highest sequence; remember the best per-source sequence to
de-prioritise sources that lag.

### M-4 (medium): extra blocks in a CAR are kept and re-served

**Description.** `car::read` returns every block whose hash matches, and `channel::verify`
ignores unreferenced ones. `ChannelApp::mirror` builds `Hosted` from **all** blocks of the
fetched CAR and takes the root from the **CAR header**, not from the verified record
(`lib.rs:613-617`). `ChannelApp::open` (used by `restore` with a CAR fetched from mirrors) does
the same for the owner's own onion when the record is fresh (`lib.rs:256-262`). The page stores
`r.car()` as received for mirrors (`channels.js` `serveMirror`). `gateway::respond` serves any
held CID (`gateway.rs:343-357`).

**Reproduction.** `f2_junk_blocks_ride_along_and_are_served_by_a_mirror`: a CAR with a valid
channel plus an unrelated raw block and a CBOR bomb, header root set to the junk block. Verify
passes; the mirror's `Hosted.root` is the junk CID; `GET /ipfs/<junk>?format=raw` returns the
third-party bytes; `GET /ipfs/<bomb>?format=car` peaks at 839 MiB (H-1).

**Impact.** A malicious source makes followers' mirror onions, and an owner's own channel onion
after a restore, host arbitrary content by CID (content the follower or owner never saw), and
plants H-1 bombs in mirrors for later remote crashes. The mirror's exported CAR (Kubo
instructions, D.7.2) is also rooted at the attacker's CID.

**Fix.** After verification, keep only the blocks reachable from the record's root
(`Hosted::new` can filter with the existing `dag()` walk), use the record's root, and re-write
the CAR (`car::write`) before storing it.

### M-5 (medium): exit traffic of vault and channels shares one circuit

**Description.** `https::request` dials with `tor.connect(name, port, false)`
(`crates/channel-web/src/https.rs:37`); `Tor::connect` adds isolation only when `fresh`
(`crates/tor/src/web/mod.rs:144-150`). arti's default `StreamIsolationPreference::None` with a
single client isolation token (`arti-client-0.46.0/src/client.rs:676-684`) lets all such
streams share an exit circuit for its lifetime (~10 min). The vault is fetched/published every
5 min and 2 s after each change; `publish_ipfs` sends a channel's name.

**Impact.** `delegated-ipfs.dev` sees `PUT /ipns/<vault>` and `PUT /ipns/<channel A>`, `<channel B>`
from the same exit IP within seconds, which groups an identity's channels and its vault. This
contradicts D.3/D4 ("nothing in a channel can be linked…") and D.11.5 ("an observer cannot group
them; the DHT and the routing API see only Tor exits"). Bounded: it needs the routing operator
(or anyone it shares logs with), and exits are shared by other users.

**Fix.** `fresh = true` (a new isolation group) for every `https::request`, and jitter the
vault publish after a change (not a fixed 2 s after the post that readers can see).

### M-6 (medium): chunked HTTP parser panics, aborting the tab

**Description.** In `https.rs::parse`, after a chunk `pos += size + 2` (`:85`) can exceed
`rest.len()`; the next iteration slices `rest[pos..]` (`:78`) and panics. The release profile
uses `panic = "abort"`.

**Reproduction.** `f4_https_chunked_panics` (verbatim copy of `parse`):
`"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n5\r\nhello"` panics.

**Impact.** A response that ends inside the CRLF after a chunk's data crashes the whole Tor
page. Sources: the routing service (TLS-authenticated), or a legitimate long chunked answer cut
by the 64 KiB read cap at that byte (`:52`), or the stream ending early. Low likelihood, total
impact.

**Fix.** `let rest_pos = rest.get(pos..).ok_or("chunk")?;`, use `checked_add` for `pos + size`,
and require the CRLF after the data (`rest.get(pos + size..pos + size + 2) == Some(b"\r\n")`).
Add the input above as a unit test (the module is wasm-gated; move `parse` into a native-tested
module of `ephem-channel` like `gateway::parse_response`).

### M-7 (medium, unconfirmed): co-hosted onions share uptime

**Description.** A Tor-mode tab hosts the chat onion and every channel (and mirror) onion side
by side (C-P2, `Tor::launch`); they appear and disappear together (tab open/closed, iOS
foreground, Safari throttling, network loss).

**Impact.** A contact (who knows the chat onion) or anyone who learned a mirror's onion can poll
the descriptors of a public channel onion and of the chat onion and correlate availability over
days, linking channel and chat identity, which D.3 says cannot be linked. D.3's caveats list
writing style and posting times, not uptime. Not measured; it is a known class of
onion-service deanonymisation.

**Fix.** Document it in D.3 and in the creation warning; offer "host my channels in a separate
tab/window without chat" (a channel-only session), which the separate-identity sign-in already
nearly provides.

### L-1 (low): onion hints and mirror entries are not validated

**Description.** `openLink` takes `o=` entries as given (`channels.js:118`); `take` unions the
manifest's `mirrors` into the follower's list for good (`:304`); manifests are checked for
format only when the owner sets them (`channel.rs:254`), not when read (`channel.rs:107-111`).
`ChannelApp::read` dials every entry at once (`lib.rs:567-581`).

**Impact.** A link or a channel owner can make readers send plain-HTTP `GET /ipns/<name>` to
clearnet hosts through Tor exits (the exit learns which channel is read); an owner rotating
mirror lists grows a follower's address list (and its key-file TLV) without bound, and each
10-minute refresh dials all of them.

**Fix.** Accept only `^[a-z2-7]{56}\.onion$` in `read`/`openLink`/manifest decoding; keep the
follower's list as link entries + the latest signed list (replace, do not union); cap it at
1 + `MAX_MIRRORS`.

### L-2 (low): quadratic base36 decoding

`cid.rs:169-187` (`unbase36`) is O(n²). `f9`: a 100 KB `k…` name takes 3.5 s natively (slower in
wasm; a 1 MB pasted link freezes the tab for minutes). Fix: reject names longer than 64
characters in `Cid::parse` before decoding (a valid name is 62).

### I-1 (info): bridge broker check

`bridge.rs:125-132`: `http://` is accepted for hosts starting with `localhost:`/`127.0.0.1:`,
and the host test includes userinfo, so `http://localhost:1@other.example/` passes. Not
exploitable today (`tor.html` CSP allows only `https:` and `fetch` rejects URLs with
credentials), but the loopback exception should be lab-only (`cfg` or a lab flag) and the
userinfo check applied to both schemes.

### I-2 (info): unchecked addition in `Cid::read`

`cid.rs:69` `src.get(start..start + len)`: overflow panics in debug builds (fuzzing with debug
assertions); release wraps to an empty `get`. Use `start.checked_add(len)?` as `car.rs` does.

### I-3 (info): lab API TOML interpolation

`TorNet::new` (`web/mod.rs:281-288`, `js-api` feature, lab page only) passes unvalidated
fingerprints into `config::build`, which interpolates them into TOML (`config.rs:451`). The app
path validates them (`bridge::fingerprint`). Validate in `config::build` itself.

## Attack-surface inventory

| Entry of untrusted bytes | First parser | Notes |
|---|---|---|
| Snowflake proxy DataChannel messages | `carrier.rs:318-331` → `encap::Decoder` → `kcp::input` → `smux::input` | fixed buffers, fuzzed; Tor TLS inside |
| Broker answer (JSON, SDP) | `carrier.rs:266-277` (`JSON.parse`, `setRemoteDescription`) | broker is trusted for IP (§21) |
| Bridge lines (pasted, `#b=`, key-file TLV 0x05) | `bridge::parse` | fuzzed; I-1 |
| Onion streams to channel/mirror onions | `serve_one` → `gateway::respond` | H-2 |
| Onion streams to the chat onion | `crates/wasm/src/tor.rs::incoming` | H-2 item 6 (other auditor's slice) |
| Responses from owner/mirror onions | `http_get` (≤ 64 MiB) → `parse_response` → `car::read` → `channel::verify` | H-1, M-3, M-4 |
| Public gateway responses (page `fetch`) | `record_root`, `verify` | H-1 |
| Routing service responses (vault, HTTPS via exit) | `https::parse` → `vault::open` | M-6; vault AEAD |
| Channel links (`#c=…&o=…`) | `openLink`, `Cid::parse` | L-1, L-2 |
| Stored CAR/record (OPFS, IndexedDB), imported backups | `ChannelApp::open` | owner's own data; same-origin only |
| Directory snapshot (IndexedDB) | `tor_dirmgr::cache_import` | re-validated by arti; same-origin only |

## What is solid

- **Snowflake stack**: encapsulation, KCP and smux use fixed-capacity buffers with explicit
  overflow errors; 2 M fuzz iterations per target found nothing; the transport guard
  (`net.rs`) refuses every address except the placeholder bridge addresses, all listeners,
  Unix sockets and UDP.
- **IPNS verification** uses only the V2 signature over `data` and requires every V1 field
  present to match it (`ipns.rs:157-163`); the key comes from the name; size capped at 10 KiB.
- **DAG-CBOR strictness**: shortest-form integers, sorted unique keys, no indefinite lengths,
  no floats, trailing bytes refused, depth ≤ 16; hashes and signatures always refer to the
  same bytes (apart from H-1's allocation pattern).
- **CAR reading** verifies every block's hash; `read_pages` guards loops ("more pages than
  posts"), rejects gaps in `seq`, and cannot be made to claim more or fewer posts than it links.
- **Signatures** are domain-separated (`ephem-channel-manifest:`, `ephem-channel-post:`,
  `ipns-signature:`); forged posts, swapped blocks and other keys fail (unit tests and fuzzing).
- **Vault crypto**: XChaCha20-Poly1305 with a random 24-byte nonce per record and the record
  sequence in the AAD (a ciphertext cannot be re-signed under another sequence); padding
  buckets; tamper tests pass.
- **Plain onion page** (`page.rs`): every owner string is escaped (`& < > " '`), the CSP is
  `default-src 'none'` with no scripts, frames or forms; `json.rs` escapes control characters
  and U+2028/2029; the page renders posts with `textContent` only.
- **HTTPS through exits** verifies certificates against the Mozilla roots (`webpki-roots`) with
  rustls; no TLS bypass outside the lab's extra root.
- **Gateway request parsing** is bounded (8 KiB head, 431), GET only, no path traversal (CIDs
  are parsed, not used as paths).

## Recommended next audits

1. After fixing H-1/H-2, a lab run that floods a channel onion with parked streams and a
   bomb-carrying mirror, watching tab memory (Chromium and iOS Safari).
2. `crates/wasm/src/tor.rs` accept path in depth (first-frame timeout, IK trial decryption cost
   per waiting chat, card-secret dials) — only skimmed here.
3. The vendored `tor-dirmgr` memory store: `cache_import` validation and unbounded growth of
   microdescriptors across a long session.
4. Multi-device races of the lease (two devices publishing the same vault sequence) once M-2's
   re-read-before-write exists; add a lab test with a deliberately stale mirror.
5. `cargo audit`/`cargo deny` over the arti 0.46 dependency tree once the tools are available.
