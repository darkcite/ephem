//! E7 (docs/P2P-CHAT.md Appendix C.4): the parsers that read what a Snowflake proxy sends us
//! (encapsulation, KCP, smux, the whole session) take arbitrary and mutated input without
//! panicking, and fail only with their typed errors. Deterministic (seeded xorshift), so a
//! failure reproduces; `FUZZ_ITERS` raises the budget (`cargo test --release` recommended).
//!
//! Valid traffic comes from a simulated server: a peer KCP carrying smux frames for our stream,
//! encapsulated as the Go server writes it; the mutations then flip, insert, drop and splice
//! bytes of that stream.

use ephem_snowflake::encap::{self, Decoder};
use ephem_snowflake::kcp::{Kcp, MTU};
use ephem_snowflake::smux::{self, Smux};
use ephem_snowflake::{Error, Session};

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
    fn bytes(&mut self, n: usize) -> Vec<u8> {
        (0..n).map(|_| self.next() as u8).collect()
    }
    /// Up to `max - 1` random bytes.
    fn blob(&mut self, max: usize) -> Vec<u8> {
        let n = self.below(max);
        self.bytes(n)
    }
}

fn iters(default: usize) -> usize {
    std::env::var("FUZZ_ITERS").ok().and_then(|v| v.parse().ok()).unwrap_or(default)
}

const CONV: u32 = 0x1234_5678;

/// What the Go server sends a client: smux PSH/UPD/NOP frames for stream 3 in KCP segments,
/// encapsulated (with padding chunks mixed in).
fn server_stream(rng: &mut Rng) -> Vec<u8> {
    let mut kcp = Kcp::new(CONV);
    let mut frames = Vec::new();
    for _ in 0..1 + rng.below(8) {
        let (cmd, body) = match rng.below(3) {
            0 => (2u8, { let n = 1 + rng.below(3000); rng.bytes(n) }),
            1 => (4u8, rng.bytes(8)),
            _ => (3u8, Vec::new()),
        };
        frames.extend_from_slice(&[2, cmd]);
        frames.extend_from_slice(&(body.len() as u16).to_le_bytes());
        frames.extend_from_slice(&smux::STREAM_ID.to_le_bytes());
        frames.extend_from_slice(&body);
    }
    let mut off = 0;
    while off < frames.len() {
        off += kcp.send(&frames[off..]);
    }
    let mut out = Vec::new();
    let mut pre = [0u8; 3];
    kcp.flush(0, &mut |pkt: &[u8]| {
        if pkt.len() % 3 == 0 {
            out.push(0x05); // a 5-byte padding chunk
            out.extend_from_slice(&[0; 5]);
        }
        let n = encap::prefix(pkt.len(), &mut pre);
        out.extend_from_slice(&pre[..n]);
        out.extend_from_slice(pkt);
    });
    out
}

fn mutate(rng: &mut Rng, v: &mut Vec<u8>) {
    for _ in 0..1 + rng.below(6) {
        if v.is_empty() {
            v.push(rng.next() as u8);
            continue;
        }
        let i = rng.below(v.len());
        match rng.below(6) {
            0 => v[i] ^= 1 << rng.below(8),
            1 => v[i] = rng.next() as u8,
            2 => v.insert(i, rng.next() as u8),
            3 => {
                v.remove(i);
            }
            4 => {
                // An interesting value in a length/number field.
                let x: [u8; 4] = [[0xFF; 4], [0; 4], [0x7F, 0xFF, 0xFF, 0xFF], [0xFF, 0xFF, 0, 0]][rng.below(4)];
                for (k, b) in x.iter().enumerate() {
                    if i + k < v.len() {
                        v[i + k] = *b;
                    }
                }
            }
            _ => {
                let j = rng.below(v.len());
                let n = rng.below(64).min(v.len() - i.max(j));
                let chunk = v[j..j + n].to_vec();
                v.splice(i..i, chunk);
            }
        }
    }
}

/// Feeds `data` in random pieces.
fn pieces(rng: &mut Rng, data: &[u8], mut f: impl FnMut(&[u8]) -> bool) {
    let mut off = 0;
    while off < data.len() {
        let n = (1 + rng.below(2 * MTU)).min(data.len() - off);
        if !f(&data[off..off + n]) {
            return;
        }
        off += n;
    }
}

#[test]
fn encapsulation_decoder() {
    let mut rng = Rng(0x9E37_79B9_7F4A_7C15);
    let mut dec: Box<Decoder<2048>> = Box::new(Decoder::new());
    for i in 0..iters(20_000) {
        let mut data = if i % 2 == 0 { server_stream(&mut rng) } else { rng.blob(4096) };
        mutate(&mut rng, &mut data);
        dec.reset();
        pieces(&mut rng, &data, |p| {
            dec.feed(p, |pkt| assert!(pkt.len() <= 2048)).is_ok()
        });
    }
}

#[test]
fn kcp_input() {
    let mut rng = Rng(0xD1B5_4A32_D192_ED03);
    let mut buf = vec![0u8; 64 * 1024];
    let mut kcp = Box::new(Kcp::new(CONV));
    let mut dec: Box<Decoder<2048>> = Box::new(Decoder::new());
    for i in 0..iters(20_000) {
        if i % 64 == 0 {
            kcp = Box::new(Kcp::new(CONV));
        }
        let mut data = server_stream(&mut rng);
        dec.reset();
        let mut pkts = Vec::new();
        let _ = dec.feed(&data, |p| pkts.push(p.to_vec()));
        for p in &mut pkts {
            if rng.below(2) == 0 {
                mutate(&mut rng, p);
            }
            let _ = kcp.input(i as u32, p);
        }
        data.clear();
        let _ = kcp.input(i as u32, &rng.blob(200));
        while kcp.readable() {
            let n = kcp.recv(&mut buf);
            assert!(n <= buf.len());
            if n == 0 {
                break;
            }
        }
        kcp.flush(i as u32, &mut |pkt: &[u8]| assert!(pkt.len() <= MTU));
        let _ = kcp.send(&rng.blob(3000));
    }
}

#[test]
fn smux_input() {
    let mut rng = Rng(0xA076_1D64_78BD_642F);
    let mut out = vec![0u8; 32 * 1024];
    let mut s = Box::new(Smux::new());
    for i in 0..iters(50_000) {
        if i % 32 == 0 {
            s = Box::new(Smux::new());
        }
        let mut frame = vec![2, [0u8, 1, 2, 3, 4][rng.below(5)]];
        let body = rng.blob(600);
        frame.extend_from_slice(&(body.len() as u16).to_le_bytes());
        frame.extend_from_slice(&[smux::STREAM_ID, 5, 0][rng.below(3)].to_le_bytes());
        frame.extend_from_slice(&body);
        mutate(&mut rng, &mut frame);
        if s.input(&frame).is_err() {
            s = Box::new(Smux::new());
            continue;
        }
        assert!(s.readable() <= smux::RX_BUF);
        let n = s.read(&mut out);
        assert!(n <= out.len());
        s.control(&mut |c: &[u8]| assert!(c.len() >= smux::HEADER));
    }
}

#[test]
fn session_from_proxy() {
    let mut rng = Rng(0xE703_7ED1_A0B4_28DB);
    let mut out = vec![0u8; 64 * 1024];
    let mut errors = 0;
    for i in 0..iters(3_000) {
        let mut s = Session::new([7; 8], CONV);
        s.on_channel();
        let mut data = server_stream(&mut rng);
        if i % 3 != 0 {
            mutate(&mut rng, &mut data);
        }
        let mut now = 0u32;
        pieces(&mut rng, &data, |p| {
            now += 10;
            let r = s.on_data(now, p);
            let _ = s.poll(now, &mut |pkt: &[u8]| assert!(pkt.len() <= 2 * MTU));
            let _ = s.read(&mut out);
            match r {
                Ok(()) => true,
                Err(Error::Encapsulation(_) | Error::Kcp(_) | Error::Smux(_)) => {
                    errors += 1;
                    false
                }
                Err(e) => panic!("unexpected error {e:?}"),
            }
        });
        // A failed session stays failed.
        if let Some(e) = s.error() {
            assert_eq!(s.on_data(now, &[0x80]), Err(e));
        }
    }
    assert!(errors > 0, "mutations must hit the error paths");
}
