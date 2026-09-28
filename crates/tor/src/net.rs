//! The only "network" the Tor code has: the Snowflake bridge (§28.5 transport guard).
//!
//! arti is configured with the Snowflake bridges at placeholder addresses ([`BRIDGE_ADDRS`], as
//! in Tor Browser's Snowflake bridge lines). Its "TCP connect" to such an address returns a new
//! [`SnowflakeStream`] toward that bridge; every other address, every listener, every Unix
//! socket and all UDP are refused. So nothing in the Tor code can reach any host except
//! through Snowflake.

use crate::stream::SnowflakeStream;
use async_trait::async_trait;
use futures::stream;
use std::io::{self, Result as IoResult};
use std::net::SocketAddr;
use std::sync::Arc;
use tor_general_addr::unix;
use tor_rtcompat::{NetStreamListener, NetStreamProvider, UdpProvider, UdpSocket};

/// The Snowflake bridges' placeholder addresses (TEST-NET-1, never routed): bridge `i` is
/// reached at `BRIDGE_ADDRS[i]`.
pub const BRIDGE_ADDRS: [SocketAddr; 2] = [
    SocketAddr::V4(std::net::SocketAddrV4::new(std::net::Ipv4Addr::new(192, 0, 2, 3), 80)),
    SocketAddr::V4(std::net::SocketAddrV4::new(std::net::Ipv4Addr::new(192, 0, 2, 4), 80)),
];

fn refused(what: &str) -> io::Error {
    io::Error::new(io::ErrorKind::PermissionDenied, format!("Tor mode: {what} is not allowed (only the Snowflake bridge)"))
}

/// Opens a new Snowflake connection (starts the rendezvous; the stream becomes usable when a
/// proxy's DataChannel is up).
pub trait Dialer: Send + Sync + 'static {
    fn dial(&self) -> IoResult<SnowflakeStream>;
}

#[derive(Clone)]
pub struct BridgeNet {
    /// One dialer per bridge, in [`BRIDGE_ADDRS`] order.
    dialers: Arc<[Arc<dyn Dialer>]>,
}

impl BridgeNet {
    pub fn new(dialers: Vec<Arc<dyn Dialer>>) -> Self {
        assert!(!dialers.is_empty() && dialers.len() <= BRIDGE_ADDRS.len(), "1 or 2 Snowflake bridges");
        Self { dialers: dialers.into() }
    }
}

impl std::fmt::Debug for BridgeNet {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("BridgeNet")
    }
}

/// No listener exists (uninhabited).
pub enum NoListener {}

impl<A: Send + Sync + 'static> NetStreamListener<A> for NoListener {
    type Stream = SnowflakeStream;
    type Incoming = stream::Pending<IoResult<(SnowflakeStream, A)>>;

    fn incoming(self) -> Self::Incoming {
        match self {}
    }

    fn local_addr(&self) -> IoResult<A> {
        match *self {}
    }
}

#[async_trait]
impl NetStreamProvider<SocketAddr> for BridgeNet {
    type Stream = SnowflakeStream;
    type Listener = NoListener;
    type ConnectOptions = ();
    type ListenOptions = tor_rtcompat::TcpListenOptions;

    async fn connect(&self, addr: &SocketAddr, _options: &()) -> IoResult<SnowflakeStream> {
        match BRIDGE_ADDRS[..self.dialers.len()].iter().position(|a| a == addr) {
            Some(i) => self.dialers[i].dial(),
            None => Err(refused(&format!("a connection to {addr}"))),
        }
    }

    async fn listen(&self, _addr: &SocketAddr, _options: &Self::ListenOptions) -> IoResult<NoListener> {
        Err(refused("listening"))
    }
}

#[async_trait]
impl NetStreamProvider<unix::SocketAddr> for BridgeNet {
    type Stream = SnowflakeStream;
    type Listener = NoListener;
    type ConnectOptions = ();
    type ListenOptions = tor_rtcompat::UnixListenOptions;

    async fn connect(&self, _addr: &unix::SocketAddr, _options: &()) -> IoResult<SnowflakeStream> {
        Err(refused("a Unix socket"))
    }

    async fn listen(&self, _addr: &unix::SocketAddr, _options: &Self::ListenOptions) -> IoResult<NoListener> {
        Err(refused("a Unix socket"))
    }
}

/// No UDP socket exists (uninhabited).
pub enum NoUdp {}

#[async_trait]
impl UdpSocket for NoUdp {
    async fn recv(&self, _buf: &mut [u8]) -> IoResult<(usize, SocketAddr)> {
        match *self {}
    }
    async fn send(&self, _buf: &[u8], _target: &SocketAddr) -> IoResult<usize> {
        match *self {}
    }
    fn local_addr(&self) -> IoResult<SocketAddr> {
        match *self {}
    }
}

#[async_trait]
impl UdpProvider for BridgeNet {
    type UdpSocket = NoUdp;

    async fn bind(&self, _addr: &SocketAddr) -> IoResult<NoUdp> {
        Err(refused("UDP"))
    }
}
