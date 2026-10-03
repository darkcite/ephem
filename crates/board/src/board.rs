// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
// Copyright 2026 Anton (darkcite)
//! The owner's board (G.5): the single writer. It numbers posts, bumps and prunes threads,
//! applies moderation, and turns the state into dag-cbor blocks and a signed IPNS record.
//!
//! ```text
//! IPNS record ──▶ root {v, kind:"board", manifest⁴², cat:[bucket⁴² ×10], threads⁴², archive⁴²,
//!                       arch_threads⁴², dels⁴², modlog⁴², own⁴², ev: null, next_no, updated}
//!   bucket i     {t: [{no, thread (ref), bump, r, sub, ex, st, lk}]}   threads with no mod 10 = i
//!   thread       {no, sub, chunks: [chunk⁴² …] ≤ 8, r, st, lk}
//!   chunk        {p: [post …] ≤ 64}
//!   post         {no, ts, s, sig, cap}  or  tombstone {no, ts, del, by}
//! ```
//!
//! Pin links (⁴²) are followed for pinning and garbage collection; refs (CID bytes) are not.
//! A full chunk keeps its CID until a post in it is deleted, so a reply re-encodes only the
//! last chunk, its thread, one catalog bucket, the `threads` index and the root.
//!
//! Setup and publish path (§22): it allocates; the per-request path is `crate::pipeline` (BD-2).

use crate::post::{self, Signed};
use crate::{BoardError, limits};
use ed25519_dalek::{Signer, SigningKey};
use ephem_channel::car::Block;
use ephem_channel::cbor::{self, Value};
use ephem_channel::cid::{Cid, DAG_CBOR, RAW};
use ephem_channel::ipns::{self, Record};
use sha2::{Digest, Sha256};

/// Who deleted a post (`del` of a tombstone).
pub mod del {
    pub const OWNER: u8 = 1;
    pub const JANITOR: u8 = 2;
    pub const POSTER: u8 = 3;
    pub const FILTER: u8 = 4;
}

/// Capcodes (`cap`): who the post is shown as.
pub mod cap {
    pub const ANON: u8 = 0;
    pub const OWNER: u8 = 1;
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Manifest {
    pub title: String,
    pub about: String,
    pub rules: String,
    pub pk: [u8; 32],
    pub created: u64,
    /// Mirror onions the owner vouches for (`<56 chars>.onion`).
    pub mirrors: Vec<String>,
    /// Other boards the owner recommends (names or links), shown as plain links (G.17, v1).
    pub see_also: Vec<String>,
    /// Poster IDs per thread (B5: off by default).
    pub ids: bool,
}

/// A live post: the poster's signed part plus what the host adds.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Post {
    pub no: u64,
    /// Unix seconds, rounded down to the minute (G.5.1: exact times would expose the host's clock).
    pub ts: u64,
    pub s: Signed,
    pub sig: [u8; 64],
    pub cap: u8,
}

// Posts and tombstones are kept inline, contiguous in a thread's vector: boxing the large variant
// would cost one heap allocation and one pointer chase per post (§22).
#[allow(clippy::large_enum_variant)]
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Entry {
    Post(Post),
    /// A deleted post: its number stays, its content is gone.
    Tomb { no: u64, ts: u64, del: u8, by: u8 },
}

impl Entry {
    pub fn no(&self) -> u64 {
        match self {
            Entry::Post(p) => p.no,
            Entry::Tomb { no, .. } => *no,
        }
    }

    pub(crate) fn to_value(&self) -> Value {
        match self {
            Entry::Post(p) => cbor::map(vec![
                ("no", Value::Uint(p.no)),
                ("ts", Value::Uint(p.ts)),
                ("s", p.s.to_value()),
                ("sig", Value::Bytes(p.sig.to_vec())),
                ("cap", Value::Uint(u64::from(p.cap))),
            ]),
            Entry::Tomb { no, ts, del, by } => cbor::map(vec![
                ("no", Value::Uint(*no)),
                ("ts", Value::Uint(*ts)),
                ("del", Value::Uint(u64::from(*del))),
                ("by", Value::Uint(u64::from(*by))),
            ]),
        }
    }
}

/// `SHA-256(dag-cbor(s))`: what the deletion list names (G.5.1 `dels`).
pub fn post_hash(s: &Signed) -> [u8; 32] {
    Sha256::digest(s.to_value().encode()).into()
}

#[derive(Clone, Debug)]
pub struct Thread {
    pub no: u64,
    pub sub: String,
    pub entries: Vec<Entry>,
    /// Last bump, Unix seconds (the OP's time until a reply bumps it).
    pub bump: u64,
    pub created: u64,
    pub sticky: bool,
    pub locked: bool,
    /// Encoded full chunks, in order; dropped from the first one that changed.
    full: Vec<Block>,
}

impl Thread {
    /// Replies (tombstones included): `r` of the catalog and thread blocks.
    pub fn replies(&self) -> u64 {
        self.entries.len() as u64 - 1
    }

    /// The OP's body, cut to `EXCERPT` bytes on a character boundary (empty once deleted).
    pub fn excerpt(&self) -> String {
        match &self.entries[0] {
            Entry::Post(p) => {
                let mut end = p.s.body.len().min(limits::EXCERPT);
                while !p.s.body.is_char_boundary(end) {
                    end -= 1;
                }
                p.s.body[..end].to_owned()
            }
            Entry::Tomb { .. } => String::new(),
        }
    }

    fn invalidate_from(&mut self, idx: usize) {
        self.full.truncate(idx / limits::CHUNK);
    }

    /// The chunk blocks (cached full ones, a fresh last one) and the thread block. Full chunks
    /// the caller already `held` are not copied out again; every CID goes to `live`.
    fn blocks(&mut self, out: &mut Vec<Block>, held: &dyn Fn(&Cid) -> bool, live: &mut Vec<Cid>) -> Cid {
        let n_full = self.entries.len() / limits::CHUNK;
        while self.full.len() < n_full {
            let i = self.full.len();
            self.full.push(chunk_block(&self.entries[i * limits::CHUNK..(i + 1) * limits::CHUNK]));
        }
        let mut links: Vec<Value> = Vec::with_capacity(n_full + 1);
        for (c, b) in &self.full {
            links.push(Value::Link(c.clone()));
            live.push(c.clone());
            if !held(c) {
                out.push((c.clone(), b.clone()));
            }
        }
        let rest = &self.entries[n_full * limits::CHUNK..];
        if !rest.is_empty() {
            let b = chunk_block(rest);
            links.push(Value::Link(b.0.clone()));
            live.push(b.0.clone());
            out.push(b);
        }
        let t = cbor::map(vec![
            ("no", Value::Uint(self.no)),
            ("sub", Value::Text(self.sub.clone())),
            ("chunks", Value::Array(links)),
            ("r", Value::Uint(self.replies())),
            ("st", Value::Bool(self.sticky)),
            ("lk", Value::Bool(self.locked)),
        ])
        .encode();
        let cid = Cid::of(DAG_CBOR, &t);
        live.push(cid.clone());
        out.push((cid.clone(), t));
        cid
    }
}

fn chunk_block(entries: &[Entry]) -> Block {
    let b = cbor::map(vec![("p", Value::Array(entries.iter().map(Entry::to_value).collect()))]).encode();
    (Cid::of(DAG_CBOR, &b), b)
}

/// A pruned thread kept as text for `ARCHIVE_S` (its blocks stay pinned by `arch_threads`).
#[derive(Clone, Debug)]
pub struct Archived {
    pub no: u64,
    pub sub: String,
    pub ex: String,
    pub pruned: u64,
    pub thread: Cid,
    blocks: Vec<Block>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ModEntry {
    pub ts: u64,
    pub act: String,
    pub no: u64,
    pub why: String,
}

/// The owner's board.
pub struct Board {
    key: SigningKey,
    name: Cid,
    pub manifest: Manifest,
    pub threads: Vec<Thread>,
    pub archive: Vec<Archived>,
    /// `(post hash, deleted at)` (G.5.1).
    pub dels: Vec<([u8; 32], u64)>,
    pub modlog: Vec<ModEntry>,
    /// The encrypted owner-state block (G.5.1 `own`), opaque here.
    pub own: Vec<u8>,
    pub next_no: u64,
    /// The last record's sequence.
    pub seq: u64,
}

impl Board {
    /// A new board for the board key `sign_seed` (`HKDF(seed, "p2pchat/board/" ‖ index)`, G.4).
    pub fn new(sign_seed: &[u8; 32], title: &str, about: &str, rules: &str, created: u64) -> Result<Self, BoardError> {
        let key = SigningKey::from_bytes(sign_seed);
        if title.len() > limits::TITLE || about.len() > limits::ABOUT || rules.len() > limits::RULES {
            return Err(BoardError::TooLong);
        }
        let pk = key.verifying_key().to_bytes();
        let manifest = Manifest { title: title.into(), about: about.into(), rules: rules.into(), pk, created, mirrors: Vec::new(), see_also: Vec::new(), ids: false };
        Ok(Self { name: Cid::ipns_name(&pk), key, manifest, threads: Vec::new(), archive: Vec::new(), dels: Vec::new(), modlog: Vec::new(), own: Vec::new(), next_no: 1, seq: 0 })
    }

    /// The board's IPNS name (`k51…`), what `s.b` must say.
    pub fn name(&self) -> &Cid {
        &self.name
    }

    /// The board key, for owner posts (capcode) and actions.
    pub fn signing_key(&self) -> &SigningKey {
        &self.key
    }

    fn thread_mut(&mut self, no: u64) -> Option<&mut Thread> {
        self.threads.iter_mut().find(|t| t.no == no)
    }

    /// Takes a post (its signature checked) and returns its number. `cap` is `cap::OWNER` only
    /// for a post signed with the board key.
    pub fn accept(&mut self, s: Signed, sig: [u8; 64], cap: u8, now_s: u64) -> Result<u64, BoardError> {
        s.check()?;
        if s.b != self.name.to_text() {
            return Err(BoardError::Invalid);
        }
        if cap == cap::OWNER && s.k != self.manifest.pk || cap > cap::OWNER {
            return Err(BoardError::Refused);
        }
        s.verify(&sig)?;
        let ts = now_s - now_s % 60;
        if s.t == 0 {
            self.make_room(now_s)?;
            let no = self.next_no;
            self.next_no += 1;
            let sub = s.sub.clone();
            self.threads.push(Thread { no, sub, entries: vec![Entry::Post(Post { no, ts, s, sig, cap })], bump: now_s, created: now_s, sticky: false, locked: false, full: Vec::new() });
            return Ok(no);
        }
        let next_no = self.next_no;
        let t = self.thread_mut(s.t).ok_or(BoardError::NotFound)?;
        if t.locked {
            return Err(BoardError::Refused);
        }
        let recent = t.entries.len().saturating_sub(limits::CHUNK);
        if t.entries[recent..].iter().any(|e| matches!(e, Entry::Post(p) if p.s.body == s.body && !s.body.is_empty())) {
            return Err(BoardError::Duplicate);
        }
        let sage = s.sage;
        t.entries.push(Entry::Post(Post { no: next_no, ts, s, sig, cap }));
        if !sage && t.entries.len() <= limits::BUMP_LIMIT + 1 {
            t.bump = now_s;
        }
        if t.entries.len() >= limits::THREAD_POSTS {
            t.locked = true;
        }
        self.next_no += 1;
        Ok(next_no)
    }

    /// Room for one more thread: prunes the oldest-bumped unprotected thread when full (G.8:
    /// a young or recently bumped thread is never pruned; the new thread is refused instead).
    fn make_room(&mut self, now_s: u64) -> Result<(), BoardError> {
        if self.threads.len() < limits::THREADS {
            return Ok(());
        }
        let victim = self
            .threads
            .iter()
            .filter(|t| !t.sticky && now_s.saturating_sub(t.created) >= limits::PROTECT_AGE_S && now_s.saturating_sub(t.bump) >= limits::PROTECT_BUMP_S)
            .min_by_key(|t| t.bump)
            .map(|t| t.no)
            .ok_or(BoardError::Busy)?;
        self.prune(victim, now_s)
    }

    /// Moves a thread to the text archive (G.5.3).
    pub fn prune(&mut self, no: u64, now_s: u64) -> Result<(), BoardError> {
        let i = self.threads.iter().position(|t| t.no == no).ok_or(BoardError::NotFound)?;
        let mut t = self.threads.remove(i);
        let mut blocks = Vec::with_capacity(t.entries.len() / limits::CHUNK + 2);
        let cid = t.blocks(&mut blocks, &|_| false, &mut Vec::new());
        self.archive.push(Archived { no, sub: t.sub.clone(), ex: t.excerpt(), pruned: now_s, thread: cid, blocks });
        self.expire_archive(now_s);
        Ok(())
    }

    fn expire_archive(&mut self, now_s: u64) {
        self.archive.retain(|a| now_s.saturating_sub(a.pruned) < limits::ARCHIVE_S);
        let over = self.archive.len().saturating_sub(limits::ARCHIVE);
        self.archive.drain(..over);
    }

    /// Deletes post `no`: a tombstone keeps its number, its hash goes on the deletion list.
    /// Deleting an OP prunes its thread out of the archive too (4chan behaviour, G.5.3).
    pub fn delete(&mut self, no: u64, by_kind: u8, by: u8, now_s: u64) -> Result<(), BoardError> {
        let ti = self.threads.iter().position(|t| t.entries.iter().any(|e| e.no() == no)).ok_or(BoardError::NotFound)?;
        let t = &mut self.threads[ti];
        let ei = t.entries.iter().position(|e| e.no() == no).ok_or(BoardError::NotFound)?;
        let Entry::Post(p) = &t.entries[ei] else { return Err(BoardError::NotFound) };
        let h = post_hash(&p.s);
        let ts = p.ts;
        t.entries[ei] = Entry::Tomb { no, ts, del: by_kind, by };
        t.invalidate_from(ei);
        self.dels.push((h, now_s));
        if ei == 0 {
            // The OP is gone: so is the thread, archive included (its other posts go with it).
            let t = self.threads.remove(ti);
            for e in &t.entries {
                if let Entry::Post(p) = e {
                    self.dels.push((post_hash(&p.s), now_s));
                }
            }
        }
        self.trim_dels(now_s);
        Ok(())
    }

    fn trim_dels(&mut self, now_s: u64) {
        self.dels.retain(|(_, at)| now_s.saturating_sub(*at) < limits::DELS_S);
        let over = self.dels.len().saturating_sub(limits::DELS);
        self.dels.drain(..over);
    }

    pub fn set_sticky(&mut self, no: u64, on: bool) -> Result<(), BoardError> {
        self.thread_mut(no).ok_or(BoardError::NotFound)?.sticky = on;
        Ok(())
    }

    pub fn set_locked(&mut self, no: u64, on: bool) -> Result<(), BoardError> {
        self.thread_mut(no).ok_or(BoardError::NotFound)?.locked = on;
        Ok(())
    }

    /// Appends to the public moderation log (oldest dropped past `MODLOG`).
    pub fn log(&mut self, ts: u64, act: &str, no: u64, why: &str) {
        self.modlog.push(ModEntry { ts, act: act.to_owned(), no, why: why.to_owned() });
        let over = self.modlog.len().saturating_sub(limits::MODLOG);
        self.modlog.drain(..over);
    }

    pub fn set_own(&mut self, bytes: Vec<u8>) -> Result<(), BoardError> {
        if bytes.len() > limits::OWN {
            return Err(BoardError::TooLong);
        }
        self.own = bytes;
        Ok(())
    }

    pub fn set_mirrors(&mut self, mirrors: Vec<String>) -> Result<(), BoardError> {
        if mirrors.len() > limits::MIRRORS || mirrors.iter().any(|m| m.len() != 62 || !m.ends_with(".onion")) {
            return Err(BoardError::Invalid);
        }
        self.manifest.mirrors = mirrors;
        Ok(())
    }

    fn manifest_value(&self) -> Value {
        let m = &self.manifest;
        let texts = |v: &[String]| Value::Array(v.iter().cloned().map(Value::Text).collect());
        let unsigned = cbor::map(vec![
            ("v", Value::Uint(1)),
            ("kind", Value::Text("board".into())),
            ("title", Value::Text(m.title.clone())),
            ("about", Value::Text(m.about.clone())),
            ("rules", Value::Text(m.rules.clone())),
            ("pk", Value::Bytes(m.pk.to_vec())),
            ("created", Value::Uint(m.created)),
            ("mirrors", texts(&m.mirrors)),
            ("see_also", texts(&m.see_also)),
            ("ids", Value::Bool(m.ids)),
        ]);
        let sig = self.key.sign(&post::domain(post::MANIFEST, &unsigned)).to_bytes();
        let Value::Map(mut e) = unsigned else { unreachable!("a map") };
        e.push(("sig".into(), Value::Bytes(sig.to_vec())));
        cbor::map(e.iter().map(|(k, v)| (k.as_str(), v.clone())).collect())
    }

    /// Every block of the current state and the root CID (the root block comes last).
    pub fn build(&mut self, now_s: u64) -> (Cid, Vec<Block>) {
        self.build_into(now_s, &|_| false, &mut Vec::new())
    }

    /// As [`Self::build`], but blocks that never change once written (full chunks, archived
    /// threads) are left out when the caller already `held` them: a publish copies only what
    /// changed, not the whole board (G.5.3). `live` gets every CID of the new state, for the
    /// store's garbage collection.
    pub fn build_into(&mut self, now_s: u64, held: &dyn Fn(&Cid) -> bool, live: &mut Vec<Cid>) -> (Cid, Vec<Block>) {
        self.expire_archive(now_s);
        self.trim_dels(now_s);
        let mut out: Vec<Block> = Vec::with_capacity(16 + self.threads.len() * 3);
        let add = |out: &mut Vec<Block>, live: &mut Vec<Cid>, v: Value| {
            let b = v.encode();
            let c = Cid::of(DAG_CBOR, &b);
            live.push(c.clone());
            out.push((c.clone(), b));
            c
        };
        let manifest = add(&mut out, live, self.manifest_value());
        let mut buckets: Vec<Vec<Value>> = vec![Vec::new(); limits::BUCKETS];
        let mut pins = Vec::with_capacity(self.threads.len());
        for t in &mut self.threads {
            let cid = t.blocks(&mut out, held, live);
            buckets[(t.no % limits::BUCKETS as u64) as usize].push(cbor::map(vec![
                ("no", Value::Uint(t.no)),
                ("thread", Value::Bytes(cid.to_bytes())),
                ("bump", Value::Uint(t.bump)),
                ("r", Value::Uint(t.replies())),
                ("sub", Value::Text(t.sub.clone())),
                ("ex", Value::Text(t.excerpt())),
                ("st", Value::Bool(t.sticky)),
                ("lk", Value::Bool(t.locked)),
            ]));
            pins.push(Value::Link(cid));
        }
        let cat: Vec<Value> = buckets.into_iter().map(|t| Value::Link(add(&mut out, live, cbor::map(vec![("t", Value::Array(t))])))).collect();
        let threads = add(&mut out, live, cbor::map(vec![("t", Value::Array(pins))]));
        let mut arch = Vec::with_capacity(self.archive.len());
        let mut arch_pins = Vec::with_capacity(self.archive.len());
        for a in &self.archive {
            arch.push(cbor::map(vec![
                ("no", Value::Uint(a.no)),
                ("sub", Value::Text(a.sub.clone())),
                ("ex", Value::Text(a.ex.clone())),
                ("pruned", Value::Uint(a.pruned)),
                ("thread", Value::Bytes(a.thread.to_bytes())),
            ]));
            arch_pins.push(Value::Link(a.thread.clone()));
            for (c, b) in &a.blocks {
                live.push(c.clone());
                if !held(c) {
                    out.push((c.clone(), b.clone()));
                }
            }
        }
        let archive = add(&mut out, live, cbor::map(vec![("t", Value::Array(arch))]));
        let arch_threads = add(&mut out, live, cbor::map(vec![("t", Value::Array(arch_pins))]));
        let dels = add(&mut out, live, cbor::map(vec![("d", Value::Array(self.dels.iter().map(|(h, at)| cbor::map(vec![("h", Value::Bytes(h.to_vec())), ("at", Value::Uint(*at))])).collect()))]));
        let modlog = add(
            &mut out,
            live,
            cbor::map(vec![(
                "a",
                Value::Array(
                    self.modlog
                        .iter()
                        .map(|m| cbor::map(vec![("ts", Value::Uint(m.ts)), ("act", Value::Text(m.act.clone())), ("no", Value::Uint(m.no)), ("why", Value::Text(m.why.clone()))]))
                        .collect(),
                ),
            )]),
        );
        let own_cid = Cid::of(RAW, &self.own);
        live.push(own_cid.clone());
        out.push((own_cid.clone(), self.own.clone()));
        let root = cbor::map(vec![
            ("v", Value::Uint(1)),
            ("kind", Value::Text("board".into())),
            ("manifest", Value::Link(manifest)),
            ("cat", Value::Array(cat)),
            ("threads", Value::Link(threads)),
            ("archive", Value::Link(archive)),
            ("arch_threads", Value::Link(arch_threads)),
            ("dels", Value::Link(dels)),
            ("modlog", Value::Link(modlog)),
            ("own", Value::Link(own_cid)),
            ("ev", Value::Null),
            ("next_no", Value::Uint(self.next_no)),
            ("updated", Value::Uint(now_s)),
        ]);
        let rcid = add(&mut out, live, root);
        (rcid, out)
    }

    /// The next signed record for `root`: `sequence = max(last + 1, now_ms)` (time-based, so a
    /// device taking over never publishes below a reader's high-water mark, G.5.3), valid 72 h.
    pub fn record(&mut self, root: &Cid, now_ms: u64) -> Vec<u8> {
        self.seq = (self.seq + 1).max(now_ms);
        let r = Record { value: format!("/ipfs/{}", root.to_text()), sequence: self.seq, validity: now_ms / 1000 + limits::VALIDITY_S, ttl_ns: limits::TTL_NS };
        ipns::create(&self.key, &r)
    }

    /// Reopens the owner's board from a verified view (its stored blocks), continuing at `seq`.
    pub fn load(sign_seed: &[u8; 32], v: crate::verify::View, archived_blocks: Vec<Block>) -> Result<Self, BoardError> {
        let key = SigningKey::from_bytes(sign_seed);
        if key.verifying_key().to_bytes() != v.manifest.pk {
            return Err(BoardError::Invalid);
        }
        let threads = v
            .threads
            .into_iter()
            .map(|t| Thread { no: t.no, sub: t.sub, created: t.entries.first().map_or(0, |e| match e { Entry::Post(p) => p.ts, Entry::Tomb { ts, .. } => *ts }), entries: t.entries, bump: t.bump, sticky: t.sticky, locked: t.locked, full: Vec::new() })
            .collect();
        let archive = v
            .archive
            .into_iter()
            .map(|a| {
                let blocks = ephem_channel::gateway::reachable(&a.thread, archived_blocks.clone());
                Archived { no: a.no, sub: a.sub, ex: a.ex, pruned: a.pruned, thread: a.thread, blocks }
            })
            .collect();
        Ok(Self { name: v.name, key, manifest: v.manifest, threads, archive, dels: v.dels, modlog: v.modlog, own: v.own, next_no: v.next_no, seq: v.sequence })
    }
}
