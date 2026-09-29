// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
// Copyright 2026 Anton (darkcite)
//! CIDv1 (multiformats): `varint(1) ‖ varint(codec) ‖ multihash`. Blocks use SHA2-256
//! (multihash `0x12`, 32 bytes); an IPNS name is a `libp2p-key` CID over the identity multihash
//! of the protobuf-encoded public key. Text forms: base32 lower (`b…`, blocks) and base36
//! lower (`k…`, IPNS names, as Kubo prints them).

use crate::varint;
use sha2::{Digest, Sha256};

pub const DAG_CBOR: u64 = 0x71;
pub const RAW: u64 = 0x55;
pub const LIBP2P_KEY: u64 = 0x72;
const SHA2_256: u64 = 0x12;
const IDENTITY: u64 = 0x00;

/// A block CID (SHA2-256) or an IPNS name CID (identity multihash of an Ed25519 key).
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Cid {
    pub codec: u64,
    /// SHA2-256 digest (blocks) or, for [`LIBP2P_KEY`], the 36-byte protobuf public key.
    pub hash: Vec<u8>,
}

impl Cid {
    /// The CID of a block's bytes.
    pub fn of(codec: u64, block: &[u8]) -> Self {
        Self { codec, hash: Sha256::digest(block).to_vec() }
    }

    /// The IPNS name of an Ed25519 public key.
    pub fn ipns_name(ed25519_pk: &[u8; 32]) -> Self {
        Self { codec: LIBP2P_KEY, hash: pubkey_protobuf(ed25519_pk).to_vec() }
    }

    /// For an IPNS name: the Ed25519 key it names.
    pub fn ed25519_key(&self) -> Option<[u8; 32]> {
        if self.codec != LIBP2P_KEY || self.hash.len() != 36 || self.hash[..4] != [0x08, 0x01, 0x12, 0x20] {
            return None;
        }
        self.hash[4..].try_into().ok()
    }

    /// Whether `block` hashes to this CID.
    pub fn verifies(&self, block: &[u8]) -> bool {
        self.codec != LIBP2P_KEY && self.hash.len() == 32 && Sha256::digest(block)[..] == self.hash[..]
    }

    pub fn to_bytes(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(4 + self.hash.len());
        varint::put(&mut out, 1);
        varint::put(&mut out, self.codec);
        varint::put(&mut out, if self.codec == LIBP2P_KEY { IDENTITY } else { SHA2_256 });
        varint::put(&mut out, self.hash.len() as u64);
        out.extend_from_slice(&self.hash);
        out
    }

    /// Reads a CID from the front of `src`; returns it and the bytes used. Only the forms this
    /// crate writes are accepted (CIDv1; SHA2-256 blocks of dag-cbor/raw; identity-hashed
    /// libp2p-key names).
    pub fn read(src: &[u8]) -> Option<(Self, usize)> {
        let (ver, a) = varint::get(src)?;
        let (codec, b) = varint::get(&src[a..])?;
        let (mh, c) = varint::get(&src[a + b..])?;
        let (len, d) = varint::get(&src[a + b + c..])?;
        let start = a + b + c + d;
        let len = usize::try_from(len).ok()?;
        let hash = src.get(start..start + len)?.to_vec();
        let ok = ver == 1
            && match codec {
                DAG_CBOR | RAW => mh == SHA2_256 && len == 32,
                LIBP2P_KEY => mh == IDENTITY && len == 36,
                _ => false,
            };
        ok.then_some((Self { codec, hash }, start + len))
    }

    pub fn from_bytes(src: &[u8]) -> Option<Self> {
        Self::read(src).filter(|(_, n)| *n == src.len()).map(|(c, _)| c)
    }

    /// `b` + base32 (RFC 4648 lower, no padding) for blocks, `k` + base36 for names.
    pub fn to_text(&self) -> String {
        let b = self.to_bytes();
        if self.codec == LIBP2P_KEY {
            format!("k{}", base36(&b))
        } else {
            format!("b{}", base32(&b))
        }
    }

    /// Parses `b…` (base32) or `k…` (base36) text.
    pub fn parse(s: &str) -> Option<Self> {
        let bytes = match s.as_bytes().first()? {
            b'b' => unbase32(&s[1..])?,
            b'k' => unbase36(&s[1..])?,
            _ => return None,
        };
        Self::from_bytes(&bytes)
    }
}

/// libp2p `PublicKey { Type = Ed25519 (1), Data = pk }`, protobuf.
pub fn pubkey_protobuf(pk: &[u8; 32]) -> [u8; 36] {
    let mut out = [0u8; 36];
    out[..4].copy_from_slice(&[0x08, 0x01, 0x12, 0x20]);
    out[4..].copy_from_slice(pk);
    out
}

const B32: &[u8; 32] = b"abcdefghijklmnopqrstuvwxyz234567";
const B36: &[u8; 36] = b"0123456789abcdefghijklmnopqrstuvwxyz";

pub fn base32(src: &[u8]) -> String {
    let mut out = String::with_capacity(src.len() * 8 / 5 + 1);
    let (mut acc, mut bits) = (0u32, 0u32);
    for &b in src {
        acc = (acc << 8) | u32::from(b);
        bits += 8;
        while bits >= 5 {
            bits -= 5;
            out.push(B32[((acc >> bits) & 31) as usize] as char);
        }
    }
    if bits > 0 {
        out.push(B32[((acc << (5 - bits)) & 31) as usize] as char);
    }
    out
}

pub fn unbase32(s: &str) -> Option<Vec<u8>> {
    let mut out = Vec::with_capacity(s.len() * 5 / 8);
    let (mut acc, mut bits) = (0u32, 0u32);
    for c in s.bytes() {
        let v = B32.iter().position(|&x| x == c.to_ascii_lowercase())? as u32;
        acc = (acc << 5) | v;
        bits += 5;
        if bits >= 8 {
            bits -= 8;
            out.push((acc >> bits) as u8);
        }
    }
    // Leftover bits must be zero padding (canonical encoding).
    (bits < 5 && acc & ((1 << bits) - 1) == 0).then_some(out)
}

pub fn base36(src: &[u8]) -> String {
    let zeros = src.iter().take_while(|&&b| b == 0).count();
    let mut digits: Vec<u8> = Vec::with_capacity(src.len() * 2);
    for &b in &src[zeros..] {
        let mut carry = u32::from(b);
        for d in digits.iter_mut() {
            carry += u32::from(*d) << 8;
            *d = (carry % 36) as u8;
            carry /= 36;
        }
        while carry > 0 {
            digits.push((carry % 36) as u8);
            carry /= 36;
        }
    }
    let mut out = String::with_capacity(zeros + digits.len());
    out.extend(std::iter::repeat_n('0', zeros));
    out.extend(digits.iter().rev().map(|&d| B36[d as usize] as char));
    out
}

pub fn unbase36(s: &str) -> Option<Vec<u8>> {
    let zeros = s.bytes().take_while(|&c| c == b'0').count();
    let mut bytes: Vec<u8> = Vec::with_capacity(s.len());
    for c in s.bytes().skip(zeros) {
        let mut carry = B36.iter().position(|&x| x == c.to_ascii_lowercase())? as u32;
        for b in bytes.iter_mut() {
            carry += u32::from(*b) * 36;
            *b = carry as u8;
            carry >>= 8;
        }
        while carry > 0 {
            bytes.push(carry as u8);
            carry >>= 8;
        }
    }
    let mut out = vec![0u8; zeros];
    out.extend(bytes.iter().rev());
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn known_forms() {
        // The empty dag-cbor map `a0`, as every IPFS implementation prints it.
        let c = Cid::of(DAG_CBOR, &[0xa0]);
        assert_eq!(c.to_text(), "bafyreigbtj4x7ip5legnfznufuopl4sg4knzc2cof6duas4b3q2fy6swua");
        assert_eq!(Cid::parse(&c.to_text()), Some(c.clone()));
        assert!(c.verifies(&[0xa0]) && !c.verifies(&[0xa1]));
        let name = Cid::ipns_name(&[7; 32]);
        let t = name.to_text();
        assert!(t.starts_with("k51"), "{t}");
        assert_eq!(Cid::parse(&t), Some(name.clone()));
        assert_eq!(name.ed25519_key(), Some([7; 32]));
        assert_eq!(Cid::parse("bafyreigbtj4x7ip5legnfznufuopl4sg4knzc2cof6duas4b3q2fy6swu"), None, "truncated");
        assert_eq!(unbase36(&base36(&[0, 0, 1, 2, 255])), Some(vec![0, 0, 1, 2, 255]));
    }
}
