// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
// Copyright 2026 Anton (darkcite)
//! The single writer (G.3): the owner's [`Board`], its [`Intake`] and what its onion serves
//! ([`Served`]), sans-IO. The browser drives it: each submit stream calls
//! [`Host::submit_header`], then [`Host::submit_body`], then waits for [`Host::answer`]; a timer
//! calls [`Host::publish`] when [`Host::due`] (at most once a second, once every 5 s while busy,
//! and at least once a day so the 72 h record never lapses while the tab is open).
//!
//! Each publish builds only what changed ([`Board::build_into`]), signs the next record and
//! leaves a [`Delta`] for the page's block store (G.5.3: one file per block, unreachable ones
//! deleted).
//!
//! **Moderation (G.9, BD-5)** is the owner's, applied here (no replay surface): deletes wait
//! [`UNDO_MS`] for an undo; mass delete (since a time, all posts of a key); lock, sticky, prune;
//! bans; the switches (pause, new threads closed, trips-only, approved trips only,
//! pre-moderation, the panic mode's choice); held posts; the public mod log. Bans, approvals,
//! known trips, held posts, switches and efforts live in the encrypted `own` block
//! ([`crate::own`]), so a reopen keeps them.

use crate::board::{Board, Entry, cap, del};
use crate::gateway::Served;
use crate::own::{self, Held, Own, Switches};
use crate::pipeline::{HELD, Intake, Next, Refusal, Ticket, caps};
use crate::post::Signed;
use crate::submit::{HEADER_LEN, Header, kind};
use crate::BoardError;
use ephem_channel::car::Block;
use ephem_channel::cid::Cid;

/// Publishing at most this often (G.5.3), and this often while more than half the cap is used.
pub const PUBLISH_MS: u64 = 1_000;
pub const PUBLISH_BUSY_MS: u64 = 5_000;
/// Re-sign an unchanged board after this (records are valid 72 h, R1).
pub const REPUBLISH_MS: u64 = 24 * 3600 * 1000;
/// An owner delete waits this long for an undo before it is published (G.9.1).
pub const UNDO_MS: u64 = 5_000;
/// Answers kept for submitters still waiting.
const ANSWERS: usize = 2 * caps::RING;

/// A post admitted, waiting for the next publish.
struct Pending {
    id: u64,
    ticket: Ticket,
    s: Signed,
    sig: [u8; 64],
    cap: u8,
}

/// What the store must write and delete since it last asked.
#[derive(Default, Debug)]
pub struct Delta {
    /// The latest signed record (empty: unchanged).
    pub record: Vec<u8>,
    pub added: Vec<Block>,
    pub removed: Vec<Cid>,
}

pub type Outcome = Result<(u64, u64), Refusal>;

pub struct Host {
    pub board: Board,
    pub intake: Intake,
    pub served: Served,
    pending: Vec<Pending>,
    answers: std::collections::VecDeque<(u64, Outcome)>,
    batch: [Option<Ticket>; caps::RING],
    last_ms: u64,
    /// Changed outside the intake (an owner post or action): publish at the next chance.
    dirty: bool,
    delta: Delta,
    /// Moderation state (sealed into the board's `own` block at each publish).
    pub own: Own,
    own_key: [u8; 32],
    own_dirty: bool,
    /// Owner deletes waiting for their undo window: `(no, publish at, ms)`.
    deletes: Vec<(u64, u64)>,
}

impl Host {
    /// Hosts `board` (new, or reopened from the store) and signs its first record here.
    /// `own_key` opens (and seals) the moderation state; a reopened board gets its bans,
    /// switches, efforts and held posts back from its `own` block.
    pub fn new(mut board: Board, mut intake: Intake, own_key: [u8; 32], now_ms: u64) -> Self {
        let own = if board.own.is_empty() { Own { efforts: intake.base, ..Own::default() } } else { Own::open(&own_key, &board.manifest.pk, &board.own).unwrap_or_default() };
        apply_switches(&mut intake, &own);
        let mut nonce = [0u8; own::NONCE];
        nonce[..8].copy_from_slice(&now_ms.to_le_bytes());
        if let Some(b) = own.seal(&own_key, &board.manifest.pk, &nonce) {
            let _ = board.set_own(b);
        }
        let mut live = Vec::new();
        let (root, blocks) = board.build_into(now_ms / 1000, &|_| false, &mut live);
        let record = board.record(&root, now_ms);
        let delta = Delta { record: record.clone(), added: blocks.clone(), removed: Vec::new() };
        let served = Served::new(board.name().clone(), root, record, blocks).expect("the owner's own board");
        Self { board, intake, served, pending: Vec::with_capacity(caps::RING), answers: std::collections::VecDeque::with_capacity(ANSWERS), batch: [None; caps::RING], last_ms: now_ms, dirty: false, delta, own, own_key, own_dirty: false, deletes: Vec::new() }
    }

    /// G.6.2 steps 2–4 from the 188 header bytes: `(header, Next)` or the refusal.
    pub fn submit_header(&mut self, head: &[u8; HEADER_LEN], content_length: usize, now_s: u64) -> Result<(Header, Next), Refusal> {
        let h = Header::parse(head).ok_or(Refusal::Refused)?;
        match self.intake.check_header(&h, content_length, now_s)? {
            // A held post (pre-moderation) has no number yet: answered as 0.
            Next::Done { no: HELD, seq } => Ok((h, Next::Done { no: 0, seq })),
            n => Ok((h, n)),
        }
    }

    /// Steps 5–9: the body, then the publish ring. Returns the ticket to wait on.
    pub fn submit_body(&mut self, h: &Header, slot: u32, body: &[u8], now_s: u64) -> Result<u64, Refusal> {
        let s = self.intake.check_body(h, body)?;
        let cap = if h.kind == kind::CAPCODE { cap::OWNER } else { cap::ANON };
        if cap == cap::OWNER && s.k != self.board.manifest.pk {
            return Err(Refusal::Refused);
        }
        if s.t != 0 && !self.board.threads.iter().any(|t| t.no == s.t && !t.locked) {
            return Err(Refusal::Refused); // step 6: a live, unlocked thread
        }
        if cap == cap::ANON {
            // Step 7: bans and the switches on keys (G.9.1).
            if self.own.bans.contains(&s.k) {
                return Err(Refusal::Refused);
            }
            if self.intake.approved_only && !self.own.approved.contains(&s.k) {
                return Err(Refusal::Paused);
            }
            if self.intake.trips_only && !(s.trip && self.own.trip_ok(&s.k)) {
                return Err(Refusal::Paused);
            }
        }
        let (ticket, evicted) = self.intake.admit(h, slot, now_s)?;
        if let Some(e) = evicted {
            self.pending.retain(|p| p.id != e.id);
            self.answered(e.id, Err(Refusal::Busy));
        }
        self.pending.push(Pending { id: ticket.id, ticket, s, sig: h.sig, cap });
        Ok(ticket.id)
    }

    fn answered(&mut self, id: u64, o: Outcome) {
        if self.answers.len() == ANSWERS {
            self.answers.pop_front();
        }
        self.answers.push_back((id, o));
    }

    /// The outcome of ticket `id`, once published (taken: asked once).
    pub fn answer(&mut self, id: u64) -> Option<Outcome> {
        let i = self.answers.iter().position(|(x, _)| *x == id)?;
        self.answers.remove(i).map(|(_, o)| o)
    }

    /// A post by the owner, signed with the board key (capcode), straight in: no proof of work.
    pub fn post_owner(&mut self, thread: u64, sub: &str, body: &str, sage: bool, now_s: u64) -> Result<u64, BoardError> {
        let mut n = [0u8; 16];
        n[..8].copy_from_slice(&now_s.to_le_bytes());
        n[8..].copy_from_slice(&self.board.next_no.to_le_bytes());
        let s = Signed { b: self.board.name().to_text(), t: thread, k: self.board.manifest.pk, n, sub: sub.into(), body: body.into(), sage, e: crate::pow::epoch(now_s), trip: false };
        let sig = s.sign(self.board.signing_key());
        let no = self.board.accept(s, sig, cap::OWNER, now_s)?;
        self.dirty = true;
        Ok(no)
    }

    fn post_of(&self, no: u64) -> Option<(&crate::board::Thread, &crate::board::Post)> {
        self.board.threads.iter().find_map(|t| {
            t.entries.iter().find_map(|e| match e {
                Entry::Post(p) if p.no == no => Some((t, p)),
                _ => None,
            })
        })
    }

    /// The owner deletes post `no` (an OP takes its thread with it), published after
    /// [`UNDO_MS`] unless undone.
    pub fn delete(&mut self, no: u64, now_ms: u64) -> Result<(), BoardError> {
        self.post_of(no).ok_or(BoardError::NotFound)?;
        if !self.deletes.iter().any(|(n, _)| *n == no) {
            self.deletes.push((no, now_ms + UNDO_MS));
        }
        Ok(())
    }

    /// Mass delete (G.9.1): every live post with `ts ≥ since_s` (minute-rounded times), or every
    /// post signed by `key`. Returns how many were queued (each with the undo window).
    pub fn delete_since(&mut self, since_s: u64, now_ms: u64) -> usize {
        self.delete_where(|p| p.ts >= since_s, now_ms)
    }

    /// Mass delete from post No. `no` on (exact, where `ts` is rounded to the minute).
    pub fn delete_from(&mut self, no: u64, now_ms: u64) -> usize {
        self.delete_where(|p| p.no >= no, now_ms)
    }

    pub fn delete_by_key(&mut self, key: &[u8; 32], now_ms: u64) -> usize {
        self.delete_where(|p| p.s.k == *key, now_ms)
    }

    fn delete_where(&mut self, f: impl Fn(&crate::board::Post) -> bool, now_ms: u64) -> usize {
        let nos: Vec<u64> = self.board.threads.iter().flat_map(|t| t.entries.iter()).filter_map(|e| if let Entry::Post(p) = e && f(p) { Some(p.no) } else { None }).collect();
        nos.iter().filter(|no| self.delete(**no, now_ms).is_ok()).count()
    }

    /// Undoes pending deletes (`no` = 0: all of them). Returns how many.
    pub fn undo(&mut self, no: u64) -> usize {
        let before = self.deletes.len();
        self.deletes.retain(|(n, _)| no != 0 && *n != no);
        before - self.deletes.len()
    }

    /// The deletes still waiting for their undo window.
    pub fn pending_deletes(&self) -> Vec<u64> {
        self.deletes.iter().map(|(n, _)| *n).collect()
    }

    fn logged(&mut self, now_s: u64, act: &str, no: u64, why: &str) {
        self.board.log(now_s, act, no, why);
        self.dirty = true;
    }

    pub fn set_locked(&mut self, no: u64, on: bool, now_s: u64) -> Result<(), BoardError> {
        self.board.set_locked(no, on)?;
        self.logged(now_s, if on { "lock" } else { "unlock" }, no, "");
        Ok(())
    }

    pub fn set_sticky(&mut self, no: u64, on: bool, now_s: u64) -> Result<(), BoardError> {
        self.board.set_sticky(no, on)?;
        self.logged(now_s, if on { "sticky" } else { "unsticky" }, no, "");
        Ok(())
    }

    /// Moves a thread to the archive now.
    pub fn prune(&mut self, no: u64, now_s: u64) -> Result<(), BoardError> {
        self.board.prune(no, now_s)?;
        self.logged(now_s, "prune", no, "");
        Ok(())
    }

    /// Bans the key of post `no` (G.9.1: meaningful for trips; with IDs off every post has a
    /// fresh key, which the UI says).
    pub fn ban(&mut self, no: u64, why: &str, now_s: u64) -> Result<[u8; 32], BoardError> {
        let k = self.post_of(no).ok_or(BoardError::NotFound)?.1.s.k;
        if k == self.board.manifest.pk {
            return Err(BoardError::Refused);
        }
        if !self.own.bans.contains(&k) {
            if self.own.bans.len() == own::BANS {
                self.own.bans.remove(0);
            }
            self.own.bans.push(k);
        }
        self.own_dirty = true;
        self.logged(now_s, "ban", no, why);
        Ok(k)
    }

    pub fn unban(&mut self, key: &[u8; 32]) {
        self.own.bans.retain(|k| k != key);
        self.own_dirty = true;
        self.dirty = true;
    }

    /// Approves a trip key (the approved-trips switch admits only these).
    pub fn approve_trip(&mut self, key: &[u8; 32], on: bool) {
        self.own.approved.retain(|k| k != key);
        if on {
            if self.own.approved.len() == own::APPROVED {
                self.own.approved.remove(0);
            }
            self.own.approved.push(*key);
        }
        self.own_dirty = true;
        self.dirty = true;
    }

    /// The owner's switches now.
    pub fn switches(&self) -> Switches {
        let i = &self.intake;
        Switches { paused: i.paused, threads_closed: i.threads_closed, trips_only: i.trips_only, approved_only: i.approved_only, premod: i.premod, panic_trips: i.panic_trips }
    }

    pub fn set_switches(&mut self, sw: Switches) {
        if !sw.threads_closed && self.intake.threads_closed {
            self.intake.reopen_threads();
        }
        self.own.sw = sw;
        apply_switches(&mut self.intake, &self.own);
        self.own_dirty = true;
        self.dirty = true;
    }

    pub fn set_efforts(&mut self, e: crate::pipeline::Efforts) {
        self.intake.base = e;
        self.own.efforts = e;
        self.own_dirty = true;
        self.dirty = true;
    }

    /// Approves held post `i` (pre-moderation): it is numbered now. Returns its number.
    pub fn approve(&mut self, i: usize, now_s: u64) -> Result<u64, BoardError> {
        let h = self.own.held.get(i).cloned().ok_or(BoardError::NotFound)?;
        let trip = h.s.trip.then_some(h.s.k);
        let no = self.board.accept(h.s, h.sig, h.cap, now_s)?;
        self.own.held.remove(i);
        if let Some(k) = trip {
            self.own.saw_trip(&k);
        }
        self.own_dirty = true;
        self.logged(now_s, "approve", no, "");
        Ok(no)
    }

    /// Drops held post `i` unpublished.
    pub fn reject(&mut self, i: usize) -> Result<(), BoardError> {
        if i >= self.own.held.len() {
            return Err(BoardError::NotFound);
        }
        self.own.held.remove(i);
        self.own_dirty = true;
        self.dirty = true;
        Ok(())
    }

    /// Whether a publish is due now.
    pub fn due(&self, now_ms: u64) -> bool {
        let gap = now_ms.saturating_sub(self.last_ms);
        let min = if self.intake.busy() { PUBLISH_BUSY_MS } else { PUBLISH_MS };
        let deletes = self.deletes.iter().any(|(_, at)| *at <= now_ms);
        ((!self.pending.is_empty() || self.dirty || deletes) && gap >= min) || gap >= REPUBLISH_MS
    }

    /// Takes the admitted posts (highest effort first), builds what changed, signs the next
    /// record and answers every submitter. Returns the record's sequence.
    pub fn publish(&mut self, now_ms: u64) -> u64 {
        let now_s = now_ms / 1000;
        let n = self.intake.drain(&mut self.batch);
        let mut done: Vec<(Ticket, Outcome)> = Vec::with_capacity(n);
        for i in 0..n {
            let Some(t) = self.batch[i].take() else { continue };
            let Some(j) = self.pending.iter().position(|p| p.id == t.id) else { continue };
            let p = self.pending.swap_remove(j);
            if self.intake.premod && p.cap == cap::ANON {
                // Held for the owner (pre-moderation): no number yet.
                let r = if self.own.held.len() < own::HELD {
                    self.own.held.push(Held { s: p.s, sig: p.sig, cap: p.cap, at: now_s });
                    self.own_dirty = true;
                    Ok((HELD, 0))
                } else {
                    Err(Refusal::Busy)
                };
                done.push((p.ticket, r));
                continue;
            }
            let trip = p.s.trip.then_some(p.s.k);
            let r = self.board.accept(p.s, p.sig, p.cap, now_s).map_err(|e| match e {
                BoardError::Busy => Refusal::Busy,
                _ => Refusal::Refused,
            });
            if let (Ok(_), Some(k)) = (&r, trip) {
                self.own.saw_trip(&k);
                self.own_dirty = true;
            }
            done.push((p.ticket, r.map(|no| (no, 0))));
        }
        // Owner deletes whose undo window has passed.
        let mut i = 0;
        while i < self.deletes.len() {
            let (no, at) = self.deletes[i];
            if at > now_ms {
                i += 1;
                continue;
            }
            self.deletes.swap_remove(i);
            if self.board.delete(no, del::OWNER, 0, now_s).is_ok() {
                self.board.log(now_s, "delete", no, "");
            }
        }
        // R8 may have switched something on in a tick: keep the sealed copy in step.
        let sw = self.switches();
        if sw != self.own.sw {
            self.own.sw = sw;
            self.own_dirty = true;
        }
        if self.own_dirty {
            let mut nonce = [0u8; own::NONCE];
            nonce[..8].copy_from_slice(&now_ms.to_le_bytes());
            nonce[8..16].copy_from_slice(&self.board.seq.to_le_bytes());
            if let Some(b) = self.own.seal(&self.own_key, &self.board.manifest.pk, &nonce) {
                let _ = self.board.set_own(b);
            }
            self.own_dirty = false;
        }
        let mut live = Vec::with_capacity(self.served.blocks().size_hint().0);
        let served = &self.served;
        let (root, added) = self.board.build_into(now_s, &|c| served.block(c).is_some(), &mut live);
        let record = self.board.record(&root, now_ms);
        let seq = self.board.seq;
        for (c, _) in &added {
            self.delta.removed.retain(|x| x != c);
        }
        self.delta.added.extend(added.iter().cloned());
        let gone = self.served.update(root, record.clone(), added, &live).expect("the owner's own board");
        for c in gone {
            self.delta.added.retain(|(x, _)| *x != c);
            self.delta.removed.push(c);
        }
        self.delta.record = record;
        for (t, r) in done {
            let r = r.map(|(no, _)| (no, seq));
            if let Ok((no, seq)) = r {
                self.intake.published(&t, no, seq);
            }
            // A held post is answered with number 0.
            self.answered(t.id, r.map(|(no, seq)| (if no == HELD { 0 } else { no }, seq)));
        }
        self.last_ms = now_ms;
        self.dirty = false;
        seq
    }

    /// What the store must write and delete since the last call.
    pub fn take_delta(&mut self) -> Delta {
        std::mem::take(&mut self.delta)
    }
}

fn apply_switches(intake: &mut Intake, own: &Own) {
    let sw = own.sw;
    (intake.paused, intake.threads_closed, intake.trips_only, intake.approved_only, intake.premod, intake.panic_trips) = (sw.paused, sw.threads_closed, sw.trips_only, sw.approved_only, sw.premod, sw.panic_trips);
    intake.base = own.efforts;
}
