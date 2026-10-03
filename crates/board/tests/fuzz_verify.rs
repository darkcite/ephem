// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
// Copyright 2026 Anton (darkcite)
//! BD-1: the reader's verification never panics on hostile blocks (a mirror or a gateway can
//! serve anything). `FUZZ_ITERS` sets the number of mutated CARs (default 20 000).

use ed25519_dalek::SigningKey;
use ephem_board::board::{Board, cap};
use ephem_board::post::Signed;
use ephem_board::verify;
use ephem_channel::car;
use ephem_channel::cid::Cid;

struct Rng(u64);
impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }
    fn below(&mut self, n: usize) -> usize {
        (self.next() % n.max(1) as u64) as usize
    }
}

#[test]
fn mutated_boards_never_panic() {
    const NOW: u64 = 1_790_000_000;
    let mut b = Board::new(&[4; 32], "/f/", "fuzz", "", NOW).unwrap();
    for i in 0..3u64 {
        let key = SigningKey::from_bytes(&[i as u8 + 1; 32]);
        let s = Signed { b: b.name().to_text(), t: 0, k: key.verifying_key().to_bytes(), n: [0; 16], sub: format!("t{i}"), body: "op".into(), sage: false, e: 0 };
        let sig = s.sign(&key);
        let no = b.accept(s, sig, cap::ANON, NOW).unwrap();
        let s = Signed { b: b.name().to_text(), t: no, k: key.verifying_key().to_bytes(), n: [1; 16], sub: String::new(), body: "re".into(), sage: false, e: 0 };
        let sig = s.sign(&key);
        b.accept(s, sig, cap::ANON, NOW).unwrap();
    }
    let (root, blocks) = b.build(NOW);
    let rec = b.record(&root, NOW * 1000);
    let name = b.name().clone();
    let iters = std::env::var("FUZZ_ITERS").ok().and_then(|v| v.parse().ok()).unwrap_or(20_000);
    let key = SigningKey::from_bytes(&[4; 32]).verifying_key();
    let mut r = Rng(0x2545_F491_4F6C_DD1D);
    for _ in 0..iters {
        // Mutate one block, then re-address it and every block above it up to the root (CIDs
        // have one length, so a parent's link bytes can be swapped in place): what a hostile
        // board owner can serve under a record it signs itself.
        let mut bl = blocks.clone();
        let i = r.below(bl.len());
        let data = &mut bl[i].1;
        for _ in 0..1 + r.below(4) {
            if data.is_empty() {
                break;
            }
            let at = r.below(data.len());
            match r.below(3) {
                0 => data[at] = r.next() as u8,
                1 => data.insert(at, r.next() as u8),
                _ => {
                    data.remove(at);
                }
            }
        }
        let mut new_root = root.clone();
        let mut changed = vec![(bl[i].0.to_bytes(), i)];
        while let Some((old, j)) = changed.pop() {
            let fresh = Cid::of(bl[j].0.codec, &bl[j].1);
            if bl[j].0 == new_root {
                new_root = fresh.clone();
            }
            bl[j].0 = fresh.clone();
            let new = fresh.to_bytes();
            for k in 0..bl.len() {
                if k != j && let Some(pos) = bl[k].1.windows(old.len()).position(|w| w == old.as_slice()) {
                    let before = bl[k].0.to_bytes();
                    bl[k].1[pos..pos + new.len()].copy_from_slice(&new);
                    changed.push((before, k));
                }
            }
        }
        let _ = verify::read(&key, &name, &new_root, &bl, &[]);
        let mut rec2 = rec.clone();
        let at = r.below(rec2.len());
        rec2[at] ^= 1 << r.below(8);
        let _ = verify::verify(&name, &rec2, &blocks, NOW * 1000, 0, &[]);
        let file = car::write(std::slice::from_ref(&new_root), &bl);
        let _ = car::read(&file);
    }
}
