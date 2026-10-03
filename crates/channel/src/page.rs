// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
// Copyright 2026 Anton (darkcite)
//! The channel as a plain web page (§D.2): what an onion serves at `/`, so anyone can read the
//! channel in Tor Browser by its onion address alone, with no JavaScript and no Ephem.
//!
//! The page carries no signatures a browser could check; the onion address is what vouches
//! for it (Tor authenticates onion services by their address). On the owner's onion that
//! address belongs to the channel (its key is derived from the owner's identity, §D.3); on a
//! mirror it belongs to a follower, who checked every signature when copying. The page says
//! which of the two it is. Built once per version (setup path), then served as is.

use crate::channel::{Post, View};
use crate::time::rfc3339;

/// Who serves the page.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum Served {
    /// The channel's own onion, from its owner's tab.
    Owner,
    /// A follower's mirror onion (§D.7.1).
    Mirror,
}

/// Newest posts shown; older ones are in the channel (the Ephem link, or its CAR).
pub const MAX_POSTS: usize = 200;

/// The response headers of the page: no scripts, frames, forms, images or referrers.
pub const CSP: &str = "default-src 'none'; style-src 'unsafe-inline'; base-uri 'none'; form-action 'none'; frame-ancestors 'none'";

/// Escapes text for HTML element content and quoted attributes.
pub fn esc(out: &mut String, s: &str) {
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#39;"),
            _ => out.push(c),
        }
    }
}

/// The first `n` characters of `s` (on a character boundary).
fn head(s: &str, n: usize) -> &str {
    s.char_indices().nth(n).map_or(s, |(i, _)| &s[..i])
}

const STYLE: &str = "body{margin:0;background:#0f1115;color:#d7dbe3;font:15px/1.5 ui-monospace,Menlo,Consolas,monospace}\
main{max-width:720px;margin:0 auto;padding:16px}h1{font-size:20px;margin:8px 0}\
.about{color:#8a93a6;white-space:pre-wrap}.box{border:1px solid #262b36;border-radius:6px;padding:10px 12px;margin:14px 0;background:#171a21;font-size:13px}\
.box b{color:#39c26c}.mirror b{color:#e2b13c}ol{list-style:none;padding:0}li{border:1px solid #262b36;border-radius:6px;padding:8px 12px;margin:8px 0;background:#171a21}\
.meta,.quote,.note,footer{color:#8a93a6;font-size:12px}.quote{border-left:2px solid #5b9cf5;padding-left:6px;margin:4px 0}\
.body{white-space:pre-wrap;overflow-wrap:anywhere}.deleted .body{font-style:italic;color:#8a93a6}footer{border-top:1px solid #262b36;margin-top:18px;padding-top:10px;overflow-wrap:anywhere}";

/// The whole page for `view`, served as `served`.
pub fn html(view: &View, served: Served) -> Vec<u8> {
    let m = &view.manifest;
    let mut s = String::with_capacity(4096 + view.posts.len().min(MAX_POSTS) * 512);
    s.push_str("<!doctype html><html lang=\"en\"><head><meta charset=\"utf-8\"><meta name=\"viewport\" content=\"width=device-width, initial-scale=1\"><meta name=\"referrer\" content=\"no-referrer\"><title>");
    esc(&mut s, &m.title);
    s.push_str("</title><style>");
    s.push_str(STYLE);
    s.push_str("</style></head><body><main><h1>");
    esc(&mut s, &m.title);
    s.push_str("</h1><p class=\"about\">");
    esc(&mut s, &m.about);
    s.push_str("</p>");
    let version = format!("version {}, updated {}", view.record.sequence, rfc3339(view.updated));
    match served {
        Served::Owner => {
            s.push_str("<div class=\"box\"><b>Served by the channel's own onion address.</b> Tor authenticates onion addresses: only the holder of this address's key can serve it, and that key belongs to the channel. So this is the channel as its owner last published it (");
            esc(&mut s, &version);
            s.push_str("). It is online while the owner's Ephem tab is open.</div>");
        }
        Served::Mirror => {
            s.push_str("<div class=\"box mirror\"><b>Served by a mirror</b>, a follower's Ephem that keeps a copy of this channel. It checked the owner's signature on every post when it copied them (");
            esc(&mut s, &version);
            s.push_str("). This address vouches for the mirror, not for the owner: for the owner's own address, and to check the signatures yourself, open the channel's Ephem link.</div>");
        }
    }
    let by_seq = |seq: u64| view.posts.iter().find(|p| p.seq == seq);
    let shown = view.posts.len().min(MAX_POSTS);
    s.push_str("<ol>");
    for p in view.posts.iter().rev().take(shown) {
        post(&mut s, p, if p.reply == 0 { None } else { by_seq(p.reply) });
    }
    s.push_str("</ol>");
    if view.posts.is_empty() && view.missing == 0 {
        s.push_str("<p class=\"note\">No posts yet.</p>");
    } else if view.posts.len() > shown || view.missing > 0 {
        let total = view.posts.len() as u64 + view.missing;
        s.push_str(&format!("<p class=\"note\">The {shown} newest of {total} posts. The Ephem app shows all that this host has; older ones appear when a host that holds them is online.</p>"));
    }
    s.push_str("<footer>Channel name (IPNS): ");
    esc(&mut s, &view.name.to_text());
    s.push_str("<br>A read-only page: no scripts, no cookies, nothing you send. To follow the channel, mirror it or verify its signatures, open the Ephem link its owner shared (it names this address).</footer></main></body></html>");
    s.into_bytes()
}

fn post(s: &mut String, p: &Post, quoted: Option<&Post>) {
    s.push_str(if p.deleted { "<li class=\"deleted\">" } else { "<li>" });
    s.push_str(&format!("<div class=\"meta\">#{} · {}</div>", p.seq, rfc3339(p.ts)));
    if let Some(q) = quoted {
        s.push_str(&format!("<div class=\"quote\">↪ #{}: ", q.seq));
        if q.deleted {
            s.push_str("(deleted)");
        } else {
            esc(s, head(&q.body, 120));
        }
        s.push_str("</div>");
    }
    s.push_str("<div class=\"body\">");
    if p.deleted {
        s.push_str("(deleted by the owner)");
    } else {
        esc(s, &p.body);
    }
    s.push_str("</div></li>");
}
