//! Tor mode of the adapter (§28): chat links over Tor streams to onion services.
//!
//! The tab hosts one onion service whose key is derived from the identity seed; its invites
//! are TOR_INVITEs (§28.4). Every incoming stream is offered to the waiting chats until one
//! accepts its first handshake message ([`ephem_core::Session::tor_accept`]); nothing else is
//! read from a stream before that. A dialer connects to the inviter's onion and redials after
//! a loss (§28.5: no recovery ladder).
//!
//! On the stream every core frame is prefixed with its length, `u16` little-endian (§28.5).
//! Copies (§11.6): arti delivers plaintext in its own buffers → one copy into the link's
//! reassembly buffer (frames span reads) → one into the preallocated RX slot, where the frame
//! is decrypted in place. Sending, one copy into the write buffer (arti writes
//! asynchronously). Per-stream setup allocates; the per-frame path reuses those buffers.

use crate::{Inner, Link, Out, Shared, emit, ev, now_ms, on_event, room};
use ephem_core::{Role, Session, State};
use ephem_proto::ErrorCode;
use ephem_proto::code::{Code, TOR_CODE_LEN, flags};
use ephem_proto::frame::MAX_FRAME;
use ephem_tor::web::{DataReader, DataStream, DataWriter, Snowflake, Tor, onion_address, sleep_ms};
use futures::channel::mpsc;
use futures::{AsyncReadExt, AsyncWriteExt, StreamExt};
use std::cell::RefCell;
use std::rc::Rc;

/// The virtual port of Ephem onion services (fixed, §28.4).
pub(crate) const PORT: u16 = 1;
/// Onion service nickname (arti-local, never on the network).
const NICK: &str = "ephem";
/// Redial backoff (§12: 1, 2, 4, 8, 16 s).
const REDIAL_FIRST_MS: u32 = 1_000;
const REDIAL_MAX_MS: u32 = 16_000;
/// Reassembly buffer of a stream: one frame plus a read's worth.
const RX_CAP: usize = 2 + MAX_FRAME + 4096;

/// A link's Tor stream, write side: frames go to a buffer the writer task drains.
pub(crate) struct TorWire {
    tx: Rc<RefCell<Vec<u8>>>,
    kick: mpsc::UnboundedSender<()>,
}

impl TorWire {
    pub(crate) fn send(&self, frame: &[u8]) {
        debug_assert!(frame.len() <= MAX_FRAME);
        {
            let mut b = self.tx.borrow_mut();
            b.extend_from_slice(&(frame.len() as u16).to_le_bytes());
            b.extend_from_slice(frame);
        }
        let _ = self.kick.unbounded_send(());
    }

    /// Stops the writer (the stream closes when both halves are dropped).
    pub(crate) fn close(&self) {
        self.kick.close_channel();
    }
}

/// Tor state of the tab.
#[derive(Default)]
pub(crate) struct TorState {
    pub(crate) tor: Option<Rc<Tor>>,
    /// Our `.onion` address (empty until hosted).
    pub(crate) onion: String,
    /// Services hosted so far (each gets its own nickname).
    hosted: u32,
}

/// ev::TOR for the UI: 1 starting (text = status), 2 ready (text = our .onion), 3 failed.
fn progress(n: f64, text: &str) {
    emit(ev::TOR, n, text.as_bytes());
}

/// Starts arti (Snowflake, bootstrap), hosts our onion service and accepts its streams.
pub(crate) fn start(inner: &Shared, sf: Snowflake, network_toml: &str) -> Result<(), ErrorCode> {
    if inner.borrow().tor.tor.is_some() {
        return Err(ErrorCode::NotPermitted);
    }
    let tor = match Tor::new(sf, network_toml) {
        Ok(t) => Rc::new(t),
        Err(e) => {
            progress(3.0, &e);
            return Err(ErrorCode::TorUnavailable);
        }
    };
    inner.borrow_mut().tor.tor = Some(tor.clone());
    progress(1.0, "starting");
    let inner = inner.clone();
    wasm_bindgen_futures::spawn_local(async move {
        if let Err(e) = tor.bootstrap().await {
            progress(3.0, &e);
            return;
        }
        if host(&inner, &tor).is_err() {
            return;
        }
        loop {
            let s = tor.accept().await;
            wasm_bindgen_futures::spawn_local(incoming(inner.clone(), s));
        }
    });
    Ok(())
}

/// A new 1:1 chat and its TOR_INVITE (emitted as CODE 5). Streams for it arrive at our onion.
pub(crate) fn invite(inner: &Shared, ttl_s: u32) -> Result<(), ErrorCode> {
    // The identity changed since the service went up (sign-in): host the new one's (§28.7).
    let rehost = {
        let g = inner.borrow();
        let t = &g.tor;
        (!t.onion.is_empty() && t.onion != onion_address(&g.identity().onion_pk())).then(|| t.tor.clone()).flatten()
    };
    if let Some(tor) = rehost {
        host(inner, &tor)?;
    }
    let now_s = (now_ms() / 1000) as u32;
    let (inv, room) = crate::ids();
    // Copy of the 104-byte code out of the session (setup path): emitting borrows the tab.
    let mut code = [0u8; TOR_CODE_LEN];
    {
        let mut g = inner.borrow_mut();
        if g.tor.tor.is_none() {
            return Err(ErrorCode::TorUnavailable);
        }
        g.reset();
        let s = Session::tor_host(g.identity(), inv, room, now_s + ttl_s.clamp(60, 1800), g.settings());
        code.copy_from_slice(s.local_code());
        g.add_link(1, s, false);
    }
    crate::emit_code(inner, 0, &code)
}

/// A chat from a TOR_INVITE: dial the inviter's onion (in the background, until connected).
pub(crate) fn join(inner: &Shared, code: &[u8], now_s: u32, scanned: bool) -> Result<(), ErrorCode> {
    let lid = {
        let mut g = inner.borrow_mut();
        if g.tor.tor.is_none() {
            return Err(ErrorCode::TorUnavailable);
        }
        // Rooms over Tor: TOR-4.
        if Code::decode(code)?.flags & flags::GROUP != 0 {
            return Err(ErrorCode::NotPermitted);
        }
        let s = Session::tor_dialer(g.identity(), code, now_s, g.settings(), scanned)?;
        g.reset();
        g.add_link(0, s, false)
    };
    dial(inner, lid);
    Ok(())
}

/// Hosts the onion service of the tab's current identity (replacing the previous one).
fn host(inner: &Shared, tor: &Tor) -> Result<(), ErrorCode> {
    let (mut secret, n) = {
        let mut g = inner.borrow_mut();
        g.tor.hosted += 1;
        (g.identity().onion_secret(), g.tor.hosted)
    };
    // arti names every service; a replacement needs a new name.
    let hosted = tor.host(&format!("{NICK}{n}"), &secret);
    secret.fill(0);
    match hosted {
        Ok(onion) => {
            progress(2.0, &onion);
            inner.borrow_mut().tor.onion = onion;
            Ok(())
        }
        Err(e) => {
            progress(3.0, &e);
            Err(ErrorCode::TorUnavailable)
        }
    }
}

/// Bootstrap status line for the UI.
pub(crate) fn status(g: &Inner) -> String {
    g.tor.tor.as_ref().map_or_else(String::new, |t| t.status())
}

/// The write half: a task that drains the link's buffer into the stream.
fn writer(mut w: DataWriter) -> TorWire {
    let tx = Rc::new(RefCell::new(Vec::with_capacity(4096)));
    let (kick, mut rx) = mpsc::unbounded::<()>();
    let buf = tx.clone();
    wasm_bindgen_futures::spawn_local(async move {
        let mut out = Vec::with_capacity(4096);
        while rx.next().await.is_some() {
            core::mem::swap(&mut out, &mut *buf.borrow_mut());
            if out.is_empty() {
                continue;
            }
            if w.write_all(&out).await.is_err() || w.flush().await.is_err() {
                return;
            }
            out.clear();
        }
        let _ = w.close().await;
    });
    TorWire { tx, kick }
}

/// The read half of link path `lid`: frames into the session until the stream ends, then the
/// path is lost (and a dialer redials).
fn reader(inner: Shared, lid: u32, mut r: DataReader, mut buf: Vec<u8>) {
    wasm_bindgen_futures::spawn_local(async move {
        let mut chunk = vec![0u8; 4096];
        'stream: loop {
            // Every complete frame in the buffer.
            loop {
                if buf.len() < 2 {
                    break;
                }
                let n = u16::from_le_bytes([buf[0], buf[1]]) as usize;
                if n == 0 || n > MAX_FRAME {
                    break 'stream;
                }
                if buf.len() < 2 + n {
                    break;
                }
                if !deliver(&inner, lid, &buf[2..2 + n]) {
                    return; // the link took another path or is gone
                }
                buf.drain(..2 + n);
            }
            match r.read(&mut chunk).await {
                Ok(0) | Err(_) => break,
                Ok(m) => {
                    if buf.len() + m > RX_CAP {
                        break;
                    }
                    buf.extend_from_slice(&chunk[..m]);
                }
            }
        }
        lost(&inner, lid);
    });
}

/// One frame to the session of path `lid`. False if that path no longer exists.
fn deliver(inner: &Shared, lid: u32, frame: &[u8]) -> bool {
    {
        let mut g = inner.borrow_mut();
        let Some(i) = g.find(lid) else { return false };
        let now = now_ms();
        let Inner { links, meta, inbox, rx, .. } = &mut *g;
        let Link { sess, rtc, tor, id, member, peer, .. } = &mut links[i];
        // The frame is decrypted in place in the RX slot (the one copy from the stream).
        rx[..frame.len()].copy_from_slice(frame);
        let mut out = Out { rtc: rtc.as_ref(), tor: tor.as_ref(), meta, inbox, peer, link: *id, member: *member };
        sess.on_frame(now, &mut rx[..frame.len()], &mut |e| on_event(&mut out, e));
    }
    room::drain(inner);
    true
}

/// The stream of path `lid` ended: the chat is suspended; the dialer dials again.
fn lost(inner: &Shared, lid: u32) {
    let redial = {
        let mut g = inner.borrow_mut();
        let Some(i) = g.find(lid) else { return };
        let now = now_ms();
        crate::on_link!(g, i, |s, k, _t| s.path_lost(now, ErrorCode::IceFailed, &mut k));
        let l = &g.links[i];
        (l.sess.role() == Role::Answerer && l.sess.state() == State::Suspended).then(|| g.new_path(i))
    };
    if let Some(lid) = redial {
        dial(inner, lid);
    }
}

/// An incoming stream: its first frame decides which chat it belongs to.
async fn incoming(inner: Shared, s: DataStream) {
    let (mut r, w) = s.split();
    let mut buf = Vec::with_capacity(RX_CAP);
    let mut chunk = [0u8; 2048];
    let n = loop {
        if buf.len() >= 2 {
            let n = u16::from_le_bytes([buf[0], buf[1]]) as usize;
            if n == 0 || n > MAX_FRAME {
                return;
            }
            if buf.len() >= 2 + n {
                break n;
            }
        }
        match r.read(&mut chunk).await {
            Ok(0) | Err(_) => return,
            Ok(m) => buf.extend_from_slice(&chunk[..m]),
        }
    };
    let wire = writer(w);
    let taken = {
        let mut g = inner.borrow_mut();
        let now = now_ms();
        let mut taken = None;
        // The path id the taking link gets (new_path below), so its events already carry it.
        let next = g.next_id.wrapping_add(1);
        for i in 0..g.links.len() {
            if !g.links[i].sess.tor() {
                continue;
            }
            let Inner { id, links, meta, inbox, .. } = &mut *g;
            let Link { sess, rtc, member, peer, .. } = &mut links[i];
            // Our reply (IK message 2) goes to this new stream.
            let mut out = Out { rtc: rtc.as_ref(), tor: Some(&wire), meta, inbox, peer, link: next, member: *member };
            if sess.tor_accept(id, now, &buf[2..2 + n], &mut |e| on_event(&mut out, e)) == Ok(true) {
                taken = Some(i);
                break;
            }
        }
        taken.map(|i| {
            let lid = g.new_path(i);
            debug_assert_eq!(lid, next);
            g.links[i].tor = Some(wire);
            lid
        })
    };
    // A stream for none of our chats is dropped unanswered (§28.4).
    if let Some(lid) = taken {
        buf.drain(..2 + n);
        reader(inner.clone(), lid, r, buf);
        room::drain(&inner);
    }
}

/// Dials the peer's onion for path `lid` (first connection or redial) until connected or the
/// chat ends.
pub(crate) fn dial(inner: &Shared, lid: u32) {
    let inner = inner.clone();
    wasm_bindgen_futures::spawn_local(async move {
        let mut backoff = REDIAL_FIRST_MS;
        loop {
            let (tor, onion) = {
                let g = inner.borrow();
                let Some(i) = g.find(lid) else { return };
                if g.links[i].sess.state() == State::Closed {
                    return;
                }
                let Some(tor) = g.tor.tor.clone() else { return };
                (tor, onion_address(&g.links[i].sess.peer_onion()))
            };
            if !tor.ready() {
                sleep_ms(500).await;
                continue;
            }
            match tor.connect(&onion, PORT).await {
                Ok(s) => {
                    let (r, w) = s.split();
                    let mut g = inner.borrow_mut();
                    let Some(i) = g.find(lid) else { return };
                    let Inner { id, links, .. } = &mut *g;
                    if links[i].sess.tor_dial(id).is_err() {
                        return;
                    }
                    links[i].tor = Some(writer(w));
                    let now = now_ms();
                    crate::on_link!(g, i, |s, k, _t| s.on_open(now, &mut k));
                    drop(g);
                    reader(inner.clone(), lid, r, Vec::with_capacity(RX_CAP));
                    return;
                }
                Err(e) => {
                    tracing::info!("tor: dial {onion}: {e}");
                    sleep_ms(backoff).await;
                    backoff = (backoff * 2).min(REDIAL_MAX_MS);
                }
            }
        }
    });
}
