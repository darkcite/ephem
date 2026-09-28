//! The Snowflake byte stream as arti sees a TCP connection to the bridge.
//!
//! arti reads and writes through [`SnowflakeStream`] (`AsyncRead`/`AsyncWrite`); a *carrier*
//! (browser DataChannels, or a WebSocket in the native lab tests) moves the
//! [`ephem_snowflake::Session`]'s packets and wakes the stream. Both sides share one [`Link`].
//!
//! Locking: a `std::sync::Mutex` makes the stream `Send + Sync` as arti requires. On
//! `wasm32-unknown-unknown` there is one thread, so it is never contended.

use ephem_snowflake::Session;
use futures::io::{AsyncRead, AsyncWrite};
use std::io;
use std::pin::Pin;
use std::sync::{Arc, Mutex, MutexGuard};
use std::task::{Context, Poll, Waker};
use tor_rtcompat::StreamOps;

/// Session state shared by the stream (arti side) and the carrier (network side).
pub struct Shared {
    pub sess: Session,
    read_waker: Option<Waker>,
    write_waker: Option<Waker>,
    /// Wakes the carrier when arti wrote (so it flushes without waiting for its timer).
    carrier_waker: Option<Waker>,
    /// arti closed the stream.
    pub closed: bool,
    /// The carrier gave up (no proxy, rendezvous failed, KCP dead).
    pub failed: Option<String>,
}

#[derive(Clone)]
pub struct Link(Arc<Mutex<Shared>>);

impl Link {
    pub fn new(sess: Session) -> Self {
        Self(Arc::new(Mutex::new(Shared { sess, read_waker: None, write_waker: None, carrier_waker: None, closed: false, failed: None })))
    }

    pub fn lock(&self) -> MutexGuard<'_, Shared> {
        self.0.lock().unwrap_or_else(|e| e.into_inner())
    }
}

impl Shared {
    /// Carrier: wakes arti if it can make progress now.
    pub fn wake(&mut self) {
        if (self.sess.readable() || self.sess.eof() || self.failed.is_some() || self.sess.error().is_some())
            && let Some(w) = self.read_waker.take()
        {
            w.wake();
        }
        if (self.sess.writable() || self.failed.is_some())
            && let Some(w) = self.write_waker.take()
        {
            w.wake();
        }
    }

    /// Carrier: register to be woken when arti writes.
    pub fn set_carrier_waker(&mut self, w: &Waker) {
        self.carrier_waker = Some(w.clone());
    }

    fn io_error(&self) -> Option<io::Error> {
        if let Some(f) = &self.failed {
            return Some(io::Error::new(io::ErrorKind::ConnectionAborted, f.clone()));
        }
        self.sess.error().map(|e| io::Error::new(io::ErrorKind::ConnectionReset, format!("snowflake: {e:?}")))
    }
}

/// arti's "TCP connection" to the Snowflake bridge.
pub struct SnowflakeStream {
    link: Link,
}

impl SnowflakeStream {
    pub fn new(link: Link) -> Self {
        Self { link }
    }
}

impl AsyncRead for SnowflakeStream {
    fn poll_read(self: Pin<&mut Self>, cx: &mut Context<'_>, buf: &mut [u8]) -> Poll<io::Result<usize>> {
        let mut g = self.link.lock();
        let n = g.sess.read(buf);
        if n > 0 {
            return Poll::Ready(Ok(n));
        }
        if let Some(e) = g.io_error() {
            return Poll::Ready(Err(e));
        }
        if g.sess.eof() {
            return Poll::Ready(Ok(0));
        }
        g.read_waker = Some(cx.waker().clone());
        Poll::Pending
    }
}

impl AsyncWrite for SnowflakeStream {
    fn poll_write(self: Pin<&mut Self>, cx: &mut Context<'_>, buf: &[u8]) -> Poll<io::Result<usize>> {
        let mut g = self.link.lock();
        if let Some(e) = g.io_error() {
            return Poll::Ready(Err(e));
        }
        let n = g.sess.write(buf);
        if n > 0 {
            if let Some(w) = g.carrier_waker.take() {
                w.wake();
            }
            return Poll::Ready(Ok(n));
        }
        g.write_waker = Some(cx.waker().clone());
        Poll::Pending
    }

    fn poll_flush(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        // The carrier flushes on its own timer and whenever it is woken by a write.
        Poll::Ready(Ok(()))
    }

    fn poll_close(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        let mut g = self.link.lock();
        g.closed = true;
        if let Some(w) = g.carrier_waker.take() {
            w.wake();
        }
        Poll::Ready(Ok(()))
    }
}

impl StreamOps for SnowflakeStream {}
