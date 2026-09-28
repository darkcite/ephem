//! Ephem browser adapter (docs/P2P-CHAT.md §6.1): the only crate that touches the browser.
//!
//! Rust owns all state; JS renders the DOM and forwards user input. Events go to JS through one
//! imported function, `ephemEvent(kind, num, ptr, len)`, where `ptr/len` points into wasm linear
//! memory (no copy on our side; JS decodes text with `TextDecoder` over a memory view, §11.6).
//! Extra fixed-layout fields of an event are written to the `meta` block (see [`meta`]).
//!
//! **Re-entrancy rule for JS:** an `ephemEvent` handler must not call back into `App`
//! synchronously (it may update the DOM; anything else goes through `queueMicrotask`).

mod qr;
mod rtc;

use core::cell::RefCell;
use ephem_core::{Event, MsgRef, Privacy, Session, Settings, State};
use ephem_crypto::{Identity, keyfile};
use ephem_proto::ErrorCode;
use ephem_proto::b64url;
use ephem_proto::buf::Buf;
use ephem_proto::code::{Code, Kind, MAX_CODE_LEN};
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
    /// num = chat_seq; text = UTF-8; meta: ttl_s u32 @0, has_reply u8 @4, reply_mine u8 @5, reply_seq f64 @8.
    pub const CHAT: u32 = 4;
    /// num = cumulative delivered chat_seq.
    pub const DELIVERED: u32 = 5;
    pub const DEGRADED: u32 = 6;
    pub const ALIVE: u32 = 7;
    /// num = 1 hidden / 0 visible.
    pub const PEER_HIDDEN: u32 = 8;
    /// Chat ended. num = error code; text = `E_...` name.
    pub const CLOSED: u32 = 9;
    /// Non-fatal failure of a user action. num = error code; text = `E_...` name.
    pub const ERROR: u32 = 10;
    /// Progress. num: 1 = gathering, 2 = connecting, 3 = channel open.
    pub const PROGRESS: u32 = 11;
    /// Diagnostics of the selected path (getStats); text = human readable. num bit0 = relay seen.
    pub const PATH: u32 = 12;
    /// The peer set the self-destruct timer. num = seconds (0 = off).
    pub const SETTING: u32 = 13;
    /// The peer edited its message. num = seq; text = new text.
    pub const EDITED: u32 = 14;
    /// Deleted for everyone. num = +seq (mine) / -seq (peer's).
    pub const DELETED: u32 = 15;
    /// Self-destruct fired. num = +seq (mine) / -seq (peer's).
    pub const EXPIRED: u32 = 16;
    /// Cumulative read chat_seq.
    pub const READ: u32 = 17;
    /// num = 1 typing / 0 stopped.
    pub const TYPING: u32 = 18;
    /// Path lost; the chat waits for a reconnect code.
    pub const SUSPENDED: u32 = 19;
}

/// Byte offsets inside the meta block.
pub mod meta {
    pub const TTL: usize = 0;
    pub const HAS_REPLY: usize = 4;
    pub const REPLY_MINE: usize = 5;
    pub const REPLY_SEQ: usize = 8;
    pub const RESUMED: usize = 0;
    pub const LEN: usize = 16;
}

#[inline(always)]
fn emit(kind: u32, num: f64, bytes: &[u8]) {
    js_event(kind, num, bytes.as_ptr() as u32, bytes.len() as u32);
}

#[inline]
fn emit_err(kind: u32, e: ErrorCode) {
    emit(kind, e.code() as f64, e.name().as_bytes());
}

#[inline(always)]
fn now_ms() -> u64 {
    js_sys::Date::now() as u64
}

#[inline(always)]
fn signed(m: MsgRef) -> f64 {
    if m.mine { m.seq as f64 } else { -(m.seq as f64) }
}

/// User preferences applied to the next chat.
#[derive(Copy, Clone)]
struct Prefs {
    privacy: Privacy,
    drop_ipv6: bool,
    settings: Settings,
}

/// A saved identity's file key, kept so it can be re-saved without asking again (§7.3).
struct Saved {
    key: Zeroizing<[u8; 32]>,
    salt: [u8; 16],
    label: Vec<u8>,
    nick: Vec<u8>,
    tlv: Zeroizing<Vec<u8>>,
}

/// Everything the tab owns. Buffers are allocated once at start and reused for every chat.
pub(crate) struct Inner {
    id: Identity,
    saved: Option<Saved>,
    prefs: Prefs,
    sess: Option<Box<Session>>,
    rtc: Option<rtc::Rtc>,
    /// Bumped on every new path so late callbacks of an old RTCPeerConnection are ignored.
    generation: u32,
    rx: Box<[u8; MAX_FRAME]>,
    text: Box<[u8; MAX_TEXT]>,
    /// Encoded codes and small event payloads.
    scratch: Box<[u8; 1024]>,
    meta: Box<[u8; meta::LEN]>,
    /// RGBA camera frame for the QR scanner; sized on first use (setup path).
    scan: Vec<u8>,
}

pub(crate) type Shared = Rc<RefCell<Inner>>;

impl Inner {
    /// Ends the current chat (GOODBYE if connected) and its path.
    fn reset(&mut self) {
        let Inner { sess, rtc, meta, .. } = self;
        if let (Some(s), Some(r)) = (sess.as_mut(), rtc.as_ref()) {
            s.close(now_ms(), &mut sink(r, meta));
        }
        self.new_path();
        self.sess = None;
    }

    /// Drops the current RTCPeerConnection; the chat (if any) stays.
    fn new_path(&mut self) {
        if let Some(r) = self.rtc.take() {
            r.close();
        }
        self.generation = self.generation.wrapping_add(1);
    }
}

/// Maps core events to the DataChannel and to JS.
pub(crate) fn sink<'a>(rtc: &'a rtc::Rtc, meta: &'a mut [u8; meta::LEN]) -> impl FnMut(Event<'_>) + 'a {
    move |e| on_event(Some(rtc), meta, e)
}

fn on_event(rtc: Option<&rtc::Rtc>, meta: &mut [u8; meta::LEN], e: Event<'_>) {
    match e {
        Event::Send(frame) => {
            if let Some(r) = rtc {
                r.send(frame);
            }
        }
        Event::Connected { sas, peer, resumed } => {
            let mut b = [0u8; 15];
            b[..4].copy_from_slice(&sas.emoji);
            b[4..].copy_from_slice(&peer.handle());
            meta[meta::RESUMED] = resumed as u8;
            emit(ev::CONNECTED, sas.digits as f64, &b);
        }
        Event::Hello { nick, sas_optional } => emit(ev::HELLO, sas_optional as u8 as f64, nick),
        Event::Chat { seq, text, ttl_s, reply } => {
            meta[meta::TTL..meta::TTL + 4].copy_from_slice(&ttl_s.to_le_bytes());
            meta[meta::HAS_REPLY] = reply.is_some() as u8;
            let r = reply.unwrap_or(MsgRef { mine: false, seq: 0 });
            meta[meta::REPLY_MINE] = r.mine as u8;
            meta[meta::REPLY_SEQ..meta::REPLY_SEQ + 8].copy_from_slice(&(r.seq as f64).to_le_bytes());
            emit(ev::CHAT, seq as f64, text);
        }
        Event::Setting { ttl_s } => emit(ev::SETTING, ttl_s as f64, &[]),
        Event::Edited { seq, text } => emit(ev::EDITED, seq as f64, text),
        Event::Deleted(m) => emit(ev::DELETED, signed(m), &[]),
        Event::Expired(m) => emit(ev::EXPIRED, signed(m), &[]),
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

/// Runs a session action with the right sink (with or without a live path), splitting the
/// borrows of `Inner` so the text buffer can be read while the session is mutated.
/// Evaluates to `None` when there is no chat.
macro_rules! with_session {
    ($g:expr, |$s:ident, $k:ident, $text:ident| $body:expr) => {{
        let Inner { sess, rtc, meta, text: $text, .. } = &mut *$g;
        match sess.as_mut() {
            Some($s) => {
                let r = rtc.as_ref();
                let mut $k = |e: Event<'_>| on_event(r, meta, e);
                Some($body)
            }
            None => None,
        }
    }};
}

fn ids() -> ([u8; 16], [u8; 16]) {
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
                sess: None,
                rtc: None,
                generation: 0,
                rx: Box::new([0; MAX_FRAME]),
                text: Box::new([0; MAX_TEXT]),
                scratch: Box::new([0; 1024]),
                meta: Box::new([0; meta::LEN]),
                scan: Vec::new(),
            })),
        }
    }

    // ---- identity (§7.2, §7.3) ----

    /// Display handle (`anon_xxxxxx`), not authentication.
    pub fn handle(&self) -> String {
        let h = self.inner.borrow().id.peer_id().handle();
        String::from_utf8_lossy(&h).into_owned()
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
            let blob = keyfile::seal(&key, &salt, label.as_bytes(), g.id.seed(), &[], &[])?;
            g.saved = Some(Saved { key, salt, label: label.as_bytes().to_vec(), nick: Vec::new(), tlv: Zeroizing::new(Vec::new()) });
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
        keyfile::seal(&s.key, &s.salt, &s.label, g.id.seed(), &s.nick, &s.tlv).unwrap_or_default()
    }

    /// Signs in with a key file. Only while no chat is open. Returns 0 or an error code.
    pub fn load_identity(&self, blob: &[u8], pass: &mut [u8]) -> u32 {
        let res = (|| {
            if self.inner.borrow().sess.as_ref().is_some_and(|s| s.state() != State::Closed) {
                return Err(ErrorCode::NotPermitted);
            }
            let o = keyfile::open(blob, pass)?;
            let mut g = self.inner.borrow_mut();
            g.id = Identity::from_seed(&o.seed);
            g.saved = Some(Saved { key: o.key, salt: o.salt, label: o.label, nick: o.nick, tlv: o.tlv });
            Ok(())
        })();
        pass.fill(0);
        match res {
            Ok(()) => 0,
            Err(e) => {
                emit_err(ev::ERROR, e);
                e.code() as u32
            }
        }
    }

    /// Replaces the identity with a new temporary one (sign out). Only while no chat is open.
    pub fn new_temporary_identity(&self) -> u32 {
        let mut g = self.inner.borrow_mut();
        if g.sess.as_ref().is_some_and(|s| s.state() != State::Closed) {
            return ErrorCode::NotPermitted.code() as u32;
        }
        g.id = Identity::generate();
        g.saved = None;
        0
    }

    // ---- preferences ----

    /// `privacy`: 0 LAN-only, 1 default, 2 max connectivity. Applies to the next code.
    pub fn set_prefs(&self, privacy: u8, drop_ipv6: bool, read_receipts: bool, typing: bool) {
        self.inner.borrow_mut().prefs = Prefs { privacy: Privacy::from_u8(privacy), drop_ipv6, settings: Settings { read_receipts, typing } };
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

    /// Alice: new chat and invite. Emits CODE(1).
    pub fn create_invite(&self, ttl_s: u32) {
        let now_s = (now_ms() / 1000) as u32;
        let (inv, room) = ids();
        let (generation, privacy) = {
            let mut g = self.inner.borrow_mut();
            g.reset();
            let p = g.prefs;
            let s = Session::offerer(&g.id, inv, room, now_s + ttl_s.clamp(60, 1800), p.privacy, p.drop_ipv6, p.settings);
            g.sess = Some(Box::new(s));
            (g.generation, p.privacy)
        };
        rtc::start(self.inner.clone(), generation, privacy, rtc::Step::Offer);
    }

    /// T3: a reconnect code for the current chat (§13). Emits CODE(3).
    pub fn create_resume(&self, ttl_s: u32) -> u32 {
        let now_s = (now_ms() / 1000) as u32;
        let (inv, _) = ids();
        let res = {
            let mut g = self.inner.borrow_mut();
            let r = g.sess.as_mut().ok_or(ErrorCode::NotPermitted).and_then(|s| s.resume_invite(inv, now_s + ttl_s.clamp(60, 1800)));
            if r.is_ok() {
                g.new_path();
            }
            r.map(|()| (g.generation, g.sess.as_ref().map_or(Privacy::Default, |s| s.privacy())))
        };
        match res {
            Ok((generation, privacy)) => {
                rtc::start(self.inner.clone(), generation, privacy, rtc::Step::Offer);
                0
            }
            Err(e) => {
                emit_err(ev::ERROR, e);
                e.code() as u32
            }
        }
    }

    /// Whether this tab can use the code (for the tab hand-off, §8.7): an answer to our open
    /// invite, or a reconnect code for our chat. Never has side effects.
    pub fn code_fits(&self, text: &str) -> bool {
        let Some((bin, n)) = decode_text(text) else { return false };
        let Ok(c) = Code::decode(&bin[..n]) else { return false };
        let g = self.inner.borrow();
        let Some(s) = g.sess.as_ref() else { return false };
        match c.kind {
            Kind::Answer | Kind::ResumeAnswer => s.state() == State::AwaitingAnswer && s.invite_id() == c.invite_id,
            Kind::ResumeInvite => s.ever_connected() && s.state() != State::Closed && s.room_id() == c.room_id,
            Kind::Invite => false,
        }
    }

    /// Applies a code: a full link, `#i=` / `#a=` / `#r=` / `#q=`, or bare base64url.
    /// `scanned` = it came from the in-app camera (SAS policy, §10.4).
    /// Returns 0 or an error code (also emitted as ERROR).
    pub fn apply_code(&self, text: &str, scanned: bool) -> u32 {
        match self.apply_code_inner(text, scanned) {
            Ok(()) => 0,
            Err(e) => {
                emit_err(ev::ERROR, e);
                e.code() as u32
            }
        }
    }

    fn apply_code_inner(&self, text: &str, scanned: bool) -> Result<(), ErrorCode> {
        let (bin, n) = decode_text(text).ok_or(ErrorCode::InvalidInvite)?;
        let code = &bin[..n];
        let now_s = (now_ms() / 1000) as u32;
        let kind = Code::decode(code)?.kind;
        match kind {
            Kind::Invite => {
                let (generation, privacy) = {
                    let mut g = self.inner.borrow_mut();
                    let p = g.prefs;
                    let s = Session::answerer(&g.id, code, now_s, p.privacy, p.drop_ipv6, p.settings, scanned)?;
                    g.reset();
                    let privacy = s.privacy();
                    g.sess = Some(Box::new(s));
                    (g.generation, privacy)
                };
                rtc::start(self.inner.clone(), generation, privacy, rtc::Step::Answer);
            }
            Kind::ResumeInvite => {
                let (generation, privacy) = {
                    let mut g = self.inner.borrow_mut();
                    let Inner { id, sess, .. } = &mut *g;
                    let s = sess.as_mut().ok_or(ErrorCode::InvalidRoom)?;
                    s.accept_resume(id, code, now_s)?;
                    let privacy = s.privacy();
                    g.new_path();
                    (g.generation, privacy)
                };
                rtc::start(self.inner.clone(), generation, privacy, rtc::Step::Answer);
            }
            Kind::Answer | Kind::ResumeAnswer => {
                let generation = {
                    let mut g = self.inner.borrow_mut();
                    let Inner { id, sess, generation, .. } = &mut *g;
                    sess.as_mut().ok_or(ErrorCode::AnswerMismatch)?.apply_answer(id, code, now_s, scanned)?;
                    *generation
                };
                rtc::apply_answer(self.inner.clone(), generation);
            }
        }
        Ok(())
    }

    /// Our public addresses as the peer sees them (srflx candidates of our last code), one per
    /// line as `v4 1.2.3.4` / `v6 2001:db8::1` (§29.2 "what your peer sees").
    pub fn exposure(&self) -> String {
        let g = self.inner.borrow();
        let Some(s) = g.sess.as_ref() else { return String::new() };
        let Ok(c) = Code::decode(s.local_code()) else { return String::new() };
        let mut out = [0u8; 512];
        let mut b = Buf::new(&mut out);
        for cand in c.ice.candidates() {
            if cand.tag.is_srflx() {
                let _ = b.put(if cand.tag.is_v6() { b"v6 " } else { b"v4 " });
                let _ = cand.render_addr(&mut b);
                let _ = b.put(b"\n");
            }
        }
        String::from_utf8_lossy(b.as_slice()).into_owned()
    }

    // ---- chat ----

    /// Sends `len` bytes previously written at `text_ptr()`. `reply_seq` = 0 for no reply,
    /// otherwise the quoted message (`reply_mine` = it is ours). Returns chat_seq or -error code.
    pub fn send(&self, len: u32, reply_mine: bool, reply_seq: f64) -> f64 {
        let mut g = self.inner.borrow_mut();
        let len = (len as usize).min(MAX_TEXT);
        let reply = (reply_seq > 0.0).then_some(MsgRef { mine: reply_mine, seq: reply_seq as u64 });
        let r = with_session!(g, |s, k, text| s.send_chat(now_ms(), &text[..len], reply, &mut k).map(|q| q as f64));
        result_f64(r)
    }

    /// Edits our message `seq` with `len` bytes at `text_ptr()`. Returns 0 or -error code.
    pub fn edit(&self, seq: f64, len: u32) -> f64 {
        let mut g = self.inner.borrow_mut();
        let len = (len as usize).min(MAX_TEXT);
        let r = with_session!(g, |s, k, text| s.edit(now_ms(), seq as u64, &text[..len], &mut k).map(|()| 0.0));
        result_f64(r)
    }

    /// Deletes a message: ours for everyone, the peer's for me only. Returns 0 or -error code.
    pub fn delete(&self, mine: bool, seq: f64) -> f64 {
        let mut g = self.inner.borrow_mut();
        let r = with_session!(g, |s, k, _t| s.delete(now_ms(), MsgRef { mine, seq: seq as u64 }, &mut k).map(|()| 0.0));
        result_f64(r)
    }

    /// Sets the chat's self-destruct timer. Returns the notice's chat_seq or -error code.
    pub fn set_ttl(&self, ttl_s: u32) -> f64 {
        let mut g = self.inner.borrow_mut();
        let r = with_session!(g, |s, k, _t| s.set_ttl(now_ms(), ttl_s, &mut k).map(|q| q as f64));
        result_f64(r)
    }

    /// The user has seen the peer's messages up to `seq`.
    pub fn mark_read(&self, seq: f64) {
        let mut g = self.inner.borrow_mut();
        with_session!(g, |s, k, _t| s.mark_read(now_ms(), seq as u64, &mut k));
    }

    pub fn typing(&self, active: bool) {
        let mut g = self.inner.borrow_mut();
        with_session!(g, |s, k, _t| s.typing(now_ms(), active, &mut k));
    }

    /// Timer (JS calls it every second). `hidden` = `document.hidden`, carried in PING.
    pub fn tick(&self, hidden: bool) {
        let check = {
            let mut g = self.inner.borrow_mut();
            with_session!(g, |s, k, _t| s.tick(now_ms(), hidden, &mut k));
            g.sess.as_ref().is_some_and(|s| s.state() == State::Connected) && g.rtc.is_some()
        };
        if check {
            rtc::check_path(self.inner.clone());
        }
    }

    /// Drops the network path without leaving the chat, as a network change would. Diagnostics
    /// ("Simulate network loss"): both sides then go through the reconnect flow (§13 T3).
    pub fn drop_path(&self) {
        let mut g = self.inner.borrow_mut();
        g.new_path();
        let Inner { sess, meta, .. } = &mut *g;
        if let Some(s) = sess.as_mut() {
            s.path_lost(now_ms(), ErrorCode::IceFailed, &mut |e| on_event(None, meta, e));
        }
    }

    /// Leaves the chat (GOODBYE), closes the connection, wipes session keys and messages.
    pub fn close(&self) {
        self.inner.borrow_mut().reset();
    }

    /// 0 none, 1 gathering, 2 awaiting answer, 3 connecting, 4 connected, 5 closed, 6 suspended.
    pub fn state(&self) -> u8 {
        match self.inner.borrow().sess.as_ref().map(|s| s.state()) {
            None => 0,
            Some(State::Gathering) => 1,
            Some(State::AwaitingAnswer) => 2,
            Some(State::Connecting) => 3,
            Some(State::Connected) => 4,
            Some(State::Closed) => 5,
            Some(State::Suspended) => 6,
        }
    }

    /// Whether the current chat has ever been connected (i.e. it can be resumed).
    pub fn chat_active(&self) -> bool {
        self.inner.borrow().sess.as_ref().is_some_and(|s| s.ever_connected() && s.state() != State::Closed)
    }

    /// The chat's self-destruct timer in seconds (0 = off).
    pub fn chat_ttl(&self) -> u32 {
        self.inner.borrow().sess.as_ref().map_or(0, |s| s.chat_ttl())
    }
}

impl Default for App {
    fn default() -> Self {
        Self::new()
    }
}

#[inline]
fn result_f64(r: Option<Result<f64, ErrorCode>>) -> f64 {
    match r {
        Some(Ok(v)) => v,
        Some(Err(e)) => -(e.code() as f64),
        None => -(ErrorCode::PeerOffline.code() as f64),
    }
}

/// Accepts `https://…/#i=CODE`, `#a=`, `#r=`, `#q=`, `i=CODE`, or `CODE`.
fn extract_payload(s: &str) -> &str {
    let s = s.trim();
    let s = s.rsplit_once('#').map_or(s, |(_, f)| f);
    for k in ["i=", "a=", "r=", "q="] {
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
