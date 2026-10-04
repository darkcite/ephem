// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
// Copyright 2026 Anton (darkcite)
//! The owner's board (G.5): the single writer. It numbers posts, bumps and prunes threads,
//! applies moderation, and turns the state into dag-cbor blocks and a signed IPNS record.
//!
//! ```text
//! IPNS record ──▶ root {v, kind:"board", manifest⁴², cat:[bucket⁴² ×10], threads⁴², archive⁴²,
//!                       arch_threads⁴², dels⁴², modlog⁴², own⁴², ev: null, next_no, updated}
//!   manifest     {v, kind, title, about, rules, pk, created, host, mirrors, see_also, ids, sig}
//!   bucket i     {t: [{no, thread (ref), op, bump, r, sub, ex, st, lk}]}   threads with no mod 10 = i
//!   thread       {no, sub, chunks: [chunk⁴² …] ≤ 8, r, st, lk}
//!   chunk        {p: [post …] ≤ 64}
//!   post         {no, ts, s, sig, cap}  or  tombstone {no, ts, del, by}
//! ```
//!
//! Pin links (⁴²) are followed for pinning and garbage collection; refs (CID bytes) are not.
//! A full chunk keeps its CID until a post in it is deleted, and a thread that did not change
//! keeps its last chunk and thread block (BC-8), so a reply re-encodes only the last chunk, its
//! thread, the catalog buckets, the `threads` index and the root.
//!
//! **Deletion (G.5.1, BC-1, BC-3, BC-4):** a deleted post becomes a tombstone and its hash
//! (`SHA-256(dag-cbor(s))`) goes on the deletion list, live or archived alike; a deleted OP takes
//! its thread with it and lists only the OP (readers drop a thread whose OP is listed, and the
//! catalog's `op` lets them blank its subject). A listed post is never accepted again.
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
use std::collections::{HashMap, HashSet};

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
    /// The board's own onion, where posts go (BF-1: a link's `o=` is only a hint to read from).
    pub host: String,
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

    /// The entry as stored (no signature check: the owner's own blocks).
    pub(crate) fn from_value(v: &Value) -> Option<Self> {
        let no = v.get("no")?.uint()?;
        let ts = v.get("ts")?.uint()?;
        if let Some(d) = v.get("del") {
            return Some(Entry::Tomb { no, ts, del: u8::try_from(d.uint()?).ok()?, by: u8::try_from(v.get("by")?.uint()?).ok()? });
        }
        let s = Signed::from_value(v.get("s")?).ok()?;
        Some(Entry::Post(Post { no, ts, s, sig: v.get("sig")?.bytes()?.try_into().ok()?, cap: u8::try_from(v.get("cap")?.uint()?).ok()? }))
    }

    fn len(&self) -> u64 {
        self.to_value().encode().len() as u64
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

/// A board's title (one line), "about" and rules (multi-line) as readers may be shown them
/// (`ephem_proto::text`).
pub(crate) fn text_ok(title: &str, about: &str, rules: &str) -> bool {
    ephem_proto::text::line_ok(title) && ephem_proto::text::body_ok(about) && ephem_proto::text::body_ok(rules)
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
    /// The encoded last chunk (if not full) and thread block; `None` after a change (BC-8).
    tail: Option<(Option<Block>, Block)>,
    /// Encoded bytes of the entries (BC-7).
    bytes: u64,
    /// The OP's hash (the catalog's `op`).
    op: [u8; 32],
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

    fn new(no: u64, sub: String, entries: Vec<Entry>, bump: u64, created: u64, sticky: bool, locked: bool) -> Self {
        let bytes = entries.iter().map(Entry::len).sum();
        let op = match entries.first() {
            Some(Entry::Post(p)) => post_hash(&p.s),
            _ => [0; 32],
        };
        Thread { no, sub, entries, bump, created, sticky, locked, full: Vec::new(), tail: None, bytes, op }
    }

    /// Something in the thread changed from entry `idx` on (`entries.len()`: only its flags).
    fn changed(&mut self, idx: usize) {
        self.full.truncate(idx / limits::CHUNK);
        self.tail = None;
    }

    /// The chunk blocks and the thread block, encoded only when they changed. Blocks the caller
    /// already `held` are not copied out again; every CID goes to `live`.
    fn blocks(&mut self, out: &mut Vec<Block>, held: &dyn Fn(&Cid) -> bool, live: &mut Vec<Cid>) -> Cid {
        let n_full = self.entries.len() / limits::CHUNK;
        while self.full.len() < n_full {
            let i = self.full.len();
            self.full.push(chunk_block(&self.entries[i * limits::CHUNK..(i + 1) * limits::CHUNK]));
        }
        if self.tail.is_none() {
            let mut links: Vec<Value> = Vec::with_capacity(n_full + 1);
            links.extend(self.full.iter().map(|(c, _)| Value::Link(c.clone())));
            let rest = &self.entries[n_full * limits::CHUNK..];
            let last = (!rest.is_empty()).then(|| chunk_block(rest));
            if let Some((c, _)) = &last {
                links.push(Value::Link(c.clone()));
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
            self.tail = Some((last, (Cid::of(DAG_CBOR, &t), t)));
        }
        let (last, thread) = self.tail.as_ref().expect("encoded above");
        for (c, b) in self.full.iter().chain(last.iter()).chain(std::iter::once(thread)) {
            live.push(c.clone());
            if !held(c) {
                out.push((c.clone(), b.clone()));
            }
        }
        thread.0.clone()
    }

    /// Tombstones the posts of `want` here; their hashes and numbers go to `hashes`, `done`.
    fn tomb(&mut self, want: &HashSet<u64>, del: u8, by: u8, hashes: &mut Vec<[u8; 32]>, done: &mut Vec<u64>) {
        let mut first = None;
        for (i, e) in self.entries.iter_mut().enumerate() {
            let Entry::Post(p) = e else { continue };
            if !want.contains(&p.no) {
                continue;
            }
            hashes.push(post_hash(&p.s));
            let (no, ts) = (p.no, p.ts);
            done.push(no);
            let old = e.len();
            *e = Entry::Tomb { no, ts, del, by };
            self.bytes = self.bytes + e.len() - old;
            first.get_or_insert(i);
        }
        if let Some(i) = first {
            self.changed(i);
        }
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
    /// Its last post's number: a post `no` can only be here if `self.no ≤ no ≤ last`.
    last: u64,
    bytes: u64,
    blocks: Vec<Block>,
}

impl Archived {
    fn new(no: u64, sub: String, ex: String, pruned: u64, thread: Cid, blocks: Vec<Block>) -> Option<Self> {
        let mut a = Archived { no, sub, ex, pruned, thread, last: no, bytes: blocks.iter().map(|(_, b)| b.len() as u64).sum(), blocks };
        a.last = a.decode()?.entries.last()?.no();
        Some(a)
    }

    /// The thread back from its blocks (an owner path: moderation of archived posts, BC-1).
    fn decode(&self) -> Option<Thread> {
        let by: HashMap<&Cid, &Vec<u8>> = self.blocks.iter().map(|(c, b)| (c, b)).collect();
        let t = Value::decode(by.get(&self.thread)?)?;
        let mut entries = Vec::new();
        for c in t.get("chunks")?.array()? {
            for e in Value::decode(by.get(c.link()?)?)?.get("p")?.array()? {
                entries.push(Entry::from_value(e)?);
            }
        }
        Some(Thread::new(self.no, t.get("sub")?.text()?.to_owned(), entries, 0, 0, t.get("st")?.boolean()?, t.get("lk")?.boolean()?))
    }
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
    /// `(post hash, deleted at)` (G.5.1), oldest first.
    pub dels: Vec<([u8; 32], u64)>,
    del_set: HashSet<[u8; 32]>,
    /// The encoded `dels` block while it is unchanged.
    dels_block: Option<Block>,
    pub modlog: Vec<ModEntry>,
    /// The encrypted owner-state block (G.5.1 `own`), opaque here.
    pub own: Vec<u8>,
    pub next_no: u64,
    /// The last record's sequence.
    pub seq: u64,
}

impl Board {
    /// A new board for the board key `sign_seed` (`HKDF(seed, "p2pchat/board/" ‖ index)`, G.4),
    /// hosted at onion `host` (signed into the manifest: where posts go).
    pub fn new(sign_seed: &[u8; 32], host: &str, title: &str, about: &str, rules: &str, created: u64) -> Result<Self, BoardError> {
        let key = SigningKey::from_bytes(sign_seed);
        if title.len() > limits::TITLE || about.len() > limits::ABOUT || rules.len() > limits::RULES {
            return Err(BoardError::TooLong);
        }
        if !crate::onion::valid(host) || !text_ok(title, about, rules) {
            return Err(BoardError::Invalid);
        }
        let pk = key.verifying_key().to_bytes();
        let manifest = Manifest { title: title.into(), about: about.into(), rules: rules.into(), pk, created, host: host.into(), mirrors: Vec::new(), see_also: Vec::new(), ids: false };
        Ok(Self { name: Cid::ipns_name(&pk), key, manifest, threads: Vec::new(), archive: Vec::new(), dels: Vec::new(), del_set: HashSet::new(), dels_block: None, modlog: Vec::new(), own: Vec::new(), next_no: 1, seq: 0 })
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
        s.verify_as(&sig, cap == cap::OWNER)?;
        let h = post_hash(&s);
        if self.del_set.contains(&h) {
            return Err(BoardError::Refused); // deleted once: never back (BC-3)
        }
        let ts = now_s - now_s % 60;
        let e = Entry::Post(Post { no: self.next_no, ts, s, sig, cap });
        let len = e.len();
        let Entry::Post(post) = &e else { unreachable!("a post") };
        if post.s.t == 0 {
            self.make_room(now_s)?;
            self.fit(len, 0, now_s)?;
            let no = self.next_no;
            self.next_no += 1;
            let sub = post.s.sub.clone();
            self.threads.push(Thread::new(no, sub, vec![e], now_s, now_s, false, false));
            return Ok(no);
        }
        let t = self.thread_mut(post.s.t).ok_or(BoardError::NotFound)?;
        if t.locked {
            return Err(BoardError::Refused);
        }
        let recent = t.entries.len().saturating_sub(limits::CHUNK);
        if t.entries[recent..].iter().any(|x| matches!(x, Entry::Post(p) if p.s.body == post.s.body && !post.s.body.is_empty())) {
            return Err(BoardError::Duplicate);
        }
        let thread = post.s.t;
        self.fit(len, thread, now_s)?;
        let no = self.next_no;
        let t = self.thread_mut(thread).ok_or(BoardError::NotFound)?;
        let sage = matches!(&e, Entry::Post(p) if p.s.sage);
        t.entries.push(e);
        t.bytes += len;
        t.changed(t.entries.len() - 1);
        if !sage && t.entries.len() <= limits::BUMP_LIMIT + 1 {
            t.bump = now_s;
        }
        if t.entries.len() >= limits::THREAD_POSTS {
            t.locked = true;
        }
        self.next_no += 1;
        Ok(no)
    }

    /// The oldest-bumped thread that may be pruned now (G.8: never a sticky, young or recently
    /// bumped one), other than `keep`.
    fn victim(&self, now_s: u64, keep: u64) -> Option<u64> {
        self.threads
            .iter()
            .filter(|t| t.no != keep && !t.sticky && now_s.saturating_sub(t.created) >= limits::PROTECT_AGE_S && now_s.saturating_sub(t.bump) >= limits::PROTECT_BUMP_S)
            .min_by_key(|t| t.bump)
            .map(|t| t.no)
    }

    /// Room for one more thread: prunes the oldest-bumped unprotected thread when full (G.8:
    /// a young or recently bumped thread is never pruned; the new thread is refused instead).
    fn make_room(&mut self, now_s: u64) -> Result<(), BoardError> {
        if self.threads.len() < limits::THREADS {
            return Ok(());
        }
        let victim = self.victim(now_s, 0).ok_or(BoardError::Busy)?;
        self.prune(victim, now_s)
    }

    /// Encoded bytes of the live threads and the archive (BC-7).
    pub fn bytes(&self) -> u64 {
        self.threads.iter().map(|t| t.bytes).sum::<u64>() + self.archive.iter().map(|a| a.bytes).sum::<u64>()
    }

    /// Room for `need` more bytes (BC-7): the archive's oldest threads go first, then the
    /// oldest unprotected live thread (not `keep`) is pruned; `Busy` when nothing can go.
    fn fit(&mut self, need: u64, keep: u64, now_s: u64) -> Result<(), BoardError> {
        while self.bytes() + need > limits::BYTES {
            if !self.archive.is_empty() {
                self.archive.remove(0);
                continue;
            }
            let v = self.victim(now_s, keep).ok_or(BoardError::Busy)?;
            let i = self.threads.iter().position(|t| t.no == v).expect("a live thread");
            self.threads.remove(i);
        }
        Ok(())
    }

    /// Moves a thread to the text archive (G.5.3).
    pub fn prune(&mut self, no: u64, now_s: u64) -> Result<(), BoardError> {
        let i = self.threads.iter().position(|t| t.no == no).ok_or(BoardError::NotFound)?;
        let mut t = self.threads.remove(i);
        let mut blocks = Vec::with_capacity(t.entries.len() / limits::CHUNK + 2);
        let cid = t.blocks(&mut blocks, &|_| false, &mut Vec::new());
        let last = t.entries.last().map_or(no, Entry::no);
        let bytes = blocks.iter().map(|(_, b)| b.len() as u64).sum();
        self.archive.push(Archived { no, sub: t.sub.clone(), ex: t.excerpt(), pruned: now_s, thread: cid, last, bytes, blocks });
        self.expire_archive(now_s);
        Ok(())
    }

    fn expire_archive(&mut self, now_s: u64) {
        self.archive.retain(|a| now_s.saturating_sub(a.pruned) < limits::ARCHIVE_S);
        let over = self.archive.len().saturating_sub(limits::ARCHIVE);
        self.archive.drain(..over);
    }

    /// Deletes post `no` (live or archived): a tombstone keeps its number, its hash goes on the
    /// deletion list. Deleting an OP takes its thread with it (4chan behaviour, G.5.3).
    pub fn delete(&mut self, no: u64, by_kind: u8, by: u8, now_s: u64) -> Result<(), BoardError> {
        if self.delete_many(&[no], by_kind, by, now_s).is_empty() { Err(BoardError::NotFound) } else { Ok(()) }
    }

    /// Deletes every post of `nos` in one pass (mass delete, BC-10). Returns the numbers deleted
    /// (a reply in a thread whose OP is deleted counts as deleted).
    pub fn delete_many(&mut self, nos: &[u64], by_kind: u8, by: u8, now_s: u64) -> Vec<u64> {
        let want: HashSet<u64> = nos.iter().copied().collect();
        let mut done = Vec::new();
        let mut hashes = Vec::new();
        let gone = |t: &Thread, done: &mut Vec<u64>| done.extend(t.entries.iter().map(Entry::no).filter(|n| want.contains(n)));
        let mut i = 0;
        while i < self.threads.len() {
            let t = &mut self.threads[i];
            if !t.entries.iter().any(|e| want.contains(&e.no())) {
                i += 1;
            } else if want.contains(&t.no) {
                hashes.push(t.op);
                gone(t, &mut done);
                self.threads.remove(i);
            } else {
                t.tomb(&want, by_kind, by, &mut hashes, &mut done);
                i += 1;
            }
        }
        // Archived threads (BC-1): decoded, changed and re-encoded; their OP drops the entry.
        let mut i = 0;
        while i < self.archive.len() {
            let a = &self.archive[i];
            let Some(mut t) = want.iter().any(|n| (a.no..=a.last).contains(n)).then(|| a.decode()).flatten() else {
                i += 1;
                continue;
            };
            if want.contains(&t.no) {
                hashes.push(t.op);
                gone(&t, &mut done);
                self.archive.remove(i);
                continue;
            }
            let before = done.len();
            t.tomb(&want, by_kind, by, &mut hashes, &mut done);
            if done.len() > before {
                let mut blocks = Vec::new();
                let a = &mut self.archive[i];
                a.thread = t.blocks(&mut blocks, &|_| false, &mut Vec::new());
                a.bytes = blocks.iter().map(|(_, b)| b.len() as u64).sum();
                a.blocks = blocks;
            }
            i += 1;
        }
        for h in hashes {
            self.listed(h, now_s);
        }
        self.trim_dels(now_s);
        done
    }

    fn listed(&mut self, h: [u8; 32], now_s: u64) {
        if self.del_set.insert(h) {
            self.dels.push((h, now_s));
            self.dels_block = None;
        }
    }

    /// Post `no`, live or archived (the owner's moderation: ban, delete).
    pub fn find(&self, no: u64) -> Option<Post> {
        let hit = |t: &Thread| t.entries.iter().find_map(|e| match e {
            Entry::Post(p) if p.no == no => Some(p.clone()),
            _ => None,
        });
        self.threads.iter().find_map(hit).or_else(|| self.archive.iter().filter(|a| (a.no..=a.last).contains(&no)).find_map(|a| hit(&a.decode()?)))
    }

    /// The numbers of every post, live or archived, that `f` selects (mass delete, G.9.1).
    pub fn select(&self, f: impl Fn(&Post) -> bool) -> Vec<u64> {
        let mut out = Vec::new();
        let mut take = |t: &Thread| out.extend(t.entries.iter().filter_map(|e| if let Entry::Post(p) = e && f(p) { Some(p.no) } else { None }));
        self.threads.iter().for_each(&mut take);
        self.archive.iter().filter_map(Archived::decode).for_each(|t| take(&t));
        out
    }

    /// Entries past 30 days go; past `DELS` the oldest go too, but only those older than any
    /// record that may still be served (BC-4); `DELS_MAX` is the hard cap.
    fn trim_dels(&mut self, now_s: u64) {
        let before = self.dels.len();
        self.dels.retain(|(_, at)| now_s.saturating_sub(*at) < limits::DELS_S);
        let over = self.dels.len().saturating_sub(limits::DELS);
        let old = self.dels.iter().take(over).take_while(|(_, at)| now_s.saturating_sub(*at) > limits::VALIDITY_S + 3600).count();
        self.dels.drain(..old);
        let over = self.dels.len().saturating_sub(limits::DELS_MAX);
        self.dels.drain(..over);
        if self.dels.len() != before {
            self.del_set = self.dels.iter().map(|(h, _)| *h).collect();
            self.dels_block = None;
        }
    }

    pub fn set_sticky(&mut self, no: u64, on: bool) -> Result<(), BoardError> {
        let t = self.thread_mut(no).ok_or(BoardError::NotFound)?;
        t.sticky = on;
        t.changed(t.entries.len());
        Ok(())
    }

    pub fn set_locked(&mut self, no: u64, on: bool) -> Result<(), BoardError> {
        let t = self.thread_mut(no).ok_or(BoardError::NotFound)?;
        t.locked = on;
        t.changed(t.entries.len());
        Ok(())
    }

    /// Appends to the public moderation log (oldest dropped past `MODLOG`).
    pub fn log(&mut self, ts: u64, act: &str, no: u64, why: &str) {
        // A reason as readers accept it: one line, nothing invisible or reordering.
        let why: String = why.chars().filter(|&c| !c.is_control() && !ephem_proto::text::direction(c) && !ephem_proto::text::invisible(c)).collect();
        self.modlog.push(ModEntry { ts, act: act.to_owned(), no, why });
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
        if mirrors.len() > limits::MIRRORS || !mirrors.iter().all(|m| crate::onion::valid(m)) {
            return Err(BoardError::Invalid);
        }
        self.manifest.mirrors = mirrors;
        Ok(())
    }

    /// "See also" (G.12): other boards the owner points to, each `<name>@<onion>` (≤ 16).
    pub fn set_see_also(&mut self, links: Vec<String>) -> Result<(), BoardError> {
        if links.len() > limits::SEE_ALSO || !links.iter().all(|l| crate::onion::see_also_valid(l)) {
            return Err(BoardError::Invalid);
        }
        self.manifest.see_also = links;
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
            ("host", Value::Text(m.host.clone())),
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
            if !held(&c) {
                out.push((c.clone(), b));
            }
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
                ("op", Value::Bytes(t.op.to_vec())),
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
        let (dels, b) = self.dels_block.get_or_insert_with(|| {
            let b = cbor::map(vec![("d", Value::Array(self.dels.iter().map(|(h, at)| cbor::map(vec![("h", Value::Bytes(h.to_vec())), ("at", Value::Uint(*at))])).collect()))]).encode();
            (Cid::of(DAG_CBOR, &b), b)
        });
        live.push(dels.clone());
        if !held(dels) {
            out.push((dels.clone(), b.clone()));
        }
        let dels = dels.clone();
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
        if !held(&own_cid) {
            out.push((own_cid.clone(), self.own.clone()));
        }
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
    /// Every archived thread must be held whole (a takeover refuses an incomplete source, BW-2).
    pub fn load(sign_seed: &[u8; 32], v: crate::verify::View, archived_blocks: Vec<Block>) -> Result<Self, BoardError> {
        let key = SigningKey::from_bytes(sign_seed);
        if key.verifying_key().to_bytes() != v.manifest.pk {
            return Err(BoardError::Invalid);
        }
        let threads = v
            .threads
            .into_iter()
            .map(|t| {
                let created = t.entries.first().map_or(0, |e| match e {
                    Entry::Post(p) => p.ts,
                    Entry::Tomb { ts, .. } => *ts,
                });
                Thread::new(t.no, t.sub, t.entries, t.bump, created, t.sticky, t.locked)
            })
            .collect();
        // One map, each archived thread's blocks moved out of it: O(total), not O(archive ×
        // total) (BW-5); blocks no archived thread reaches are dropped.
        let mut held: HashMap<Cid, Vec<u8>> = archived_blocks.into_iter().collect();
        let mut archive = Vec::with_capacity(v.archive.len());
        for a in v.archive {
            let mut blocks = Vec::new();
            let mut todo = vec![a.thread.clone()];
            while let Some(c) = todo.pop() {
                let Some(data) = held.remove(&c) else { continue };
                if let Some(x) = Value::decode(&data) {
                    ephem_channel::gateway::links(&x, &mut todo);
                }
                blocks.push((c, data));
            }
            archive.push(Archived::new(a.no, a.sub, a.ex, a.pruned, a.thread, blocks).ok_or(BoardError::Invalid)?);
        }
        let del_set = v.dels.iter().map(|(h, _)| *h).collect();
        Ok(Self { name: v.name, key, manifest: v.manifest, threads, archive, dels: v.dels, del_set, dels_block: None, modlog: v.modlog, own: v.own, next_no: v.next_no, seq: v.sequence })
    }
}
