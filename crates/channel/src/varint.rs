//! Unsigned LEB128 varints (multiformats, protobuf).

/// Appends `v`.
pub fn put(out: &mut Vec<u8>, mut v: u64) {
    while v >= 0x80 {
        out.push(v as u8 | 0x80);
        v >>= 7;
    }
    out.push(v as u8);
}

/// Reads one from the front of `src`; returns it and the bytes used. Rejects overlong forms
/// (more than 10 bytes, or a trailing zero byte).
pub fn get(src: &[u8]) -> Option<(u64, usize)> {
    let mut v = 0u64;
    for (i, &b) in src.iter().enumerate().take(10) {
        v |= u64::from(b & 0x7f) << (7 * i);
        if b & 0x80 == 0 {
            if i > 0 && b == 0 {
                return None;
            }
            if i == 9 && b > 1 {
                return None;
            }
            return Some((v, i + 1));
        }
    }
    None
}

#[cfg(test)]
mod tests {
    #[test]
    fn round_trip() {
        for v in [0u64, 1, 127, 128, 300, 1 << 35, u64::MAX] {
            let mut b = Vec::new();
            super::put(&mut b, v);
            assert_eq!(super::get(&b), Some((v, b.len())));
        }
        assert_eq!(super::get(&[0x80, 0x00]), None, "overlong");
        assert_eq!(super::get(&[0x80]), None, "truncated");
    }
}
