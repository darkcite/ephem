// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
// Copyright 2026 Anton (darkcite)
//! The boards core audit (docs/security/AUDIT-2026-10-03-boards-core.md, BC-n; BF-1, BF-3 of
//! the front-end audit): each test failed before its fix.

use ed25519_dalek::SigningKey;
use ephem_board::board::{Board, Entry, cap, del};
use ephem_board::gateway::{self, Answer, Route, SHORT};
use ephem_board::host::Host;
use ephem_board::pipeline::{Efforts, Intake, Next, PowInfo, Refusal, caps, submission};
use ephem_board::post::Signed;
use ephem_board::pow::{self, Challenge};
use ephem_board::submit::{HEADER_LEN, Header, kind};
use ephem_board::{BoardError, limits, onion, verify};
use ephem_channel::car::Block;
use ephem_channel::cbor::Value;
use ephem_channel::cid::{Cid, DAG_CBOR};
use ephem_channel::ipns::{self, Record};
use equix::SolverMemory;

const NOW: u64 = 1_790_000_000;
const SEED: [u8; 32] = [1; 32];
const LOW: Efforts = Efforts { reply: 1, thread: 1 };

fn host_onion() -> String {
    onion::address(&[0xAA; 32])
}

fn board() -> Board {
    Board::new(&SEED, &host_onion(), "/audit/", "", "", NOW).unwrap()
}

fn signed(b: &Board, t: u64, body: &str, i: u64) -> (Signed, [u8; 64]) {
    let mut seed = [0u8; 32];
    seed[..8].copy_from_slice(&i.to_le_bytes());
    seed[31] = 7;
    let key = SigningKey::from_bytes(&seed);
    let s = Signed { b: b.name().to_text(), t, k: key.verifying_key().to_bytes(), n: [i as u8; 16], sub: if t == 0 { format!("T{i}") } else { String::new() }, body: body.into(), sage: false, e: 1, trip: false };
    let sig = s.sign(&key);
    (s, sig)
}

fn put(b: &mut Board, t: u64, body: &str, i: u64, now: u64) -> u64 {
    let (s, sig) = signed(b, t, body, i);
    b.accept(s, sig, cap::ANON, now).unwrap()
}

fn view(b: &mut Board, now: u64, known: &[[u8; 32]]) -> verify::View {
    let (root, blocks) = b.build(now);
    let rec = b.record(&root, now * 1000);
    verify::verify(b.name(), &rec, &blocks, now * 1000, 0, known).unwrap()
}

// ---- BC-1: archived posts can be deleted ----

#[test]
fn bc1_a_post_in_an_archived_thread_can_be_deleted() {
    let mut b = board();
    let t = put(&mut b, 0, "op", 1, NOW);
    let r = put(&mut b, t, "illegal reply", 2, NOW);
    b.prune(t, NOW + 1).unwrap();
    assert!(b.threads.is_empty() && b.archive.len() == 1);
    b.delete(r, del::OWNER, 0, NOW + 2).expect("found in the archive");
    let (_, blocks) = b.build(NOW + 3);
    assert!(!blocks.iter().any(|(_, d)| d.windows(13).any(|w| w == b"illegal reply")), "gone from every served block");
    assert_eq!(b.dels.len(), 1);
    // The archived OP takes the archive entry with it.
    b.delete(t, del::OWNER, 0, NOW + 4).unwrap();
    assert!(b.archive.is_empty());
    assert_eq!(b.delete(r, del::OWNER, 0, NOW + 5), Err(BoardError::NotFound));
}

#[test]
fn bc1_the_owner_finds_and_mass_deletes_archived_posts() {
    let mut b = board();
    let t = put(&mut b, 0, "op", 1, NOW);
    let (s, sig) = signed(&b, t, "from a key", 9);
    let k = s.k;
    let r = b.accept(s, sig, cap::ANON, NOW).unwrap();
    b.prune(t, NOW + 1).unwrap();
    assert_eq!(b.find(r).map(|p| p.s.k), Some(k), "a ban finds the key of an archived post");
    assert_eq!(b.select(|p| p.s.k == k), vec![r], "mass delete by key reaches the archive");
}

// ---- BC-3: a deleted post never comes back; a capcode signature is its own ----

#[test]
fn bc3_a_deleted_post_is_refused_when_resubmitted() {
    let mut b = board();
    let t = put(&mut b, 0, "op", 1, NOW);
    let (s, sig) = signed(&b, t, "doxx", 2);
    let no = b.accept(s.clone(), sig, cap::ANON, NOW).unwrap();
    b.delete(no, del::OWNER, 0, NOW + 1).unwrap();
    assert_eq!(b.accept(s, sig, cap::ANON, NOW + 2), Err(BoardError::Refused));
    // A deleted OP, resubmitted as a new thread.
    let (op, opsig) = signed(&b, 0, "deleted op", 3);
    let t2 = b.accept(op.clone(), opsig, cap::ANON, NOW).unwrap();
    b.delete(t2, del::OWNER, 0, NOW + 1).unwrap();
    assert_eq!(b.accept(op, opsig, cap::ANON, NOW + 2), Err(BoardError::Refused));
}

#[test]
fn bc3_bc4_a_stale_root_with_a_deleted_op_shows_nothing_of_it() {
    let mut b = board();
    let t = put(&mut b, 0, "op text", 1, NOW);
    put(&mut b, t, "a reply", 2, NOW);
    let (root, blocks) = b.build(NOW);
    let old_rec = b.record(&root, NOW * 1000);
    b.delete(t, del::OWNER, 0, NOW + 60).unwrap();
    assert_eq!(b.dels.len(), 1, "one entry for the thread");
    let known: Vec<[u8; 32]> = b.dels.iter().map(|(h, _)| *h).collect();
    // A reader who saw the newer deletion list reads the older root from a stale mirror.
    let v = verify::verify(b.name(), &old_rec, &blocks, NOW * 1000 + 120_000, 0, &known).unwrap();
    assert!(v.catalog[0].sub.is_empty() && v.catalog[0].ex.is_empty(), "no subject or excerpt");
    assert!(v.threads[0].sub.is_empty() && v.threads[0].entries.iter().all(|e| matches!(e, Entry::Tomb { .. })), "every post deleted");
}

// ---- BC-4: young deletions are never evicted by count ----

#[test]
fn bc4_deletions_younger_than_a_record_stay_past_the_soft_cap() {
    let mut b = board();
    let t = put(&mut b, 0, "op", 1, NOW);
    let first = put(&mut b, t, "deleted first", 2, NOW);
    b.delete(first, del::OWNER, 0, NOW).unwrap();
    let h = b.dels[0].0;
    // A flood cleaned up an hour later: more deletions than the soft cap.
    let mut i = 10;
    let mut nos = Vec::new();
    let mut into = t;
    for n in 0..limits::DELS + 10 {
        if n % 400 == 0 {
            i += 1;
            into = put(&mut b, 0, &format!("flood thread {i}"), i, NOW + 3600);
        }
        i += 1;
        nos.push(put(&mut b, into, &format!("flood {i}"), i, NOW + 3600));
    }
    b.delete_many(&nos, del::OWNER, 0, NOW + 3600);
    assert!(b.dels.len() > limits::DELS);
    assert!(b.dels.iter().any(|(x, _)| *x == h), "the first deletion is still listed");
    view(&mut b, NOW + 3600, &[]);
}

// ---- BC-7: the board's bytes are bounded ----

#[test]
fn bc7_the_board_stays_within_its_byte_budget() {
    let mut b = board();
    let body = "x".repeat(limits::BODY);
    let mut i = 0;
    let mut busy = false;
    'fill: for th in 0..limits::THREADS as u64 {
        i += 1;
        let now = NOW + th * 3600; // old enough to prune
        let t = match b.accept(signed(&b, 0, &body, i).0, signed(&b, 0, &body, i).1, cap::ANON, now) {
            Ok(t) => t,
            Err(BoardError::Busy) => {
                busy = true;
                break 'fill;
            }
            Err(e) => panic!("{e:?}"),
        };
        for _ in 0..100 {
            i += 1;
            let (s, sig) = signed(&b, t, &format!("{i} {body}")[..limits::BODY], i);
            match b.accept(s, sig, cap::ANON, now) {
                Ok(_) => {}
                Err(BoardError::Busy) => {
                    busy = true;
                    break 'fill;
                }
                Err(e) => panic!("{e:?}"),
            }
        }
        assert!(b.bytes() <= limits::BYTES, "{} > budget", b.bytes());
    }
    assert!(b.bytes() <= limits::BYTES);
    assert!(busy || b.threads.len() < limits::THREADS, "old threads went to keep the budget");
}

// ---- BC-8: a publish copies only what changed ----

#[test]
fn bc8_one_reply_copies_a_few_blocks_not_every_thread() {
    let b = board();
    let intake = Intake::new(b.name(), [2; 32], LOW, NOW);
    let mut ms = NOW * 1000;
    let mut h = Host::new(b, intake, [9; 32], ms);
    let mut i = 0;
    let mut threads = Vec::new();
    for _ in 0..50 {
        i += 1;
        threads.push(h.post_owner(0, &format!("thread {i}"), "op", false, ms / 1000).unwrap());
        for _ in 0..10 {
            i += 1;
            h.post_owner(*threads.last().unwrap(), "", &format!("reply {i}"), false, ms / 1000).unwrap();
        }
    }
    ms += 2_000;
    h.publish(ms);
    h.take_delta();
    h.post_owner(threads[0], "", "one more", false, ms / 1000).unwrap();
    ms += 2_000;
    h.publish(ms);
    let d = h.take_delta();
    // The thread's last chunk and block, the catalog buckets, `threads`, the root, the manifest
    // and the small lists: not 50 threads' tails.
    assert!(d.added.len() <= 20, "{} blocks copied", d.added.len());
}

// ---- BC-9, BC-14, BF-1, BF-3: what a hostile owner signs is checked ----

/// The built board with catalog bucket `i`'s entry changed by `f`, the thread's chunks
/// withheld, the root re-linked and re-signed.
fn tampered(b: &mut Board, f: impl Fn(&mut Vec<(String, Value)>)) -> (Vec<u8>, Vec<Block>, Cid) {
    let (root, blocks) = b.build(NOW);
    let by: std::collections::HashMap<Cid, Vec<u8>> = blocks.iter().cloned().collect();
    let Value::Map(mut r) = Value::decode(&by[&root]).unwrap() else { panic!() };
    let mut out: Vec<Block> = blocks.iter().filter(|(c, _)| *c != root).cloned().collect();
    let mut thread = None;
    for (k, v) in r.iter_mut() {
        if k != "cat" {
            continue;
        }
        let Value::Array(cat) = v else { panic!() };
        for l in cat.iter_mut() {
            let Value::Map(mut bucket) = Value::decode(&by[l.link().unwrap()]).unwrap() else { panic!() };
            let Value::Array(entries) = &mut bucket[0].1 else { panic!() };
            if let Some(Value::Map(e)) = entries.first_mut() {
                thread = e.iter().find(|(k, _)| k == "thread").and_then(|(_, v)| v.bytes()).and_then(Cid::from_bytes);
                f(e);
                let enc = Value::Map(bucket).encode();
                let c = Cid::of(DAG_CBOR, &enc);
                out.push((c.clone(), enc));
                *l = Value::Link(c);
            }
        }
    }
    // Withhold the thread's chunks (keep the thread block).
    let t = Value::decode(&by[thread.as_ref().unwrap()]).unwrap();
    let chunks: Vec<Cid> = t.get("chunks").unwrap().array().unwrap().iter().map(|c| c.link().unwrap().clone()).collect();
    out.retain(|(c, _)| !chunks.contains(c));
    let enc = Value::Map(r).encode();
    let new_root = Cid::of(DAG_CBOR, &enc);
    out.push((new_root.clone(), enc));
    let rec = ipns::create(b.signing_key(), &Record { value: format!("/ipfs/{}", new_root.to_text()), sequence: NOW * 1000 + 5, validity: NOW + limits::VALIDITY_S, ttl_ns: limits::TTL_NS });
    (rec, out, new_root)
}

#[test]
fn bc9_a_huge_reply_count_is_refused_and_never_sizes_a_page() {
    let mut b = board();
    let t = put(&mut b, 0, "op", 1, NOW);
    let (rec, blocks, root) = tampered(&mut b, |e| {
        for (k, v) in e.iter_mut() {
            if k == "r" {
                *v = Value::Uint(1 << 40);
            }
        }
    });
    assert_eq!(verify::verify(b.name(), &rec, &blocks, NOW * 1000 + 10, 0, &[]).err(), Some(BoardError::Invalid), "readers and mirrors refuse it");
    // Even if served, the page is not sized from it.
    let served = gateway::Served::new(b.name().clone(), root, rec, blocks, ephem_board::page::Served::Mirror).unwrap();
    let page = served.respond(&Route::Thread(t));
    assert!(page.len() < 1 << 20);
}

#[test]
fn bc14_bf1_bf3_manifest_onions_are_checked_by_readers() {
    let mut b = board();
    b.manifest.mirrors = vec!["x".repeat(70_000)];
    let (root, blocks) = b.build(NOW);
    let rec = b.record(&root, NOW * 1000);
    assert_eq!(verify::verify(b.name(), &rec, &blocks, NOW * 1000, 0, &[]).err(), Some(BoardError::Invalid), "a mirror string that is no onion");
    let mut b = board();
    b.manifest.mirrors = vec![format!("{}.onion", "a".repeat(56))];
    let (root, blocks) = b.build(NOW);
    let rec = b.record(&root, NOW * 1000);
    assert!(verify::verify(b.name(), &rec, &blocks, NOW * 1000, 0, &[]).is_err(), "no v3 checksum");
    let mut b = board();
    b.manifest.host = "attacker.example".into();
    let (root, blocks) = b.build(NOW);
    let rec = b.record(&root, NOW * 1000);
    assert!(verify::verify(b.name(), &rec, &blocks, NOW * 1000, 0, &[]).is_err(), "the signed host is a v3 onion");
    let mut b = board();
    let v = view(&mut b, NOW, &[]);
    assert_eq!(v.manifest.host, host_onion(), "readers learn where posts go from the signed manifest");
    assert!(b.set_mirrors(vec![format!("{}.onion", "a".repeat(56))]).is_err());
    assert!(b.set_mirrors(vec![onion::address(&[3; 32])]).is_ok());
}

// ---- BC-2, BC-11, BC-12: the intake ----

fn intake() -> (Board, Intake) {
    let b = board();
    let i = Intake::new(b.name(), [2; 32], Efforts { reply: 10, thread: 2 }, NOW);
    (b, i)
}

/// A header solved at `effort` (kind from `t`).
fn solved(b: &Board, info: &PowInfo, t: u64, effort: u32, seed: u8) -> (Header, Vec<u8>) {
    let key = SigningKey::from_bytes(&[seed; 32]);
    let k = key.verifying_key().to_bytes();
    let mut n = [seed; 16];
    let mut c = Challenge::new(&b.name().to_bytes(), &info.seed, if t == 0 { kind::THREAD } else { kind::REPLY }, t, &k, &n, effort).unwrap();
    let solution = pow::solve(&mut c, &mut n, effort, &mut SolverMemory::new(), 100_000).unwrap();
    let s = Signed { b: b.name().to_text(), t, k, n, sub: if t == 0 { "s".into() } else { String::new() }, body: format!("b{seed}"), sage: false, e: info.epoch, trip: false };
    let bytes = submission(&s, &key, effort, solution).unwrap();
    (Header::parse(bytes[..HEADER_LEN].try_into().unwrap()).unwrap(), bytes)
}

#[test]
fn bc2_refused_threads_raise_only_the_thread_effort_and_only_in_a_flood() {
    let (b, mut i) = intake();
    let mut seed = 0u8;
    // One new thread with a valid header each minute; the budget admits one per 2 minutes.
    let mut thread = |i: &mut Intake, t: u64| {
        let info = i.pow_info(t);
        seed += 1;
        let (h, bytes) = solved(&b, &info, 0, info.effort_thread, seed);
        if let Ok(Next::ReadBody(slot)) = i.check_header(&h, bytes.len(), t) {
            i.check_body(&h, &bytes[HEADER_LEN..]).unwrap();
            let _ = i.admit(&h, slot, t);
        }
        h
    };
    let mut t = NOW;
    // The attack: a refused new thread or two a minute. No ramp, and R8 never fires.
    for _ in 0..40 {
        t += 60;
        let h = thread(&mut i, t);
        let _ = i.admit(&h, 0, t);
        i.tick(t);
    }
    let p = i.pow_info(t);
    assert_eq!((p.effort_reply, p.effort_thread), (10, 2), "a refused thread or two a minute is no flood");
    assert!(!i.threads_closed, "R8 does not fire");
    // A real flood of new threads raises the thread effort, never the reply effort.
    for _ in 0..3 {
        t += 60;
        let h = thread(&mut i, t);
        for k in 0..u64::from(caps::THREAD_FLOOD) {
            let _ = i.admit(&h, 0, t + k);
        }
        i.tick(t);
    }
    let p = i.pow_info(t);
    assert_eq!(p.effort_reply, 10, "replies are not taxed by a thread flood");
    assert!(p.effort_thread > 2);
}

#[test]
fn bc2_a_raise_is_enforced_after_the_grace() {
    let (_, mut i) = intake();
    i.pow_info(NOW);
    // Posts-cap pressure: more than half the cap in a minute.
    let p0 = i.pow_info(NOW);
    i.base = Efforts { reply: 40, thread: 320 }; // the owner raises the base
    let p1 = i.pow_info(NOW + 1);
    assert_eq!((p1.effort_reply, p1.min_reply), (40, p0.effort_reply), "the old value during the grace");
    let p2 = i.pow_info(NOW + 1 + caps::GRACE_S);
    assert_eq!(p2.min_reply, 40, "then the new one: no epoch-long minimum");
}

#[test]
fn bc11_an_epoch_at_the_maximum_is_refused_without_overflow() {
    let (b, mut i) = intake();
    let info = i.pow_info(NOW);
    let (mut h, bytes) = solved(&b, &info, 0, info.effort_thread, 1);
    h.epoch = u32::MAX;
    assert_eq!(i.check_header(&h, bytes.len(), NOW), Err(Refusal::Pow));
}

#[test]
fn bc12_a_submission_refused_busy_may_be_sent_again() {
    let (b, mut i) = intake();
    let info = i.pow_info(NOW);
    let (h, bytes) = solved(&b, &info, 0, info.effort_thread, 1);
    let (h2, bytes2) = solved(&b, &info, 0, info.effort_thread, 2);
    let Ok(Next::ReadBody(s1)) = i.check_header(&h, bytes.len(), NOW) else { panic!() };
    i.check_body(&h, &bytes[HEADER_LEN..]).unwrap();
    i.admit(&h, s1, NOW).unwrap();
    // A second new thread hits the thread budget: Busy.
    let Ok(Next::ReadBody(s2)) = i.check_header(&h2, bytes2.len(), NOW) else { panic!() };
    i.check_body(&h2, &bytes2[HEADER_LEN..]).unwrap();
    assert_eq!(i.admit(&h2, s2, NOW).err(), Some(Refusal::Busy));
    // The same submission later: its body is read again, not refused as a replay.
    assert_eq!(i.check_header(&h2, bytes2.len(), NOW + 1), Ok(Next::ReadBody(s2)));
    // While the first is in the ring, its retry waits.
    assert_eq!(i.check_header(&h, bytes.len(), NOW + 1), Err(Refusal::Busy));
}

// ---- BC-6: pre-moderation keeps the highest efforts and holds a text once ----

fn short(f: impl FnOnce(&mut [u8; SHORT]) -> usize) -> Vec<u8> {
    let mut out = [0u8; SHORT];
    let n = f(&mut out);
    out[..n].to_vec()
}

/// Submits `body` as a reply to `t` at `effort` straight through the host; publishes.
fn reply(h: &mut Host, ms: u64, t: u64, body: &str, effort: u32, seed: u8) -> Answer {
    let now = ms / 1000;
    let info = h.intake.pow_info(now);
    let key = SigningKey::from_bytes(&[seed; 32]);
    let k = key.verifying_key().to_bytes();
    let mut n = [seed; 16];
    let name = h.served.name.clone();
    let mut c = Challenge::new(&name.to_bytes(), &info.seed, kind::REPLY, t, &k, &n, effort).unwrap();
    let solution = pow::solve(&mut c, &mut n, effort, &mut SolverMemory::new(), 100_000).unwrap();
    let s = Signed { b: name.to_text(), t, k, n, sub: String::new(), body: body.into(), sage: false, e: info.epoch, trip: false };
    let bytes = submission(&s, &key, effort, solution).unwrap();
    let head: &[u8; HEADER_LEN] = bytes[..HEADER_LEN].try_into().unwrap();
    let resp = match h.submit_header(head, bytes.len(), now) {
        Ok((hd, Next::ReadBody(slot))) => match h.submit_body(&hd, slot, &bytes[HEADER_LEN..], now) {
            Ok(id) => {
                h.publish(ms + 2_000);
                match h.answer(id).unwrap() {
                    Ok((no, seq)) => short(|o| gateway::answer(no, seq, o)),
                    Err(r) => short(|o| gateway::refusal(r, o)),
                }
            }
            Err(r) => short(|o| gateway::refusal(r, o)),
        },
        Ok((_, Next::Done { no, seq })) => short(|o| gateway::answer(no, seq, o)),
        Err(r) => short(|o| gateway::refusal(r, o)),
    };
    gateway::parse_answer(&resp).unwrap()
}

#[test]
fn bc6_a_full_held_queue_keeps_the_highest_efforts() {
    use ephem_board::own::{HELD, Switches};
    let b = board();
    let intake = Intake::new(b.name(), [2; 32], LOW, NOW);
    let mut ms = NOW * 1000;
    let mut h = Host::new(b, intake, [9; 32], ms);
    let t = h.post_owner(0, "thread", "op", false, NOW).unwrap();
    h.set_switches(Switches { premod: true, ..Switches::default() });
    for j in 0..HELD as u8 {
        ms += 3_000;
        assert!(matches!(reply(&mut h, ms, t, &format!("junk {j}"), 1, 10 + j), Answer::Posted { no: 0, .. }));
    }
    ms += 3_000;
    assert!(matches!(reply(&mut h, ms, t, "junk 0", 4, 60), Answer::Refused { .. }), "the same text is held once");
    ms += 3_000;
    let a = reply(&mut h, ms, t, "an honest reply", 4, 61);
    assert!(matches!(a, Answer::Posted { no: 0, .. }), "a higher effort takes a low one's place: {a:?}");
    assert!(h.own.held.iter().any(|x| x.s.body == "an honest reply"));
    assert_eq!(h.own.held.len(), HELD);
}

// ---- BC-13: a random nonce for every seal ----

#[test]
fn bc13_every_sealed_own_block_has_its_own_nonce() {
    let b = board();
    let intake = Intake::new(b.name(), [2; 32], LOW, NOW);
    let mut ms = NOW * 1000;
    let mut h = Host::new(b, intake, [9; 32], ms);
    let first = h.board.own[..24].to_vec();
    h.unban(&[5; 32]); // marks the own block for a new seal
    ms += 2_000;
    h.publish(ms);
    assert_ne!(h.board.own[..24], first[..]);
}

// ---- BC-10: mass delete in one pass ----

#[test]
fn bc10_mass_delete_is_linear() {
    let mut b = board();
    let mut i = 0;
    let mut nos = Vec::new();
    for th in 0..40u64 {
        i += 1;
        let t = put(&mut b, 0, "op", i, NOW + th);
        for _ in 0..499 {
            i += 1;
            nos.push(put(&mut b, t, &format!("flood {i}"), i, NOW + th));
        }
    }
    let t0 = std::time::Instant::now();
    let done = b.delete_many(&nos, del::OWNER, 0, NOW + 100);
    let ms = t0.elapsed().as_millis();
    assert_eq!(done.len(), nos.len());
    // Before: 20 000 deletes scanned the board once each (quadratic, seconds).
    assert!(ms < 2_000, "{} posts deleted in {ms} ms", done.len());
}

// ---- shown text: no direction controls or invisible characters (spoofing) ----

#[test]
fn text_that_could_disguise_itself_is_refused() {
    use ephem_board::post::Signed;
    let onion = host_onion();
    assert_eq!(Board::new(&SEED, &onion, "news\u{202E}gpj.exe", "", "", NOW).err(), Some(BoardError::Invalid), "a title with a right-to-left override");
    assert_eq!(Board::new(&SEED, &onion, "Board\u{200B}", "", "", NOW).err(), Some(BoardError::Invalid), "a title with a zero-width space");
    assert!(Board::new(&SEED, &onion, "Доска · لوحة · 板 ❤️", "line one\nline two", "", NOW).is_ok(), "any script, emoji, new lines in about");
    let mut b = board();
    let t = put(&mut b, 0, "op", 1, NOW);
    let (mut s, _) = signed(&b, t, "x", 2);
    for bad in ["click\u{202E}gpj.exe", "a\u{2066}b", "hidden\u{E0041}tag"] {
        s.body = bad.into();
        assert_eq!(s.check(), Err(BoardError::Invalid), "{bad:?}");
    }
    s.body = "fine 👨\u{200D}👩\n>greentext".into();
    assert_eq!(s.check(), Ok(()));
    let op = Signed { sub: "sub\u{202E}ject".into(), ..signed(&b, 0, "op", 3).0 };
    assert_eq!(op.check(), Err(BoardError::Invalid), "a subject");
    // An owner's ban reason is cleaned, never a board readers refuse.
    b.log(NOW, "ban", t, "spam\u{202E}x");
    view(&mut b, NOW, &[]);
}

#[test]
fn readers_refuse_a_disguised_title() {
    let mut b = board();
    b.manifest.title = "news\u{202E}gpj.exe".into();
    let (root, blocks) = b.build(NOW);
    let rec = b.record(&root, NOW * 1000);
    assert_eq!(verify::verify(b.name(), &rec, &blocks, NOW * 1000, 0, &[]).err(), Some(BoardError::Invalid));
}
