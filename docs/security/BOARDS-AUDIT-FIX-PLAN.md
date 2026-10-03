<!-- SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0 -->
<!-- Copyright 2026 Anton (darkcite) -->
# Boards audit 2026-10-03: fix plan

Reports: [core (BC)](AUDIT-2026-10-03-boards-core.md), [browser Rust (BW)](AUDIT-2026-10-03-boards-web-rust.md),
[front end (BF)](AUDIT-2026-10-03-boards-front.md). 33 findings: 3 high, 13 medium, 13 low, 4 info.

**Status (2026-10-03): done** except BF-10 (accepted until the spike measurements); BW-1 and BC-5 mitigated, BW-3 partly fixed by design (see each report's Fixes section).

Rules: each fix gets a test that fails before it (native where possible, the lab for the wasm
paths), the report's status table is updated with the commit, `docs/BOARDS.md` is corrected where the
design or its "as built" note changes. Boards v1 has no users yet beyond the owner's tests, so format
changes (manifest `host`, capcode prefix) are made now, without a migration.

## Batch A: `crates/board` (native tests)

| Order | Finding | Fix |
|---|---|---|
| A1 | BC-1 high | `Board::delete` and `Host::post_of` search the archive; a deleted post in an archived thread re-encodes that thread (tombstone, hash into `dels`); deleting an archived OP drops the archive entry |
| A2 | BC-2 high | Thread-budget refusals raise **thread** effort only; reply effort follows posts-cap pressure only. The grace floor is the previous epoch's advertised value, not the within-epoch minimum |
| A3 | BC-3 | `accept` refuses a post whose hash is in `dels` (hash set beside the list); capcode posts get their own signing prefix; `verify::read` blanks `sub`/`ex` of a catalog entry whose OP is deleted |
| A4 | BC-4 | Deletion entries younger than record validity + margin are never evicted by count; one entry per deleted thread (its OP) instead of one per post |
| A5 | BC-9 | No capacity from block fields (`page.rs`); `verify::read` checks catalog integers against their limits whether or not the thread is held |
| A6 | BC-5 | A trip counts as known only after posting in ≥ 2 distinct hours; regulars (older than 24 h) are evicted last |
| A7 | BC-6 | Held queue: per-key and per-body duplicates refused; when full, the lowest effort is evicted (as the publish ring), not every newcomer refused |
| A8 | BC-12 | The replay slot records the outcome: a retry after a dropped body or a `Busy` reads the body again / answers `Busy`, not `409` |
| A9 | BC-11, BC-13, BC-14 + BF-3 (reader) | `checked_add` on epochs; random 24-byte `own` nonce; mirrors and see-also verified as v3 onions (decode, version, checksum) by the owner **and** by `verify::manifest` |
| A10 | BF-1 (format) | The manifest signs the host onion (`host`); set by `create`/`take_over` |
| A11 | BC-8, BC-10, BW-5 | Per-thread encoded cache with a dirty flag (publish copies only what changed); mass delete with one index and one pass; `load` builds the block map once; plain pages cached per root |
| A12 | BC-7 | A byte counter in `Board`; `make_room` prunes by bytes (archive first); `Busy` at the per-board budget |

## Batch B: `crates/channel-web`, `crates/tor` (traces → lab checks)

| Order | Finding | Fix |
|---|---|---|
| B1 | BW-1 high | One deadline per slot from accept (head + header + body), a 16 KiB/s floor after 2 s, a drain-rate bound on writes; separate pools (4 submit, 12 read); ≤ 2 slots per circuit where the stream exposes it |
| B2 | BW-2 | `take_over(…, floor_seq, root)`: all sources in parallel, the newest wins, refused below the vault's sequence; completeness required (every catalog thread with its chunks, the `own` block when linked); otherwise an explicit "continue without" |
| B3 | BW-3 | 404 sets `vault_known` only when nothing newer is known; a sticky `vault_newer` refuses every publish |
| B4 | BW-4, BW-7 | Thread CARs must hold the complete thread; pull and fence: all sources in parallel, short per-source timeouts, `Ok(None)` moves on; `mirror()` keeps the newest |
| B5 | BW-8, BW-9 | Identity generation counter checked after every await in `take_over`/`mirror`; `forget_identity` drops board mirrors and `known_dels` |
| B6 | BW-6 | Brokers kept per bridge line: a fingerprint is offered only to its own line's brokers |

## Batch C: `app/` (harness + lab)

| Order | Finding | Fix |
|---|---|---|
| C1 | BF-1 (page) | Post only to the signed `host`; `o=`/`m=` are read hints; follow entries store the signed host first. The online notice (F.7) probes the signed host too |
| C2 | BF-2 | Client maximum effort (`E_BOARD_POW_TOO_HIGH`), estimate shown; solves cancellable and bound to their box |
| C3 | BF-3 (page), BF-4, BF-5 | Follow entry = host + current signed mirrors (replace, ≤ 9); `saveFollows` reports a refused section; view cache: small (title, sequence, first catalog rows), LRU 8 boards, labelled "last verified copy"; for a saved identity the cache and mirror/IPFS lists live in a key-file section, mirror seeds derived from the identity; cleared on sign-out |
| C4 | BF-6 | Workers fetched with SRI from a `<meta>` hash, started from `blob:` (inherit the CSP); fail closed |
| C5 | BF-7, BF-8, BF-9 | Board fingerprint (end of the name) on rows, links, header; trip warning once per board and the field cleared on board change; bulk moderation confirmed and undoable, no re-render under the pointer |
| C6 | BF-10 | Spike pages removed from the published branch once B-P1b/B-P11 are measured (waiting on the owner's runs): documented |

## Verification

Native: `cargo test --release` for board, channel, crypto, tor; clippy. Wasm: `./build.sh`. Lab on
fresh networks: board 21+, board UI 13+, board devices 8+, vault, channel, online notice, bridges,
plus new lab checks for B1 (a slow-loris client) and B2 (a takeover from a stale mirror).
