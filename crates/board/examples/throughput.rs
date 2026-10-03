// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
// Copyright 2026 Anton (darkcite)
//! Spike B-P6 (docs/BOARDS.md G.16.2), the CPU part: how many posts a host verifies, admits,
//! builds and signs per second, natively, on a board that fills up to its caps (150 threads).
//! `cargo run --release -p ephem-board --example throughput`; the wasm figure is about 2–3×
//! slower (B-P1). The OPFS part is `checks/spikes/board_store/run.mjs`.

use ed25519_dalek::SigningKey;
use ephem_board::board::Board;
use ephem_board::host::Host;
use ephem_board::pipeline::{Efforts, Intake, Next, submission};
use ephem_board::post::Signed;
use ephem_board::pow::{self, Challenge};
use ephem_board::submit::{HEADER_LEN, kind};
use equix::SolverMemory;
use std::time::Instant;

const T0: u64 = 1_790_000_000;

fn main() {
    let board = Board::new(&[1; 32], &ephem_board::onion::address(&[0xAA; 32]), "Throughput", "", "", T0).unwrap();
    let name = board.name().clone();
    let mut h = Host::new(board, Intake::new(&name, [2; 32], Efforts { reply: 1, thread: 1 }, T0), [9; 32], T0 * 1000);
    // 150 threads by the owner (no intake), then replies from posters through the intake.
    for i in 0..150 {
        h.post_owner(0, &format!("thread {i}"), "op", false, T0 + i).unwrap();
    }
    let mut ms = T0 * 1000 + 3_600_000;
    let t = Instant::now();
    h.publish(ms);
    println!("first build of 150 threads: {:.1} ms", t.elapsed().as_secs_f64() * 1e3);

    // Intake: header (PoW verify), body (CBOR, signature), admit. Published in batches of up to
    // 60 (the ring holds 64), as the host's publish loop does at most once a second. Each batch
    // is solved in its own epoch first (the posters' work, not timed).
    let n = 2_040usize;
    let mut mem = SolverMemory::new();
    let (mut intake_s, mut publish_s, mut accepted, mut publishes) = (0f64, 0f64, 0usize, 0usize);
    for start in (0..n).step_by(60) {
        let now = ms / 1000;
        let info = h.intake.pow_info(now);
        let chunk: Vec<Vec<u8>> = (start..start + 60)
            .map(|i| {
                let key = SigningKey::from_bytes(&[(i % 250) as u8 + 3; 32]);
                let k = key.verifying_key().to_bytes();
                let thread = 1 + (i as u64 % 150);
                let mut nn = [0u8; 16];
                nn[..8].copy_from_slice(&(i as u64).to_le_bytes());
                let mut c = Challenge::new(&name.to_bytes(), &info.seed, kind::REPLY, thread, &k, &nn, info.effort_reply).unwrap();
                let sol = pow::solve(&mut c, &mut nn, info.effort_reply, &mut mem, 10_000).unwrap();
                let s = Signed { b: name.to_text(), t: thread, k, n: nn, sub: String::new(), body: format!("reply {i} {}", "x".repeat(200)), sage: false, e: info.epoch, trip: false };
                submission(&s, &key, info.effort_reply, sol).unwrap()
            })
            .collect();
        let t = Instant::now();
        for b in &chunk {
            let head: &[u8; HEADER_LEN] = b[..HEADER_LEN].try_into().unwrap();
            let Ok((hd, Next::ReadBody(slot))) = h.submit_header(head, b.len(), now) else { continue };
            if h.submit_body(&hd, slot, &b[HEADER_LEN..], now).is_ok() {
                accepted += 1;
            }
        }
        intake_s += t.elapsed().as_secs_f64();
        ms += 61_000; // a new minute: the posts cap (120/min) does not bind the measurement
        let t = Instant::now();
        h.publish(ms);
        publish_s += t.elapsed().as_secs_f64();
        publishes += 1;
        h.intake.tick(ms / 1000);
    }
    println!("intake: {accepted} posts in {:.0} ms = {:.0} posts/s ({:.2} ms each)", intake_s * 1e3, accepted as f64 / intake_s, intake_s * 1e3 / accepted as f64);
    println!("publish (build changed blocks + sign): {publishes} batches, {:.1} ms each", publish_s * 1e3 / publishes as f64);
    println!("end to end: {:.0} posts/s with a publish per batch of 60", accepted as f64 / (intake_s + publish_s));
    println!("blocks served: {}", h.served.blocks().count());
}
