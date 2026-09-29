// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
// Copyright 2026 Anton (darkcite)
//! DAG-CBOR, the subset channels use: unsigned integers, byte and text strings, arrays, maps
//! with text keys, booleans, null and CID links (tag 42). Encoding is canonical (shortest
//! integer forms, map keys sorted by length then bytes); decoding is strict: anything that
//! would not re-encode to the same bytes is rejected, so a block's hash and its signed content
//! always agree.

use crate::cid::Cid;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Value {
    Uint(u64),
    Bytes(Vec<u8>),
    Text(String),
    Array(Vec<Value>),
    /// Keys are kept in canonical order (see [`map`]).
    Map(Vec<(String, Value)>),
    Bool(bool),
    Null,
    Link(Cid),
}

/// A map value with its keys in canonical order.
pub fn map(mut entries: Vec<(&str, Value)>) -> Value {
    entries.sort_by(|a, b| a.0.len().cmp(&b.0.len()).then_with(|| a.0.cmp(b.0)));
    Value::Map(entries.into_iter().map(|(k, v)| (k.to_owned(), v)).collect())
}

impl Value {
    pub fn get(&self, key: &str) -> Option<&Value> {
        match self {
            Value::Map(m) => m.iter().find(|(k, _)| k == key).map(|(_, v)| v),
            _ => None,
        }
    }

    pub fn uint(&self) -> Option<u64> {
        match self {
            Value::Uint(v) => Some(*v),
            _ => None,
        }
    }

    pub fn bytes(&self) -> Option<&[u8]> {
        match self {
            Value::Bytes(v) => Some(v),
            _ => None,
        }
    }

    pub fn text(&self) -> Option<&str> {
        match self {
            Value::Text(v) => Some(v),
            _ => None,
        }
    }

    pub fn link(&self) -> Option<&Cid> {
        match self {
            Value::Link(c) => Some(c),
            _ => None,
        }
    }

    pub fn array(&self) -> Option<&[Value]> {
        match self {
            Value::Array(v) => Some(v),
            _ => None,
        }
    }

    pub fn boolean(&self) -> Option<bool> {
        match self {
            Value::Bool(v) => Some(*v),
            _ => None,
        }
    }

    pub fn encode(&self) -> Vec<u8> {
        let mut out = Vec::new();
        self.write(&mut out);
        out
    }

    fn write(&self, out: &mut Vec<u8>) {
        match self {
            Value::Uint(v) => head(out, 0, *v),
            Value::Bytes(b) => {
                head(out, 2, b.len() as u64);
                out.extend_from_slice(b);
            }
            Value::Text(t) => {
                head(out, 3, t.len() as u64);
                out.extend_from_slice(t.as_bytes());
            }
            Value::Array(a) => {
                head(out, 4, a.len() as u64);
                for v in a {
                    v.write(out);
                }
            }
            Value::Map(m) => {
                debug_assert!(m.windows(2).all(|w| key_order(&w[0].0, &w[1].0)), "map keys out of order");
                head(out, 5, m.len() as u64);
                for (k, v) in m {
                    head(out, 3, k.len() as u64);
                    out.extend_from_slice(k.as_bytes());
                    v.write(out);
                }
            }
            Value::Bool(b) => out.push(if *b { 0xf5 } else { 0xf4 }),
            Value::Null => out.push(0xf6),
            Value::Link(c) => {
                head(out, 6, 42);
                let b = c.to_bytes();
                head(out, 2, b.len() as u64 + 1);
                out.push(0); // multibase identity prefix
                out.extend_from_slice(&b);
            }
        }
    }

    /// Decodes exactly one value filling `src`.
    pub fn decode(src: &[u8]) -> Option<Value> {
        let mut r = Reader { src, pos: 0, depth: 0 };
        let v = r.value()?;
        (r.pos == src.len()).then_some(v)
    }
}

fn key_order(a: &str, b: &str) -> bool {
    (a.len(), a.as_bytes()) < (b.len(), b.as_bytes())
}

fn head(out: &mut Vec<u8>, major: u8, v: u64) {
    let m = major << 5;
    match v {
        0..=23 => out.push(m | v as u8),
        24..=0xff => out.extend_from_slice(&[m | 24, v as u8]),
        0x100..=0xffff => {
            out.push(m | 25);
            out.extend_from_slice(&(v as u16).to_be_bytes());
        }
        0x1_0000..=0xffff_ffff => {
            out.push(m | 26);
            out.extend_from_slice(&(v as u32).to_be_bytes());
        }
        _ => {
            out.push(m | 27);
            out.extend_from_slice(&v.to_be_bytes());
        }
    }
}

struct Reader<'a> {
    src: &'a [u8],
    pos: usize,
    depth: u8,
}

/// Nesting limit: channel blocks are at most three levels deep.
const MAX_DEPTH: u8 = 16;

impl Reader<'_> {
    fn take(&mut self, n: usize) -> Option<&[u8]> {
        let s = self.src.get(self.pos..self.pos.checked_add(n)?)?;
        self.pos += n;
        Some(s)
    }

    /// A head: (major, argument), rejecting non-shortest forms and indefinite lengths.
    fn head(&mut self) -> Option<(u8, u64)> {
        let b = self.take(1)?[0];
        let (major, info) = (b >> 5, b & 31);
        let v = match info {
            0..=23 => u64::from(info),
            24 => {
                let v = u64::from(self.take(1)?[0]);
                if v < 24 {
                    return None;
                }
                v
            }
            25 => {
                let v = u64::from(u16::from_be_bytes(self.take(2)?.try_into().ok()?));
                if v <= 0xff {
                    return None;
                }
                v
            }
            26 => {
                let v = u64::from(u32::from_be_bytes(self.take(4)?.try_into().ok()?));
                if v <= 0xffff {
                    return None;
                }
                v
            }
            27 => {
                let v = u64::from_be_bytes(self.take(8)?.try_into().ok()?);
                if v <= 0xffff_ffff {
                    return None;
                }
                v
            }
            _ => return None,
        };
        Some((major, v))
    }

    fn len(&mut self, v: u64) -> Option<usize> {
        let n = usize::try_from(v).ok()?;
        // No declared length beyond what is left (every item takes at least one byte).
        (n <= self.src.len() - self.pos).then_some(n)
    }

    fn value(&mut self) -> Option<Value> {
        self.depth += 1;
        if self.depth > MAX_DEPTH {
            return None;
        }
        let (major, arg) = self.head()?;
        let v = match major {
            0 => Value::Uint(arg),
            2 => {
                let n = self.len(arg)?;
                Value::Bytes(self.take(n)?.to_vec())
            }
            3 => {
                let n = self.len(arg)?;
                Value::Text(String::from_utf8(self.take(n)?.to_vec()).ok()?)
            }
            4 => {
                let n = self.len(arg)?;
                let mut a = Vec::with_capacity(n);
                for _ in 0..n {
                    a.push(self.value()?);
                }
                Value::Array(a)
            }
            5 => {
                let n = self.len(arg)?;
                let mut m: Vec<(String, Value)> = Vec::with_capacity(n);
                for _ in 0..n {
                    let Value::Text(k) = self.value()? else { return None };
                    if m.last().is_some_and(|(prev, _)| !key_order(prev, &k)) {
                        return None; // unsorted or duplicate key
                    }
                    let v = self.value()?;
                    m.push((k, v));
                }
                Value::Map(m)
            }
            6 if arg == 42 => {
                let Value::Bytes(b) = self.value()? else { return None };
                if b.first() != Some(&0) {
                    return None;
                }
                Value::Link(Cid::from_bytes(&b[1..])?)
            }
            7 => match arg {
                20 => Value::Bool(false),
                21 => Value::Bool(true),
                22 => Value::Null,
                _ => return None,
            },
            _ => return None,
        };
        self.depth -= 1;
        Some(v)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cid::{Cid, DAG_CBOR};

    #[test]
    fn canonical_round_trip() {
        let link = Cid::of(DAG_CBOR, &[0xa0]);
        let v = map(vec![
            ("zz", Value::Uint(1_000_000)),
            ("a", Value::Text("héllo".into())),
            ("bb", Value::Array(vec![Value::Bool(true), Value::Null, Value::Link(link.clone())])),
            ("c", Value::Bytes(vec![1, 2, 3])),
        ]);
        let b = v.encode();
        assert_eq!(Value::decode(&b), Some(v.clone()));
        // Keys by length, then bytes: a, c, bb, zz.
        let Value::Map(m) = &v else { unreachable!() };
        assert_eq!(m.iter().map(|(k, _)| k.as_str()).collect::<Vec<_>>(), ["a", "c", "bb", "zz"]);
        assert_eq!(v.get("bb").and_then(Value::array).and_then(|a| a[2].link()), Some(&link));
        assert_eq!(Value::Map(vec![]).encode(), [0xa0]);
    }

    #[test]
    fn strict() {
        assert_eq!(Value::decode(&[0x18, 0x05]), None, "non-shortest integer");
        assert_eq!(Value::decode(&[0x5f]), None, "indefinite length");
        assert_eq!(Value::decode(&[0xa2, 0x61, 0x62, 0x01, 0x61, 0x61, 0x02]), None, "unsorted keys");
        assert_eq!(Value::decode(&[0xa2, 0x61, 0x61, 0x01, 0x61, 0x61, 0x02]), None, "duplicate key");
        assert_eq!(Value::decode(&[0x01, 0x02]), None, "trailing bytes");
        assert_eq!(Value::decode(&[0x9b, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff]), None, "huge array");
        assert_eq!(Value::decode(&[0xf9, 0, 0]), None, "floats are not used");
        let mut deep = vec![0x81; 40];
        deep.push(0x00);
        assert_eq!(Value::decode(&deep), None, "too deep");
    }
}
