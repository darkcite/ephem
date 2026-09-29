// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
// Copyright 2026 Anton (darkcite)
//! Two sessions run against each other through an in-memory "network".

use super::*;
use core::fmt::Write;

const NOW_S: u32 = 1_790_000_000;
const NOW_MS: u64 = NOW_S as u64 * 1000;

fn local_sdp(ufrag: &str, fp: u8, mdns: &str, port: u16) -> ([u8; 1024], usize) {
    let mut out = [0u8; 1024];
    let mut b = Buf::new(&mut out);
    write!(
        b,
        "v=0\r\na=candidate:1 1 udp 2122260223 {mdns}.local {port} typ host\r\n\
         a=candidate:2 1 udp 2122260223 192.168.1.5 {port} typ host\r\n\
         a=candidate:3 1 udp 1686052607 203.0.113.9 {port} typ srflx raddr 0.0.0.0 rport 0\r\n\
         a=ice-ufrag:{ufrag}\r\na=ice-pwd:55bZhqbovQp1LFLkFt9yUC/k\r\na=fingerprint:sha-256 "
    )
    .unwrap();
    for i in 0..32 {
        write!(b, "{}{:02X}", if i > 0 { ":" } else { "" }, fp).unwrap();
    }
    b.put(b"\r\n").unwrap();
    let n = b.len();
    (out, n)
}

fn sdp_str(x: &([u8; 1024], usize)) -> &str {
    core::str::from_utf8(&x.0[..x.1]).unwrap()
}

/// Frames in flight from one side (copied, as the network would).
struct Wire {
    frames: Vec<Vec<u8>>,
}

#[derive(Default)]
struct Seen {
    sas: Option<Sas>,
    connects: u32,
    resumed: bool,
    sas_optional: Option<bool>,
    chats: Vec<(u64, Vec<u8>, u32, Option<MsgRef>)>,
    settings: Vec<u32>,
    edited: Vec<(u64, Vec<u8>)>,
    deleted: Vec<MsgRef>,
    expired: Vec<MsgRef>,
    delivered: u64,
    read: u64,
    typing: Option<bool>,
    suspended: u32,
    closed: Option<ErrorCode>,
    nick: Vec<u8>,
    reactions: Vec<(MsgRef, Vec<u8>)>,
    signals: Vec<(bool, IceParams)>,
    peer_ready: bool,
    identity_sent: bool,
    identity: Vec<u8>,
    room: Vec<(u8, Vec<u8>)>,
}

fn sink<'w>(wire: &'w mut Wire, seen: &'w mut Seen) -> impl FnMut(Event<'_>) + 'w {
    move |e| match e {
        Event::Send(f) => wire.frames.push(f.to_vec()),
        Event::Connected { sas, resumed, .. } => {
            seen.sas = Some(sas);
            seen.connects += 1;
            seen.resumed = resumed;
        }
        Event::Hello { sas_optional, nick, .. } => {
            seen.sas_optional = Some(sas_optional);
            seen.nick = nick.to_vec();
        }
        Event::Reaction { msg, emoji, .. } => seen.reactions.push((msg, emoji.to_vec())),
        Event::Room { rtype, body } => seen.room.push((rtype, body.to_vec())),
        Event::SignalOffer(ice) => seen.signals.push((true, ice)),
        Event::SignalAnswer(ice) => seen.signals.push((false, ice)),
        Event::PeerReady => seen.peer_ready = true,
        Event::IdentitySent => seen.identity_sent = true,
        Event::IdentityReceived(b) => seen.identity = b.to_vec(),
        Event::Chat { msg, text, ttl_s, reply } => seen.chats.push((msg.seq, text.to_vec(), ttl_s, reply)),
        Event::Setting { ttl_s, .. } => seen.settings.push(ttl_s),
        Event::Edited { msg, text } => seen.edited.push((msg.seq, text.to_vec())),
        Event::Deleted(m) => seen.deleted.push(m),
        Event::Expired(m) => seen.expired.push(m),
        Event::Delivered { seq } => seen.delivered = seq,
        Event::Read { seq } => seen.read = seq,
        Event::PeerTyping(t) => seen.typing = Some(t),
        Event::Suspended => seen.suspended += 1,
        Event::Closed(e) => seen.closed = Some(e),
        Event::Degraded | Event::Alive | Event::PeerHidden(_) => {}
    }
}

struct Side {
    id: Identity,
    s: Box<Session>,
    out: Wire,
    seen: Seen,
}

struct Pair {
    a: Side,
    b: Side,
    now: u64,
}

macro_rules! act {
    ($side:expr, $method:ident ( $($arg:expr),* )) => {{
        let side = &mut $side;
        let mut k = sink(&mut side.out, &mut side.seen);
        side.s.$method($($arg,)* &mut k)
    }};
}

impl Pair {
    /// Delivers everything in flight, both ways, until quiet.
    fn settle(&mut self) {
        for _ in 0..16 {
            let fa: Vec<Vec<u8>> = self.a.out.frames.drain(..).collect();
            let fb: Vec<Vec<u8>> = self.b.out.frames.drain(..).collect();
            if fa.is_empty() && fb.is_empty() {
                return;
            }
            for mut f in fa {
                let now = self.now;
                act!(self.b, on_frame(now, &mut f));
            }
            for mut f in fb {
                let now = self.now;
                act!(self.a, on_frame(now, &mut f));
            }
        }
        panic!("network never settled");
    }

    /// Both sides lose the path (frames in flight are lost too).
    fn cut(&mut self) {
        self.a.out.frames.clear();
        self.b.out.frames.clear();
        let now = self.now;
        act!(self.a, path_lost(now, ErrorCode::IceFailed));
        act!(self.b, path_lost(now, ErrorCode::IceFailed));
    }

    /// `resumer` makes a RESUME_INVITE, the other side answers, the path reconnects.
    fn resume(&mut self, a_resumes: bool) {
        let (sdp_x, sdp_y) = (local_sdp("RRRR", 0xCC, "2222b126-3aae-4a3e-b714-5d089ddfbff0", 41000), local_sdp("QQQQ", 0xDD, "3333b126-3aae-4a3e-b714-5d089ddfbff0", 42000));
        let (x, y) = if a_resumes { (&mut self.a, &mut self.b) } else { (&mut self.b, &mut self.a) };
        x.s.resume_invite([5; 16], NOW_S + 600).unwrap();
        let inv = x.s.build_code(&x.id, sdp_str(&sdp_x)).unwrap().to_vec();
        assert_eq!(Code::decode(&inv).unwrap().kind, Kind::ResumeInvite);
        y.s.accept_resume(&y.id, &inv, NOW_S).unwrap();
        let ans = y.s.build_code(&y.id, sdp_str(&sdp_y)).unwrap().to_vec();
        assert_eq!(Code::decode(&ans).unwrap().kind, Kind::ResumeAnswer);
        x.s.apply_answer(&x.id, &ans, NOW_S, false).unwrap();
        let now = self.now;
        if a_resumes {
            act!(self.a, on_open(now));
            act!(self.b, on_open(now));
        } else {
            act!(self.b, on_open(now));
            act!(self.a, on_open(now));
        }
        self.settle();
    }
}

fn connect_with(sa: Settings, sb: Settings, a_scanned: bool, b_scanned: bool) -> Pair {
    let alice = Identity::from_seed(&[1; 32]);
    let bob = Identity::from_seed(&[2; 32]);
    let mut a = Box::new(Session::offerer(&alice, [7; 16], [9; 16], NOW_S + 300, Privacy::Default, false, sa));
    let invite = a.build_code(&alice, sdp_str(&local_sdp("AAAA", 0xAA, "9090b126-3aae-4a3e-b714-5d089ddfbff0", 40000))).unwrap().to_vec();
    let mut b = Box::new(Session::answerer(&bob, &invite, NOW_S, Privacy::Default, false, sb, b_scanned).unwrap());
    let mut sdp = [0u8; MAX_SDP_LEN];
    assert!(b.remote_sdp(&mut sdp).unwrap().contains("a=setup:actpass"));
    let answer = b.build_code(&bob, sdp_str(&local_sdp("BBBB", 0xBB, "1111b126-3aae-4a3e-b714-5d089ddfbff0", 50000))).unwrap().to_vec();
    a.apply_answer(&alice, &answer, NOW_S + 10, a_scanned).unwrap();
    assert_eq!(a.apply_answer(&alice, &answer, NOW_S + 10, a_scanned), Err(ErrorCode::InviteConsumed));
    let r = a.remote_sdp(&mut sdp).unwrap();
    assert!(r.contains("a=setup:active") && r.contains("a=ice-ufrag:BBBB"));
    let mut p = Pair {
        a: Side { id: alice, s: a, out: Wire { frames: vec![] }, seen: Seen::default() },
        b: Side { id: bob, s: b, out: Wire { frames: vec![] }, seen: Seen::default() },
        now: NOW_MS,
    };
    act!(p.b, on_open(NOW_MS));
    act!(p.a, on_open(NOW_MS));
    assert_eq!(p.a.out.frames.len(), 1);
    p.settle();
    p
}

fn connect() -> Pair {
    connect_with(Settings::default(), Settings::default(), false, false)
}

#[test]
fn handshake_hello_sas() {
    let p = connect();
    assert_eq!(p.a.s.state(), State::Connected);
    assert_eq!(p.b.s.state(), State::Connected);
    assert!(p.a.seen.sas.is_some() && p.a.seen.sas == p.b.seen.sas, "SAS equal on both sides");
    assert_eq!(p.a.seen.sas_optional, Some(false), "codes by link → SAS prompted");
    assert!(!p.a.seen.resumed);
    let q = connect_with(Settings::default(), Settings::default(), true, true);
    assert_eq!((q.a.seen.sas_optional, q.b.seen.sas_optional), (Some(true), Some(true)), "both scanned → optional");
    let q = connect_with(Settings::default(), Settings::default(), true, false);
    assert_eq!(q.a.seen.sas_optional, Some(false));
}

#[test]
fn chat_ack_read_rekey_close() {
    let mut p = connect();
    let seq = act!(p.a, send_chat(NOW_MS, None, "héllo".as_bytes(), None)).unwrap();
    p.settle();
    assert_eq!(p.b.seen.chats, vec![(1, "héllo".as_bytes().to_vec(), 0, None)]);
    assert_eq!(p.a.seen.delivered, seq);
    assert_eq!(p.a.s.pending_count(), 0);

    act!(p.b, mark_read(NOW_MS + 2000, 1));
    p.settle();
    assert_eq!(p.a.seen.read, 1);

    let r = act!(p.b, send_chat(NOW_MS, None, b"hi", Some(MsgRef { sender: 0, seq: 1 }))).unwrap();
    p.settle();
    assert_eq!(p.a.seen.chats[0].3, Some(MsgRef { sender: 0, seq: 1 }), "reply refers to Alice's message");
    assert_eq!(r, 1);

    act!(p.a, tick(NOW_MS + REKEY_MS, false));
    p.settle();
    act!(p.a, send_chat(NOW_MS, None, b"after rekey", None)).unwrap();
    p.settle();
    assert_eq!(p.b.seen.chats.len(), 2);
    assert!(p.b.seen.closed.is_none());

    act!(p.a, close(NOW_MS));
    p.settle();
    assert_eq!(p.b.seen.closed, Some(ErrorCode::PeerOffline));
    assert_eq!(p.b.s.state(), State::Closed);
}

#[test]
fn edit_delete_typing() {
    let mut p = connect();
    act!(p.a, send_chat(NOW_MS, None, b"typo", None)).unwrap();
    p.settle();
    act!(p.a, edit(NOW_MS, 1, b"fixed")).unwrap();
    p.settle();
    assert_eq!(p.b.seen.edited, vec![(1, b"fixed".to_vec())]);
    assert_eq!(act!(p.a, edit(NOW_MS, 9, b"x")), Err(ErrorCode::NotPermitted), "not our message");

    act!(p.a, delete(NOW_MS, MsgRef { sender: 0, seq: 1 })).unwrap();
    p.settle();
    assert_eq!(p.b.seen.deleted, vec![MsgRef { sender: 0, seq: 1 }]);
    act!(p.b, delete(NOW_MS, MsgRef { sender: 0, seq: 1 })).unwrap();
    assert!(p.a.out.frames.is_empty() && p.b.out.frames.is_empty(), "delete-for-me sends nothing");

    act!(p.a, typing(NOW_MS, true));
    p.settle();
    assert_eq!(p.b.seen.typing, Some(true));
    act!(p.a, typing(NOW_MS + 100, true));
    assert!(p.a.out.frames.is_empty(), "rate limited");
    act!(p.b, tick(NOW_MS + TYPING_CLEAR_MS, false));
    assert_eq!(p.b.seen.typing, Some(false), "cleared after 6 s");
    act!(p.a, typing(NOW_MS + 200, false));
    p.settle();
    act!(p.a, send_chat(NOW_MS, None, b"x", None)).unwrap();
    act!(p.a, typing(NOW_MS + 300, true));
    p.settle();
    assert_eq!(p.b.seen.typing, Some(true));
    act!(p.a, send_chat(NOW_MS, None, b"y", None)).unwrap();
    p.settle();
    assert_eq!(p.b.seen.typing, Some(false), "a message clears typing");
}

#[test]
fn receipts_and_typing_are_reciprocal() {
    let off = Settings { read_receipts: false, typing: false, ..Settings::default() };
    let mut p = connect_with(Settings::default(), off, false, false);
    act!(p.a, send_chat(NOW_MS, None, b"1", None)).unwrap();
    p.settle();
    act!(p.b, mark_read(NOW_MS + 5000, 1));
    p.settle();
    assert_eq!(p.a.seen.read, 0, "Bob does not send READ");
    act!(p.a, typing(NOW_MS, true));
    p.settle();
    assert_eq!(p.b.seen.typing, None, "Bob does not see typing");
    act!(p.b, typing(NOW_MS, true));
    assert!(p.b.out.frames.is_empty(), "Bob does not send typing");
}

#[test]
fn self_destruct() {
    let mut p = connect();
    act!(p.a, set_ttl(NOW_MS, None, 5)).unwrap();
    assert_eq!(act!(p.a, set_ttl(NOW_MS, None, 7)), Err(ErrorCode::NotPermitted));
    p.settle();
    assert_eq!(p.b.seen.settings, vec![5]);
    assert_eq!(p.b.s.chat_ttl(), 5, "either person sets it for both");
    let seq = act!(p.a, send_chat(NOW_MS, None, b"secret", None)).unwrap();
    p.settle();
    assert_eq!(p.b.seen.chats[0].2, 5);
    // Countdowns start when read (recipient) and when READ arrives (sender).
    act!(p.b, tick(NOW_MS + 60_000, false));
    act!(p.a, tick(NOW_MS + 60_000, false));
    assert!(p.a.seen.expired.is_empty() && p.b.seen.expired.is_empty());
    act!(p.b, mark_read(NOW_MS + 60_000, seq));
    p.settle();
    act!(p.b, tick(NOW_MS + 64_999, false));
    assert!(p.b.seen.expired.is_empty());
    act!(p.b, tick(NOW_MS + 65_000, false));
    act!(p.a, tick(NOW_MS + 65_000, false));
    assert_eq!(p.b.seen.expired, vec![MsgRef { sender: 0, seq }]);
    assert_eq!(p.a.seen.expired, vec![MsgRef { sender: 0, seq }]);
    act!(p.b, set_ttl(NOW_MS, None, 0)).unwrap();
    p.settle();
    assert_eq!(p.a.seen.settings, vec![0]);
    act!(p.a, send_chat(NOW_MS, None, b"plain", None)).unwrap();
    p.settle();
    assert_eq!(p.b.seen.chats.last().unwrap().2, 0);
}

#[test]
fn resume_resends_pending_and_keeps_seq() {
    let mut p = connect();
    act!(p.a, send_chat(NOW_MS, None, b"one", None)).unwrap();
    p.settle();
    p.cut();
    assert_eq!((p.a.seen.suspended, p.b.seen.suspended), (1, 1));
    assert_eq!(p.a.s.state(), State::Suspended);
    // Typed while offline: queued; edit and delete rewrite the queue in place.
    let s2 = act!(p.a, send_chat(NOW_MS, None, b"two (draft)", None)).unwrap();
    let s3 = act!(p.a, send_chat(NOW_MS, None, b"three", None)).unwrap();
    let s4 = act!(p.a, send_chat(NOW_MS, None, b"four", None)).unwrap();
    act!(p.a, edit(NOW_MS, s2, b"two")).unwrap();
    act!(p.a, delete(NOW_MS, MsgRef { sender: 0, seq: s3 })).unwrap();
    assert!(p.a.out.frames.is_empty(), "nothing sent while suspended");
    assert_eq!(p.a.s.pending_count(), 3);
    assert_eq!(act!(p.a, edit(NOW_MS, 1, b"x")), Err(ErrorCode::PeerOffline), "delivered message, peer offline");

    // Bob sends the resume invite this time: roles of the path swap, chat state does not.
    p.resume(false);
    assert_eq!(p.a.s.state(), State::Connected);
    assert!(p.a.seen.resumed && p.b.seen.resumed);
    let got: Vec<(u64, &[u8])> = p.b.seen.chats.iter().map(|c| (c.0, c.1.as_slice())).collect();
    assert_eq!(got, vec![(1, &b"one"[..]), (s2, &b"two"[..]), (s4, &b"four"[..])], "final versions only, deleted never sent");
    assert_eq!(p.a.s.pending_count(), 0);

    // Messages and replies keep working across the new path in both directions.
    act!(p.b, send_chat(NOW_MS, None, b"back", Some(MsgRef { sender: 0, seq: s4 }))).unwrap();
    p.settle();
    assert_eq!(p.a.seen.chats.last().unwrap().3, Some(MsgRef { sender: 0, seq: s4 }));

    // Cut while frames are in flight: the unacked message is resent after the next resume.
    act!(p.a, send_chat(NOW_MS, None, b"lost in flight", None)).unwrap();
    p.cut();
    p.resume(true);
    assert_eq!(p.b.seen.chats.last().unwrap().1, b"lost in flight".to_vec());
    let n = p.b.seen.chats.len();
    p.cut();
    p.resume(true);
    assert_eq!(p.b.seen.chats.len(), n, "nothing duplicated");
}

#[test]
fn resume_is_bound_to_chat_and_peer() {
    let mut p = connect();
    p.cut();
    let sdp = local_sdp("RRRR", 0xCC, "2222b126-3aae-4a3e-b714-5d089ddfbff0", 41000);
    p.a.s.resume_invite([5; 16], NOW_S + 600).unwrap();
    let mut inv = p.a.s.build_code(&p.a.id, sdp_str(&sdp)).unwrap().to_vec();
    // A stranger cannot use it.
    let mallory = Identity::from_seed(&[3; 32]);
    let mut m = Session::offerer(&mallory, [1; 16], [1; 16], NOW_S + 300, Privacy::Default, false, Settings::default());
    assert_eq!(m.accept_resume(&mallory, &inv, NOW_S), Err(ErrorCode::InvalidRoom));
    // Wrong room id.
    inv[4 + 16] ^= 1;
    assert_eq!(p.b.s.accept_resume(&p.b.id, &inv, NOW_S), Err(ErrorCode::InvalidRoom));
    inv[4 + 16] ^= 1;
    // Wrong static key.
    inv[4 + 32] ^= 1;
    assert_eq!(p.b.s.accept_resume(&p.b.id, &inv, NOW_S), Err(ErrorCode::AuthFailed));
    inv[4 + 32] ^= 1;
    assert!(p.b.s.accept_resume(&p.b.id, &inv, NOW_S).is_ok());
}

#[test]
fn suspended_grace_and_first_path_failure() {
    let mut p = connect();
    p.cut();
    act!(p.a, tick(NOW_MS + SUSPEND_GRACE_MS - 1, false));
    assert!(p.a.seen.closed.is_none());
    act!(p.a, tick(NOW_MS + SUSPEND_GRACE_MS, false));
    assert_eq!(p.a.seen.closed, Some(ErrorCode::PeerOffline));

    // Before the first connection a lost path ends the chat with the adapter's reason.
    let alice = Identity::from_seed(&[1; 32]);
    let mut s = Session::offerer(&alice, [7; 16], [9; 16], NOW_S + 300, Privacy::Default, false, Settings::default());
    let mut w = Wire { frames: vec![] };
    let mut seen = Seen::default();
    s.path_lost(NOW_MS, ErrorCode::NoDirectPath, &mut sink(&mut w, &mut seen));
    assert_eq!(seen.closed, Some(ErrorCode::NoDirectPath));
    assert_eq!(s.send_chat(NOW_MS, None, b"x", None, &mut sink(&mut w, &mut seen)), Err(ErrorCode::PeerOffline));
}

#[test]
fn tamper_closes() {
    let mut p = connect();
    act!(p.a, send_chat(NOW_MS, None, b"x", None)).unwrap();
    p.a.out.frames[0][HEADER_LEN] ^= 1;
    p.settle();
    assert_eq!(p.b.seen.closed, Some(ErrorCode::CryptoFailed));
    assert_eq!(p.b.s.state(), State::Closed);
}

#[test]
fn privacy_filter_and_expiry() {
    let alice = Identity::from_seed(&[1; 32]);
    let bob = Identity::from_seed(&[2; 32]);
    let sdp = local_sdp("AAAA", 0xAA, "9090b126-3aae-4a3e-b714-5d089ddfbff0", 40000);
    for (mode, want) in [(Privacy::LanOnly, 1), (Privacy::Default, 2), (Privacy::MaxConnectivity, 3)] {
        let mut a = Session::offerer(&alice, [7; 16], [9; 16], NOW_S + 300, mode, false, Settings::default());
        let c = Code::decode(a.build_code(&alice, sdp_str(&sdp)).unwrap()).unwrap();
        assert_eq!(c.ice.n_cand, want, "{mode:?}");
        assert_eq!(c.flags & flags::LAN_ONLY != 0, mode == Privacy::LanOnly);
    }
    let mut a = Session::offerer(&alice, [7; 16], [9; 16], NOW_S + 300, Privacy::Default, false, Settings::default());
    let inv = a.build_code(&alice, sdp_str(&sdp)).unwrap().to_vec();
    let late = NOW_S + 300 + SKEW_S + 1;
    assert_eq!(Session::answerer(&bob, &inv, late, Privacy::Default, false, Settings::default(), false).err(), Some(ErrorCode::ExpiredInvite));
    assert_eq!(Session::answerer(&alice, &inv, NOW_S, Privacy::Default, false, Settings::default(), false).err(), Some(ErrorCode::InvalidInvite), "own invite");
    let mut w = Wire { frames: vec![] };
    let mut seen = Seen::default();
    a.tick(NOW_MS + 301_000, false, &mut sink(&mut w, &mut seen));
    assert_eq!(seen.closed, Some(ErrorCode::ExpiredInvite));
    assert!(w.frames.is_empty(), "nothing sent before a channel exists");
}

#[test]
fn nickname_reactions_and_app_rtt() {
    let mut named = Settings::default();
    assert!(named.set_nick("Алиса".as_bytes()));
    assert!(!named.set_nick(&[b'x'; 33]));
    let mut p = connect_with(named, Settings::default(), false, false);
    assert_eq!(p.b.seen.nick, "Алиса".as_bytes());
    assert!(p.a.seen.nick.is_empty());

    act!(p.a, send_chat(NOW_MS, None, b"hi", None)).unwrap();
    p.settle();
    act!(p.b, react(NOW_MS, MsgRef { sender: 0, seq: 1 }, "👍".as_bytes())).unwrap();
    act!(p.a, react(NOW_MS, MsgRef { sender: 0, seq: 1 }, "🎉".as_bytes())).unwrap();
    p.settle();
    assert_eq!(p.a.seen.reactions, vec![(MsgRef { sender: 0, seq: 1 }, "👍".as_bytes().to_vec())]);
    assert_eq!(p.b.seen.reactions, vec![(MsgRef { sender: 0, seq: 1 }, "🎉".as_bytes().to_vec())]);
    act!(p.b, react(NOW_MS, MsgRef { sender: 0, seq: 1 }, b"")).unwrap();
    p.settle();
    assert_eq!(p.a.seen.reactions[1].1, b"", "empty removes");
    assert_eq!(act!(p.b, react(NOW_MS, MsgRef { sender: 0, seq: 9 }, b"x")), Err(ErrorCode::NotPermitted), "unknown message");
    assert_eq!(act!(p.b, react(NOW_MS, MsgRef { sender: 0, seq: 1 }, &[b'x'; 33])), Err(ErrorCode::MessageTooLarge));

    act!(p.a, tick(NOW_MS + PING_IDLE_MS, false));
    p.now = NOW_MS + PING_IDLE_MS + 40;
    p.settle();
    let d = p.a.s.diag(NOW_MS + PING_IDLE_MS + 40);
    assert_eq!(d.app_rtt_ms, 40, "PING→PONG");
    assert_eq!(d.rekeys, 0);
}

#[test]
fn in_band_ice_restart_signals() {
    let mut p = connect();
    let sdp = local_sdp("NEWU", 0xAA, "5555b126-3aae-4a3e-b714-5d089ddfbff0", 43000);
    // Bob (the path's answerer, DTLS client) re-offers; Alice answers.
    act!(p.b, signal(NOW_MS, true, sdp_str(&sdp))).unwrap();
    p.settle();
    let (offer, ice) = p.a.seen.signals[0];
    assert!(offer && ice.ufrag.as_str() == "NEWU");
    let mut out = [0u8; MAX_SDP_LEN];
    let r = p.a.s.render_signal(&ice, true, &mut out).unwrap();
    assert!(r.contains("a=setup:actpass") && r.contains(" 3 IN IP4"), "re-offer, version 3");
    act!(p.a, signal(NOW_MS, false, sdp_str(&local_sdp("ANSW", 0xBB, "6666b126-3aae-4a3e-b714-5d089ddfbff0", 44000)))).unwrap();
    p.settle();
    let (offer, ice) = p.b.seen.signals[0];
    assert!(!offer);
    // Alice was the DTLS server (she offered first), so her re-answer is rendered passive for Bob.
    let r = p.b.s.render_signal(&ice, false, &mut out).unwrap();
    assert!(r.contains("a=setup:passive"));
    // And Alice renders Bob's answers as active.
    let r = p.a.s.render_signal(&ice, false, &mut out).unwrap();
    assert!(r.contains("a=setup:active") && r.contains(" 4 IN IP4"));
}

/// Old device (Bob, signed in) sends its key file to a new device (Alice, TRANSFER invite).
#[test]
fn identity_transfer() {
    let new_dev = Identity::from_seed(&[5; 32]);
    let old_dev = Identity::from_seed(&[6; 32]);
    let mut a = Box::new(Session::transfer_receiver(&new_dev, [7; 16], [9; 16], NOW_S + 300, Privacy::Default, false));
    let invite = a.build_code(&new_dev, sdp_str(&local_sdp("AAAA", 0xAA, "9090b126-3aae-4a3e-b714-5d089ddfbff0", 40000))).unwrap().to_vec();
    assert_ne!(Code::decode(&invite).unwrap().flags & flags::TRANSFER, 0);
    let mut b = Box::new(Session::answerer(&old_dev, &invite, NOW_S, Privacy::Default, false, Settings::default(), true).unwrap());
    assert!(b.transfer());
    let answer = b.build_code(&old_dev, sdp_str(&local_sdp("BBBB", 0xBB, "1111b126-3aae-4a3e-b714-5d089ddfbff0", 50000))).unwrap().to_vec();
    a.apply_answer(&new_dev, &answer, NOW_S, true).unwrap();
    let mut p = Pair {
        a: Side { id: new_dev, s: a, out: Wire { frames: vec![] }, seen: Seen::default() },
        b: Side { id: old_dev, s: b, out: Wire { frames: vec![] }, seen: Seen::default() },
        now: NOW_MS,
    };
    act!(p.b, on_open(NOW_MS));
    act!(p.a, on_open(NOW_MS));
    p.settle();
    assert_eq!(p.a.seen.sas_optional, Some(false), "SAS mandatory even when both scanned");
    assert_eq!(act!(p.b, send_chat(NOW_MS, None, b"x", None)), Err(ErrorCode::NotPermitted), "not a chat");

    let blob: Vec<u8> = (0..30_000u32).map(|i| (i * 7) as u8).collect();
    // Old device confirms first: nothing is sent until the new device confirms too.
    act!(p.b, confirm_sas(NOW_MS, Some(&blob))).unwrap();
    assert!(p.b.out.frames.is_empty());
    act!(p.a, confirm_sas(NOW_MS, None)).unwrap();
    p.settle();
    assert!(p.b.seen.peer_ready && p.b.seen.identity_sent);
    assert_eq!(p.a.seen.identity, blob, "3 chunks reassembled");
    assert!(p.a.seen.closed.is_none() && p.b.seen.closed.is_none());
}

#[test]
fn sas_confirm_on_a_normal_chat_sends_nothing() {
    // Outside a TRANSFER link a confirmed SAS never starts an identity transfer.
    let mut p = connect();
    assert_eq!(act!(p.b, confirm_sas(NOW_MS, Some(b"x"))), Ok(()), "plain SAS confirm");
    assert!(p.b.out.frames.is_empty(), "no transfer on a normal chat");
}

// ---- rooms (§14) ----------------------------------------------------------------------------

use crate::room::{self, RoomRole, RoomState};
use ephem_crypto::seal;

const ROOM: [u8; 16] = [0x42; 16];

/// A connected pairwise room link between `x` (offerer) and `y` (answerer) of room `ROOM`.
fn group_link(x_seed: u8, y_seed: u8, extra: u8) -> Pair {
    let (xi, yi) = (Identity::from_seed(&[x_seed; 32]), Identity::from_seed(&[y_seed; 32]));
    let mut x = Box::new(Session::offerer(&xi, [x_seed ^ y_seed; 16], ROOM, NOW_S + 300, Privacy::Default, false, Settings::default()));
    x.add_flags(flags::GROUP | extra);
    let inv = x.build_code(&xi, sdp_str(&local_sdp("XXXX", x_seed, "9090b126-3aae-4a3e-b714-5d089ddfbff0", 40000))).unwrap().to_vec();
    let mut y = Box::new(Session::answerer(&yi, &inv, NOW_S, Privacy::Default, false, Settings::default(), false).unwrap());
    let ans = y.build_code(&yi, sdp_str(&local_sdp("YYYY", y_seed, "1111b126-3aae-4a3e-b714-5d089ddfbff0", 50000))).unwrap().to_vec();
    x.apply_answer(&xi, &ans, NOW_S, false).unwrap();
    let mut p = Pair { a: Side { id: xi, s: x, out: Wire { frames: vec![] }, seen: Seen::default() }, b: Side { id: yi, s: y, out: Wire { frames: vec![] }, seen: Seen::default() }, now: NOW_MS };
    act!(p.b, on_open(NOW_MS));
    act!(p.a, on_open(NOW_MS));
    p.settle();
    assert_eq!(p.a.s.state(), State::Connected);
    p
}

fn set_link(side: &mut Side, me: u8, peer: u8, my_role: RoomRole, peer_role: RoomRole) {
    side.s.set_room(crate::session::RoomLink { me, peer, my_role, peer_role });
}

#[test]
fn room_state_intro_roles_and_moderation() {
    use RoomRole::{Member, Observer, Owner};
    // Owner O (seed 1) invites A (seed 2, member) and B (seed 3, observer).
    let mut oa = group_link(1, 2, 0);
    let mut ob = group_link(1, 3, flags::OBSERVER);
    assert_ne!(ob.b.s.code_flags() & flags::OBSERVER, 0, "role requested in the invite");
    let owner = Identity::from_seed(&[1; 32]);
    let mut st = RoomState::new(ROOM, &owner);
    let a_idx = st.admit(oa.b.id.peer_id(), oa.b.id.sign_pk(), Member).unwrap();
    let b_idx = st.admit(ob.b.id.peer_id(), ob.b.id.sign_pk(), Observer).unwrap();
    set_link(&mut oa.a, 0, a_idx, Owner, Member);
    set_link(&mut ob.a, 0, b_idx, Owner, Observer);

    // The signed state reaches both; each verifies it against the owner's key from HELLO.
    let blob = st.sign(&owner);
    act!(oa.a, send_room(NOW_MS, rtype::ROOM_STATE, &blob)).unwrap();
    act!(ob.a, send_room(NOW_MS, rtype::ROOM_STATE, &blob)).unwrap();
    oa.settle();
    ob.settle();
    for (link, idx, role) in [(&mut oa, a_idx, Member), (&mut ob, b_idx, Observer)] {
        let (rt, body) = link.b.seen.room.pop().unwrap();
        assert_eq!(rt, rtype::ROOM_STATE);
        let got = RoomState::verify(&body, &owner.peer_id(), &link.b.s.peer_sign_pk()).unwrap();
        assert_eq!(got, st);
        set_link(&mut link.b, idx, 0, role, Owner);
    }

    // Introduction A → B through O: A offers, sealed for B; O forwards what it cannot read.
    let (ai, bi) = (Identity::from_seed(&[2; 32]), Identity::from_seed(&[3; 32]));
    let mut ab_a = Box::new(Session::offerer(&ai, [0x77; 16], ROOM, NOW_S + 300, Privacy::Default, false, Settings::default()));
    ab_a.add_flags(flags::GROUP);
    let inv = ab_a.build_code(&ai, sdp_str(&local_sdp("ABAB", 0xAB, "2222b126-3aae-4a3e-b714-5d089ddfbff0", 41000))).unwrap().to_vec();
    let sealed = seal::seal(&ai, &bi.peer_id(), &room::seal_context(&ROOM, a_idx, b_idx), &inv);
    act!(oa.b, send_room(NOW_MS, rtype::ROOM_SIGNAL, &room::signal_encode(a_idx, b_idx, &sealed))).unwrap();
    oa.settle();
    let (_, sig) = oa.a.seen.room.pop().unwrap();
    let (from, to, boxed) = room::signal_decode(&sig).unwrap();
    assert_eq!((from, to), (a_idx, b_idx), "owner checks `from` = the member on this link");
    assert!(seal::open(&owner, &ai.peer_id(), &room::seal_context(&ROOM, from, to), boxed).is_none(), "owner cannot read it");
    let mut tampered = boxed.to_vec();
    tampered[20] ^= 1;
    assert!(seal::open(&bi, &ai.peer_id(), &room::seal_context(&ROOM, from, to), &tampered).is_none(), "owner cannot alter it");
    let code = seal::open(&bi, &ai.peer_id(), &room::seal_context(&ROOM, from, to), boxed).unwrap();
    assert_eq!(Code::decode(&code).unwrap().static_pk, st.member(a_idx).unwrap().peer.0, "B checks the key against the signed state");
    let mut ab_b = Box::new(Session::answerer(&bi, &code, NOW_S, Privacy::Default, false, Settings::default(), false).unwrap());
    let ans = ab_b.build_code(&bi, sdp_str(&local_sdp("BABA", 0xBA, "3333b126-3aae-4a3e-b714-5d089ddfbff0", 42000))).unwrap().to_vec();
    ab_a.apply_answer(&ai, &ans, NOW_S, false).unwrap();
    let mut ab = Pair { a: Side { id: ai, s: ab_a, out: Wire { frames: vec![] }, seen: Seen::default() }, b: Side { id: bi, s: ab_b, out: Wire { frames: vec![] }, seen: Seen::default() }, now: NOW_MS };
    set_link(&mut ab.a, a_idx, b_idx, Member, Observer);
    set_link(&mut ab.b, b_idx, a_idx, Observer, Member);
    act!(ab.b, on_open(NOW_MS));
    act!(ab.a, on_open(NOW_MS));
    ab.settle();
    assert_eq!(ab.a.s.state(), State::Connected, "direct A↔B link");

    // One room message, the same (sender, seq) on every link (§14.3).
    act!(oa.b, send_chat(NOW_MS, Some(1), b"hello room", None)).unwrap();
    act!(ab.a, send_chat(NOW_MS, Some(1), b"hello room", None)).unwrap();
    oa.settle();
    ab.settle();
    assert_eq!(oa.a.seen.chats[0].0, 1);
    assert_eq!(ab.b.seen.chats[0].0, 1);
    // The owner replies to A's message on its link to B: the quote names A (a third party for that link).
    act!(ob.a, send_chat(NOW_MS, Some(1), b"re", Some(MsgRef { sender: a_idx, seq: 1 }))).unwrap();
    ob.settle();
    assert_eq!(ob.b.seen.chats[0].3, Some(MsgRef { sender: a_idx, seq: 1 }));

    // Observers are read-only: locally refused, and dropped by every receiver even if forced.
    assert_eq!(act!(ab.b, send_chat(NOW_MS, Some(1), b"x", None)), Err(ErrorCode::NotPermitted));
    set_link(&mut ab.b, b_idx, a_idx, Member, Member); // a lying observer
    act!(ab.b, send_chat(NOW_MS, Some(1), b"sneaky", None)).unwrap();
    act!(ab.b, delete(NOW_MS, MsgRef { sender: b_idx, seq: 1 })).unwrap();
    ab.settle();
    assert!(ab.a.seen.chats.is_empty() && ab.a.seen.deleted.is_empty(), "A drops what an observer sends");
    assert!(ab.a.seen.closed.is_none());

    // Owner moderation: O deletes A's message for everyone; a member cannot.
    act!(ob.a, delete(NOW_MS, MsgRef { sender: a_idx, seq: 1 })).unwrap();
    ob.settle();
    assert_eq!(ob.b.seen.deleted, vec![MsgRef { sender: a_idx, seq: 1 }]);
    // ... and on its link to the author, whose own copy goes too.
    act!(oa.a, delete(NOW_MS, MsgRef { sender: a_idx, seq: 1 })).unwrap();
    oa.settle();
    assert_eq!(oa.b.seen.deleted, vec![MsgRef { sender: a_idx, seq: 1 }]);
    act!(ab.a, delete(NOW_MS, MsgRef { sender: 0, seq: 1 })).unwrap();
    assert!(ab.a.out.frames.is_empty(), "a member's delete of someone else's message stays local");
    // Only the owner sets the room timer.
    assert_eq!(act!(oa.b, set_ttl(NOW_MS, Some(2), 60)), Err(ErrorCode::NotPermitted));
    act!(oa.a, set_ttl(NOW_MS, Some(2), 60)).unwrap();
    oa.settle();
    assert_eq!(oa.b.seen.settings, vec![60]);
}

#[test]
fn room_records_only_on_room_links() {
    let mut p = connect();
    assert_eq!(act!(p.a, send_room(NOW_MS, rtype::ROOM_STATE, b"x")), Err(ErrorCode::NotPermitted));
}

/// Tor mode (§28.4): one-way invite, Noise IK, the host picks the stream by decrypting message 1,
/// and the dialer redials after a loss (same handshake, pinned key).
#[test]
fn tor_invite_ik_handshake_and_redial() {
    let (ha, hb) = (Identity::from_seed(&[1; 32]), Identity::from_seed(&[2; 32]));
    let host = Session::tor_host(&ha, [0x51; 16], [0x52; 16], NOW_S + 300, Settings::default());
    let invite = host.local_code().to_vec();
    assert_eq!(invite.len(), ephem_proto::code::TOR_CODE_LEN);
    let c = Code::decode(&invite).unwrap();
    assert_eq!((c.kind, c.onion_pk, c.static_pk), (Kind::TorInvite, ha.onion_pk(), ha.peer_id().0));
    let dialer = Session::tor_dialer(&hb, &invite, NOW_S, Settings::default(), true).unwrap();
    let mut p = Pair { a: Side { id: ha, s: Box::new(host), out: Wire { frames: vec![] }, seen: Seen::default() }, b: Side { id: hb, s: Box::new(dialer), out: Wire { frames: vec![] }, seen: Seen::default() }, now: NOW_MS };

    // Another chat of the host (other invite) must not take the stream.
    let mut other = Session::tor_host(&p.a.id, [0x61; 16], [0x62; 16], NOW_S + 300, Settings::default());
    p.b.s.tor_dial(&p.b.id).unwrap();
    act!(p.b, on_open(NOW_MS));
    let msg1 = p.b.out.frames.remove(0);
    let (mut w, mut s) = (Wire { frames: vec![] }, Seen::default());
    assert_eq!(other.tor_accept(&p.a.id, NOW_MS, &msg1, |_| true, &mut sink(&mut w, &mut s)), Ok(false), "not its invite");
    let id = Identity::from_seed(&[1; 32]); // the host's identity (Identity is not Clone)
    assert_eq!(act!(p.a, tor_accept(&id, NOW_MS, &msg1, |_| true)), Ok(true));
    p.settle();
    assert_eq!(p.a.s.state(), State::Connected);
    assert_eq!(p.b.s.state(), State::Connected);
    assert_eq!(p.a.seen.sas, p.b.seen.sas, "same SAS");
    assert_eq!(p.a.s.remote(), p.b.id.peer_id(), "the host learned the dialer's key");
    assert_eq!(p.a.s.peer_onion(), p.b.id.onion_pk(), "and its onion key");
    assert_eq!(p.b.s.peer_onion(), p.a.id.onion_pk());
    act!(p.b, send_chat(NOW_MS, None, b"over tor", None)).unwrap();
    p.settle();
    assert_eq!(p.a.seen.chats[0].1, b"over tor");

    // The stream drops; a message is queued; the dialer redials; the host takes the new stream
    // only from the pinned key.
    p.cut();
    act!(p.a, send_chat(NOW_MS, None, b"while away", None)).unwrap();
    let intruder = Identity::from_seed(&[9; 32]);
    let mut fake = Session::tor_dialer(&intruder, &invite, NOW_S, Settings::default(), false).unwrap();
    fake.tor_dial(&intruder).unwrap();
    let (mut w, mut s) = (Wire { frames: vec![] }, Seen::default());
    fake.on_open(NOW_MS, &mut sink(&mut w, &mut s));
    assert_eq!(act!(p.a, tor_accept(&id, NOW_MS, &w.frames[0], |_| true)), Ok(false), "only the pinned peer may resume");
    p.b.s.tor_dial(&p.b.id).unwrap();
    act!(p.b, on_open(NOW_MS));
    let msg1 = p.b.out.frames.remove(0);
    assert_eq!(act!(p.a, tor_accept(&id, NOW_MS, &msg1, |_| true)), Ok(true));
    p.settle();
    assert!(p.b.seen.resumed);
    assert_eq!(p.b.seen.chats.last().unwrap().1, b"while away", "queued message delivered after the redial");
}

/// Tor mode (§28.7): a contact dials the stored onion without an invite; the host accepts only
/// keys in its contacts, and an invite host does not take a contact's stream.
#[test]
fn tor_contact_dial() {
    let (ha, hb) = (Identity::from_seed(&[3; 32]), Identity::from_seed(&[4; 32]));
    let (a_key, b_key) = (ha.peer_id(), hb.peer_id());
    let host = Session::tor_contact_host(&ha, Settings::default(), [0; 16]);
    let dialer = Session::tor_contact_dialer(&hb, a_key, ha.onion_pk(), Settings::default(), [0; 16]);
    assert!(host.contact() && dialer.contact());
    assert_eq!(dialer.peer_onion(), ha.onion_pk());
    let mut p = Pair { a: Side { id: ha, s: Box::new(host), out: Wire { frames: vec![] }, seen: Seen::default() }, b: Side { id: hb, s: Box::new(dialer), out: Wire { frames: vec![] }, seen: Seen::default() }, now: NOW_MS };
    p.b.s.tor_dial(&p.b.id).unwrap();
    act!(p.b, on_open(NOW_MS));
    let msg1 = p.b.out.frames.remove(0);
    let id = Identity::from_seed(&[3; 32]);
    let (mut w, mut s) = (Wire { frames: vec![] }, Seen::default());
    let mut invite_host = Session::tor_host(&id, [0x71; 16], [0x72; 16], NOW_S + 300, Settings::default());
    assert_eq!(invite_host.tor_accept(&id, NOW_MS, &msg1, |_| true, &mut sink(&mut w, &mut s)), Ok(false), "an invite does not take a contact's stream");
    assert_eq!(act!(p.a, tor_accept(&id, NOW_MS, &msg1, |k| *k != b_key)), Ok(false), "not a contact");
    assert_eq!(p.a.s.state(), State::AwaitingAnswer);
    assert_eq!(act!(p.a, tor_accept(&id, NOW_MS, &msg1, |k| *k == b_key)), Ok(true));
    p.settle();
    assert_eq!((p.a.s.state(), p.b.s.state()), (State::Connected, State::Connected));
    assert_eq!(p.a.seen.sas, p.b.seen.sas);
    assert_eq!(p.a.s.remote(), b_key);
    assert_eq!(p.a.s.peer_onion(), p.b.id.onion_pk());
    act!(p.a, send_chat(NOW_MS, None, b"hi contact", None)).unwrap();
    p.settle();
    assert_eq!(p.b.seen.chats[0].1, b"hi contact");
}

/// Tor mode, contact cards (§28.4 case 3): a dial carrying the host's card secret is taken by
/// the card session from any key; a contact session (zero secret) does not take it, and a wrong
/// secret is taken by neither.
#[test]
fn tor_card_dial() {
    let (ha, hb) = (Identity::from_seed(&[5; 32]), Identity::from_seed(&[6; 32]));
    let secret = [0x5c; 16];
    let id = Identity::from_seed(&[5; 32]);
    let dial = |secret: [u8; 16]| {
        let hb = Identity::from_seed(&[6; 32]);
        let mut d = Session::tor_contact_dialer(&hb, ha.peer_id(), ha.onion_pk(), Settings::default(), secret);
        d.tor_dial(&hb).unwrap();
        let (mut w, mut s) = (Wire { frames: vec![] }, Seen::default());
        d.on_open(NOW_MS, &mut sink(&mut w, &mut s));
        (d, w.frames.remove(0))
    };
    let (mut w, mut s) = (Wire { frames: vec![] }, Seen::default());
    let (_, wrong) = dial([0x11; 16]);
    let mut card = Session::tor_contact_host(&id, Settings::default(), secret);
    assert_eq!(card.tor_accept(&id, NOW_MS, &wrong, |_| true, &mut sink(&mut w, &mut s)), Ok(false), "wrong secret");
    let (dialer, msg1) = dial(secret);
    let mut contacts = Session::tor_contact_host(&id, Settings::default(), [0; 16]);
    assert_eq!(contacts.tor_accept(&id, NOW_MS, &msg1, |_| true, &mut sink(&mut w, &mut s)), Ok(false), "not a contact dial");
    let mut p = Pair { a: Side { id: ha, s: Box::new(card), out: Wire { frames: vec![] }, seen: Seen::default() }, b: Side { id: hb, s: Box::new(dialer), out: Wire { frames: vec![] }, seen: Seen::default() }, now: NOW_MS };
    assert_eq!(act!(p.a, tor_accept(&id, NOW_MS, &msg1, |_| true)), Ok(true));
    p.settle();
    assert_eq!((p.a.s.state(), p.b.s.state()), (State::Connected, State::Connected));
    assert_eq!(p.a.s.remote(), p.b.id.peer_id());
    assert_eq!(p.a.seen.sas, p.b.seen.sas);
}
