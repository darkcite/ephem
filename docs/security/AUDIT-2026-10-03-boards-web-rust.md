<!-- SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0 -->
<!-- Copyright 2026 Anton (darkcite) -->
# Security audit 2026-10-03: boards in the browser (Rust side), vault v2, trip keys, PoW ABI, default brokers

## Scope and method

**Audited at** `145d931` (main). Commit `ff06b25` (online probe, `ChannelApp::probe`) landed while this audit was running. It does not touch the code in scope, but it shifts line numbers in `crates/channel-web/src/lib.rs`, so the `lib.rs` references below are to `ff06b25`.

**Read in full:**
- `crates/channel-web/src/boards.rs`, and in `crates/channel-web/src/lib.rs` the parts `publish_vault`, `vault_fetch`, `vault_json`, `forget_identity` and `with_timeout`;
- `crates/channel/src/vault.rs`, `crates/crypto/src/identity.rs`, `crates/pow/src/lib.rs`, `crates/tor/src/bridge.rs` (with the diff of `145d931`);
- the code these call: `crates/board/src/{gateway,host,verify,pow,lib}.rs`, `board.rs` (`load`, `record`), `pipeline.rs` (`PowInfo`, `submission`), `own.rs` (seal/open), `crates/channel/src/{ipns,gateway,car}.rs` (parsers), `crates/tor/src/web/{mod,carrier}.rs` (accept limits, broker use);
- the JS callers in `app/boards.js` (`takeOver`, `continueHere`, `renew`, `mirrorSeed`) and `app/channels.js` (`vaultApi`).

**Design read:** `docs/BOARDS.md` G.3, G.4, G.6.2, G.10, G.11, G.13 (with its "as built" note), G.14, G.18, and `docs/P2P-CHAT.md` §D.11 and Appendix F.2.

**Ran:** scratch clone `$SCRATCH/aud2`, with new proof tests that are not in the repo:
- `crates/board/tests/audit_bw.rs`, 4 tests. They replay, with real served bytes, exactly what `pull`, `take_over` and `Pulled::into_mirror` accept.
- `crates/tor/tests/audit_bw_bridge.rs`, 1 test.

All five pass: each asserts the vulnerable behaviour. The baseline `cargo test --release -p ephem-board -p ephem-channel -p ephem-crypto -p ephem-pow` is green.

**Not run:**
- the wasm build and the Tor lab (`checks/tor-lab`). Wasm-only paths (serve loop timing, vault publishing) are proved by precise traces instead;
- `cargo audit` and `cargo deny` (out of this focused scope).

## Summary

| ID | Severity | Area | Title | Status | Proof |
|---|---|---|---|---|---|
| BW-1 | **high** | host / mirror serve loop | 16 slots held for 20 s each by two circuits sending no PoW (slow-loris): the board takes no posts and serves no reads. The code is weaker than G.6.2 and G.18.3 | open | confirmed by trace |
| BW-2 | **medium** | takeover (G.13) | `take_over` adopts the first source that verifies, whether stale or with thread and `own` blocks left out. Deleted threads come back, threads vanish, bans and switches reset, and the result is republished with a winning sequence | open | confirmed (native test) |
| BW-3 | **medium** | vault v2 | A 404 from the routing service sets `vault_known = true` even after `E_VAULT_NEWER` or a remembered floor, so this app overwrites a newer-format vault, or publishes one with an empty channel lease and list | open | confirmed by trace |
| BW-4 | low | mirrors | `pull` accepts thread CARs that lack the thread, and the mirror serves holes. One stale or hostile source stops a pull round (`Ok(None) => break`). `mirror()` adopts the first source, not the newest | open | confirmed (native test) + trace |
| BW-5 | low | takeover / reopen | `Board::load` clones the whole block set once per archived thread, so cost is O(archive × bytes). Junk in CARs inflates it (+64 MiB → 3 s natively). A pull has no total byte budget | open | confirmed (measured) |
| BW-6 | low | bridges (F.2) | A custom Snowflake line without `url=` adds the Tor Project brokers to one flat broker list. A private bridge's fingerprint and the client's SDP then go to the Tor Project broker whenever the private broker fails | open | confirmed (native test) + trace |
| BW-7 | low | fencing | `fence_loop` checks its sources one after another, each with a 90 s timeout. One listed mirror that accepts and stalls delays every fencing round. G.13.5 says "at most every 30 s" | open | confirmed by trace |
| BW-8 | low | identity switch | Switching identity while a `take_over` is in flight hosts the previous identity's board in the new identity's state. `serve` then puts it on the new identity's board onion, and the new identity's vault lists it | open | confirmed by trace |
| BW-9 | info | identity switch | `forget_identity` leaves board mirrors and `known_dels` in place (M-1 took channel mirrors down) | open | trace |

Counts: 1 high, 2 medium, 5 low, 1 info.

## Findings

### BW-1 (high): slot exhaustion of a board's host or mirror with no proof of work

**Description.** Every board onion, the owner's and each mirror's, serves streams from a pool of `SLOTS = 16` buffers (`crates/channel-web/src/boards.rs:43`). When the pool is empty, a new stream is closed at once (`boards.rs:1198-1201`). A slot is held for as long as these allow:

- **Request head:** `READ_MS = 10 s` for the whole head (`boards.rs:1218-1233`), with no rate floor.
- **`/submit` header:** a *further* `READ_MS` for the 188 header bytes (`boards.rs:1307`), before any proof of work is checked.
- **Body:** another `READ_MS` (`boards.rs:1320`). This needs a valid PoW header.
- **Response write:** up to `SERVE_MS = 60 s` for the whole request (`boards.rs:50`, `:1205`). The response is written with `write_all` (`boards.rs:1276`), so a reader that never drains a large CAR or index keeps the slot for 60 s.

The Tor layer allows `CIRCUIT_STREAMS = 8` new streams per circuit per 10 s and `MAX_CIRCUIT_STREAMS = 8` open (`crates/tor/src/web/mod.rs:229-232`), and up to `LIVE_CIRCUITS = 32` circuits. Nothing limits slots per circuit.

The design promises a 10 s *total* deadline and **≥ 16 KiB/s after the first 2 s** (`docs/BOARDS.md:204`, `:531`), plus ≤ 4 concurrent streams and ≤ 1 submit per 10 s per circuit (`:530`). None of these is implemented. The code is therefore worse than the document, and this is not the accepted "introduction flood" of G.18.4.

**Trigger (trace).**
1. The attacker opens 2 rendezvous circuits to the board onion and 8 streams on each, at 8 per circuit per 10 s, inside `CIRCUIT_STREAMS`.
2. On each stream it sends `POST /submit HTTP/1.1\r\nContent-Type: application/vnd.ephem.board-submit\r\nContent-Length: 300\r\n\r\n`. `gateway::route` gives `Route::Submit(300)`.
3. It then sends one header byte every ~1 s. `read_to` (`boards.rs:1289-1298`) keeps the slot until the 10 s timeout gives 408. Trickling the head first adds up to another 10 s.
4. Each slot is busy about 20 s. The 16 streams are renewed every ~20 s, that is 8 new streams per circuit per 20 s, under the limit.
5. All 16 slots stay taken for as long as the attacker wants. Every honest stream is dropped at `boards.rs:1199`: `/pow`, `/submit`, index reads, mirror pulls, and the other device's fencing reads.

The same works against each mirror (same `serve_loop`), so two circuits per onion take the whole board offline. No PoW is computed and no post is made. The cost is about 1.6 streams per second per onion.

**Impact.** Remote denial of service of a whole board, posting and reading, at negligible cost. A side effect: fencing reads from the owner's other device fail too (BW-7).

**Fix proposal.**
- Give each slot one absolute deadline from accept (`READ_MS` for head + header + body together, not per phase) and a rate floor. In both read loops, after 2 s, refuse with 408 when `have * 1000 / elapsed_ms < 16 * 1024` (G.6.2). This needs no allocation: two `u64`s per slot.
- Bound the response write by a drain rate (bytes written / elapsed) rather than only by `SERVE_MS`.
- Expose the rendezvous circuit's identity on `DataStream` (the accept loop already counts streams per circuit). Allow ≤ 2 slots per circuit, and ≤ 1 `/submit` per circuit per 10 s, as G.18.3 states. Bring `CIRCUIT_STREAMS`/`MAX_CIRCUIT_STREAMS` down to the documented 4 for board onions.
- Keep separate pools (for example 4 submit slots and 12 read slots), so that read slow-loris cannot stop posting and the reverse.

### BW-2 (medium): a takeover adopts a stale or incomplete board and republishes it as the newest

**Description.** `BoardApp::take_over` (`boards.rs:478-521`) tries its sources one after another: its own onion, then the mirrors from the vault entry (`app/boards.js:238`). It hosts the **first** version that `pull` returns (`boards.rs:499-506`).

**What is not checked:**
- **The version.** There is no floor: the vault entry's `seq` and `root` are known to the page but not passed. There is no comparison across sources, unlike `read`, which takes the newest with `NEWEST_GRACE_MS`.
- **Completeness.** `verify::read` treats a missing thread block or chunk as "catalog only" (`crates/board/src/verify.rs:180-189`). `Board::load` keeps only `v.threads`, the threads whose blocks were present (`crates/board/src/board.rs:534-538`).
- **The `own` block.** It is fetched best-effort (`boards.rs:1100-1107`). Without it, `Host::new` starts from `Own::default()`: no bans, no approved or known trips, no held posts, default switches and efforts (`crates/board/src/host.rs:85`).

Because records are time-based (`board.rs:523`, `seq = max(last+1, now_ms)`), the new host's first record beats every earlier one. Honest mirrors follow it (`into_mirror` keeps only what the new root reaches), and readers accept it. The damage becomes the board's newest state everywhere.

**When it happens.** An automatic takeover starts only once the other device's lease is long expired (`app/boards.js:214-220`), which is exactly when the own onion does not answer. The listed mirrors then decide, and mirrors are third-party volunteers (G.10). A listed mirror can:
- (a) serve any older version whose record is still valid (72 h);
- (b) serve the newest index but thread CARs that leave out the thread blocks, and not serve the `own` block.

Case (b) also happens without malice: a mirror whose best-effort `own` fetch failed holds no `own` block.

**Proof** (scratch `crates/board/tests/audit_bw.rs`, both pass):
- `bw_takeover_from_a_stale_source_resurrects_deleted_threads_and_drops_switches`:
  1. The owner deletes thread 1 and turns on pre-moderation and pause, then publishes.
  2. A takeover 1 h later reads the version from before, which is still valid.
  3. The new host's catalog is `[1, 2, 3]`: the deleted thread is back. `premod` and `paused` are off, and its sequence is above the newest record.
- `bw_takeover_from_a_source_that_omits_blocks_drops_threads_and_moderation_state`: from the newest index, with junk thread CARs and no `own` block, the new host publishes an empty catalog with `trips_only` and `premod` reset.

**Impact.** Integrity of the board, limited to takeovers:
- moderation is rolled back: deleted content, possibly illegal (G.9.2), is republished, and bans and switches are lifted;
- threads are silently lost for everyone.

G.13.6 ("max from every source") and G.13.8 ("the own block of the latest root") are not met.

**Fix proposal.**
- Pass the vault entry's `seq` (and `root`) into `take_over` and refuse any version below it. If a version with exactly that `root` arrives, prefer it.
- Read all sources in parallel as `read` does and keep the newest. `next_no` becomes the maximum over the sources.
- Require completeness before hosting: every catalog thread present with all its chunks (`view.threads.len() == view.catalog.len()`), every archived thread reachable, and the `own` block present when the root links one. If that fails, try the next source. When none is complete, let the page offer "continue without …" explicitly, as `continue_board` does today.

### BW-3 (medium): a 404 from the routing service lets this app publish over a vault it could not read

**Description.** In `vault_fetch`, a 404 (or a 200 whose body says "not found") sets `vault_known = true` unconditionally (`crates/channel-web/src/lib.rs:484-490`). That holds even when this tab already knows a newer vault exists:
- after `E_VAULT_NEWER`, which sets `vault_seq = max(seq)` and `vault_known = false` (`lib.rs:497-503`);
- after `vault_floor` (`lib.rs:520-529`).

The publish guard (`publish_vault`, `if (st.vault_seq > 0 || !channels) && !st.vault_known`) then passes. A board renewal (`channels = false`) copies the channel lease and entries from `st.vault`. In a tab that never decoded a vault, that is `Vault::default()`: device zeros, `until` 0, no channels. It is published at `vault_seq + 1`, above the real record.

**Trigger (trace).**
1. A device runs this app (v2) and the identity's other device runs a newer app (v3). `vault_fetch` returns `E_VAULT_NEWER(N)`.
2. Board renewal (`app/boards.js:304-317`) calls `vaultApi.fetch()` then `publishBoards`. On a later renewal the routing service answers 404. Delegated DHT lookups miss transiently, and the operator of `delegated-ipfs.dev` can also cause it.
3. `fetch()` resolves `""`, then `vault_publish` writes a v2 vault at `N + 1` with the old v2 contents. In a fresh tab those contents are the empty default.

The same happens on a device that knows only the floor `N` from an earlier visit. The channel path (`channels = true`) likewise drops every channel entry this tab has not read.

**Impact.**
- G.13.1 ("never overwrite a vault it cannot decode") and D.11.3 step 5 ("never publishes over a vault it could not read") are broken.
- The other device's channel entries, its lease and its v3 data are replaced. The other device then sees its lease gone, and may stand down or have to re-take.
- Channels and boards are not lost: their keys are derived. But the list, leases, `next_no` floors (BW-2) and `record_seq` floors are.

**Fix proposal.**
- On 404, set `vault_known = true` only when `st.vault_seq == 0`. Otherwise return `Err("the vault (version N) is not found right now; not overwriting it")`.
- Keep a sticky `vault_newer: Option<u64>`, set on `Newer` and cleared only by decoding a vault whose sequence is ≥ it. Refuse every publish while it is set.

### BW-4 (low): mirrors accept incomplete thread CARs, and one source can stall a mirror

**Description.**
- **Holes.** `pull` appends whatever valid-CID blocks a thread CAR holds (`boards.rs:1093-1097`) and verifies with `verify::verify`, which accepts absent thread blocks or chunks. `into_mirror` then builds `Served::new` over this. The mirror's catalog lists the threads, but `/ipfs/<thread>?format=car` answers 404.
- **Holes that persist.** Later pulls skip any thread whose root CID is already held (`boards.rs:1091-1092`), so a missing **chunk** of an unchanged thread is never fetched again.
- **One source can stall the round.** In `pull_loop` (`boards.rs:1143-1155`), `Ok(None)`, which covers a "not newer" *or forged* record (`boards.rs:1086-1089`, every `BoardError::Record`), `break`s out of the source list. When the owner is offline, the first listed mirror that answers with an old record stops the round, and newer mirrors after it are never asked.
- **First, not newest.** `mirror()` adopts the first source that verifies, not the newest (`boards.rs:573-588`).

**Proof.** `bw_a_mirror_accepts_thread_cars_without_the_thread_and_serves_holes` (scratch, passes): with junk thread CARs, the mirror is built, lists 3 threads, and answers 404 for each. The rest is confirmed by trace at the lines given.

**Impact.** Availability and staleness of honest mirrors, caused by another listed mirror. This goes beyond G.14's "withholding" by a malicious mirror *itself*: here it degrades honest mirrors.

**Fix proposal.**
- After `car::read`, require `roots == [c]` and that the thread is complete. A small `verify::thread_complete(name, &blocks, c)` would do, or check that `verify` returned a `ThreadView` for that catalog entry.
- In `pull_loop`, continue to the next source on `Ok(None)` and `Err`, and stop only on `Ok(Some)`.
- In `mirror()`, read the sources in parallel and keep the newest.

### BW-5 (low): `Board::load` is O(archived threads × all bytes); pulls have no byte budget

**Description.** `Board::load` calls `reachable(&a.thread, archived_blocks.clone())` once per archive entry (`crates/board/src/board.rs:540-544`), cloning every block, including unreached junk, up to 256 times.

`take_over` hands it everything `pull` fetched. A pull fetches every catalog and archive thread, each up to `MAX_CAR = 1.5 MiB`, with any extra valid-CID junk the source adds. That is up to (150 + 256) × 1.5 MiB within the 180 s timeout, with no total budget. `open()` pays the same cost on every reopen of a board with a large archive.

**Proof.** `bw_board_load_clones_every_block_once_per_archived_thread` (scratch, `--release`), 200 archived threads:
- 17.6 ms with the board's own blocks;
- 2.98 s with 64 MiB of unreached junk.

Wasm on the main thread is slower still.

**Impact.** The hosting tab freezes, with a doubled memory peak, during a takeover fed by a hostile mirror, and on reopening large boards. Bounded; no crash shown.

**Fix proposal.**
- Build one `HashMap<Cid, Vec<u8>>` once and walk each archived thread from it, moving the blocks out. That is O(total).
- In `pull`, keep only the blocks reachable from the requested CID right after each `car::read`.
- Add a total byte budget per pull, sized to the honest maximum of the limits.

### BW-6 (low): a line without `url=` routes private-bridge offers to the Tor Project broker

**Description.** Since `145d931`, `parse_line` pushes `DEFAULT_BROKERS` for a line without `url=` (`crates/tor/src/bridge.rs:218-238`). The result is still one flat `brokers` list and one flat `fingerprints` list. `negotiate` sends each fingerprint's offer, with the SDP and gathered candidates (including the srflx public IP) plus `"fingerprint"`, through `post_offer`. `post_offer` tries the brokers **in order** (`crates/tor/src/web/carrier.rs:245-266`, `:290-306`).

So when a user mixes a self-hosted Snowflake stack (F.2.1) with one line copied from bridges.torproject.org, every offer for the private bridge goes to the Tor Project broker as soon as the private broker is unreachable. A private line that merely forgot `url=` is accepted and pointed at the Tor Project broker with only a note (`Problem::DefaultBroker` is not an error).

F.2.2 says the fallback to the default Snowflake is **off** by default because "someone who chose bridges may not want to touch the default broker" (`docs/P2P-CHAT.md:1947`).

**Proof.** Scratch `crates/tor/tests/audit_bw_bridge.rs` (passes):
- `brokers == [my-broker, snowflake-broker.torproject.net, cdn77]` with both fingerprints in one list;
- a private line without `url=` gets `DEFAULT_BROKERS`.

The order of `post_offer` is shown by trace.

**Impact.** A semi-secret private bridge (F.2.2 "Storage") and the user's IP in the SDP are disclosed to the Tor Project broker against the user's documented choice. Low: the Tor Project broker is the default trust anchor anyway, and a note is shown.

**Fix proposal.**
- Keep brokers per line. Either make `SnowflakeParams` a list of `(brokers, fingerprints)` groups, or keep a map from fingerprint to its line's brokers, and offer a fingerprint only to its own line's brokers.
- Alternatively, apply `DEFAULT_BROKERS` only when **no** line has `url=`, and show `DefaultBroker` in the custom-bridges UI as a warning to confirm, not a passive note.

### BW-7 (low): fencing can be delayed far beyond 30 s by one listed mirror

**Description.** `fence_loop` (`boards.rs:1036-1071`) checks the manifest's mirrors, then the own onion, **one after another**. Each check runs `with_timeout(FETCH_MS = 90 s, …)` on a fresh circuit. A listed mirror that accepts the stream and never answers costs 90 s per round. Eight such mirrors cost 12 min, before the own onion is even read. Offline mirrors also cost a Tor connection failure each.

G.13.5 promises "at most every 30 s". In practice, the page's lease renewal (`app/boards.js:304-317`, every lease/3) is the effective fence, and that only holds for a device that did not take over within the last lease.

**Impact.** A longer split-brain window after a takeover, with two writers publishing.

**Fix proposal.** Fence with all sources in parallel (as `read` does), a short timeout per source (for example 20 s), and the own onion included. Stop on the first newer record.

### BW-8 (low): an identity switch during `take_over` leaks the old identity's board into the new one

**Description.** `take_over` copies the board seeds at call time. After its awaits (up to 4 rounds × sources × 180 s), it calls `start_host(&st, index, …)` (`boards.rs:505`) without checking that the tab's identity is still the same.

If the user switches identity meanwhile (`forget_identity`, `lib.rs:110-118`):
1. The previous identity's board is pushed into the new state's `boards.hosted`.
2. The page then calls `serve(i)` (`app/boards.js:242`). `serve` launches the board with the **new** identity's `board_seeds(index)[1]` (`boards.rs:211-217`), so the old identity's board is served on the new identity's board onion.
3. `publishBoards` writes it into the **new** identity's vault (`Boards::vault_entries`).

The same pattern applies to `mirror()` (`boards.rs:589-599`).

**Impact.** It links two identities: a board name of A is served at an onion derived from B, and B's vault lists A's board. M-1 intended that nothing of the previous identity stays online. It needs user action during a slow operation.

**Fix proposal.** Give `State` an identity generation counter, bumped in `forget_identity`. Capture it in `take_over` and `mirror` before the first await, and abort after the await if it changed.

### BW-9 (info): `forget_identity` keeps board mirrors and read history

`forget_identity` (`lib.rs:110-118`) clears `self.mirrors` (channel mirrors) and `boards.hosted`, but not `boards.mirrors` nor `boards.known_dels`. Board mirror seeds are per browser (`app/boards.js:678-686`), so the impact is small. It is still inconsistent with M-1: the same mirror onion stays up across the switch, beside the new identity's onions (A-M5 already links onions in a tab).

**Fix:** clear both, or document that board mirrors and deletion memory are per browser.

## Attack-surface inventory (boards, browser side)

| Untrusted input | Enters at | Parser / first allocation | Bound |
|---|---|---|---|
| Stream bytes to the host and mirror onions | `serve_one` | in place in the 8 KiB + `MAX_SUBMIT` slot; `gateway::route` | 16 slots, `READ_MS`/`SERVE_MS` (BW-1) |
| `/submit` header and body | `submit_one` → `Host::submit_header`/`submit_body` | the fixed 188 bytes, then the body slice | `end ≤ SLOT` (checked: `len ≤ MAX_SUBMIT`, `body_at ≤ MAX_HEAD`) |
| Index, record, CAR and raw responses from owner onions and mirrors | `fetch` | `Vec` capped at `cap + MAX_HEAD`; `parse_index`, `car::read`, `ipns::verify`, `verify::*` | per request only (BW-5) |
| The `own` block from a source | `pull` | `own.verifies(b)`, then `Own::open` (AEAD) | `OWN + 1024` |
| `/pow` answer from any board onion | `draft` | `PowInfo::read` on exactly 58 bytes | 1024 |
| Submit answer | `submit` | `parse_answer` / `parse_any_response` | 1024 |
| Vault record from the routing service | `vault_fetch` | `ipns::verify` (V2 data only), AEAD, `Vault::decode` v1/v2/newer | 10 KiB record; ≤ 16 channels, ≤ 4 boards |
| Bridge lines (paste, key file, `#b=`) | `bridge::parse` | slices of the text | existing fuzz target |
| Store blocks and record at reopen | `open` | `cid.verifies`, `ipns::verify(…, 0)`, `verify::read` | the page's store |
| PoW exchange buffer | Worker → `solve` | fixed 256-byte static; `name_len ≤ 48` checked | static |

## What is solid

These are controls this audit tried to break and could not:

- **No panic reachable from network input.** The slot index arithmetic is in bounds (`end ≤ MAX_HEAD + MAX_SUBMIT`, `have ≤ MAX_HEAD`). The `expect`s (`"188 bytes"`, `"a short response fits"`, the owner's own `Served::new`/`update`) are not input-dependent. `complete`, `parse_response`, `parse_index`, `car::read` and `PowInfo::read` are bounds-checked. `page::catalog` clamps `?p=`.
- **No RefCell borrow is held across an await** in `boards.rs`. Every `borrow_mut` is a statement temporary, `drop(h)` comes before every await in `submit_one`, and listeners are called only after all borrows end (publish and fence loops). No re-entrancy panic was found.
- **Fencing cannot be triggered without the board key.** `ipns::verify` trusts only the signed V2 `data` (sequence included), and `rec.sequence > mine` is read after the fetch (fix `42aab9a`).
- **No foreign or unreached blocks get adopted.** `verify::read` keeps only blocks that hash to their CID. `Served::new` keeps only what the signed root reaches. The `own` CID comes from the signed root. Archive blocks pass through `reachable` per thread. An attacker can withhold blocks (BW-2, BW-4) but cannot inject them.
- **Readers.** `read` races all sources, keeps the newest valid sequence, and applies `known_dels` to older roots. `min_seq` and the future-sequence refusal hold.
- **Key derivation.**
  - `board_seeds` and `trip_seed` have distinct HKDF info prefixes that do not collide with `channel/`, `vault-*` or `onion-ed25519` (no label is a prefix of another at a variable boundary).
  - Board names in `trip_seed` are canonical CID text, which has no `/`.
  - Trip keys are unlinkable across boards and labels, and to the chat identity, without the seed.
- **The PoW wasm ABI.** One static, single-threaded Worker; the layout is checked at compile time (`SOLUTION + 32 ≤ 256`); `name_len` is checked against `MAX_NAME`; there is no pointer from JS other than `buf()`.
- **Vault v2 decoding.** Version 1 opens, newer versions give `Newer(seq)`, `MAX_BOARDS` and the board index are checked, AEAD is bound to the sequence, and padding buckets hold. A replayed older vault is ignored once a newer one has been read.
- **JSON for the page.** `view_json`, `held` and `vault_json` escape all strings (including U+2028/2029); the remaining fields are integers and booleans.

## Recommended next audits

1. The fixes for BW-1 and BW-2, in the Tor lab: a slow-loris client against host and mirror, and a takeover from a stale mirror.
2. A fuzz target for `verify::verify` + `Board::load` + `Host::new` with mutated block sets (adding withholding to the existing `fuzz_verify`).
3. `app/boards.js`: the takeover and lease logic and its interaction with BW-3. The web side (XSS in board views, `#B=` links) was outside this scope.
4. The `own` block nonce (`ms ‖ seq`, deterministic, one key shared by all devices): consider a random 24-byte nonce. No collision was shown here.
