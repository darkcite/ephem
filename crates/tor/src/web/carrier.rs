//! Snowflake in the browser: broker rendezvous (`fetch`), WebRTC DataChannels to volunteer
//! proxies, and the Turbotunnel session's packets over them (§28.3; spike E2 for the exchange).
//!
//! **Warm pool.** arti gives a bridge connection 5 s for connect + TLS + link handshake, while
//! a rendezvous takes 3–10 s (E2). Like the reference Snowflake client, we keep [`POOL`] proxies
//! connected in advance; a dial takes one and the handshakes run over an open channel at once.
//! [`WarmPool::ready`] lets bootstrap wait for the first one, so arti never times out on it
//! (which would mark the bridge down for minutes).
//!
//! When a proxy goes away (DataChannel closed, or silent for [`SILENT_MS`]) the carrier takes
//! another and continues the same session; arti does not notice. These `RTCPeerConnection`s
//! carry only Tor traffic and use only the Snowflake STUN list (§28.5).
//!
//! Copies: one per received DataChannel message (JS `ArrayBuffer` → the carrier's buffer,
//! unavoidable, §11.6); the send path hands the browser a view of wasm memory.

use super::rt::sleep_ms;
use crate::net::Dialer;
use crate::stream::{Link, SnowflakeStream};
use ephem_snowflake::Session;
use js_sys::{ArrayBuffer, Uint8Array};
use std::cell::{Cell, RefCell};
use std::collections::VecDeque;
use std::rc::Rc;
use wasm_bindgen::JsCast;
use wasm_bindgen::prelude::*;
use wasm_bindgen_futures::JsFuture;
use web_sys::{
    MessageEvent, RequestInit, Response, RtcConfiguration, RtcDataChannel, RtcDataChannelInit, RtcDataChannelState, RtcDataChannelType,
    RtcIceGatheringState, RtcIceServer, RtcPeerConnection, RtcSdpType, RtcSessionDescriptionInit,
};

/// A proxy that sends nothing for this long is replaced (Snowflake's `SnowflakeTimeout`).
const SILENT_MS: f64 = 20_000.0;
/// ...or for this long while our data waits for its acknowledgement: a proxy that died under
/// load (arti gives up on a directory download after 10 s without data, and failures mark the
/// bridge down for minutes).
const STALL_MS: f64 = 4_000.0;
const OPEN_TIMEOUT_MS: f64 = 10_000.0;
const GATHER_TIMEOUT_MS: f64 = 5_000.0;
/// Proxies kept connected in advance, per bridge.
pub const POOL: usize = 2;
/// A carrier without a proxy for this long gives up (the stream fails, arti retries).
const NO_PROXY_MS: f64 = 60_000.0;

/// Where to find a proxy for which bridge.
#[derive(Clone, Debug)]
pub struct SnowflakeParams {
    /// Broker URLs, tried in order when one cannot be reached (the direct broker, then its
    /// CDN URL, which works from browsers without domain fronting: §24.2 E2).
    pub brokers: Vec<String>,
    /// The Snowflake bridges (RSA fingerprints): the broker matches a proxy to one of them.
    pub fingerprints: Vec<String>,
    /// `stun:` URLs for the proxy connections only.
    pub ice: Vec<String>,
    /// NAT type hint for the broker's proxy matching: "unknown" on the real network (the
    /// broker then offers unrestricted proxies); the offline lab says "unrestricted".
    pub nat: String,
}

/// An open DataChannel to a proxy, not yet carrying a session.
struct Warm {
    pc: RtcPeerConnection,
    dc: RtcDataChannel,
}

impl Warm {
    fn open(&self) -> bool {
        self.dc.ready_state() == RtcDataChannelState::Open
    }

    fn close(&self) {
        self.dc.set_onmessage(None);
        self.dc.close();
        self.pc.close();
    }
}

struct PoolInner {
    params: SnowflakeParams,
    /// The bridge this pool's proxies relay to.
    fingerprint: String,
    ready: VecDeque<Warm>,
    pending: usize,
    last_error: String,
}

/// Proxies connected ahead of use.
#[derive(Clone)]
pub struct WarmPool(Rc<RefCell<PoolInner>>);

impl WarmPool {
    /// Starts filling the pool of proxies for bridge `fingerprint` (runs for the life of the
    /// page).
    pub fn start(params: SnowflakeParams, fingerprint: String) -> Self {
        let pool = WarmPool(Rc::new(RefCell::new(PoolInner { params, fingerprint, ready: VecDeque::new(), pending: 0, last_error: String::new() })));
        wasm_bindgen_futures::spawn_local(pool.clone().refill());
        pool
    }

    async fn refill(self) {
        let mut failures = 0u32;
        loop {
            let (want, params) = {
                let mut g = self.0.borrow_mut();
                g.ready.retain(|w| {
                    let ok = w.open();
                    if !ok {
                        w.close();
                    }
                    ok
                });
                (POOL.saturating_sub(g.ready.len() + g.pending), (g.params.clone(), g.fingerprint.clone()))
            };
            for _ in 0..want {
                self.0.borrow_mut().pending += 1;
                let pool = self.clone();
                let (p, fp) = params.clone();
                wasm_bindgen_futures::spawn_local(async move {
                    let r = rendezvous(&p, &fp).await;
                    let mut g = pool.0.borrow_mut();
                    g.pending -= 1;
                    match r {
                        Ok(w) => g.ready.push_back(w),
                        Err(e) => g.last_error = e,
                    }
                });
            }
            let empty = self.0.borrow().ready.is_empty();
            failures = if empty && want > 0 { failures + 1 } else { 0 };
            sleep_ms(if empty { 500 * failures.clamp(1, 8) } else { 1000 }).await;
        }
    }

    fn take(&self) -> Option<Warm> {
        let mut g = self.0.borrow_mut();
        while let Some(w) = g.ready.pop_front() {
            if w.open() {
                return Some(w);
            }
            w.close();
        }
        None
    }

    /// Whether a proxy is ready now.
    pub fn has_ready(&self) -> bool {
        self.0.borrow().ready.iter().any(Warm::open)
    }

    /// Resolves once a proxy is ready (or with the last rendezvous error after `timeout_ms`).
    pub async fn ready(&self, timeout_ms: f64) -> Result<(), String> {
        let t0 = now();
        loop {
            if self.0.borrow().ready.iter().any(Warm::open) {
                return Ok(());
            }
            if now() - t0 > timeout_ms {
                let e = self.0.borrow().last_error.clone();
                return Err(if e.is_empty() { "no Snowflake proxy yet".into() } else { e });
            }
            sleep_ms(100).await;
        }
    }
}

pub struct WebDialer {
    pub pool: WarmPool,
}

// SAFETY: single-threaded target (see web::rt); the pool is only touched on the page's thread.
unsafe impl Send for WebDialer {}
unsafe impl Sync for WebDialer {}

impl Dialer for WebDialer {
    fn dial(&self) -> std::io::Result<SnowflakeStream> {
        let mut seed = [0u8; 12];
        getrandom03::fill(&mut seed).map_err(|e| std::io::Error::other(e.to_string()))?;
        let link = Link::new(Session::new(seed[..8].try_into().expect("8"), u32::from_le_bytes(seed[8..].try_into().expect("4"))));
        wasm_bindgen_futures::spawn_local(run(link.clone(), self.pool.clone()));
        Ok(SnowflakeStream::new(link))
    }
}

fn now() -> f64 {
    js_sys::Date::now()
}

fn js_err(e: JsValue) -> String {
    e.as_string().or_else(|| js_sys::JSON::stringify(&e).ok().and_then(|s| s.as_string())).unwrap_or_else(|| "?".into())
}

/// Carrier task: one session over successive proxies until arti closes the stream.
async fn run(link: Link, pool: WarmPool) {
    let t0 = now();
    let mut waiting_since = now();
    loop {
        if link.lock().closed {
            return;
        }
        let Some(w) = pool.take() else {
            if now() - waiting_since > NO_PROXY_MS {
                let mut g = link.lock();
                g.failed = Some("snowflake: no proxy available".into());
                g.wake();
                return;
            }
            sleep_ms(100).await;
            continue;
        };
        pump(&link, &w, t0).await;
        w.close();
        link.lock().sess.on_channel_lost();
        waiting_since = now();
    }
}

/// Broker rendezvous and an open DataChannel to one proxy.
async fn rendezvous(p: &SnowflakeParams, fingerprint: &str) -> Result<Warm, String> {
    let cfg = RtcConfiguration::new();
    let servers = js_sys::Array::new();
    for url in &p.ice {
        let s = RtcIceServer::new();
        s.set_urls_str(url);
        servers.push(&s);
    }
    cfg.set_ice_servers(&servers);
    let pc = RtcPeerConnection::new_with_configuration(&cfg).map_err(js_err)?;
    let init = RtcDataChannelInit::new();
    init.set_ordered(true);
    let dc = pc.create_data_channel_with_data_channel_dict("snowflake", &init);
    dc.set_binary_type(RtcDataChannelType::Arraybuffer);
    let w = Warm { pc, dc };
    match negotiate(p, fingerprint, &w).await {
        Ok(()) => Ok(w),
        Err(e) => {
            w.close();
            Err(e)
        }
    }
}

async fn negotiate(p: &SnowflakeParams, fingerprint: &str, w: &Warm) -> Result<(), String> {
    let (pc, dc) = (&w.pc, &w.dc);
    // Offer with all candidates (no trickle over the broker).
    let offer = JsFuture::from(pc.create_offer()).await.map_err(js_err)?;
    let sdp = js_sys::Reflect::get(&offer, &"sdp".into()).ok().and_then(|v| v.as_string()).ok_or("no offer sdp")?;
    let desc = RtcSessionDescriptionInit::new(RtcSdpType::Offer);
    desc.set_sdp(&sdp);
    JsFuture::from(pc.set_local_description(&desc)).await.map_err(js_err)?;
    let start = now();
    while pc.ice_gathering_state() != RtcIceGatheringState::Complete && now() - start < GATHER_TIMEOUT_MS {
        sleep_ms(50).await;
    }
    let local = pc.local_description().ok_or("no local description")?.sdp();

    // Broker: `1.0\n` + {"offer": "<SDP JSON>", "nat": ..., "fingerprint": ...}.
    let offer_json = js_sys::JSON::stringify(&json_obj(&[("type", "offer"), ("sdp", &local)])).map_err(js_err)?.as_string().unwrap_or_default();
    let body = js_sys::JSON::stringify(&json_obj(&[("offer", &offer_json), ("nat", &p.nat), ("fingerprint", fingerprint)]))
        .map_err(js_err)?
        .as_string()
        .unwrap_or_default();
    let body = format!("1.0\n{body}");
    let text = post_offer(&p.brokers, &body).await?;
    let j = js_sys::JSON::parse(&text).map_err(|_| format!("broker said {}", text.chars().take(80).collect::<String>()))?;
    let answer = js_sys::Reflect::get(&j, &"answer".into()).ok().and_then(|v| v.as_string()).filter(|s| !s.is_empty());
    let Some(answer) = answer else {
        let err = js_sys::Reflect::get(&j, &"error".into()).ok().and_then(|v| v.as_string()).unwrap_or_default();
        return Err(format!("no proxy ({err})"));
    };
    let a = js_sys::JSON::parse(&answer).map_err(js_err)?;
    let asdp = js_sys::Reflect::get(&a, &"sdp".into()).ok().and_then(|v| v.as_string()).ok_or("bad answer")?;
    let desc = RtcSessionDescriptionInit::new(RtcSdpType::Answer);
    desc.set_sdp(&asdp);
    JsFuture::from(pc.set_remote_description(&desc)).await.map_err(js_err)?;
    let start = now();
    while dc.ready_state() != RtcDataChannelState::Open {
        if now() - start > OPEN_TIMEOUT_MS || dc.ready_state() == RtcDataChannelState::Closed {
            return Err("proxy DataChannel did not open".into());
        }
        sleep_ms(20).await;
    }
    tracing::info!("snowflake: proxy ready after {:.0} ms", now() - start);
    Ok(())
}

/// POSTs the offer to the first broker that answers; returns its reply.
async fn post_offer(brokers: &[String], body: &str) -> Result<String, String> {
    let window = web_sys::window().ok_or("no window")?;
    let mut last = String::from("no broker configured");
    for b in brokers {
        let req = RequestInit::new();
        req.set_method("POST");
        req.set_body(&JsValue::from_str(body));
        let url = format!("{}/client", b.trim_end_matches('/'));
        match JsFuture::from(window.fetch_with_str_and_init(&url, &req)).await {
            Ok(r) => {
                let resp: Response = r.unchecked_into();
                return Ok(JsFuture::from(resp.text().map_err(js_err)?).await.map_err(js_err)?.as_string().unwrap_or_default());
            }
            Err(e) => {
                last = js_err(e);
                tracing::info!("snowflake: broker {b} unreachable: {last}");
            }
        }
    }
    Err(last)
}

/// Carries the session over one proxy until it is lost or arti closes the stream.
async fn pump(link: &Link, w: &Warm, t0: f64) {
    let dc = &w.dc;
    let last_rx = Rc::new(Cell::new(now()));
    let rx_buf = Rc::new(RefCell::new(vec![0u8; 64 * 1024]));
    let (l2, lr, rb) = (link.clone(), last_rx.clone(), rx_buf.clone());
    let on_msg = Closure::<dyn FnMut(MessageEvent)>::new(move |e: MessageEvent| {
        let Ok(ab) = e.data().dyn_into::<ArrayBuffer>() else { return };
        let v = Uint8Array::new(&ab);
        let n = v.length() as usize;
        let mut b = rb.borrow_mut();
        if b.len() < n {
            b.resize(n, 0);
        }
        v.copy_to(&mut b[..n]);
        lr.set(now());
        let mut g = l2.lock();
        let _ = g.sess.on_data((now() - t0) as u32, &b[..n]);
        g.wake();
    });
    dc.set_onmessage(Some(on_msg.as_ref().unchecked_ref()));
    link.lock().sess.on_channel();
    // Since when our data has waited for an acknowledgement (None: nothing outstanding).
    let mut waiting_since: Option<f64> = None;
    loop {
        {
            let mut g = link.lock();
            if g.closed {
                break;
            }
            let r = g.sess.poll((now() - t0) as u32, &mut |m: &[u8]| {
                let _ = dc.send_with_u8_array(m);
            });
            g.wake();
            if let Err(e) = r {
                g.failed = Some(format!("snowflake: {e:?}"));
                break;
            }
            waiting_since = if g.sess.unacked() > 0 { waiting_since.or(Some(now())) } else { None };
        }
        // Silent while idle (keep-alives come every 10 s), or no reply to data we sent: the
        // stall is timed from the later of our send and the last byte received, not from the
        // last byte alone (an idle link's next keep-alive would look like a stall).
        let t = now();
        let silent = t - last_rx.get() > SILENT_MS;
        let stalled = waiting_since.is_some_and(|w| t - w.max(last_rx.get()) > STALL_MS);
        if dc.ready_state() != RtcDataChannelState::Open || silent || stalled {
            tracing::info!("snowflake: proxy lost, switching");
            break;
        }
        sleep_ms(ephem_snowflake::session::TICK_MS).await;
    }
    dc.set_onmessage(None);
    drop(on_msg);
}

fn json_obj(kv: &[(&str, &str)]) -> js_sys::Object {
    let o = js_sys::Object::new();
    for (k, v) in kv {
        let _ = js_sys::Reflect::set(&o, &(*k).into(), &(*v).into());
    }
    o
}
