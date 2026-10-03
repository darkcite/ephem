// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
// Copyright 2026 Anton (darkcite)
//! Regression tests for the security audit of 2026-10-03 (docs/security/): each was a working
//! exploit against the session before its fix.

use ephem_core::session::{Event, Privacy, Session, Settings, State};
use ephem_crypto::Identity;
use ephem_crypto::noise::{Handshake, Transport};
use ephem_proto::buf::Buf;
use ephem_proto::candidate::CandidateBin;
use ephem_proto::code::{Code, Cred, IceParams, Kind, MAX_CODE_LEN};
use ephem_proto::frame::{FrameType, HEADER_LEN, Header, MAX_FRAME, rtype, write_record};
use ephem_proto::sdp::MAX_SDP_LEN;

const NOW_S: u32 = 1_790_000_000;
const NOW_MS: u64 = NOW_S as u64 * 1000;

fn ice(cands: &[(&str, &str, &str)]) -> IceParams {
    let mut i = IceParams { ufrag: Cred::new(b"A/uL").unwrap(), pwd: Cred::new(b"55bZhqbovQp1LFLkFt9yUC/k").unwrap(), fingerprint: [7; 32], ..IceParams::EMPTY };
    for (a, p, t) in cands {
        assert!(i.push(CandidateBin::from_sdp_parts(a, p, t).unwrap()));
    }
    i
}

const ALICE_SDP: &str = "v=0\r\na=candidate:1 1 udp 2122260223 9090b126-3aae-4a3e-b714-5d089ddfbff0.local 5000 typ host\r\na=ice-ufrag:abcd\r\na=ice-pwd:55bZhqbovQp1LFLkFt9yUC/k\r\na=fingerprint:sha-256 01:01:01:01:01:01:01:01:01:01:01:01:01:01:01:01:01:01:01:01:01:01:01:01:01:01:01:01:01:01:01:01\r\n";

/// F-02: in LAN-only mode with Drop IPv6, the peer's public and IPv6 candidates never reach the
/// browser (it would send connectivity checks to them from our public address).
#[test]
fn remote_candidates_follow_the_privacy_mode() {
    let attacker = Identity::from_seed(&[9; 32]);
    let victim = Identity::from_seed(&[1; 32]);
    let cands = [("198.51.100.7", "40000", "srflx"), ("2001:db8::7", "40001", "srflx"), ("203.0.113.5", "40002", "host"), ("192.168.1.20", "40003", "host"), ("fd00::20", "40004", "host")];
    let inv = Code {
        kind: Kind::Invite, flags: 0, invite_id: [1; 16], room_id: [2; 16], static_pk: attacker.peer_id().0, onion_pk: [0; 32],
        expires_at: NOW_S + 300,
        ice: ice(&cands),
    };
    let mut buf = [0u8; MAX_CODE_LEN];
    let n = inv.encode(&mut buf).unwrap();
    let mut out = [0u8; MAX_SDP_LEN];

    let s = Session::answerer(&victim, &buf[..n], NOW_S, Privacy::LanOnly, true, Settings::default(), false).unwrap();
    let sdp = s.remote_sdp(&mut out).unwrap();
    assert!(!sdp.contains("198.51.100.7") && !sdp.contains("2001:db8::7") && !sdp.contains("203.0.113.5") && !sdp.contains("fd00::20"), "{sdp}");
    assert!(sdp.contains("192.168.1.20 40003 typ host"), "a LAN address stays: {sdp}");

    let s = Session::answerer(&victim, &buf[..n], NOW_S, Privacy::Default, true, Settings::default(), false).unwrap();
    let sdp = s.remote_sdp(&mut out).unwrap();
    assert!(sdp.contains("198.51.100.7") && !sdp.contains("2001:db8::7") && !sdp.contains("fd00::20"), "{sdp}");

    let s = Session::answerer(&victim, &buf[..n], NOW_S, Privacy::Default, false, Settings::default(), false).unwrap();
    assert!(s.remote_sdp(&mut out).unwrap().contains("2001:db8::7"), "unchanged without Drop IPv6");
}

/// A connected Alice (real Session) and a hand-driven Bob that holds the transport keys.
struct Rig {
    alice: Session,
    bob_tx: Transport,
    out: Vec<Vec<u8>>,
    events: Vec<String>,
}

fn rig(transfer: bool) -> Rig {
    let a = Identity::from_seed(&[1; 32]);
    let b = Identity::from_seed(&[2; 32]);
    let mut alice = if transfer {
        Session::transfer_receiver(&a, [3; 16], [4; 16], NOW_S + 300, Privacy::Default, false)
    } else {
        Session::offerer(&a, [3; 16], [4; 16], NOW_S + 300, Privacy::Default, false, Settings::default())
    };
    let invite = alice.build_code(&a, ALICE_SDP).unwrap().to_vec();
    let ans = Code { kind: Kind::Answer, flags: 0, invite_id: [3; 16], room_id: [0; 16], static_pk: b.peer_id().0, onion_pk: [0; 32], expires_at: 0, ice: ice(&[("198.51.100.9", "4000", "srflx")]) };
    let mut abuf = [0u8; MAX_CODE_LEN];
    let an = ans.encode(&mut abuf).unwrap();
    alice.apply_answer(&a, &abuf[..an], NOW_S, false).unwrap();
    let mut out: Vec<Vec<u8>> = Vec::new();
    alice.on_open(NOW_MS, &mut |e| if let Event::Send(f) = e { out.push(f.to_vec()) });
    let m1 = out.pop().unwrap();
    let mut hs = Handshake::new(&b, &a.peer_id(), false, &invite, &abuf[..an]).unwrap();
    hs.read(&m1[HEADER_LEN..]).unwrap();
    let mut m2 = [0u8; 128];
    Header { ftype: FrameType::Handshake, flags: 0, seq: 1 }.write(&mut m2).unwrap();
    let n = hs.write(&mut m2[HEADER_LEN..]).unwrap();
    let mut f = m2[..HEADER_LEN + n].to_vec();
    alice.on_frame(NOW_MS, &mut f, &mut |_| {});
    assert_eq!(alice.state(), State::Connected);
    let (bob_tx, _) = hs.finish().unwrap();
    Rig { alice, bob_tx, out: Vec::new(), events: Vec::new() }
}

impl Rig {
    fn send(&mut self, rt: u8, body: &[u8]) {
        let mut f = [0u8; MAX_FRAME];
        let n = {
            let mut b = Buf::new(&mut f[HEADER_LEN..MAX_FRAME - 16]);
            write_record(&mut b, rt, 0, body).unwrap();
            b.len()
        };
        let len = self.bob_tx.seal(&mut f, n).unwrap();
        let (out, ev) = (&mut self.out, &mut self.events);
        self.alice.on_frame(NOW_MS, &mut f[..len], &mut |e| match e {
            Event::Send(x) => out.push(x.to_vec()),
            Event::Hello { nick, .. } => ev.push(format!("hello {:?}", core::str::from_utf8(nick).unwrap())),
            Event::IdentityReceived(b) => ev.push(format!("identity {}", b.len())),
            Event::Closed(e) => ev.push(format!("closed {e:?}")),
            _ => {}
        });
    }
}

/// F-04: a HELLO nickname with tabs and new lines (it forged rows in the adapter's member and
/// contact lists), an invisible suffix or a check mark closes the session instead of being shown.
#[test]
fn hello_nick_with_row_separators_is_refused() {
    for nick in [&b"\n3\t0\tanon_abcdef\tme\tBoss"[..], "Alice\u{200B}".as_bytes(), "Alice \u{2714}".as_bytes()] {
        let mut r = rig(false);
        let mut body = vec![1u8, 1];
        body.extend_from_slice(&0u32.to_le_bytes());
        body.extend_from_slice(&4096u16.to_le_bytes());
        body.extend_from_slice(&[5; 32]);
        body.push(nick.len() as u8);
        body.extend_from_slice(nick);
        r.send(rtype::HELLO, &body);
        assert!(!r.events.iter().any(|e| e.starts_with("hello")), "{:?}", r.events);
        assert_ne!(r.alice.state(), State::Connected);
    }
}
