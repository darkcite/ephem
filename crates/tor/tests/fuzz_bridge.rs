// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
// Copyright 2026 Anton (darkcite)
//! BR-1 (docs/P2P-CHAT.md Appendix F.2.3): bridge-line parsing takes arbitrary and mutated text
//! without panicking, and whatever it accepts is valid. Deterministic (seeded xorshift);
//! `FUZZ_ITERS` raises the budget (`cargo test --release -p ephem-tor --test fuzz_bridge`).

use ephem_tor::bridge;

const SEED: &str = "Bridge snowflake 192.0.2.3:80 2B280B23E1107BB62ABFC40DDCC8824814F80A72 fingerprint=2B280B23E1107BB62ABFC40DDCC8824814F80A72 url=https://1098762253.rsc.cdn77.org/ fronts=www.cdn77.com ice=stun:stun.antisip.com:3478,turn:t:1 utls-imitate=hellorandomizedalpn
obfs4 198.51.100.1:443 0123456789ABCDEF0123456789ABCDEF01234567 cert=x iat-mode=0
# comment
snowflake 192.0.2.4:80 8838024498816A039FCBBAB14E6F40A0843051FA url=http://127.0.0.1:1/ ice=stun:127.0.0.1:3478";

const ALPHABET: &[u8] = b"snowflake=:/., \n#0123456789ABCDEFabcdef-_?@[]urlicestunhtps";

struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }
    fn below(&mut self, n: usize) -> usize {
        (self.next() % n as u64) as usize
    }
}

#[test]
fn mutated_lines() {
    let iters = std::env::var("FUZZ_ITERS").ok().and_then(|v| v.parse().ok()).unwrap_or(200_000);
    let mut r = Rng(0x9E37_79B9_7F4A_7C15);
    let seed = SEED.as_bytes();
    let mut buf = Vec::with_capacity(seed.len() * 2);
    for _ in 0..iters {
        buf.clear();
        buf.extend_from_slice(seed);
        for _ in 0..1 + r.below(8) {
            let at = r.below(buf.len() + 1);
            match r.below(4) {
                0 if at < buf.len() => buf[at] = ALPHABET[r.below(ALPHABET.len())],
                1 => buf.insert(at, ALPHABET[r.below(ALPHABET.len())]),
                2 if at < buf.len() => {
                    let n = r.below(buf.len() - at).min(40);
                    buf.drain(at..at + n);
                }
                _ if at < buf.len() => buf[at] = r.next() as u8,
                _ => {}
            }
        }
        let text = String::from_utf8_lossy(&buf);
        let b = bridge::parse(&text);
        // Whatever is accepted is well formed.
        assert!(b.fingerprints.len() <= ephem_tor::net::BRIDGE_ADDRS.len());
        assert!(b.fingerprints.iter().all(|f| f.len() == 40 && f.bytes().all(|c| c.is_ascii_hexdigit())));
        assert!(b.brokers.iter().all(|u| u.starts_with("https://") || u.starts_with("http://127.0.0.1") || u.starts_with("http://localhost")));
        assert!(b.ice.iter().all(|s| s.starts_with("stun:")));
        assert!(b.problems.iter().all(|(n, _)| *n >= 1));
    }
}
