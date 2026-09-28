//! Message state of one chat (§11.3, §11.7): the pending ring and self-destruct timers.
//!
//! Both are fixed-capacity and allocated once per chat (setup path). Nothing here grows.

use ephem_proto::frame::MAX_TEXT;

/// Unacknowledged CHAT records kept for resend (§11.3).
pub const PENDING_CAP: usize = 256;
/// Live self-destruct timers per direction. Overflow expires the oldest early (never late).
pub const TTL_CAP: usize = 256;
/// Allowed self-destruct values (§11.7), seconds. 0 = off.
pub const TTL_CHOICES: [u32; 7] = [0, 5, 30, 60, 300, 3_600, 86_400];

/// Reference to a message from this side's point of view.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct MsgRef {
    pub mine: bool,
    pub seq: u64,
}

/// One pending CHAT: the exact record body fields, so a resend is byte-identical in meaning.
#[derive(Copy, Clone)]
pub struct Slot {
    pub seq: u64,
    pub rflags: u8,
    /// Removed before it was ever delivered: skipped on resend.
    pub deleted: bool,
    pub ttl_s: u32,
    pub reply: MsgRef,
    pub len: u16,
    pub text: [u8; MAX_TEXT],
}

impl Slot {
    const EMPTY: Self = Self { seq: 0, rflags: 0, deleted: false, ttl_s: 0, reply: MsgRef { mine: false, seq: 0 }, len: 0, text: [0; MAX_TEXT] };

    #[inline(always)]
    pub fn text(&self) -> &[u8] {
        &self.text[..self.len as usize]
    }

    fn wipe(&mut self) {
        self.text[..self.len as usize].fill(0);
        self.seq = 0;
        self.rflags = 0;
        self.deleted = false;
        self.ttl_s = 0;
        self.reply = MsgRef { mine: false, seq: 0 };
        self.len = 0;
    }
}

/// Ring of unacknowledged CHATs, indexed by `seq % PENDING_CAP`. Entries are the contiguous
/// range `acked+1 ..= last`, so no search is ever needed.
pub struct Pending {
    slots: Box<[Slot]>,
    acked: u64,
    last: u64,
}

impl Pending {
    pub fn new() -> Self {
        Self { slots: vec![Slot::EMPTY; PENDING_CAP].into_boxed_slice(), acked: 0, last: 0 }
    }

    #[inline(always)]
    pub fn acked(&self) -> u64 {
        self.acked
    }

    #[inline(always)]
    pub fn last(&self) -> u64 {
        self.last
    }

    #[inline(always)]
    pub fn is_full(&self) -> bool {
        self.last - self.acked >= PENDING_CAP as u64
    }

    #[inline(always)]
    fn idx(seq: u64) -> usize {
        (seq % PENDING_CAP as u64) as usize
    }

    /// Appends the next CHAT (seq = last + 1). The caller checked `is_full`.
    pub fn push(&mut self, rflags: u8, ttl_s: u32, reply: MsgRef, text: &[u8]) -> &Slot {
        debug_assert!(!self.is_full() && text.len() <= MAX_TEXT);
        self.last += 1;
        let s = &mut self.slots[Self::idx(self.last)];
        s.seq = self.last;
        s.rflags = rflags;
        s.deleted = false;
        s.ttl_s = ttl_s;
        s.reply = reply;
        s.len = text.len() as u16;
        s.text[..text.len()].copy_from_slice(text);
        s
    }

    /// A pending slot, if `seq` is still unacknowledged.
    pub fn get_mut(&mut self, seq: u64) -> Option<&mut Slot> {
        (seq > self.acked && seq <= self.last).then(|| &mut self.slots[Self::idx(seq)])
    }

    pub fn get(&self, seq: u64) -> Option<&Slot> {
        (seq > self.acked && seq <= self.last).then(|| &self.slots[Self::idx(seq)])
    }

    /// Cumulative ACK: frees and wipes every slot up to `seq`. Returns false for a bogus ACK.
    pub fn ack(&mut self, seq: u64) -> bool {
        if seq > self.last {
            return false;
        }
        while self.acked < seq {
            self.acked += 1;
            self.slots[Self::idx(self.acked)].wipe();
        }
        true
    }

    /// Wipes everything (chat closed).
    pub fn clear(&mut self) {
        for s in self.slots.iter_mut() {
            s.wipe();
        }
        self.acked = self.last;
    }
}

impl Default for Pending {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Copy, Clone)]
struct Timer {
    msg: MsgRef,
    ttl_ms: u64,
    /// 0 = not started.
    deadline_ms: u64,
}

/// Self-destruct timers for one direction.
pub struct Timers {
    t: [Timer; TTL_CAP],
    n: usize,
}

impl Timers {
    pub const fn new() -> Self {
        Self { t: [Timer { msg: MsgRef { mine: false, seq: 0 }, ttl_ms: 0, deadline_ms: 0 }; TTL_CAP], n: 0 }
    }

    /// Registers a message with a TTL (countdown not started). If full, the oldest entry is
    /// returned so the caller expires it now.
    pub fn add(&mut self, msg: MsgRef, ttl_s: u32) -> Option<MsgRef> {
        let mut evicted = None;
        if self.n == TTL_CAP {
            evicted = Some(self.t[0].msg);
            self.t.copy_within(1.., 0);
            self.n -= 1;
        }
        self.t[self.n] = Timer { msg, ttl_ms: ttl_s as u64 * 1000, deadline_ms: 0 };
        self.n += 1;
        evicted
    }

    /// Starts countdowns of every registered message with `seq <= upto`.
    pub fn start_upto(&mut self, upto: u64, now_ms: u64) {
        for t in self.t[..self.n].iter_mut() {
            if t.deadline_ms == 0 && t.msg.seq <= upto {
                t.deadline_ms = now_ms + t.ttl_ms;
            }
        }
    }

    /// Forgets a message (deleted before expiry).
    pub fn remove(&mut self, seq: u64) {
        if let Some(i) = self.t[..self.n].iter().position(|t| t.msg.seq == seq) {
            self.t.copy_within(i + 1..self.n, i);
            self.n -= 1;
        }
    }

    /// Removes and reports every expired message.
    pub fn expire(&mut self, now_ms: u64, mut f: impl FnMut(MsgRef)) {
        let mut i = 0;
        while i < self.n {
            let t = self.t[i];
            if t.deadline_ms != 0 && t.deadline_ms <= now_ms {
                f(t.msg);
                self.t.copy_within(i + 1..self.n, i);
                self.n -= 1;
            } else {
                i += 1;
            }
        }
    }
}

impl Default for Timers {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const NONE: MsgRef = MsgRef { mine: false, seq: 0 };

    #[test]
    fn pending_ring() {
        let mut p = Pending::new();
        for i in 0..PENDING_CAP {
            assert!(!p.is_full());
            assert_eq!(p.push(0, 0, NONE, &[i as u8]).seq, i as u64 + 1);
        }
        assert!(p.is_full());
        assert!(!p.ack(PENDING_CAP as u64 + 1), "ack beyond last");
        assert!(p.ack(10));
        assert!(p.get(10).is_none() && p.get(11).is_some());
        assert_eq!(p.push(0, 0, NONE, b"x").seq, PENDING_CAP as u64 + 1);
        assert_eq!(p.get(PENDING_CAP as u64 + 1).unwrap().text(), b"x");
        assert_eq!(p.get(11).unwrap().text(), &[10]);
        p.clear();
        assert!(p.get(12).is_none());
    }

    #[test]
    fn timers() {
        let mut t = Timers::new();
        t.add(MsgRef { mine: true, seq: 1 }, 5);
        t.add(MsgRef { mine: true, seq: 2 }, 30);
        t.start_upto(1, 1000);
        let mut out = [0u64; 4];
        let mut n = 0;
        t.expire(5999, |m| {
            out[n] = m.seq;
            n += 1
        });
        assert_eq!(n, 0);
        t.expire(6000, |m| {
            out[n] = m.seq;
            n += 1
        });
        assert_eq!((n, out[0]), (1, 1));
        t.expire(u64::MAX, |_| n += 1);
        assert_eq!(n, 1, "seq 2 never started");
        for s in 0..TTL_CAP as u64 {
            assert!(t.add(MsgRef { mine: false, seq: 10 + s }, 5).is_none() || s == TTL_CAP as u64 - 1);
        }
        assert_eq!(t.add(MsgRef { mine: false, seq: 999 }, 5), Some(MsgRef { mine: false, seq: 10 }));
    }
}
