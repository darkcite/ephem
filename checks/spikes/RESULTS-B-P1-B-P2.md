<!-- SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0 -->
<!-- Copyright 2026 Anton (darkcite) -->
# Spikes B-P1 and B-P2 (docs/BOARDS.md G.8, G.16), 2026-09-29

Container: 4 vCPU, Intel Xeon @ 2.10 GHz, Linux 6.18; rustc/cargo 1.94.1; Node 22.22.2;
headless Chromium 141.0.7390.37 (Playwright, `navigator.hardwareConcurrency` = 4).
Measured in Chromium only: Firefox, Safari and iPhone numbers (asked for by G.16) are **not** measured.

## B-P1: Equi-X speed

Crate: `equix` **0.7.0** + `hashx` 0.9.1 from crates.io (the version the vendored
`tor-hsservice` 0.46.0 declares; a fresh lock of arti picks 0.7.1, whose only change is a lint attribute).
Code: `checks/spikes/equix_bench/` (standalone crate, empty `[workspace]`).
Challenge = 100 bytes (the size of Tor's v1 challenge), distinct nonce per attempt; 0 of 200
challenges were skipped for `ProgramConstraints`. **Solutions per attempt: 2.16** (432 / 200).

Native (`cargo run --release -- 200 4`), one Equi-X attempt, solver memory reused:

| hashx runtime | build program | solve (mean / median / p95) | verify one solution (incl. program build) | 4 threads |
|---|---|---|---|---|
| compiled (JIT, x86_64) | 0.09 ms | 7.80 / 7.63 / 9.30 ms | 0.079 ms | 434 attempts/s = 903 solutions/s |
| interpreted | 0.07 ms | 51.8 / 51.1 / 63.0 ms | 0.069 ms | 75 attempts/s = 156 solutions/s |

Fresh `SolverMemory` per call (as `equix::solve` does) adds ~0.2 ms. The JIT is ~6.6x faster.

wasm32-unknown-unknown (interpreted hashx; equix built with `default-features = false`, no
dynasmrt), 55 KB wasm, `node run.mjs 200 4` in headless Chromium, timed with `performance.now()`
(coarsened to 0.1 ms):

| | mean | median | p95 | min / max |
|---|---|---|---|---|
| attempt (program build + solve), main thread | **88.6 ms** | 83.6 ms | 122.5 ms | 63.9 / 130.8 ms |
| verify one solution (incl. program build) | ~0.1 ms (at timer resolution) | | | |

- One core: **11.3 attempts/s = 24.4 solutions/s** (41.0 ms per solution). wasm is 1.7x slower
  than native interpreted and 11x slower than native compiled (what an attacker runs).
- `+simd128` build: 82.4 ms mean on 100 attempts: no meaningful change.
- Web Workers (module workers, each with its own wasm instance and 1.8 MB solver memory):
  2 workers 20.8 attempts/s, **4 workers 42.2 attempts/s = 79.2 solutions/s** (3.7x one core,
  per-attempt time unchanged at 88.7 ms mean). The work is embarrassingly parallel over nonces.

### Effort to time

Tor v1 (`tor-hscrypto` 0.46.0 `src/pow/v1/solve.rs:108-129`, `challenge.rs:127-139`): each nonce
gives one Equi-X attempt; every solution of that attempt is tested with
`u32::from_be(BLAKE2b-32(challenge ‖ solution)).checked_mul(effort)` succeeding, i.e. with
probability ~1/effort; the nonce is incremented until one passes. G.8 uses the same predicate.
So **expected solutions = E, expected attempts = E / 2.16**, and the time is ~exponential:
median = ln 2 x mean, p95 = 3 x mean.

Here, one core of wasm: mean = 41.0 ms x E, median = 28.4 ms x E, p95 = 123 ms x E.

| Target median | E on this container (1 core) | E on a phone 2-4x slower (estimate, not measured) |
|---|---|---|
| ~10 s (G.8) | **~350** (mean 14.4 s, p95 43 s) | ~90-175 |
| ~3 s (G.16 B-P1 text) | ~105 | ~25-55 |

With 4 workers the same wall time buys ~3.7x the effort. A native attacker with the JIT on the
same 4 cores does 903 solutions/s: **~37x a one-core browser poster** (G.8's "cost, not identity").

### Consequences for G.8
- **An attempt cannot be sliced to ~20 ms.** One `solve` is 64-131 ms here and cannot be
  interrupted (arti's own docs say "roughly 10 ms compiled, 250 ms interpreted"); on a phone
  expect ~170-500 ms per blocking call. On the main thread every attempt is a long frame. A
  dedicated Web Worker (or several) is needed for a smooth UI; G.8/G.6 "the app has no workers"
  should change for PoW.
- Host verification is cheap: ~0.1 ms per solution in wasm (~10 000/s).

## B-P2: Tor onion-service PoW (hs-pow v1, prop 327/362) in the vendored arti 0.46.0

Feature: **`hs-pow-full`** on `arti-client` (`vendor/arti-client/Cargo.toml:156`, forwards to
`tor-hsclient?/hs-pow-full` and `tor-hsservice?/hs-pow-full`), `tor-hsservice`
(`vendor/tor-hsservice/Cargo.toml:85`, pulls `equix` 0.7 with default features, i.e. the
dynasmrt JIT), `tor-hsclient`, `tor-hscrypto`, `tor-netdoc`, `tor-cell`. It is marked
experimental (`__is_experimental`). Our build (`crates/tor/Cargo.toml`) does **not** enable it:
`equix` is absent from `Cargo.lock`; the stubs `pow/v1_stub.rs` are compiled.

### Service side (`vendor/tor-hsservice/src`) — implemented, not wasm-ready
- Config: `enable_pow` (default false, `config.rs:71`), `pow_rend_queue_depth` (8192, `:85`),
  `disable_pow_compilation` (`:111`); `enable_pow = true` without the feature is a config error
  (`config.rs:317-321`). `enable_pow` cannot change while running (TODO #2082, `config.rs:248`).
- Descriptor: `publish/descriptor.rs:166-171` adds `pow-params v1` (seed, expiry, suggested effort)
  when `enable_pow`; seeds per time period with rotation in `pow/v1.rs:609-667` (`get_pow_params`)
  and `:502` (`rotate_seeds_if_expiring`), persisted through `storage_handle("pow_manager")`.
- INTRODUCE2 verification: `pow/v1.rs:672-711` (`check_solve`: nonce replay log per seed, then
  `Verifier::check`); verifier built with `TryCompile` unless `disable_pow_compilation` (`:456-478`).
- Priority by effort: `RendRequestOrdByEffort` (`:741-822`), a `BTreeSet` queue popping the
  highest effort first, effort capped at the consensus `HiddenServiceProofOfWorkV1MaxEffort`
  (default 10 000), overflow drops the lowest (`:1168-1176`).
- Suggested-effort controller per prop 362 (`update_suggested_effort`, `:968-1036`).
- Intro-point DoS limits (independent of PoW, no feature): `rate_limit_at_intro`
  (`config.rs:56`, sent as `DOS_PARAMS` in ESTABLISH_INTRO) and `max_concurrent_streams_per_circuit`
  (`:66`). Our service (`crates/tor/src/web/mod.rs:159`) sets neither.
- **Blocker for wasm:** `PowManager::new` (called unconditionally at service launch,
  `lib.rs:335`) calls `start_accept_thread` (`pow/v1.rs:315`), which runs `accept_loop` and
  `expire_old_requests_loop` under `runtime.spawn_blocking` (`:937`, `:954`) and those call
  `runtime.reenter_block_on(...)` (`:1055`, `:1105`, `:1208`, `:1236`). Our browser runtime runs
  `spawn_blocking` inline and **panics in `reenter_block_on`** (`crates/tor/src/web/rt.rs:55-69`).
  So with `hs-pow-full` the service would panic at launch in the tab **even with `enable_pow = false`**.
  The PoW replay log is already ephemeral on wasm32 (our `replay.rs:145-151` patch covers it).

### Client side (`tor-hsclient` 0.46.0, crates.io) — implemented, not wasm-ready
- `connect.rs:929` builds `HsPowClient` from the descriptor, `:986` solves before each
  INTRODUCE1, `:1093` raises effort on failure (x1.5, min 8, max 10 000: `pow/v1.rs:18-36`).
- `pow/v1.rs:87-113` solves on **`std::thread::spawn`**, which panics on wasm32-unknown-unknown.
  Effort 0 (the suggested effort of an idle service) returns early without a thread; any
  service asking for effort > 0 would panic our tab. Without the feature (today) the stub
  ignores pow-params and introduces without PoW (lowest priority at a PoW-enabled service under load).

### Build check
`checks/spikes/hspow_check/` (standalone, same features as `crates/tor` + `hs-pow-full`, the
vendored `[patch.crates-io]`): `cargo check --target wasm32-unknown-unknown` **succeeds** (1 m 28 s;
equix 0.7.1, dynasmrt/memmap2 compile but the JIT code is x86_64/aarch64-only). It compiles; it would
not run (above). Native check not run; lab flood test (G.16 "done when") not done.

### What enabling it would take
1. `crates/tor/Cargo.toml`: add `hs-pow-full` to `arti-client` features (optionally
   `equix` without default features to drop dynasmrt from the wasm build).
2. Vendor patch in `tor-hsservice/src/pow/v1.rs` (wasm32 only): turn `accept_loop` and
   `expire_old_requests_loop` into async tasks (`runtime.spawn` + `.await` instead of
   `spawn_blocking` + `reenter_block_on`). Verification inline on the page is ~0.1 ms, acceptable.
3. Vendor `tor-hsclient` (new vendored crate) and patch `pow/v1.rs` solve for wasm32: run
   `Solver::run_step` in an async loop yielding between steps (~90 ms each here), or in a Worker.
4. Set `enable_pow(true)` (+ `rate_limit_at_intro`) in `crates/tor/src/web/mod.rs`, then the lab
   flood test from G.16.
