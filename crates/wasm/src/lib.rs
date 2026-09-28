//! Ephem browser adapter (docs/P2P-CHAT.md §6.1): the only crate that touches the browser.
//!
//! Rust owns all state; JS renders the DOM and forwards user input. Events go to JS through one
//! imported function, `ephemEvent(kind, num, ptr, len)`, where `ptr/len` points into wasm linear
//! memory (no copy on our side; JS decodes text with `TextDecoder` over a memory view, §11.6).
//!
//! **Re-entrancy rule for JS:** an `ephemEvent` handler must not call back into `App`
//! synchronously (it may update the DOM; anything else goes through `queueMicrotask`).

mod qr;
mod rtc;

use core::cell::RefCell;
use ephem_core::{Event, Privacy, Session, State};
use ephem_crypto::Identity;
use ephem_proto::ErrorCode;
use ephem_proto::b64url;
use ephem_proto::code::MAX_CODE_LEN;
use ephem_proto::frame::{MAX_FRAME, MAX_TEXT};
use std::rc::Rc;
use wasm_bindgen::prelude::*;

pub use qr::qr_svg_path;

#[wasm_bindgen]
extern "C" {
    #[wasm_bindgen(js_name = ephemEvent)]
    fn js_event(kind: u32, num: f64, ptr: u32, len: u32);
}

/// Event kinds for `ephemEvent` (mirrored in `app/app.js`).
pub mod ev {
    /// num = 1 invite / 2 answer; ptr/len = base64url code.
    pub const CODE: u32 = 1;
    /// num = SAS digits; ptr/len = 4 emoji indices ‖ 11-byte peer handle.
    pub const CONNECTED: u32 = 2;
    /// ptr/len = nickname (may be empty).
    pub const HELLO: u32 = 3;
    /// num = chat_seq; ptr/len = UTF-8 text.
    pub const CHAT: u32 = 4;
    /// num = cumulative delivered chat_seq.
    pub const DELIVERED: u32 = 5;
    pub const DEGRADED: u32 = 6;
    pub const ALIVE: u32 = 7;
    /// num = 1 hidden / 0 visible.
    pub const PEER_HIDDEN: u32 = 8;
    /// Link ended. num = error code; ptr/len = `E_...` name.
    pub const CLOSED: u32 = 9;
    /// Non-fatal failure of a user action. num = error code; ptr/len = `E_...` name.
    pub const ERROR: u32 = 10;
    /// Progress. num: 1 = gathering, 2 = connecting, 3 = channel open.
    pub const PROGRESS: u32 = 11;
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

/// Everything the tab owns. Allocated once at start; buffers are reused for every link.
pub(crate) struct Inner {
    id: Identity,
    sess: Option<Box<Session>>,
    rtc: Option<rtc::Rtc>,
    /// Bumped on every new link so late callbacks of an old RTCPeerConnection are ignored.
    generation: u32,
    rx: Box<[u8; MAX_FRAME]>,
    text: Box<[u8; MAX_TEXT]>,
    /// Encoded codes and small event payloads.
    scratch: Box<[u8; 1024]>,
}

pub(crate) type Shared = Rc<RefCell<Inner>>;

impl Inner {
    fn reset(&mut self) {
        if let (Some(s), Some(r)) = (self.sess.as_mut(), self.rtc.as_ref()) {
            s.close(now_ms(), &mut sink(r));
        }
        if let Some(r) = self.rtc.take() {
            r.close();
        }
        self.sess = None;
        self.generation = self.generation.wrapping_add(1);
    }
}

/// Maps core events to the DataChannel and to JS.
pub(crate) fn sink(rtc: &rtc::Rtc) -> impl FnMut(Event<'_>) + '_ {
    move |e| match e {
        Event::Send(frame) => rtc.send(frame),
        Event::Connected { sas, peer } => {
            let mut b = [0u8; 15];
            b[..4].copy_from_slice(&sas.emoji);
            b[4..].copy_from_slice(&peer.handle());
            emit(ev::CONNECTED, sas.digits as f64, &b);
        }
        Event::Hello { nick, .. } => emit(ev::HELLO, 0.0, nick),
        Event::Chat { seq, text } => emit(ev::CHAT, seq as f64, text),
        Event::Delivered { seq } => emit(ev::DELIVERED, seq as f64, &[]),
        Event::Degraded => emit(ev::DEGRADED, 0.0, &[]),
        Event::Alive => emit(ev::ALIVE, 0.0, &[]),
        Event::PeerHidden(h) => emit(ev::PEER_HIDDEN, h as u8 as f64, &[]),
        Event::Closed(e) => emit_err(ev::CLOSED, e),
    }
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
                sess: None,
                rtc: None,
                generation: 0,
                rx: Box::new([0; MAX_FRAME]),
                text: Box::new([0; MAX_TEXT]),
                scratch: Box::new([0; 1024]),
            })),
        }
    }

    /// Display handle (`anon_xxxxxx`), not authentication.
    pub fn handle(&self) -> String {
        let h = self.inner.borrow().id.peer_id().handle();
        String::from_utf8_lossy(&h).into_owned()
    }

    /// Where JS writes outgoing UTF-8 (`TextEncoder.encodeInto`), `MAX_TEXT` bytes.
    pub fn text_ptr(&self) -> u32 {
        self.inner.borrow().text.as_ptr() as u32
    }

    pub fn text_cap(&self) -> u32 {
        MAX_TEXT as u32
    }

    /// Alice: new invite. `privacy`: 0 LAN-only, 1 default, 2 max connectivity. Emits CODE(1).
    pub fn create_invite(&self, privacy: u8, drop_ipv6: bool, ttl_s: u32) {
        let now_s = (now_ms() / 1000) as u32;
        let mut ids = [0u8; 32];
        ephem_crypto::random(&mut ids);
        let privacy = Privacy::from_u8(privacy);
        let generation = {
            let mut g = self.inner.borrow_mut();
            g.reset();
            let (inv, room) = (ids[..16].try_into().expect("16"), ids[16..].try_into().expect("16"));
            let s = Session::offerer(&g.id, inv, room, now_s + ttl_s.clamp(60, 1800), privacy, drop_ipv6);
            g.sess = Some(Box::new(s));
            g.generation
        };
        rtc::start(self.inner.clone(), generation, privacy, rtc::Step::Offer);
    }

    /// Applies a pasted/opened code: a full link, `#i=…`/`#a=…`, or bare base64url.
    /// An invite starts Bob's side (emits CODE(2)); an answer completes Alice's side.
    /// Returns 0 or an error code (also emitted as ERROR).
    pub fn apply_code(&self, text: &str, privacy: u8, drop_ipv6: bool) -> u32 {
        match self.apply_code_inner(text, Privacy::from_u8(privacy), drop_ipv6) {
            Ok(()) => 0,
            Err(e) => {
                emit_err(ev::ERROR, e);
                e.code() as u32
            }
        }
    }

    fn apply_code_inner(&self, text: &str, privacy: Privacy, drop_ipv6: bool) -> Result<(), ErrorCode> {
        let payload = extract_payload(text.trim());
        let mut bin = [0u8; MAX_CODE_LEN];
        let n = b64url::decode(payload.as_bytes(), &mut bin).map_err(|_| ErrorCode::InvalidInvite)?;
        let code = &bin[..n];
        let now_s = (now_ms() / 1000) as u32;
        match code.get(1) {
            Some(1) => {
                let generation = {
                    let mut g = self.inner.borrow_mut();
                    let s = Session::answerer(&g.id, code, now_s, privacy, drop_ipv6)?;
                    g.reset();
                    let lan_only = s.privacy() == Privacy::LanOnly;
                    g.sess = Some(Box::new(s));
                    (g.generation, lan_only)
                };
                let p = if generation.1 { Privacy::LanOnly } else { privacy };
                rtc::start(self.inner.clone(), generation.0, p, rtc::Step::Answer);
                Ok(())
            }
            Some(2) => {
                let generation = {
                    let mut g = self.inner.borrow_mut();
                    let Inner { id, sess, generation, .. } = &mut *g;
                    let s = sess.as_mut().ok_or(ErrorCode::AnswerMismatch)?;
                    s.apply_answer(id, code, now_s)?;
                    *generation
                };
                rtc::apply_answer(self.inner.clone(), generation);
                Ok(())
            }
            _ => Err(ErrorCode::InvalidInvite),
        }
    }

    /// Sends `len` bytes previously written at `text_ptr()`. Returns chat_seq, or -error code.
    pub fn send(&self, len: u32) -> f64 {
        let mut g = self.inner.borrow_mut();
        let Inner { sess, rtc, text, .. } = &mut *g;
        let (Some(s), Some(r)) = (sess.as_mut(), rtc.as_ref()) else {
            return -(ErrorCode::PeerOffline.code() as f64);
        };
        let len = (len as usize).min(MAX_TEXT);
        match s.send_chat(now_ms(), &text[..len], &mut sink(r)) {
            Ok(seq) => seq as f64,
            Err(e) => -(e.code() as f64),
        }
    }

    /// Timer (JS calls it every second). `hidden` = `document.hidden`, carried in PING.
    pub fn tick(&self, hidden: bool) {
        let mut g = self.inner.borrow_mut();
        let Inner { sess, rtc, .. } = &mut *g;
        if let (Some(s), Some(r)) = (sess.as_mut(), rtc.as_ref()) {
            s.tick(now_ms(), hidden, &mut sink(r));
        } else if let Some(s) = sess.as_mut() {
            // Before a DataChannel exists only expiry can fire; nothing is sent.
            s.tick(now_ms(), hidden, &mut |e| {
                if let Event::Closed(e) = e {
                    emit_err(ev::CLOSED, e);
                }
            });
        }
    }

    /// Leaves the chat (GOODBYE), closes the connection, zeroizes session keys.
    pub fn close(&self) {
        self.inner.borrow_mut().reset();
    }

    /// 0 none, 1 gathering, 2 awaiting answer, 3 connecting, 4 connected, 5 closed.
    pub fn state(&self) -> u8 {
        match self.inner.borrow().sess.as_ref().map(|s| s.state()) {
            None => 0,
            Some(State::Gathering) => 1,
            Some(State::AwaitingAnswer) => 2,
            Some(State::Connecting) => 3,
            Some(State::Connected) => 4,
            Some(State::Closed) => 5,
        }
    }
}

impl Default for App {
    fn default() -> Self {
        Self::new()
    }
}

/// Accepts `https://…/#i=CODE`, `#a=CODE`, `i=CODE`, or `CODE`.
fn extract_payload(s: &str) -> &str {
    let s = s.rsplit_once('#').map_or(s, |(_, f)| f);
    for k in ["i=", "a="] {
        if let Some(rest) = s.strip_prefix(k) {
            return rest;
        }
    }
    s
}

#[cfg(test)]
mod tests {
    #[test]
    fn payload_forms() {
        assert_eq!(super::extract_payload("https://x.io/ephem/app/#i=AbC"), "AbC");
        assert_eq!(super::extract_payload("#a=Z-_"), "Z-_");
        assert_eq!(super::extract_payload("Qwe"), "Qwe");
    }
}
