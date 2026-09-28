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
    /// The running onion service (kept alive) and its accepted, not yet taken, streams.
    services: RefCell<Vec<Arc<RunningOnionService>>>,
    incoming: Rc<RefCell<VecDeque<DataStream>>>,
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
        Ok(Tor { client, pools, services: RefCell::default(), incoming: Rc::default() })
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

    /// Hosts an onion service whose identity is the Ed25519 key `secret` (Ephem derives it from
    /// the identity seed, §7.1). Returns `"<56 chars>.onion"`. Every stream to any port is
    /// accepted; take them with [`Self::accept`]. It replaces the service hosted before (the
    /// tab's identity changed); `nickname` must differ from that one's.
    pub fn host(&self, nickname: &str, secret: &[u8; 32]) -> Result<String, String> {
        let kp = ed25519::Keypair::from_bytes(secret);
        let hsid: HsId = tor_hscrypto::pk::HsIdKey::from(*ed25519::ExpandedKeypair::from(&kp).public()).id();
        let cfg = OnionServiceConfigBuilder::default()
            .nickname(nickname.parse().map_err(|e: tor_hsservice::InvalidNickname| e.to_string())?)
            .build()
            .map_err(|e| e.to_string())?;
        let (svc, rend) = self
            .client
            .launch_onion_service_with_hsid(cfg, HsIdKeypair::from(ed25519::ExpandedKeypair::from(&kp)))
            .map_err(|e| e.to_string())?
            .ok_or("onion services are disabled")?;
        // Dropping the previous service shuts it down (and ends its stream of requests).
        *self.services.borrow_mut() = vec![svc];
        let incoming = self.incoming.clone();
        wasm_bindgen_futures::spawn_local(async move {
            let mut streams = tor_hsservice::handle_rend_requests(rend);
            while let Some(req) = streams.next().await {
                match req.accept(tor_cell::relaycell::msg::Connected::new_empty()).await {
                    Ok(s) => incoming.borrow_mut().push_back(s),
                    Err(e) => tracing::info!("onion: stream not accepted: {e}"),
                }
            }
        });
        Ok(safelog::DisplayRedacted::display_unredacted(&hsid).to_string())
    }

    /// The next incoming stream of the hosted onion services.
    pub async fn accept(&self) -> DataStream {
        loop {
            if let Some(s) = self.incoming.borrow_mut().pop_front() {
                return s;
            }
            rt::sleep_ms(50).await;
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
