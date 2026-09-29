//! CAR v1 (content-addressed archive): `varint(len) ‖ dag-cbor {roots, version: 1}`, then
//! blocks `varint(len) ‖ CID ‖ bytes`. What the trustless gateway API serves for
//! `?format=car`, what Kubo's `ipfs dag import` reads, and the channel's backup format.

use crate::cbor::{self, Value};
use crate::cid::Cid;
use crate::varint;

/// A block and its CID.
pub type Block = (Cid, Vec<u8>);

pub fn write(roots: &[Cid], blocks: &[Block]) -> Vec<u8> {
    let header = cbor::map(vec![("roots", Value::Array(roots.iter().cloned().map(Value::Link).collect())), ("version", Value::Uint(1))]).encode();
    let mut out = Vec::with_capacity(header.len() + blocks.iter().map(|(_, b)| b.len() + 48).sum::<usize>());
    varint::put(&mut out, header.len() as u64);
    out.extend_from_slice(&header);
    for (cid, data) in blocks {
        let c = cid.to_bytes();
        varint::put(&mut out, (c.len() + data.len()) as u64);
        out.extend_from_slice(&c);
        out.extend_from_slice(data);
    }
    out
}

/// Reads a CAR: its roots and its blocks, each **verified** against its CID (a block that does
/// not hash to its CID fails the whole file).
pub fn read(src: &[u8]) -> Option<(Vec<Cid>, Vec<Block>)> {
    let (hlen, n) = varint::get(src)?;
    let mut pos = n;
    let hend = pos.checked_add(usize::try_from(hlen).ok()?)?;
    let header = Value::decode(src.get(pos..hend)?)?;
    pos = hend;
    if header.get("version")?.uint()? != 1 {
        return None;
    }
    let roots = header.get("roots")?.array()?.iter().map(|v| v.link().cloned()).collect::<Option<Vec<_>>>()?;
    let mut blocks = Vec::new();
    while pos < src.len() {
        let (len, n) = varint::get(&src[pos..])?;
        pos += n;
        let end = pos.checked_add(usize::try_from(len).ok()?)?;
        let section = src.get(pos..end)?;
        let (cid, used) = Cid::read(section)?;
        let data = &section[used..];
        if !cid.verifies(data) {
            return None;
        }
        blocks.push((cid, data.to_vec()));
        pos = end;
    }
    Some((roots, blocks))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cid::{DAG_CBOR, RAW};

    #[test]
    fn round_trip_and_tamper() {
        let a = Value::Uint(5).encode();
        let b = b"raw bytes".to_vec();
        let (ca, cb) = (Cid::of(DAG_CBOR, &a), Cid::of(RAW, &b));
        let car = write(std::slice::from_ref(&ca), &[(ca.clone(), a.clone()), (cb.clone(), b.clone())]);
        let (roots, blocks) = read(&car).unwrap();
        assert_eq!(roots, std::slice::from_ref(&ca));
        assert_eq!(blocks, [(ca, a), (cb, b)]);
        let mut bad = car.clone();
        *bad.last_mut().unwrap() ^= 1;
        assert!(read(&bad).is_none(), "a block that does not match its CID");
        assert!(read(&car[..car.len() - 1]).is_none(), "truncated");
    }
}
