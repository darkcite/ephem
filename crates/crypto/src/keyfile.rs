// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
// Copyright 2026 Anton (darkcite)
//! Encrypted identity key file v2 (§7.3). Sign-in/save path only (allocates; Argon2id needs
//! about 19 MiB), never on the chat path.
//!
//! Outer (all authenticated as AAD): `"P2PK" | ver 2 | kdf 1 | m_mib u16 | t u8 | p u8 |
//! salt 16 | nonce 24 | label_len u8 | label | ct_len u32`, then XChaCha20-Poly1305 ciphertext of
//! the body `seed 32 | nick_len u8 | nick | TLV sections…`. Unknown TLVs are kept on re-save.

use argon2::{Algorithm, Argon2, Params, Version};
use chacha20poly1305::aead::{AeadInPlace, KeyInit};
use chacha20poly1305::{Tag, XChaCha20Poly1305, XNonce};
use ephem_proto::ErrorCode;
use ephem_proto::buf::Rd;
use zeroize::Zeroizing;

pub const MAGIC: &[u8; 4] = b"P2PK";
pub const VERSION: u8 = 2;
pub const KDF_ARGON2ID: u8 = 1;
/// Defaults (§7.3): OWASP minimum memory, t = 4.
pub const M_MIB: u16 = 19;
pub const T_COST: u8 = 4;
pub const P_COST: u8 = 1;
pub const MAX_LABEL: usize = 32;
pub const MAX_NICK: usize = 32;
pub const MAX_BODY: usize = 65_536;
/// Minimum passphrase length enforced for saved identities (§21: a stolen file is guessed offline).
pub const MIN_PASSPHRASE: usize = 12;
const TAG_LEN: usize = 16;

/// A decrypted key file. Secrets are wiped on drop.
pub struct Opened {
    pub seed: Zeroizing<[u8; 32]>,
    pub nick: Vec<u8>,
    pub label: Vec<u8>,
    /// TLV sections after the nickname, kept verbatim for the next save.
    pub tlv: Zeroizing<Vec<u8>>,
    /// The derived file key, kept in memory so the app can re-save without asking again.
    pub key: Zeroizing<[u8; 32]>,
    pub salt: [u8; 16],
}

fn derive(pass: &[u8], salt: &[u8; 16], m_mib: u16, t: u8, p: u8) -> Result<Zeroizing<[u8; 32]>, ErrorCode> {
    let params = Params::new(m_mib as u32 * 1024, t as u32, p as u32, Some(32)).map_err(|_| ErrorCode::KeyfileInvalid)?;
    let mut key = Zeroizing::new([0u8; 32]);
    Argon2::new(Algorithm::Argon2id, Version::V0x13, params)
        .hash_password_into(pass, salt, key.as_mut())
        .map_err(|_| ErrorCode::KeyfileInvalid)?;
    Ok(key)
}

/// Derives the file key for a new save (fresh salt). Checks the passphrase policy.
pub fn new_key(pass: &[u8]) -> Result<(Zeroizing<[u8; 32]>, [u8; 16]), ErrorCode> {
    if core::str::from_utf8(pass).map_or(0, |s| s.chars().count()) < MIN_PASSPHRASE {
        return Err(ErrorCode::NotPermitted);
    }
    let mut salt = [0u8; 16];
    crate::random(&mut salt);
    Ok((derive(pass, &salt, M_MIB, T_COST, P_COST)?, salt))
}

/// Encrypts an identity with an already derived key (fresh nonce every save).
pub fn seal(key: &[u8; 32], salt: &[u8; 16], label: &[u8], seed: &[u8; 32], nick: &[u8], tlv: &[u8]) -> Result<Vec<u8>, ErrorCode> {
    if label.len() > MAX_LABEL || nick.len() > MAX_NICK || core::str::from_utf8(label).is_err() || core::str::from_utf8(nick).is_err() {
        return Err(ErrorCode::NotPermitted);
    }
    let body_len = 32 + 1 + nick.len() + tlv.len();
    if body_len > MAX_BODY {
        return Err(ErrorCode::NotPermitted);
    }
    let mut nonce = [0u8; 24];
    crate::random(&mut nonce);
    let outer_len = 4 + 1 + 1 + 4 + 16 + 24 + 1 + label.len() + 4;
    let mut out = Vec::with_capacity(outer_len + body_len + TAG_LEN);
    out.extend_from_slice(MAGIC);
    out.push(VERSION);
    out.push(KDF_ARGON2ID);
    out.extend_from_slice(&M_MIB.to_le_bytes());
    out.push(T_COST);
    out.push(P_COST);
    out.extend_from_slice(salt);
    out.extend_from_slice(&nonce);
    out.push(label.len() as u8);
    out.extend_from_slice(label);
    out.extend_from_slice(&((body_len + TAG_LEN) as u32).to_le_bytes());
    out.extend_from_slice(seed);
    out.push(nick.len() as u8);
    out.extend_from_slice(nick);
    out.extend_from_slice(tlv);
    let (aad, body) = out.split_at_mut(outer_len);
    let tag = XChaCha20Poly1305::new(key.into())
        .encrypt_in_place_detached(XNonce::from_slice(&nonce), aad, body)
        .map_err(|_| ErrorCode::CryptoFailed)?;
    out.extend_from_slice(&tag);
    Ok(out)
}

/// The plaintext label, for sign-in lists. Not authenticated until [`open`] succeeds.
pub fn label(blob: &[u8]) -> Option<&[u8]> {
    let mut r = Rd::new(blob);
    (r.take(4)? == MAGIC).then_some(())?;
    r.take(1 + 1 + 4 + 16 + 24)?;
    let n = r.u8()? as usize;
    r.take(n)
}

/// Decrypts a key file. Wrong passphrase and damaged file are the same error on purpose.
pub fn open(blob: &[u8], pass: &[u8]) -> Result<Opened, ErrorCode> {
    use ErrorCode::KeyfileInvalid as Bad;
    let mut r = Rd::new(blob);
    if r.take(4).ok_or(Bad)? != MAGIC || r.u8().ok_or(Bad)? != VERSION || r.u8().ok_or(Bad)? != KDF_ARGON2ID {
        return Err(Bad);
    }
    let m_mib = r.u16().ok_or(Bad)?;
    let (t, p) = (r.u8().ok_or(Bad)?, r.u8().ok_or(Bad)?);
    // Bound the cost an attacker-supplied file can make us pay (and what a phone can do).
    if !(8..=256).contains(&m_mib) || !(1..=16).contains(&t) || !(1..=4).contains(&p) {
        return Err(Bad);
    }
    let salt = r.arr::<16>().ok_or(Bad)?;
    let nonce = r.arr::<24>().ok_or(Bad)?;
    let ll = r.u8().ok_or(Bad)? as usize;
    if ll > MAX_LABEL {
        return Err(Bad);
    }
    let label = r.take(ll).ok_or(Bad)?.to_vec();
    let ct_len = r.u32().ok_or(Bad)? as usize;
    if !(32 + 1 + TAG_LEN..=MAX_BODY + TAG_LEN).contains(&ct_len) || r.remaining() != ct_len {
        return Err(Bad);
    }
    let aad_len = blob.len() - ct_len;
    let key = derive(pass, &salt, m_mib, t, p)?;
    let mut body = Zeroizing::new(blob[aad_len..blob.len() - TAG_LEN].to_vec());
    let tag = Tag::from_slice(&blob[blob.len() - TAG_LEN..]);
    XChaCha20Poly1305::new(key.as_ref().into())
        .decrypt_in_place_detached(XNonce::from_slice(&nonce), &blob[..aad_len], &mut body, tag)
        .map_err(|_| Bad)?;
    let mut b = Rd::new(&body);
    let seed = Zeroizing::new(b.arr::<32>().ok_or(Bad)?);
    let nl = b.u8().ok_or(Bad)? as usize;
    let nick = b.take(nl).filter(|n| n.len() <= MAX_NICK && core::str::from_utf8(n).is_ok()).ok_or(Bad)?.to_vec();
    let tlv = Zeroizing::new(b.take(b.remaining()).unwrap_or(&[]).to_vec());
    Ok(Opened { seed, nick, label, tlv, key, salt })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip_label_and_failures() {
        let pass = b"correct horse battery";
        let (key, salt) = new_key(pass).unwrap();
        let blob = seal(&key, &salt, b"Work", &[7; 32], b"alice", &[0x04, 2, 0, 9, 9]).unwrap();
        assert_eq!(label(&blob), Some(&b"Work"[..]));
        let o = open(&blob, pass).unwrap();
        assert_eq!(*o.seed, [7; 32]);
        assert_eq!(o.nick, b"alice");
        assert_eq!(o.label, b"Work");
        assert_eq!(&o.tlv[..], &[0x04, 2, 0, 9, 9], "unknown TLV kept");
        assert_eq!(*o.key, *key);

        assert_eq!(open(&blob, b"wrong horse battery").err(), Some(ErrorCode::KeyfileInvalid));
        let mut bad = blob.clone();
        let label_at = 4 + 1 + 1 + 4 + 16 + 24 + 1;
        bad[label_at] = b'X'; // the plaintext label is authenticated
        assert_eq!(open(&bad, pass).err(), Some(ErrorCode::KeyfileInvalid));
        assert_eq!(open(&blob[..blob.len() - 1], pass).err(), Some(ErrorCode::KeyfileInvalid));
        assert_eq!(new_key(b"short").err(), Some(ErrorCode::NotPermitted));

        // Re-save with the kept key: new nonce, same passphrase opens it.
        let again = seal(&o.key, &o.salt, &o.label, &o.seed, &o.nick, &o.tlv).unwrap();
        assert_ne!(again, blob);
        assert_eq!(*open(&again, pass).unwrap().seed, [7; 32]);
    }
}
