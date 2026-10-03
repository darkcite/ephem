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

use crate::board::{Board, cap};
use crate::gateway::Served;
use crate::pipeline::{Intake, Next, Refusal, Ticket, caps};
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
}

impl Host {
    /// Hosts `board` (new, or reopened from the store) and signs its first record here.
    pub fn new(mut board: Board, intake: Intake, now_ms: u64) -> Self {
        let mut live = Vec::new();
        let (root, blocks) = board.build_into(now_ms / 1000, &|_| false, &mut live);
        let record = board.record(&root, now_ms);
        let delta = Delta { record: record.clone(), added: blocks.clone(), removed: Vec::new() };
        let served = Served::new(board.name().clone(), root, record, blocks).expect("the owner's own board");
        Self { board, intake, served, pending: Vec::with_capacity(caps::RING), answers: std::collections::VecDeque::with_capacity(ANSWERS), batch: [None; caps::RING], last_ms: now_ms, dirty: false, delta }
    }

    /// G.6.2 steps 2–4 from the 188 header bytes: `(header, Next)` or the refusal.
    pub fn submit_header(&mut self, head: &[u8; HEADER_LEN], content_length: usize, now_s: u64) -> Result<(Header, Next), Refusal> {
        let h = Header::parse(head).ok_or(Refusal::Refused)?;
        self.intake.check_header(&h, content_length, now_s).map(|n| (h, n))
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
        let s = Signed { b: self.board.name().to_text(), t: thread, k: self.board.manifest.pk, n, sub: sub.into(), body: body.into(), sage, e: crate::pow::epoch(now_s) };
        let sig = s.sign(self.board.signing_key());
        let no = self.board.accept(s, sig, cap::OWNER, now_s)?;
        self.dirty = true;
        Ok(no)
    }

    /// The owner deletes post `no` (an OP takes its thread with it).
    pub fn delete(&mut self, no: u64, now_s: u64) -> Result<(), BoardError> {
        self.board.delete(no, crate::board::del::OWNER, 0, now_s)?;
        self.dirty = true;
        Ok(())
    }

    /// Whether a publish is due now.
    pub fn due(&self, now_ms: u64) -> bool {
        let gap = now_ms.saturating_sub(self.last_ms);
        let min = if self.intake.busy() { PUBLISH_BUSY_MS } else { PUBLISH_MS };
        ((!self.pending.is_empty() || self.dirty) && gap >= min) || gap >= REPUBLISH_MS
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
            let r = self.board.accept(p.s, p.sig, p.cap, now_s).map_err(|e| match e {
                BoardError::Busy => Refusal::Busy,
                _ => Refusal::Refused,
            });
            done.push((p.ticket, r.map(|no| (no, 0))));
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
            self.answered(t.id, r);
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
