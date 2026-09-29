//! The channel model (§D.5.1): manifest, posts, pages and the root, as dag-cbor blocks.
//!
//! ```text
//! IPNS record ──▶ root {manifest, head, count, updated}
//!                  ├─▶ manifest {v, title, about, pk, created, mirrors, sig}
//!                  └─▶ head page {posts: [post…] (≤ 64, oldest first), prev} ─▶ older page ─▶ …
//! post = {seq, ts, body, reply, deleted, sig}
//! ```
//!
//! The manifest and every post are signed with the channel key (Ed25519, domain-separated),
//! so a mirror or a gateway can serve them but not change them; the IPNS record signs the root,
//! which pins the rest by CID. A deleted post is re-signed with `deleted = true` and no body.

use crate::car::Block;
use crate::cbor::{self, Value};
use crate::cid::{Cid, DAG_CBOR};
use crate::ipns::{self, Record, RecordError};
use ed25519_dalek::{Signature, Signer, SigningKey, Verifier, VerifyingKey};
use std::collections::HashMap;

pub const MAX_BODY: usize = 4096;
pub const MAX_TITLE: usize = 128;
pub const MAX_ABOUT: usize = 1024;
pub const MAX_MIRRORS: usize = 8;
pub const PAGE_POSTS: usize = 64;
/// IPNS record lifetime and TTL (§D.5.2).
pub const RECORD_VALIDITY_S: u64 = 30 * 24 * 3600;
pub const RECORD_TTL_NS: u64 = 60_000_000_000;
const MANIFEST_SIG: &[u8] = b"ephem-channel-manifest:";
const POST_SIG: &[u8] = b"ephem-channel-post:";

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Manifest {
    pub title: String,
    pub about: String,
    pub pk: [u8; 32],
    pub created: u64,
    /// Mirror onion addresses the owner vouches for (`<56 chars>.onion`), §D.7.1.
    pub mirrors: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Post {
    pub seq: u64,
    /// Unix seconds.
    pub ts: u64,
    pub body: String,
    /// The `seq` this post replies to (0 = none).
    pub reply: u64,
    pub deleted: bool,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum ChannelError {
    TooLong,
    NotFound,
    Record(RecordError),
    /// A block is missing, malformed, or not what the root and record say.
    Invalid,
    /// A signature does not verify with the channel key.
    BadSignature,
}

fn signed(prefix: &[u8], unsigned: &Value) -> Vec<u8> {
    let mut m = prefix.to_vec();
    m.extend_from_slice(&unsigned.encode());
    m
}

fn with_sig(v: Value, sig: [u8; 64]) -> Value {
    let Value::Map(mut m) = v else { unreachable!("blocks are maps") };
    m.push(("sig".into(), Value::Bytes(sig.to_vec())));
    let entries = m.iter().map(|(k, v)| (k.as_str(), v.clone())).collect();
    cbor::map(entries)
}

/// The map without its `sig` entry, and the signature.
fn split_sig(v: &Value) -> Option<(Value, Signature)> {
    let Value::Map(m) = v else { return None };
    let sig = Signature::from_slice(v.get("sig")?.bytes()?).ok()?;
    Some((Value::Map(m.iter().filter(|(k, _)| k != "sig").cloned().collect()), sig))
}

impl Manifest {
    fn unsigned(&self) -> Value {
        cbor::map(vec![
            ("v", Value::Uint(1)),
            ("title", Value::Text(self.title.clone())),
            ("about", Value::Text(self.about.clone())),
            ("pk", Value::Bytes(self.pk.to_vec())),
            ("created", Value::Uint(self.created)),
            ("mirrors", Value::Array(self.mirrors.iter().cloned().map(Value::Text).collect())),
        ])
    }

    fn from_value(v: &Value, key: &VerifyingKey) -> Result<Self, ChannelError> {
        let (unsigned, sig) = split_sig(v).ok_or(ChannelError::Invalid)?;
        key.verify(&signed(MANIFEST_SIG, &unsigned), &sig).map_err(|_| ChannelError::BadSignature)?;
        let text = |k: &str| v.get(k).and_then(Value::text).map(str::to_owned).ok_or(ChannelError::Invalid);
        let m = Manifest {
            title: text("title")?,
            about: text("about")?,
            pk: v.get("pk").and_then(Value::bytes).and_then(|b| b.try_into().ok()).ok_or(ChannelError::Invalid)?,
            created: v.get("created").and_then(Value::uint).ok_or(ChannelError::Invalid)?,
            mirrors: v.get("mirrors").and_then(Value::array).ok_or(ChannelError::Invalid)?.iter().map(|x| x.text().map(str::to_owned)).collect::<Option<_>>().ok_or(ChannelError::Invalid)?,
        };
        if v.get("v").and_then(Value::uint) != Some(1) || m.pk != key.to_bytes() || m.mirrors.len() > MAX_MIRRORS {
            return Err(ChannelError::Invalid);
        }
        Ok(m)
    }
}

impl Post {
    fn unsigned(&self) -> Value {
        cbor::map(vec![
            ("seq", Value::Uint(self.seq)),
            ("ts", Value::Uint(self.ts)),
            ("body", Value::Text(self.body.clone())),
            ("reply", Value::Uint(self.reply)),
            ("deleted", Value::Bool(self.deleted)),
        ])
    }

    fn from_value(v: &Value, key: &VerifyingKey) -> Result<Self, ChannelError> {
        let (unsigned, sig) = split_sig(v).ok_or(ChannelError::Invalid)?;
        key.verify(&signed(POST_SIG, &unsigned), &sig).map_err(|_| ChannelError::BadSignature)?;
        let p = Post {
            seq: v.get("seq").and_then(Value::uint).ok_or(ChannelError::Invalid)?,
            ts: v.get("ts").and_then(Value::uint).ok_or(ChannelError::Invalid)?,
            body: v.get("body").and_then(Value::text).ok_or(ChannelError::Invalid)?.to_owned(),
            reply: v.get("reply").and_then(Value::uint).ok_or(ChannelError::Invalid)?,
            deleted: v.get("deleted").and_then(Value::boolean).ok_or(ChannelError::Invalid)?,
        };
        if p.body.len() > MAX_BODY || (p.deleted && !p.body.is_empty()) {
            return Err(ChannelError::Invalid);
        }
        Ok(p)
    }
}

/// The owner's channel: its key, manifest and posts. Every change is followed by
/// [`Channel::build`] (blocks) and [`Channel::record`] (the signed IPNS record).
pub struct Channel {
    key: SigningKey,
    pub manifest: Manifest,
    pub posts: Vec<Post>,
    /// The IPNS sequence of the last record (grows on every change).
    pub revision: u64,
}

/// A verified channel as a reader sees it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct View {
    pub name: Cid,
    pub root: Cid,
    pub record: Record,
    pub manifest: Manifest,
    pub posts: Vec<Post>,
    pub updated: u64,
}

impl Channel {
    /// A new channel from its signing seed (§D.3).
    pub fn new(sign_seed: &[u8; 32], title: &str, about: &str, created: u64) -> Result<Self, ChannelError> {
        if title.len() > MAX_TITLE || about.len() > MAX_ABOUT {
            return Err(ChannelError::TooLong);
        }
        let key = SigningKey::from_bytes(sign_seed);
        let pk = key.verifying_key().to_bytes();
        Ok(Self { key, manifest: Manifest { title: title.into(), about: about.into(), pk, created, mirrors: Vec::new() }, posts: Vec::new(), revision: 0 })
    }

    /// The channel's IPNS name.
    pub fn name(&self) -> Cid {
        Cid::ipns_name(&self.manifest.pk)
    }

    /// Adds a post; returns its `seq` (1, 2, …).
    pub fn post(&mut self, body: &str, reply: u64, now_s: u64) -> Result<u64, ChannelError> {
        if body.len() > MAX_BODY || body.is_empty() {
            return Err(ChannelError::TooLong);
        }
        if reply != 0 && !self.posts.iter().any(|p| p.seq == reply) {
            return Err(ChannelError::NotFound);
        }
        let seq = self.posts.last().map_or(1, |p| p.seq + 1);
        self.posts.push(Post { seq, ts: now_s, body: body.into(), reply, deleted: false });
        Ok(seq)
    }

    /// Deletes a post: its body goes, a signed tombstone stays (older copies may survive on
    /// mirrors, §D.5.1).
    pub fn delete(&mut self, seq: u64) -> Result<(), ChannelError> {
        let p = self.posts.iter_mut().find(|p| p.seq == seq && !p.deleted).ok_or(ChannelError::NotFound)?;
        p.body.clear();
        p.deleted = true;
        Ok(())
    }

    /// Sets the signed mirror list (§D.7.1).
    pub fn set_mirrors(&mut self, mirrors: Vec<String>) -> Result<(), ChannelError> {
        if mirrors.len() > MAX_MIRRORS || mirrors.iter().any(|m| m.len() != 62 || !m.ends_with(".onion")) {
            return Err(ChannelError::Invalid);
        }
        self.manifest.mirrors = mirrors;
        Ok(())
    }

    fn sign(&self, prefix: &[u8], unsigned: Value) -> Value {
        let sig = self.key.sign(&signed(prefix, &unsigned)).to_bytes();
        with_sig(unsigned, sig)
    }

    /// All blocks of the current state and the root CID (the root block comes last).
    pub fn build(&self, now_s: u64) -> (Cid, Vec<Block>) {
        let mut blocks = Vec::with_capacity(3 + self.posts.len() / PAGE_POSTS);
        let manifest = self.sign(MANIFEST_SIG, self.manifest.unsigned()).encode();
        let mcid = Cid::of(DAG_CBOR, &manifest);
        blocks.push((mcid.clone(), manifest));
        let mut prev = Value::Null;
        for chunk in self.posts.chunks(PAGE_POSTS) {
            let posts = chunk.iter().map(|p| self.sign(POST_SIG, p.unsigned())).collect();
            let page = cbor::map(vec![("posts", Value::Array(posts)), ("prev", prev)]).encode();
            let cid = Cid::of(DAG_CBOR, &page);
            blocks.push((cid.clone(), page));
            prev = Value::Link(cid);
        }
        let root = cbor::map(vec![
            ("manifest", Value::Link(mcid)),
            ("head", prev),
            ("count", Value::Uint(self.posts.len() as u64)),
            ("updated", Value::Uint(now_s)),
        ])
        .encode();
        let rcid = Cid::of(DAG_CBOR, &root);
        blocks.push((rcid.clone(), root));
        (rcid, blocks)
    }

    /// The next signed IPNS record for `root` (sequence + 1, valid 30 days, TTL 60 s).
    pub fn record(&mut self, root: &Cid, now_s: u64) -> Vec<u8> {
        self.revision += 1;
        let r = Record { value: format!("/ipfs/{}", root.to_text()), sequence: self.revision, validity: now_s + RECORD_VALIDITY_S, ttl_ns: RECORD_TTL_NS };
        ipns::create(&self.key, &r)
    }

    /// Reopens the owner's channel from its stored blocks (OPFS or a CAR backup) and the
    /// sequence of its last record.
    pub fn load(sign_seed: &[u8; 32], root: &Cid, blocks: &[Block], revision: u64) -> Result<Self, ChannelError> {
        let key = SigningKey::from_bytes(sign_seed);
        let (manifest, posts, _) = read_dag(&key.verifying_key(), root, blocks)?;
        Ok(Self { key, manifest, posts, revision })
    }
}

/// Reads and verifies the DAG under `root`: manifest, posts (oldest first), updated.
fn read_dag(key: &VerifyingKey, root: &Cid, blocks: &[Block]) -> Result<(Manifest, Vec<Post>, u64), ChannelError> {
    let by_cid: HashMap<&Cid, &Vec<u8>> = blocks.iter().filter(|(c, b)| c.verifies(b)).map(|(c, b)| (c, b)).collect();
    let block = |c: &Cid| by_cid.get(c).and_then(|b| Value::decode(b)).ok_or(ChannelError::Invalid);
    let r = block(root)?;
    let manifest = Manifest::from_value(&block(r.get("manifest").and_then(Value::link).ok_or(ChannelError::Invalid)?)?, key)?;
    let count = r.get("count").and_then(Value::uint).ok_or(ChannelError::Invalid)?;
    let updated = r.get("updated").and_then(Value::uint).ok_or(ChannelError::Invalid)?;
    let mut pages = Vec::new();
    let mut next = r.get("head").ok_or(ChannelError::Invalid)?.clone();
    while let Value::Link(c) = next {
        if pages.len() as u64 > count / PAGE_POSTS as u64 + 1 {
            return Err(ChannelError::Invalid); // a loop or more pages than posts
        }
        let page = block(&c)?;
        next = page.get("prev").ok_or(ChannelError::Invalid)?.clone();
        pages.push(page);
    }
    if next != Value::Null {
        return Err(ChannelError::Invalid);
    }
    let mut posts = Vec::with_capacity(count as usize);
    for page in pages.iter().rev() {
        let list = page.get("posts").and_then(Value::array).ok_or(ChannelError::Invalid)?;
        if list.is_empty() || list.len() > PAGE_POSTS {
            return Err(ChannelError::Invalid);
        }
        for p in list {
            let p = Post::from_value(p, key)?;
            if posts.last().is_some_and(|q: &Post| q.seq >= p.seq) {
                return Err(ChannelError::Invalid);
            }
            posts.push(p);
        }
    }
    if posts.len() as u64 != count {
        return Err(ChannelError::Invalid);
    }
    Ok((manifest, posts, updated))
}

/// A reader: verifies a channel's record and blocks (e.g. from a CAR) at `now_s`.
/// `min_sequence` is the reader's high-water mark: an older record is refused (§D.8).
pub fn verify(name: &Cid, record: &[u8], blocks: &[Block], now_s: u64, min_sequence: u64) -> Result<View, ChannelError> {
    let rec = ipns::verify(name, record, now_s).map_err(ChannelError::Record)?;
    if rec.sequence < min_sequence {
        return Err(ChannelError::Invalid);
    }
    let root = rec.value.strip_prefix("/ipfs/").and_then(Cid::parse).ok_or(ChannelError::Invalid)?;
    let pk = name.ed25519_key().ok_or(ChannelError::Invalid)?;
    let key = VerifyingKey::from_bytes(&pk).map_err(|_| ChannelError::Invalid)?;
    let (manifest, posts, updated) = read_dag(&key, &root, blocks)?;
    Ok(View { name: name.clone(), root, record: rec, manifest, posts, updated })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::car;

    const NOW: u64 = 1_790_000_000;

    fn sample(n: u64) -> Channel {
        let mut c = Channel::new(&[4; 32], "Ephem news", "A test channel", NOW).unwrap();
        for i in 1..=n {
            c.post(&format!("post number {i}"), 0, NOW + i).unwrap();
        }
        c
    }

    #[test]
    fn owner_to_reader_through_a_car() {
        let mut c = sample(150); // three pages
        c.post("a reply", 7, NOW + 200).unwrap();
        c.delete(3).unwrap();
        c.set_mirrors(vec![format!("{}.onion", "a".repeat(56))]).unwrap();
        let (root, blocks) = c.build(NOW + 300);
        let rec = c.record(&root, NOW + 300);
        let file = car::write(std::slice::from_ref(&root), &blocks);
        let (roots, read) = car::read(&file).unwrap();
        assert_eq!(roots, std::slice::from_ref(&root));
        let v = verify(&c.name(), &rec, &read, NOW + 400, 0).unwrap();
        assert_eq!(v.manifest.title, "Ephem news");
        assert_eq!(v.manifest.mirrors.len(), 1);
        assert_eq!(v.posts.len(), 151);
        assert!(v.posts[2].deleted && v.posts[2].body.is_empty());
        assert_eq!(v.posts[150].reply, 7);
        assert_eq!(v.record.sequence, 1);
        assert_eq!(verify(&c.name(), &rec, &read, NOW + 400, 2), Err(ChannelError::Invalid), "older than the high-water mark");
        // The owner reopens it from the stored blocks.
        let again = Channel::load(&[4; 32], &root, &read, 1).unwrap();
        assert_eq!(again.posts, c.posts);
        assert_eq!(again.manifest, c.manifest);
    }

    #[test]
    fn forgeries_fail() {
        let mut c = sample(3);
        let (root, blocks) = c.build(NOW);
        let rec = c.record(&root, NOW);
        // Someone else's key signs a channel claiming the same name: the record fails.
        let mut fake = Channel::new(&[5; 32], "Ephem news", "", NOW).unwrap();
        let (froot, fblocks) = fake.build(NOW);
        let frec = fake.record(&froot, NOW);
        assert!(matches!(verify(&c.name(), &frec, &fblocks, NOW, 0), Err(ChannelError::Record(_))));
        // A block swapped for another with a valid CID but wrong content: the chain breaks.
        let mut swapped = blocks.clone();
        swapped.retain(|(cid, _)| *cid != root);
        assert_eq!(verify(&c.name(), &rec, &swapped, NOW, 0), Err(ChannelError::Invalid), "root missing");
        // A post re-signed by another key inside a page with a matching CID.
        let other = SigningKey::from_bytes(&[6; 32]);
        let forged = with_sig(Post { seq: 1, ts: NOW, body: "forged".into(), reply: 0, deleted: false }.unsigned(), other.sign(b"x").to_bytes());
        let page = cbor::map(vec![("posts", Value::Array(vec![forged])), ("prev", Value::Null)]).encode();
        let pcid = Cid::of(DAG_CBOR, &page);
        let mcid = blocks[0].0.clone();
        let r = cbor::map(vec![("manifest", Value::Link(mcid)), ("head", Value::Link(pcid.clone())), ("count", Value::Uint(1)), ("updated", Value::Uint(NOW))]).encode();
        let rcid = Cid::of(DAG_CBOR, &r);
        let rec2 = c.record(&rcid, NOW);
        let bad = vec![blocks[0].clone(), (pcid, page), (rcid, r)];
        assert_eq!(verify(&c.name(), &rec2, &bad, NOW, 0), Err(ChannelError::BadSignature));
    }

    #[test]
    fn limits() {
        let mut c = sample(0);
        assert_eq!(c.post(&"x".repeat(MAX_BODY + 1), 0, NOW), Err(ChannelError::TooLong));
        assert_eq!(c.post("hi", 5, NOW), Err(ChannelError::NotFound), "reply to nothing");
        assert_eq!(c.delete(1), Err(ChannelError::NotFound));
        assert!(Channel::new(&[1; 32], &"t".repeat(MAX_TITLE + 1), "", NOW).is_err());
        assert!(c.set_mirrors(vec!["example.com".into()]).is_err());
    }
}
