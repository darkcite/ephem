//! Turbotunnel session (snowflake `client/lib`): one KCP connection and one smux stream that
//! outlive the WebRTC DataChannels (proxies) under them.
//!
//! Every new DataChannel starts with `Token ‖ ClientID`, then carries encapsulated KCP packets
//! both ways. When a proxy goes away the caller opens a DataChannel to another proxy and calls
//! [`Session::on_channel`]; unacknowledged data is simply retransmitted on the new one.
//!
//! Copies on the receive path (documented, §11.6 style): DataChannel → wasm (1, the adapter's),
//! encapsulation reassembly (1, packets may span DataChannel messages), KCP reorder ring (1),
//! smux stream ring (1), read by the TLS layer (1). All buffers are allocated once.

use crate::encap::{self, DecodeError, Decoder};
use crate::kcp::{self, Kcp};
use crate::smux::{self, Smux, SmuxError};

/// Turbotunnel opt-in token (snowflake `common/turbotunnel/consts.go`).
pub const TOKEN: [u8; 8] = [0x12, 0x93, 0x60, 0x5d, 0x27, 0x81, 0x75, 0xf5];
/// Largest DataChannel message we send.
pub const MAX_MESSAGE: usize = 16 * 1024;
/// smux keep-alive (smux `KeepAliveInterval`).
const NOP_EVERY_MS: u32 = 10_000;
/// KCP flush interval.
pub const TICK_MS: u32 = 10;

#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub enum Error {
    Encapsulation(DecodeError),
    Kcp(kcp::InputError),
    Smux(SmuxError),
    /// KCP gave up retransmitting: the bridge is unreachable through every proxy tried.
    Dead,
}

pub struct Session {
    kcp: Kcp,
    smux: Smux,
    dec: Decoder<{ kcp::MTU + 64 }>,
    client_id: [u8; 8],
    channel: bool,
    need_prefix: bool,
    out: Box<[u8; MAX_MESSAGE]>,
    out_len: usize,
    last_nop: u32,
    err: Option<Error>,
}

impl Session {
    /// `client_id` and `conv` come from the CSPRNG, once per Tor connection.
    pub fn new(client_id: [u8; 8], conv: u32) -> Self {
        Self {
            kcp: Kcp::new(conv),
            smux: Smux::new(),
            dec: Decoder::new(),
            client_id,
            channel: false,
            need_prefix: false,
            out: Box::new([0; MAX_MESSAGE]),
            out_len: 0,
            last_nop: 0,
            err: None,
        }
    }

    #[inline(always)]
    pub fn error(&self) -> Option<Error> {
        self.err
    }

    /// A DataChannel to a proxy opened (the first, or a replacement).
    pub fn on_channel(&mut self) {
        self.channel = true;
        self.need_prefix = true;
        self.dec.reset();
    }

    /// The current DataChannel closed; nothing is sent until the next one.
    pub fn on_channel_lost(&mut self) {
        self.channel = false;
        self.out_len = 0;
    }

    /// Bytes from the DataChannel.
    pub fn on_data(&mut self, now: u32, data: &[u8]) -> Result<(), Error> {
        if let Some(e) = self.err {
            return Err(e);
        }
        let Self { dec, kcp, err, .. } = self;
        let r = dec.feed(data, |pkt| {
            if err.is_none()
                && let Err(e) = kcp.input(now, pkt)
            {
                *err = Some(Error::Kcp(e));
            }
        });
        if let Err(e) = r {
            self.err = Some(Error::Encapsulation(e));
        }
        // KCP in-order bytes → smux frames.
        let mut tmp = [0u8; 8 * 1024];
        while self.err.is_none() && self.kcp.readable() && self.smux.readable() + tmp.len() <= smux::RX_BUF {
            let n = self.kcp.recv(&mut tmp);
            if let Err(e) = self.smux.input(&tmp[..n]) {
                self.err = Some(Error::Smux(e));
            }
        }
        self.err.map_or(Ok(()), Err)
    }

    /// Stream bytes toward the bridge; returns how many were taken (flow control, KCP ring).
    pub fn write(&mut self, data: &[u8]) -> usize {
        if self.err.is_some() {
            return 0;
        }
        let Self { kcp, smux, .. } = self;
        let mut put_ctl = |f: &[u8]| {
            let n = kcp.send(f);
            debug_assert_eq!(n, f.len());
        };
        smux.control(&mut put_ctl);
        let limit = kcp.send_space();
        smux.write(data, limit, &mut |h, p| {
            let a = kcp.send(h);
            let b = kcp.send(p);
            debug_assert_eq!(a + b, h.len() + p.len());
        })
    }

    /// Stream bytes from the bridge (0 = none yet; see [`Self::eof`]).
    pub fn read(&mut self, out: &mut [u8]) -> usize {
        let n = self.smux.read(out);
        // Freed smux space may unblock data waiting in KCP.
        let mut tmp = [0u8; 8 * 1024];
        while self.err.is_none() && self.kcp.readable() && self.smux.readable() + tmp.len() <= smux::RX_BUF {
            let m = self.kcp.recv(&mut tmp);
            if let Err(e) = self.smux.input(&tmp[..m]) {
                self.err = Some(Error::Smux(e));
            }
        }
        n
    }

    #[inline(always)]
    pub fn readable(&self) -> bool {
        self.smux.readable() > 0
    }

    /// The bridge closed the stream and everything was read.
    #[inline(always)]
    pub fn eof(&self) -> bool {
        self.smux.eof()
    }

    /// Whether [`Self::write`] can take bytes now.
    #[inline]
    pub fn writable(&self) -> bool {
        self.smux.writable() > 0 && self.kcp.send_space() > smux::HEADER
    }

    /// Timer and output: control frames, keep-alive, KCP flush. `send` gets whole DataChannel
    /// messages (≤ [`MAX_MESSAGE`]). Call every [`TICK_MS`] and after `on_data` / `write`.
    pub fn poll(&mut self, now: u32, send: &mut impl FnMut(&[u8])) -> Result<(), Error> {
        if let Some(e) = self.err {
            return Err(e);
        }
        if !self.channel {
            return Ok(());
        }
        {
            let Self { kcp, smux, .. } = self;
            smux.control(&mut |f: &[u8]| {
                kcp.send(f);
            });
        }
        if now.wrapping_sub(self.last_nop) >= NOP_EVERY_MS {
            self.last_nop = now;
            self.kcp.send(&Smux::nop());
        }
        if self.need_prefix {
            self.out[..8].copy_from_slice(&TOKEN);
            self.out[8..16].copy_from_slice(&self.client_id);
            self.out_len = 16;
            self.need_prefix = false;
        }
        let Self { kcp, out, out_len, .. } = self;
        kcp.flush(now, &mut |pkt: &[u8]| {
            let mut pre = [0u8; 3];
            let p = encap::prefix(pkt.len(), &mut pre);
            if *out_len + p + pkt.len() > MAX_MESSAGE {
                send(&out[..*out_len]);
                *out_len = 0;
            }
            out[*out_len..*out_len + p].copy_from_slice(&pre[..p]);
            out[*out_len + p..*out_len + p + pkt.len()].copy_from_slice(pkt);
            *out_len += p + pkt.len();
        });
        if self.out_len > 0 {
            send(&self.out[..self.out_len]);
            self.out_len = 0;
        }
        if self.kcp.dead() {
            self.err = Some(Error::Dead);
            return Err(Error::Dead);
        }
        Ok(())
    }
}
