// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
// Copyright 2026 Anton (darkcite)
//! A reader checks a board (G.5.1 reader rules, G.5.3): the record, then the blocks it holds.
//!
//! A reader rarely holds the whole board: it fetches the catalog buckets, then the threads it
//! opens. So every thread is optional; one whose blocks are absent appears in the catalog
//! only. Whatever is present must be exactly right:
//!
//! - the record verifies for the board's name, its sequence is not below the reader's
//!   high-water mark and not more than an hour ahead of its clock;
//! - the root and the manifest are a board's (`kind: "board"`), the manifest signed by the key
//!   the name is;
//! - every catalog entry is in its bucket (`no mod 10`), names a thread pinned by `threads`, and
//!   says what that thread says (subject, replies, sticky, locked);
//! - in a thread: the OP's `no` is the thread's, its `s.t` is 0; every reply's `s.t` is the
//!   thread; numbers strictly increase and stay below `next_no`; every chunk but the last holds
//!   exactly 64 posts; `r` is posts − 1; each post's `s.b` is this board and its signature
//!   verifies (a capcode post with the board key);
//! - a post whose hash is on the deletion list (this root's, or a newer one the reader saw) is
//!   shown as deleted, even from an older root that a stale mirror serves; a thread whose OP is
//!   listed is deleted whole, and its catalog entry loses its subject and excerpt (BC-3, BC-4);
//! - the manifest's host, mirrors and "see also" entries are well-formed v3 onions (BF-1, BF-3);
//!   catalog numbers stay within their limits whether or not the thread is held (BC-9).

use crate::board::{Entry, Manifest, ModEntry, Post, post_hash};
use crate::post::{self, Signed};
use crate::{BoardError, limits};
use ed25519_dalek::{Signature, Verifier, VerifyingKey};
use ephem_channel::car::Block;
use ephem_channel::cbor::Value;
use ephem_channel::cid::Cid;
use ephem_channel::ipns;
use std::collections::HashMap;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CatalogEntry {
    pub no: u64,
    pub thread: Cid,
    /// The OP's hash: a listed OP blanks the entry.
    pub op: [u8; 32],
    pub bump: u64,
    pub replies: u64,
    pub sub: String,
    pub ex: String,
    pub sticky: bool,
    pub locked: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ThreadView {
    pub no: u64,
    pub sub: String,
    pub entries: Vec<Entry>,
    pub bump: u64,
    pub sticky: bool,
    pub locked: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ArchiveEntry {
    pub no: u64,
    pub sub: String,
    pub ex: String,
    pub pruned: u64,
    pub thread: Cid,
}

#[derive(Clone, Debug)]
pub struct View {
    pub name: Cid,
    pub root: Cid,
    pub sequence: u64,
    pub manifest: Manifest,
    /// Sorted for display: sticky first, then by bump (newest first; on a tie, the newer thread).
    pub catalog: Vec<CatalogEntry>,
    /// The threads whose blocks were held, catalog order.
    pub threads: Vec<ThreadView>,
    pub archive: Vec<ArchiveEntry>,
    pub dels: Vec<([u8; 32], u64)>,
    pub modlog: Vec<ModEntry>,
    pub own: Vec<u8>,
    pub next_no: u64,
    pub updated: u64,
    /// The record has expired: the last verified state, shown as stale (G.5.3).
    pub stale: bool,
}

type Blocks<'a> = HashMap<&'a Cid, &'a Vec<u8>>;

fn get<'a>(by: &Blocks<'a>, c: &Cid) -> Option<&'a Vec<u8>> {
    by.get(c).copied()
}

fn node(by: &Blocks<'_>, c: &Cid) -> Result<Value, BoardError> {
    get(by, c).and_then(|b| Value::decode(b)).ok_or(BoardError::Invalid)
}

fn uint(v: &Value, k: &str) -> Result<u64, BoardError> {
    v.get(k).and_then(Value::uint).ok_or(BoardError::Invalid)
}

fn text(v: &Value, k: &str) -> Result<String, BoardError> {
    v.get(k).and_then(Value::text).map(str::to_owned).ok_or(BoardError::Invalid)
}

fn flag(v: &Value, k: &str) -> Result<bool, BoardError> {
    v.get(k).and_then(Value::boolean).ok_or(BoardError::Invalid)
}

fn link(v: &Value, k: &str) -> Result<Cid, BoardError> {
    v.get(k).and_then(Value::link).cloned().ok_or(BoardError::Invalid)
}

fn list<'a>(v: &'a Value, k: &str) -> Result<&'a [Value], BoardError> {
    v.get(k).and_then(Value::array).ok_or(BoardError::Invalid)
}

fn texts(v: &Value, k: &str, max: usize) -> Result<Vec<String>, BoardError> {
    let l = list(v, k)?;
    if l.len() > max {
        return Err(BoardError::Invalid);
    }
    l.iter().map(|x| x.text().map(str::to_owned).ok_or(BoardError::Invalid)).collect()
}

fn manifest(v: &Value, key: &VerifyingKey) -> Result<Manifest, BoardError> {
    let Value::Map(m) = v else { return Err(BoardError::Invalid) };
    let sig = v.get("sig").and_then(Value::bytes).and_then(|b| Signature::from_slice(b).ok()).ok_or(BoardError::Invalid)?;
    let unsigned = Value::Map(m.iter().filter(|(k, _)| k != "sig").cloned().collect());
    key.verify(&post::domain(post::MANIFEST, &unsigned), &sig).map_err(|_| BoardError::BadSignature)?;
    if uint(v, "v")? != 1 || text(v, "kind")? != "board" {
        return Err(BoardError::Invalid);
    }
    let man = Manifest {
        title: text(v, "title")?,
        about: text(v, "about")?,
        rules: text(v, "rules")?,
        pk: v.get("pk").and_then(Value::bytes).and_then(|b| b.try_into().ok()).ok_or(BoardError::Invalid)?,
        created: uint(v, "created")?,
        host: text(v, "host")?,
        mirrors: texts(v, "mirrors", limits::MIRRORS)?,
        see_also: texts(v, "see_also", limits::SEE_ALSO)?,
        ids: flag(v, "ids")?,
    };
    if man.pk != key.to_bytes() || man.title.len() > limits::TITLE || man.about.len() > limits::ABOUT || man.rules.len() > limits::RULES || !crate::board::text_ok(&man.title, &man.about, &man.rules) {
        return Err(BoardError::Invalid);
    }
    if !crate::onion::valid(&man.host) || !man.mirrors.iter().all(|m| crate::onion::valid(m)) || !man.see_also.iter().all(|l| crate::onion::see_also_valid(l)) {
        return Err(BoardError::Invalid);
    }
    Ok(man)
}

fn entry(v: &Value, name: &str, board_pk: &[u8; 32], deleted: &dyn Fn(&[u8; 32]) -> bool) -> Result<Entry, BoardError> {
    let no = uint(v, "no")?;
    let ts = uint(v, "ts")?;
    if let Some(del) = v.get("del") {
        let del = del.uint().and_then(|d| u8::try_from(d).ok()).ok_or(BoardError::Invalid)?;
        let by = u8::try_from(uint(v, "by")?).map_err(|_| BoardError::Invalid)?;
        return Ok(Entry::Tomb { no, ts, del, by });
    }
    let s = Signed::from_value(v.get("s").ok_or(BoardError::Invalid)?)?;
    let sig: [u8; 64] = v.get("sig").and_then(Value::bytes).and_then(|b| b.try_into().ok()).ok_or(BoardError::Invalid)?;
    let cap = u8::try_from(uint(v, "cap")?).map_err(|_| BoardError::Invalid)?;
    if s.b != name || cap > crate::board::cap::OWNER || (cap == crate::board::cap::OWNER && s.k != *board_pk) {
        return Err(BoardError::Invalid);
    }
    s.verify_as(&sig, cap == crate::board::cap::OWNER)?;
    if deleted(&post_hash(&s)) {
        // Deleted since (a newer deletion list): shown as deleted by the owner.
        return Ok(Entry::Tomb { no, ts, del: crate::board::del::OWNER, by: 0 });
    }
    Ok(Entry::Post(Post { no, ts, s, sig, cap }))
}

/// A thread from its block and chunks, against the reader rules.
fn thread(by: &Blocks<'_>, cid: &Cid, name: &str, pk: &[u8; 32], next_no: u64, deleted: &dyn Fn(&[u8; 32]) -> bool) -> Result<Option<ThreadView>, BoardError> {
    let Some(raw) = get(by, cid) else { return Ok(None) };
    let t = Value::decode(raw).ok_or(BoardError::Invalid)?;
    let no = uint(&t, "no")?;
    let chunks = list(&t, "chunks")?;
    if chunks.is_empty() || chunks.len() > limits::THREAD_POSTS.div_ceil(limits::CHUNK) {
        return Err(BoardError::Invalid);
    }
    let mut entries = Vec::with_capacity(chunks.len() * limits::CHUNK);
    for (i, c) in chunks.iter().enumerate() {
        let c = c.link().ok_or(BoardError::Invalid)?;
        let Some(raw) = get(by, c) else { return Ok(None) }; // a chunk not fetched: catalog only
        let chunk = Value::decode(raw).ok_or(BoardError::Invalid)?;
        let p = list(&chunk, "p")?;
        let full = i + 1 < chunks.len();
        if p.is_empty() || p.len() > limits::CHUNK || (full && p.len() != limits::CHUNK) {
            return Err(BoardError::Invalid);
        }
        for e in p {
            entries.push(entry(e, name, pk, deleted)?);
        }
    }
    let mut last = 0;
    for (i, e) in entries.iter().enumerate() {
        let n = e.no();
        if n <= last || n >= next_no || (i == 0 && n != no) {
            return Err(BoardError::Invalid);
        }
        last = n;
        if let Entry::Post(p) = e
            && ((i == 0) != (p.s.t == 0) || (i > 0 && p.s.t != no))
        {
            return Err(BoardError::Invalid);
        }
    }
    if uint(&t, "r")? != entries.len() as u64 - 1 || entries.len() > limits::THREAD_POSTS {
        return Err(BoardError::Invalid);
    }
    // A listed OP (a stale root of a thread deleted since): the whole thread is deleted.
    if matches!(entries[0], Entry::Tomb { .. }) {
        for e in entries.iter_mut().skip(1) {
            if let Entry::Post(p) = e {
                *e = Entry::Tomb { no: p.no, ts: p.ts, del: crate::board::del::OWNER, by: 0 };
            }
        }
    }
    Ok(Some(ThreadView { no, sub: text(&t, "sub")?, entries, bump: 0, sticky: flag(&t, "st")?, locked: flag(&t, "lk")? }))
}

/// Verifies a board record and the blocks held. `min_sequence`: the reader's high-water mark;
/// `known_dels`: deletion-list hashes from newer roots the reader saw (G.5.1).
pub fn verify(name: &Cid, record: &[u8], blocks: &[Block], now_ms: u64, min_sequence: u64, known_dels: &[[u8; 32]]) -> Result<View, BoardError> {
    verify_with(name, record, blocks, now_ms, min_sequence, known_dels, false)
}

/// As [`verify`], but an expired record is accepted and the view marked [`View::stale`]: what a
/// reader shows, labelled, when the host has been offline past the record's validity (G.5.3).
/// Everything else is checked as usual.
pub fn verify_stale(name: &Cid, record: &[u8], blocks: &[Block], now_ms: u64, min_sequence: u64, known_dels: &[[u8; 32]]) -> Result<View, BoardError> {
    verify_with(name, record, blocks, now_ms, min_sequence, known_dels, true)
}

fn verify_with(name: &Cid, record: &[u8], blocks: &[Block], now_ms: u64, min_sequence: u64, known_dels: &[[u8; 32]], allow_expired: bool) -> Result<View, BoardError> {
    let fresh = ipns::verify(name, record, now_ms / 1000);
    let stale = fresh.is_err() && allow_expired;
    let rec = if stale { ipns::verify(name, record, 0) } else { fresh }.map_err(|_| BoardError::Record)?;
    if rec.sequence < min_sequence || rec.sequence > now_ms + limits::FUTURE_MS {
        return Err(BoardError::Record);
    }
    let root = rec.value.strip_prefix("/ipfs/").and_then(Cid::parse).ok_or(BoardError::Invalid)?;
    let pk = name.ed25519_key().ok_or(BoardError::Invalid)?;
    let key = VerifyingKey::from_bytes(&pk).map_err(|_| BoardError::Invalid)?;
    let mut v = read(&key, name, &root, blocks, known_dels)?;
    v.sequence = rec.sequence;
    v.stale = stale;
    Ok(v)
}

/// The blocks under `root` (no record check): what [`verify`] does after the record, and what
/// the owner uses to reopen its board from its store.
pub fn read(key: &VerifyingKey, name: &Cid, root: &Cid, blocks: &[Block], known_dels: &[[u8; 32]]) -> Result<View, BoardError> {
    // Only blocks that hash to their CID count, whoever handed them over.
    let by: Blocks<'_> = blocks.iter().filter(|(c, b)| c.verifies(b)).map(|(c, b)| (c, b)).collect();
    let pk = key.to_bytes();
    let r = node(&by, root)?;
    if uint(&r, "v")? != 1 || text(&r, "kind")? != "board" || !matches!(r.get("ev"), Some(Value::Null)) {
        return Err(BoardError::Invalid);
    }
    let man = manifest(&node(&by, &link(&r, "manifest")?)?, key)?;
    let next_no = uint(&r, "next_no")?;
    let dels: Vec<([u8; 32], u64)> = list(&node(&by, &link(&r, "dels")?)?, "d")?
        .iter()
        .map(|d| Ok((d.get("h").and_then(Value::bytes).and_then(|b| b.try_into().ok()).ok_or(BoardError::Invalid)?, uint(d, "at")?)))
        .collect::<Result<_, BoardError>>()?;
    if dels.len() > limits::DELS_MAX {
        return Err(BoardError::Invalid);
    }
    let deleted = |h: &[u8; 32]| dels.iter().any(|(x, _)| x == h) || known_dels.contains(h);
    let name_text = name.to_text();
    let pinned: Vec<Cid> = list(&node(&by, &link(&r, "threads")?)?, "t")?.iter().map(|x| x.link().cloned().ok_or(BoardError::Invalid)).collect::<Result<_, _>>()?;
    let cat = list(&r, "cat")?;
    if cat.len() != limits::BUCKETS || pinned.len() > limits::THREADS {
        return Err(BoardError::Invalid);
    }
    let mut catalog = Vec::with_capacity(pinned.len());
    for (i, b) in cat.iter().enumerate() {
        let b = node(&by, b.link().ok_or(BoardError::Invalid)?)?;
        for e in list(&b, "t")? {
            let ce = CatalogEntry {
                no: uint(e, "no")?,
                thread: e.get("thread").and_then(Value::bytes).and_then(Cid::from_bytes).ok_or(BoardError::Invalid)?,
                op: e.get("op").and_then(Value::bytes).and_then(|b| b.try_into().ok()).ok_or(BoardError::Invalid)?,
                bump: uint(e, "bump")?,
                replies: uint(e, "r")?,
                sub: text(e, "sub")?,
                ex: text(e, "ex")?,
                sticky: flag(e, "st")?,
                locked: flag(e, "lk")?,
            };
            if ce.no % limits::BUCKETS as u64 != i as u64 || !pinned.contains(&ce.thread) || ce.no >= next_no || ce.ex.len() > limits::EXCERPT || ce.sub.len() > limits::SUBJECT || ce.replies >= limits::THREAD_POSTS as u64 || !ephem_proto::text::line_ok(&ce.sub) || !ephem_proto::text::body_ok(&ce.ex) {
                return Err(BoardError::Invalid);
            }
            catalog.push(ce);
        }
    }
    if catalog.len() != pinned.len() {
        return Err(BoardError::Invalid);
    }
    catalog.sort_by(|a, b| b.sticky.cmp(&a.sticky).then(b.bump.cmp(&a.bump)).then(b.no.cmp(&a.no)));
    let mut threads = Vec::new();
    for ce in &catalog {
        if let Some(mut t) = thread(&by, &ce.thread, &name_text, &pk, next_no, &deleted)? {
            if t.no != ce.no || t.sub != ce.sub || t.entries.len() as u64 - 1 != ce.replies || t.sticky != ce.sticky || t.locked != ce.locked {
                return Err(BoardError::Invalid);
            }
            t.bump = ce.bump;
            if deleted(&ce.op) {
                t.sub.clear();
            }
            threads.push(t);
        }
    }
    for ce in catalog.iter_mut().filter(|ce| deleted(&ce.op)) {
        ce.sub.clear();
        ce.ex.clear();
    }
    let archive = list(&node(&by, &link(&r, "archive")?)?, "t")?
        .iter()
        .map(|a| {
            Ok(ArchiveEntry {
                no: uint(a, "no")?,
                sub: text(a, "sub")?,
                ex: text(a, "ex")?,
                pruned: uint(a, "pruned")?,
                thread: a.get("thread").and_then(Value::bytes).and_then(Cid::from_bytes).ok_or(BoardError::Invalid)?,
            })
        })
        .collect::<Result<Vec<_>, BoardError>>()?;
    let arch_pins: Vec<Cid> = list(&node(&by, &link(&r, "arch_threads")?)?, "t")?.iter().map(|x| x.link().cloned().ok_or(BoardError::Invalid)).collect::<Result<_, _>>()?;
    let shown = |sub: &str, ex: &str| ephem_proto::text::line_ok(sub) && ephem_proto::text::body_ok(ex);
    if archive.len() > limits::ARCHIVE || archive.len() != arch_pins.len() || archive.iter().any(|a| !arch_pins.contains(&a.thread) || !shown(&a.sub, &a.ex)) {
        return Err(BoardError::Invalid);
    }
    let modlog = list(&node(&by, &link(&r, "modlog")?)?, "a")?
        .iter()
        .map(|m| Ok(ModEntry { ts: uint(m, "ts")?, act: text(m, "act")?, no: uint(m, "no")?, why: text(m, "why")? }))
        .collect::<Result<Vec<_>, BoardError>>()?;
    if modlog.iter().any(|m| !shown(&m.act, &m.why)) {
        return Err(BoardError::Invalid);
    }
    let own_cid = link(&r, "own")?;
    let own = get(&by, &own_cid).cloned().unwrap_or_default();
    if own.len() > limits::OWN {
        return Err(BoardError::Invalid);
    }
    Ok(View { name: name.clone(), root: root.clone(), sequence: 0, manifest: man, catalog, threads, archive, dels, modlog, own, next_no, updated: uint(&r, "updated")?, stale: false })
}
