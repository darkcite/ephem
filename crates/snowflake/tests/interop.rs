//! Interop with the reference Go snowflake server (v2.14.1: kcp-go v5.6.24, smux v1.5.56), E3 in docs/P2P-CHAT.md Appendix C.5.
//!
//! The server runs as a Tor server transport would (PT environment), with its ORPort pointed at
//! a local echo service. Our `Session` talks to it over WebSocket exactly as a Snowflake proxy
//! relays a DataChannel, pushes 10 MB through smux/KCP/encapsulation and checks the echo, and
//! switches to a new WebSocket ("proxy") in the middle of the transfer.
//!
//! Needs `checks/tor-lab/bin/sf-server` (built by `checks/tor-lab/lab.sh up`); skipped if absent.

use ephem_snowflake::Session;
use std::io::{ErrorKind, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};
use tungstenite::{Message, WebSocket};

struct Server(Child);

impl Drop for Server {
    fn drop(&mut self) {
        let _ = self.0.kill();
    }
}

fn free_port() -> u16 {
    TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap().port()
}

fn echo() -> u16 {
    let l = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = l.local_addr().unwrap().port();
    std::thread::spawn(move || {
        for c in l.incoming() {
            let mut c = c.unwrap();
            std::thread::spawn(move || {
                let mut r = c.try_clone().unwrap();
                let mut b = [0u8; 65536];
                loop {
                    match r.read(&mut b) {
                        Ok(0) | Err(_) => return,
                        Ok(n) => c.write_all(&b[..n]).unwrap(),
                    }
                }
            });
        }
    });
    port
}

fn start_server(bin: &str, or_port: u16) -> (Server, u16) {
    let port = free_port();
    let state = std::env::temp_dir().join(format!("ephem-sf-state-{port}"));
    let child = Command::new(bin)
        .args(["-disable-tls", "-unsafe-logging"])
        .env("TOR_PT_MANAGED_TRANSPORT_VER", "1")
        .env("TOR_PT_SERVER_TRANSPORTS", "snowflake")
        .env("TOR_PT_SERVER_BINDADDR", format!("snowflake-127.0.0.1:{port}"))
        .env("TOR_PT_ORPORT", format!("127.0.0.1:{or_port}"))
        .env("TOR_PT_STATE_LOCATION", state)
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let t0 = Instant::now();
    while TcpStream::connect(("127.0.0.1", port)).is_err() {
        assert!(t0.elapsed() < Duration::from_secs(10), "snowflake server did not start");
        std::thread::sleep(Duration::from_millis(50));
    }
    (Server(child), port)
}

fn ws(port: u16) -> WebSocket<TcpStream> {
    let tcp = TcpStream::connect(("127.0.0.1", port)).unwrap();
    tcp.set_nodelay(true).unwrap();
    let (mut w, _) = tungstenite::client(format!("ws://127.0.0.1:{port}/"), tcp).unwrap();
    w.get_mut().set_read_timeout(Some(Duration::from_millis(5))).unwrap();
    w.flush().unwrap();
    w
}

fn now_ms(t0: Instant) -> u32 {
    t0.elapsed().as_millis() as u32
}

#[test]
fn echo_through_go_server_with_proxy_switch() {
    let bin = concat!(env!("CARGO_MANIFEST_DIR"), "/../../checks/tor-lab/bin/sf-server");
    if !std::path::Path::new(bin).exists() {
        eprintln!("skipped: {bin} missing (run checks/tor-lab/lab.sh up)");
        return;
    }
    let (_srv, port) = start_server(bin, echo());
    let t0 = Instant::now();
    let mut s = Session::new(*b"ephemtst", 0x1234_5678);
    let mut w = ws(port);
    s.on_channel();

    const TOTAL: usize = 10 * 1024 * 1024;
    let src: Vec<u8> = (0..TOTAL).map(|i| (i * 131 % 251) as u8).collect();
    let (mut sent, mut got) = (0usize, Vec::with_capacity(TOTAL));
    let mut buf = vec![0u8; 64 * 1024];
    let mut switched = false;
    while got.len() < TOTAL {
        assert!(t0.elapsed() < Duration::from_secs(120), "stalled: sent {sent}, echoed {}", got.len());
        if !switched && got.len() > TOTAL / 3 {
            // The proxy goes away: a new "DataChannel" continues the same session.
            let _ = w.close(None);
            s.on_channel_lost();
            w = ws(port);
            s.on_channel();
            switched = true;
        }
        if sent < TOTAL {
            sent += s.write(&src[sent..(sent + 256 * 1024).min(TOTAL)]);
        }
        let now = now_ms(t0);
        s.poll(now, &mut |m| w.send(Message::binary(m.to_vec())).unwrap()).unwrap();
        loop {
            match w.read() {
                Ok(Message::Binary(b)) => s.on_data(now_ms(t0), &b).unwrap(),
                Ok(_) => {}
                Err(tungstenite::Error::Io(e)) if e.kind() == ErrorKind::WouldBlock || e.kind() == ErrorKind::TimedOut => break,
                Err(e) => panic!("websocket: {e}"),
            }
        }
        loop {
            let n = s.read(&mut buf);
            if n == 0 {
                break;
            }
            got.extend_from_slice(&buf[..n]);
        }
    }
    assert!(got == src, "echo differs");
    assert!(switched);
    eprintln!("10 MB echoed through the Go snowflake server in {:?} (one proxy switch)", t0.elapsed());
}
