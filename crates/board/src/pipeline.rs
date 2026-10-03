// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
// Copyright 2026 Anton (darkcite)
//! The host's intake (G.6.2, G.8), sans-IO: the gateway feeds it a header, then the body, and
//! publishes what it admits. Everything a submission can grow is a fixed table allocated once,
//! when the board is served; the refusal path allocates nothing (§22), except one allocation
//! inside Equi-X when a proof of work is actually checked (its HashX program; measured in the tests).
//!
//! Order (each step refuses before the next costs anything):
//! 1. the 188-byte header: kind, lengths, epoch (current or previous), effort ≥ the grace minimum
//!    for the kind, board not paused, new threads open;
//! 2. the replay table: a solution seen before is refused, or, for the same body already
//!    published, answered with its number (a retried submit is idempotent, G.6.1);
//! 3. the proof of work, from the header alone; the solution enters the replay table now;
//! 4. the body: its hash, then `s` decoded and matched to the header, then the signature;
//! 5. the caps: posts per minute, the thread budget; under contention the publish ring keeps the
//!    highest efforts (G.8, as Tor's prop 327).
//!
//! Every minute [`Intake::tick`] adapts the efforts and, when the board stays flooded, closes new
//! threads (R8) until the owner reopens them.

use crate::BoardError;
use crate::post::Signed;
use crate::pow::{self, Challenge};
use crate::submit::{HEADER_LEN, Header, body_hash, flag, kind};
use blake2::digest::{Update, VariableOutput};
use ephem_channel::cbor::Value;

/// Why a submission is refused: a stable code (G.6.3) and the HTTP status the gateway answers.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum Refusal {
    /// Bad or insufficient work, or a stale epoch.
    Pow,
    /// No room now (ring, caps, table): retry later, possibly with more effort.
    Busy,
    /// Malformed, mismatched, duplicate or not allowed.
    Refused,
    /// Posting paused by the owner, or new threads closed.
    Paused,
    /// Not the board's host (a mirror): posting needs the owner's onion (G.10).
    Offline,
}

impl Refusal {
    pub fn code(self) -> u16 {
        match self {
            Refusal::Pow => 0x0070,
            Refusal::Busy => 0x0071,
            Refusal::Refused => 0x0072,
            Refusal::Paused => 0x0073,
            Refusal::Offline => 0x0075,
        }
    }

    pub fn status(self) -> u16 {
        match self {
            Refusal::Pow => 403,
            Refusal::Busy => 503,
            Refusal::Refused => 409,
            Refusal::Paused => 423,
            Refusal::Offline => 503,
        }
    }
}

/// The base efforts the owner sets (G.8): the expected number of Equi-X solutions.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct Efforts {
    pub reply: u32,
    pub thread: u32,
}

impl Efforts {
    /// B-P1b, 4 Workers: an iPhone (iOS 18.7, Safari) solves 96.8/s, so a 10 s median there is
    /// effort ≈ 1 400. A mid-range Android is assumed about half as fast (to be measured), which puts
    /// the G.8 target (10 s median on a mid-range phone) near 700. At 700 the iPhone's median reply
    /// is 5 s (p95 22 s), a new thread (×8) 40 s (p95 2.9 min). The owner raises the base from here.
    pub const DEFAULT: Self = Self { reply: 700, thread: 700 * 8 };
}

/// Caps and budgets (G.5.2, G.8).
pub mod caps {
    /// Accepted posts per minute, board-wide.
    pub const POSTS_PER_MIN: u32 = 120;
    /// One new thread per this many seconds, board-wide (tightens under pressure).
    pub const THREAD_EVERY_S: u64 = 120;
    pub const THREAD_EVERY_MAX_S: u64 = 30 * 60;
    /// Submissions admitted between two publishes.
    pub const RING: usize = 64;
    /// Replay table slots: > two epochs at the maximum post rate (600/min × 20 min).
    pub const REPLAY: usize = 16_384;
    /// Adaptive effort: ×2 per step, at most ×64.
    pub const MAX_SHIFT: u32 = 6;
    /// R8: new threads close when the thread budget stays exhausted this long, or the posts cap
    /// stays saturated this long.
    pub const PANIC_THREADS_S: u64 = 30 * 60;
    pub const PANIC_POSTS_S: u64 = 10 * 60;
}

#[derive(Copy, Clone)]
struct Slot {
    key: [u8; 16],
    epoch: u32,
    /// The body hash, for an idempotent answer.
    h: [u8; 32],
    /// The published number and record sequence; 0 while in flight.
    no: u64,
    seq: u64,
}

const EMPTY: Slot = Slot { key: [0; 16], epoch: 0, h: [0; 32], no: 0, seq: 0 };

/// The number a held post is answered with (pre-moderation): no number yet. The gateway sends 0.
pub const HELD: u64 = u64::MAX;

/// A submission admitted to the publish ring.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct Ticket {
    pub effort: u32,
    pub replay_slot: u32,
    pub id: u64,
}

/// What the header check decided.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum Next {
    /// Read the body and call [`Intake::check_body`].
    ReadBody(u32),
    /// Already published: answer `{no, seq}` without reading the body.
    Done { no: u64, seq: u64 },
}

/// The `/pow` answer (G.6.1 step 1), 58 bytes little-endian.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct PowInfo {
    pub epoch: u32,
    pub seed: [u8; 32],
    pub effort_thread: u32,
    pub effort_reply: u32,
    pub min_thread: u32,
    pub min_reply: u32,
    pub paused: bool,
    pub threads_open: bool,
    /// The poster needs a trip the board knows (or an approved one).
    pub trips_only: bool,
    /// Only owner-approved trips may post.
    pub approved_only: bool,
    /// Posts are held until the owner approves them.
    pub premod: bool,
}

impl PowInfo {
    /// epoch 4, seed 32, four efforts 16, paused 1, threads open 1, switches 1 (bit 0 trips
    /// only, bit 1 approved trips only, bit 2 pre-moderation), reserved 3.
    pub const LEN: usize = 58;

    pub fn write(&self, out: &mut [u8; Self::LEN]) {
        out[0..4].copy_from_slice(&self.epoch.to_le_bytes());
        out[4..36].copy_from_slice(&self.seed);
        out[36..40].copy_from_slice(&self.effort_thread.to_le_bytes());
        out[40..44].copy_from_slice(&self.effort_reply.to_le_bytes());
        out[44..48].copy_from_slice(&self.min_thread.to_le_bytes());
        out[48..52].copy_from_slice(&self.min_reply.to_le_bytes());
        out[52] = u8::from(self.paused);
        out[53] = u8::from(self.threads_open);
        out[54] = u8::from(self.trips_only) | u8::from(self.approved_only) << 1 | u8::from(self.premod) << 2;
        out[55..58].fill(0);
    }

    pub fn read(b: &[u8; Self::LEN]) -> Self {
        let u = |at: usize| u32::from_le_bytes([b[at], b[at + 1], b[at + 2], b[at + 3]]);
        let mut seed = [0u8; 32];
        seed.copy_from_slice(&b[4..36]);
        PowInfo { epoch: u(0), seed, effort_thread: u(36), effort_reply: u(40), min_thread: u(44), min_reply: u(48), paused: b[52] != 0, threads_open: b[53] != 0, trips_only: b[54] & 1 != 0, approved_only: b[54] & 2 != 0, premod: b[54] & 4 != 0 }
    }
}

pub struct Intake {
    name: [u8; pow::MAX_NAME],
    name_len: usize,
    name_text: String,
    secret: [u8; 32],
    pub base: Efforts,
    /// Adaptive multiplier: efforts are `base << shift`.
    shift: u32,
    /// The lowest efforts advertised in the current and the previous epoch (the grace, G.8):
    /// `(epoch, thread, reply)`.
    adv: [(u32, u32, u32); 2],
    pub paused: bool,
    /// R8: closed automatically when flooded, reopened by the owner.
    pub threads_closed: bool,
    /// Set when the board closed itself; the app tells the owner and clears it.
    pub closed_notice: bool,
    /// The owner's other switches (G.9.1); the host checks them against the post (`Host`).
    pub trips_only: bool,
    pub approved_only: bool,
    pub premod: bool,
    /// R8's panic mode turns on trips-only instead of closing new threads.
    pub panic_trips: bool,
    replay: Box<[Slot]>,
    ring: [Option<Ticket>; caps::RING],
    next_id: u64,
    // Posts-per-minute window and thread budget.
    minute: u64,
    posts_this_minute: u32,
    next_thread_at: u64,
    thread_every: u64,
    saturated_posts_since: Option<u64>,
    saturated_threads_since: Option<u64>,
    /// A refusal for each cap during the current minute: saturation means one every minute.
    refused_posts: bool,
    refused_threads: bool,
    calm_since: u64,
}

impl Intake {
    /// `name`: the board's IPNS name; `secret`: its PoW secret (G.4).
    pub fn new(name: &ephem_channel::cid::Cid, secret: [u8; 32], base: Efforts, now_s: u64) -> Self {
        let bytes = name.to_bytes();
        let mut n = [0u8; pow::MAX_NAME];
        n[..bytes.len()].copy_from_slice(&bytes);
        let e = pow::epoch(now_s);
        Intake {
            name: n,
            name_len: bytes.len(),
            name_text: name.to_text(),
            secret,
            base,
            shift: 0,
            adv: [(e, base.thread, base.reply); 2],
            paused: false,
            threads_closed: false,
            closed_notice: false,
            trips_only: false,
            approved_only: false,
            premod: false,
            panic_trips: false,
            replay: vec![EMPTY; caps::REPLAY].into_boxed_slice(),
            ring: [None; caps::RING],
            next_id: 1,
            minute: now_s / 60,
            posts_this_minute: 0,
            next_thread_at: 0,
            thread_every: caps::THREAD_EVERY_S,
            saturated_posts_since: None,
            saturated_threads_since: None,
            refused_posts: false,
            refused_threads: false,
            calm_since: now_s,
        }
    }

    fn effort_now(&self, k: u8) -> u32 {
        let base = if k == kind::THREAD { self.base.thread } else { self.base.reply };
        base.saturating_mul(1 << self.shift)
    }

    fn advertise(&mut self, now_s: u64) {
        let e = pow::epoch(now_s);
        let (t, r) = (self.effort_now(kind::THREAD), self.effort_now(kind::REPLY));
        if self.adv[0].0 != e {
            self.adv[1] = self.adv[0];
            self.adv[0] = (e, t, r);
        } else {
            self.adv[0].1 = self.adv[0].1.min(t);
            self.adv[0].2 = self.adv[0].2.min(r);
        }
    }

    /// The `/pow` answer at `now_s`.
    pub fn pow_info(&mut self, now_s: u64) -> PowInfo {
        self.advertise(now_s);
        let e = pow::epoch(now_s);
        let prev = if self.adv[1].0 + 1 == e { self.adv[1] } else { self.adv[0] };
        PowInfo {
            epoch: e,
            seed: pow::seed(&self.secret, e),
            effort_thread: self.effort_now(kind::THREAD),
            effort_reply: self.effort_now(kind::REPLY),
            min_thread: self.adv[0].1.min(prev.1),
            min_reply: self.adv[0].2.min(prev.2),
            paused: self.paused,
            threads_open: !self.threads_closed,
            trips_only: self.trips_only,
            approved_only: self.approved_only,
            premod: self.premod,
        }
    }

    /// The lowest effort accepted for `k` in `epoch`: what was advertised then (grace, G.8).
    fn grace_min(&self, k: u8, epoch: u32) -> Option<u32> {
        let a = self.adv.iter().find(|a| a.0 == epoch)?;
        Some(if k == kind::THREAD { a.1 } else { a.2 })
    }

    fn replay_key(h: &Header) -> [u8; 16] {
        let mut b = blake2::Blake2bVar::new(16).expect("16-byte output");
        b.update(&h.k);
        b.update(&h.n);
        b.update(&h.solution);
        b.update(&h.epoch.to_le_bytes());
        let mut k = [0u8; 16];
        b.finalize_variable(&mut k).expect("16-byte output");
        k
    }

    /// Linear probing over the fixed table; slots of epochs older than the previous one are free.
    fn replay_find(&self, key: &[u8; 16], cur: u32) -> Result<usize, Option<usize>> {
        let n = self.replay.len();
        let start = u64::from_le_bytes([key[0], key[1], key[2], key[3], key[4], key[5], key[6], key[7]]) as usize % n;
        let mut free = None;
        for i in 0..n.min(64) {
            let j = (start + i) % n;
            let s = &self.replay[j];
            let live = s.epoch + 1 >= cur && s.key != [0; 16];
            if live && s.key == *key {
                return Ok(j);
            }
            if !live && free.is_none() {
                free = Some(j);
            }
        }
        Err(free)
    }

    /// Step 1–3: the header (G.6.2). `content_length` is what the request head said.
    pub fn check_header(&mut self, h: &Header, content_length: usize, now_s: u64) -> Result<Next, Refusal> {
        self.roll(now_s);
        if content_length != h.total_len() || h.kind == kind::SELF_DELETE || h.kind == kind::REPORT || h.kind == kind::ACTION {
            return Err(Refusal::Refused); // actions arrive with moderation (BD-5)
        }
        if self.paused || (h.kind == kind::THREAD && self.threads_closed) {
            return Err(Refusal::Paused);
        }
        let cur = pow::epoch(now_s);
        if h.epoch != cur && h.epoch + 1 != cur {
            return Err(Refusal::Pow);
        }
        if h.effort < self.grace_min(h.kind, h.epoch).unwrap_or(u32::MAX) {
            return Err(Refusal::Pow);
        }
        let key = Self::replay_key(h);
        let free = match self.replay_find(&key, cur) {
            Ok(j) => {
                let s = self.replay[j];
                return if s.h == h.h && s.no != 0 { Ok(Next::Done { no: s.no, seq: s.seq }) } else { Err(Refusal::Refused) };
            }
            Err(free) => free.ok_or(Refusal::Busy)?,
        };
        let c = Challenge::new(&self.name[..self.name_len], &pow::seed(&self.secret, h.epoch), h.kind, h.thread, &h.k, &h.n, h.effort).ok_or(Refusal::Refused)?;
        if !pow::verify(&c, &h.solution, h.effort) {
            return Err(Refusal::Pow);
        }
        // Spent now, before the body is read: one valid header serves one upload.
        self.replay[free] = Slot { key, epoch: h.epoch, h: h.h, no: 0, seq: 0 };
        Ok(Next::ReadBody(free as u32))
    }

    /// Step 4: the body (`s`, then image bytes in v2) against the header. The decoded `s` is the
    /// only allocation here (≤ 2.4 KiB, bounded by the header's length).
    pub fn check_body(&self, h: &Header, body: &[u8]) -> Result<Signed, Refusal> {
        if body.len() + HEADER_LEN != h.total_len() || body_hash(body) != h.h {
            return Err(Refusal::Refused);
        }
        let s = Value::decode(&body[..h.text_len as usize]).ok_or(Refusal::Refused).and_then(|v| Signed::from_value(&v).map_err(|_| Refusal::Refused))?;
        let sage = h.flags & flag::SAGE != 0;
        if s.k != h.k || s.n != h.n || s.t != h.thread || s.e != h.epoch || s.sage != sage || s.b != self.name_text {
            return Err(Refusal::Refused);
        }
        s.verify(&h.sig).map_err(|_| Refusal::Refused)?;
        Ok(s)
    }

    /// Step 5: caps and the publish ring. Returns the ticket, and a ticket evicted to make room
    /// (its submitter gets `Busy`).
    pub fn admit(&mut self, h: &Header, replay_slot: u32, now_s: u64) -> Result<(Ticket, Option<Ticket>), Refusal> {
        self.roll(now_s);
        if self.posts_this_minute >= caps::POSTS_PER_MIN {
            self.saturated_posts_since.get_or_insert(now_s);
            self.refused_posts = true;
            return Err(Refusal::Busy);
        }
        if h.kind == kind::THREAD && now_s < self.next_thread_at {
            self.saturated_threads_since.get_or_insert(now_s);
            self.refused_threads = true;
            return Err(Refusal::Busy);
        }
        let t = Ticket { effort: h.effort, replay_slot, id: self.next_id };
        let evicted = match self.ring.iter().position(Option::is_none) {
            Some(i) => {
                self.ring[i] = Some(t);
                None
            }
            None => {
                // Full: the lowest effort leaves, if this one paid more (G.8).
                let (i, low) = self.ring.iter().enumerate().filter_map(|(i, x)| x.map(|x| (i, x))).min_by_key(|(_, x)| x.effort).expect("a full ring");
                if low.effort >= h.effort {
                    return Err(Refusal::Busy);
                }
                self.ring[i] = Some(t);
                Some(low)
            }
        };
        self.next_id += 1;
        self.posts_this_minute += 1;
        if h.kind == kind::THREAD {
            self.next_thread_at = now_s + self.thread_every;
        }
        Ok((t, evicted))
    }

    /// The admitted tickets, highest effort first, emptying the ring (the publish batch).
    pub fn drain(&mut self, out: &mut [Option<Ticket>; caps::RING]) -> usize {
        let mut n = 0;
        for x in self.ring.iter_mut() {
            if let Some(t) = x.take() {
                out[n] = Some(t);
                n += 1;
            }
        }
        out[..n].sort_unstable_by(|a, b| b.map(|t| t.effort).cmp(&a.map(|t| t.effort)));
        n
    }

    /// Records what a published ticket became (for idempotent retries). A held post (pre-
    /// moderation) is recorded as [`HELD`].
    pub fn published(&mut self, t: &Ticket, no: u64, seq: u64) {
        if let Some(s) = self.replay.get_mut(t.replay_slot as usize) {
            s.no = no;
            s.seq = seq;
        }
    }

    /// A new minute: a cap that refused nobody in the minute that ended is no longer saturated.
    fn roll(&mut self, now_s: u64) {
        let m = now_s / 60;
        if m != self.minute {
            if !self.refused_posts {
                self.saturated_posts_since = None;
            }
            if !self.refused_threads {
                self.saturated_threads_since = None;
            }
            (self.refused_posts, self.refused_threads) = (false, false);
            self.minute = m;
            self.posts_this_minute = 0;
        }
    }

    /// Once a minute: adaptive effort (×2 under pressure, ÷2 after 10 calm minutes), the thread
    /// budget tightening, and R8's automatic close of new threads.
    pub fn tick(&mut self, now_s: u64) {
        self.roll(now_s);
        let pressed = self.saturated_posts_since.is_some() || self.saturated_threads_since.is_some() || self.posts_this_minute * 2 > caps::POSTS_PER_MIN;
        if pressed {
            self.shift = (self.shift + 1).min(caps::MAX_SHIFT);
            self.calm_since = now_s;
            if self.saturated_threads_since.is_some() {
                self.thread_every = (self.thread_every * 2).min(caps::THREAD_EVERY_MAX_S);
            }
        } else if now_s.saturating_sub(self.calm_since) >= 600 {
            self.shift = self.shift.saturating_sub(1);
            self.thread_every = caps::THREAD_EVERY_S.max(self.thread_every / 2);
            self.calm_since = now_s;
        }
        let flooded_threads = self.saturated_threads_since.is_some_and(|t| now_s - t >= caps::PANIC_THREADS_S);
        let flooded_posts = self.saturated_posts_since.is_some_and(|t| now_s - t >= caps::PANIC_POSTS_S);
        if flooded_threads || flooded_posts {
            let (switch, other) = if self.panic_trips { (&mut self.trips_only, self.threads_closed) } else { (&mut self.threads_closed, self.trips_only) };
            if !*switch && !other {
                *switch = true;
                self.closed_notice = true;
            }
        }
        self.advertise(now_s);
    }

    /// More than half the posts cap used this minute: publishing slows to once every 5 s
    /// (G.5.3).
    pub fn busy(&self) -> bool {
        self.posts_this_minute * 2 > caps::POSTS_PER_MIN
    }

    /// The owner reopens new threads (after R8 closed them).
    pub fn reopen_threads(&mut self) {
        self.threads_closed = false;
        self.saturated_threads_since = None;
        self.saturated_posts_since = None;
        self.thread_every = caps::THREAD_EVERY_S;
    }
}

/// The poster's side: a header and body for `s`, signed with `key`, given a solved nonce. (The
/// app solves in Workers with [`pow::attempt`] while the user types, then builds `s` with the
/// nonce that solved.)
pub fn submission(s: &Signed, key: &ed25519_dalek::SigningKey, effort: u32, solution: [u8; pow::SOLUTION_LEN]) -> Result<Vec<u8>, BoardError> {
    let text = s.to_value().encode();
    if text.len() > crate::submit::MAX_TEXT {
        return Err(BoardError::TooLong);
    }
    let h = Header {
        kind: if s.t == 0 { kind::THREAD } else { kind::REPLY },
        flags: if s.sage { flag::SAGE } else { 0 },
        text_len: text.len() as u16,
        img_len: 0,
        epoch: s.e,
        effort,
        thread: s.t,
        k: s.k,
        n: s.n,
        h: body_hash(&text),
        solution,
        sig: s.sign(key),
    };
    let mut out = vec![0u8; HEADER_LEN + text.len()];
    let head: &mut [u8; HEADER_LEN] = (&mut out[..HEADER_LEN]).try_into().expect("188 bytes");
    h.write(head);
    out[HEADER_LEN..].copy_from_slice(&text);
    Ok(out)
}
