// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
// Copyright 2026 Anton (darkcite)
//! The board's onion (G.6.1, G.11.1): the channel gateway's HTTP/1.1 subset (`Connection:
//! close`, one request per Tor stream, no chunking), extended for boards. Sans-IO.
//!
//! | Request | Answer |
//! |---|---|
//! | `GET /pow` | the 58-byte [`PowInfo`] |
//! | `POST /submit` (`Content-Type: application/vnd.ephem.board-submit`, `Content-Length`) | `200` + `{no u64, seq u64}`, or a status + `u16` code ([`Refusal`]) |
//! | `GET /ipns/<name>?format=ipns-record` | the signed record (≤ 10 KiB) |
//! | `GET /ipns/<name>?format=ephem-board` | **the index**: `u16 len ‖ record ‖ CAR(root; manifest, 10 buckets, threads, archive, arch_threads, dels, modlog)`, one response, so the record and the blocks it names never come from two versions |
//! | `GET /ipfs/<cid>?format=car` | a thread with its chunks, a bucket, a chunk; **`406` for the root, `threads` and `arch_threads`**: their DAG is the whole board (B-M10) |
//! | `GET /ipfs/<cid>?format=raw` | one block |
//! | `GET /`, `/?p=2`…, `/t/<no>` | the board as plain pages for Tor Browser ([`crate::page`]) |
//!
//! **Copies (G.14.2):** the request head is parsed in place; refusals, `/pow` and answers are
//! written into a caller's fixed buffer (no allocation). Block responses are built once into a
//! `Vec` (the HTTP head, then the body: one copy of public data); the index is built once per
//! version and shared (`Rc`), and so are the last [`PAGES`] plain pages asked for (BC-10).

use crate::pipeline::{PowInfo, Refusal};
use crate::page;
use crate::submit::MAX_SUBMIT;
use ephem_channel::car::{self, Block};
use ephem_channel::cbor::Value;
use ephem_channel::cid::Cid;
use std::collections::{HashMap, HashSet};
use std::io::Write;
use std::rc::Rc;

pub const CT_SUBMIT: &str = "application/vnd.ephem.board-submit";
pub const CT_POW: &str = "application/vnd.ephem.board-pow";
pub const CT_INDEX: &str = "application/vnd.ephem.board-index";
pub const CT_ANSWER: &str = "application/vnd.ephem.board-answer";
pub const CT_RECORD: &str = ephem_channel::gateway::CT_RECORD;
pub const CT_CAR: &str = ephem_channel::gateway::CT_CAR;
pub const CT_RAW: &str = ephem_channel::gateway::CT_RAW;
/// Largest request head (G.6.2 step 1).
pub const MAX_HEAD: usize = 8 * 1024;
/// Response caps a reader enforces (G.11.1, A-m3).
pub const MAX_RECORD: usize = 10 * 1024;
pub const MAX_CAR: usize = 1536 * 1024;
/// `{no u64, seq u64}`.
pub const ANSWER_LEN: usize = 16;
/// A buffer for the short responses (refusals, answers, `/pow`).
pub const SHORT: usize = 256;
/// Plain pages kept per version (a full thread's page is about 1 MiB).
pub const PAGES: usize = 16;


/// What a request asks for.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Route {
    /// A catalog page (1–10).
    Page(usize),
    /// A thread's page.
    Thread(u64),
    Pow,
    /// `POST /submit` with this `Content-Length` (≤ [`MAX_SUBMIT`]).
    Submit(usize),
    Record,
    Index,
    Car(Cid),
    Raw(Cid),
}

/// The lines of a request head (CRLF; a bare LF is tolerated), the request line first.
fn lines(head: &[u8]) -> impl Iterator<Item = &[u8]> {
    head.split(|&b| b == b'\n').map(|l| l.strip_suffix(b"\r").unwrap_or(l)).take_while(|l| !l.is_empty())
}

fn header<'a>(head: &'a [u8], name: &[u8]) -> Option<&'a [u8]> {
    lines(head).skip(1).find_map(|l| {
        let colon = l.iter().position(|&b| b == b':')?;
        l[..colon].eq_ignore_ascii_case(name).then(|| l[colon + 1..].trim_ascii())
    })
}

fn query_format(query: &[u8]) -> &[u8] {
    query.split(|&b| b == b'&').find_map(|kv| kv.strip_prefix(b"format=")).unwrap_or(b"")
}

/// Whether `buf` holds a whole request head.
pub fn head_complete(buf: &[u8]) -> bool {
    buf.windows(4).any(|w| w == b"\r\n\r\n")
}

/// The route of one request head, or the status to refuse it with. No allocation on the
/// `/submit` and `/pow` paths (G.6.2: the refusal path).
pub fn route(head: &[u8], name: &str) -> Result<Route, u16> {
    if head.len() > MAX_HEAD {
        return Err(431);
    }
    let first = lines(head).next().ok_or(400u16)?;
    let mut parts = first.split(|&b| b == b' ');
    let (method, target, version) = (parts.next().unwrap_or(b""), parts.next().unwrap_or(b""), parts.next().unwrap_or(b""));
    if !version.starts_with(b"HTTP/1.") || parts.next().is_some() {
        return Err(400);
    }
    let (path, query) = match target.iter().position(|&b| b == b'?') {
        Some(q) => (&target[..q], &target[q + 1..]),
        None => (target, &b""[..]),
    };
    if path == b"/submit" {
        if method != b"POST" {
            return Err(405);
        }
        if header(head, b"content-type") != Some(CT_SUBMIT.as_bytes()) {
            return Err(415);
        }
        let len = header(head, b"content-length").and_then(|v| std::str::from_utf8(v).ok()).and_then(|v| v.parse::<usize>().ok()).ok_or(411u16)?;
        if header(head, b"transfer-encoding").is_some() {
            return Err(400);
        }
        return if len > MAX_SUBMIT { Err(413) } else { Ok(Route::Submit(len)) };
    }
    if method != b"GET" {
        return Err(405);
    }
    let format = query_format(query);
    match path {
        b"/" | b"/index.html" => {
            let p = query.split(|&b| b == b'&').find_map(|kv| kv.strip_prefix(b"p=")).and_then(|v| std::str::from_utf8(v).ok()).and_then(|v| v.parse().ok()).unwrap_or(1);
            return Ok(Route::Page(p));
        }
        b"/pow" => return Ok(Route::Pow),
        _ => {}
    }
    if let Some(no) = path.strip_prefix(b"/t/") {
        return std::str::from_utf8(no).ok().and_then(|n| n.parse().ok()).map(Route::Thread).ok_or(404);
    }
    if let Some(n) = path.strip_prefix(b"/ipns/") {
        if n != name.as_bytes() {
            return Err(404);
        }
        return match format {
            b"ipns-record" => Ok(Route::Record),
            b"ephem-board" => Ok(Route::Index),
            _ => Err(406),
        };
    }
    if let Some(c) = path.strip_prefix(b"/ipfs/") {
        let cid = std::str::from_utf8(c).ok().and_then(Cid::parse).ok_or(400u16)?;
        return match format {
            b"car" => Ok(Route::Car(cid)),
            b"raw" => Ok(Route::Raw(cid)),
            _ => Err(406),
        };
    }
    Err(404)
}

fn reason(status: u16) -> &'static str {
    match status {
        200 => "OK",
        400 => "Bad Request",
        403 => "Forbidden",
        404 => "Not Found",
        405 => "Method Not Allowed",
        406 => "Not Acceptable",
        408 => "Request Timeout",
        409 => "Conflict",
        411 => "Length Required",
        413 => "Content Too Large",
        415 => "Unsupported Media Type",
        423 => "Locked",
        431 => "Request Header Fields Too Large",
        _ => "Service Unavailable",
    }
}

fn head_into(w: &mut impl Write, status: u16, ctype: &str, extra: &str, len: usize) -> std::io::Result<()> {
    write!(w, "HTTP/1.1 {status} {}\r\nContent-Type: {ctype}\r\nContent-Length: {len}\r\nCache-Control: no-store\r\nX-Content-Type-Options: nosniff\r\n{extra}Connection: close\r\n\r\n", reason(status))
}

/// A short response into `out`: its length (no allocation).
fn short(status: u16, ctype: &str, body: &[u8], out: &mut [u8; SHORT]) -> usize {
    let mut w = &mut out[..];
    head_into(&mut w, status, ctype, "", body.len()).and_then(|()| w.write_all(body)).expect("a short response fits");
    SHORT - w.len()
}

/// A refusal: the status and the stable `u16` code (G.6.3), little-endian.
pub fn refusal(r: Refusal, out: &mut [u8; SHORT]) -> usize {
    short(r.status(), CT_ANSWER, &r.code().to_le_bytes(), out)
}

/// A status with no code (a malformed request: 400, 405, 411, 413, 415, 431, 408).
pub fn status(status: u16, out: &mut [u8; SHORT]) -> usize {
    short(status, CT_ANSWER, &[], out)
}

/// A post published: `{no, seq}`.
pub fn answer(no: u64, seq: u64, out: &mut [u8; SHORT]) -> usize {
    let mut b = [0u8; ANSWER_LEN];
    b[..8].copy_from_slice(&no.to_le_bytes());
    b[8..].copy_from_slice(&seq.to_le_bytes());
    short(200, CT_ANSWER, &b, out)
}

/// The `/pow` answer.
pub fn pow(info: &PowInfo, out: &mut [u8; SHORT]) -> usize {
    let mut b = [0u8; PowInfo::LEN];
    info.write(&mut b);
    short(200, CT_POW, &b, out)
}

fn response(status: u16, ctype: &str, extra: &str, body: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(256 + body.len());
    head_into(&mut out, status, ctype, extra, body.len()).expect("a Vec grows");
    out.extend_from_slice(body);
    out
}

/// What the board's onion serves: the current version, verified (the owner built it; a mirror
/// checked it). Updated in place on each publish.
pub struct Served {
    pub name: Cid,
    name_text: String,
    pub root: Cid,
    pub record: Vec<u8>,
    blocks: HashMap<Cid, Vec<u8>>,
    /// The root, `threads` and `arch_threads`: a CAR of any of them is the whole board.
    whole: [Cid; 3],
    /// The whole `/ipns/<name>?format=ephem-board` response of this version.
    index: Rc<[u8]>,
    /// Who serves it (the plain pages say so).
    pub served: page::Served,
    /// The record's sequence (checked once, not per page).
    seq: u64,
    /// Plain pages of this version, most recent last.
    pages: std::cell::RefCell<Vec<(Route, Rc<[u8]>)>>,
}

impl page::Blocks for Served {
    fn block(&self, cid: &Cid) -> Option<&[u8]> {
        self.blocks.get(cid).map(Vec::as_slice)
    }
}

/// The CIDs of the index (the root first), from the root block.
fn index_cids(root: &Cid, blocks: &HashMap<Cid, Vec<u8>>) -> Option<(Vec<Cid>, [Cid; 3])> {
    let r = Value::decode(blocks.get(root)?)?;
    let l = |k: &str| r.get(k).and_then(Value::link).cloned();
    let (threads, arch_threads) = (l("threads")?, l("arch_threads")?);
    let mut out = Vec::with_capacity(18);
    out.push(root.clone());
    for k in ["manifest", "archive", "dels", "modlog"] {
        out.push(l(k)?);
    }
    out.push(threads.clone());
    out.push(arch_threads.clone());
    for b in r.get("cat")?.array()? {
        out.push(b.link()?.clone());
    }
    Some((out, [root.clone(), threads, arch_threads]))
}

impl Served {
    /// `blocks`: the whole state under `root` (more is dropped: only what the root links to is
    /// served, as channels, M-4). `None` if the root is not a board's.
    pub fn new(name: Cid, root: Cid, record: Vec<u8>, blocks: Vec<Block>, served: page::Served) -> Option<Self> {
        let blocks: HashMap<Cid, Vec<u8>> = ephem_channel::gateway::reachable(&root, blocks).into_iter().collect();
        let (cids, whole) = index_cids(&root, &blocks)?;
        let index = index_response(&record, &root, &cids, &blocks)?;
        let seq = ephem_channel::ipns::verify(&name, &record, 0).map_or(0, |r| r.sequence);
        Some(Self { name_text: name.to_text(), name, root, record, blocks, whole, index, served, seq, pages: Default::default() })
    }

    /// The next version: `added` blocks (those the store does not hold yet) and `live`, every
    /// CID of the new state. Returns the CIDs no longer served (the store deletes them).
    pub fn update(&mut self, root: Cid, record: Vec<u8>, added: Vec<Block>, live: &[Cid]) -> Option<Vec<Cid>> {
        for (c, b) in added {
            self.blocks.insert(c, b);
        }
        let keep: HashSet<&Cid> = live.iter().collect();
        let gone: Vec<Cid> = self.blocks.keys().filter(|c| !keep.contains(c)).cloned().collect();
        for c in &gone {
            self.blocks.remove(c);
        }
        let (cids, whole) = index_cids(&root, &self.blocks)?;
        self.index = index_response(&record, &root, &cids, &self.blocks)?;
        self.seq = ephem_channel::ipns::verify(&self.name, &record, 0).map_or(0, |r| r.sequence);
        self.pages.borrow_mut().clear();
        (self.root, self.record, self.whole) = (root, record, whole);
        Some(gone)
    }

    /// The board's name as text (`k51…`), what [`route`] compares.
    pub fn name_text(&self) -> &str {
        &self.name_text
    }

    pub fn block(&self, cid: &Cid) -> Option<&[u8]> {
        self.blocks.get(cid).map(Vec::as_slice)
    }

    /// Every held block (the store and backups).
    pub fn blocks(&self) -> impl Iterator<Item = (&Cid, &[u8])> {
        self.blocks.iter().map(|(c, b)| (c, b.as_slice()))
    }

    /// A plain page, from the cache or built now.
    fn page(&self, r: &Route) -> Rc<[u8]> {
        if let Some((_, p)) = self.pages.borrow().iter().find(|(x, _)| x == r) {
            return p.clone();
        }
        let p = match r {
            Route::Page(n) => html(page::catalog(self, &self.name_text, &self.root, self.seq, self.served, *n)),
            Route::Thread(no) => html(page::thread(self, &self.name_text, &self.root, self.seq, self.served, *no)),
            _ => unreachable!("a page route"),
        };
        let mut pages = self.pages.borrow_mut();
        if pages.len() == PAGES {
            pages.remove(0);
        }
        pages.push((r.clone(), p.clone()));
        p
    }

    /// The response to a read route (`Pow` and `Submit` belong to the host's intake).
    pub fn respond(&self, r: &Route) -> Rc<[u8]> {
        match r {
            Route::Page(_) | Route::Thread(_) => self.page(r),
            Route::Record => response(200, CT_RECORD, "", &self.record).into(),
            Route::Index => self.index.clone(),
            Route::Raw(c) => match self.blocks.get(c) {
                Some(b) => response(200, CT_RAW, "", b).into(),
                None => response(404, CT_ANSWER, "", b"").into(),
            },
            Route::Car(c) if self.whole.contains(c) => response(406, CT_ANSWER, "", b"").into(),
            Route::Car(c) => match self.dag(c) {
                Some(car) if car.len() <= MAX_CAR => response(200, CT_CAR, "", &car).into(),
                Some(_) => response(406, CT_ANSWER, "", b"").into(),
                None => response(404, CT_ANSWER, "", b"").into(),
            },
            Route::Pow | Route::Submit(_) => response(404, CT_ANSWER, "", b"").into(),
        }
    }

    /// A CAR of the blocks under `cid` (a thread and its chunks; one block otherwise).
    fn dag(&self, cid: &Cid) -> Option<Vec<u8>> {
        let mut order: Vec<&Cid> = Vec::with_capacity(16);
        let mut todo = vec![cid.clone()];
        let mut seen = HashSet::new();
        while let Some(c) = todo.pop() {
            let (k, data) = self.blocks.get_key_value(&c)?;
            if !seen.insert(k) {
                continue;
            }
            if let Some(v) = Value::decode(data) {
                ephem_channel::gateway::links(&v, &mut todo);
            }
            order.push(k);
        }
        let mut out = Vec::with_capacity(order.iter().map(|c| self.blocks[*c].len() + 48).sum::<usize>() + 64);
        car::write_into(&mut out, std::slice::from_ref(cid), order.into_iter().map(|c| (c, self.blocks[c].as_slice())));
        Some(out)
    }
}

/// A plain page with its headers (no scripts, no referrer), or the board's 404 page.
fn html(page: Option<Vec<u8>>) -> Rc<[u8]> {
    let extra = format!("Content-Security-Policy: {}\r\nReferrer-Policy: no-referrer\r\n", page::CSP);
    match page {
        Some(h) => response(200, "text/html; charset=utf-8", &extra, &h).into(),
        None => response(404, "text/html; charset=utf-8", &extra, b"<!doctype html><title>Not found</title><p>Not on this board (pruned, deleted, or never there). <a href=\"/\">The board</a>").into(),
    }
}

fn index_response(record: &[u8], root: &Cid, cids: &[Cid], blocks: &HashMap<Cid, Vec<u8>>) -> Option<Rc<[u8]>> {
    let mut body = Vec::with_capacity(2 + record.len() + cids.iter().map(|c| blocks.get(c).map_or(0, Vec::len) + 48).sum::<usize>());
    body.extend_from_slice(&u16::try_from(record.len()).ok()?.to_le_bytes());
    body.extend_from_slice(record);
    let held: Vec<(&Cid, &[u8])> = cids.iter().map(|c| blocks.get(c).map(|b| (c, b.as_slice()))).collect::<Option<_>>()?;
    car::write_into(&mut body, std::slice::from_ref(root), held.into_iter());
    Some(response(200, CT_INDEX, "", &body).into())
}

// ---- the poster's and reader's side ----

/// A poster's `POST /submit` (head and body).
pub fn submit_request(host: &str, body: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(160 + body.len());
    write!(out, "POST /submit HTTP/1.1\r\nHost: {host}\r\nContent-Type: {CT_SUBMIT}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n", body.len()).expect("a Vec grows");
    out.extend_from_slice(body);
    out
}

/// The host's answer to a submit.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum Answer {
    Posted { no: u64, seq: u64 },
    /// A refusal: the HTTP status and the `u16` code (0 when the body carried none).
    Refused { status: u16, code: u16 },
}

/// Parses a whole submit response.
pub fn parse_answer(resp: &[u8]) -> Option<Answer> {
    let (status, body) = ephem_channel::gateway::parse_any_response(resp).ok()?;
    if status == 200 {
        let b: &[u8; ANSWER_LEN] = body.as_slice().try_into().ok()?;
        let u = |at: usize| u64::from_le_bytes(b[at..at + 8].try_into().expect("8 bytes"));
        return Some(Answer::Posted { no: u(0), seq: u(8) });
    }
    let code = body.get(..2).map_or(0, |c| u16::from_le_bytes([c[0], c[1]]));
    Some(Answer::Refused { status, code })
}

/// The index body: the record, the root and its blocks (each checked against its CID by
/// `car::read`). `None` if malformed or over the caps.
pub fn parse_index(body: &[u8]) -> Option<(Vec<u8>, Cid, Vec<Block>)> {
    let len = usize::from(u16::from_le_bytes([*body.first()?, *body.get(1)?]));
    if len > MAX_RECORD || body.len() > 2 + MAX_RECORD + MAX_CAR {
        return None;
    }
    let record = body.get(2..2 + len)?.to_vec();
    let (roots, blocks) = car::read(&body[2 + len..])?;
    let root = roots.into_iter().next()?;
    Some((record, root, blocks))
}
