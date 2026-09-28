//! web-sys RTCPeerConnection adapter (§9). Setup-path code: closures, promises and SDP strings
//! allocate here, once per link. The per-frame path (`onmessage` → core → `send`) does not
//! allocate on the Rust side; the one RX copy (JS `ArrayBuffer` → wasm RX slot) is documented
//! in §11.6.


use crate::{Inner, Shared, emit, emit_err, ev, meta, now_ms, on_link, room};
use core::cell::{Cell, RefCell};
use ephem_core::{Privacy, State};
use ephem_proto::ErrorCode;
use ephem_proto::b64url;
use ephem_proto::code::{IceParams, MAX_CODE_LEN};
use ephem_proto::frame::MAX_FRAME;
use ephem_proto::sdp::MAX_SDP_LEN;
use js_sys::{ArrayBuffer, Promise, Uint8Array};
use std::rc::{Rc, Weak};
use wasm_bindgen::JsCast;
use wasm_bindgen::prelude::*;
use wasm_bindgen_futures::JsFuture;
use web_sys::{
    MessageEvent, RtcBundlePolicy, RtcConfiguration, RtcDataChannel, RtcDataChannelInit, RtcDataChannelType, RtcIceConnectionState,
    RtcIceGatheringState, RtcIceServer, RtcIceTransportPolicy, RtcOfferOptions, RtcPeerConnection, RtcPeerConnectionIceEvent,
    RtcPeerConnectionState, RtcSdpType, RtcSessionDescriptionInit,
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

/// The RTCPeerConnection of one link path. Its callbacks carry the path's link id: once the link
/// takes a new path (or is gone) they find nothing and do nothing.
pub(crate) struct Rtc {
    pc: RtcPeerConnection,
    /// For callbacks and async work spawned from inside a core event (§13 T1).
    weak: Weak<RefCell<Inner>>,
    id: u32,
    srflx_at: Rc<Cell<f64>>,
    /// Perfect negotiation (§13): an in-band re-offer of ours is in flight.
    making_offer: Cell<bool>,
    restarts: Cell<u32>,
    /// Last selected-pair description, to report only changes (§9.2: check on every change).
    path: RefCell<String>,
    /// Ticks until the next getStats check.
    stats_in: Cell<u8>,
    dc: RtcDataChannel,
    _on_ice: Closure<dyn FnMut(RtcPeerConnectionIceEvent)>,
    _on_state: Closure<dyn FnMut()>,
    _on_ice_state: Closure<dyn FnMut()>,
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

    /// The peer's in-band re-offer / re-answer (§13 T1). Runs after the current core call returns.
    pub fn on_signal(&self, offer: bool, ice: IceParams) {
        let (weak, id) = (self.weak.clone(), self.id);
        wasm_bindgen_futures::spawn_local(async move {
            let Some(inner) = weak.upgrade() else { return };
            if let Err(e) = handle_signal(&inner, id, offer, ice).await {
                fail(&inner, id, e);
            }
        });
    }

    /// `ICE state · DC state · buffered bytes · restarts` for the diagnostics view (§18).
    pub fn describe(&self) -> String {
        format!(
            "ICE {:?} · DTLS/connection {:?} · channel {:?} · buffered {} B · ICE restarts {}",
            self.pc.ice_connection_state(),
            self.pc.connection_state(),
            self.dc.ready_state(),
            self.dc.buffered_amount(),
            self.restarts.get()
        )
        .to_lowercase()
    }

    pub fn close(&self) {
        self.pc.set_onicecandidate(None);
        self.pc.set_onconnectionstatechange(None);
        self.pc.set_oniceconnectionstatechange(None);
        self.dc.set_onopen(None);
        self.dc.set_onmessage(None);
        self.dc.set_onclose(None);
        self.dc.close();
        self.pc.close();
    }
}

/// Runs `f` on the link whose current path is `id`; stale callbacks are ignored.
fn with_link(weak: &Weak<RefCell<Inner>>, id: u32, f: impl FnOnce(&mut Inner, usize)) {
    let Some(inner) = weak.upgrade() else { return };
    let mut g = inner.borrow_mut();
    if let Some(i) = g.find(id).filter(|i| g.links[*i].rtc.is_some()) {
        f(&mut g, i);
    }
}

/// The path of link `i` is gone: before the first connection the link ends with `e`; afterwards
/// it is suspended and waits for a reconnect code (§13 T3, or T2 in a room).
fn lost(g: &mut Inner, i: usize, e: ErrorCode) {
    let now = now_ms();
    on_link!(g, i, |s, k, _t| s.path_lost(now, e, &mut k));
}

/// A protocol violation on the path ends the link.
fn abort(g: &mut Inner, i: usize, e: ErrorCode) {
    on_link!(g, i, |s, k, _t| s.abort(e, &mut k));
}

/// An async step of path `id` failed.
fn fail(inner: &Shared, id: u32, e: ErrorCode) {
    let mut g = inner.borrow_mut();
    if let Some(i) = g.find(id) {
        lost(&mut g, i, e);
    }
}

/// PROGRESS of link `i` (meta carries its member index).
fn progress(g: &mut Inner, i: usize, n: f64) {
    g.meta[meta::MEMBER] = g.links[i].member;
    emit(ev::PROGRESS, n, &[]);
}

fn build(inner: &Shared, id: u32, privacy: Privacy, srflx_at: Rc<Cell<f64>>) -> Result<Rtc, JsValue> {
    let srflx = srflx_at.clone();
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
            with_link(&w, id, |g, i| {
                let connected = g.links[i].sess.state() == State::Connected;
                lost(g, i, if connected { ErrorCode::IceFailed } else { ErrorCode::NoDirectPath });
            });
        }
    });
    pc.set_onconnectionstatechange(Some(on_state.as_ref().unchecked_ref()));

    // T1 (§13): a path that stays `disconnected` for 2 s gets an in-band ICE restart while the
    // channel may still carry the signalling.
    let (w, pc2) = (weak.clone(), pc.clone());
    let on_ice_state = Closure::<dyn FnMut()>::new(move || {
        if pc2.ice_connection_state() != RtcIceConnectionState::Disconnected {
            return;
        }
        let (w, pc3) = (w.clone(), pc2.clone());
        wasm_bindgen_futures::spawn_local(async move {
            sleep(2000).await;
            if pc3.ice_connection_state() == RtcIceConnectionState::Disconnected
                && let Some(inner) = w.upgrade()
            {
                restart(inner, id);
            }
        });
    });
    pc.set_oniceconnectionstatechange(Some(on_ice_state.as_ref().unchecked_ref()));

    let w = weak.clone();
    let on_open = Closure::<dyn FnMut()>::new(move || {
        with_link(&w, id, |g, i| {
            progress(g, i, 3.0);
            let now = now_ms();
            on_link!(g, i, |s, k, _t| s.on_open(now, &mut k));
        });
    });
    dc.set_onopen(Some(on_open.as_ref().unchecked_ref()));

    let w = weak.clone();
    let on_msg = Closure::<dyn FnMut(MessageEvent)>::new(move |e: MessageEvent| {
        let Ok(buf) = e.data().dyn_into::<ArrayBuffer>() else { return };
        let view = Uint8Array::new(&buf);
        let len = view.length() as usize;
        with_link(&w, id, |g, i| {
            if len > MAX_FRAME {
                abort(g, i, ErrorCode::MessageTooLarge);
                return;
            }
            let now = now_ms();
            let Inner { links, meta, inbox, rx, .. } = g;
            let crate::Link { sess, rtc, id, member, peer, .. } = &mut links[i];
            // The single documented RX copy (§11.6): JS ArrayBuffer → preallocated wasm slot.
            view.copy_to(&mut rx[..len]);
            let mut out = crate::Out { rtc: rtc.as_ref(), meta, inbox, peer, link: *id, member: *member };
            sess.on_frame(now, &mut rx[..len], &mut |e| crate::on_event(&mut out, e));
        });
        // Room records the frame carried (§14), handled with the borrow released.
        if let Some(inner) = w.upgrade() {
            room::drain(&inner);
        }
    });
    dc.set_onmessage(Some(on_msg.as_ref().unchecked_ref()));

    let w = weak.clone();
    let on_close = Closure::<dyn FnMut()>::new(move || with_link(&w, id, |g, i| lost(g, i, ErrorCode::IceFailed)));
    dc.set_onclose(Some(on_close.as_ref().unchecked_ref()));

    Ok(Rtc {
        pc,
        weak,
        id,
        srflx_at: srflx,
        making_offer: Cell::new(false),
        restarts: Cell::new(0),
        dc,
        path: RefCell::new(String::new()),
        stats_in: Cell::new(0),
        _on_ice: on_ice,
        _on_state: on_state,
        _on_ice_state: on_ice_state,
        _on_open: on_open,
        _on_msg: on_msg,
        _on_close: on_close,
    })
}

/// Creates the RTCPeerConnection for path `id` and runs offer or answer negotiation.
pub(crate) fn start(inner: Shared, id: u32, privacy: Privacy, step: Step) {
    let srflx_at = Rc::new(Cell::new(0.0));
    let rtc = match build(&inner, id, privacy, srflx_at.clone()) {
        Ok(r) => r,
        Err(_) => {
            emit_err(ev::ERROR, ErrorCode::BrowserUnsupported);
            return;
        }
    };
    let pc = rtc.pc.clone();
    {
        let mut g = inner.borrow_mut();
        let Some(i) = g.find(id) else {
            rtc.close();
            return;
        };
        g.links[i].rtc = Some(rtc);
        progress(&mut g, i, 1.0);
    }
    wasm_bindgen_futures::spawn_local(async move {
        if let Err(e) = negotiate(&inner, id, &pc, &srflx_at, step).await {
            fail(&inner, id, e);
        }
    });
}

/// Offerer: sets the answer (already validated by the core) as the remote description.
pub(crate) fn apply_answer(inner: Shared, id: u32) {
    wasm_bindgen_futures::spawn_local(async move {
        let res = async {
            let (pc, desc) = remote_description(&inner, id, RtcSdpType::Answer)?;
            wait(pc.set_remote_description(&desc)).await?;
            let mut g = inner.borrow_mut();
            if let Some(i) = g.find(id) {
                progress(&mut g, i, 2.0);
            }
            Ok(())
        };
        if let Err(e) = res.await {
            fail(&inner, id, e);
        }
    });
}

fn remote_description(inner: &Shared, id: u32, ty: RtcSdpType) -> Result<(RtcPeerConnection, RtcSessionDescriptionInit), ErrorCode> {
    let g = inner.borrow();
    let l = g.find(id).map(|i| &g.links[i]).ok_or(ErrorCode::NotPermitted)?;
    let r = l.rtc.as_ref().ok_or(ErrorCode::NotPermitted)?;
    let mut sdp = [0u8; MAX_SDP_LEN];
    let text = l.sess.remote_sdp(&mut sdp)?;
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

async fn negotiate(inner: &Shared, id: u32, pc: &RtcPeerConnection, srflx_at: &Cell<f64>, step: Step) -> Result<(), ErrorCode> {
    let local_ty = match step {
        Step::Offer => RtcSdpType::Offer,
        Step::Answer => {
            let (_, desc) = remote_description(inner, id, RtcSdpType::Offer)?;
            wait(pc.set_remote_description(&desc)).await?;
            RtcSdpType::Answer
        }
    };
    let local = wait(match step {
        Step::Offer => pc.create_offer(),
        Step::Answer => pc.create_answer(),
    })
    .await?;
    let local_sdp = set_local(pc, local_ty, &local, srflx_at).await?;

    // The code is copied out of the session (≤ MAX_CODE_LEN bytes, stack) so the room layer can
    // seal it after the borrow ends.
    let mut code = [0u8; MAX_CODE_LEN];
    let (n, via_owner, i) = {
        let mut g = inner.borrow_mut();
        let Some(i) = g.find(id) else { return Ok(()) };
        let Inner { id: me, links, .. } = &mut *g;
        let c = links[i].sess.build_code(me, &local_sdp)?;
        code[..c.len()].copy_from_slice(c);
        (c.len(), links[i].via_owner, i)
    };
    if via_owner {
        // Introduction or T2 inside a room: sealed to the member, relayed by the owner (§14.4).
        room::relay_code(inner, id, &code[..n])?;
    } else {
        let mut g = inner.borrow_mut();
        let kind = code[1];
        let len = b64url::encode(&code[..n], &mut g.scratch[..]).map_err(|_| ErrorCode::InvalidInvite)?;
        g.meta[meta::MEMBER] = g.links[i].member;
        let ptr = g.scratch.as_ptr() as u32;
        drop(g);
        // Emitted after the borrow ends; the scratch buffer is stable (boxed at start).
        crate::js_event(ev::CODE, kind as f64, ptr, len as u32);
    }
    if step == Step::Answer {
        let mut g = inner.borrow_mut();
        if let Some(i) = g.find(id) {
            progress(&mut g, i, 2.0);
        }
    }
    Ok(())
}

/// Seconds between getStats checks while connected.
const STATS_EVERY_S: u8 = 5;

/// §9.2 point 3 and §18: reads the selected candidate pair of every link. A relay on either side
/// closes that link with `E_RELAY_REJECTED`; otherwise the pair is reported (PATH) when it
/// changes. Diagnostics path: runs every few seconds, allocations here are acceptable.
pub(crate) fn check_paths(inner: &Shared) {
    let due: Vec<(RtcPeerConnection, u32)> = {
        let g = inner.borrow();
        g.links
            .iter()
            .filter_map(|l| {
                let r = l.rtc.as_ref()?;
                let left = r.stats_in.get();
                if left > 0 {
                    r.stats_in.set(left - 1);
                    return None;
                }
                r.stats_in.set(STATS_EVERY_S);
                Some((r.pc.clone(), l.id))
            })
            .collect()
    };
    for (pc, id) in due {
        let inner = inner.clone();
        wasm_bindgen_futures::spawn_local(async move {
            let Ok(report) = JsFuture::from(pc.get_stats()).await else { return };
            let Some((text, relay)) = selected_pair(report.unchecked_ref()) else { return };
            let mut g = inner.borrow_mut();
            let Some(i) = g.find(id) else { return };
            g.meta[meta::MEMBER] = g.links[i].member;
            if relay {
                emit(ev::PATH, 1.0, text.as_bytes());
                abort(&mut g, i, ErrorCode::RelayRejected);
                return;
            }
            let changed = g.links[i].rtc.as_ref().is_some_and(|r| {
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
    // `type addr:port`, IPv6 in brackets so the UI can hide addresses (§18).
    let desc = |c: &JsValue| {
        let addr = field(c, "address").or_else(|| field(c, "ip")).and_then(|x| x.as_string()).unwrap_or_else(|| "?".into());
        let port = field(c, "port").and_then(|x| x.as_f64()).unwrap_or(0.0);
        let ty = text_field(c, "candidateType");
        if addr.contains(':') { format!("{ty} [{addr}]:{port}") } else { format!("{ty} {addr}:{port}") }
    };
    let relay = text_field(&local, "candidateType") == "relay" || text_field(&remote, "candidateType") == "relay";
    let num = |k: &str| field(&pair, k).and_then(|x| x.as_f64()).unwrap_or(0.0);
    let kb = |b: f64| b / 1024.0;
    Some((
        format!(
            "you {} ↔ peer {} ({}) · RTT {:.0} ms · ↑ {:.1} KB ↓ {:.1} KB",
            desc(&local),
            desc(&remote),
            text_field(&local, "protocol"),
            num("currentRoundTripTime") * 1000.0,
            kb(num("bytesSent")),
            kb(num("bytesReceived"))
        ),
        relay,
    ))
}

/// Sets a created offer/answer as local description, gathers (§9.5) and returns the full SDP.
async fn set_local(pc: &RtcPeerConnection, ty: RtcSdpType, created: &JsValue, srflx_at: &Cell<f64>) -> Result<String, ErrorCode> {
    let sdp = js_sys::Reflect::get(created, &JsValue::from_str("sdp")).ok().and_then(|v| v.as_string()).ok_or(ErrorCode::IceFailed)?;
    srflx_at.set(0.0);
    wait(pc.set_local_description(&description(ty, &sdp))).await?;
    gather(pc, srflx_at).await;
    pc.local_description().map(|d| d.sdp()).ok_or(ErrorCode::IceFailed)
}


/// Our side of an in-band ICE restart (§13 T1): re-offer with new credentials over the channel.
/// Triggered by a path stuck in `disconnected`, a network change, or the diagnostics button.
pub(crate) fn restart(inner: Shared, id: u32) {
    let (pc, srflx) = {
        let g = inner.borrow();
        let Some(l) = g.find(id).map(|i| &g.links[i]) else { return };
        match l.rtc.as_ref() {
            Some(r) if l.sess.state() == State::Connected && !r.making_offer.get() => {
                r.making_offer.set(true);
                r.restarts.set(r.restarts.get() + 1);
                (r.pc.clone(), r.srflx_at.clone())
            }
            _ => return,
        }
    };
    wasm_bindgen_futures::spawn_local(async move {
        let res = async {
            let opts = RtcOfferOptions::new();
            opts.set_ice_restart(true);
            let offer = wait(pc.create_offer_with_rtc_offer_options(&opts)).await?;
            let sdp = set_local(&pc, RtcSdpType::Offer, &offer, &srflx).await?;
            let mut g = inner.borrow_mut();
            let Some(i) = g.find(id) else { return Ok(()) };
            let now = now_ms();
            on_link!(g, i, |s, k, _t| s.signal(now, true, &sdp, &mut k))
        };
        let r = res.await;
        let g = inner.borrow();
        if r.is_err()
            && let Some(rtc) = g.find(id).and_then(|i| g.links[i].rtc.as_ref())
        {
            rtc.making_offer.set(false);
        }
    });
}

/// Applies the peer's re-offer (answering it) or re-answer (§13 T1), with the perfect-negotiation
/// rule for glare: the polite peer (greater `PeerId`) rolls back its own offer, the other ignores
/// the colliding one.
async fn handle_signal(inner: &Shared, id: u32, offer: bool, ice: IceParams) -> Result<(), ErrorCode> {
    let (pc, srflx, desc, polite, colliding) = {
        let mut g = inner.borrow_mut();
        let Some(i) = g.find(id) else { return Ok(()) };
        let me = g.id.peer_id();
        let crate::Link { sess, rtc, .. } = &mut g.links[i];
        let Some(r) = rtc.as_ref() else { return Ok(()) };
        let polite = me > sess.remote();
        let colliding = offer && r.making_offer.get();
        if colliding && !polite {
            return Ok(());
        }
        let mut sdp = [0u8; MAX_SDP_LEN];
        let text = sess.render_signal(&ice, offer, &mut sdp)?;
        let ty = if offer { RtcSdpType::Offer } else { RtcSdpType::Answer };
        (r.pc.clone(), r.srflx_at.clone(), description(ty, text), polite, colliding)
    };
    if colliding && polite {
        wait(pc.set_local_description(&RtcSessionDescriptionInit::new(RtcSdpType::Rollback))).await?;
    }
    wait(pc.set_remote_description(&desc)).await?;
    if !offer {
        let g = inner.borrow();
        if let Some(r) = g.find(id).and_then(|i| g.links[i].rtc.as_ref()) {
            r.making_offer.set(false);
        }
        return Ok(());
    }
    let answer = wait(pc.create_answer()).await?;
    let sdp = set_local(&pc, RtcSdpType::Answer, &answer, &srflx).await?;
    let mut g = inner.borrow_mut();
    let Some(i) = g.find(id) else { return Ok(()) };
    if colliding && let Some(r) = g.links[i].rtc.as_ref() {
        r.making_offer.set(false);
    }
    let now = now_ms();
    on_link!(g, i, |s, k, _t| s.signal(now, false, &sdp, &mut k))
}
