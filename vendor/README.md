# Vendored crates (patched)

Four arti 0.46.0 crates, copied from crates.io and patched through `[patch.crates-io]` in the
workspace `Cargo.toml`. Both patches affect **only** `wasm32-unknown-unknown` (the browser);
native builds compile the upstream code paths unchanged. arti's core crates (protocol,
circuits, channels, guards, onion services, crypto) are used unmodified from crates.io.

Drop a patch as soon as upstream arti covers the case (both spots are marked "TODO wasm" or
use SQLite unconditionally in 0.46).

| Crate | Patch | Why |
|---|---|---|
| `arti-client` | `src/client.rs` `statemgr_from_config`: on wasm32, an in-memory `TestingStateMgr` (lock taken at once) instead of `unimplemented!()` | A browser tab has no state directory. Guard and circuit-timeout state lives in memory for the session |
| `arti-client` | `wait_for_stop` (experimental API) compiled out on wasm32 | It waits on the on-disk state manager's lock |
| `tor-persist` | new `src/state_dir_mem.rs`, used as `state_dir` on wasm32 (`lib.rs`) | Onion services keep per-instance state in a locked directory. In a tab it is a map of JSON values for the life of the session (an Ephem Tor session is ephemeral; the onion key is re-derived from the identity seed) |
| `tor-hsservice` | `replay.rs` `new_logged` returns an ephemeral replay log on wasm32; `ipt_mgr.rs` skips expiring replay-log files there | Replay logs are files upstream; in a tab they live as long as the service |
| `tor-dirmgr` | new `src/storage/memory.rs` (`MemoryStore`); `config.rs` `open_store` returns it on wasm32; `err.rs`/`storage.rs` gate the SQLite error variant, its match arms and the module; `lib.rs` allows the then-unused SQLite helpers on wasm32; `Cargo.toml` makes `rusqlite` a non-wasm dependency | Upstream always opens a SQLite cache (C library). The memory store keeps the same data with the same selection and expiry rules; everything in it is public directory data |

To update: copy the new upstream versions over these directories and re-apply the marked
"Ephem patch" hunks (search for `Ephem patch`).
