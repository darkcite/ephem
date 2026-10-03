<!-- SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0 -->
<!-- Copyright 2026 Anton (darkcite) -->
# Security audit 2026-10-03: boards core (`crates/board`)

## Scope and method

**Read:** all of `crates/board/src` (`board.rs`, `verify.rs`, `post.rs`, `pow.rs`, `submit.rs`,
`pipeline.rs`, `gateway.rs`, `host.rs`, `own.rs`, `page.rs`, `lib.rs`) and `crates/board/tests`,
checked against `docs/BOARDS.md` (Appendix G: G.5, G.6, G.8, G.9 and its "as built" notes, G.14,
G.18, G.19). For reachability and impact I also read the helpers the crate calls
(`ephem_channel::cbor` decoder, `page::esc`/`CSP`, `gateway::reachable`, `time::rfc3339`) and the
serve and submit loop that drives the host (`crates/channel-web/src/boards.rs:1213-1337`, and
`:759` for the effort the poster solves at). Those files were read for context only; they were
not audited.

**Ran:** every test ran in a scratch clone (`git clone` of HEAD `145d931` into the session
scratchpad, `CARGO_TARGET_DIR` also in the scratchpad), never in the repo. The existing suite
(`cargo test --release -p ephem-board`: lib, `fuzz_verify`, `host`, `model`, `pipeline`) passes.
The proofs are three scratch test files, `tests/audit.rs`, `tests/audit_perf.rs` (with a
counting global allocator) and `tests/audit_page.rs`. Their names are given below as `scratch:<test>`.
They are not part of the repo.

**Not run:** wasm32 builds and timings (the native timings below run faster than the browser);
the live Tor/Snowflake lab; `cargo audit` and `cargo deny` (not installed); `cargo fuzz`.

## Summary table

| ID | Severity | Area | Title | Status |
|---|---|---|---|---|
| BC-1 | high | moderation (`board.rs`, `host.rs`) | Posts in archived threads cannot be deleted, and a poster can force any thread into the archive within minutes | fixed
| BC-2 | high | PoW / intake (`pipeline.rs`) | One refused new thread a minute raises every effort ×64 while the attacker pays only the grace minimum; R8 then closes new threads | fixed
| BC-3 | medium | moderation / submit (`board.rs:258`, `host.rs:110`) | Anyone can resubmit a deleted post with its published `s`+`sig` (an owner capcode post included); it is published again | fixed
| BC-4 | medium | deletion list (`board.rs:355`) | The 4 096-entry count cap evicts deletions long before 30 days; a stale mirror's pre-deletion root shows the post again | fixed
| BC-5 | medium | switches (`own.rs:87-99`, `host.rs:127`) | Trips-only can be preloaded with an attacker's own "trips", which push the real regulars out of the 512-entry list | fixed (mitigated)
| BC-6 | medium | pre-moderation (`host.rs:348-356`, `own.rs:30`) | 8 posts fill the held queue; with pre-moderation on, everyone else is refused | fixed
| BC-7 | medium | host memory (G.5.2/B-M11) | The RAM budget (64 MiB/board, 128 MiB/tab) is not implemented: 936 MiB live and 1.26 GiB peak for one board | fixed (budget; copies remain)
| BC-8 | medium | publish (`board.rs:152-186`) | Each publish re-encodes and re-copies every thread's last chunk (19.4 MiB and 180 ms native for one reply) | fixed
| BC-9 | medium | `page.rs:177`, `verify.rs:278` | A board owner can abort a mirror's tab: the catalog `r` is unchecked for a thread whose chunks are withheld, and it sizes an allocation | fixed
| BC-10 | low | owner-tab CPU | Mass delete and reopen are quadratic; plain pages are rebuilt (with a record verify) on every request | fixed
| BC-11 | low | `pipeline.rs:335` | `h.epoch + 1` overflows on attacker input (panics in debug builds; wraps in release) | fixed
| BC-12 | low | `pipeline.rs:342-354` | A solution is spent when the header is checked: a stream that drops before the body, or a ticket that is evicted, makes the retry `409`, not idempotent | fixed
| BC-13 | info | `own.rs`, `host.rs:86-87,391-393` | The `own` AEAD nonce is deterministic (time ‖ sequence); no reuse found, but nothing random backs it | fixed
| BC-14 | info | `board.rs:386-401` | `set_mirrors`/`set_see_also` check only length and the `.onion` suffix, not base32 v3 onions | fixed

## Findings

### BC-1 (high): posts in archived threads cannot be deleted; a poster forces the prune

**Description.** `Board::delete` searches only live threads (`board.rs:333`:
`self.threads.iter().position(...)`), and `Host::delete` checks first with `post_of`, which also
looks only at `self.board.threads` (`host.rs:165-172,177`). Once a thread is pruned
(`board.rs:314-322`, by `make_room` or by the owner's "prune"), its full text sits in `archive`
and stays pinned in `arch_threads` for 7 days (`limits::ARCHIVE_S`). It is served as raw and as a
CAR, and mirrors pull archived threads (`channel-web/src/boards.rs:1092`). No API removes an
archived thread or a post inside one. The doc comment on `delete` (`board.rs:331`, "Deleting an
OP prunes its thread out of the archive too") and G.9.2 ("removes the blocks from the owner's
store and from every later snapshot") promise more than the code does.

**Trigger (an anonymous poster).** On a full board (150 threads), post the illegal reply with
**sage** to the least-recently-bumped unprotected thread; sage keeps its `bump`, so the thread
stays the prune victim (`board.rs:287,303-309`). Then submit one new thread (the budget allows one
every 2 min). `make_room` archives the victim, the reply with it. The owner's Delete returns
`NotFound`, and so do Delete of the OP and Prune.

**Proof.** `scratch:bc2_archived_post_cannot_be_deleted`: it uses a full board and the real
submit path. The output is `post No.151 in archived thread 1: delete -> NotFound, still served`,
and the served blocks still contain the text after the publish.

**Impact.** The owner cannot remove illegal content for up to 7 days, and its own onion and
every mirror keep serving it. This is the owner's legal exposure (G.9.2), and an anonymous poster
triggers it at the cost of one reply and one thread PoW. The owner's own "prune" action has the
same trap.

**Fix.** Make `delete` find posts in `archive` as well. Re-encode the archived thread with a
tombstone, push the hash to `dels`, and replace `Archived.blocks` and `thread`. Deleting an
archived OP should drop the archive entry. Add `Board::unarchive(no)` for the owner. In
`Host::post_of`, search `archive` too: decode its chunk blocks, which happens only on this owner
path. Add a test that deletes a post after `prune`.

### BC-2 (high): thread-budget refusals ramp every effort ×64; the attacker pays the grace minimum

**Description.** `tick` treats any refused thread in the last minute
(`saturated_threads_since`, set at `pipeline.rs:382-386`) as pressure and doubles `shift` every
minute (`pipeline.rs:453-459`). `shift` multiplies **reply** efforts too (`effort_now`,
`:252-255`). Grace (`advertise`, `:257-267`; `grace_min`, `:290-293`) accepts anything at or
above the **lowest** effort advertised in the current or the previous epoch. A ramp inside one
epoch therefore never raises what the attacker must pay. The honest app solves at the
**advertised** effort (`channel-web/src/boards.rs:759`: `effort_thread`/`effort_reply`, not
`min_*`).

**Trigger.** Each minute, one thread submission that the budget refuses (`Busy` at `admit`).
Every PoW is solved at `min_thread`.

**Proof.** `scratch:bc5b_attacker_pays_grace_minimum_while_ramping` (base reply 10, thread 80):
after 8 minutes the board advertises reply 640 and thread 5 120. It still accepts reply 10 and
thread 80, and the attacker spent **800 solutions in all**. `scratch:bc5_…` (base 1): after
7 minutes reply effort is ×64, and after 31 minutes `threads_closed` is set by R8. At the
defaults (700/5 600) the ramp costs the attacker ≈ 5 600 solutions a minute, about 25 s of one
native core (B-P1: 226/s). Honest phones are asked to pay 44 800 per reply, ≈ 7.7 min median on
the B-P1b iPhone (96.8/s), and 358 400 per thread. The effort then decays by one step per 10 calm
minutes (`:460-463`), so one ramp keeps posting from phones impractical for about an hour.
After 30 minutes new threads close, or trips-only turns on (BC-5), until the owner acts.

**Impact.** A cheap remote DoS of posting for everyone on phones and mid-range desktops. It is
far cheaper than the "rented server out-solves phones" limit that G.8 accepts, because here the
attacker does not out-solve anyone: it pays the minimum.

**Fix.** (a) Only posts-cap pressure should raise reply effort; the thread budget should raise
thread effort only. (b) Have the app solve at `min_*` when that is enough. Alternatively, make
the grace window track efforts from when the client fetched `/pow`: drop the within-epoch `min`,
keep the previous epoch's value, and advertise at most one step up per epoch. (c) Count refusals
per circuit rather than per minute before treating them as pressure.

### BC-3 (medium): a deleted post can be resubmitted by anyone, including the owner's capcode posts

**Description.** `Board::accept` (`board.rs:258-295`) never checks `dels`. The only replay
defence after the replay table is the duplicate-body check over the last 64 entries
(`:281-284`), and a deletion turns the original into a tombstone, which that check skips. The
published chunk carries `s` and `sig`. The PoW is bound to `k`/`n`/`kind`/`thread`/`effort`,
none of which needs the private key, and `effort` is a free field that the resubmitter walks.
Kind 6 (`CAPCODE`) uses the same `POST` signing prefix as kind 2, and `host.rs:112-115` grants
`cap::OWNER` whenever `s.k` is the board key. That path also skips bans, trips-only and
approved-only (`:119`).

**Trigger.** Within the post's epoch window (`s.e` must be the current or previous epoch, so up
to about 20 min after it was written), fetch the deleted post's chunk from any older root or
mirror and submit `s` and `sig` with a fresh PoW. For a deleted owner post, use `kind = 6`.

**Proof.** `scratch:bc1_deleted_post_replayed_by_anyone`: a deleted trip reply ("DOXX…") comes
back as a new number, and the Tor Browser page `/t/<no>` shows it in full. A deleted owner post
comes back with `## Owner`. `scratch:bc1_deleted_op_subject_back_in_verified_catalog`: a deleted
OP comes back as a new thread. App readers hide the post (its hash is in `dels`), but the
verified catalog entry still carries its `sub` and `ex`, and so does catalog page 1.

**Impact.** Moderation is undone for content deleted quickly (spam, doxx), which is the common
case. Plain pages (`page.rs:199-235`, which ignores `dels`) and the app catalog show it again.
The owner's own retracted statement reappears under the owner capcode.

**Fix.** In `Board::accept`, refuse with `Refused` when `post_hash(&s)` is in `dels`. Keep
`dels` hashed in a `HashSet` beside the `Vec` so the check costs O(1). In `verify::read`, blank
`sub` and `ex` (or refuse) for a catalog entry whose thread's OP is deleted. Give capcode posts
their own signing prefix (`"ephem-board-cap-v1:"`) so a kind-2 signature never verifies as kind 6.

### BC-4 (medium): the deletion list is evicted by count, well inside a root's validity

**Description.** `trim_dels` keeps at most 4 096 entries and drops the oldest
(`board.rs:355-359`). Deleting an OP pushes one entry per post of its thread (`:342-349`), up to
500. A single clean-up of nine flooded threads therefore evicts every older deletion. The same
cap applies to readers' `known_dels`. G.5.1 says entries live "30 days (longer than any record's
validity)" and are enforced "even from an older root"; at 4 096 entries that holds only while
deletions are rare.

**Proof.** `scratch:bc11_dels_evicted_by_count`: post X is deleted. One hour later the owner
deletes nine flood threads of 500 posts. A reader holding the **newest** deletion list then
verifies the 2-hour-old root (record still valid for 72 h), and X verifies and is shown.

**Impact.** A stale or Sybil mirror can again serve deleted content within the 72 h window to
readers who have the current list. The attacker controls the flood that forces the clean-up.

**Fix.** Never let count eviction drop an entry younger than `VALIDITY_S` plus a margin. When
the list is full, refuse new posts (or keep the extra entries in a second block) instead of
forgetting. Better: record one hash per deleted **thread** (the OP's `s`) and have readers drop
a whole thread whose OP is listed, instead of 500 entries.

### BC-5 (medium): trips-only is preloadable, and the attacker evicts the regulars

**Description.** Any post flagged `trip: true` and accepted is added to `own.known`
(`host.rs:360-367`, `own.rs:91-99`). The list is FIFO with 512 entries. Trips-only admits
`known ∪ approved` (`own.rs:87-89`, `host.rs:127`). The doc's guarantee that "a new trip cannot
start posting while the switch is on" holds only for keys first seen after the switch.

**Proof.** `scratch:bc3_trips_only_preloaded_and_regulars_evicted`: one regular trip posts, then
512 fresh attacker keys flagged as trips post. Trips-only is turned on (by the owner or by R8's
`panic_trips`, which the attacker can itself trigger, BC-2). The regular gets `423`/`Paused`; an
attacker key is admitted.

**Impact.** The switch meant to stop a flood admits the flooder and locks out the community.

**Fix.** Score known trips by age and post count, and let a trip qualify only after it has
posted across ≥ N distinct minutes or days. Keep regulars in a separate list that FIFO churn
cannot reach (for example, entries older than 24 h are evicted last). Show the owner how many
known trips were added in the last hour before the switch takes effect.

### BC-6 (medium): with pre-moderation on, 8 posts lock the board for everyone

**Description.** Held posts are capped at `own::HELD = 8`. When the queue is full, every further
anonymous post gets `Busy` (`host.rs:348-356`). Held posts skip the duplicate check (they are not
in a thread yet), so even one signed `s` resubmitted eight times fills it.

**Proof.** `scratch:bc4_premod_queue_lockout`: eight junk replies are held (answer `0`), and the
ninth, honest reply gets `503`/`Busy`.

**Impact.** Pre-moderation, presented as a flood brake (G.8, G.9.1), lets one attacker keep the
board closed to everyone for 8 reply PoWs per owner clean-up.

**Fix.** Make the held queue a bounded ring ordered by effort: evict the lowest effort, as the
publish ring does. Raise the cap; `own` holds 64 KiB, and BC-12's measurement shows the full
state at 53.7 KiB, so held posts need their own block or a smaller summary. Add a per-key and
per-body duplicate check in the held queue.

### BC-7 (medium): the board RAM budget is not implemented

**Description.** G.5.2 and B-M11 promise "64 MiB per board, 128 MiB per hosting tab, pruned by
RAM bytes". Nothing in `crates/board` counts bytes. Each post lives in four places: the decoded
`Signed` in `Thread.entries`, encoded full chunks in `Thread.full`, `Served.blocks`, and the
`Archived.blocks` copy for the archive. Pending deltas add a fifth.

**Proof.** `scratch:bc6_board_ram_unbounded` builds 150 live threads and 150 archived threads,
each of 500 posts of 2 000 B, then hosts them. Measured: **936 MiB live heap, 1 258 MiB peak**,
with 322 MiB of served blocks.

**Reachability.** About 60 posts a minute stays below the pressure that raises effort
(`pipeline.rs:453`). At effort 700 that is ≈ 700 solutions/s, about 3 native cores. Filling
75 000 live posts takes ≈ 21 h; the archive fills over the following days. wasm memory never
shrinks.

**Impact.** The owner's tab grows past 1 GiB and can be killed by the browser, which takes the
board (and anything else in the tab) offline.

**Fix.** Keep a byte counter in `Board`, updated in `accept`, `delete`, `prune` and
`expire_archive`, and prune by bytes in `make_room` (archived threads first). Drop
`Thread.full`/`Archived.blocks` duplicates by serving from `Served.blocks` only. Refuse (`Busy`)
when the per-tab total is reached.

### BC-8 (medium): every publish re-encodes and re-copies every thread's tail

**Description.** `Thread::blocks` pushes the non-full last chunk and the thread block into `out`
for **every** thread on every publish, ignoring `held` (`board.rs:166-185`). `Host::publish`
appends them all to `delta.added` (`host.rs:404-407`), and the page copies them into JS for the
store worker (`channel-web/src/boards.rs:362-371`). G.14.2 and the doc comment on `build_into`
say each block is copied once, when it is new.

**Proof.** `scratch:bc7_publish_recopies_every_tail` uses a board of 150 threads × 60 posts.
Each single reply's publish takes **≈ 180 ms native**, with `delta.added` = 318 blocks,
**19.4 MiB**, repeated on every publish.

**Impact.** Every accepted post makes the owner's tab spend about 0.2 s natively (more in wasm)
and copy and write about 20 MiB. Publishing is allowed once a second, so a modest post rate
keeps the tab's CPU and OPFS busy, which also delays Tor processing in the same tab.

**Fix.** Cache the encoded tail chunk and thread block per thread, with a `dirty` flag set in
`accept`, `delete` and `set_*`. Rebuild only dirty threads and their bucket, and skip `held`
CIDs for every block, not only full chunks.

### BC-9 (medium): a malicious board owner aborts a mirror's tab through `/t/<no>`

**Description.** `verify::read` checks a catalog entry's `r` against the thread only when the
thread **and all its chunks** are present. When a chunk is missing, `thread()` returns
`Ok(None)` (`verify.rs:179`), and the catalog entry is accepted with any `r` (`:278`). The
mirror then serves the thread block. `page::thread` passes `4096 + row.r as usize * 400` to
`String::with_capacity` (`page.rs:177`). The multiplication is unchecked: it wraps in release,
and on wasm32 `as usize` truncates first.

**Proof.** `scratch:bc13_catalog_r_unchecked_crashes_mirror_page`: an owner sets `r = 2^40` and
withholds the chunk. The record and blocks verify, and `Served::new(…, Mirror)` succeeds. Then
`respond(Route::Thread)` aborts: `memory allocation of 439804651114496 bytes failed`, SIGABRT.

**Impact.** Any GET by a Tor Browser user (or the attacker) kills the mirror's tab, which runs
with `panic = "abort"`. Volunteer mirrors of a hostile or compromised board can be crashed on
demand.

**Fix.** Never size buffers from block fields: use a fixed `with_capacity(16 * 1024)` or
`min(row.r, THREAD_POSTS)`. In `verify::read`, refuse `ce.replies >= THREAD_POSTS`. Check every
catalog integer against its limit whether or not the thread is held.

### BC-10 (low): quadratic owner-side paths and uncached pages stall the tab

**Description and timings** (native release, full board of 75 000 posts):
- `delete_where` calls `delete(no)`, which runs `post_of` (a scan of every post) plus
  `deletes.iter().any` for each post (`host.rs:176-202`). `publish` then calls `Board::delete`,
  which scans again, followed by `trim_dels` (`board.rs:333,351`). Measured: **queuing 75 000
  deletes took 16.9 s and applying them 22.4 s**. Clean-up after a flood is exactly when the
  owner needs this, and the tab (and its arti) is frozen meanwhile.
- `Board::load` clones every archived block once per archive entry
  (`archived_blocks.clone()` at `board.rs:543`): O(256 × archive bytes) at reopen.
- `Served::respond` rebuilds plain pages on every request and verifies the record signature each
  time (`gateway.rs:295-303`). A full thread page measures 1 MiB and 6.4 ms per request natively,
  with no cache, while the index response is cached.
- A reader's `verify` of a full board took 10.6 s. This is dominated by per-post Ed25519 and
  `post_hash`, not by the `dels`/`known_dels` scans, so it is not a finding by itself.

**Fix.** Build an index `no → (thread, entry)` once per mass delete. Keep pending deletes in a
`HashSet`, and apply a batch with one pass per thread and one `trim_dels`. In `load`, build the
archived-block map once. Cache rendered pages per root (as `index`), and keep the record's
sequence in `Served` instead of verifying again.

### BC-11 (low): input-reachable integer overflow

`pipeline.rs:335`: `h.epoch + 1` with `epoch = u32::MAX` from the header.
`scratch:bc9_epoch_overflow` in a debug build panics with `attempt to add with overflow`. Release
builds (`build.sh` uses `--release`, no `overflow-checks`) wrap and refuse with `Pow`, so shipped
builds are not affected. The same pattern appears at `:273` and `:314`. G.14.1 promises
"`panic = "abort"` never reachable from input" in any build. **Fix:** use `h.epoch.checked_add(1)`
or compare `cur.checked_sub(1)`.

### BC-12 (low): the solution is spent before the body arrives, so retries after drops or evictions are refused

`check_header` writes the replay slot with `no = 0` before the body is read (`pipeline.rs:354`).
An identical retry matches the slot and, since `no == 0`, gets `Refused`/`409` (`:345`). This
happens after a stream drops between header and body (a common Tor event), after a `Busy`
eviction (`host.rs:132-135`), and after a ring-full `Busy` at `admit`. G.6.1 step 5 promises
idempotent resubmits, and G.8 says "a refused client may retry with more effort", but the same
solution can never be used again. Traced; no test. **Fix:** record the outcome in the slot
(`Busy`: let a retry with the same `h` read the body again, while a different `h` is refused),
and answer `Busy` again instead of `Refused`.

### BC-13 (info): deterministic `own` nonces

`Host::new` seals with `now_ms ‖ 0` and `publish` with `now_ms ‖ prev seq`
(`host.rs:86-87,391-393`). I found no reuse: `seq` strictly increases, and `Host::new` uses 0.
Two `Host::new` calls in the same millisecond (or with the same coarse Date.now under a
fingerprinting-resistant browser) after a clock step, or BD-7 split-brain, would reuse a
(key, nonce) pair with different plaintexts. XChaCha's 192-bit nonce exists so that it can be
random. **Fix:** fill the 24 bytes from the RNG the crypto crate already uses (getrandom), or
append 8 random bytes to the time ‖ seq prefix.

### BC-14 (info): weak validation of mirror and see-also onions

`set_mirrors` and `set_see_also` check only `len == 62 && ends_with(".onion")`
(`board.rs:386-401`). For example, `attacker.example/aaaa…aa.onion` passes and becomes an
`http://` link on the Tor Browser page (`page.rs:137-140`; escaping holds, so this is not an
injection). Only the owner can set these. **Fix:** decode the 56 base32 characters, then check
the v3 version byte and checksum.

## Attack-surface inventory

| Entry of untrusted bytes | Parser / first cost | Notes |
|---|---|---|
| Request head (any stream) | `gateway::route`: in place, ≤ 8 KiB, `Content-Length` parsed to `usize`, `Transfer-Encoding` refused | Sound; one request per stream |
| Submit header (188 B) | `Header::parse` → `check_header`: replay probe (≤ 64 slots), keyed seed, one BLAKE2b predicate, then Equi-X | Allocation-free except Equi-X; BC-2, BC-11, BC-12 |
| Submit body (≤ 2 400 B) | `cbor::Value::decode` (depth 16, prealloc ≤ 16), `Signed::from_value` (exactly 9 keys), Ed25519 | BC-3 |
| Admitted posts | `Host::submit_body`, then `publish` → `Board::accept` | BC-1, BC-5, BC-6, BC-7, BC-8 |
| Read requests | `Served::respond` (pages, CAR, raw, index) | BC-9, BC-10 |
| Index / CAR from a host or mirror (readers, mirrors) | `parse_index` (size caps), `car::read` (CID checked), `verify::read` | BC-4, BC-9 |
| `own` block (owner's store) | `Own::open`: size, step and AEAD checks first | Sound; BC-13 |

## What is solid

- **The PoW binding.** The challenge covers board, seed(epoch), kind, thread, `k`, `n` and
  effort (`pow.rs`). I found no replay of one solution across boards, threads, kinds or epochs.
  The replay key includes the epoch, and the solution is not published, so nobody can make the
  host answer another poster's ticket (the idempotent `Done` requires the secret solution).
  `Header::parse` ties kind 1 to thread 0.
- **The refusal order.** Content-Length, kind, pause, epoch and the grace minimum are checked
  before any hashing. The BLAKE2b effort predicate runs before Equi-X, so forcing an Equi-X
  verify costs the attacker about `effort` hashes. The body hash is checked before decoding.
- **Capcode forgery.** Forging a capcode with a new text is impossible: the host
  (`host.rs:113`) and readers (`verify.rs:156`) require `s.k` to be the board key. Only replay
  (BC-3) works.
- **Strict CBOR.** The decoder rejects non-shortest forms and indefinite lengths, limits depth to
  16 and caps preallocation. `verify` bounds threads, chunks, posts per chunk, archive, mirrors
  and see-also. The fuzz test passes; I found no panic in `verify` on mutated blocks.
- **Reader rules.** CIDs are re-hashed and the record is verified. Future sequences are refused
  (`> now + 1 h`), and the high-water mark is enforced. `s.b`/`s.t` placement and `next_no` are
  checked, and `r` is checked when a thread is held (but see BC-9).
- **HTML.** `esc` covers `& < > " '` everywhere text is written, numbers are formatted, `href`s
  carry a fixed scheme, and the CSP is `default-src 'none'`, with no scripts and no forms.
- **The whole-board CAR refusal (B-M10).** The root, `threads` and `arch_threads` are refused,
  other DAGs are capped at 1.5 MiB, and the index is built once per version.
- **`own` sealing.** XChaCha20-Poly1305 with AAD bound to the board key, length-prefixed
  padding, size and step checked before decrypting. The full documented state seals at
  53.7 KiB, under the 64 KiB cap (`scratch:bc12_own_seal_capacity`).

## Recommended next audits

1. `crates/channel-web/src/boards.rs`: the serve loop (slot pool, deadlines, the 16 KiB/s
   minimum rate, which I did not see enforced in `submit_one`), mirror pulls (sizes, the
   withholding that BC-9 relies on), and the answer polling.
2. BD-0 accept loop in `crates/tor`: per-circuit submit limits, which bound BC-2 and BC-10.
3. A wasm32 measurement of BC-7, BC-8 and BC-10 in the hosting tab (memory ceiling and
   publish time).
4. The app's poster side: the effort choice (BC-2, fix b) and how it handles `409` after a drop
   (BC-12).

## Fixes (2026-10-03)

Plan: [BOARDS-AUDIT-FIX-PLAN.md](BOARDS-AUDIT-FIX-PLAN.md). Tests: `crates/board/tests/audit.rs` (16, each a finding), plus the existing suites updated where the behaviour changed on purpose.

| ID | Change | Test |
|---|---|---|
| BC-1 | `Board::delete`/`delete_many` find posts in archived threads (decoded from their blocks, tombstoned, re-encoded; an archived OP drops the entry); `Board::find` and `Board::select` (ban, mass delete) reach the archive. Each archived thread keeps its number range, so only candidates are decoded | `bc1_a_post_in_an_archived_thread_can_be_deleted`, `bc1_the_owner_finds_and_mass_deletes_archived_posts` |
| BC-2 | Two multipliers: posts-cap pressure raises both efforts, a **thread flood** (at least `THREAD_FLOOD` = 3 refused new threads in a minute) raises the thread effort only; one or two refused threads a minute are no pressure and never reach R8. The grace accepts the efforts advertised before a raise for `GRACE_S` = 120 s, not the lowest of a whole epoch | `bc2_refused_threads_raise_only_the_thread_effort_and_only_in_a_flood`, `bc2_a_raise_is_enforced_after_the_grace` |
| BC-3 | `accept` refuses a post whose hash is listed (a hash set beside the list); capcode posts sign with their own prefix (`ephem-board-cap-v1:`); readers blank the subject and excerpt of a catalog entry whose OP is listed (the catalog carries `op`, the OP's hash) and delete a thread whose OP is listed whole | `bc3_a_deleted_post_is_refused_when_resubmitted`, `bc3_bc4_a_stale_root_with_a_deleted_op_shows_nothing_of_it`, `forgeries_and_misplaced_posts_fail` |
| BC-4 | A deleted thread lists its OP only (not 500 entries); past `DELS` (4 096) only entries older than a record's validity + 1 h go; `DELS_MAX` = 16 384 is the hard cap readers accept | `bc4_deletions_younger_than_a_record_stay_past_the_soft_cap` |
| BC-5 | Known trips record first/last hour and hours seen; one qualifies for trips-only after posting in 2 different hours; a full list evicts unqualified keys first, then young ones, regulars (≥ 24 h) last. A burst cannot qualify; an attacker preparing keys a day ahead still can (mitigated, not closed) | `own::tests::trips_qualify_slowly_and_regulars_stay`, `owner_moderation` |
| BC-6 | The held queue keeps the highest efforts (a newcomer that paid more evicts the lowest), and the same text is held once. The cap stays 8 (the `own` block's 64 KiB) | `bc6_a_full_held_queue_keeps_the_highest_efforts` |
| BC-7 | A byte budget, `BYTES` = 24 MiB of encoded posts (live threads and archive): past it the archive's oldest go, then unprotected threads, then posts are refused (`Busy`). A full board of short posts fits; one of 2 000-byte posts holds about 11 000. The tab still holds about 3 copies (decoded posts, blocks, the served map): deduplicating them is left for later | `bc7_the_board_stays_within_its_byte_budget` |
| BC-8 | Each thread caches its encoded last chunk and thread block until it changes; every block already served (`held`) is skipped, root-level blocks included, and the `dels` block is cached | `bc8_one_reply_copies_a_few_blocks_not_every_thread` |
| BC-9 | `page::thread` no longer sizes from `r`; `verify::read` refuses a catalog `r ≥ THREAD_POSTS` whether or not the thread is held | `bc9_a_huge_reply_count_is_refused_and_never_sizes_a_page` |
| BC-10 | Mass delete selects in one pass and applies with `delete_many` (one pass per thread, one trim); `load` moves each archived thread's blocks out of one map; plain pages are cached per version (16), the record's sequence checked once | `bc10_mass_delete_is_linear` (20 000 posts deleted in one pass; before: 39 s for 75 000 natively) |
| BC-11 | `checked_add`/`saturating_add` on header epochs | `bc11_an_epoch_at_the_maximum_is_refused_without_overflow` (debug build) |
| BC-12 | Replay slots keep a state: header seen (a retry reads the body again), admitted (`Busy`: wait), published (the number), refused (`Refused`); a `Busy` at the caps or an eviction returns the slot to "header seen" | `bc12_a_submission_refused_busy_may_be_sent_again`, `every_refusal` |
| BC-13 | Every seal draws a random 24-byte nonce (`getrandom`, host feature only) | `bc13_every_sealed_own_block_has_its_own_nonce` |
| BC-14 | Mirrors, "see also" onions and the new signed `host` are checked as v3 onions (base32, checksum, version) by the owner **and** by every reader (`crates/board/src/onion.rs`) | `bc14_bf1_bf3_manifest_onions_are_checked_by_readers`, `onion::tests::known_address` |
