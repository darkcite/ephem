// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
// Copyright 2026 Anton (darkcite)
//! The board as plain web pages for Tor Browser (G.11.2): `/` (the catalog, 10 pages of 15,
//! `/?p=2`…) and `/t/<no>` (a thread). No JavaScript, no forms, no images: posting needs the app
//! (B3). Served under the channel page's CSP.
//!
//! Built on request from the blocks the onion serves, which were checked before they got there
//! (the owner built them; a mirror verified every signature when copying), so here they are
//! only decoded. The onion address vouches for what is shown; the page says whether it is the
//! board's own onion or a mirror's.

use crate::board::cap;
use crate::limits;
use crate::post::trip_text;
use ephem_channel::cbor::Value;
use ephem_channel::cid::Cid;
use ephem_channel::page::esc;
use ephem_channel::time::rfc3339;
use std::fmt::Write;

pub use ephem_channel::page::{CSP, Served};

/// Threads per catalog page (10 pages, B4).
pub const PER_PAGE: usize = 15;

const STYLE: &str = "h1,h2,.body,.ex,.about,.rules,li{unicode-bidi:isolate}body{margin:0;background:#0f1115;color:#d7dbe3;font:15px/1.5 ui-monospace,Menlo,Consolas,monospace}\
main{max-width:860px;margin:0 auto;padding:16px}h1{font-size:20px;margin:8px 0}h2{font-size:16px;margin:4px 0}a{color:#5b9cf5}\
.about,.rules{color:#8a93a6;white-space:pre-wrap}.box{border:1px solid #262b36;border-radius:6px;padding:10px 12px;margin:14px 0;background:#171a21;font-size:13px}\
.box b{color:#39c26c}.mirror b{color:#e2b13c}ol{list-style:none;padding:0}li{border:1px solid #262b36;border-radius:6px;padding:8px 12px;margin:8px 0;background:#171a21}\
.meta,.note,footer,.pages{color:#8a93a6;font-size:12px}.trip{color:#39c26c}.cap{color:#e2b13c}.ex,.body{white-space:pre-wrap;overflow-wrap:anywhere}\
.gt{color:#8fbf5a}.deleted .body{font-style:italic;color:#8a93a6}footer{border-top:1px solid #262b36;margin-top:18px;padding-top:10px;overflow-wrap:anywhere}";

/// What a page needs from the served blocks.
pub trait Blocks {
    fn block(&self, cid: &Cid) -> Option<&[u8]>;
}

fn node<B: Blocks + ?Sized>(b: &B, c: &Cid) -> Option<Value> {
    Value::decode(b.block(c)?)
}

struct Row {
    no: u64,
    thread: Cid,
    bump: u64,
    r: u64,
    sub: String,
    ex: String,
    st: bool,
    lk: bool,
}

struct Head {
    title: String,
    about: String,
    rules: String,
    see_also: Vec<String>,
    rows: Vec<Row>,
    updated: u64,
}

fn head<B: Blocks + ?Sized>(b: &B, root: &Cid) -> Option<Head> {
    let r = node(b, root)?;
    let m = node(b, r.get("manifest")?.link()?)?;
    let t = |v: &Value, k: &str| v.get(k).and_then(Value::text).map(str::to_owned);
    let mut rows = Vec::with_capacity(limits::THREADS);
    for bucket in r.get("cat")?.array()? {
        for e in node(b, bucket.link()?)?.get("t")?.array()? {
            rows.push(Row {
                no: e.get("no")?.uint()?,
                thread: e.get("thread").and_then(Value::bytes).and_then(Cid::from_bytes)?,
                bump: e.get("bump")?.uint()?,
                r: e.get("r")?.uint()?,
                sub: t(e, "sub")?,
                ex: t(e, "ex")?,
                st: e.get("st")?.boolean()?,
                lk: e.get("lk")?.boolean()?,
            });
        }
    }
    rows.sort_by(|a, b| b.st.cmp(&a.st).then(b.bump.cmp(&a.bump)).then(b.no.cmp(&a.no)));
    let see_also = m.get("see_also").and_then(Value::array).map(|a| a.iter().filter_map(|x| x.text().map(str::to_owned)).collect()).unwrap_or_default();
    Some(Head { title: t(&m, "title")?, about: t(&m, "about")?, rules: t(&m, "rules")?, see_also, rows, updated: r.get("updated")?.uint()? })
}

fn open(s: &mut String, title: &str) {
    s.push_str("<!doctype html><html lang=\"en\"><head><meta charset=\"utf-8\"><meta name=\"viewport\" content=\"width=device-width, initial-scale=1\"><meta name=\"referrer\" content=\"no-referrer\"><title>");
    esc(s, title);
    s.push_str("</title><style>");
    s.push_str(STYLE);
    s.push_str("</style></head><body><main>");
}

fn served_box(s: &mut String, served: Served, seq: u64, updated: u64) {
    let version = format!("version {seq}, updated {}", rfc3339(updated));
    match served {
        Served::Owner => {
            s.push_str("<div class=\"box\"><b>Served by the board's own onion address</b>, from its owner's Ephem tab: this is the board as its owner last published it (");
            esc(s, &version);
            s.push_str("). Posting needs the Ephem app.</div>");
        }
        Served::Mirror => {
            s.push_str("<div class=\"box mirror\"><b>Served by a mirror</b>, a reader's Ephem that keeps a copy of this board and checked every signature when it copied it (");
            esc(s, &version);
            s.push_str("). This address vouches for the mirror, not for the owner. Posting needs the Ephem app and the board's own onion.</div>");
        }
    }
}

fn close(s: &mut String, name: &str) {
    s.push_str("<footer>Board name (IPNS): ");
    esc(s, name);
    s.push_str("<br>A read-only page: no scripts, no cookies, nothing you send. To post, follow or verify the board, open the Ephem link its owner shared.</footer></main></body></html>");
}

/// Catalog page `page` (1–10), or `None` if the root cannot be read.
pub fn catalog<B: Blocks + ?Sized>(b: &B, name: &str, root: &Cid, seq: u64, served: Served, page: usize) -> Option<Vec<u8>> {
    let h = head(b, root)?;
    let pages = h.rows.len().div_ceil(PER_PAGE).max(1);
    let page = page.clamp(1, pages);
    let mut s = String::with_capacity(8192);
    open(&mut s, &h.title);
    s.push_str("<h1>");
    esc(&mut s, &h.title);
    s.push_str("</h1><p class=\"about\">");
    esc(&mut s, &h.about);
    s.push_str("</p>");
    if !h.rules.is_empty() {
        s.push_str("<details><summary>Rules</summary><p class=\"rules\">");
        esc(&mut s, &h.rules);
        s.push_str("</p></details>");
    }
    if !h.see_also.is_empty() {
        s.push_str("<p class=\"note\">See also: ");
        for l in &h.see_also {
            if let Some((_, onion)) = l.split_once('@') {
                s.push_str("<a href=\"http://");
                esc(&mut s, onion);
                s.push_str("/\">");
                esc(&mut s, onion);
                s.push_str("</a> ");
            }
        }
        s.push_str("</p>");
    }
    served_box(&mut s, served, seq, h.updated);
    s.push_str("<ol>");
    for r in h.rows.iter().skip((page - 1) * PER_PAGE).take(PER_PAGE) {
        let _ = write!(s, "<li><h2><a href=\"/t/{}\">No. {}</a> ", r.no, r.no);
        esc(&mut s, &r.sub);
        let _ = write!(s, "</h2><div class=\"meta\">{} replies · bumped {}{}{}</div><div class=\"ex\">", r.r, rfc3339(r.bump), if r.st { " · sticky" } else { "" }, if r.lk { " · locked" } else { "" });
        esc(&mut s, &r.ex);
        s.push_str("</div></li>");
    }
    s.push_str("</ol>");
    if h.rows.is_empty() {
        s.push_str("<p class=\"note\">No threads yet.</p>");
    }
    s.push_str("<p class=\"pages\">Pages: ");
    for p in 1..=pages {
        if p == page {
            let _ = write!(s, "<b>{p}</b> ");
        } else {
            let _ = write!(s, "<a href=\"/?p={p}\">{p}</a> ");
        }
    }
    s.push_str("</p>");
    close(&mut s, name);
    Some(s.into_bytes())
}

/// Thread `no`'s page; `None` if it is not on the board (pruned or deleted) or not held.
pub fn thread<B: Blocks + ?Sized>(b: &B, name: &str, root: &Cid, seq: u64, served: Served, no: u64) -> Option<Vec<u8>> {
    let h = head(b, root)?;
    let row = h.rows.iter().find(|r| r.no == no)?;
    let t = node(b, &row.thread)?;
    // Never sized from a block field (BC-9): a mirror serves what a hostile owner signed.
    let mut s = String::with_capacity(16 * 1024);
    open(&mut s, &format!("{} · {}", h.title, if row.sub.is_empty() { format!("No. {no}") } else { row.sub.clone() }));
    s.push_str("<p><a href=\"/\">← ");
    esc(&mut s, &h.title);
    s.push_str("</a></p><h1>");
    esc(&mut s, &row.sub);
    s.push_str("</h1>");
    served_box(&mut s, served, seq, h.updated);
    s.push_str("<ol>");
    for c in t.get("chunks")?.array()? {
        for p in node(b, c.link()?)?.get("p")?.array()? {
            post(&mut s, p);
        }
    }
    s.push_str("</ol>");
    if row.lk {
        s.push_str("<p class=\"note\">This thread is locked.</p>");
    }
    close(&mut s, name);
    Some(s.into_bytes())
}

fn post(s: &mut String, p: &Value) {
    let no = p.get("no").and_then(Value::uint).unwrap_or(0);
    let ts = p.get("ts").and_then(Value::uint).unwrap_or(0);
    let Some(sv) = p.get("s") else {
        let _ = write!(s, "<li class=\"deleted\" id=\"p{no}\"><div class=\"meta\">No. {no} · {}</div><div class=\"body\">(deleted)</div></li>", rfc3339(ts));
        return;
    };
    let text = |k: &str| sv.get(k).and_then(Value::text).unwrap_or("");
    let _ = write!(s, "<li id=\"p{no}\"><div class=\"meta\">No. {no} · {}", rfc3339(ts));
    if sv.get("trip").and_then(Value::boolean) == Some(true)
        && let Some(k) = sv.get("k").and_then(Value::bytes).and_then(|k| <[u8; 32]>::try_from(k).ok())
    {
        s.push_str(" · <span class=\"trip\">");
        esc(s, &trip_text(&k));
        s.push_str("</span>");
    }
    if p.get("cap").and_then(Value::uint) == Some(u64::from(cap::OWNER)) {
        s.push_str(" · <span class=\"cap\">## Owner</span>");
    }
    if sv.get("sage").and_then(Value::boolean) == Some(true) {
        s.push_str(" · sage");
    }
    s.push_str("</div><div class=\"body\">");
    // Greentext: lines starting with '>' (not '>>N' links) in green.
    for (i, line) in text("body").split('\n').enumerate() {
        if i > 0 {
            s.push('\n');
        }
        if line.starts_with('>') && !line.starts_with(">>") {
            s.push_str("<span class=\"gt\">");
            esc(s, line);
            s.push_str("</span>");
        } else {
            esc(s, line);
        }
    }
    s.push_str("</div></li>");
}
