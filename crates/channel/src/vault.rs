// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
// Copyright 2026 Anton (darkcite)
//! The vault (docs/P2P-CHAT.md §D.11): what one identity's devices share about its channels,
//! so a device that has only the identity finds them and can keep posting.
//!
//! ```text
//! IPNS record (V2-only, the vault name) ─▶ value "/ipfs/<CIDv1 raw, identity multihash>"
//!   the CID inlines: nonce(24) ‖ XChaCha20-Poly1305(vault key, AAD, padded plaintext) ‖ tag(16)
//!   plaintext = u16 length ‖ dag-cbor {v, dev, until, ch: [entry…]} ‖ zero padding
//! ```
//!
//! The ciphertext rides inside the record, so reading it needs no block from anyone; the
//! record lives on the public DHT (published through a Tor exit). Keys come from the identity
//! seed (`Identity::vault_seeds`); the name links to nothing else. Setup path: owned buffers.

use crate::cbor::{self, Value};
use crate::cid::{Cid, RAW, base32, unbase32};
use crate::ipns::{self, Record, RecordError};
use crate::varint;
use chacha20poly1305::aead::{AeadInPlace, KeyInit};
use chacha20poly1305::{Tag, XChaCha20Poly1305, XNonce};
use ed25519_dalek::SigningKey;

/// Largest padded plaintext: the record stays under the 10 KiB IPNS limit (base32 text of the
/// inline CID is 1.6× the ciphertext; asserted in the tests).
pub const MAX_PLAIN: usize = 5632;
/// Padding buckets: the size of the record says little about what changed.
const BUCKETS: [usize; 5] = [512, 1024, 2048, 4096, MAX_PLAIN];
const NONCE: usize = 24;
const TAG: usize = 16;
const AAD: &[u8] = b"ephem-vault-v1";
const IDENTITY_MH: u64 = 0x00;
/// Vault record lifetime (as channel records, §D.5.2); devices republish every 12 h (§D.11.3).
pub const VALIDITY_S: u64 = 30 * 24 * 3600;
pub const TTL_NS: u64 = 60_000_000_000;
/// Owned channels a vault lists (the page looks at indices 0‥16).
pub const MAX_ENTRIES: usize = 16;

/// One owned channel as the vault remembers it: enough to list it, re-sign its manifest and
/// continue its chain without holding its blocks (§D.11.3 step 4).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Entry {
    pub index: u32,
    pub title: String,
    /// Dropped (empty) only when the vault would not fit otherwise.
    pub about: String,
    pub created: u64,
    pub mirrors: Vec<String>,
    /// The newest page of posts (`None`: no post yet).
    pub head: Option<Cid>,
    /// Posts in the whole chain, and the last `seq`.
    pub count: u64,
    pub last_seq: u64,
    /// The sequence of the channel's own latest IPNS record.
    pub record_seq: u64,
}

/// Which device writes (hosts and posts) the channels, until when (Unix seconds).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Lease {
    pub device: [u8; 16],
    pub until: u64,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Vault {
    pub lease: Lease,
    pub entries: Vec<Entry>,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum VaultError {
    /// Even without `about` texts and mirror lists the vault is larger than [`MAX_PLAIN`].
    TooLarge,
    Record(RecordError),
    /// Not an inline CID, a bad tag (wrong key or tampered), or malformed plaintext.
    Invalid,
}

impl Entry {
    fn value(&self) -> Value {
        cbor::map(vec![
            ("i", Value::Uint(u64::from(self.index))),
            ("t", Value::Text(self.title.clone())),
            ("a", Value::Text(self.about.clone())),
            ("c", Value::Uint(self.created)),
            ("m", Value::Array(self.mirrors.iter().cloned().map(Value::Text).collect())),
            ("h", self.head.clone().map_or(Value::Null, Value::Link)),
            ("n", Value::Uint(self.count)),
            ("s", Value::Uint(self.last_seq)),
            ("r", Value::Uint(self.record_seq)),
        ])
    }

    fn from_value(v: &Value) -> Option<Self> {
        let text = |k: &str| v.get(k).and_then(Value::text).map(str::to_owned);
        let uint = |k: &str| v.get(k).and_then(Value::uint);
        let head = match v.get("h")? {
            Value::Null => None,
            Value::Link(c) => Some(c.clone()),
            _ => return None,
        };
        let e = Entry {
            index: u32::try_from(uint("i")?).ok()?,
            title: text("t")?,
            about: text("a")?,
            created: uint("c")?,
            mirrors: v.get("m")?.array()?.iter().map(|m| m.text().map(str::to_owned)).collect::<Option<_>>()?,
            head,
            count: uint("n")?,
            last_seq: uint("s")?,
            record_seq: uint("r")?,
        };
        (e.last_seq >= e.count && (e.head.is_some() == (e.count > 0))).then_some(e)
    }
}

impl Vault {
    fn encode(&self) -> Vec<u8> {
        cbor::map(vec![
            ("v", Value::Uint(1)),
            ("dev", Value::Bytes(self.lease.device.to_vec())),
            ("until", Value::Uint(self.lease.until)),
            ("ch", Value::Array(self.entries.iter().map(Entry::value).collect())),
        ])
        .encode()
    }

    fn decode(b: &[u8]) -> Option<Self> {
        let v = Value::decode(b)?;
        if v.get("v")?.uint()? != 1 {
            return None;
        }
        let entries: Vec<Entry> = v.get("ch")?.array()?.iter().map(Entry::from_value).collect::<Option<_>>()?;
        if entries.len() > MAX_ENTRIES {
            return None;
        }
        Some(Vault { lease: Lease { device: v.get("dev")?.bytes()?.try_into().ok()?, until: v.get("until")?.uint()? }, entries })
    }

    /// The padded plaintext; drops `about` texts, then mirror lists, if that is what it takes
    /// to fit (the channel's own manifest still has them).
    fn plaintext(&self) -> Result<Vec<u8>, VaultError> {
        let mut v = self.clone();
        let mut enc = v.encode();
        if enc.len() + 2 > MAX_PLAIN {
            v.entries.iter_mut().for_each(|e| e.about.clear());
            enc = v.encode();
        }
        if enc.len() + 2 > MAX_PLAIN {
            v.entries.iter_mut().for_each(|e| e.mirrors.clear());
            enc = v.encode();
        }
        let size = BUCKETS.iter().copied().find(|&b| enc.len() + 2 <= b).ok_or(VaultError::TooLarge)?;
        let mut out = vec![0u8; size];
        out[..2].copy_from_slice(&(enc.len() as u16).to_be_bytes());
        out[2..2 + enc.len()].copy_from_slice(&enc);
        Ok(out)
    }
}

fn aad(sequence: u64) -> [u8; AAD.len() + 8] {
    let mut a = [0u8; AAD.len() + 8];
    a[..AAD.len()].copy_from_slice(AAD);
    a[AAD.len()..].copy_from_slice(&sequence.to_be_bytes());
    a
}

/// `/ipfs/b…`: a CIDv1, raw codec, identity multihash of `data` (the data is the CID).
fn inline_path(data: &[u8]) -> String {
    let mut b = Vec::with_capacity(data.len() + 6);
    varint::put(&mut b, 1);
    varint::put(&mut b, RAW);
    varint::put(&mut b, IDENTITY_MH);
    varint::put(&mut b, data.len() as u64);
    b.extend_from_slice(data);
    format!("/ipfs/b{}", base32(&b))
}

fn inline_data(path: &str) -> Option<Vec<u8>> {
    let b = unbase32(path.strip_prefix("/ipfs/b")?)?;
    let (ver, a) = varint::get(&b)?;
    let (codec, c) = varint::get(&b[a..])?;
    let (mh, d) = varint::get(&b[a + c..])?;
    let (len, e) = varint::get(&b[a + c + d..])?;
    let start = a + c + d + e;
    (ver == 1 && codec == RAW && mh == IDENTITY_MH && b.len() - start == usize::try_from(len).ok()?).then(|| b[start..].to_vec())
}

/// The vault's IPNS name, from its signing seed.
pub fn name(sign_seed: &[u8; 32]) -> Cid {
    Cid::ipns_name(&SigningKey::from_bytes(sign_seed).verifying_key().to_bytes())
}

/// The signed vault record: `vault` sealed with `key` under `nonce` (24 random bytes, fresh for
/// every record), as record `sequence`, valid from `now_s` for [`VALIDITY_S`].
pub fn seal(sign_seed: &[u8; 32], key: &[u8; 32], vault: &Vault, sequence: u64, now_s: u64, nonce: &[u8; NONCE]) -> Result<Vec<u8>, VaultError> {
    let mut plain = vault.plaintext()?;
    let tag = XChaCha20Poly1305::new(key.into()).encrypt_in_place_detached(XNonce::from_slice(nonce), &aad(sequence), &mut plain).map_err(|_| VaultError::Invalid)?;
    let mut sealed = Vec::with_capacity(NONCE + plain.len() + TAG);
    sealed.extend_from_slice(nonce);
    sealed.extend_from_slice(&plain);
    sealed.extend_from_slice(&tag);
    let r = Record { value: inline_path(&sealed), sequence, validity: now_s + VALIDITY_S, ttl_ns: TTL_NS };
    Ok(ipns::create_v2(&SigningKey::from_bytes(sign_seed), &r))
}

/// Verifies a vault record for `name` at `now_s` and opens it with `key`: the vault and the
/// record's sequence.
pub fn open(name: &Cid, key: &[u8; 32], record: &[u8], now_s: u64) -> Result<(Vault, u64), VaultError> {
    let r = ipns::verify(name, record, now_s).map_err(VaultError::Record)?;
    let mut sealed = inline_data(&r.value).ok_or(VaultError::Invalid)?;
    if sealed.len() < NONCE + TAG + 2 {
        return Err(VaultError::Invalid);
    }
    let tag = Tag::clone_from_slice(&sealed[sealed.len() - TAG..]);
    let nonce = XNonce::clone_from_slice(&sealed[..NONCE]);
    let n = sealed.len() - TAG;
    let body = &mut sealed[NONCE..n];
    XChaCha20Poly1305::new(key.into()).decrypt_in_place_detached(&nonce, &aad(r.sequence), body, &tag).map_err(|_| VaultError::Invalid)?;
    let len = usize::from(u16::from_be_bytes([body[0], body[1]]));
    let enc = body.get(2..2 + len).ok_or(VaultError::Invalid)?;
    let v = Vault::decode(enc).ok_or(VaultError::Invalid)?;
    Ok((v, r.sequence))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cid::DAG_CBOR;

    const NOW: u64 = 1_790_000_000;

    fn entry(i: u32, about: usize) -> Entry {
        Entry {
            index: i,
            title: format!("Channel {i}"),
            about: "a".repeat(about),
            created: NOW,
            mirrors: vec![format!("{}.onion", "m".repeat(56)); 8],
            head: Some(Cid::of(DAG_CBOR, &i.to_be_bytes())),
            count: 70,
            last_seq: 71,
            record_seq: 9,
        }
    }

    fn vault(n: u32, about: usize) -> Vault {
        Vault { lease: Lease { device: [7; 16], until: NOW + 900 }, entries: (0..n).map(|i| entry(i, about)).collect() }
    }

    #[test]
    fn seal_and_open() {
        let (sign, key) = ([1; 32], [2; 32]);
        let v = vault(3, 200);
        let rec = seal(&sign, &key, &v, 5, NOW, &[3; 24]).unwrap();
        assert_eq!(open(&name(&sign), &key, &rec, NOW + 60), Ok((v.clone(), 5)));
        assert_eq!(open(&name(&sign), &[9; 32], &rec, NOW), Err(VaultError::Invalid), "another identity's key");
        assert!(matches!(open(&name(&[4; 32]), &key, &rec, NOW), Err(VaultError::Record(_))), "another name");
        let empty = Vault::default();
        let rec = seal(&sign, &key, &empty, 1, NOW, &[3; 24]).unwrap();
        assert_eq!(open(&name(&sign), &key, &rec, NOW), Ok((empty, 1)));
    }

    #[test]
    fn padding_hides_small_changes() {
        let (sign, key) = ([1; 32], [2; 32]);
        let a = seal(&sign, &key, &vault(1, 10), 1, NOW, &[3; 24]).unwrap();
        let b = seal(&sign, &key, &vault(1, 90), 1, NOW, &[3; 24]).unwrap();
        assert_eq!(a.len(), b.len());
    }

    #[test]
    fn the_largest_vault_fits_an_ipns_record() {
        let (sign, key) = ([1; 32], [2; 32]);
        // 16 channels with the longest texts: `about`s go, then the mirror lists.
        let mut big = vault(MAX_ENTRIES as u32, 1024);
        big.entries.iter_mut().for_each(|e| e.title = "t".repeat(128));
        let rec = seal(&sign, &key, &big, u64::MAX, NOW, &[3; 24]).unwrap();
        assert!(rec.len() <= ipns::MAX_RECORD, "{} bytes", rec.len());
        let (v, _) = open(&name(&sign), &key, &rec, NOW).unwrap();
        assert!(v.entries.iter().all(|e| e.about.is_empty() && e.title.len() == 128));
        // A padded plaintext of the maximum size, too.
        let v = Vault { lease: Lease::default(), entries: vec![Entry { about: "x".repeat(4800), ..entry(0, 0) }] };
        assert!(v.plaintext().unwrap().len() == MAX_PLAIN);
        assert!(seal(&sign, &key, &v, 1, NOW, &[3; 24]).unwrap().len() <= ipns::MAX_RECORD);
    }

    #[test]
    fn tampering_and_garbage_fail() {
        let (sign, key) = ([1; 32], [2; 32]);
        let rec = seal(&sign, &key, &vault(2, 5), 5, NOW, &[3; 24]).unwrap();
        for i in (0..rec.len()).step_by(97) {
            let mut bad = rec.clone();
            bad[i] ^= 0x20;
            assert!(open(&name(&sign), &key, &bad, NOW).is_err(), "byte {i}");
        }
        assert_eq!(inline_data("/ipfs/bafyreigbtj4x7ip5legnfznufuopl4sg4knzc2cof6duas4b3q2fy6swua"), None, "a hashed CID is not inline");
        assert_eq!(inline_data(&inline_path(b"hello")).as_deref(), Some(&b"hello"[..]));
    }
}
