<!-- SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0 -->
<!-- Copyright 2026 Anton (darkcite) -->
# Security audit 2026-10-03: boards, web front end

## Scope and method

**Scope:** the page side of boards and the changes around it: `app/boards.js`, `app/pow-worker.js`,
`app/store-worker.js`, the boards integration in `app/channels.js` (`torEngine`, `vaultApi`, follow
entries with `k: 'board'`, `openLink` handing `#B=` links over) and `app/app.js` (fragment filter,
`applyCode` routing), the board sections of `app/index.html`, `tools/stamp.py` (SRI, CSP),
`app/sw.js` (precache lists), and the spike pages published on the same origin
(`checks/spikes/board_soak/probe.html`/`probe.js`, `checks/spikes/equix_bench/phone.html`/`phone.js`).
The Rust side was read only where the page relies on it (`crates/channel-web/src/boards.rs`:
`read`, `draft`, `view_json`; `crates/board/src/verify.rs`; `crates/board/src/board.rs`;
`crates/crypto/src/contacts.rs`).

**Read first:** `docs/P2P-CHAT.md` §17, §21, §28; `docs/BOARDS.md` G.4, G.6, G.8, G.10–G.14;
the earlier web audit `docs/security/AUDIT-2026-10-03-web.md` (W-2, same-origin pages, is accepted;
W-5, plaintext metadata, is open; W-9, framing, is fixed).

**Ran (all in the session scratchpad, nothing in the repo):**

- `bf/run.mjs`: Playwright (Chromium from `checks/node_modules`) with `serve`/`launch` from
  `checks/e2e_lib.mjs`. It serves the repo read-only plus a harness page under `/__h/` that loads
  the real `app/boards.js`, `app/ui.js`, `app/pow-worker.js` and `app/pkg/ephem_pow.wasm`, with an
  import map that swaps `app/channels.js` for a mock and a mock `BoardApp` that records the
  arguments of `read`, `draft` and `post_draft` and returns crafted views. The mock's
  `saveFollowList` applies the key file's real section limit (`contacts.rs:36`). Result: 11/11
  checks pass; each proof is quoted in its finding.
- `bf/csp.mjs`: a page with the app's CSP (no `'unsafe-eval'`) starts a same-origin dedicated
  Worker. Result: `{"pageEval":"EvalError","worker":{"ev":4,"ws":"allowed"}}`, so the Worker does
  not run under the page's CSP.

**Could not run:** the Tor lab (`checks/tor-lab/`): the proofs stub `BoardApp`, and the Rust side of
each path is followed by reading the code (cited). Firefox and Safari: the results are from Chromium.

## Summary table

| ID | Severity | Area | Title | Status |
|---|---|---|---|---|
| BF-1 | **medium** | Links / posting | The `o=` onion of a `#B=` link is never checked, but it is where every post goes, and it is stored first in the follow entry; the page still says "Verified" | open (confirmed, harness) |
| BF-2 | **medium** | PoW | No upper limit on the effort a host asks for, and the PoW Workers are never stopped when the reply box changes: a hostile board burns 4 cores per thread opened, until the tab closes | open (confirmed, harness) |
| BF-3 | **medium** | Follow list | A followed board's onion list only grows (signed mirror strings, unchecked): past 64 KiB the key-file section is refused and the **whole follow list** silently stops saving; the newest mirrors drop off the 9-onion read cap | open (confirmed, harness + code trace) |
| BF-4 | **medium** | localStorage | Every board opened is cached in localStorage with no size bound and no eviction; one board can fill the origin's quota, and the app's later writes fail silently | open (confirmed, harness) |
| BF-5 | low | Storage at rest | Board metadata in plaintext outside the key file: catalog text of every board opened (followed or not, also for temporary identities), mirror onion seeds, mirror and IPFS lists, drafts | open (confirmed, harness) |
| BF-6 | low | Supply chain / CSP | `pow-worker.js` and `store-worker.js` have no integrity pin, and Workers run without the page's CSP | open (confirmed, csp.mjs) |
| BF-7 | low | UI / identity | A board's identity is never shown to readers; untitled rows and see-also links show only `k51qzi5uqu5d…`, the prefix every board name has | open (confirmed, trace) |
| BF-8 | low | Trips / consent | No warning that a trip removes deniability (G.4 promises one), and the trip label stays filled when the reader moves to another board | open (confirmed, trace) |
| BF-9 | low | Owner UI | Irreversible bulk moderation (delete all of a trip, ban, prune, mass delete) has no confirmation, and the owner view re-renders every 3 s under the pointer | open (unconfirmed: UI race) |
| BF-10 | info | Same origin | The spike pages share the app's origin and storage and load the Tor build without integrity; they have no injection sink | open (info) |

Counts: 4 medium, 5 low, 1 info. No critical or high. No DOM injection was found (see "What is solid").

## Findings

### BF-1 (medium): the link's onion decides where posts go, unauthenticated

**Description.** A board link is `tor.html#B=<name>&o=<onion>[&m=<mirror>,…]`. The name is
authenticated: the record and blocks are verified against it. The onions are not. The owner's onion
comes from its own HKDF seed (G.4) and is not in the signed manifest, so a reader cannot check it.
`boards.js` uses the first onion of the link for `/pow` and `POST /submit`. An attacker who sends
a link with a real board's name, their own onion as `o=` and the real onion as `m=` gets:

- the real board shown, labelled "Verified through Tor: signed by the board key" (true for the
  content), because the read races every onion and the real one serves the newest version. The
  attacker's onion can also proxy a valid copy;
- every post of that reader sent to the attacker's onion. It can drop them, delay them, answer
  "Posted as No. N" with an invented number, or forward them. Held (pre-moderated) posts reach it
  too. It also sees each reply-box focus (a `/pow`) and each 30 s read, so it knows when the
  reader has the board open (through Tor: no IP);
- this for good: `follow()` stores `[...c.onions, ...v.mirrors]` with the link's `o=` first, and
  every later visit from Following uses `f.o[0]` for posting.

**Evidence.**
- `app/boards.js:87-94` (`openLink`): `onions = [p.get('o'), ...m]`, no check beyond Rust's
  onion syntax.
- `app/boards.js:794-795` (`presolve`): `const onion = c.onions[0]; … b.draft(c.read, onion, …)`.
  `crates/channel-web/src/boards.rs:620-649`: `draft` sends `/pow` to that onion, and
  `post_draft` (`:656-665`) submits to `draft.onion`.
- `app/boards.js:664` (`follow`): `o: [...new Set([...c.onions, ...v.mirrors])]`, link first.
- `app/boards.js:565-567`: the "Verified…" text does not say where posts go.

**Reproduction** (`bf/run.mjs`, harness with a recording `BoardApp`):
```
PASS  BF-1 read uses the link onion first  — ["eee….onion","ggg….onion"]
PASS  BF-1 view says verified  — Verified through Tor: signed by the board key, version 7, …
PASS  BF-1 /pow and submit go to the link onion  — {"d":["eee….onion"],"p":["eee….onion"],"st":"Posted as No. 5."}
PASS  BF-1 follow entry stores the link onion first  — [["eee….onion","ggg….onion"]]
```

**Impact.** One malicious link (a user action) gives targeted, silent censorship of a reader's posts
behind a "Verified" label, a fake success message, and a presence signal. Posts cannot be forged:
they are signed by the poster key, and trips are per board. G.14 lists a malicious *host* and
*mirror*, but not a third party picking the posting endpoint.

**Fix proposal.** Sign the host onion into the manifest (`host: "<56>.onion"`, set by
`Board::create`/`take_over` from `onion_of(seeds[1])`). Readers then post only to the signed
`host`, and `o=`/`m=` become read hints only. Until then: post only to an onion that is in the
signed mirror list or that the user confirms, and show "Posting to <onion>" next to the reply box.
In `follow()`, store the signed onions first and the unsigned link onions after them.

### BF-2 (medium): unbounded PoW effort and Workers that are never stopped

**Description.** The effort comes from the host's `/pow` answer (`u32`, `PowInfo::read`,
`crates/board/src/pipeline.rs:173`) and is passed to the Workers unchanged. The Workers solve until
they find a solution. With effort `0xFFFFFFFF` that never happens. `solve()` terminates its
Workers only when its promise settles (`app/boards.js:886-888`). Opening another thread, changing
the trip or opening another board sets `box = null` (`setBox`, `:777`), but the running Workers keep
going: nothing cancels them. Each reply-box focus with a new key starts `min(4, cores)` more. The
progress callback of the old solve also keeps writing "Preparing your post…" into the new box.

**Evidence.** `app/boards.js:785-808` (`presolve`, started on `focus`, `:948`), `:866-889`
(`solve`, no cancellation), `app/pow-worker.js:23-26` (loops until solved). No cap in
`crates/channel-web/src/boards.rs:649` or in the page. G.8/G.12 promise an estimate (median and
p95) before solving; none is shown.

**Reproduction** (`bf/run.mjs`, real `pow-worker.js` and `ephem_pow.wasm`, effort `0xFFFFFFFF`):
```
PASS  BF-2 proof-of-work workers accumulate (never terminated when the box changes)
      — workers before=0 after 3 thread switches=12 (hardwareConcurrency=4)
```
(The script prints these checks under the name "BF-5", its working number for this finding.)

**Impact.** Any board owner (or the attacker of BF-1) can pin every reader's CPU: 4 more busy
Workers for each thread whose reply box the reader focuses, until the tab closes. Phones overheat,
drain and get throttled, and posting is impossible. This is a bounded DoS of the reading tab.
Honest boards also leak Workers: an unfinished solve keeps running after the reader moves on.

**Fix proposal.** (1) Refuse efforts above a client maximum, with a stable error (e.g.
`E_BOARD_POW_TOO_HIGH` above 64 × the default base). Show the estimated median and p95 before
solving, as G.12 says. (2) Make `solve` cancellable: keep the current `{workers, reject}` in `box`,
and terminate them in `setBox()` and whenever `presolve` replaces `box`. (3) Bind the progress
callback to the box it belongs to.

### BF-3 (medium): the follow entry's onion list grows forever and can break saving the follow list

**Description.** On every successful read of a followed board:
`f.o = [...new Set([...f.o, ...v.mirrors])]; channels.saveFollowList();` (`app/boards.js:545,549`).
Nothing is ever removed. The reader's verify checks only the *number* of mirrors (≤ 8), not their
form or length (`crates/board/src/verify.rs:135`; the owner-side `set_mirrors` check in
`board.rs:386-391` does not bind a hostile owner). The follow list is one key-file TLV section,
limited to 65 535 bytes (`crates/crypto/src/contacts.rs:36`). When it is over, `set_section` fails,
and `saveFollows` (`app/channels.js:294-300`) skips `ctx.persist()` and says nothing. Every follow,
unfollow or "seen" change after that, for **channels and boards**, is lost at the next reload.
Ordinary mirror churn has a second effect: `refresh` passes `[...c.onions, ...f.o, ...known.mirrors]`
(`app/boards.js:536`), and Rust keeps only the first `1 + MIRRORS` = 9 valid onions
(`crates/channel-web/src/boards.rs:686-690`). After a few mirror changes, the board's *current*
signed mirrors are never tried, so the board reads as unreachable while the owner is offline,
although live mirrors exist.

**Reproduction** (`bf/run.mjs`):
```
PASS  BF-3 follow entry onion list grows with every mirror ever signed  — f.o.length=58
PASS  BF-3 read() asked with > 9 onions: the latest signed mirrors are beyond the cap  — onions passed=50
PASS  BF-3 follow list section exceeds the 65535-byte TLV limit -> set_section refuses, saveFollows silently does not persist  — {"bytes":73889,"ok":false}
```
The last step is one signed "mirror" string of 70 000 characters in the manifest, one refresh.

**Impact.** A malicious board owner whose board the user follows silently stops the user's whole
follow list from being saved: a remote DoS of persistence, fixed only by unfollowing the board,
which the user has no reason to do. Without any malice, followers slowly lose the mirror fallback.

**Fix proposal.** In the reader, refuse mirrors and see-also entries that are not `<56 base32>.onion`
(and `k51…@onion`) in `verify::manifest` itself, as `set_mirrors`/`set_see_also` already require.
In the page, set `f.o = [link/owner onion, ...v.mirrors]` (replace, not union; at most 9 entries)
and order the read list as signed mirrors after the owner onion. In `saveFollows`, report a
non-zero `set_section` to the user instead of dropping it.

### BF-4 (medium): an unbounded, never-evicted localStorage cache of every board opened

**Description.** Every successful read stores the whole view without thread bodies:
`store$.set('ephem-board-view:<name>', {...v, threads: []})` (`app/boards.js:725-727`). That covers
title, about, rules, mirrors, see-also, the catalog, the archive and the mod log. There is no size
limit and no eviction, and failures are swallowed (`:721`). Opening a link is enough: no follow is
needed. A failed write keeps the old value, so a board that grows a little with each version
*ratchets* the item up to the origin's quota without knowing how full it is. Control characters
help it: one byte in the record becomes six characters (`\u0001`) in the stored JSON. Once
localStorage is full, the app's other new writes fail silently: the device id on a profile that has
none yet (`app/channels.js:840-848`: a fresh random id on each call, so this device's own board lease
reads as "another device" and `renew()` stands down, `boards.js:312-313`), the vault rollback floor
for an identity first used after that (`channels.js:828-832`, M-2), board and channel mirror seeds
(the mirror's onion changes on every visit), the board mirror and IPFS lists, and settings toggles.
The cached copy is also shown with the text "Verified through Tor: signed by the board key…"
(`:565-567`) before any network read, although it is not re-verified. G.11.1 says the cold-start
cache holds "CIDs in IndexedDB".

**Reproduction** (`bf/run.mjs`; versions growing by 16 384 control characters, then by 40, one per
refresh, 230 refreshes):
```
PASS  BF-4 one board fills localStorage; later small writes of the app fail
      — {"usedChars":5242716,"item":5242638,"tries":{"ephem-board-mirror:…":"stored","ephem-vault-seq-…":"stored","ephem-device":"QuotaExceededError"}}
```
The board's single item ends within 164 characters of Chromium's 5 242 880-character quota; the
third small write fails.

**Impact.** Remote, bounded DoS of every localStorage-backed feature for a reader who keeps a
hostile board open (about 2 hours at one version per 30 s refresh; faster with several boards
through see-also). Two of the writes that fail back security controls (rollback floor, device
lease).

**Fix proposal.** Cache only what B-UX-3 needs: name, sequence, title, and the first N catalog rows
with `sub`/`ex` truncated, capped at, say, 32 KiB per board and 8 boards (LRU). Better, follow
G.11.1: keep CIDs (or the verified CAR) in IndexedDB/OPFS and re-verify on load. Label a cached
view "Last verified copy from <time>", not "Verified through Tor".

### BF-5 (low): board metadata in plaintext outside the key file

**Description.** D2 puts the follow list in the encrypted key file, and §16/F.2.2 keep metadata out
of plain storage. Boards add these in plaintext, kept after sign-out and written for temporary
identities too:
- `ephem-board-view:<name>`: the catalog (subjects and excerpts) of **every board opened**, followed
  or not (`app/boards.js:725-732`);
- `ephem-board-mirror:<name>`: the 32-byte seed of the mirror's onion key, so anyone who reads it can
  run that mirror's onion (`:678-687`);
- `ephem-board-mirrors` (boards mirrored, restarted on every Tor-mode load for whoever uses the
  profile, `:734-746`) and `ephem-board-ipfs` (`:748-755`);
- drafts in `sessionStorage` under `ephem-board-draft:<name>:<thread>` (`:782`, `:949`).

**Reproduction** (`bf/run.mjs`, signed out):
`PASS BF-4 board view (catalog text) stored in plaintext localStorage, signed out — [["ephem-board-view:k51qzi5uqu5dlegitboard0",70367,true]]`
(the script's working number for this check is BF-4).

**Impact.** Anyone with access to the device or profile learns which boards were read, what they
contained, and which boards the user mirrors (with their keys). This is W-5's class, made wider.

**Fix proposal.** For a saved identity, put the view cache, the mirror list and the IPFS list in a
key-file section (as the follow list) or seal them with the vault key. Derive mirror seeds from the
identity (`HKDF(seed, "p2pchat/board-mirror/" ‖ name)`) instead of storing them. Temporary
identities keep them in RAM. Clear `ephem-board-*` on sign-out.

### BF-6 (low): Workers without integrity, and outside the page's CSP

**Description.** `pow-worker.js` and `store-worker.js` are started with
`new Worker(new URL(...))` (`app/boards.js:100`, `:874`). `tools/stamp.py:29` hashes them into the
build id, but nothing checks the hash when they load (a Worker has no `integrity`). When the
service worker does not control the page (first visit, private window, Shift+Reload,
`updateViaCache` misses), they come from the network and may belong to another deploy than the
pinned page. A dedicated Worker from an http(s) URL gets the CSP of its own response (CSP3).
GitHub Pages sends none, so the Workers run with no CSP at all: `eval`, `ws:`/`http:` connections,
any `importScripts`. The store Worker holds every block of the owner's boards. Minor: `module()`
fetches `ephem_pow.wasm` without `integrity` when its `<meta>` is missing (`:858-859`, fail-open).

**Reproduction** (`bf/csp.mjs`): page under the app's CSP → `pageEval: "EvalError"`; its Worker →
`eval` = 4, `new WebSocket('ws://…')` allowed.

**Impact.** Hardening. Under the accepted W-1/W-2 model (the host and the origin are trusted), this
needs a hostile or mixed deploy, but the stamped SRI chain does not cover these two files.

**Fix proposal.** Fetch the Worker source with `integrity` (hash in a `<meta>` as for the wasm) and
start it from a `blob:` URL (`worker-src 'self' blob:`). A `blob:` Worker inherits the page's CSP.
Fail closed when the `<meta>` is missing.

### BF-7 (low): a board's identity is never shown

**Description.** The reader view shows the board's title (chosen by its owner) but never its name or
a fingerprint (`app/boards.js:559-588`). Untitled rows use `short(n)` = the first 12 characters
(`:76-77`), and see-also links show `n.slice(0, 14)` (`:582`). Every Ed25519 IPNS name begins with
`k51qzi5uqu5d` (12 characters), so all untitled rows look the same, and see-also links differ in two
characters at most. A copy board with the same title and about (from BF-1-style links, or from a
see-also on a board the user trusts less) cannot be told apart. Posts are not at risk: trips are per
board name. Being misled about which community one is reading is.

**Fix proposal.** Show a short fingerprint taken from the *end* of the name (or a word-list hash)
beside the title, on rows, in see-also links and in the reader header. Add "Board key: …" with copy
under the title.

### BF-8 (low): trips without the promised warning, and a trip carried to the next board

**Description.** G.4: "A trip signature removes deniability… (the UI says so when a trip is first
used)". No such text exists: the field is `<input id="bd-trip" placeholder="trip label (optional)">`
(`app/index.html:476`), and `boards.js` shows nothing (`:771-808`). `setBox()` (`:771-780`) does not
clear `#bd-trip` when the reader opens another thread or board, so a label typed on board A also
signs the next post on board B with B's trip key (`presolve`, `:788`).

**Impact.** Posts signed with the identity-derived trip key without the user meaning to, on a board
where they meant to stay anonymous. A seized key file then proves they wrote them.

**Fix proposal.** Clear the trip field on board change (or keep labels per board). Show a one-time
confirmation the first time a trip is used on a board: "Posts under a trip are signed by a key
derived from your identity; anyone holding your key file can prove you wrote them."

### BF-9 (low, unconfirmed): bulk moderation without confirmation, re-rendered under the pointer

**Description.** "Delete all of this trip", "Ban trip", "Prune", "Lock" and the mass delete "every
post from No. N" act at once, with no confirmation and no undo (`app/boards.js:453-467`, `:933-936`;
only single deletes have the 5 s undo, `:479-489`). The owner view re-renders every 3 s unless focus
is inside it (`:397-399`). The catalog sits above the open thread (`app/index.html:445-446`), so a
new thread posted by anyone moves every moderation button down by one row, which can happen between
the owner aiming and clicking.

**Impact.** A mis-click wipes all posts of an innocent trip or prunes a thread. An anonymous poster
can try to time it. Not reproduced (a UI race).

**Fix proposal.** Confirm the bulk actions (naming the trip and the count), give them the same undo
window as single deletes, and stop the periodic re-render while the pointer is over the owner view
(`pointerenter`/`pointerleave`), not only while it has focus.

### BF-10 (info): spike pages on the app's origin

`checks/spikes/board_soak/probe.html` and `checks/spikes/equix_bench/phone.html` are published on
`darkcite.github.io` (BOARDS.md B-P1b, B-P11), so they share the app's origin and storage (W-2,
accepted). Checked: they parse no URL fragment, write only with `textContent`, set CSP with
`script-src 'self'`, and sit outside the service worker's scope (`/ephem/app/`). They have no
injection sink, so they cannot be used to run code on the origin. The probe loads
`app/pkg/ephem_tor.js` and `ephem_tor_bg.wasm` without `integrity` (`probe.js:5,48`). It runs a
second Tor client in the same origin, with whatever the Tor build keeps in IndexedDB (see W-8),
from the repository head and not the version the user accepted. It is not frame-protected, but it
has no action worth framing. Recommendation: take both off the published branch once their
measurements are done (or serve them from another origin), as W-2 advises for `checks/`.

## Attack-surface inventory (boards front end)

| Entry | Source | Parser / first use | Notes |
|---|---|---|---|
| `#B=` fragment | link, paste, QR | `app.js:1771-1776` (`takeFragment`), `:2297`, `:1492`; `channels.js:166`; `boards.js:87` (`URLSearchParams`) | name checked in Rust `read`/`draft` (`Cid::parse`, Ed25519); onions syntax-checked by `is_onion`; **no authentication of `o=`** (BF-1) |
| Board view JSON | host or mirrors (Tor) | Rust `verify` → `view_json` → `JSON.parse` → DOM | all text via `textContent`/text nodes; mirrors and see-also strings unchecked (BF-3, BF-4) |
| `/pow` answer | host onion | `PowInfo::read` → `Draft.params` → Worker | effort unbounded (BF-2) |
| Submit answer | host onion | `gateway::parse_answer` | fixed format; `no` shown as a number |
| Error strings | Rust (with onion names, codes) | `textContent` | no sink |
| localStorage `ephem-board-*` | this origin | `store$.get` (`JSON.parse`) | only same-origin writers (W-2); spread copies `__proto__` as an own key (no pollution) |
| sessionStorage drafts | this tab | `value =` | plain text |
| Store Worker messages | this page only | `ops[op]` | names from `b.name(i)`, CIDs from Rust `to_text`; OPFS rejects `/` and `..` in names |
| PoW Worker messages | this page only | `pow-worker.js` | buffers ≤ 256 bytes at fixed offsets; name ≤ 48 bytes |
| Vault board entries | own vault (sealed) | `scanOwned`, `autoTakeOver`, `numberFloor` | trusted (own key); rollback guarded by the floor (but BF-4) |

## What is solid

- **No DOM injection.** Every board string (title, about, rules, subjects, bodies, trips, held
  posts, mod log, error texts) goes through `textContent` or text nodes. The only `innerHTML`
  writes are fixed literals (`boards.js:333`, `:593`). Harness:
  `PASS solid: board content renders as text; see-also hrefs stay fragments — {"pwn":false,"imgs":0,
  "hrefs":["#B=javascript:window.PWN=1//&o=x", …]}`. A see-also `href` always starts with `#B=`,
  so `javascript:` or another scheme cannot appear, and a click is intercepted (`preventDefault`).
  Opened in a new tab, it can only reach the `#B=` route.
- **Link routing.** `#B=` is uppercase-distinct from bridge links (`#b=`). The mode redirect
  (`app.js:2064`) leaves it alone. `takeFragment` strips it from the URL. A `#B=` link cannot reach
  `applyCode`'s invite or transfer branches.
- **Trips are per board** (`HKDF(seed, board name ‖ label)`, `boards.rs:630`) and derived only when
  the user types a label. Posts without a label use a fresh random key. Trip keys are not linkable
  across boards.
- **Follow list:** board entries live in the encrypted key-file section with the channels (D2),
  not in plain storage. (Its size is the problem: BF-3.)
- **OPFS paths:** board names and CIDs come from this tab's own Rust state, never from a reader's
  input, and `getDirectoryHandle`/`getFileHandle` refuse `/`, `..` and empty names, so no traversal
  into `channels/` or `mirrors/` is possible. `drop` is not wired to any UI.
- **Lab hooks:** `ephemBoards` is exposed only if `globalThis.ephemTorLab` exists before the page
  runs (`boards.js:58`). The CSP blocks page scripts, and no HTML injection exists that could
  DOM-clobber it.
- **Framing:** `main()` refuses to run in a frame (W-9), before `boards.init`, so owner actions
  cannot be clickjacked by another site.
- **PoW wasm:** compiled once from the SHA-384-pinned bytes; Workers get the compiled `Module`,
  not a URL. Buffers move by transfer, and each block has its own `ArrayBuffer`, so the transfer
  list has no duplicates.
- **Mode and network:** reads, `/pow` and submits go only to syntax-checked onions through Tor. The
  submit uses a fresh isolation group (G.6.1, A-M9).

## Recommended next audits

1. The host side of `/pow`/`/submit` under a flood, in the Tor lab (`crates/board::gateway`,
   `pipeline`), once BF-2's client cap exists, to check the grace rule against the cap.
2. Reader verify for every manifest string (mirrors, see-also, about) and every catalog field:
   length and character class (BF-3/BF-4 point at the absent checks in `verify.rs`).
3. `channels.js`'s own localStorage and OPFS use under quota exhaustion (BF-4 showed the swallowed
   failures).
4. The plain HTML page of a board (`crates/board/src/page.rs`). See-also onions are escaped
   (`esc`, `:133-144`, glanced at only), but they reach the page unchecked, as an `http://<text>/`
   href. A mirror serving another board's page is the case to test.
