//! smux v2 client (xtaci/smux compatible; ported from v1.5.24, interop-tested against v1.5.56), one stream: the Tor connection to the bridge.
//!
//! Frame: `ver u8 = 2, cmd u8, len u16 LE, sid u32 LE`, then `len` bytes. Commands: SYN 0, FIN 1,
//! PSH 2, NOP 3 (keep-alive), UPD 4 (`consumed u32, window u32`: flow control). Sans-IO: frames
//! are produced into / parsed from the KCP byte stream by the caller ([`crate::Session`]).
//!
//! Flow control: we may have at most `peer_window` bytes written and not yet consumed by the
//! bridge (initially 256 KiB, then as it reports). We tell the bridge our window ([`RX_BUF`])
//! after the first read and whenever half of it was consumed, as smux does.

pub const HEADER: usize = 8;
/// Our stream receive buffer, advertised as the window (Snowflake's `StreamSize`).
pub const RX_BUF: usize = 1 << 20;
/// Largest PSH frame we send.
pub const MAX_FRAME: usize = 16 * 1024;
/// Client streams are odd; the Go client's first one is 3.
pub const STREAM_ID: u32 = 3;
const VERSION: u8 = 2;
const SYN: u8 = 0;
const FIN: u8 = 1;
const PSH: u8 = 2;
const NOP: u8 = 3;
const UPD: u8 = 4;
const INITIAL_PEER_WINDOW: u32 = 262_144;

#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub enum SmuxError {
    /// Wrong version or unknown command.
    Protocol,
    /// The bridge sent more than our window.
    Overflow,
}

/// Where the frame parser is in the incoming byte stream.
#[derive(Copy, Clone)]
enum Rx {
    Header,
    Body { cmd: u8, sid: u32, left: usize },
}

pub struct Smux {
    // ---- receive ----
    rx: Rx,
    hdr: [u8; HEADER],
    hdr_len: usize,
    upd: [u8; 8],
    upd_len: usize,
    /// Ring of stream data not yet read by the application.
    buf: Box<[u8; RX_BUF]>,
    head: usize,
    len: usize,
    num_read: u32,
    incr: u32,
    fin: bool,
    // ---- send ----
    syn_sent: bool,
    num_written: u32,
    peer_consumed: u32,
    peer_window: u32,
    /// Consumed count to report in an UPD (0 = none due).
    upd_due: u32,
}

impl Default for Smux {
    fn default() -> Self {
        Self::new()
    }
}

impl Smux {
    pub fn new() -> Self {
        Self {
            rx: Rx::Header,
            hdr: [0; HEADER],
            hdr_len: 0,
            upd: [0; 8],
            upd_len: 0,
            buf: crate::boxed(0),
            head: 0,
            len: 0,
            num_read: 0,
            incr: 0,
            fin: false,
            syn_sent: false,
            num_written: 0,
            peer_consumed: 0,
            peer_window: INITIAL_PEER_WINDOW,
            upd_due: 0,
        }
    }

    /// The bridge closed the stream (FIN) and everything was read.
    #[inline(always)]
    pub fn eof(&self) -> bool {
        self.fin && self.len == 0
    }

    #[inline(always)]
    pub fn readable(&self) -> usize {
        self.len
    }

    /// Bytes of stream data the bridge's window admits now.
    #[inline]
    pub fn writable(&self) -> usize {
        let inflight = self.num_written.wrapping_sub(self.peer_consumed) as i64;
        (self.peer_window as i64 - inflight).max(0) as usize
    }

    #[inline(always)]
    fn frame_header(cmd: u8, len: usize, sid: u32) -> [u8; HEADER] {
        let mut h = [0u8; HEADER];
        h[0] = VERSION;
        h[1] = cmd;
        h[2..4].copy_from_slice(&(len as u16).to_le_bytes());
        h[4..8].copy_from_slice(&sid.to_le_bytes());
        h
    }

    /// Control frames due (SYN once, UPD); `put` must take the whole frame.
    pub fn control(&mut self, put: &mut impl FnMut(&[u8])) {
        if !self.syn_sent {
            put(&Self::frame_header(SYN, 0, STREAM_ID));
            self.syn_sent = true;
        }
        if self.upd_due != 0 {
            let mut f = [0u8; HEADER + 8];
            f[..HEADER].copy_from_slice(&Self::frame_header(UPD, 8, STREAM_ID));
            f[HEADER..HEADER + 4].copy_from_slice(&self.upd_due.to_le_bytes());
            f[HEADER + 4..].copy_from_slice(&(RX_BUF as u32).to_le_bytes());
            put(&f);
            self.upd_due = 0;
        }
    }

    /// A keep-alive NOP frame.
    pub fn nop() -> [u8; HEADER] {
        Self::frame_header(NOP, 0, 0)
    }

    /// Frames up to `limit` bytes of `data` (bridge window, [`MAX_FRAME`] per frame); `put` is
    /// called with each header and payload. Returns the payload bytes framed.
    pub fn write(&mut self, data: &[u8], limit: usize, put: &mut impl FnMut(&[u8], &[u8])) -> usize {
        let n = data.len().min(self.writable()).min(limit.saturating_sub(HEADER));
        let mut done = 0;
        while done < n {
            let take = (n - done).min(MAX_FRAME);
            put(&Self::frame_header(PSH, take, STREAM_ID), &data[done..done + take]);
            done += take;
        }
        self.num_written = self.num_written.wrapping_add(done as u32);
        done
    }

    /// Reads stream data; schedules an UPD as smux does.
    pub fn read(&mut self, out: &mut [u8]) -> usize {
        let n = out.len().min(self.len);
        let first = n.min(RX_BUF - self.head);
        out[..first].copy_from_slice(&self.buf[self.head..self.head + first]);
        out[first..n].copy_from_slice(&self.buf[..n - first]);
        self.head = (self.head + n) % RX_BUF;
        self.len -= n;
        if n > 0 {
            self.num_read = self.num_read.wrapping_add(n as u32);
            self.incr += n as u32;
            if self.incr >= (RX_BUF / 2) as u32 || self.num_read == n as u32 {
                self.upd_due = self.num_read;
                self.incr = 0;
            }
        }
        n
    }

    /// Parses frames from the KCP byte stream.
    pub fn input(&mut self, mut data: &[u8]) -> Result<(), SmuxError> {
        while !data.is_empty() {
            match self.rx {
                Rx::Header => {
                    let take = (HEADER - self.hdr_len).min(data.len());
                    self.hdr[self.hdr_len..self.hdr_len + take].copy_from_slice(&data[..take]);
                    self.hdr_len += take;
                    data = &data[take..];
                    if self.hdr_len < HEADER {
                        break;
                    }
                    self.hdr_len = 0;
                    let h = self.hdr;
                    if h[0] != VERSION {
                        return Err(SmuxError::Protocol);
                    }
                    let sid = u32::from_le_bytes([h[4], h[5], h[6], h[7]]);
                    let len = u16::from_le_bytes([h[2], h[3]]) as usize;
                    match h[1] {
                        NOP | SYN => {}
                        FIN => {
                            if sid == STREAM_ID {
                                self.fin = true;
                            }
                        }
                        PSH if len > 0 => self.rx = Rx::Body { cmd: PSH, sid, left: len },
                        PSH => {}
                        UPD => self.rx = Rx::Body { cmd: UPD, sid, left: 8 },
                        _ => return Err(SmuxError::Protocol),
                    }
                }
                Rx::Body { cmd, sid, left } => {
                    let take = left.min(data.len());
                    if cmd == PSH {
                        if sid == STREAM_ID {
                            if self.len + take > RX_BUF {
                                return Err(SmuxError::Overflow);
                            }
                            let tail = (self.head + self.len) % RX_BUF;
                            let first = take.min(RX_BUF - tail);
                            self.buf[tail..tail + first].copy_from_slice(&data[..first]);
                            self.buf[..take - first].copy_from_slice(&data[first..take]);
                            self.len += take;
                        }
                    } else {
                        self.upd[self.upd_len..self.upd_len + take].copy_from_slice(&data[..take]);
                        self.upd_len += take;
                        if self.upd_len == 8 {
                            self.upd_len = 0;
                            if sid == STREAM_ID {
                                let u = self.upd;
                                self.peer_consumed = u32::from_le_bytes([u[0], u[1], u[2], u[3]]);
                                self.peer_window = u32::from_le_bytes([u[4], u[5], u[6], u[7]]);
                            }
                        }
                    }
                    data = &data[take..];
                    self.rx = if left == take { Rx::Header } else { Rx::Body { cmd, sid, left: left - take } };
                }
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn frame(cmd: u8, sid: u32, body: &[u8]) -> Vec<u8> {
        let mut v = Smux::frame_header(cmd, body.len(), sid).to_vec();
        v.extend_from_slice(body);
        v
    }

    #[test]
    fn syn_then_psh_and_window() {
        let mut s = Smux::new();
        let mut out = Vec::new();
        s.control(&mut |f| out.extend_from_slice(f));
        assert_eq!(out, frame(SYN, 3, b""));
        out.clear();
        let data = vec![1u8; 300_000];
        let n = s.write(&data, usize::MAX, &mut |h, p| {
            out.extend_from_slice(h);
            out.extend_from_slice(p);
        });
        assert_eq!(n, 262_144, "initial peer window");
        assert_eq!(s.writable(), 0);
        let mut upd = 262_144u32.to_le_bytes().to_vec();
        upd.extend_from_slice(&(1u32 << 20).to_le_bytes());
        s.input(&frame(UPD, 3, &upd)).unwrap();
        assert_eq!(s.writable(), 1 << 20);
    }

    #[test]
    fn receive_split_frames_and_upd() {
        let mut s = Smux::new();
        let mut wire = frame(NOP, 0, b"");
        wire.extend(frame(PSH, 3, b"hello "));
        wire.extend(frame(PSH, 5, b"other stream"));
        wire.extend(frame(PSH, 3, b"world"));
        wire.extend(frame(FIN, 3, b""));
        for c in wire.chunks(3) {
            s.input(c).unwrap();
        }
        let mut b = [0u8; 64];
        let n = s.read(&mut b);
        assert_eq!(&b[..n], b"hello world");
        assert!(s.eof());
        let mut ctl = Vec::new();
        s.control(&mut |f| ctl.extend_from_slice(f));
        let upd = &ctl[HEADER..]; // after SYN
        assert_eq!(upd[1], UPD);
        assert_eq!(u32::from_le_bytes(upd[8..12].try_into().unwrap()), 11, "first read reports consumption");
        assert!(s.input(&[1, 2, 0, 0, 0, 0, 0, 0]).is_err(), "version 1 frame refused");
    }
}
