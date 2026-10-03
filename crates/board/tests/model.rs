// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
// Copyright 2026 Anton (darkcite)
//! BD-1 (docs/BOARDS.md G.16.3): the owner's model and the reader's verification, end to end.

use ed25519_dalek::SigningKey;
use ephem_board::board::{Board, Entry, cap, del, post_hash};
use ephem_board::post::Signed;
use ephem_board::verify::{self, View};
use ephem_board::{BoardError, limits};
use ephem_channel::car::{self, Block};
use ephem_channel::cid::Cid;
use ephem_channel::gateway::reachable;
use std::collections::HashSet;

const NOW: u64 = 1_790_000_000;
const SEED: [u8; 32] = [4; 32];

fn board() -> Board {
    Board::new(&SEED, "/test/", "A test board", "Be nice", NOW).unwrap()
}

/// A post by a fresh poster key (IDs off: one key per post).
fn post(b: &Board, t: u64, body: &str, i: u64) -> (Signed, [u8; 64]) {
    let mut seed = [0u8; 32];
    seed[..8].copy_from_slice(&i.to_le_bytes());
    seed[8] = (t & 0xff) as u8;
    seed[31] = 1;
    let key = SigningKey::from_bytes(&seed);
    let s = Signed { b: b.name().to_text(), t, k: key.verifying_key().to_bytes(), n: [i as u8; 16], sub: if t == 0 { format!("Thread {i}") } else { String::new() }, body: body.into(), sage: false, e: 1, trip: false };
    let sig = s.sign(&key);
    (s, sig)
}

fn put(b: &mut Board, t: u64, body: &str, i: u64, now: u64) -> u64 {
    let (s, sig) = post(b, t, body, i);
    b.accept(s, sig, cap::ANON, now).unwrap()
}

fn publish(b: &mut Board, now: u64) -> (Cid, Vec<Block>, Vec<u8>) {
    let (root, blocks) = b.build(now);
    let rec = b.record(&root, now * 1000);
    (root, blocks, rec)
}

fn read(b: &Board, rec: &[u8], blocks: &[Block], dels: &[[u8; 32]]) -> View {
    verify::verify(b.name(), rec, blocks, NOW * 1000 + 10_000_000, 0, dels).unwrap()
}

#[test]
fn a_full_board_builds_and_verifies() {
    let mut b = board();
    let mut i = 0;
    for t in 0..limits::THREADS as u64 {
        i += 1;
        let no = put(&mut b, 0, &format!("OP {t}"), i, NOW + t);
        for r in 0..(limits::THREAD_POSTS - 1) as u64 {
            i += 1;
            put(&mut b, no, &format!("reply {r} in {t}"), i, NOW + t);
        }
    }
    let (root, blocks, rec) = publish(&mut b, NOW + 1000);
    let file = car::write(std::slice::from_ref(&root), &blocks);
    let (_, read_blocks) = car::read(&file).unwrap();
    let v = read(&b, &rec, &read_blocks, &[]);
    assert_eq!(v.catalog.len(), limits::THREADS);
    assert_eq!(v.threads.len(), limits::THREADS);
    assert!(v.threads.iter().all(|t| t.entries.len() == limits::THREAD_POSTS && t.locked), "500 posts lock a thread");
    assert_eq!(v.next_no, 1 + (limits::THREADS * limits::THREAD_POSTS) as u64);
    println!("full board: {} blocks, CAR {} KiB", blocks.len(), file.len() / 1024);
}

#[test]
fn full_chunks_keep_their_cid() {
    let mut b = board();
    let a = put(&mut b, 0, "A", 1, NOW);
    let other = put(&mut b, 0, "B", 2, NOW);
    for i in 0..70 {
        put(&mut b, a, &format!("a{i}"), 10 + i, NOW);
    }
    let (_, first, _) = publish(&mut b, NOW);
    put(&mut b, other, "unrelated", 500, NOW + 1);
    put(&mut b, a, "one more in A", 501, NOW + 1);
    let (_, second, _) = publish(&mut b, NOW + 1);
    let set = |bl: &[Block]| bl.iter().map(|(c, _)| c.clone()).collect::<HashSet<_>>();
    let (s1, s2) = (set(&first), set(&second));
    // Thread A's first chunk (64 posts) is full: same CID; its second chunk changed.
    let chunk0 = |bl: &[Block]| bl.iter().find(|(_, d)| d.windows(4).any(|w| w == b"a62\x00") || String::from_utf8_lossy(d).contains("a61")).map(|(c, _)| c.clone());
    let c0 = chunk0(&first).unwrap();
    assert!(s2.contains(&c0), "a full chunk is not re-encoded");
    assert!(s1.len() > 5 && !s1.is_subset(&s2), "the last chunk, thread, bucket, index and root changed");
}

#[test]
fn every_block_is_pinned_from_the_root_archive_included() {
    let mut b = board();
    let a = put(&mut b, 0, "to be pruned", 1, NOW);
    put(&mut b, a, "reply", 2, NOW);
    put(&mut b, 0, "stays", 3, NOW);
    b.prune(a, NOW + 10).unwrap();
    b.set_own(vec![7; 300]).unwrap();
    let (root, blocks, rec) = publish(&mut b, NOW + 20);
    let kept = reachable(&root, blocks.clone());
    assert_eq!(kept.len(), blocks.iter().map(|(c, _)| c).collect::<HashSet<_>>().len(), "GC keeps exactly what the root pins (B1: the archive is pinned)");
    let v = read(&b, &rec, &kept, &[]);
    assert_eq!(v.archive.len(), 1);
    assert_eq!(v.archive[0].no, a);
    assert_eq!(v.own, vec![7; 300]);
}

#[test]
fn deletion_is_enforced_even_from_an_older_root() {
    let mut b = board();
    let t = put(&mut b, 0, "op", 1, NOW);
    let bad = put(&mut b, t, "doxx", 2, NOW);
    let (_, old_blocks, old_rec) = publish(&mut b, NOW);
    b.delete(bad, del::OWNER, 0, NOW + 5).unwrap();
    let (_, new_blocks, new_rec) = publish(&mut b, NOW + 5);
    let new = read(&b, &new_rec, &new_blocks, &[]);
    assert!(matches!(new.threads[0].entries[1], Entry::Tomb { no, .. } if no == bad));
    // A stale mirror serves the old root: with the newer deletion list the post is still gone.
    let known: Vec<[u8; 32]> = new.dels.iter().map(|(h, _)| *h).collect();
    let mut stale = board_clone_view(&b, &old_rec, &old_blocks, &known);
    assert!(matches!(stale.threads.remove(0).entries[1], Entry::Tomb { .. }));
    // Without it, the old root shows it (that is what the deletion list is for).
    let plain = board_clone_view(&b, &old_rec, &old_blocks, &[]);
    assert!(matches!(&plain.threads[0].entries[1], Entry::Post(p) if post_hash(&p.s) == known[0]));
}

fn board_clone_view(b: &Board, rec: &[u8], blocks: &[Block], dels: &[[u8; 32]]) -> View {
    verify::verify(b.name(), rec, blocks, NOW * 1000 + 10_000_000, 0, dels).unwrap()
}

#[test]
fn deleting_an_op_removes_its_thread() {
    let mut b = board();
    let t = put(&mut b, 0, "op", 1, NOW);
    put(&mut b, t, "r", 2, NOW);
    b.delete(t, del::OWNER, 0, NOW).unwrap();
    assert!(b.threads.is_empty());
    assert_eq!(b.dels.len(), 2, "the OP and its reply");
}

#[test]
fn prune_protection_bump_limit_sage_and_duplicates() {
    let mut b = board();
    for i in 0..limits::THREADS as u64 {
        put(&mut b, 0, &format!("op {i}"), i + 1, NOW);
    }
    // Every thread is young: a new one is refused, nothing is pruned.
    let (s, sig) = post(&b, 0, "one too many", 999);
    assert_eq!(b.accept(s, sig, cap::ANON, NOW + 60), Err(BoardError::Busy));
    // An hour later the oldest-bumped one goes to the archive.
    let (s, sig) = post(&b, 0, "now there is room", 1000);
    let later = NOW + limits::PROTECT_AGE_S + limits::PROTECT_BUMP_S;
    b.accept(s, sig, cap::ANON, later).unwrap();
    assert_eq!(b.threads.len(), limits::THREADS);
    assert_eq!(b.archive.len(), 1);

    let mut b = board();
    let t = put(&mut b, 0, "op", 1, NOW);
    let (mut s, _) = post(&b, t, "sage", 2);
    s.sage = true;
    let key = SigningKey::from_bytes(&{
        let mut k = [0u8; 32];
        k[..8].copy_from_slice(&2u64.to_le_bytes());
        k[8] = t as u8;
        k[31] = 1;
        k
    });
    let sig = s.sign(&key);
    b.accept(s, sig, cap::ANON, NOW + 100).unwrap();
    assert_eq!(b.threads[0].bump, NOW, "sage does not bump");
    put(&mut b, t, "same words", 3, NOW + 200);
    let (s, sig) = post(&b, t, "same words", 4);
    assert_eq!(b.accept(s, sig, cap::ANON, NOW + 201), Err(BoardError::Duplicate));
    for i in 0..limits::BUMP_LIMIT as u64 {
        put(&mut b, t, &format!("r{i}"), 100 + i, NOW + 300 + i);
    }
    let bumped = b.threads[0].bump;
    put(&mut b, t, "past the bump limit", 9000, NOW + 5000);
    assert_eq!(b.threads[0].bump, bumped, "no bump past the bump limit");
}

#[test]
fn forgeries_and_misplaced_posts_fail() {
    let mut b = board();
    let t = put(&mut b, 0, "op", 1, NOW);
    // A post signed for another board, or a bad signature, is refused by the host.
    let (mut s, sig) = post(&b, t, "x", 2);
    s.b = "k51other".into();
    assert_eq!(b.accept(s, sig, cap::ANON, NOW), Err(BoardError::Invalid));
    let (s, mut sig) = post(&b, t, "x", 3);
    sig[0] ^= 1;
    assert_eq!(b.accept(s, sig, cap::ANON, NOW), Err(BoardError::BadSignature));
    // An anonymous post cannot claim the owner's capcode.
    let (s, sig) = post(&b, t, "I am the owner", 4);
    assert_eq!(b.accept(s, sig, cap::OWNER, NOW), Err(BoardError::Refused));
    // The owner's own capcode post works.
    let s = Signed { b: b.name().to_text(), t, k: b.manifest.pk, n: [5; 16], sub: String::new(), body: "Rules updated".into(), sage: false, e: 1, trip: false };
    let sig = s.sign(b.signing_key());
    b.accept(s, sig, cap::OWNER, NOW).unwrap();
    let (root, blocks, rec) = publish(&mut b, NOW);
    read(&b, &rec, &blocks, &[]);
    // Another key's record for this name fails; a swapped block breaks the chain.
    let mut other = Board::new(&[5; 32], "/test/", "", "", NOW).unwrap();
    let (oroot, oblocks) = other.build(NOW);
    let orec = other.record(&oroot, NOW * 1000);
    assert_eq!(verify::verify(b.name(), &orec, &oblocks, NOW * 1000, 0, &[]).err(), Some(BoardError::Record));
    let swapped: Vec<Block> = blocks.iter().filter(|(c, _)| *c != root).cloned().chain(oblocks.into_iter().filter(|(c, _)| *c == oroot).map(|(_, d)| (root.clone(), d))).collect();
    assert!(verify::verify(b.name(), &rec, &swapped, NOW * 1000, 0, &[]).is_err());
}

#[test]
fn sequence_is_time_based_and_bounded() {
    let mut b = board();
    put(&mut b, 0, "op", 1, NOW);
    let (root, blocks) = b.build(NOW);
    let rec = b.record(&root, NOW * 1000);
    assert_eq!(b.seq, NOW * 1000, "a fresh board starts at the clock");
    let rec2 = b.record(&root, NOW * 1000 - 5_000);
    assert_eq!(b.seq, NOW * 1000 + 1, "never backwards, even with a slow clock");
    assert!(verify::verify(b.name(), &rec2, &blocks, NOW * 1000, NOW * 1000 + 2, &[]).is_err(), "below the reader's high-water mark");
    // A reader whose clock is more than an hour behind the sequence refuses the record.
    assert_eq!(verify::verify(b.name(), &rec, &blocks, NOW * 1000 - 2 * 3_600_000, 0, &[]).err(), Some(BoardError::Record));
}

#[test]
fn a_reader_with_the_catalog_only_and_the_owner_reopening() {
    let mut b = board();
    let t1 = put(&mut b, 0, "first", 1, NOW);
    let t2 = put(&mut b, 0, "second", 2, NOW + 10);
    put(&mut b, t1, "bump it", 3, NOW + 20);
    let (root, blocks, rec) = publish(&mut b, NOW + 30);
    // The catalog and one thread only (what a phone fetches first).
    let t2_cid = {
        let v = read(&b, &rec, &blocks, &[]);
        v.catalog.iter().find(|c| c.no == t2).unwrap().thread.clone()
    };
    let partial: Vec<Block> = blocks.iter().filter(|(c, _)| *c != t2_cid).cloned().collect();
    let v = read(&b, &rec, &partial, &[]);
    assert_eq!(v.catalog.len(), 2);
    assert_eq!(v.catalog[0].no, t1, "bumped first");
    assert_eq!(v.threads.len(), 1);
    // The owner reopens from the store and continues numbering.
    let key = SigningKey::from_bytes(&SEED).verifying_key();
    let mut again = Board::load(&SEED, { let mut v = verify::read(&key, b.name(), &root, &blocks, &[]).unwrap(); v.sequence = b.seq; v }, blocks.clone()).unwrap();
    assert_eq!(put(&mut again, t2, "after reopening", 4, NOW + 40), 4);
    assert_eq!(again.seq, b.seq);
}
