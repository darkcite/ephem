// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
// Copyright 2026 Anton (darkcite)
//! `tor_bg.wasm`: arti in the page, reaching Tor only through Snowflake.
//!
//! [`Tor`] is the Rust handle (used by the Ephem app build, `crates/wasm` feature `tor`):
//! bootstrap, dial `onion:port`, host an onion service with a given key, accept its streams.
//! With the `js-api` feature, [`TorNet`] / [`TorStream`] expose the same to JS (lab page).
//! Setup path throughout: allocations are fine here.

mod carrier;
mod rt;

use crate::config;
use crate::net::{BridgeNet, Dialer};
use crate::tls::TorTls;
use arti_client::config::onion_service::OnionServiceConfigBuilder;
use arti_client::{StreamPrefs, TorClient};
use futures::StreamExt;
use std::cell::RefCell;
use std::collections::VecDeque;
use tor_hscrypto::pk::{HsId, HsIdKeypair};
use tor_hsservice::RunningOnionService;
use tor_llcrypto::pk::ed25519;
use carrier::{SnowflakeParams, WarmPool, WebDialer};
use rt::WebTask;
use std::rc::Rc;
use std::sync::Arc;
use tor_rtcompat::{CompoundRuntime, RealCoarseTimeProvider};

pub type Runtime = CompoundRuntime<WebTask, WebTask, RealCoarseTimeProvider, BridgeNet, BridgeNet, TorTls, BridgeNet>;

pub use arti_client::DataStream;
pub use carrier::SnowflakeParams as Snowflake;
pub use rt::sleep_ms;
pub use tor_proto::client::stream::{DataReader, DataWriter};

/// A comma-separated list (broker URLs, STUN URLs), empty entries dropped.
/// The page's Tor client, shared by its chats and its channels (one client per page, Appendix
/// F.3.3); empty until started.
pub type TorSlot = std::rc::Rc<std::cell::RefCell<Option<std::rc::Rc<Tor>>>>;

pub fn list(s: &str) -> Vec<String> {
    s.split(',').map(str::trim).filter(|x| !x.is_empty()).map(str::to_owned).collect()
}

/// `"<56 base32 chars>.onion"` of an onion service key.
pub fn onion_address(pk: &[u8; 32]) -> String {
    safelog::DisplayRedacted::display_unredacted(&HsId::from(*pk)).to_string()
}

/// Logs to the browser console (no timestamps: `SystemTime::now` is unavailable in wasm).
#[cfg_attr(feature = "js-api", wasm_bindgen::prelude::wasm_bindgen)]
pub fn tor_log(level: &str) {
    struct Console;
    impl std::io::Write for Console {
        fn write(&mut self, b: &[u8]) -> std::io::Result<usize> {
            web_sys::console::log_1(&String::from_utf8_lossy(b).trim_end().into());
            Ok(b.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }
    let level = level.parse().unwrap_or(tracing::Level::INFO);
    let _ = tracing_subscriber::fmt().with_max_level(level).without_time().with_ansi(false).with_writer(|| Console).try_init();
}

/// arti in the page (single-threaded; share it with `Rc`).
pub struct Tor {
    client: Arc<TorClient<Runtime>>,
    /// One pool of warm Snowflake proxies per bridge.
    pools: Vec<WarmPool>,
    /// The chat's onion service (replaced when the identity changes).
    chat: RefCell<Option<Rc<Service>>>,
}

impl Tor {
    /// `network_toml`: empty for the real Tor network; the lab passes its private network.
    /// `cache`: a directory snapshot of [`Self::cache`] from an earlier session (warm start),
    /// or empty.
    pub fn new(sf: SnowflakeParams, network_toml: &str, cache: &[u8]) -> Result<Tor, String> {
        if !cache.is_empty() && !tor_dirmgr::cache_import(cache) {
            tracing::info!("tor: directory snapshot unreadable, starting cold");
        }
        let fps = sf.fingerprints.clone();
        let pools: Vec<WarmPool> = fps.iter().map(|fp| WarmPool::start(sf.clone(), fp.clone())).collect();
        let net = BridgeNet::new(pools.iter().map(|p| Arc::new(WebDialer { pool: p.clone() }) as Arc<dyn Dialer>).collect());
        let rt: Runtime = CompoundRuntime::new(WebTask::default(), WebTask::default(), RealCoarseTimeProvider::new(), net.clone(), net.clone(), TorTls::default(), net);
        let cfg = config::build(&fps, network_toml, "/ephem")?;
        let client = TorClient::with_runtime(rt).config(cfg).create_unbootstrapped().map_err(|e| e.to_string())?;
        Ok(Tor { client, pools, chat: RefCell::default() })
    }

    /// Directory ready (the bridge descriptor may still follow; connects retry until it is
    /// there). Waits first for Snowflake proxies (every bridge's for a few seconds, then any),
    /// so arti's first bridge connection does not time out (which would mark the bridge down
    /// for minutes).
    pub async fn bootstrap(&self) -> Result<(), String> {
        let t0 = js_sys::Date::now();
        let mut first_err = None;
        for p in &self.pools {
            let left = (15_000.0 - (js_sys::Date::now() - t0)).max(0.0);
            if let Err(e) = p.ready(left).await {
                first_err.get_or_insert(e);
            }
        }
        if first_err.is_some() {
            let mut any = false;
            while !any && js_sys::Date::now() - t0 < 90_000.0 {
                any = self.pools.iter().any(WarmPool::has_ready);
                if !any {
                    sleep_ms(200).await;
                }
            }
            if !any {
                return Err(first_err.unwrap_or_default());
            }
        }
        self.client.bootstrap().await.map_err(|e| e.to_string())
    }

    /// The directory as a gzip snapshot for the next session's warm start (public data:
    /// consensus, authority certificates, microdescriptors), or `None` before one was
    /// downloaded.
    pub fn cache(&self) -> Option<Vec<u8>> {
        tor_dirmgr::cache_export()
    }

    /// Bootstrap progress for the UI, e.g. "45%: connecting successfully; …".
    pub fn status(&self) -> String {
        self.client.bootstrap_status().to_string()
    }

    /// Whether circuits can be built now.
    pub fn ready(&self) -> bool {
        self.client.bootstrap_status().ready_for_traffic()
    }

    /// A stream to `host:port` (an onion address in Ephem). `fresh`: in a new isolation group,
    /// so nothing cached for earlier attempts is reused. arti keeps an onion service's
    /// descriptor until it expires (hours) and refetches it only after an introduction NACK;
    /// a descriptor whose intro points silently stopped working (the service was re-hosted,
    /// e.g. after a sign-in) would otherwise fail every retry.
    pub async fn connect(&self, host: &str, port: u16, fresh: bool) -> Result<DataStream, String> {
        let mut prefs = StreamPrefs::new();
        if fresh {
            prefs.new_isolation_group();
        }
        self.client.connect_with_prefs((host, port), &prefs).await.map_err(|e| e.to_string())
    }

    /// Launches an onion service whose identity is the Ed25519 key `secret` (Ephem derives it
    /// from the identity seed, §7.1, or a channel's, §D.3). Incoming streams are queued on the
    /// returned [`Service`]; the service stays up while that value lives. Several services can
    /// run side by side (C-P2); nicknames must differ.
    ///
    /// Every stage is bounded (security audit H-2; the onion addresses of channels are public and
    /// the chat onion is known to every contact): at most [`REND_IN_FLIGHT`] rendezvous circuits
    /// being built and [`LIVE_CIRCUITS`] open at once (the rest are dropped unanswered), at most
    /// [`CIRCUIT_STREAMS`] new streams per circuit in [`CIRCUIT_WINDOW_MS`] (more closes the
    /// circuit), at most [`MAX_CIRCUIT_STREAMS`] open per circuit (arti closes it), and a queue of
    /// [`QUEUE`] accepted streams (more are refused with END DONE). The introduction points are
    /// asked to pass at most [`INTRO_RATE`]/s (burst [`INTRO_BURST`]); that protects the tab, not
    /// availability, under a flood.
    pub fn launch(&self, nickname: &str, secret: &[u8; 32]) -> Result<Service, String> {
        let kp = ed25519::Keypair::from_bytes(secret);
        let hsid: HsId = tor_hscrypto::pk::HsIdKey::from(*ed25519::ExpandedKeypair::from(&kp).public()).id();
        let cfg = OnionServiceConfigBuilder::default()
            .nickname(nickname.parse().map_err(|e: tor_hsservice::InvalidNickname| e.to_string())?)
            .max_concurrent_streams_per_circuit(MAX_CIRCUIT_STREAMS)
            .rate_limit_at_intro(Some(tor_hsservice::config::TokenBucketConfig::new(INTRO_RATE, INTRO_BURST)))
            .build()
            .map_err(|e| e.to_string())?;
        let (svc, rend) = self
            .client
            .launch_onion_service_with_hsid(cfg, HsIdKeypair::from(ed25519::ExpandedKeypair::from(&kp)))
            .map_err(|e| e.to_string())?
            .ok_or("onion services are disabled")?;
        let incoming: Rc<RefCell<VecDeque<DataStream>>> = Rc::new(RefCell::new(VecDeque::with_capacity(QUEUE)));
        wasm_bindgen_futures::spawn_local(accept_loop(rend, incoming.clone()));
        Ok(Service { onion: safelog::DisplayRedacted::display_unredacted(&hsid).to_string(), incoming, _running: svc })
    }

    /// The chat's onion service (§28.4): like [`Self::launch`], but it replaces the service
    /// hosted before (the tab's identity changed). Returns `"<56 chars>.onion"`; take its
    /// streams with [`Self::accept`].
    pub fn host(&self, nickname: &str, secret: &[u8; 32]) -> Result<String, String> {
        let s = self.launch(nickname, secret)?;
        let onion = s.onion.clone();
        // Dropping the previous service shuts it down (and ends its stream of requests).
        *self.chat.borrow_mut() = Some(Rc::new(s));
        Ok(onion)
    }

    /// Whether clients can reach the chat's onion service yet (`Service::reach`); empty before
    /// it is hosted.
    pub fn chat_reach(&self) -> &'static str {
        self.chat.borrow().as_ref().map_or("", |s| s.reach())
    }

    /// The next incoming stream of the chat's onion service. The current service is looked up
    /// on every poll: after [`Self::host`] replaced it, streams of the new one are taken (and
    /// no reference keeps the old one alive).
    pub async fn accept(&self) -> DataStream {
        loop {
            let next = self.chat.borrow().as_ref().and_then(|s| s.try_accept());
            if let Some(s) = next {
                return s;
            }
            rt::sleep_ms(50).await;
        }
    }
}

/// A running onion service and its accepted, not yet taken, streams.
/// Rendezvous circuits being built at once, per service; more requests are dropped (the client
/// sees a timeout and may retry).
pub const REND_IN_FLIGHT: u32 = 8;
/// Open rendezvous circuits per service (a reader holds one per refresh, a chat one).
pub const LIVE_CIRCUITS: u32 = 32;
/// New streams one circuit may open per window before it is closed (a channel refresh needs 2–3).
pub const CIRCUIT_STREAMS: u32 = 8;
pub const CIRCUIT_WINDOW_MS: f64 = 10_000.0;
/// Streams open at once on one circuit (arti's `max_concurrent_streams_per_circuit`).
pub const MAX_CIRCUIT_STREAMS: u32 = 8;
/// Accepted streams waiting for the app.
pub const QUEUE: usize = 32;
/// Introductions per second (and burst) the introduction points pass on (`DOS_PARAMS`).
pub const INTRO_RATE: u32 = 10;
pub const INTRO_BURST: u32 = 50;

/// The bounded replacement for `tor_hsservice::handle_rend_requests` (which builds every
/// rendezvous circuit at once and accepts every stream): see [`Tor::launch`].
async fn accept_loop(mut rend: impl futures::Stream<Item = tor_hsservice::RendRequest> + Unpin, queue: Rc<RefCell<VecDeque<DataStream>>>) {
    let building = Rc::new(std::cell::Cell::new(0u32));
    let live = Rc::new(std::cell::Cell::new(0u32));
    while let Some(req) = rend.next().await {
        if building.get() >= REND_IN_FLIGHT || live.get() >= LIVE_CIRCUITS {
            let _ = req.reject().await;
            tracing::info!("onion: a rendezvous was dropped (busy)");
            continue;
        }
        building.set(building.get() + 1);
        let (building, live, queue) = (building.clone(), live.clone(), queue.clone());
        wasm_bindgen_futures::spawn_local(async move {
            let streams = req.accept().await;
            building.set(building.get() - 1);
            let mut streams = match streams {
                Ok(s) => s,
                Err(e) => return tracing::info!("onion: rendezvous failed: {e}"),
            };
            live.set(live.get() + 1);
            let (mut window_at, mut opened) = (js_sys::Date::now(), 0u32);
            while let Some(sr) = streams.next().await {
                let now = js_sys::Date::now();
                if now - window_at >= CIRCUIT_WINDOW_MS {
                    (window_at, opened) = (now, 0);
                }
                opened += 1;
                if opened > CIRCUIT_STREAMS {
                    tracing::info!("onion: a circuit opened too many streams; closed");
                    let _ = sr.shutdown_circuit();
                    break;
                }
                if queue.borrow().len() >= QUEUE {
                    let _ = sr.reject(tor_cell::relaycell::msg::End::new_with_reason(tor_cell::relaycell::msg::EndReason::DONE)).await;
                    continue;
                }
                match sr.accept(tor_cell::relaycell::msg::Connected::new_empty()).await {
                    Ok(s) => queue.borrow_mut().push_back(s),
                    Err(e) => tracing::info!("onion: stream not accepted: {e}"),
                }
            }
            live.set(live.get() - 1);
        });
    }
}

pub struct Service {
    onion: String,
    incoming: Rc<RefCell<VecDeque<DataStream>>>,
    _running: Arc<RunningOnionService>,
}

impl Service {
    /// `"<56 chars>.onion"`.
    pub fn onion(&self) -> &str {
        &self.onion
    }

    /// The next incoming stream.
    pub async fn accept(&self) -> DataStream {
        loop {
            if let Some(s) = self.try_accept() {
                return s;
            }
            rt::sleep_ms(50).await;
        }
    }

    /// An incoming stream, if one is waiting.
    pub fn try_accept(&self) -> Option<DataStream> {
        self.incoming.borrow_mut().pop_front()
    }

    /// Whether clients can reach the service yet, from arti's status: `"publishing"` (building
    /// introduction points, publishing its descriptor: usually under a minute after launch),
    /// `"reachable"`, `"degraded"` (reachable, with problems), `"unreachable"` or `"down"`.
    pub fn reach(&self) -> &'static str {
        use tor_hsservice::status::State;
        match self._running.status().state() {
            State::Running => "reachable",
            State::DegradedReachable => "degraded",
            State::Bootstrapping | State::Recovering => "publishing",
            State::DegradedUnreachable | State::Broken => "unreachable",
            _ => "down",
        }
    }
}

#[cfg(feature = "js-api")]
pub use js::{TorNet, TorStream};

/// The same API for JS (the lab page).
#[cfg(feature = "js-api")]
mod js {
    use super::*;
    use futures::{AsyncReadExt, AsyncWriteExt};
    use wasm_bindgen::prelude::*;

    fn err(e: impl std::fmt::Display) -> JsValue {
        JsValue::from_str(&e.to_string())
    }

    #[wasm_bindgen]
    pub struct TorNet {
        tor: Rc<Tor>,
    }

    #[wasm_bindgen]
    impl TorNet {
        /// `broker`: comma-separated broker URLs (tried in order). `ice`: comma-separated
        /// `stun:` URLs for the proxy connections. `nat`: the broker
        /// hint (empty = "unknown"). `network_toml`: empty for the real Tor network.
        #[wasm_bindgen(constructor)]
        pub fn new(broker: &str, bridge_fp: &str, ice: &str, nat: &str, network_toml: &str) -> Result<TorNet, JsValue> {
            let sf = SnowflakeParams {
                brokers: list(broker),
                fingerprints: list(bridge_fp),
                ice: list(ice),
                nat: if nat.is_empty() { "unknown".to_owned() } else { nat.to_owned() },
            };
            Ok(TorNet { tor: Rc::new(Tor::new(sf, network_toml, &[]).map_err(err)?) })
        }

        pub fn bootstrap(&self) -> js_sys::Promise {
            let t = self.tor.clone();
            wasm_bindgen_futures::future_to_promise(async move {
                t.bootstrap().await.map_err(err)?;
                Ok(JsValue::UNDEFINED)
            })
        }

        pub fn status(&self) -> String {
            self.tor.status()
        }

        pub fn connect(&self, host: String, port: u16) -> js_sys::Promise {
            let t = self.tor.clone();
            wasm_bindgen_futures::future_to_promise(async move { Ok(TorStream::new(t.connect(&host, port, false).await.map_err(err)?).into()) })
        }

        pub fn host(&self, nickname: &str, secret: &[u8]) -> Result<String, JsValue> {
            let sk: [u8; 32] = secret.try_into().map_err(|_| err("onion key must be 32 bytes"))?;
            self.tor.host(nickname, &sk).map_err(err)
        }

        pub fn accept(&self) -> js_sys::Promise {
            let t = self.tor.clone();
            wasm_bindgen_futures::future_to_promise(async move { Ok(TorStream::new(t.accept().await).into()) })
        }
    }

    /// A Tor stream (to or from an onion service), read and written independently.
    #[wasm_bindgen]
    pub struct TorStream {
        reader: Rc<futures::lock::Mutex<DataReader>>,
        writer: Rc<futures::lock::Mutex<DataWriter>>,
    }

    impl TorStream {
        fn new(s: DataStream) -> Self {
            let (r, w) = s.split();
            Self { reader: Rc::new(futures::lock::Mutex::new(r)), writer: Rc::new(futures::lock::Mutex::new(w)) }
        }
    }

    #[wasm_bindgen]
    impl TorStream {
        pub fn write(&self, data: Vec<u8>) -> js_sys::Promise {
            let s = self.writer.clone();
            wasm_bindgen_futures::future_to_promise(async move {
                let mut g = s.lock().await;
                g.write_all(&data).await.map_err(err)?;
                g.flush().await.map_err(err)?;
                Ok(JsValue::UNDEFINED)
            })
        }

        /// Up to `max` bytes (empty = end of stream).
        pub fn read(&self, max: u32) -> js_sys::Promise {
            let s = self.reader.clone();
            wasm_bindgen_futures::future_to_promise(async move {
                let mut buf = vec![0u8; max as usize];
                let n = s.lock().await.read(&mut buf).await.map_err(err)?;
                buf.truncate(n);
                Ok(js_sys::Uint8Array::from(&buf[..]).into())
            })
        }
    }
}
