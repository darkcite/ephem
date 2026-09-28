//! Turbotunnel encapsulation (snowflake `common/encapsulation`): packets inside a byte stream.
//!
//! Each chunk starts with a 1–3 byte length prefix: `dcxxxxxx [cyyyyyyy [0zzzzzzz]]`, where `d` = 1
//! for data (0 for padding) and `c` = another prefix byte follows. The value bits concatenate
//! (6 + 7 + 7 = 20 bits, max 1 048 575). Padding chunks are skipped.

/// Largest encodable chunk.
pub const MAX_CHUNK: usize = 0xF_FFFF;

/// Writes the data prefix for a `len`-byte packet into `out`; returns the prefix length.
#[inline]
pub fn prefix(len: usize, out: &mut [u8; 3]) -> usize {
    debug_assert!(len <= MAX_CHUNK);
    if len < 1 << 6 {
        out[0] = 0x80 | len as u8;
        1
    } else if len < 1 << 13 {
        out[0] = 0xC0 | (len >> 7) as u8;
        out[1] = (len & 0x7F) as u8;
        2
    } else {
        out[0] = 0xC0 | (len >> 14) as u8;
        out[1] = 0x80 | ((len >> 7) & 0x7F) as u8;
        out[2] = (len & 0x7F) as u8;
        3
    }
}

/// Streaming decoder: feed any split of the byte stream, get whole data packets.
/// Packets longer than `N` are a protocol error (the peer's KCP MTU is 1400).
pub struct Decoder<const N: usize> {
    state: St,
    data: bool,
    need: usize,
    have: usize,
    pkt: [u8; N],
}

#[derive(Copy, Clone, PartialEq, Eq, Debug)]
enum St {
    Prefix0,
    Prefix(u8),
    Body,
}

#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub enum DecodeError {
    /// A length prefix longer than 3 bytes.
    TooLong,
    /// A data packet larger than the decoder's buffer.
    Oversize,
}

impl<const N: usize> Default for Decoder<N> {
    fn default() -> Self {
        Self::new()
    }
}

impl<const N: usize> Decoder<N> {
    pub const fn new() -> Self {
        Self { state: St::Prefix0, data: false, need: 0, have: 0, pkt: [0; N] }
    }

    /// Restarts at a chunk boundary (a new DataChannel starts a new stream).
    pub fn reset(&mut self) {
        self.state = St::Prefix0;
        self.have = 0;
    }

    /// Consumes `input`, calling `on_packet` for every complete data packet (a view into the
    /// decoder's buffer, valid for the call).
    pub fn feed(&mut self, mut input: &[u8], mut on_packet: impl FnMut(&[u8])) -> Result<(), DecodeError> {
        while !input.is_empty() {
            match self.state {
                St::Prefix0 => {
                    let b = input[0];
                    input = &input[1..];
                    self.data = b & 0x80 != 0;
                    self.need = (b & 0x3F) as usize;
                    self.state = if b & 0x40 != 0 { St::Prefix(1) } else { self.body_start()? };
                }
                St::Prefix(n) => {
                    if n >= 3 {
                        return Err(DecodeError::TooLong);
                    }
                    let b = input[0];
                    input = &input[1..];
                    self.need = (self.need << 7) | (b & 0x7F) as usize;
                    self.state = if b & 0x80 != 0 { St::Prefix(n + 1) } else { self.body_start()? };
                }
                St::Body => {
                    let take = (self.need - self.have).min(input.len());
                    if self.data {
                        self.pkt[self.have..self.have + take].copy_from_slice(&input[..take]);
                    }
                    self.have += take;
                    input = &input[take..];
                    if self.have == self.need {
                        if self.data {
                            on_packet(&self.pkt[..self.need]);
                        }
                        self.state = St::Prefix0;
                    }
                }
            }
            // A zero-length chunk completes immediately.
            if self.state == St::Body && self.need == 0 {
                if self.data {
                    on_packet(&[]);
                }
                self.state = St::Prefix0;
            }
        }
        Ok(())
    }

    fn body_start(&mut self) -> Result<St, DecodeError> {
        if self.data && self.need > N {
            return Err(DecodeError::Oversize);
        }
        self.have = 0;
        Ok(St::Body)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn encode(pkts: &[&[u8]], out: &mut Vec<u8>) {
        for p in pkts {
            let mut pre = [0u8; 3];
            let n = prefix(p.len(), &mut pre);
            out.extend_from_slice(&pre[..n]);
            out.extend_from_slice(p);
        }
    }

    #[test]
    fn prefixes_match_the_reference_formats() {
        let mut p = [0u8; 3];
        assert_eq!((prefix(4, &mut p), p[0]), (1, 0x84));
        assert_eq!(prefix(1400, &mut p), 2);
        assert_eq!(&p[..2], &[0xC0 | (1400 >> 7) as u8, (1400 & 0x7F) as u8]);
        assert_eq!(prefix(MAX_CHUNK, &mut p), 3);
        assert_eq!(p, [0xFF, 0xFF, 0x7F]);
    }

    #[test]
    fn decode_any_split_and_skip_padding() {
        let big = vec![7u8; 1400];
        let mut s = Vec::new();
        encode(&[b"hello", &big, b""], &mut s);
        // padding: 3 bytes (00000011) and a two-byte-prefix padding of 130 bytes
        s.extend_from_slice(&[0x03, 1, 2, 3, 0x41, 0x02]);
        s.extend_from_slice(&[9u8; 130]);
        encode(&[b"end"], &mut s);
        for split in [1, 2, 3, 7, 100, s.len()] {
            let mut d = Decoder::<1500>::new();
            let mut got: Vec<Vec<u8>> = Vec::new();
            for c in s.chunks(split) {
                d.feed(c, |p| got.push(p.to_vec())).unwrap();
            }
            assert_eq!(got, vec![b"hello".to_vec(), big.clone(), vec![], b"end".to_vec()], "split {split}");
        }
    }

    #[test]
    fn errors() {
        let mut d = Decoder::<1500>::new();
        assert_eq!(d.feed(&[0xC0, 0x80, 0x80, 0x00], |_| {}), Err(DecodeError::TooLong));
        let mut d = Decoder::<16>::new();
        assert_eq!(d.feed(&[0x91], |_| {}), Err(DecodeError::Oversize));
        // Oversized padding is fine: it is skipped, not stored.
        let mut d = Decoder::<16>::new();
        d.feed(&[0x11], |_| {}).unwrap();
    }
}
