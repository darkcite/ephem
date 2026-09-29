// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
// Copyright 2026 Anton (darkcite)
//! The read-only subset of the IPFS trustless-gateway API a channel's onion serves (§D.2):
//!
//! - `GET /ipns/<name>?format=ipns-record` → the signed record;
//! - `GET /ipfs/<cid>?format=car` → a CAR of the DAG under `<cid>`;
//! - `GET /ipfs/<cid>?format=raw` → one block.
//!
//! Sans-IO: [`respond`] turns one request head into the whole response; [`get`] and
//! [`parse_response`] are the reader's side. HTTP/1.1 with `Connection: close` (one request per
//! Tor stream), no chunking, no keep-alive: simple enough to review completely.

use crate::car::{self, Block};
use crate::cbor::Value;
use crate::cid::Cid;
use std::collections::{HashMap, HashSet};

/// Largest request head read before answering 431.
pub const MAX_HEAD: usize = 8 * 1024;
/// Largest response a reader accepts (a channel of a few thousand posts is a few MB).
pub const MAX_RESPONSE: usize = 64 * 1024 * 1024;
pub const CT_CAR: &str = "application/vnd.ipld.car";
pub const CT_RAW: &str = "application/vnd.ipld.raw";
pub const CT_RECORD: &str = "application/vnd.ipfs.ipns-record";

/// What one onion serves: one channel (the owner's, or a mirrored one), already verified.
pub struct Hosted {
    pub name: Cid,
    pub root: Cid,
    pub record: Vec<u8>,
    blocks: HashMap<Cid, Vec<u8>>,
}

impl Hosted {
    pub fn new(name: Cid, root: Cid, record: Vec<u8>, blocks: Vec<Block>) -> Self {
        Self { name, root, record, blocks: blocks.into_iter().collect() }
    }

    /// The blocks reachable from `cid` (itself first), or `None` if it is not held.
    pub fn dag(&self, cid: &Cid) -> Option<Vec<Block>> {
        let mut out = Vec::new();
        let mut seen = HashSet::new();
        let mut todo = vec![cid.clone()];
        while let Some(c) = todo.pop() {
            if !seen.insert(c.clone()) {
                continue;
            }
            let data = self.blocks.get(&c)?;
            if let Some(v) = Value::decode(data) {
                links(&v, &mut todo);
            }
            out.push((c, data.clone()));
        }
        Some(out)
    }

    /// The whole channel as one CAR rooted at its root (the backup and Kubo import format).
    pub fn car(&self) -> Vec<u8> {
        car::write(std::slice::from_ref(&self.root), &self.dag(&self.root).unwrap_or_default())
    }
}

fn links(v: &Value, out: &mut Vec<Cid>) {
    match v {
        Value::Link(c) => out.push(c.clone()),
        Value::Array(a) => a.iter().for_each(|x| links(x, out)),
        Value::Map(m) => m.iter().for_each(|(_, x)| links(x, out)),
        _ => {}
    }
}

fn response(status: &str, ctype: &str, body: &[u8]) -> Vec<u8> {
    let mut out = format!(
        "HTTP/1.1 {status}\r\nContent-Type: {ctype}\r\nContent-Length: {}\r\nCache-Control: no-store\r\nX-Content-Type-Options: nosniff\r\nConnection: close\r\n\r\n",
        body.len()
    )
    .into_bytes();
    out.extend_from_slice(body);
    out
}

fn error(status: &str) -> Vec<u8> {
    response(status, "text/plain; charset=utf-8", status.as_bytes())
}

/// Whether `buf` holds a complete request head (ends with an empty line).
pub fn head_complete(buf: &[u8]) -> bool {
    buf.windows(4).any(|w| w == b"\r\n\r\n")
}

/// The whole HTTP response to one request head.
pub fn respond(head: &[u8], h: &Hosted) -> Vec<u8> {
    if head.len() > MAX_HEAD {
        return error("431 Request Header Fields Too Large");
    }
    let Ok(text) = std::str::from_utf8(head) else { return error("400 Bad Request") };
    let mut lines = text.split("\r\n");
    let mut first = lines.next().unwrap_or("").split(' ');
    let (method, target, version) = (first.next().unwrap_or(""), first.next().unwrap_or(""), first.next().unwrap_or(""));
    if !version.starts_with("HTTP/1.") {
        return error("400 Bad Request");
    }
    if method != "GET" {
        return error("405 Method Not Allowed");
    }
    let accept = lines.find_map(|l| l.split_once(':').filter(|(k, _)| k.eq_ignore_ascii_case("accept")).map(|(_, v)| v.trim().to_owned())).unwrap_or_default();
    let (path, query) = target.split_once('?').unwrap_or((target, ""));
    let format = query.split('&').find_map(|kv| kv.strip_prefix("format=")).unwrap_or("");
    let wants = |f: &str, ct: &str| format == f || accept.contains(ct);
    if let Some(name) = path.strip_prefix("/ipns/") {
        return match Cid::parse(name) {
            Some(n) if n == h.name && wants("ipns-record", CT_RECORD) => response("200 OK", CT_RECORD, &h.record),
            Some(n) if n == h.name => error("406 Not Acceptable"),
            _ => error("404 Not Found"),
        };
    }
    if let Some(cid) = path.strip_prefix("/ipfs/") {
        let Some(cid) = Cid::parse(cid.trim_end_matches('/')) else { return error("400 Bad Request") };
        if wants("car", CT_CAR) {
            return match h.dag(&cid) {
                Some(blocks) => response("200 OK", CT_CAR, &car::write(std::slice::from_ref(&cid), &blocks)),
                None => error("404 Not Found"),
            };
        }
        if wants("raw", CT_RAW) {
            return match h.blocks.get(&cid) {
                Some(b) => response("200 OK", CT_RAW, b),
                None => error("404 Not Found"),
            };
        }
        return error("406 Not Acceptable");
    }
    error("404 Not Found")
}

/// A reader's request head for `path` (`/ipns/…?format=ipns-record`, `/ipfs/…?format=car`).
pub fn get(host: &str, path: &str) -> Vec<u8> {
    format!("GET {path} HTTP/1.1\r\nHost: {host}\r\nAccept: */*\r\nConnection: close\r\n\r\n").into_bytes()
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum HttpError {
    Malformed,
    Status(u16),
    TooLarge,
    /// The body ended before `Content-Length`.
    Truncated,
}

/// Whether `resp` holds a whole response: the head and `Content-Length` bytes of body. A reader
/// stops there: how the server then closes the Tor stream (arti ends it with reason MISC,
/// which a reader sees as an error) does not matter.
pub fn complete(resp: &[u8]) -> bool {
    let Some(end) = resp.windows(4).position(|w| w == b"\r\n\r\n") else { return false };
    let Ok(head) = std::str::from_utf8(&resp[..end]) else { return true };
    match head.split("\r\n").find_map(|l| l.split_once(':').filter(|(k, _)| k.eq_ignore_ascii_case("content-length")).map(|(_, v)| v.trim().parse::<usize>())) {
        Some(Ok(n)) => resp.len() - end - 4 >= n,
        _ => false,
    }
}

/// Parses a whole response (read until complete or closed); returns the body.
pub fn parse_response(resp: &[u8]) -> Result<&[u8], HttpError> {
    if resp.len() > MAX_RESPONSE + MAX_HEAD {
        return Err(HttpError::TooLarge);
    }
    let end = resp.windows(4).position(|w| w == b"\r\n\r\n").ok_or(HttpError::Malformed)?;
    let head = std::str::from_utf8(&resp[..end]).map_err(|_| HttpError::Malformed)?;
    let mut lines = head.split("\r\n");
    let status: u16 = lines.next().and_then(|l| l.split(' ').nth(1)).and_then(|s| s.parse().ok()).ok_or(HttpError::Malformed)?;
    if status != 200 {
        return Err(HttpError::Status(status));
    }
    let body = &resp[end + 4..];
    let len = lines.find_map(|l| l.split_once(':').filter(|(k, _)| k.eq_ignore_ascii_case("content-length")).map(|(_, v)| v.trim().parse::<usize>()));
    match len {
        Some(Ok(n)) if n > MAX_RESPONSE => Err(HttpError::TooLarge),
        Some(Ok(n)) if body.len() < n => Err(HttpError::Truncated),
        Some(Ok(n)) => Ok(&body[..n]),
        Some(Err(_)) => Err(HttpError::Malformed),
        None => Ok(body),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::channel::{self, Channel};

    #[test]
    fn serve_and_read() {
        let mut c = Channel::new(&[3; 32], "t", "", 1_790_000_000).unwrap();
        c.post("one", 0, 1_790_000_001).unwrap();
        let (root, blocks) = c.build(1_790_000_002);
        let record = c.record(&root, 1_790_000_002);
        let h = Hosted::new(c.name(), root.clone(), record.clone(), blocks);
        let name = c.name().to_text();
        let rec = respond(&get("x.onion", &format!("/ipns/{name}?format=ipns-record")), &h);
        assert_eq!(parse_response(&rec), Ok(&record[..]));
        let car = respond(&get("x.onion", &format!("/ipfs/{}?format=car", root.to_text())), &h);
        let (_, read) = car::read(parse_response(&car).unwrap()).unwrap();
        let v = channel::verify(&c.name(), &record, &read, 1_790_000_003, 0).unwrap();
        assert_eq!(v.posts[0].body, "one");
        // One block, raw.
        let raw = respond(&get("x", &format!("/ipfs/{}?format=raw", root.to_text())), &h);
        assert!(root.verifies(parse_response(&raw).unwrap()));
        // What is not served.
        assert_eq!(parse_response(&respond(&get("x", "/ipfs/bafyreigbtj4x7ip5legnfznufuopl4sg4knzc2cof6duas4b3q2fy6swua?format=car"), &h)), Err(HttpError::Status(404)));
        assert_eq!(parse_response(&respond(b"POST /ipfs/x HTTP/1.1\r\n\r\n", &h)), Err(HttpError::Status(405)));
        assert_eq!(parse_response(&respond(&get("x", &format!("/ipns/{name}")), &h)), Err(HttpError::Status(406)));
        assert_eq!(parse_response(&respond(&vec![b'a'; MAX_HEAD + 1], &h)), Err(HttpError::Status(431)));
        assert_eq!(parse_response(b"HTTP/1.1 200 OK\r\nContent-Length: 5\r\n\r\nab"), Err(HttpError::Truncated));
        assert!(!complete(b"HTTP/1.1 200 OK\r\nContent-Length: 5\r\n\r\nab"));
        assert!(complete(&rec), "a served response is complete by its Content-Length");
        assert!(!complete(&rec[..rec.len() - 1]));
    }
}
