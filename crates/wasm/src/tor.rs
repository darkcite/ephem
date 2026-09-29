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
use ephem_core::room::OWNER_IDX;
use ephem_core::{Role, Session, State};
use ephem_crypto::PeerId;
use ephem_crypto::contacts::cflags;
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
/// A connected Tor link that hears nothing for this long is treated as lost (the peer PINGs
/// every 15 s when idle). arti may report a stream dead only minutes after a network change
/// (its channel and the Snowflake session wait out long timers); the dialler then redials.
const SILENT_MS: u64 = 45_000;
/// One dial attempt (descriptor, introduction, rendezvous) before it counts as failed.
const DIAL_TIMEOUT_MS: u32 = 30_000;
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
    /// Bootstrap failed: chats cannot connect (no fallback, §28.5).
    failed: bool,
}

/// ev::TOR for the UI: 1 starting (text = status), 2 ready (text = our .onion), 3 failed.
fn progress(n: f64, text: &str) {
    emit(ev::TOR, n, text.as_bytes());
}

/// Starts arti (Snowflake, bootstrap), hosts our onion service and accepts its streams.
pub(crate) fn start(inner: &Shared, sf: Snowflake, network_toml: &str, cache: &[u8]) -> Result<(), ErrorCode> {
    if inner.borrow().tor.tor.is_some() {
        return Err(ErrorCode::NotPermitted);
    }
    let tor = match Tor::new(sf, network_toml, cache) {
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
            inner.borrow_mut().tor.failed = true;
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
    let now_s = (now_ms() / 1000) as u32;
    let (inv, room) = crate::ids();
    let id = {
        let mut g = inner.borrow_mut();
        if g.tor.tor.is_none() {
            return Err(ErrorCode::TorUnavailable);
        }
        g.fresh()?;
        let s = Session::tor_host(g.identity(), inv, room, now_s + ttl_s.clamp(60, 1800), g.settings());
        g.add_link(1, s, false)
    };
    offer(inner, id);
    Ok(())
}

/// The TOR_INVITE of hosting link `id`: to the UI (CODE 5), or sealed to a room member through
/// the owner (§14.4). Nothing else to start: the peer dials us.
pub(crate) fn offer(inner: &Shared, id: u32) {
    // Copy of the 104-byte code out of the session (setup path): sending it borrows the tab.
    let mut code = [0u8; TOR_CODE_LEN];
    let (i, via_owner) = {
        let mut g = inner.borrow_mut();
        let Some(i) = g.find(id) else { return };
        code.copy_from_slice(g.links[i].sess.local_code());
        (i, g.links[i].via_owner)
    };
    let r = if via_owner { room::relay_code(inner, id, &code) } else { crate::emit_code(inner, i, &code) };
    if let Err(e) = r {
        crate::emit_err(ev::ERROR, e);
    }
}

/// A chat from a TOR_INVITE: dial the inviter's onion (in the background, until connected).
pub(crate) fn join(inner: &Shared, code: &[u8], now_s: u32, scanned: bool) -> Result<(), ErrorCode> {
    let lid = {
        let mut g = inner.borrow_mut();
        if g.tor.tor.is_none() {
            return Err(ErrorCode::TorUnavailable);
        }
        let f = Code::decode(code)?.flags;
        let group = f & flags::GROUP != 0;
        // Room links: no read receipts, no typing (§11.7).
        let settings = if group { room::room_settings(&g) } else { g.settings() };
        let s = Session::tor_dialer(g.identity(), code, now_s, settings, scanned)?;
        g.fresh()?;
        if group {
            g.room = Some(room::Room::joining(f & flags::OBSERVER != 0));
        }
        g.add_link(if group { OWNER_IDX } else { 0 }, s, false)
    };
    dial(inner, lid, false);
    Ok(())
}

/// Dials a contact's stored onion (§28.7, "Connect"): a new chat, no code.
pub(crate) fn call(inner: &Shared, peer: PeerId) -> Result<(), ErrorCode> {
    let lid = {
        let mut g = inner.borrow_mut();
        if g.tor.tor.is_none() {
            return Err(ErrorCode::TorUnavailable);
        }
        let c = g.saved.as_ref().and_then(|s| s.contacts.get(&peer)).copied().ok_or(ErrorCode::NotAContact)?;
        if c.flags & cflags::HAS_ONION == 0 {
            return Err(ErrorCode::NotPermitted);
        }
        // Added from their card and never connected: the card's secret opens the first dial.
        let secret = if c.flags & cflags::FROM_CARD != 0 { c.card_secret } else { [0; 16] };
        tracing::info!("tor: dialling a contact{}", if secret == [0; 16] { "" } else { " with their card's secret" });
        let s = Session::tor_contact_dialer(g.identity(), peer, c.onion_pk, g.settings(), secret);
        g.fresh()?;
        g.add_link(0, s, false)
    };
    dial(inner, lid, false);
    Ok(())
}

/// A contact dials (§28.7), or someone with our live contact card (§28.4 case 3: any key, the
/// user is asked next): a new chat. Returns the new link's index (in the loaded new chat) and
/// whether it came through the card.
fn contact_host(g: &mut Inner, now: u64, frame: &[u8], wire: &TorWire) -> Option<(usize, bool)> {
    let sv = g.saved.as_ref()?;
    let now_s = (now / 1000) as u32;
    let card = sv.contacts.card.filter(|k| k.live(now_s)).map(|k| k.secret);
    let contacts = !sv.contacts.list().is_empty();
    if !contacts && card.is_none() {
        return None;
    }
    g.fresh().ok()?;
    // Contacts (zero secret, known keys only), then the card (its secret, any key).
    for secret in [contacts.then_some([0u8; 16]), card].into_iter().flatten() {
        let s = Session::tor_contact_host(g.identity(), g.settings(), secret);
        g.add_link(1, s, false);
        let i = g.links.len() - 1;
        let taken = {
            let Inner { id, links, meta, inbox, saved, .. } = &mut *g;
            let known = &saved.as_ref()?.contacts;
            let Link { sess, rtc, id: lid, member, peer, .. } = &mut links[i];
            let mut out = Out { rtc: rtc.as_ref(), tor: Some(wire), meta, inbox, peer, link: *lid, member: *member };
            let from_card = secret != [0; 16];
            sess.tor_accept(id, now, frame, |k| from_card || known.get(k).is_some(), &mut |e| on_event(&mut out, e)) == Ok(true)
        };
        if taken {
            return Some((i, secret != [0; 16]));
        }
        g.links.pop();
    }
    None
}

/// The identity changed (sign-in, sign-out): host the new one's onion service, so invites and
/// contacts reach it (§28.7). Before the first service is up, `start` hosts the current one.
pub(crate) fn sync_identity(inner: &Shared) -> Result<(), ErrorCode> {
    let rehost = {
        let g = inner.borrow();
        let t = &g.tor;
        (!t.onion.is_empty() && t.onion != onion_address(&g.identity().onion_pk())).then(|| t.tor.clone()).flatten()
    };
    match rehost {
        Some(tor) => host(inner, &tor),
        None => Ok(()),
    }
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

/// The Tor directory for the next session's warm start (§28.3; public data), or empty.
pub(crate) fn cache(g: &Inner) -> Vec<u8> {
    g.tor.tor.as_ref().and_then(|t| t.cache()).unwrap_or_default()
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
    let card_used;
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
        card_used = forget_card_secret(&mut g, i);
    }
    if card_used {
        emit(ev::CARD, 2.0, &[]);
    }
    room::drain(inner);
    true
}

/// A contact added from a card has connected (§7.5): later dials are plain contact dials, so
/// its card's secret is dropped (the card may have been reset since). True if the key file
/// changed (ev::CARD 2: the page saves it).
fn forget_card_secret(g: &mut Inner, i: usize) -> bool {
    let Inner { links, saved, .. } = g;
    let l = &links[i];
    if !(l.sess.contact() && l.sess.role() == Role::Answerer && l.sess.ever_connected()) {
        return false;
    }
    let peer = l.sess.remote();
    match saved.as_mut() {
        Some(sv) if sv.contacts.get(&peer).is_some_and(|c| c.flags & cflags::FROM_CARD != 0) => {
            sv.contacts.card_used(&peer);
            true
        }
        _ => false,
    }
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
        // The old circuits may be the broken part: fresh ones from the first attempt.
        dial(inner, lid, true);
    }
}

/// Every second: connected Tor links that went silent are lost (see [`SILENT_MS`]).
pub(crate) fn tick(inner: &Shared) {
    let now = now_ms();
    let silent: Vec<u32> = {
        let g = inner.borrow();
        g.links.iter().filter(|l| l.tor.is_some() && l.sess.state() == State::Connected && l.sess.rx_idle_ms(now) > SILENT_MS).map(|l| l.id).collect()
    };
    for lid in silent {
        // A new path id orphans the old stream's reader (it may still report late).
        let fresh = {
            let mut g = inner.borrow_mut();
            g.find(lid).map(|i| g.new_path(i))
        };
        if let Some(lid) = fresh {
            lost(inner, lid);
        }
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
    let mut card_event = false;
    let taken = {
        let mut g = inner.borrow_mut();
        let now = now_ms();
        let mut taken = None;
        // The path id the taking link gets (new_path below), so its events already carry it.
        let next = g.next_id.wrapping_add(1);
        // Every chat's waiting Tor links, the loaded chat first.
        let (ids, count) = g.chat_ids();
        'chats: for &c in &ids[..count] {
            g.focus(c);
            for i in 0..g.links.len() {
                if !g.links[i].sess.tor() {
                    continue;
                }
                // A link to a room member takes only the key the signed state names (§14.4).
                let pinned = g.links[i].via_owner.then(|| g.room.as_ref().and_then(|r| r.member_key(g.links[i].member)));
                let Inner { id, links, meta, inbox, .. } = &mut *g;
                let Link { sess, rtc, member, peer, .. } = &mut links[i];
                // Our reply (IK message 2) goes to this new stream.
                let mut out = Out { rtc: rtc.as_ref(), tor: Some(&wire), meta, inbox, peer, link: next, member: *member };
                let allow = |k: &PeerId| pinned.is_none_or(|p| p == Some(*k));
                if sess.tor_accept(id, now, &buf[2..2 + n], allow, &mut |e| on_event(&mut out, e)) == Ok(true) {
                    taken = Some(i);
                    break 'chats;
                }
            }
        }
        match taken {
            Some(i) => {
                let lid = g.new_path(i);
                debug_assert_eq!(lid, next);
                g.links[i].tor = Some(wire);
                Some(lid)
            }
            None => contact_host(&mut g, now, &buf[2..2 + n], &wire).map(|(i, from_card)| {
                g.links[i].tor = Some(wire);
                if from_card {
                    // The UI asks its user before the chat starts (§28.4 case 3).
                    card_event = true;
                }
                g.links[i].id
            }),
        }
    };
    if card_event {
        emit(ev::CARD, 1.0, &[]);
    }
    if taken.is_none() {
        let g = inner.borrow();
        tracing::warn!("tor: an incoming stream matched no chat ({} chats, signed in: {}); dropped", g.chat_ids().1, g.saved.is_some());
    }
    // A stream for none of our chats is dropped unanswered (§28.4).
    if let Some(lid) = taken {
        buf.drain(..2 + n);
        reader(inner.clone(), lid, r, buf);
        room::drain(&inner);
    }
}

/// Dials the peer's onion for path `lid` (first connection or redial) until connected or the
/// chat ends.
/// `fresh`: the first attempt already uses new circuits (a redial after a loss).
pub(crate) fn dial(inner: &Shared, lid: u32, fresh: bool) {
    let inner = inner.clone();
    wasm_bindgen_futures::spawn_local(async move {
        let mut backoff = REDIAL_FIRST_MS;
        let mut failed = fresh;
        loop {
            let (tor, onion) = {
                let mut g = inner.borrow_mut();
                let Some(i) = g.find(lid) else { return };
                if g.links[i].sess.state() == State::Closed {
                    return;
                }
                let Some(tor) = g.tor.tor.clone() else { return };
                if g.tor.failed {
                    drop(g);
                    let mut g = inner.borrow_mut();
                    crate::on_link!(g, i, |s, k, _t| s.abort(ErrorCode::TorUnavailable, &mut k));
                    return;
                }
                (tor, onion_address(&g.links[i].sess.peer_onion()))
            };
            if !tor.ready() {
                sleep_ms(500).await;
                continue;
            }
            // After a failure, a fresh descriptor and fresh circuits (see `Tor::connect`). An
            // attempt that hangs counts as failed.
            let attempt = tor.connect(&onion, PORT, failed);
            let timeout = sleep_ms(DIAL_TIMEOUT_MS);
            futures::pin_mut!(attempt, timeout);
            let r = match futures::future::select(attempt, timeout).await {
                futures::future::Either::Left((r, _)) => r,
                futures::future::Either::Right(_) => Err("timed out".to_owned()),
            };
            match r {
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
                    failed = true;
                    tracing::warn!("tor: dial {onion}: {e}");
                    sleep_ms(backoff).await;
                    backoff = (backoff * 2).min(REDIAL_MAX_MS);
                }
            }
        }
    });
}
