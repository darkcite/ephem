//! web-sys RTCPeerConnection adapter (§9). Setup-path code: closures, promises and SDP strings
//! allocate here, once per link. The per-frame path (`onmessage` → core → `send`) does not
//! allocate on the Rust side; the one RX copy (JS `ArrayBuffer` → wasm RX slot) is documented
//! in §11.6.

use crate::{Inner, Shared, emit, emit_err, ev, now_ms, sink};
use core::cell::{Cell, RefCell};
use ephem_core::{Privacy, State};
use ephem_proto::ErrorCode;
use ephem_proto::b64url;
use ephem_proto::frame::MAX_FRAME;
use ephem_proto::sdp::MAX_SDP_LEN;
use js_sys::{ArrayBuffer, Promise, Uint8Array};
use std::rc::{Rc, Weak};
use wasm_bindgen::JsCast;
use wasm_bindgen::prelude::*;
use wasm_bindgen_futures::JsFuture;
use web_sys::{
    MessageEvent, RtcBundlePolicy, RtcConfiguration, RtcDataChannel, RtcDataChannelInit, RtcDataChannelType, RtcIceGatheringState,
    RtcIceServer, RtcIceTransportPolicy, RtcPeerConnection, RtcPeerConnectionIceEvent, RtcPeerConnectionState, RtcSdpType,
    RtcSessionDescriptionInit,
};

/// Default STUN list (§9.3): two operators. Never TURN (§9.2).
pub const STUN: [&str; 2] = ["stun:stun.l.google.com:19302", "stun:stun.cloudflare.com:3478"];
const _: () = {
    let mut i = 0;
    while i < STUN.len() {
        let b = STUN[i].as_bytes();
        assert!(b[0] == b's' && b[1] == b't' && b[2] == b'u' && b[3] == b'n' && b[4] == b':', "only stun: URLs");
        i += 1;
    }
};

/// Gathering for a code (§9.5).
const GATHER_AFTER_SRFLX_MS: f64 = 1500.0;
const GATHER_CAP_MS: f64 = 3000.0;

#[derive(Copy, Clone, PartialEq, Eq)]
pub enum Step {
    Offer,
    Answer,
}

pub(crate) struct Rtc {
    pc: RtcPeerConnection,
    /// Last selected-pair description, to report only changes (§9.2: check on every change).
    path: RefCell<String>,
    /// Ticks until the next getStats check.
    stats_in: Cell<u8>,
    dc: RtcDataChannel,
    _on_ice: Closure<dyn FnMut(RtcPeerConnectionIceEvent)>,
    _on_state: Closure<dyn FnMut()>,
    _on_open: Closure<dyn FnMut()>,
    _on_msg: Closure<dyn FnMut(MessageEvent)>,
    _on_close: Closure<dyn FnMut()>,
}

impl Rtc {
    /// Frame out. `&[u8]` reaches JS as a view of wasm memory (no copy on our side).
    #[inline]
    pub fn send(&self, frame: &[u8]) {
        // A failed send means the channel is closing; `onclose` reports it.
        let _ = self.dc.send_with_u8_array(frame);
    }

    pub fn close(&self) {
        self.pc.set_onicecandidate(None);
        self.pc.set_onconnectionstatechange(None);
        self.dc.set_onopen(None);
        self.dc.set_onmessage(None);
        self.dc.set_onclose(None);
        self.dc.close();
        self.pc.close();
    }
}

/// Runs `f` on the live link of `generation`; stale callbacks are ignored.
fn with_link(weak: &Weak<RefCell<Inner>>, generation: u32, f: impl FnOnce(&mut Inner)) {
    let Some(inner) = weak.upgrade() else { return };
    let mut g = inner.borrow_mut();
    if g.generation == generation && g.sess.is_some() && g.rtc.is_some() {
        f(&mut g);
    }
}

/// The path is gone: before the first connection the chat ends with `e`; afterwards it is
/// suspended and waits for a reconnect code (§13 T3).
fn lost(g: &mut Inner, e: ErrorCode) {
    let Inner { sess, rtc, meta, .. } = g;
    if let (Some(s), Some(r)) = (sess.as_mut(), rtc.as_ref()) {
        s.path_lost(now_ms(), e, &mut sink(r, meta));
    }
}

/// A protocol violation on the path ends the chat.
fn abort(g: &mut Inner, e: ErrorCode) {
    let Inner { sess, rtc, meta, .. } = g;
    if let (Some(s), Some(r)) = (sess.as_mut(), rtc.as_ref()) {
        s.abort(e, &mut sink(r, meta));
    }
}

fn build(inner: &Shared, generation: u32, privacy: Privacy, srflx_at: Rc<Cell<f64>>) -> Result<Rtc, JsValue> {
    let cfg = RtcConfiguration::new();
    let servers = js_sys::Array::new();
    if privacy != Privacy::LanOnly {
        for url in STUN {
            let s = RtcIceServer::new();
            s.set_urls_str(url);
            servers.push(&s);
        }
    }
    cfg.set_ice_servers(&servers);
    cfg.set_bundle_policy(RtcBundlePolicy::MaxBundle);
    cfg.set_ice_transport_policy(RtcIceTransportPolicy::All);
    let pc = RtcPeerConnection::new_with_configuration(&cfg)?;

    let init = RtcDataChannelInit::new();
    init.set_negotiated(true);
    init.set_id(0);
    init.set_ordered(true);
    let dc = pc.create_data_channel_with_data_channel_dict("c", &init);
    dc.set_binary_type(RtcDataChannelType::Arraybuffer);

    let weak = Rc::downgrade(inner);

    let on_ice = Closure::<dyn FnMut(RtcPeerConnectionIceEvent)>::new(move |e: RtcPeerConnectionIceEvent| {
        if let Some(c) = e.candidate()
            && srflx_at.get() == 0.0
            && c.candidate().contains(" typ srflx")
        {
            srflx_at.set(js_sys::Date::now());
        }
    });
    pc.set_onicecandidate(Some(on_ice.as_ref().unchecked_ref()));

    let (w, pc2) = (weak.clone(), pc.clone());
    let on_state = Closure::<dyn FnMut()>::new(move || {
        if pc2.connection_state() == RtcPeerConnectionState::Failed {
            with_link(&w, generation, |g| {
                let connected = g.sess.as_ref().is_some_and(|s| s.state() == State::Connected);
                lost(g, if connected { ErrorCode::IceFailed } else { ErrorCode::NoDirectPath });
            });
        }
    });
    pc.set_onconnectionstatechange(Some(on_state.as_ref().unchecked_ref()));

    let w = weak.clone();
    let on_open = Closure::<dyn FnMut()>::new(move || {
        with_link(&w, generation, |g| {
            emit(ev::PROGRESS, 3.0, &[]);
            let Inner { sess, rtc, meta, .. } = g;
            if let (Some(s), Some(r)) = (sess.as_mut(), rtc.as_ref()) {
                s.on_open(now_ms(), &mut sink(r, meta));
            }
        });
    });
    dc.set_onopen(Some(on_open.as_ref().unchecked_ref()));

    let w = weak.clone();
    let on_msg = Closure::<dyn FnMut(MessageEvent)>::new(move |e: MessageEvent| {
        let Ok(buf) = e.data().dyn_into::<ArrayBuffer>() else { return };
        let view = Uint8Array::new(&buf);
        let len = view.length() as usize;
        with_link(&w, generation, |g| {
            if len > MAX_FRAME {
                abort(g, ErrorCode::MessageTooLarge);
                return;
            }
            let Inner { sess, rtc, rx, meta, .. } = g;
            if let (Some(s), Some(r)) = (sess.as_mut(), rtc.as_ref()) {
                // The single documented RX copy (§11.6): JS ArrayBuffer → preallocated wasm slot.
                view.copy_to(&mut rx[..len]);
                s.on_frame(now_ms(), &mut rx[..len], &mut sink(r, meta));
            }
        });
    });
    dc.set_onmessage(Some(on_msg.as_ref().unchecked_ref()));

    let w = weak;
    let on_close = Closure::<dyn FnMut()>::new(move || with_link(&w, generation, |g| lost(g, ErrorCode::IceFailed)));
    dc.set_onclose(Some(on_close.as_ref().unchecked_ref()));

    Ok(Rtc { pc, dc, path: RefCell::new(String::new()), stats_in: Cell::new(0), _on_ice: on_ice, _on_state: on_state, _on_open: on_open, _on_msg: on_msg, _on_close: on_close })
}

/// Creates the RTCPeerConnection for the current session and runs offer or answer negotiation.
pub(crate) fn start(inner: Shared, generation: u32, privacy: Privacy, step: Step) {
    let srflx_at = Rc::new(Cell::new(0.0));
    let rtc = match build(&inner, generation, privacy, srflx_at.clone()) {
        Ok(r) => r,
        Err(_) => {
            emit_err(ev::ERROR, ErrorCode::BrowserUnsupported);
            return;
        }
    };
    let pc = rtc.pc.clone();
    inner.borrow_mut().rtc = Some(rtc);
    emit(ev::PROGRESS, 1.0, &[]);
    wasm_bindgen_futures::spawn_local(async move {
        if let Err(e) = negotiate(&inner, generation, &pc, &srflx_at, step).await {
            let mut g = inner.borrow_mut();
            if g.generation == generation {
                lost(&mut g, e);
            }
        }
    });
}

/// Alice: sets Bob's answer (already validated by the core) as the remote description.
pub(crate) fn apply_answer(inner: Shared, generation: u32) {
    wasm_bindgen_futures::spawn_local(async move {
        let res = async {
            let (pc, desc) = remote_description(&inner, generation, RtcSdpType::Answer)?;
            wait(pc.set_remote_description(&desc)).await?;
            emit(ev::PROGRESS, 2.0, &[]);
            Ok(())
        };
        if let Err(e) = res.await {
            let mut g = inner.borrow_mut();
            if g.generation == generation {
                lost(&mut g, e);
            }
        }
    });
}

fn remote_description(inner: &Shared, generation: u32, ty: RtcSdpType) -> Result<(RtcPeerConnection, RtcSessionDescriptionInit), ErrorCode> {
    let g = inner.borrow();
    if g.generation != generation {
        return Err(ErrorCode::NotPermitted);
    }
    let (Some(s), Some(r)) = (g.sess.as_ref(), g.rtc.as_ref()) else {
        return Err(ErrorCode::NotPermitted);
    };
    let mut sdp = [0u8; MAX_SDP_LEN];
    let text = s.remote_sdp(&mut sdp)?;
    Ok((r.pc.clone(), description(ty, text)))
}

fn description(ty: RtcSdpType, sdp: &str) -> RtcSessionDescriptionInit {
    let d = RtcSessionDescriptionInit::new(ty);
    d.set_sdp(sdp);
    d
}

async fn wait(p: Promise) -> Result<JsValue, ErrorCode> {
    JsFuture::from(p).await.map_err(|_| ErrorCode::IceFailed)
}

async fn sleep(ms: i32) {
    let p = Promise::new(&mut |resolve, _| {
        if let Some(w) = web_sys::window() {
            let _ = w.set_timeout_with_callback_and_timeout_and_arguments_0(&resolve, ms);
        }
    });
    let _ = JsFuture::from(p).await;
}

/// Waits for gathering per §9.5: complete, or 1.5 s after the first srflx, capped at 3 s.
async fn gather(pc: &RtcPeerConnection, srflx_at: &Cell<f64>) {
    let start = js_sys::Date::now();
    loop {
        if pc.ice_gathering_state() == RtcIceGatheringState::Complete {
            return;
        }
        let now = js_sys::Date::now();
        let s = srflx_at.get();
        if (s > 0.0 && now - s >= GATHER_AFTER_SRFLX_MS) || now - start >= GATHER_CAP_MS {
            return;
        }
        sleep(100).await;
    }
}

async fn negotiate(inner: &Shared, generation: u32, pc: &RtcPeerConnection, srflx_at: &Cell<f64>, step: Step) -> Result<(), ErrorCode> {
    let local_ty = match step {
        Step::Offer => RtcSdpType::Offer,
        Step::Answer => {
            let (_, desc) = remote_description(inner, generation, RtcSdpType::Offer)?;
            wait(pc.set_remote_description(&desc)).await?;
            RtcSdpType::Answer
        }
    };
    let local = wait(match step {
        Step::Offer => pc.create_offer(),
        Step::Answer => pc.create_answer(),
    })
    .await?;
    let sdp = js_sys::Reflect::get(&local, &JsValue::from_str("sdp"))
        .ok()
        .and_then(|v| v.as_string())
        .ok_or(ErrorCode::IceFailed)?;
    wait(pc.set_local_description(&description(local_ty, &sdp))).await?;
    gather(pc, srflx_at).await;
    let local_sdp = pc.local_description().map(|d| d.sdp()).ok_or(ErrorCode::IceFailed)?;

    let (ptr, n, kind) = {
        let mut g = inner.borrow_mut();
        if g.generation != generation {
            return Ok(());
        }
        let Inner { id, sess, scratch, .. } = &mut *g;
        let code = sess.as_mut().ok_or(ErrorCode::NotPermitted)?.build_code(id, &local_sdp)?;
        let kind = code.get(1).copied().unwrap_or(0);
        let n = b64url::encode(code, &mut scratch[..]).map_err(|_| ErrorCode::InvalidInvite)?;
        (scratch.as_ptr() as u32, n as u32, kind)
    };
    // Emitted after the borrow ends; the scratch buffer is stable (boxed at start).
    crate::js_event(ev::CODE, kind as f64, ptr, n);
    if step == Step::Answer {
        emit(ev::PROGRESS, 2.0, &[]);
    }
    Ok(())
}

/// Seconds between getStats checks while connected.
const STATS_EVERY_S: u8 = 5;

/// §9.2 point 3 and §18: reads the selected candidate pair. A relay on either side closes the
/// chat with `E_RELAY_REJECTED`; otherwise the pair is reported (PATH) when it changes.
/// Diagnostics path: runs every few seconds, allocations here are acceptable.
pub(crate) fn check_path(inner: Shared) {
    let (pc, generation) = {
        let g = inner.borrow();
        let Some(r) = g.rtc.as_ref() else { return };
        let left = r.stats_in.get();
        if left > 0 {
            r.stats_in.set(left - 1);
            return;
        }
        r.stats_in.set(STATS_EVERY_S);
        (r.pc.clone(), g.generation)
    };
    wasm_bindgen_futures::spawn_local(async move {
        let Ok(report) = JsFuture::from(pc.get_stats()).await else { return };
        let Some((text, relay)) = selected_pair(report.unchecked_ref()) else { return };
        let mut g = inner.borrow_mut();
        if g.generation != generation {
            return;
        }
        if relay {
            emit(ev::PATH, 1.0, text.as_bytes());
            abort(&mut g, ErrorCode::RelayRejected);
            return;
        }
        let changed = g.rtc.as_ref().is_some_and(|r| {
            let mut last = r.path.borrow_mut();
            let c = *last != text;
            if c {
                last.clone_from(&text);
            }
            c
        });
        drop(g);
        if changed {
            emit(ev::PATH, 0.0, text.as_bytes());
        }
    });
}

fn field(o: &JsValue, k: &str) -> Option<JsValue> {
    js_sys::Reflect::get(o, &JsValue::from_str(k)).ok().filter(|v| !v.is_undefined() && !v.is_null())
}

fn text_field(o: &JsValue, k: &str) -> String {
    field(o, k).and_then(|v| v.as_string()).unwrap_or_default()
}

/// `(description, relay_seen)` of the selected candidate pair.
fn selected_pair(report: &js_sys::Map) -> Option<(String, bool)> {
    let mut pair: Option<JsValue> = None;
    let mut selected_id: Option<String> = None;
    report.for_each(&mut |v, _| {
        match text_field(&v, "type").as_str() {
            // Chrome, Safari: the transport names the selected pair.
            "transport" => selected_id = selected_id.take().or_else(|| field(&v, "selectedCandidatePairId").and_then(|x| x.as_string())),
            // Firefox marks the pair itself.
            "candidate-pair" if field(&v, "selected").and_then(|x| x.as_bool()) == Some(true) => pair = Some(v),
            _ => {}
        }
    });
    let pair = match (pair, selected_id) {
        (Some(p), _) => p,
        (None, Some(id)) => report.get(&JsValue::from_str(&id)),
        (None, None) => return None,
    };
    if pair.is_undefined() {
        return None;
    }
    let local = report.get(&field(&pair, "localCandidateId")?);
    let remote = report.get(&field(&pair, "remoteCandidateId")?);
    let desc = |c: &JsValue| {
        let addr = field(c, "address").or_else(|| field(c, "ip")).and_then(|x| x.as_string()).unwrap_or_else(|| "?".into());
        let port = field(c, "port").and_then(|x| x.as_f64()).unwrap_or(0.0);
        (text_field(c, "candidateType"), addr, port)
    };
    let (lt, la, lp) = desc(&local);
    let (rt, ra, rp) = desc(&remote);
    let proto = text_field(&local, "protocol");
    let relay = lt == "relay" || rt == "relay";
    Some((format!("you {lt} {la}:{lp} ↔ peer {rt} {ra}:{rp} ({proto})"), relay))
}
