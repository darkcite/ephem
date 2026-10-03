// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
// Copyright 2026 Anton (darkcite)
//! BD-2 (docs/BOARDS.md G.16.3): the submit format, the host's intake and the proof of work,
//! from a poster's bytes to a published post.

use ed25519_dalek::SigningKey;
use ephem_board::board::{Board, cap};
use ephem_board::pipeline::{Efforts, Intake, Next, PowInfo, Refusal, caps, submission};
use ephem_board::post::Signed;
use ephem_board::pow::{self, Challenge};
use ephem_board::submit::{HEADER_LEN, Header, kind};
use ephem_channel::cid::Cid;
use equix::SolverMemory;
use std::alloc::{GlobalAlloc, Layout, System};
use std::sync::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering};

struct Counting;
static ALLOCS: AtomicUsize = AtomicUsize::new(0);
unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, l: Layout) -> *mut u8 {
        ALLOCS.fetch_add(1, Ordering::Relaxed);
        unsafe { System.alloc(l) }
    }
    unsafe fn dealloc(&self, p: *mut u8, l: Layout) {
        unsafe { System.dealloc(p, l) }
    }
}
#[global_allocator]
static A: Counting = Counting;
static SERIAL: Mutex<()> = Mutex::new(());

const NOW: u64 = 1_790_000_000;
const SECRET: [u8; 32] = [8; 32];
const LOW: Efforts = Efforts { reply: 1, thread: 1 };

struct Poster {
    key: SigningKey,
}

impl Poster {
    fn new(i: u8) -> Self {
        Poster { key: SigningKey::from_bytes(&[i; 32]) }
    }

    /// Solves for (board, thread) at `info`, drawing nonces until one solves, then signs.
    fn post(&self, name: &Cid, info: &PowInfo, t: u64, body: &str, effort: u32) -> Vec<u8> {
        let k = self.key.verifying_key().to_bytes();
        let kind = if t == 0 { kind::THREAD } else { kind::REPLY };
        let mut n = [0u8; 16];
        n[..8].copy_from_slice(&(body.len() as u64 ^ u64::from(info.epoch)).to_le_bytes());
        let mut c = Challenge::new(&name.to_bytes(), &info.seed, kind, t, &k, &n, effort).unwrap();
        let mut mem = SolverMemory::new();
        let solution = loop {
            if let Some(s) = pow::attempt(&c, effort, &mut mem) {
                break s;
            }
            n[15] = n[15].wrapping_add(1);
            n[14] = n[14].wrapping_add(u8::from(n[15] == 0));
            c.set_nonce(&n);
        };
        let s = Signed { b: name.to_text(), t, k, n, sub: if t == 0 { "Subject".into() } else { String::new() }, body: body.into(), sage: false, e: info.epoch, trip: false };
        submission(&s, &self.key, effort, solution).unwrap()
    }
}

fn header(bytes: &[u8]) -> Header {
    Header::parse(bytes[..HEADER_LEN].try_into().unwrap()).unwrap()
}

/// The gateway's flow for one request: header, body, caps, accept, publish.
fn submit(intake: &mut Intake, board: &mut Board, bytes: &[u8], now: u64) -> Result<u64, Refusal> {
    let h = header(bytes);
    let slot = match intake.check_header(&h, bytes.len(), now)? {
        Next::Done { no, .. } => return Ok(no),
        Next::ReadBody(slot) => slot,
    };
    let s = intake.check_body(&h, &bytes[HEADER_LEN..])?;
    let (t, _) = intake.admit(&h, slot, now)?;
    let no = board.accept(s, h.sig, cap::ANON, now).map_err(|_| Refusal::Refused)?;
    let (root, _) = board.build(now);
    board.record(&root, now * 1000);
    intake.published(&t, no, board.seq);
    let mut out = [None; caps::RING];
    intake.drain(&mut out);
    Ok(no)
}

fn setup() -> (Board, Intake) {
    let board = Board::new(&[4; 32], &ephem_board::onion::address(&[0xAA; 32]), "/t/", "", "", NOW).unwrap();
    let intake = Intake::new(board.name(), SECRET, LOW, NOW);
    (board, intake)
}

#[test]
fn a_post_goes_through_and_a_retry_is_idempotent() {
    let _one = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let (mut b, mut i) = setup();
    let name = b.name().clone();
    let info = i.pow_info(NOW);
    let op = Poster::new(1).post(&name, &info, 0, "first thread", 1);
    let no = submit(&mut i, &mut b, &op, NOW).unwrap();
    assert_eq!(no, 1);
    let reply = Poster::new(2).post(&name, &info, no, "a reply", 1);
    assert_eq!(submit(&mut i, &mut b, &reply, NOW + 1), Ok(2));
    // The stream dropped after acceptance and the poster sends the same bytes again.
    assert_eq!(submit(&mut i, &mut b, &reply, NOW + 2), Ok(2), "the original number, not a refusal");
    assert_eq!(b.threads[0].entries.len(), 2);
}

#[test]
fn every_refusal() {
    let _one = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let (mut b, mut i) = setup();
    let name = b.name().clone();
    let info = i.pow_info(NOW);
    let good = Poster::new(1).post(&name, &info, 0, "op", 1);
    let h = header(&good);
    // Content-Length that does not match the header.
    assert_eq!(i.check_header(&h, good.len() + 1, NOW), Err(Refusal::Refused));
    // An epoch two back.
    assert_eq!(i.check_header(&h, good.len(), NOW + 2 * pow::EPOCH_S), Err(Refusal::Pow));
    // Less effort than advertised.
    let mut i2 = Intake::new(b.name(), SECRET, Efforts { reply: 1, thread: 4 }, NOW);
    i2.pow_info(NOW);
    assert_eq!(i2.check_header(&h, good.len(), NOW), Err(Refusal::Pow));
    // A wrong solution (another nonce).
    let mut bad = good.clone();
    bad[60] ^= 1;
    assert_eq!(i.check_header(&header(&bad), bad.len(), NOW), Err(Refusal::Pow));
    // Paused, and new threads closed.
    i.paused = true;
    assert_eq!(i.check_header(&h, good.len(), NOW), Err(Refusal::Paused));
    i.paused = false;
    i.threads_closed = true;
    assert_eq!(i.check_header(&h, good.len(), NOW), Err(Refusal::Paused));
    i.threads_closed = false;
    // Body: a changed byte breaks the hash. The header committed to the body's hash, so the same
    // header may send its body again (a stream that dropped, BC-12); the same solution with
    // another body hash is a replay.
    let mut body_bad = good.clone();
    *body_bad.last_mut().unwrap() ^= 1;
    let Next::ReadBody(slot) = i.check_header(&header(&body_bad), body_bad.len(), NOW).unwrap() else { panic!() };
    assert_eq!(i.check_body(&header(&body_bad), &body_bad[HEADER_LEN..]), Err(Refusal::Refused));
    assert_eq!(i.check_header(&h, good.len(), NOW), Ok(Next::ReadBody(slot)), "the body again (BC-12)");
    let mut other_body = h;
    other_body.h[0] ^= 1;
    assert_eq!(i.check_header(&other_body, good.len(), NOW), Err(Refusal::Refused), "the same solution with another body: a replay");
    // A header whose k is not the signer of s.
    let other = Poster::new(3).post(&name, &info, 0, "op2", 1);
    let mut mixed = other.clone();
    mixed[28..60].copy_from_slice(&good[28..60]);
    assert!(i.check_header(&header(&mixed), mixed.len(), NOW).is_err(), "the PoW is bound to k");
    // Actions and reports wait for BD-5.
    let mut action = Poster::new(4).post(&name, &info, 1, "a reply-shaped action", 1);
    action[4] = kind::ACTION;
    assert_eq!(i.check_header(&header(&action), action.len(), NOW), Err(Refusal::Refused));
    let _ = &mut b;
}

#[test]
fn a_raised_effort_does_not_void_solved_work() {
    let _one = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let (mut b, mut i) = setup();
    let name = b.name().clone();
    let info = i.pow_info(NOW);
    let post = Poster::new(1).post(&name, &info, 0, "solved at effort 1", 1);
    // A flood doubles the effort while the poster types.
    for _ in 0..3 {
        i.tick(NOW + 30);
    }
    let _ = i.pow_info(NOW + 30); // what new posters now see
    assert!(submit(&mut i, &mut b, &post, NOW + 40).is_ok(), "accepted at the lowest effort advertised this epoch");
}

#[test]
fn higher_effort_wins_a_full_ring() {
    let _one = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let (b, mut i) = setup();
    let name = b.name().clone();
    let info = i.pow_info(NOW);
    let mut b = b;
    let op = submit(&mut i, &mut b, &Poster::new(1).post(&name, &info, 0, "op", 1), NOW).unwrap();
    // Fill the ring with effort-1 replies (admitted, not yet published).
    let mut admitted = 0;
    for n in 0..caps::RING as u8 {
        let p = Poster::new(10 + n).post(&name, &info, op, &format!("r{n}"), 1);
        let h = header(&p);
        let Next::ReadBody(slot) = i.check_header(&h, p.len(), NOW + 1).unwrap() else { panic!() };
        i.check_body(&h, &p[HEADER_LEN..]).unwrap();
        if i.admit(&h, slot, NOW + 1).is_ok() {
            admitted += 1;
        }
    }
    assert_eq!(admitted, caps::RING);
    let low = Poster::new(200).post(&name, &info, op, "low", 1);
    let h = header(&low);
    let Next::ReadBody(slot) = i.check_header(&h, low.len(), NOW + 1).unwrap() else { panic!() };
    assert_eq!(i.admit(&h, slot, NOW + 1).err(), Some(Refusal::Busy), "the same effort waits");
    let high = Poster::new(201).post(&name, &info, op, "high", 4);
    let h = header(&high);
    let Next::ReadBody(slot) = i.check_header(&h, high.len(), NOW + 1).unwrap() else { panic!() };
    let (_, evicted) = i.admit(&h, slot, NOW + 1).unwrap();
    assert_eq!(evicted.map(|t| t.effort), Some(1), "a lower effort makes room");
    let mut out = [None; caps::RING];
    assert_eq!(i.drain(&mut out), caps::RING);
    assert_eq!(out[0].map(|t| t.effort), Some(4), "published highest effort first");
}

#[test]
fn thread_budget_and_the_panic_mode_close_new_threads() {
    let _one = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let (mut b, mut i) = setup();
    let name = b.name().clone();
    let info = i.pow_info(NOW);
    submit(&mut i, &mut b, &Poster::new(1).post(&name, &info, 0, "a", 1), NOW).unwrap();
    assert_eq!(submit(&mut i, &mut b, &Poster::new(2).post(&name, &info, 0, "b", 1), NOW + 10), Err(Refusal::Busy), "one new thread per 2 minutes");
    // The budget stays exhausted for 30 minutes: new threads close (R8), replies still work.
    let mut t = NOW + 10;
    while t < NOW + 10 + caps::PANIC_THREADS_S + 120 {
        t += 60;
        let info = i.pow_info(t);
        // Several new threads a minute, more than the budget lets through. The caps come after the
        // proof of work, so the flood goes straight to them (solving at the rising efforts would
        // only make the test slow).
        let flood = header(&Poster::new(3).post(&name, &info, 0, "flood", info.effort_thread));
        for k in 0..3u64 {
            let _ = i.admit(&flood, 0, t + k);
        }
        let mut out = [None; caps::RING];
        i.drain(&mut out);
        i.tick(t);
        if i.threads_closed {
            break;
        }
    }
    assert!(i.threads_closed && i.closed_notice, "closed by itself, owner notified");
    let info = i.pow_info(t);
    assert!(!info.threads_open);
    assert_eq!(submit(&mut i, &mut b, &Poster::new(250).post(&name, &info, 0, "late thread", info.effort_thread), t), Err(Refusal::Paused));
    assert!(submit(&mut i, &mut b, &Poster::new(251).post(&name, &info, 1, "a reply still works", info.effort_reply), t).is_ok());
    i.reopen_threads();
    assert!(i.pow_info(t).threads_open);
}

#[test]
fn pow_info_round_trip() {
    let _one = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let (_, mut i) = setup();
    let p = i.pow_info(NOW);
    let mut b = [0u8; PowInfo::LEN];
    p.write(&mut b);
    assert_eq!(PowInfo::read(&b), p);
    assert_eq!(PowInfo::LEN, 58);
}

#[test]
fn the_refusal_path_does_not_allocate() {
    let _one = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let (b, mut i) = setup();
    let name = b.name().clone();
    let info = i.pow_info(NOW);
    let good = Poster::new(1).post(&name, &info, 0, "op", 1);
    let mut bad_pow = good.clone();
    bad_pow[60] ^= 1;
    let (hg, hb) = (header(&good), header(&bad_pow));
    let before = ALLOCS.load(Ordering::Relaxed);
    assert_eq!(i.check_header(&hg, good.len() + 1, NOW), Err(Refusal::Refused));
    assert_eq!(i.check_header(&hg, good.len(), NOW + 3 * pow::EPOCH_S), Err(Refusal::Pow));
    i.paused = true;
    assert_eq!(i.check_header(&hg, good.len(), NOW), Err(Refusal::Paused));
    i.paused = false;
    let cheap = ALLOCS.load(Ordering::Relaxed) - before;
    assert_eq!(cheap, 0, "header refusals before the proof of work allocate nothing");
    let before = ALLOCS.load(Ordering::Relaxed);
    assert_eq!(i.check_header(&hb, bad_pow.len(), NOW), Err(Refusal::Pow));
    let pow_allocs = ALLOCS.load(Ordering::Relaxed) - before;
    println!("allocations for one failed Equi-X verification: {pow_allocs}");
    assert!(pow_allocs <= 4, "Equi-X verification: {pow_allocs} allocations");
}

#[test]
fn header_parser_fuzz() {
    let _one = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let iters: u64 = std::env::var("FUZZ_ITERS").ok().and_then(|v| v.parse().ok()).unwrap_or(1_000_000);
    let mut x: u64 = 0x9E37_79B9_7F4A_7C15;
    let mut b = [0u8; HEADER_LEN];
    b[..4].copy_from_slice(b"EPB2");
    for _ in 0..iters {
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        let at = 4 + (x as usize % (HEADER_LEN - 4));
        b[at] = (x >> 32) as u8;
        if let Some(h) = Header::parse(&b) {
            let mut again = [0u8; HEADER_LEN];
            h.write(&mut again);
            assert_eq!(Header::parse(&again), Some(h));
        }
    }
}
