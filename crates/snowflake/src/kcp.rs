// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
// Copyright 2026 Anton (darkcite)
//! KCP (ikcp / kcp-go v5 compatible; ported from v5.6.8, interop-tested against v5.6.24), as Snowflake configures it: stream mode, congestion
//! window off (`nc = 1`), no FEC, no crypto, MTU 1400, `nodelay = 0`, interval 10 ms, no fast
//! resend. Sans-IO: the caller passes the time and receives output packets through a closure.
//!
//! Memory: two fixed rings of [`WND`] segments (send and receive), allocated once. Segment
//! `sn` lives in slot `sn % WND`. Receive side: `[rcv_read, rcv_nxt)` is the in-order queue the
//! application reads from, and out-of-order segments wait in `[rcv_nxt, rcv_read + WND)`. Send
//! side: `[snd_una, snd_nxt)` is in flight (kcp's `snd_buf`), `[snd_nxt, snd_end)` is queued
//! (kcp's `snd_queue`).

pub const OVERHEAD: usize = 24;
pub const MTU: usize = 1400;
pub const MSS: usize = MTU - OVERHEAD;
/// Ring size in segments (power of two). Also our advertised receive window, which caps what
/// the bridge keeps in flight toward us (≈ 1.4 MB).
pub const WND: usize = 1024;

const CMD_PUSH: u8 = 81;
const CMD_ACK: u8 = 82;
const CMD_WASK: u8 = 83;
const CMD_WINS: u8 = 84;
const ASK_SEND: u8 = 1;
const ASK_TELL: u8 = 2;
const RTO_MIN: u32 = 100;
const RTO_DEF: u32 = 200;
const RTO_MAX: u32 = 60_000;
const INTERVAL: u32 = 10;
const DEAD_LINK: u32 = 20;
const PROBE_INIT: u32 = 7_000;
const PROBE_LIMIT: u32 = 120_000;

#[inline(always)]
fn diff(a: u32, b: u32) -> i32 {
    a.wrapping_sub(b) as i32
}

#[inline(always)]
fn slot(sn: u32) -> usize {
    sn as usize & (WND - 1)
}

#[derive(Copy, Clone)]
struct SndSeg {
    ts: u32,
    resendts: u32,
    rto: u32,
    fastack: u32,
    xmit: u32,
    len: u16,
    acked: bool,
    data: [u8; MSS],
}

#[derive(Copy, Clone)]
struct RcvSeg {
    present: bool,
    len: u16,
    data: [u8; MSS],
}

#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub enum InputError {
    Short,
    Conv,
    Truncated,
    Cmd,
}

pub struct Kcp {
    conv: u32,
    snd_una: u32,
    snd_nxt: u32,
    snd_end: u32,
    rcv_nxt: u32,
    rcv_read: u32,
    /// Bytes of the queue head already given to the application.
    rcv_off: u16,
    rmt_wnd: u32,
    rx_srtt: i32,
    rx_rttvar: i32,
    rx_rto: u32,
    probe: u8,
    ts_probe: u32,
    probe_wait: u32,
    dead: bool,
    acks: Box<[(u32, u32); WND]>,
    nacks: usize,
    snd: Box<[SndSeg; WND]>,
    rcv: Box<[RcvSeg; WND]>,
    buf: [u8; MTU],
}

impl Kcp {
    /// A new KCP endpoint. `conv` is random per session (the bridge learns it from our first
    /// packet).
    pub fn new(conv: u32) -> Self {
        Self {
            conv,
            snd_una: 0,
            snd_nxt: 0,
            snd_end: 0,
            rcv_nxt: 0,
            rcv_read: 0,
            rcv_off: 0,
            rmt_wnd: 32,
            rx_srtt: 0,
            rx_rttvar: 0,
            rx_rto: RTO_DEF,
            probe: 0,
            ts_probe: 0,
            probe_wait: 0,
            dead: false,
            acks: crate::boxed((0, 0)),
            nacks: 0,
            snd: crate::boxed(SndSeg { ts: 0, resendts: 0, rto: 0, fastack: 0, xmit: 0, len: 0, acked: false, data: [0; MSS] }),
            rcv: crate::boxed(RcvSeg { present: false, len: 0, data: [0; MSS] }),
            buf: [0; MTU],
        }
    }

    /// A segment was sent `DEAD_LINK` times without an ACK: the path is gone.
    #[inline(always)]
    pub fn dead(&self) -> bool {
        self.dead
    }

    /// Bytes that [`Self::send`] would accept now.
    pub fn send_space(&self) -> usize {
        let used = self.snd_end.wrapping_sub(self.snd_una) as usize;
        let mut space = (WND - used) * MSS;
        if self.snd_end != self.snd_nxt {
            space += MSS - self.snd[slot(self.snd_end.wrapping_sub(1))].len as usize;
        }
        space
    }

    /// A new path (another Snowflake proxy): everything in flight is sent again on the next
    /// flush, with the retransmission backoff and the dead-link count restarted. Otherwise a
    /// path change after losses waits out a backed-off timer (seconds) before the new proxy
    /// carries anything, and resends accumulated over several proxies declare the link dead.
    pub fn new_path(&mut self) {
        let mut sn = self.snd_una;
        while sn != self.snd_nxt {
            let s = &mut self.snd[slot(sn)];
            if !s.acked {
                s.xmit = 0;
                s.fastack = 0;
            }
            sn = sn.wrapping_add(1);
        }
    }

    /// Our data not yet acknowledged, in segments.
    #[inline(always)]
    pub fn unacked(&self) -> u32 {
        self.snd_end.wrapping_sub(self.snd_una)
    }

    /// Queues stream data; returns how many bytes were taken (the rest waits for window).
    pub fn send(&mut self, mut data: &[u8]) -> usize {
        let total = data.len();
        // Stream mode: top up the last queued, not yet transmitted segment.
        if self.snd_end != self.snd_nxt {
            let s = &mut self.snd[slot(self.snd_end.wrapping_sub(1))];
            let take = (MSS - s.len as usize).min(data.len());
            s.data[s.len as usize..s.len as usize + take].copy_from_slice(&data[..take]);
            s.len += take as u16;
            data = &data[take..];
        }
        while !data.is_empty() && (self.snd_end.wrapping_sub(self.snd_una) as usize) < WND {
            let take = MSS.min(data.len());
            let s = &mut self.snd[slot(self.snd_end)];
            s.data[..take].copy_from_slice(&data[..take]);
            s.len = take as u16;
            s.acked = false;
            s.xmit = 0;
            s.fastack = 0;
            self.snd_end = self.snd_end.wrapping_add(1);
            data = &data[take..];
        }
        total - data.len()
    }

    /// Reads in-order stream data; returns the byte count (0 = nothing ready).
    pub fn recv(&mut self, out: &mut [u8]) -> usize {
        let was_full = self.rcv_nxt.wrapping_sub(self.rcv_read) as usize >= WND;
        let mut n = 0;
        while n < out.len() && self.rcv_read != self.rcv_nxt {
            let s = &mut self.rcv[slot(self.rcv_read)];
            let off = self.rcv_off as usize;
            let take = (s.len as usize - off).min(out.len() - n);
            out[n..n + take].copy_from_slice(&s.data[off..off + take]);
            n += take;
            if off + take == s.len as usize {
                s.present = false;
                self.rcv_off = 0;
                self.rcv_read = self.rcv_read.wrapping_add(1);
            } else {
                self.rcv_off = (off + take) as u16;
            }
        }
        if was_full && (self.rcv_nxt.wrapping_sub(self.rcv_read) as usize) < WND {
            self.probe |= ASK_TELL;
        }
        n
    }

    /// Whether [`Self::recv`] has data.
    #[inline(always)]
    pub fn readable(&self) -> bool {
        self.rcv_read != self.rcv_nxt
    }

    #[inline(always)]
    fn wnd_unused(&self) -> u16 {
        (WND - self.rcv_nxt.wrapping_sub(self.rcv_read) as usize) as u16
    }

    /// One packet from the bridge (possibly several segments). Returns whether the send window
    /// slid (the caller should flush).
    pub fn input(&mut self, now: u32, mut data: &[u8]) -> Result<bool, InputError> {
        if data.len() < OVERHEAD {
            return Err(InputError::Short);
        }
        let mut latest_ts = 0u32;
        let mut got_ack = false;
        let mut slid = false;
        while data.len() >= OVERHEAD {
            let u32_at = |d: &[u8], i: usize| u32::from_le_bytes([d[i], d[i + 1], d[i + 2], d[i + 3]]);
            let conv = u32_at(data, 0);
            let cmd = data[4];
            let wnd = u16::from_le_bytes([data[6], data[7]]);
            let ts = u32_at(data, 8);
            let sn = u32_at(data, 12);
            let una = u32_at(data, 16);
            let len = u32_at(data, 20) as usize;
            if conv != self.conv {
                return Err(InputError::Conv);
            }
            let body = &data[OVERHEAD..];
            if body.len() < len {
                return Err(InputError::Truncated);
            }
            if !matches!(cmd, CMD_PUSH | CMD_ACK | CMD_WASK | CMD_WINS) {
                return Err(InputError::Cmd);
            }
            self.rmt_wnd = wnd as u32;
            slid |= self.parse_una(una);
            match cmd {
                CMD_ACK => {
                    self.parse_ack(sn);
                    self.parse_fastack(sn, ts);
                    got_ack = true;
                    latest_ts = ts;
                }
                CMD_PUSH => {
                    if diff(sn, self.rcv_nxt.wrapping_add(WND as u32)) < 0 {
                        self.ack_push(sn, ts);
                        if diff(sn, self.rcv_nxt) >= 0 && len <= MSS {
                            self.parse_data(sn, &body[..len]);
                        }
                    }
                }
                CMD_WASK => self.probe |= ASK_TELL,
                _ => {}
            }
            data = &body[len..];
        }
        if got_ack && diff(now, latest_ts) >= 0 {
            self.update_ack(diff(now, latest_ts));
        }
        Ok(slid)
    }

    fn parse_una(&mut self, una: u32) -> bool {
        // Everything before `una` is delivered (only within what we sent).
        if diff(una, self.snd_una) > 0 && diff(una, self.snd_nxt) <= 0 {
            self.snd_una = una;
            return true;
        }
        false
    }

    fn parse_ack(&mut self, sn: u32) {
        if diff(sn, self.snd_una) < 0 || diff(sn, self.snd_nxt) >= 0 {
            return;
        }
        self.snd[slot(sn)].acked = true;
    }

    fn parse_fastack(&mut self, sn: u32, ts: u32) {
        if diff(sn, self.snd_una) < 0 || diff(sn, self.snd_nxt) >= 0 {
            return;
        }
        let mut i = self.snd_una;
        while i != sn {
            let s = &mut self.snd[slot(i)];
            if !s.acked && diff(s.ts, ts) <= 0 {
                s.fastack += 1;
            }
            i = i.wrapping_add(1);
        }
    }

    fn ack_push(&mut self, sn: u32, ts: u32) {
        if self.nacks < WND {
            self.acks[self.nacks] = (sn, ts);
            self.nacks += 1;
        }
    }

    fn parse_data(&mut self, sn: u32, body: &[u8]) {
        // Our ring holds the unread queue too, so accept only within `rcv_read + WND`
        // (never more than we advertised).
        if diff(sn, self.rcv_read.wrapping_add(WND as u32)) >= 0 {
            return;
        }
        let s = &mut self.rcv[slot(sn)];
        if !s.present {
            s.present = true;
            s.len = body.len() as u16;
            s.data[..body.len()].copy_from_slice(body);
        }
        while self.rcv[slot(self.rcv_nxt)].present && self.rcv_nxt.wrapping_sub(self.rcv_read) < WND as u32 {
            self.rcv_nxt = self.rcv_nxt.wrapping_add(1);
            if self.rcv_nxt.wrapping_sub(self.rcv_read) == WND as u32 {
                break;
            }
        }
    }

    fn update_ack(&mut self, rtt: i32) {
        if self.rx_srtt == 0 {
            self.rx_srtt = rtt;
            self.rx_rttvar = rtt >> 1;
        } else {
            let mut delta = rtt - self.rx_srtt;
            self.rx_srtt += delta >> 3;
            if delta < 0 {
                delta = -delta;
            }
            if rtt < self.rx_srtt - self.rx_rttvar {
                self.rx_rttvar += (delta - self.rx_rttvar) >> 5;
            } else {
                self.rx_rttvar += (delta - self.rx_rttvar) >> 2;
            }
        }
        let rto = self.rx_srtt as u32 + INTERVAL.max((self.rx_rttvar as u32) << 2);
        self.rx_rto = rto.clamp(RTO_MIN, RTO_MAX);
    }

    fn header(&self, out: &mut [u8], cmd: u8, wnd: u16, ts: u32, sn: u32, len: u32) {
        out[0..4].copy_from_slice(&self.conv.to_le_bytes());
        out[4] = cmd;
        out[5] = 0; // frg: stream mode
        out[6..8].copy_from_slice(&wnd.to_le_bytes());
        out[8..12].copy_from_slice(&ts.to_le_bytes());
        out[12..16].copy_from_slice(&sn.to_le_bytes());
        out[16..20].copy_from_slice(&self.rcv_nxt.to_le_bytes());
        out[20..24].copy_from_slice(&len.to_le_bytes());
    }

    /// Sends ACKs, window probes and due data segments; `output` gets packets of ≤ [`MTU`]
    /// bytes. Call on every [`Self::input`] that slid the window, after [`Self::send`], and
    /// every [`INTERVAL`] ms. Returns the ms until the next retransmission is due.
    pub fn flush(&mut self, now: u32, output: &mut impl FnMut(&[u8])) -> u32 {
        let wnd = self.wnd_unused();
        let mut used = 0usize;
        macro_rules! space {
            ($n:expr) => {
                if used + $n > MTU {
                    output(&self.buf[..used]);
                    used = 0;
                }
            };
        }
        // ACKs (kcp-go drops stale ones below rcv_nxt except the last).
        let n = self.nacks;
        for i in 0..n {
            let (sn, ts) = self.acks[i];
            if diff(sn, self.rcv_nxt) >= 0 || i == n - 1 {
                space!(OVERHEAD);
                let mut h = [0u8; OVERHEAD];
                self.header(&mut h, CMD_ACK, wnd, ts, sn, 0);
                self.buf[used..used + OVERHEAD].copy_from_slice(&h);
                used += OVERHEAD;
            }
        }
        self.nacks = 0;

        // Window probing while the bridge's window is zero.
        if self.rmt_wnd == 0 {
            if self.probe_wait == 0 {
                self.probe_wait = PROBE_INIT;
                self.ts_probe = now.wrapping_add(self.probe_wait);
            } else if diff(now, self.ts_probe) >= 0 {
                self.probe_wait = (self.probe_wait + self.probe_wait / 2).min(PROBE_LIMIT);
                self.ts_probe = now.wrapping_add(self.probe_wait);
                self.probe |= ASK_SEND;
            }
        } else {
            self.ts_probe = 0;
            self.probe_wait = 0;
        }
        for (bit, cmd) in [(ASK_SEND, CMD_WASK), (ASK_TELL, CMD_WINS)] {
            if self.probe & bit != 0 {
                space!(OVERHEAD);
                let mut h = [0u8; OVERHEAD];
                self.header(&mut h, cmd, wnd, 0, 0, 0);
                self.buf[used..used + OVERHEAD].copy_from_slice(&h);
                used += OVERHEAD;
            }
        }
        self.probe = 0;

        // Move queued segments into flight, limited by the windows (no congestion window).
        let cwnd = (WND as u32).min(self.rmt_wnd);
        let mut new_segs = 0;
        while self.snd_nxt != self.snd_end && diff(self.snd_nxt, self.snd_una.wrapping_add(cwnd)) < 0 {
            self.snd_nxt = self.snd_nxt.wrapping_add(1);
            new_segs += 1;
        }

        let mut min_rto = INTERVAL as i32;
        let mut sn = self.snd_una;
        while sn != self.snd_nxt {
            let rx_rto = self.rx_rto;
            let s = &mut self.snd[slot(sn)];
            if !s.acked {
                let send = if s.xmit == 0 {
                    s.rto = rx_rto;
                    true
                } else if s.fastack > 0 && new_segs == 0 {
                    // early retransmit (fast resend is off)
                    s.rto = rx_rto;
                    true
                } else if diff(now, s.resendts) >= 0 {
                    s.rto += rx_rto; // nodelay = 0
                    true
                } else {
                    false
                };
                if send {
                    s.fastack = 0;
                    s.resendts = now.wrapping_add(s.rto);
                    s.xmit += 1;
                    s.ts = now;
                    if s.xmit >= DEAD_LINK {
                        self.dead = true;
                    }
                    let len = s.len as usize;
                    let (ts, rcv_nxt, conv) = (s.ts, self.rcv_nxt, self.conv);
                    space!(OVERHEAD + len);
                    let s = &self.snd[slot(sn)];
                    let b = &mut self.buf[used..used + OVERHEAD + len];
                    b[0..4].copy_from_slice(&conv.to_le_bytes());
                    b[4] = CMD_PUSH;
                    b[5] = 0;
                    b[6..8].copy_from_slice(&wnd.to_le_bytes());
                    b[8..12].copy_from_slice(&ts.to_le_bytes());
                    b[12..16].copy_from_slice(&sn.to_le_bytes());
                    b[16..20].copy_from_slice(&rcv_nxt.to_le_bytes());
                    b[20..24].copy_from_slice(&(len as u32).to_le_bytes());
                    b[OVERHEAD..].copy_from_slice(&s.data[..len]);
                    used += OVERHEAD + len;
                }
                let r = diff(self.snd[slot(sn)].resendts, now);
                if r > 0 && r < min_rto {
                    min_rto = r;
                }
            }
            sn = sn.wrapping_add(1);
        }
        if used > 0 {
            output(&self.buf[..used]);
        }
        min_rto as u32
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Two endpoints over a link that loses 1 in `drop_every` packets (pseudo-random) and
    /// reorders pairs.
    fn run(drop_every: u64, total: usize) {
        let mut a = Kcp::new(7);
        let mut b = Kcp::new(7);
        let src: Vec<u8> = (0..total).map(|i| (i * 31 % 251) as u8).collect();
        let mut sent = 0;
        let mut got = Vec::with_capacity(total);
        let mut buf = vec![0u8; 64 * 1024];
        let (mut ab, mut ba): (Vec<Vec<u8>>, Vec<Vec<u8>>) = (vec![], vec![]);
        let mut rng = 0x2545_F491_4F6C_DD1Du64;
        let mut lose = move || {
            rng ^= rng << 13;
            rng ^= rng >> 7;
            rng ^= rng << 17;
            drop_every != 0 && rng.is_multiple_of(drop_every)
        };
        let mut now = 0u32;
        while got.len() < total {
            now = now.wrapping_add(10);
            sent += a.send(&src[sent..]);
            a.flush(now, &mut |p| ab.push(p.to_vec()));
            b.flush(now, &mut |p| ba.push(p.to_vec()));
            if ab.len() >= 2 {
                ab.swap(0, 1);
            }
            for p in ab.drain(..) {
                if !lose() {
                    b.input(now, &p).unwrap();
                }
            }
            for p in ba.drain(..) {
                if !lose() {
                    a.input(now, &p).unwrap();
                }
            }
            let n = b.recv(&mut buf[..997]);
            got.extend_from_slice(&buf[..n]);
            assert!(now < 3_000_000, "stalled at {} of {total}", got.len());
            assert!(!a.dead());
        }
        assert_eq!(got, src);
    }

    #[test]
    fn lossless() {
        run(0, 5_000_000);
    }

    #[test]
    fn lossy_and_reordered() {
        run(7, 2_000_000);
        run(3, 300_000);
    }

    /// A path that loses everything for a long time backs off the retransmission timer; a new
    /// path resends at once and starts the dead-link count again.
    #[test]
    fn new_path_resends_at_once() {
        let mut a = Kcp::new(3);
        a.send(&[7u8; 3000]);
        let mut now = 0u32;
        let mut sends = 0;
        // 19 transmissions into the void (one short of DEAD_LINK), timer backed off to seconds.
        while sends < DEAD_LINK - 1 {
            now = now.wrapping_add(10);
            a.flush(now, &mut |_| sends += 1);
            if sends > 0 {
                break;
            }
        }
        for _ in 0..DEAD_LINK - 2 {
            let at = a.snd[slot(a.snd_una)].resendts;
            a.flush(at, &mut |_| {});
            now = at;
        }
        assert!(!a.dead());
        assert!(a.snd[slot(a.snd_una)].rto > 2_000, "backed off");
        // Without a new path, the next flush sends nothing until the backed-off timer.
        let mut out = 0;
        a.flush(now.wrapping_add(10), &mut |_| out += 1);
        assert_eq!(out, 0);
        a.new_path();
        a.flush(now.wrapping_add(20), &mut |_| out += 1);
        assert!(out > 0, "in-flight data resent on the new path at once");
        for _ in 0..5 {
            let at = a.snd[slot(a.snd_una)].resendts;
            a.flush(at, &mut |_| {});
        }
        assert!(!a.dead(), "the dead-link count restarted with the path");
    }

    #[test]
    fn conv_and_malformed() {
        let mut a = Kcp::new(1);
        let mut p = [0u8; 24];
        p[0] = 2;
        assert_eq!(a.input(0, &p), Err(InputError::Conv));
        p[0] = 1;
        p[4] = 99;
        assert_eq!(a.input(0, &p), Err(InputError::Cmd));
        p[4] = CMD_PUSH;
        p[20] = 5;
        assert_eq!(a.input(0, &p), Err(InputError::Truncated));
        assert_eq!(a.input(0, &p[..10]), Err(InputError::Short));
    }
}
