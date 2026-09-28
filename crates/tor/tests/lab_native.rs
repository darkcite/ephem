//! E4, native half: arti bootstraps over *our* Snowflake transport (crates/snowflake) and TLS
//! (crates/tor/src/tls.rs) through the offline lab's Snowflake bridge, then reaches the lab's
//! onion service. tokio only provides tasks and timers here; every byte to Tor goes through
//! `BridgeNet` (the transport guard) and our Turbotunnel/KCP/smux session, which talks to the
//! lab's Go snowflake server over WebSocket exactly as a Snowflake proxy does.
//!
//! Needs a running lab (`checks/tor-lab/lab.sh up`); skipped otherwise.

use arti_client::TorClient;
use ephem_snowflake::Session;
use ephem_tor::net::{BridgeNet, Dialer};
use ephem_tor::stream::{Link, SnowflakeStream};
use ephem_tor::tls::TorTls;
use futures::{AsyncReadExt, AsyncWriteExt};
use std::collections::HashMap;
use std::io::ErrorKind;
use std::net::TcpStream;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tor_rtcompat::{CompoundRuntime, RealCoarseTimeProvider};
use tungstenite::Message;

fn lab_env() -> Option<HashMap<String, String>> {
    let text = std::fs::read_to_string("/tmp/ephlab/lab.env").ok()?;
    Some(text.lines().filter_map(|l| l.split_once('=')).map(|(k, v)| (k.to_owned(), v.to_owned())).collect())
}

/// Carrier for native tests: one WebSocket to the lab's snowflake server per dial.
struct WsDialer {
    port: u16,
}

impl Dialer for WsDialer {
    fn dial(&self) -> std::io::Result<SnowflakeStream> {
        let mut seed = [0u8; 12];
        getrandom03::fill(&mut seed).unwrap();
        let link = Link::new(Session::new(seed[..8].try_into().unwrap(), u32::from_le_bytes(seed[8..].try_into().unwrap())));
        let port = self.port;
        let l = link.clone();
        std::thread::spawn(move || {
            let tcp = TcpStream::connect(("127.0.0.1", port)).unwrap();
            let (mut ws, _) = tungstenite::client(format!("ws://127.0.0.1:{port}/"), tcp).unwrap();
            ws.get_mut().set_read_timeout(Some(Duration::from_millis(2))).unwrap();
            l.lock().sess.on_channel();
            let t0 = Instant::now();
            loop {
                let now = t0.elapsed().as_millis() as u32;
                {
                    let mut g = l.lock();
                    if g.closed {
                        return;
                    }
                    let _ = g.sess.poll(now, &mut |m| ws.send(Message::binary(m.to_vec())).unwrap());
                    g.wake();
                }
                loop {
                    match ws.read() {
                        Ok(Message::Binary(b)) => {
                            let mut g = l.lock();
                            let _ = g.sess.on_data(t0.elapsed().as_millis() as u32, &b);
                            g.wake();
                        }
                        Ok(_) => {}
                        Err(tungstenite::Error::Io(e)) if matches!(e.kind(), ErrorKind::WouldBlock | ErrorKind::TimedOut) => break,
                        Err(e) => {
                            let mut g = l.lock();
                            g.failed = Some(format!("websocket: {e}"));
                            g.wake();
                            return;
                        }
                    }
                }
            }
        });
        Ok(SnowflakeStream::new(link))
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn bootstrap_and_reach_onion_through_our_snowflake() {
    let Some(env) = lab_env() else {
        eprintln!("skipped: no lab (checks/tor-lab/lab.sh up)");
        return;
    };
    let network = std::fs::read_to_string(format!("{}/arti-net.toml", env["LAB"])).unwrap();
    let root = std::env::temp_dir().join(format!("ephem-arti-{}", std::process::id()));
    let cfg = ephem_tor::config::build(&env["BRIDGE_FP"], &network, root.to_str().unwrap()).unwrap();

    let _ = tracing_subscriber::fmt().with_env_filter(tracing_subscriber::EnvFilter::from_default_env()).with_test_writer().try_init();
    let _ = futures_rustls::rustls::crypto::ring::default_provider().install_default();
    let tokio_rt = tor_rtcompat::tokio::TokioRustlsRuntime::current().unwrap();
    let net = BridgeNet::new(Arc::new(WsDialer { port: env["BRIDGE_PTPORT"].parse().unwrap() }));
    let rt = CompoundRuntime::new(tokio_rt.clone(), tokio_rt.clone(), RealCoarseTimeProvider::new(), net.clone(), net.clone(), TorTls::default(), net);

    let t0 = Instant::now();
    let client = TorClient::with_runtime(rt).config(cfg).create_bootstrapped().await.unwrap();
    eprintln!("bootstrapped over our Snowflake transport in {:?}", t0.elapsed());

    let t1 = Instant::now();
    // The bridge descriptor (needed to extend circuits through the bridge) may still be on
    // its way right after bootstrap: retry, as the app does.
    let mut s = loop {
        match client.connect((env["ONION"].as_str(), 5858)).await {
            Ok(s) => break s,
            Err(e) if t1.elapsed() < Duration::from_secs(90) => {
                eprintln!("onion not reachable yet ({e}), retrying");
                tokio::time::sleep(Duration::from_secs(2)).await;
            }
            Err(e) => panic!("onion: {e}"),
        }
    };
    s.write_all(b"ephem over tor").await.unwrap();
    s.flush().await.unwrap();
    let mut buf = [0u8; 14];
    s.read_exact(&mut buf).await.unwrap();
    assert_eq!(&buf, b"ephem over tor");
    eprintln!("onion echo in {:?}", t1.elapsed());
    drop(client);
    let _ = std::fs::remove_dir_all(&root);
}
