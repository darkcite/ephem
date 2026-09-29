// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
// Copyright 2026 Anton (darkcite)
//! The page's view of a channel as JSON (no serde: a handful of fields, escaped by hand).
//!
//! `{"name","root","sequence","validity","title","about","created","updated","mirrors":[…],
//! "posts":[{"seq","ts","body","reply","deleted"}…]}`

use ephem_channel::View;
use std::fmt::Write;

fn string(out: &mut String, s: &str) {
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 || c == '\u{2028}' || c == '\u{2029}' => {
                let _ = write!(out, "\\u{:04x}", c as u32);
            }
            c => out.push(c),
        }
    }
    out.push('"');
}

pub fn view(v: &View) -> String {
    let mut o = String::with_capacity(256 + v.posts.iter().map(|p| p.body.len() + 64).sum::<usize>());
    o.push_str("{\"name\":");
    string(&mut o, &v.name.to_text());
    o.push_str(",\"root\":");
    string(&mut o, &v.root.to_text());
    let _ = write!(o, ",\"sequence\":{},\"validity\":{},\"created\":{},\"updated\":{}", v.record.sequence, v.record.validity, v.manifest.created, v.updated);
    o.push_str(",\"title\":");
    string(&mut o, &v.manifest.title);
    o.push_str(",\"about\":");
    string(&mut o, &v.manifest.about);
    o.push_str(",\"mirrors\":[");
    for (i, m) in v.manifest.mirrors.iter().enumerate() {
        if i > 0 {
            o.push(',');
        }
        string(&mut o, m);
    }
    o.push_str("],\"posts\":[");
    for (i, p) in v.posts.iter().enumerate() {
        if i > 0 {
            o.push(',');
        }
        let _ = write!(o, "{{\"seq\":{},\"ts\":{},\"reply\":{},\"deleted\":{},\"body\":", p.seq, p.ts, p.reply, p.deleted);
        string(&mut o, &p.body);
        o.push('}');
    }
    o.push_str("]}");
    o
}
