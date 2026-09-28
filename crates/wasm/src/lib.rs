//! Ephem browser adapter (docs/P2P-CHAT.md §6.1): the only crate that touches the browser.
//!
//! Rust owns all state; JS renders the DOM and forwards user input. Events go to JS through one
//! imported function, `ephemEvent(kind, num, ptr, len)`, where `ptr/len` points into wasm linear
//! memory (no copy on our side; JS decodes text with `TextDecoder` over a memory view, §11.6).
//! Extra fixed-layout fields of an event are written to the `meta` block (see [`meta`]); every
//! event of a link carries that link's member index at [`meta::MEMBER`].
//!
//! State: a set of **links** (one pairwise session + RTCPeerConnection each): exactly one for a
//! 1:1 chat, up to 15 in a room (§14), plus the room itself ([`room::Room`]).
//!
//! **Re-entrancy rule for JS:** an `ephemEvent` handler must not call back into `App`
//! synchronously (it may update the DOM; anything else goes through `queueMicrotask`).

mod qr;
mod room;
mod rtc;
#[cfg(feature = "tor")]
mod tor;

/// A chat link's Tor stream (Tor build only, §28): frames with a u16 length prefix.
#[cfg(feature = "tor")]
pub(crate) use tor::TorWire;
/// The direct build has no Tor streams (uninhabited).
#[cfg(not(feature = "tor"))]
pub(crate) enum TorWire {}
#[cfg(not(feature = "tor"))]
impl TorWire {
    pub(crate) fn send(&self, _frame: &[u8]) {
        match *self {}
    }
    pub(crate) fn close(&self) {
        match *self {}
    }
}

use core::cell::RefCell;
use ephem_core::room::{OWNER_IDX, RoomRole};
use ephem_core::{Event, MsgRef, Privacy, Session, Settings, State};
use ephem_crypto::contacts::{ContactError, Contacts};
use ephem_crypto::{Identity, PeerId, keyfile};
use ephem_proto::ErrorCode;
use ephem_proto::b64url;
use ephem_proto::buf::Buf;
use ephem_proto::code::{Code, Kind, MAX_CODE_LEN, flags};
use ephem_proto::frame::{MAX_FRAME, MAX_TEXT};
use std::rc::Rc;
use wasm_bindgen::prelude::*;
use zeroize::Zeroizing;

pub use qr::{qr_svg_path, scan_rgba};

#[wasm_bindgen]
extern "C" {
    #[wasm_bindgen(js_name = ephemEvent)]
    fn js_event(kind: u32, num: f64, ptr: u32, len: u32);
}

/// Event kinds for `ephemEvent` (mirrored in `app/app.js`).
pub mod ev {
    /// num = code kind (1 invite, 2 answer, 3 resume invite, 4 resume answer); text = base64url.
    pub const CODE: u32 = 1;
    /// num = SAS digits; bytes = 4 emoji indices ‖ 11-byte peer handle; meta[0] = resumed.
    pub const CONNECTED: u32 = 2;
    /// num = 1 if the SAS check is optional (both codes scanned); text = nickname.
    pub const HELLO: u32 = 3;
    /// num = chat_seq; text = UTF-8; meta: ttl_s u32 @0, has_reply u8 @4, reply_sender u8 @5, reply_seq f64 @8.
    pub const CHAT: u32 = 4;
    /// num = cumulative delivered chat_seq (on the link of meta[MEMBER]).
    pub const DELIVERED: u32 = 5;
    pub const DEGRADED: u32 = 6;
    pub const ALIVE: u32 = 7;
    /// num = 1 hidden / 0 visible.
    pub const PEER_HIDDEN: u32 = 8;
    /// A link ended. num = error code; text = `E_...` name.
    pub const CLOSED: u32 = 9;
    /// Non-fatal failure of a user action. num = error code; text = `E_...` name.
    pub const ERROR: u32 = 10;
    /// Progress. num: 1 = gathering, 2 = connecting, 3 = channel open.
    pub const PROGRESS: u32 = 11;
    /// Diagnostics of the selected path (getStats); text = human readable. num bit0 = relay seen.
    pub const PATH: u32 = 12;
    /// A member set the self-destruct timer. num = seconds (0 = off).
    pub const SETTING: u32 = 13;
    /// A member edited its message. num = seq; text = new text.
    pub const EDITED: u32 = 14;
    /// Deleted for everyone. num = seq; meta[5] = sender.
    pub const DELETED: u32 = 15;
    /// Self-destruct fired. num = seq; meta[5] = sender.
    pub const EXPIRED: u32 = 16;
    /// Cumulative read chat_seq.
    pub const READ: u32 = 17;
    /// num = 1 typing / 0 stopped.
    pub const TYPING: u32 = 18;
    /// Path lost; the link waits for a reconnect code (or T2 in a room).
    pub const SUSPENDED: u32 = 19;
    /// Reaction. num = target seq; meta[5] = target sender; text = emoji (empty = removed).
    pub const REACTION: u32 = 20;
    /// Identity transfer (§7.6): the new device confirmed the SAS.
    pub const PEER_READY: u32 = 21;
    /// Identity transfer: our key file was sent.
    pub const IDENTITY_SENT: u32 = 22;
    /// Identity transfer: bytes = the received (still encrypted) key file.
    pub const IDENTITY_RECEIVED: u32 = 23;
    /// Room state changed (members, roles, links). num = state version (0 = not joined yet).
    pub const ROOM: u32 = 24;
    /// The room is gone for us. num = error code (`E_ROOM_DISPOSED`, `E_NOT_PERMITTED` = removed).
    pub const ROOM_CLOSED: u32 = 25;
    /// Tor build (§28): num = 1 starting (text = bootstrap status), 2 ready (text = our
    /// `.onion`), 3 failed (text = reason).
    pub const TOR: u32 = 26;
}

/// Byte offsets inside the meta block.
pub mod meta {
    pub const TTL: usize = 0;
    pub const HAS_REPLY: usize = 4;
    /// Reply target sender (CHAT), or the message sender (DELETED, EXPIRED, REACTION).
    pub const SENDER: usize = 5;
    pub const REPLY_SEQ: usize = 8;
    pub const RESUMED: usize = 0;
    /// Member index of the peer of the link that produced the event (0xFF: not admitted yet).
    pub const MEMBER: usize = 16;
    pub const LEN: usize = 24;
}

#[inline(always)]
fn emit(kind: u32, num: f64, bytes: &[u8]) {
    js_event(kind, num, bytes.as_ptr() as u32, bytes.len() as u32);
}

#[inline]
fn emit_err(kind: u32, e: ErrorCode) {
    emit(kind, e.code() as f64, e.name().as_bytes());
}

/// CODE for the UI: `code` (of link `i`) as base64url in the scratch buffer; num = its kind.
pub(crate) fn emit_code(inner: &Shared, i: usize, code: &[u8]) -> Result<(), ErrorCode> {
    let mut g = inner.borrow_mut();
    let len = b64url::encode(code, &mut g.scratch[..]).map_err(|_| ErrorCode::InvalidInvite)?;
    g.meta[meta::MEMBER] = g.links[i].member;
    let ptr = g.scratch.as_ptr() as u32;
    drop(g);
    // Emitted after the borrow ends; the scratch buffer is stable (boxed at start).
    js_event(ev::CODE, code[1] as f64, ptr, len as u32);
    Ok(())
}

#[inline(always)]
fn now_ms() -> u64 {
    js_sys::Date::now() as u64
}

/// User preferences applied to the next chat.
#[derive(Copy, Clone)]
struct Prefs {
    privacy: Privacy,
    drop_ipv6: bool,
    settings: Settings,
}

/// A saved identity's file key (kept so it can be re-saved without asking again, §7.3), its
/// contacts (§7.5) and the key-file sections this version does not know (kept verbatim).
struct Saved {
    key: Zeroizing<[u8; 32]>,
    salt: [u8; 16],
    label: Vec<u8>,
    contacts: Contacts,
    others: Zeroizing<Vec<u8>>,
}

/// The member index of a link whose peer the owner has not admitted yet.
pub(crate) const PENDING: u8 = 0xFF;

/// One pairwise link: a session and its RTCPeerConnection.
pub(crate) struct Link {
    /// Unique for every path; late callbacks of an older RTCPeerConnection are ignored.
    pub(crate) id: u32,
    /// The peer's member index (1:1: its PeerIdx; room: its room index, or [`PENDING`]).
    pub(crate) member: u8,
    pub(crate) sess: Box<Session>,
    pub(crate) rtc: Option<rtc::Rtc>,
    /// Tor mode: the link's Tor stream instead of an RTCPeerConnection.
    pub(crate) tor: Option<TorWire>,
    /// Codes of this link go to the peer sealed through the room owner (§14.4), not to the UI.
    pub(crate) via_owner: bool,
    /// Last automatic T2 attempt (ms).
    pub(crate) t2_at: u64,
    /// Room owner: the state version this member has been sent.
    pub(crate) synced: u32,
    pub(crate) peer: PeerInfo,
}

/// What the peer of a link told us: its nickname (HELLO) and the link's SAS digits.
#[derive(Default)]
pub(crate) struct PeerInfo {
    pub(crate) nick: Vec<u8>,
    pub(crate) sas: u32,
}

/// Everything the tab owns. Buffers are allocated once at start and reused.
pub(crate) struct Inner {
    id: Identity,
    saved: Option<Saved>,
    prefs: Prefs,
    pub(crate) links: Vec<Link>,
    pub(crate) room: Option<room::Room>,
    next_id: u32,
    /// Room records from link events (link id, record type, body), handled after the core call.
    pub(crate) inbox: Vec<(u32, u8, Vec<u8>)>,
    rx: Box<[u8; MAX_FRAME]>,
    text: Box<[u8; MAX_TEXT]>,
    /// Encoded codes and small event payloads.
    scratch: Box<[u8; 1024]>,
    meta: Box<[u8; meta::LEN]>,
    /// RGBA camera frame for the QR scanner; sized on first use (setup path).
    scan: Vec<u8>,
    /// Tor build: arti and our onion service (§28).
    #[cfg(feature = "tor")]
    tor: tor::TorState,
}

pub(crate) type Shared = Rc<RefCell<Inner>>;

/// Runs a session action on link `$i` with its sink, splitting the borrows of `Inner` so the
/// text buffer can be read while the session is mutated.
macro_rules! on_link {
    ($g:expr, $i:expr, |$s:ident, $k:ident, $text:ident| $body:expr) => {{
        let $crate::Inner { links, meta, inbox, text: $text, .. } = &mut *$g;
        let $crate::Link { sess: $s, rtc, tor, id, member, peer, .. } = &mut links[$i];
        let mut out = $crate::Out { rtc: rtc.as_ref(), tor: tor.as_ref(), meta: &mut **meta, inbox, peer, link: *id, member: *member };
        let mut $k = |e: ephem_core::Event<'_>| $crate::on_event(&mut out, e);
        let _ = &$text;
        $body
    }};
}
pub(crate) use on_link;

impl Inner {
    #[inline]
    pub(crate) fn find(&self, id: u32) -> Option<usize> {
        self.links.iter().position(|l| l.id == id)
    }

    #[inline]
    pub(crate) fn by_member(&self, member: u8) -> Option<usize> {
        self.links.iter().position(|l| l.member == member)
    }

    fn next_id(&mut self) -> u32 {
        self.next_id = self.next_id.wrapping_add(1);
        self.next_id
    }

    pub(crate) fn add_link(&mut self, member: u8, sess: Session, via_owner: bool) -> u32 {
        let id = self.next_id();
        self.links.push(Link { id, member, sess: Box::new(sess), rtc: None, tor: None, via_owner, t2_at: 0, synced: 0, peer: PeerInfo::default() });
        id
    }

    /// A new RTCPeerConnection for link `i` (T3/T2 resume): the old one is closed, the id changes.
    pub(crate) fn new_path(&mut self, i: usize) -> u32 {
        if let Some(r) = self.links[i].rtc.take() {
            r.close();
        }
        if let Some(t) = self.links[i].tor.take() {
            t.close();
        }
        let id = self.next_id();
        self.links[i].id = id;
        id
    }

    /// Ends link `i` (GOODBYE if connected) and removes it.
    pub(crate) fn close_link(&mut self, i: usize) {
        let now = now_ms();
        on_link!(self, i, |s, k, _t| s.close(now, &mut k));
        let l = self.links.remove(i);
        if let Some(r) = l.rtc {
            r.close();
        }
        if let Some(t) = l.tor {
            t.close();
        }
    }

    /// Ends everything: every link and the room.
    fn reset(&mut self) {
        while !self.links.is_empty() {
            self.close_link(self.links.len() - 1);
        }
        self.room = None;
        self.inbox.clear();
    }

    /// The link of a 1:1 chat (or of the room owner, for a member).
    pub(crate) fn primary(&self) -> Option<usize> {
        match &self.room {
            Some(r) if !r.owner => self.by_member(OWNER_IDX),
            Some(_) => None,
            None => (!self.links.is_empty()).then_some(0),
        }
    }

    #[inline(always)]
    pub(crate) fn identity(&self) -> &Identity {
        &self.id
    }

    #[inline(always)]
    pub(crate) fn settings(&self) -> Settings {
        self.prefs.settings
    }

    #[inline(always)]
    pub(crate) fn privacy(&self) -> (Privacy, bool) {
        (self.prefs.privacy, self.prefs.drop_ipv6)
    }
}

/// Where a link's core events go: the DataChannel, JS, and the room inbox.
pub(crate) struct Out<'a> {
    pub(crate) rtc: Option<&'a rtc::Rtc>,
    pub(crate) tor: Option<&'a TorWire>,
    pub(crate) meta: &'a mut [u8; meta::LEN],
    pub(crate) inbox: &'a mut Vec<(u32, u8, Vec<u8>)>,
    pub(crate) peer: &'a mut PeerInfo,
    pub(crate) link: u32,
    pub(crate) member: u8,
}

pub(crate) fn on_event(o: &mut Out<'_>, e: Event<'_>) {
    o.meta[meta::MEMBER] = o.member;
    match e {
        Event::Send(frame) => {
            if let Some(t) = o.tor {
                t.send(frame);
            } else if let Some(r) = o.rtc {
                r.send(frame);
            }
        }
        Event::Connected { sas, peer, resumed } => {
            let mut b = [0u8; 15];
            b[..4].copy_from_slice(&sas.emoji);
            b[4..].copy_from_slice(&peer.handle());
            o.meta[meta::RESUMED] = resumed as u8;
            o.peer.sas = sas.digits;
            emit(ev::CONNECTED, sas.digits as f64, &b);
        }
        Event::Hello { nick, sas_optional, .. } => {
            o.peer.nick.clear();
            o.peer.nick.extend_from_slice(nick);
            // A room owner admits the peer once its HELLO (signing key) is in (§14.2).
            o.inbox.push((o.link, room::HELLO, Vec::new()));
            emit(ev::HELLO, sas_optional as u8 as f64, nick);
        }
        Event::Chat { msg, text, ttl_s, reply } => {
            o.meta[meta::TTL..meta::TTL + 4].copy_from_slice(&ttl_s.to_le_bytes());
            o.meta[meta::HAS_REPLY] = reply.is_some() as u8;
            let r = reply.unwrap_or(MsgRef::NONE);
            o.meta[meta::SENDER] = r.sender;
            o.meta[meta::REPLY_SEQ..meta::REPLY_SEQ + 8].copy_from_slice(&(r.seq as f64).to_le_bytes());
            emit(ev::CHAT, msg.seq as f64, text);
        }
        Event::Setting { ttl_s, .. } => emit(ev::SETTING, ttl_s as f64, &[]),
        Event::Edited { msg, text } => emit(ev::EDITED, msg.seq as f64, text),
        Event::Deleted(m) => {
            o.meta[meta::SENDER] = m.sender;
            emit(ev::DELETED, m.seq as f64, &[]);
        }
        Event::Expired(m) => {
            o.meta[meta::SENDER] = m.sender;
            emit(ev::EXPIRED, m.seq as f64, &[]);
        }
        Event::Reaction { msg, emoji, .. } => {
            o.meta[meta::SENDER] = msg.sender;
            emit(ev::REACTION, msg.seq as f64, emoji);
        }
        Event::Room { rtype, body } => o.inbox.push((o.link, rtype, body.to_vec())),
        Event::SignalOffer(ice) | Event::SignalAnswer(ice) => {
            if let Some(r) = o.rtc {
                r.on_signal(matches!(e, Event::SignalOffer(_)), ice);
            }
        }
        Event::PeerReady => emit(ev::PEER_READY, 0.0, &[]),
        Event::IdentitySent => emit(ev::IDENTITY_SENT, 0.0, &[]),
        Event::IdentityReceived(blob) => emit(ev::IDENTITY_RECEIVED, 0.0, blob),
        Event::Delivered { seq } => emit(ev::DELIVERED, seq as f64, &[]),
        Event::Read { seq } => emit(ev::READ, seq as f64, &[]),
        Event::PeerTyping(t) => emit(ev::TYPING, t as u8 as f64, &[]),
        Event::Degraded => emit(ev::DEGRADED, 0.0, &[]),
        Event::Alive => emit(ev::ALIVE, 0.0, &[]),
        Event::PeerHidden(h) => emit(ev::PEER_HIDDEN, h as u8 as f64, &[]),
        Event::Suspended => emit(ev::SUSPENDED, 0.0, &[]),
        Event::Closed(e) => emit_err(ev::CLOSED, e),
    }
}

fn hex(b: &[u8]) -> String {
    const H: &[u8; 16] = b"0123456789abcdef";
    b.iter().flat_map(|x| [H[(x >> 4) as usize] as char, H[(x & 15) as usize] as char]).collect()
}

fn peer_from_hex(s: &str) -> Option<PeerId> {
    if s.len() != 64 {
        return None;
    }
    let mut out = [0u8; 32];
    for (i, o) in out.iter_mut().enumerate() {
        *o = u8::from_str_radix(s.get(2 * i..2 * i + 2)?, 16).ok()?;
    }
    Some(PeerId(out))
}

#[inline]
fn contact_err(e: ContactError) -> ErrorCode {
    match e {
        ContactError::Full | ContactError::BadNick => ErrorCode::NotPermitted,
        ContactError::NotFound => ErrorCode::NotAContact,
    }
}

#[inline]
fn status(r: Result<(), ErrorCode>) -> u32 {
    match r {
        Ok(()) => 0,
        Err(e) => {
            emit_err(ev::ERROR, e);
            e.code() as u32
        }
    }
}

#[inline]
fn result_f64(r: Result<f64, ErrorCode>) -> f64 {
    r.unwrap_or_else(|e| -(e.code() as f64))
}

pub(crate) fn ids() -> ([u8; 16], [u8; 16]) {
    let mut b = [0u8; 32];
    ephem_crypto::random(&mut b);
    let mut x = [0u8; 16];
    let mut y = [0u8; 16];
    x.copy_from_slice(&b[..16]);
    y.copy_from_slice(&b[16..]);
    (x, y)
}

#[wasm_bindgen]
pub struct App {
    inner: Shared,
}

/// Tor build only (tor.html, §28).
#[cfg(feature = "tor")]
#[wasm_bindgen]
impl App {
    /// Starts arti over Snowflake, then hosts our onion service (key from the identity seed).
    /// Progress arrives as TOR events. `ice`: comma-separated `stun:` URLs; `nat`: the broker's
    /// NAT hint (empty = "unknown"); `network_toml`: empty for the real Tor network.
    pub fn tor_start(&self, broker: &str, fingerprint: &str, ice: &str, nat: &str, network_toml: &str) -> u32 {
        let sf = ephem_tor::web::Snowflake {
            broker: broker.to_owned(),
            fingerprint: fingerprint.to_owned(),
            ice: ice.split(',').map(str::trim).filter(|s| !s.is_empty()).map(str::to_owned).collect(),
            nat: if nat.is_empty() { "unknown".to_owned() } else { nat.to_owned() },
        };
        status(tor::start(&self.inner, sf, network_toml))
    }

    /// Bootstrap status line (empty before `tor_start`).
    pub fn tor_status(&self) -> String {
        tor::status(&self.inner.borrow())
    }

    /// Our `.onion` address (empty until the service is up).
    pub fn onion(&self) -> String {
        self.inner.borrow().tor.onion.clone()
    }

    /// Dials a contact's onion (§28.7): the chat opens when the contact's Tor tab accepts.
    pub fn contact_connect(&self, peer_hex: &str) -> u32 {
        status(peer_from_hex(peer_hex).ok_or(ErrorCode::NotAContact).and_then(|p| tor::call(&self.inner, p)))
    }

    /// arti logs to the console at `level` (`"info"`, `"debug"`, …; diagnostics only).
    pub fn tor_log(&self, level: &str) {
        ephem_tor::web::tor_log(level);
    }
}

#[wasm_bindgen]
impl App {
    /// Starts with a fresh temporary identity (§7.2 default).
    #[wasm_bindgen(constructor)]
    pub fn new() -> App {
        App {
            inner: Rc::new(RefCell::new(Inner {
                id: Identity::generate(),
                saved: None,
                prefs: Prefs { privacy: Privacy::Default, drop_ipv6: false, settings: Settings::default() },
                links: Vec::with_capacity(ephem_core::room::MAX_MEMBERS),
                room: None,
                next_id: 0,
                inbox: Vec::new(),
                rx: Box::new([0; MAX_FRAME]),
                text: Box::new([0; MAX_TEXT]),
                scratch: Box::new([0; 1024]),
                meta: Box::new([0; meta::LEN]),
                scan: Vec::new(),
                #[cfg(feature = "tor")]
                tor: tor::TorState::default(),
            })),
        }
    }

    // ---- identity (§7.2, §7.3) ----

    /// Display handle (`anon_xxxxxx`), not authentication.
    pub fn handle(&self) -> String {
        let h = self.inner.borrow().id.peer_id().handle();
        String::from_utf8_lossy(&h).into_owned()
    }

    /// Web Lock name for the current identity (§7.2: one identity per tab).
    pub fn lock_name(&self) -> String {
        let l = self.inner.borrow().id.peer_id().lock_name();
        String::from_utf8_lossy(&l).into_owned()
    }

    /// Label of the saved identity in use, or empty for a temporary identity.
    pub fn identity_label(&self) -> String {
        self.inner.borrow().saved.as_ref().map(|s| String::from_utf8_lossy(&s.label).into_owned()).unwrap_or_default()
    }

    /// Encrypts the current identity into a key file. `pass` (UTF-8) is wiped on return.
    /// Returns the file bytes, or an empty array on error (ERROR event emitted).
    pub fn save_identity(&self, label: &str, pass: &mut [u8]) -> Vec<u8> {
        let res = (|| {
            let (key, salt) = keyfile::new_key(pass)?;
            let mut g = self.inner.borrow_mut();
            let blob = keyfile::seal(&key, &salt, label.as_bytes(), g.id.seed(), g.prefs.settings.nick(), &[])?;
            g.saved = Some(Saved { key, salt, label: label.as_bytes().to_vec(), contacts: Contacts::new(), others: Zeroizing::new(Vec::new()) });
            Ok(blob)
        })();
        pass.fill(0);
        res.unwrap_or_else(|e| {
            emit_err(ev::ERROR, e);
            Vec::new()
        })
    }

    /// Re-encrypts the saved identity with the key kept in memory (fresh nonce).
    pub fn resave_identity(&self) -> Vec<u8> {
        let g = self.inner.borrow();
        let Some(s) = g.saved.as_ref() else { return Vec::new() };
        let tlv = Zeroizing::new(s.contacts.to_tlv(&s.others));
        keyfile::seal(&s.key, &s.salt, &s.label, g.id.seed(), g.prefs.settings.nick(), &tlv).unwrap_or_default()
    }

    /// Signs in with a key file. Only while no chat is open. Returns 0 or an error code.
    pub fn load_identity(&self, blob: &[u8], pass: &mut [u8]) -> u32 {
        let res = (|| {
            if self.busy() {
                return Err(ErrorCode::NotPermitted);
            }
            let o = keyfile::open(blob, pass)?;
            let (contacts, others) = Contacts::from_tlv(&o.tlv).ok_or(ErrorCode::KeyfileInvalid)?;
            let mut g = self.inner.borrow_mut();
            g.id = Identity::from_seed(&o.seed);
            g.prefs.settings.set_nick(&o.nick);
            g.saved = Some(Saved { key: o.key, salt: o.salt, label: o.label, contacts, others: Zeroizing::new(others) });
            Ok(())
        })();
        pass.fill(0);
        #[cfg(feature = "tor")]
        let res = res.and_then(|()| tor::sync_identity(&self.inner));
        status(res)
    }

    /// Replaces the identity with a new temporary one (sign out). Only while no chat is open.
    pub fn new_temporary_identity(&self) -> u32 {
        if self.busy() {
            return ErrorCode::NotPermitted.code() as u32;
        }
        {
            let mut g = self.inner.borrow_mut();
            g.id = Identity::generate();
            g.saved = None;
            g.prefs.settings.set_nick(&[]);
        }
        #[cfg(feature = "tor")]
        return status(tor::sync_identity(&self.inner));
        #[cfg(not(feature = "tor"))]
        0
    }

    fn busy(&self) -> bool {
        self.inner.borrow().links.iter().any(|l| l.sess.state() != State::Closed)
    }

    /// Our nickname, sent to peers in HELLO and kept in the key file (§7.3).
    pub fn nick(&self) -> String {
        String::from_utf8_lossy(self.inner.borrow().prefs.settings.nick()).into_owned()
    }

    /// Sets our nickname (≤ 32 bytes); applies from the next chat. Re-save a saved identity after.
    pub fn set_nick(&self, nick: &str) -> u32 {
        let ok = self.inner.borrow_mut().prefs.settings.set_nick(nick.trim().as_bytes());
        status(if ok { Ok(()) } else { Err(ErrorCode::NotPermitted) })
    }

    // ---- contacts (§7.5, saved identities only) ----

    /// One line per contact: `peer_id_hex \t flags \t nickname \t handle`.
    pub fn contacts(&self) -> String {
        let g = self.inner.borrow();
        let Some(s) = g.saved.as_ref() else { return String::new() };
        let mut out = String::new();
        for c in s.contacts.list() {
            out.push_str(&format!("{}\t{}\t{}\t{}\n", hex(&c.peer_id.0), c.flags, String::from_utf8_lossy(c.nick()), String::from_utf8_lossy(&c.peer_id.handle())));
        }
        out
    }

    /// Saves the peer of the 1:1 chat as a contact (verified if the SAS was confirmed).
    pub fn save_contact(&self, nick: &str) -> u32 {
        let mut g = self.inner.borrow_mut();
        let Inner { links, saved, room, .. } = &mut *g;
        let r = match (links.first(), saved.as_mut()) {
            (Some(l), Some(sv)) if room.is_none() && l.sess.ever_connected() && !l.sess.transfer() => {
                let c = &mut sv.contacts;
                let r = c.save(l.sess.remote(), Some(l.sess.peer_sign_pk()), l.sess.sas_confirmed(), nick.trim().as_bytes(), (now_ms() / 1000) as u32);
                // A Tor chat also gives the contact's onion: "Connect" without a code (§28.7).
                r.and_then(|()| if l.sess.tor() { c.set_onion(&l.sess.remote(), l.sess.peer_onion()) } else { Ok(()) }).map_err(contact_err)
            }
            _ => Err(ErrorCode::NotPermitted),
        };
        drop(g);
        status(r)
    }

    pub fn rename_contact(&self, peer_hex: &str, nick: &str) -> u32 {
        let mut g = self.inner.borrow_mut();
        let r = match (peer_from_hex(peer_hex), g.saved.as_mut()) {
            (Some(p), Some(sv)) => match sv.contacts.get(&p).copied() {
                Some(c) => sv.contacts.save(p, None, false, nick.trim().as_bytes(), c.added_at).map_err(contact_err),
                None => Err(ErrorCode::NotAContact),
            },
            _ => Err(ErrorCode::NotAContact),
        };
        drop(g);
        status(r)
    }

    pub fn remove_contact(&self, peer_hex: &str) -> u32 {
        let mut g = self.inner.borrow_mut();
        let r = match (peer_from_hex(peer_hex), g.saved.as_mut()) {
            (Some(p), Some(sv)) => sv.contacts.remove(&p).map_err(contact_err),
            _ => Err(ErrorCode::NotAContact),
        };
        drop(g);
        status(r)
    }

    /// The 1:1 peer as a contact: `flags \t nickname`, or empty if not a contact.
    pub fn peer_contact(&self) -> String {
        let g = self.inner.borrow();
        let Some(i) = g.primary().filter(|_| g.room.is_none()) else { return String::new() };
        g.saved
            .as_ref()
            .and_then(|sv| sv.contacts.get(&g.links[i].sess.remote()))
            .map(|c| format!("{}\t{}", c.flags, String::from_utf8_lossy(c.nick())))
            .unwrap_or_default()
    }

    /// If the 1:1 peer calls itself by a verified contact's nickname with a different key, that
    /// contact's nickname ("This is not the Alice you verified", §7.5); otherwise empty.
    pub fn impersonates(&self, nick: &str) -> String {
        let g = self.inner.borrow();
        let Some(i) = g.primary() else { return String::new() };
        g.saved
            .as_ref()
            .and_then(|sv| sv.contacts.impersonated(nick.trim().as_bytes(), &g.links[i].sess.remote()))
            .map(|c| String::from_utf8_lossy(c.nick()).into_owned())
            .unwrap_or_default()
    }

    /// The user confirmed the SAS of the 1:1 chat or of the link to the room owner (§10.4):
    /// marks a contact verified; on an identity transfer (§7.6) it releases the key file
    /// (sender) or tells the sender (receiver).
    pub fn confirm_sas(&self) -> u32 {
        let blob = {
            let g = self.inner.borrow();
            let sending = g.primary().is_some_and(|i| g.links[i].sess.transfer()) && g.saved.is_some();
            if sending { Some(Zeroizing::new(self.resave_identity())) } else { None }
        };
        let mut g = self.inner.borrow_mut();
        let Some(i) = g.primary() else { return status(Err(ErrorCode::PeerOffline)) };
        let remote = g.links[i].sess.remote();
        if let Some(sv) = g.saved.as_mut() {
            let _ = sv.contacts.set_verified(&remote);
        }
        let now = now_ms();
        let r = on_link!(g, i, |s, k, _t| s.confirm_sas(now, blob.as_deref().map(|b| b.as_slice()), &mut k));
        drop(g);
        status(r)
    }

    // ---- preferences ----

    /// `privacy`: 0 LAN-only, 1 default, 2 max connectivity. Applies to the next code.
    pub fn set_prefs(&self, privacy: u8, drop_ipv6: bool, read_receipts: bool, typing: bool) {
        let mut g = self.inner.borrow_mut();
        let settings = Settings { read_receipts, typing, ..g.prefs.settings };
        g.prefs = Prefs { privacy: Privacy::from_u8(privacy), drop_ipv6, settings };
    }

    // ---- buffers shared with JS ----

    /// Where JS writes outgoing UTF-8 (`TextEncoder.encodeInto`), `MAX_TEXT` bytes.
    pub fn text_ptr(&self) -> u32 {
        self.inner.borrow().text.as_ptr() as u32
    }

    pub fn text_cap(&self) -> u32 {
        MAX_TEXT as u32
    }

    /// Event side-channel block (layout in [`meta`]).
    pub fn meta_ptr(&self) -> u32 {
        self.inner.borrow().meta.as_ptr() as u32
    }

    /// Camera frame buffer for [`scan_rgba`]: at least `len` bytes (allocated once, grown only
    /// if a larger camera frame appears).
    pub fn scan_buf(&self, len: u32) -> u32 {
        let mut g = self.inner.borrow_mut();
        if g.scan.len() < len as usize {
            g.scan.resize(len as usize, 0);
        }
        g.scan.as_ptr() as u32
    }

    /// Decodes a QR code from the RGBA frame in the scan buffer; empty string if none.
    pub fn scan(&self, width: u32, height: u32) -> String {
        let g = self.inner.borrow();
        let n = (width * height * 4) as usize;
        g.scan.get(..n).map(|px| scan_rgba(px, width as usize, height as usize)).unwrap_or_default()
    }

    // ---- rendezvous ----

    /// Alice: new 1:1 chat and invite. Emits CODE(1).
    pub fn create_invite(&self, ttl_s: u32) {
        self.invite(ttl_s, 0);
    }

    /// New device (§7.6): an invite asking another device for its identity. Emits CODE(1).
    pub fn create_transfer_invite(&self, ttl_s: u32) {
        self.invite(ttl_s, flags::TRANSFER);
    }

    /// Tor build: a TOR_INVITE to our onion service (§28.4); identity transfer is direct-only.
    #[cfg(feature = "tor")]
    fn invite(&self, ttl_s: u32, extra: u8) {
        status(if extra == 0 { tor::invite(&self.inner, ttl_s) } else { Err(ErrorCode::NotPermitted) });
    }

    #[cfg(not(feature = "tor"))]
    fn invite(&self, ttl_s: u32, extra: u8) {
        let now_s = (now_ms() / 1000) as u32;
        let (inv, room) = ids();
        let (id, privacy) = {
            let mut g = self.inner.borrow_mut();
            g.reset();
            let p = g.prefs;
            let expires = now_s + ttl_s.clamp(60, 1800);
            let s = if extra & flags::TRANSFER != 0 {
                Session::transfer_receiver(&g.id, inv, room, expires, p.privacy, p.drop_ipv6)
            } else {
                Session::offerer(&g.id, inv, room, expires, p.privacy, p.drop_ipv6, p.settings)
            };
            (g.add_link(1, s, false), p.privacy)
        };
        rtc::start(self.inner.clone(), id, privacy, rtc::Step::Offer);
    }

    /// `kind | flags << 8` of a code without applying it (0 if it is not a valid code), so the UI
    /// can ask before answering an identity-transfer or room invite.
    pub fn code_info(&self, text: &str) -> u32 {
        decode_text(text).and_then(|(bin, n)| Code::decode(&bin[..n]).ok()).map_or(0, |c| c.kind as u32 | (c.flags as u32) << 8)
    }

    /// Whether the 1:1 chat is an identity transfer (§7.6).
    pub fn is_transfer(&self) -> bool {
        let g = self.inner.borrow();
        g.primary().is_some_and(|i| g.links[i].sess.transfer())
    }

    /// T3: a reconnect code for the 1:1 chat or for the link to the room owner (§13). Emits CODE(3).
    pub fn create_resume(&self, ttl_s: u32) -> u32 {
        // Tor mode has no reconnect codes: the dialer redials the onion (§28.5).
        if cfg!(feature = "tor") {
            return status(Err(ErrorCode::NotPermitted));
        }
        let now_s = (now_ms() / 1000) as u32;
        let (inv, _) = ids();
        let res = {
            let mut g = self.inner.borrow_mut();
            g.primary().ok_or(ErrorCode::NotPermitted).and_then(|i| {
                g.links[i].sess.resume_invite(inv, now_s + ttl_s.clamp(60, 1800))?;
                let id = g.new_path(i);
                Ok((id, g.links[i].sess.privacy()))
            })
        };
        match res {
            Ok((id, privacy)) => {
                rtc::start(self.inner.clone(), id, privacy, rtc::Step::Offer);
                0
            }
            Err(e) => status(Err(e)),
        }
    }

    /// Whether this tab can use the code (for the tab hand-off, §8.7): an answer to one of our
    /// open invites, or a reconnect code for one of our links. Never has side effects.
    pub fn code_fits(&self, text: &str) -> bool {
        let Some((bin, n)) = decode_text(text) else { return false };
        let Ok(c) = Code::decode(&bin[..n]) else { return false };
        let g = self.inner.borrow();
        g.links.iter().any(|l| match c.kind {
            Kind::Answer | Kind::ResumeAnswer => l.sess.state() == State::AwaitingAnswer && l.sess.invite_id() == c.invite_id,
            Kind::ResumeInvite => l.sess.ever_connected() && l.sess.state() != State::Closed && l.sess.room_id() == c.room_id && l.sess.remote().0 == c.static_pk,
            Kind::Invite | Kind::TorInvite => false,
        })
    }

    /// Applies a code: a full link, `#i=` / `#a=` / `#r=` / `#q=`, or bare base64url.
    /// `scanned` = it came from the in-app camera (SAS policy, §10.4).
    /// Returns 0 or an error code (also emitted as ERROR).
    pub fn apply_code(&self, text: &str, scanned: bool) -> u32 {
        status(self.apply_code_inner(text, scanned))
    }

    fn apply_code_inner(&self, text: &str, scanned: bool) -> Result<(), ErrorCode> {
        let (bin, n) = decode_text(text).ok_or(ErrorCode::InvalidInvite)?;
        let code = &bin[..n];
        let now_s = (now_ms() / 1000) as u32;
        let c = Code::decode(code)?;
        // Modes never mix (§28.2): the Tor build takes only Tor invites, the direct build none.
        if (c.kind == Kind::TorInvite) != cfg!(feature = "tor") {
            return Err(if cfg!(feature = "tor") { ErrorCode::NotPermitted } else { ErrorCode::TorUnavailable });
        }
        match c.kind {
            Kind::Invite => {
                let group = c.flags & flags::GROUP != 0;
                let (id, privacy) = {
                    let mut g = self.inner.borrow_mut();
                    // Only a saved identity can be moved to another device (§7.6).
                    if c.flags & flags::TRANSFER != 0 && g.saved.is_none() {
                        return Err(ErrorCode::NotPermitted);
                    }
                    let mut p = g.prefs;
                    if group {
                        // Rooms: read receipts and typing are off (§11.7).
                        p.settings.read_receipts = false;
                        p.settings.typing = false;
                    }
                    let s = Session::answerer(&g.id, code, now_s, p.privacy, p.drop_ipv6, p.settings, scanned)?;
                    g.reset();
                    if group {
                        g.room = Some(room::Room::joining(c.flags & flags::OBSERVER != 0));
                    }
                    let privacy = s.privacy();
                    let member = if group { OWNER_IDX } else { 0 };
                    (g.add_link(member, s, false), privacy)
                };
                rtc::start(self.inner.clone(), id, privacy, rtc::Step::Answer);
            }
            Kind::ResumeInvite => {
                let (id, privacy) = {
                    let mut g = self.inner.borrow_mut();
                    let i = g.links.iter().position(|l| l.sess.remote().0 == c.static_pk && l.sess.room_id() == c.room_id).ok_or(ErrorCode::InvalidRoom)?;
                    let Inner { id, links, .. } = &mut *g;
                    links[i].sess.accept_resume(id, code, now_s)?;
                    let privacy = links[i].sess.privacy();
                    (g.new_path(i), privacy)
                };
                rtc::start(self.inner.clone(), id, privacy, rtc::Step::Answer);
            }
            Kind::TorInvite =>
            {
                #[cfg(feature = "tor")]
                tor::join(&self.inner, code, now_s, scanned)?
            }
            Kind::Answer | Kind::ResumeAnswer => {
                let id = {
                    let mut g = self.inner.borrow_mut();
                    let i = g.links.iter().position(|l| l.sess.invite_id() == c.invite_id && l.sess.state() == State::AwaitingAnswer).ok_or(ErrorCode::AnswerMismatch)?;
                    let Inner { id, links, .. } = &mut *g;
                    links[i].sess.apply_answer(id, code, now_s, scanned)?;
                    links[i].id
                };
                rtc::apply_answer(self.inner.clone(), id);
            }
        }
        Ok(())
    }

    /// Our public addresses as the peers see them (srflx candidates of our codes), one per
    /// line as `v4 1.2.3.4` / `v6 2001:db8::1` (§29.2 "what your peer sees").
    pub fn exposure(&self) -> String {
        let g = self.inner.borrow();
        let mut out = [0u8; 1024];
        let mut b = Buf::new(&mut out);
        for l in &g.links {
            let Ok(c) = Code::decode(l.sess.local_code()) else { continue };
            for cand in c.ice.candidates() {
                if cand.tag.is_srflx() {
                    let _ = b.put(if cand.tag.is_v6() { b"v6 " } else { b"v4 " });
                    let _ = cand.render_addr(&mut b);
                    let _ = b.put(b"\n");
                }
            }
        }
        String::from_utf8_lossy(b.as_slice()).into_owned()
    }

    // ---- rooms (§14) ----

    /// Creates a room owned by us (everything else is closed). Invite members next.
    pub fn create_room(&self) {
        {
            let mut g = self.inner.borrow_mut();
            g.reset();
            let (_, room_id) = ids();
            let r = room::Room::owned(room_id, &g.id);
            g.room = Some(r);
        }
        room::emit_state(&self.inner.borrow());
    }

    /// Owner: an invite for one more member (or read-only observer). Emits CODE(1).
    pub fn room_invite(&self, observer: bool, ttl_s: u32) -> u32 {
        match room::invite(&self.inner, observer, ttl_s) {
            Ok((id, privacy)) => {
                room::start_offer(&self.inner, id, privacy);
                0
            }
            Err(e) => status(Err(e)),
        }
    }

    /// Member: the user accepted that every member will see its IP address (§29.2):
    /// start the links to the other members.
    pub fn room_connect(&self) {
        room::confirm(&self.inner);
    }

    /// Owner: removes a member (it gets a state without itself, then GOODBYE).
    pub fn room_remove(&self, member: u8) -> u32 {
        status(room::remove(&self.inner, member))
    }

    /// One line per member: `idx \t role(0 owner,1 member,2 observer) \t handle \t link \t nick`,
    /// where link is `me`, `connected`, `connecting`, `suspended` or `none`.
    pub fn room_members(&self) -> String {
        room::members(&self.inner.borrow())
    }

    /// `my_idx \t my_role \t owner(0/1) \t version \t confirmed(0/1)`, or empty outside a room.
    pub fn room_info(&self) -> String {
        room::info(&self.inner.borrow())
    }

    pub fn in_room(&self) -> bool {
        self.inner.borrow().room.is_some()
    }

    /// Our member index (1:1: our PeerIdx), for message identities `(sender, seq)`.
    pub fn my_idx(&self) -> u8 {
        let g = self.inner.borrow();
        match (&g.room, g.links.first()) {
            (Some(r), _) => r.me,
            (None, Some(l)) => l.sess.me_idx(),
            (None, None) => 0,
        }
    }

    // ---- chat ----

    /// Sends `len` bytes previously written at `text_ptr()`. `reply_seq` = 0 for no reply,
    /// otherwise the quoted message `(reply_sender, reply_seq)`. Returns chat_seq or -error code.
    pub fn send(&self, len: u32, reply_sender: u8, reply_seq: f64) -> f64 {
        let reply = (reply_seq > 0.0).then_some(MsgRef { sender: reply_sender, seq: reply_seq as u64 });
        let len = (len as usize).min(MAX_TEXT);
        result_f64(room::fan_out(&self.inner, |s, at, now, text, mut k| s.send_chat(now, at, &text[..len], reply, &mut k)).map(|q| q as f64))
    }

    /// Edits our message `seq` with `len` bytes at `text_ptr()`. Returns 0 or -error code.
    pub fn edit(&self, seq: f64, len: u32) -> f64 {
        let len = (len as usize).min(MAX_TEXT);
        result_f64(room::each(&self.inner, |s, now, text, mut k| s.edit(now, seq as u64, &text[..len], &mut k)).map(|()| 0.0))
    }

    /// Deletes a message: ours for everyone; someone else's for me only, or for everyone by the
    /// room owner (moderation). Returns 0 or -error code.
    pub fn delete(&self, sender: u8, seq: f64) -> f64 {
        let msg = MsgRef { sender, seq: seq as u64 };
        result_f64(room::each(&self.inner, |s, now, _t, mut k| s.delete(now, msg, &mut k)).map(|()| 0.0))
    }

    /// Sets the chat's self-destruct timer (1:1: either person; room: the owner).
    /// Returns the notice's chat_seq or -error code.
    pub fn set_ttl(&self, ttl_s: u32) -> f64 {
        let r = room::fan_out(&self.inner, |s, at, now, _t, mut k| s.set_ttl(now, at, ttl_s, &mut k));
        if r.is_ok() {
            room::set_ttl(&mut self.inner.borrow_mut(), ttl_s);
        }
        result_f64(r.map(|q| q as f64))
    }

    /// Reacts to `(sender, seq)` with the emoji written at `text_ptr()` (`len` = 0 removes our
    /// reaction). Returns 0 or -error code.
    pub fn react(&self, sender: u8, seq: f64, len: u32) -> f64 {
        let len = (len as usize).min(MAX_TEXT);
        let msg = MsgRef { sender, seq: seq as u64 };
        result_f64(room::each(&self.inner, |s, now, text, mut k| s.react(now, msg, &text[..len], &mut k)).map(|()| 0.0))
    }

    /// The user has seen the 1:1 peer's messages up to `seq`.
    pub fn mark_read(&self, seq: f64) {
        let mut g = self.inner.borrow_mut();
        if let Some(i) = g.primary().filter(|_| g.room.is_none()) {
            let now = now_ms();
            on_link!(g, i, |s, k, _t| s.mark_read(now, seq as u64, &mut k));
        }
    }

    pub fn typing(&self, active: bool) {
        let mut g = self.inner.borrow_mut();
        if let Some(i) = g.primary().filter(|_| g.room.is_none()) {
            let now = now_ms();
            on_link!(g, i, |s, k, _t| s.typing(now, active, &mut k));
        }
    }

    /// In-band ICE restart (§13 T1) of every connected link: diagnostics button, or the network
    /// changed (`online`, `navigator.connection` change).
    pub fn restart_ice(&self) {
        let ids: Vec<u32> = self.inner.borrow().links.iter().map(|l| l.id).collect();
        for id in ids {
            rtc::restart(self.inner.clone(), id);
        }
    }

    /// Diagnostics (§18) of the 1:1 chat or of the link to the room owner (else the first link).
    pub fn diag(&self) -> String {
        let g = self.inner.borrow();
        let Some(i) = g.primary().or((!g.links.is_empty()).then_some(0)) else { return String::new() };
        let l = &g.links[i];
        let d = l.sess.diag(now_ms());
        let secs = d.rekey_in_ms / 1000;
        let mut out = format!(
            "app RTT {} · Noise KK, epoch {} · rekey in {}:{:02} · pending {}",
            if d.app_rtt_ms > 0 { format!("{} ms", d.app_rtt_ms) } else { "n/a".into() },
            d.rekeys,
            secs / 60,
            secs % 60,
            d.pending
        );
        if let Some(r) = l.rtc.as_ref() {
            out.push('\n');
            out.push_str(&r.describe());
        }
        out
    }

    /// Timer (JS calls it every second). `hidden` = `document.hidden`, carried in PING.
    pub fn tick(&self, hidden: bool) {
        {
            let mut g = self.inner.borrow_mut();
            let now = now_ms();
            for i in 0..g.links.len() {
                on_link!(g, i, |s, k, _t| s.tick(now, hidden, &mut k));
            }
        }
        rtc::check_paths(&self.inner);
        room::tick(&self.inner);
    }

    /// Drops network paths without leaving, as a network change would. Diagnostics ("Simulate
    /// network loss"): the 1:1 chat's path (both sides then go through T3), or in a room the
    /// member's direct links to the other members (they come back by T2 through the owner).
    pub fn drop_path(&self) {
        let mut g = self.inner.borrow_mut();
        let member = g.room.as_ref().is_some_and(|r| !r.owner);
        let now = now_ms();
        for i in 0..g.links.len() {
            if (member && g.links[i].via_owner) || (!member && Some(i) == g.primary()) {
                let _lid = g.new_path(i);
                on_link!(g, i, |s, k, _t| s.path_lost(now, ErrorCode::IceFailed, &mut k));
                // Tor: the side that dialled dials again (§28.5); the other side waits.
                #[cfg(feature = "tor")]
                if g.links[i].sess.role() == ephem_core::Role::Answerer {
                    tor::dial(&self.inner, _lid);
                }
            }
        }
    }

    /// Leaves the chat or room (a member first tells the owner; GOODBYE on every link), wipes
    /// session keys and messages.
    pub fn close(&self) {
        room::leave(&self.inner);
        self.inner.borrow_mut().reset();
    }

    /// 1:1 chat or link to the room owner: 0 none, 1 gathering, 2 awaiting answer,
    /// 3 connecting, 4 connected, 5 closed, 6 suspended. A room owner: 4.
    pub fn state(&self) -> u8 {
        let g = self.inner.borrow();
        let Some(i) = g.primary() else { return if g.room.is_some() { 4 } else { 0 } };
        match g.links[i].sess.state() {
            State::Gathering => 1,
            State::AwaitingAnswer => 2,
            State::Connecting => 3,
            State::Connected => 4,
            State::Closed => 5,
            State::Suspended => 6,
        }
    }

    /// The chat's self-destruct timer in seconds (0 = off).
    pub fn chat_ttl(&self) -> u32 {
        room::ttl(&self.inner.borrow())
    }

    /// Our role: 0 owner, 1 member, 2 observer (1:1: member).
    pub fn my_role(&self) -> u8 {
        self.inner.borrow().room.as_ref().map_or(RoomRole::Member, |r| r.role) as u8
    }
}

impl Default for App {
    fn default() -> Self {
        Self::new()
    }
}

/// Accepts `https://…/#i=CODE`, `#a=`, `#r=`, `#q=`, `i=CODE`, or `CODE`.
fn extract_payload(s: &str) -> &str {
    let s = s.trim();
    let s = s.rsplit_once('#').map_or(s, |(_, f)| f);
    for k in ["i=", "a=", "r=", "q=", "t="] {
        if let Some(rest) = s.strip_prefix(k) {
            return rest;
        }
    }
    s
}

fn decode_text(text: &str) -> Option<([u8; MAX_CODE_LEN], usize)> {
    let mut bin = [0u8; MAX_CODE_LEN];
    let n = b64url::decode(extract_payload(text).as_bytes(), &mut bin).ok()?;
    Some((bin, n))
}

#[cfg(test)]
mod tests {
    #[test]
    fn payload_forms() {
        assert_eq!(super::extract_payload("https://x.io/ephem/app/#i=AbC"), "AbC");
        assert_eq!(super::extract_payload("#a=Z-_"), "Z-_");
        assert_eq!(super::extract_payload(" #r=Q \n"), "Q");
        assert_eq!(super::extract_payload("Qwe"), "Qwe");
    }
}
