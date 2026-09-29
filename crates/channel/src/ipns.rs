// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
// Copyright 2026 Anton (darkcite)
//! IPNS records (the IPNS Record specification, V2 with V1 compatibility fields).
//!
//! Protobuf `IpnsEntry`: 1 `value`, 2 `signatureV1`, 3 `validityType` (0 = EOL), 4 `validity`
//! (RFC 3339), 5 `sequence`, 6 `ttl` (ns), 8 `signatureV2`, 9 `data`. `data` is dag-cbor
//! `{TTL, Value, Sequence, Validity, ValidityType}`; `signatureV2` is Ed25519 over
//! `"ipns-signature:" ‖ data`. Verification uses only V2 and requires any V1 field present to
//! match `data` (so a reader never acts on an unsigned field). The Ed25519 key is the one the
//! name inlines, so records carry no `pubKey`.

use crate::cbor::{self, Value};
use crate::cid::Cid;
use crate::time;
use crate::varint;
use ed25519_dalek::{Signer, SigningKey, Verifier, VerifyingKey};

/// Largest record accepted (the specification's limit).
pub const MAX_RECORD: usize = 10 * 1024;
const SIG_V2_PREFIX: &[u8] = b"ipns-signature:";

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Record {
    /// e.g. `/ipfs/bafy…`
    pub value: String,
    pub sequence: u64,
    /// End of validity, Unix seconds.
    pub validity: u64,
    pub ttl_ns: u64,
}

fn data_cbor(r: &Record, validity_text: &str) -> Vec<u8> {
    cbor::map(vec![
        ("TTL", Value::Uint(r.ttl_ns)),
        ("Value", Value::Bytes(r.value.as_bytes().to_vec())),
        ("Sequence", Value::Uint(r.sequence)),
        ("Validity", Value::Bytes(validity_text.as_bytes().to_vec())),
        ("ValidityType", Value::Uint(0)),
    ])
    .encode()
}

fn field_bytes(out: &mut Vec<u8>, n: u64, b: &[u8]) {
    varint::put(out, n << 3 | 2);
    varint::put(out, b.len() as u64);
    out.extend_from_slice(b);
}

fn field_uint(out: &mut Vec<u8>, n: u64, v: u64) {
    varint::put(out, n << 3);
    varint::put(out, v);
}

/// A signed record (protobuf bytes) for `r` with the channel key.
pub fn create(key: &SigningKey, r: &Record) -> Vec<u8> {
    let validity = time::rfc3339(r.validity);
    let data = data_cbor(r, &validity);
    let mut v1 = Vec::with_capacity(r.value.len() + validity.len() + 3);
    v1.extend_from_slice(r.value.as_bytes());
    v1.extend_from_slice(validity.as_bytes());
    v1.extend_from_slice(b"EOL");
    let mut v2 = SIG_V2_PREFIX.to_vec();
    v2.extend_from_slice(&data);
    let mut out = Vec::with_capacity(512);
    field_bytes(&mut out, 1, r.value.as_bytes());
    field_bytes(&mut out, 2, &key.sign(&v1).to_bytes());
    field_uint(&mut out, 3, 0);
    field_bytes(&mut out, 4, validity.as_bytes());
    field_uint(&mut out, 5, r.sequence);
    field_uint(&mut out, 6, r.ttl_ns);
    field_bytes(&mut out, 8, &key.sign(&v2).to_bytes());
    field_bytes(&mut out, 9, &data);
    out
}

/// A signed **V2-only** record: `signatureV2` and `data`, no V1 fields (which would repeat the
/// value; the vault needs the room, §D.11.2).
pub fn create_v2(key: &SigningKey, r: &Record) -> Vec<u8> {
    let data = data_cbor(r, &time::rfc3339(r.validity));
    let mut v2 = Vec::with_capacity(SIG_V2_PREFIX.len() + data.len());
    v2.extend_from_slice(SIG_V2_PREFIX);
    v2.extend_from_slice(&data);
    let mut out = Vec::with_capacity(data.len() + 80);
    field_bytes(&mut out, 8, &key.sign(&v2).to_bytes());
    field_bytes(&mut out, 9, &data);
    out
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum RecordError {
    Malformed,
    BadSignature,
    /// The record's key is not the name's.
    WrongKey,
    Expired,
}

/// Verifies a record for IPNS name `name` at `now_s` and returns its content.
pub fn verify(name: &Cid, bytes: &[u8], now_s: u64) -> Result<Record, RecordError> {
    use RecordError::*;
    let pk = name.ed25519_key().ok_or(WrongKey)?;
    if bytes.len() > MAX_RECORD {
        return Err(Malformed);
    }
    let (mut value, mut vtype, mut validity, mut seq, mut ttl, mut pubkey, mut sig2, mut data) = (None, None, None, None, None, None, None, None);
    let mut pos = 0;
    while pos < bytes.len() {
        let (tag, n) = varint::get(&bytes[pos..]).ok_or(Malformed)?;
        pos += n;
        match tag & 7 {
            0 => {
                let (v, n) = varint::get(&bytes[pos..]).ok_or(Malformed)?;
                pos += n;
                match tag >> 3 {
                    3 => vtype = Some(v),
                    5 => seq = Some(v),
                    6 => ttl = Some(v),
                    _ => {}
                }
            }
            2 => {
                let (len, n) = varint::get(&bytes[pos..]).ok_or(Malformed)?;
                pos += n;
                let end = pos.checked_add(usize::try_from(len).map_err(|_| Malformed)?).ok_or(Malformed)?;
                let b = bytes.get(pos..end).ok_or(Malformed)?;
                pos = end;
                match tag >> 3 {
                    1 => value = Some(b),
                    4 => validity = Some(b),
                    7 => pubkey = Some(b),
                    8 => sig2 = Some(b),
                    9 => data = Some(b),
                    _ => {} // signatureV1 and unknown fields: not trusted, not needed
                }
            }
            _ => return Err(Malformed),
        }
    }
    if let Some(pkb) = pubkey
        && pkb != crate::cid::pubkey_protobuf(&pk)
    {
        return Err(WrongKey);
    }
    let (sig2, data) = (sig2.ok_or(Malformed)?, data.ok_or(Malformed)?);
    let key = VerifyingKey::from_bytes(&pk).map_err(|_| WrongKey)?;
    let sig = ed25519_dalek::Signature::from_slice(sig2).map_err(|_| BadSignature)?;
    let mut signed = SIG_V2_PREFIX.to_vec();
    signed.extend_from_slice(data);
    key.verify(&signed, &sig).map_err(|_| BadSignature)?;
    // Only the signed `data` counts; V1 fields, if present, must agree with it.
    let d = Value::decode(data).ok_or(Malformed)?;
    let dv = d.get("Value").and_then(Value::bytes).ok_or(Malformed)?;
    let dvalidity = d.get("Validity").and_then(Value::bytes).ok_or(Malformed)?;
    let dtype = d.get("ValidityType").and_then(Value::uint).ok_or(Malformed)?;
    let dseq = d.get("Sequence").and_then(Value::uint).ok_or(Malformed)?;
    let dttl = d.get("TTL").and_then(Value::uint).ok_or(Malformed)?;
    let agrees = value.is_none_or(|v| v == dv)
        && validity.is_none_or(|v| v == dvalidity)
        && vtype.is_none_or(|v| v == dtype)
        && seq.is_none_or(|v| v == dseq)
        && ttl.is_none_or(|v| v == dttl);
    if !agrees || dtype != 0 {
        return Err(Malformed);
    }
    let text = std::str::from_utf8(dvalidity).map_err(|_| Malformed)?;
    let until = time::parse_rfc3339(text).ok_or(Malformed)?;
    if now_s > until {
        return Err(Expired);
    }
    Ok(Record { value: String::from_utf8(dv.to_vec()).map_err(|_| Malformed)?, sequence: dseq, validity: until, ttl_ns: dttl })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn create_and_verify() {
        let key = SigningKey::from_bytes(&[9; 32]);
        let name = Cid::ipns_name(&key.verifying_key().to_bytes());
        let r = Record { value: "/ipfs/bafyreigbtj4x7ip5legnfznufuopl4sg4knzc2cof6duas4b3q2fy6swua".into(), sequence: 3, validity: 1_800_000_000, ttl_ns: 60_000_000_000 };
        let rec = create(&key, &r);
        assert_eq!(verify(&name, &rec, 1_790_000_000), Ok(r.clone()));
        assert_eq!(verify(&name, &rec, 1_800_000_001), Err(RecordError::Expired));
        let other = Cid::ipns_name(&SigningKey::from_bytes(&[8; 32]).verifying_key().to_bytes());
        assert_eq!(verify(&other, &rec, 1_790_000_000), Err(RecordError::BadSignature));
        // Changing the unsigned V1 value field is caught.
        let mut bad = rec.clone();
        let i = bad.windows(6).position(|w| w == b"/ipfs/").unwrap();
        bad[i + 7] ^= 1;
        assert_eq!(verify(&name, &bad, 1_790_000_000), Err(RecordError::Malformed));
        // Changing the signed data breaks the signature.
        let mut bad = rec;
        let n = bad.len();
        bad[n - 3] ^= 1;
        assert!(verify(&name, &bad, 1_790_000_000).is_err());
    }
}
