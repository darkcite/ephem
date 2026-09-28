//! `tor_bg.wasm`: arti in the page, reaching Tor only through Snowflake.
//!
//! JS API (setup path; allocations are fine here): [`TorNet::new`] with the Snowflake broker,
//! bridge fingerprint and STUN list (and, in the offline lab, the private network's TOML);
//! [`TorNet::bootstrap`]; [`TorNet::connect`] to `onion:port`; [`TorNet::host`] an onion
//! service with a given key and [`TorNet::accept`] its incoming streams. A [`TorStream`] reads
//! and writes independently (a pending read never blocks a write).

mod carrier;
mod rt;

use crate::config;
use crate::net::BridgeNet;
use crate::tls::TorTls;
use arti_client::config::onion_service::OnionServiceConfigBuilder;
use arti_client::{DataStream, TorClient};
use futures::StreamExt;
use std::cell::RefCell;
use std::collections::VecDeque;
use tor_hscrypto::pk::{HsId, HsIdKeypair};
use tor_hsservice::RunningOnionService;
use tor_llcrypto::pk::ed25519;
use tor_proto::client::stream::{DataReader, DataWriter};
use carrier::{SnowflakeParams, WarmPool, WebDialer};
use futures::{AsyncReadExt, AsyncWriteExt};
use rt::WebTask;
use std::rc::Rc;
use std::sync::Arc;
use tor_rtcompat::{CompoundRuntime, RealCoarseTimeProvider};
use wasm_bindgen::prelude::*;

pub type Runtime = CompoundRuntime<WebTask, WebTask, RealCoarseTimeProvider, BridgeNet, BridgeNet, TorTls, BridgeNet>;

fn err(e: impl std::fmt::Display) -> JsValue {
    JsValue::from_str(&e.to_string())
}

/// Logs to the browser console (no timestamps: `SystemTime::now` is unavailable in wasm).
#[wasm_bindgen]
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

#[wasm_bindgen]
pub struct TorNet {
    client: Arc<TorClient<Runtime>>,
    pool: WarmPool,
    /// Running onion services (kept alive) and their accepted, not yet taken, streams.
    services: Rc<RefCell<Vec<Arc<RunningOnionService>>>>,
    incoming: Rc<RefCell<VecDeque<DataStream>>>,
}

#[wasm_bindgen]
impl TorNet {
    /// `ice`: comma-separated `stun:` URLs for the proxy connections. `nat`: the broker hint
    /// (empty = "unknown"). `network_toml`: empty for the real Tor network; the lab passes its
    /// private network.
    #[wasm_bindgen(constructor)]
    pub fn new(broker: &str, bridge_fp: &str, ice: &str, nat: &str, network_toml: &str) -> Result<TorNet, JsValue> {
        let params = SnowflakeParams {
            broker: broker.to_owned(),
            fingerprint: bridge_fp.to_owned(),
            ice: ice.split(',').map(str::trim).filter(|s| !s.is_empty()).map(str::to_owned).collect(),
            nat: if nat.is_empty() { "unknown".to_owned() } else { nat.to_owned() },
        };
        let pool = WarmPool::start(params);
        let net = BridgeNet::new(Arc::new(WebDialer { pool: pool.clone() }));
        let rt: Runtime = CompoundRuntime::new(WebTask::default(), WebTask::default(), RealCoarseTimeProvider::new(), net.clone(), net.clone(), TorTls::default(), net);
        let cfg = config::build(bridge_fp, network_toml, "/ephem").map_err(err)?;
        let client = TorClient::with_runtime(rt).config(cfg).create_unbootstrapped().map_err(err)?;
        Ok(TorNet { client, pool, services: Rc::default(), incoming: Rc::default() })
    }

    /// Resolves when the directory is ready (the bridge descriptor may still follow; connects
    /// retry until it is there). Waits first for a Snowflake proxy, so arti's first bridge
    /// connection does not time out (which would mark the bridge down for minutes).
    pub fn bootstrap(&self) -> js_sys::Promise {
        let (c, pool) = (self.client.clone(), self.pool.clone());
        wasm_bindgen_futures::future_to_promise(async move {
            pool.ready(90_000.0).await.map_err(err)?;
            c.bootstrap().await.map_err(err)?;
            Ok(JsValue::UNDEFINED)
        })
    }

    /// Bootstrap progress for the UI, e.g. "45%: connecting successfully; …".
    pub fn status(&self) -> String {
        self.client.bootstrap_status().to_string()
    }

    /// Opens a stream to `host:port` (an onion address in Ephem).
    pub fn connect(&self, host: String, port: u16) -> js_sys::Promise {
        let c = self.client.clone();
        wasm_bindgen_futures::future_to_promise(async move {
            let s = c.connect((host.as_str(), port)).await.map_err(err)?;
            Ok(TorStream::new(s).into())
        })
    }

    /// Hosts an onion service whose identity is the Ed25519 key `secret` (32 bytes; Ephem
    /// derives it from the identity seed, §7.1, so a saved identity keeps its address).
    /// Returns `"<56 chars>.onion"`. Every stream to any port is accepted; take them with
    /// [`Self::accept`]. The service stays up while this `TorNet` lives.
    pub fn host(&self, nickname: &str, secret: &[u8]) -> Result<String, JsValue> {
        let sk: [u8; 32] = secret.try_into().map_err(|_| err("onion key must be 32 bytes"))?;
        let kp = ed25519::Keypair::from_bytes(&sk);
        let hsid: HsId = tor_hscrypto::pk::HsIdKey::from(ed25519::ExpandedKeypair::from(&kp).public().clone()).id();
        let cfg = OnionServiceConfigBuilder::default().nickname(nickname.parse().map_err(err)?).build().map_err(err)?;
        let (svc, rend) = self
            .client
            .launch_onion_service_with_hsid(cfg, HsIdKeypair::from(ed25519::ExpandedKeypair::from(&kp)))
            .map_err(err)?
            .ok_or_else(|| err("onion services are disabled"))?;
        self.services.borrow_mut().push(svc);
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
    pub fn accept(&self) -> js_sys::Promise {
        let incoming = self.incoming.clone();
        wasm_bindgen_futures::future_to_promise(async move {
            loop {
                if let Some(s) = incoming.borrow_mut().pop_front() {
                    return Ok(TorStream::new(s).into());
                }
                rt::sleep_ms(50).await;
            }
        })
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
