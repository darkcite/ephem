//! base64url without padding (RFC 4648 §5), into caller buffers.

const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";

/// Encoded length for `n` input bytes (no padding).
#[inline(always)]
pub const fn encoded_len(n: usize) -> usize {
    (n * 4).div_ceil(3)
}

/// Encodes `src` into `dst`; returns the number of bytes written.
pub fn encode(src: &[u8], dst: &mut [u8]) -> Result<usize, ()> {
    let need = encoded_len(src.len());
    if dst.len() < need {
        return Err(());
    }
    let mut o = 0;
    let mut i = 0;
    while i + 3 <= src.len() {
        let n = (src[i] as u32) << 16 | (src[i + 1] as u32) << 8 | src[i + 2] as u32;
        dst[o] = ALPHABET[(n >> 18) as usize & 63];
        dst[o + 1] = ALPHABET[(n >> 12) as usize & 63];
        dst[o + 2] = ALPHABET[(n >> 6) as usize & 63];
        dst[o + 3] = ALPHABET[n as usize & 63];
        i += 3;
        o += 4;
    }
    let rem = src.len() - i;
    if rem == 1 {
        let n = (src[i] as u32) << 16;
        dst[o] = ALPHABET[(n >> 18) as usize & 63];
        dst[o + 1] = ALPHABET[(n >> 12) as usize & 63];
        o += 2;
    } else if rem == 2 {
        let n = (src[i] as u32) << 16 | (src[i + 1] as u32) << 8;
        dst[o] = ALPHABET[(n >> 18) as usize & 63];
        dst[o + 1] = ALPHABET[(n >> 12) as usize & 63];
        dst[o + 2] = ALPHABET[(n >> 6) as usize & 63];
        o += 3;
    }
    Ok(o)
}

#[inline(always)]
fn val(c: u8) -> Option<u32> {
    Some(match c {
        b'A'..=b'Z' => c - b'A',
        b'a'..=b'z' => c - b'a' + 26,
        b'0'..=b'9' => c - b'0' + 52,
        b'-' => 62,
        b'_' => 63,
        _ => return None,
    } as u32)
}

/// Decodes base64url (no padding) into `dst`; returns the number of bytes written.
pub fn decode(src: &[u8], dst: &mut [u8]) -> Result<usize, ()> {
    if src.len() % 4 == 1 {
        return Err(());
    }
    let need = src.len() * 3 / 4;
    if dst.len() < need {
        return Err(());
    }
    let mut o = 0;
    let mut i = 0;
    while i + 4 <= src.len() {
        let n = val(src[i]).ok_or(())? << 18
            | val(src[i + 1]).ok_or(())? << 12
            | val(src[i + 2]).ok_or(())? << 6
            | val(src[i + 3]).ok_or(())?;
        dst[o] = (n >> 16) as u8;
        dst[o + 1] = (n >> 8) as u8;
        dst[o + 2] = n as u8;
        i += 4;
        o += 3;
    }
    let rem = src.len() - i;
    if rem >= 2 {
        let mut n = val(src[i]).ok_or(())? << 18 | val(src[i + 1]).ok_or(())? << 12;
        if rem == 3 {
            n |= val(src[i + 2]).ok_or(())? << 6;
        }
        dst[o] = (n >> 16) as u8;
        o += 1;
        if rem == 3 {
            dst[o] = (n >> 8) as u8;
            o += 1;
        }
    }
    Ok(o)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip_all_lengths() {
        let data: [u8; 40] = core::array::from_fn(|i| (i * 37 + 11) as u8);
        for n in 0..=data.len() {
            let mut e = [0u8; 64];
            let el = encode(&data[..n], &mut e).unwrap();
            assert_eq!(el, encoded_len(n));
            let mut d = [0u8; 48];
            let dl = decode(&e[..el], &mut d).unwrap();
            assert_eq!(&d[..dl], &data[..n]);
        }
    }

    #[test]
    fn rejects_invalid() {
        let mut d = [0u8; 8];
        assert!(decode(b"ab=c", &mut d).is_err());
        assert!(decode(b"a", &mut d).is_err());
    }
}
