// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
// Copyright 2026 Anton (darkcite)
//! v3 onion addresses (rend-spec-v3 §6): `base32(pk ‖ checksum[2] ‖ 0x03).onion`, checksum =
//! `SHA3-256(".onion checksum" ‖ pk ‖ 0x03)[..2]`. Mirrors, "see also" links and the board's
//! signed host are checked as such, by the owner and by every reader (BC-14, BF-3).

use sha3::{Digest, Sha3_256};

const VERSION: u8 = 3;

fn checksum(pk: &[u8; 32]) -> [u8; 2] {
    let d = Sha3_256::new().chain_update(b".onion checksum").chain_update(pk).chain_update([VERSION]).finalize();
    [d[0], d[1]]
}

/// The address of onion service key `pk` (`<56 chars>.onion`).
pub fn address(pk: &[u8; 32]) -> String {
    let mut raw = [0u8; 35];
    raw[..32].copy_from_slice(pk);
    raw[32..34].copy_from_slice(&checksum(pk));
    raw[34] = VERSION;
    let mut s = ephem_channel::cid::base32(&raw);
    s.push_str(".onion");
    s
}

/// Whether `s` is a well-formed v3 onion address (lower case, checksum and version right).
pub fn valid(s: &str) -> bool {
    let Some(b32) = s.strip_suffix(".onion") else { return false };
    if b32.len() != 56 || !b32.bytes().all(|b| matches!(b, b'a'..=b'z' | b'2'..=b'7')) {
        return false;
    }
    let Some(raw) = ephem_channel::cid::unbase32(b32) else { return false };
    let Ok(raw) = <[u8; 35]>::try_from(raw.as_slice()) else { return false };
    let pk: [u8; 32] = raw[..32].try_into().expect("32 bytes");
    raw[34] == VERSION && raw[32..34] == checksum(&pk)
}

/// A "see also" entry: `<board name>@<onion>` (G.12), the name a `k51…` Ed25519 IPNS name.
pub fn see_also_valid(l: &str) -> bool {
    l.split_once('@').is_some_and(|(n, o)| n.len() <= 70 && ephem_channel::cid::Cid::parse(n).is_some_and(|c| c.ed25519_key().is_some()) && valid(o))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn known_address() {
        let a = address(&[0x42; 32]);
        assert!(valid(&a));
        assert_eq!(a.len(), 62);
        let mut bad = a.clone().into_bytes();
        bad[0] = if bad[0] == b'a' { b'b' } else { b'a' };
        assert!(!valid(std::str::from_utf8(&bad).unwrap()), "checksum");
        assert!(!valid(&format!("{}.onion", "a".repeat(56))));
        assert!(!valid(&format!("attacker.example/{}.onion", "a".repeat(39))));
        assert!(!valid(&a.to_uppercase()));
    }
}
