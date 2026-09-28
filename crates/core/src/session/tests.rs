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
}

fn sink<'w>(wire: &'w mut Wire, seen: &'w mut Seen) -> impl FnMut(Event<'_>) + 'w {
    move |e| match e {
        Event::Send(f) => wire.frames.push(f.to_vec()),
        Event::Connected { sas, resumed, .. } => {
            seen.sas = Some(sas);
            seen.connects += 1;
            seen.resumed = resumed;
        }
        Event::Hello { sas_optional, .. } => seen.sas_optional = Some(sas_optional),
        Event::Chat { seq, text, ttl_s, reply } => seen.chats.push((seq, text.to_vec(), ttl_s, reply)),
        Event::Setting { ttl_s } => seen.settings.push(ttl_s),
        Event::Edited { seq, text } => seen.edited.push((seq, text.to_vec())),
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
    let seq = act!(p.a, send_chat(NOW_MS, "héllo".as_bytes(), None)).unwrap();
    p.settle();
    assert_eq!(p.b.seen.chats, vec![(1, "héllo".as_bytes().to_vec(), 0, None)]);
    assert_eq!(p.a.seen.delivered, seq);
    assert_eq!(p.a.s.pending_count(), 0);

    act!(p.b, mark_read(NOW_MS + 2000, 1));
    p.settle();
    assert_eq!(p.a.seen.read, 1);

    let r = act!(p.b, send_chat(NOW_MS, b"hi", Some(MsgRef { mine: false, seq: 1 }))).unwrap();
    p.settle();
    assert_eq!(p.a.seen.chats[0].3, Some(MsgRef { mine: true, seq: 1 }), "reply refers to Alice's message");
    assert_eq!(r, 1);

    act!(p.a, tick(NOW_MS + REKEY_MS, false));
    p.settle();
    act!(p.a, send_chat(NOW_MS, b"after rekey", None)).unwrap();
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
    act!(p.a, send_chat(NOW_MS, b"typo", None)).unwrap();
    p.settle();
    act!(p.a, edit(NOW_MS, 1, b"fixed")).unwrap();
    p.settle();
    assert_eq!(p.b.seen.edited, vec![(1, b"fixed".to_vec())]);
    assert_eq!(act!(p.a, edit(NOW_MS, 9, b"x")), Err(ErrorCode::NotPermitted), "not our message");

    act!(p.a, delete(NOW_MS, MsgRef { mine: true, seq: 1 })).unwrap();
    p.settle();
    assert_eq!(p.b.seen.deleted, vec![MsgRef { mine: false, seq: 1 }]);
    act!(p.b, delete(NOW_MS, MsgRef { mine: false, seq: 1 })).unwrap();
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
    act!(p.a, send_chat(NOW_MS, b"x", None)).unwrap();
    act!(p.a, typing(NOW_MS + 300, true));
    p.settle();
    assert_eq!(p.b.seen.typing, Some(true));
    act!(p.a, send_chat(NOW_MS, b"y", None)).unwrap();
    p.settle();
    assert_eq!(p.b.seen.typing, Some(false), "a message clears typing");
}

#[test]
fn receipts_and_typing_are_reciprocal() {
    let off = Settings { read_receipts: false, typing: false };
    let mut p = connect_with(Settings::default(), off, false, false);
    act!(p.a, send_chat(NOW_MS, b"1", None)).unwrap();
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
    act!(p.a, set_ttl(NOW_MS, 5)).unwrap();
    assert_eq!(act!(p.a, set_ttl(NOW_MS, 7)), Err(ErrorCode::NotPermitted));
    p.settle();
    assert_eq!(p.b.seen.settings, vec![5]);
    assert_eq!(p.b.s.chat_ttl(), 5, "either person sets it for both");
    let seq = act!(p.a, send_chat(NOW_MS, b"secret", None)).unwrap();
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
    assert_eq!(p.b.seen.expired, vec![MsgRef { mine: false, seq }]);
    assert_eq!(p.a.seen.expired, vec![MsgRef { mine: true, seq }]);
    act!(p.b, set_ttl(NOW_MS, 0)).unwrap();
    p.settle();
    assert_eq!(p.a.seen.settings, vec![0]);
    act!(p.a, send_chat(NOW_MS, b"plain", None)).unwrap();
    p.settle();
    assert_eq!(p.b.seen.chats.last().unwrap().2, 0);
}

#[test]
fn resume_resends_pending_and_keeps_seq() {
    let mut p = connect();
    act!(p.a, send_chat(NOW_MS, b"one", None)).unwrap();
    p.settle();
    p.cut();
    assert_eq!((p.a.seen.suspended, p.b.seen.suspended), (1, 1));
    assert_eq!(p.a.s.state(), State::Suspended);
    // Typed while offline: queued; edit and delete rewrite the queue in place.
    let s2 = act!(p.a, send_chat(NOW_MS, b"two (draft)", None)).unwrap();
    let s3 = act!(p.a, send_chat(NOW_MS, b"three", None)).unwrap();
    let s4 = act!(p.a, send_chat(NOW_MS, b"four", None)).unwrap();
    act!(p.a, edit(NOW_MS, s2, b"two")).unwrap();
    act!(p.a, delete(NOW_MS, MsgRef { mine: true, seq: s3 })).unwrap();
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
    act!(p.b, send_chat(NOW_MS, b"back", Some(MsgRef { mine: false, seq: s4 }))).unwrap();
    p.settle();
    assert_eq!(p.a.seen.chats.last().unwrap().3, Some(MsgRef { mine: true, seq: s4 }));

    // Cut while frames are in flight: the unacked message is resent after the next resume.
    act!(p.a, send_chat(NOW_MS, b"lost in flight", None)).unwrap();
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
    assert_eq!(s.send_chat(NOW_MS, b"x", None, &mut sink(&mut w, &mut seen)), Err(ErrorCode::PeerOffline));
}

#[test]
fn tamper_closes() {
    let mut p = connect();
    act!(p.a, send_chat(NOW_MS, b"x", None)).unwrap();
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
