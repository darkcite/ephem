//! DataChannel frames (§11.1) and inner records (§11.2).
//!
//! Outer header (12 B, authenticated as AAD): `ver u8 | ftype u8 | flags u16 | seq u64`.
//! Inner records (inside the ciphertext): `rtype u8 | rflags u8 | len u16 | body`.

use crate::VERSION;
use crate::buf::{Buf, Rd};

pub const HEADER_LEN: usize = 12;
pub const TAG_LEN: usize = 16;
/// Largest frame on the wire (cross-browser safe DataChannel message).
pub const MAX_FRAME: usize = 16 * 1024;
/// Largest plaintext chat body.
pub const MAX_TEXT: usize = 4096;

#[repr(u8)]
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum FrameType {
    Handshake = 1,
    Transport = 2,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct Header {
    pub ftype: FrameType,
    pub flags: u16,
    pub seq: u64,
}

impl Header {
    pub fn write(&self, out: &mut [u8]) -> Result<(), ()> {
        let mut b = Buf::new(out.get_mut(..HEADER_LEN).ok_or(())?);
        b.u8(VERSION)?;
        b.u8(self.ftype as u8)?;
        b.u16(self.flags)?;
        b.u64(self.seq)
    }

    pub fn read(src: &[u8]) -> Result<Self, crate::ErrorCode> {
        use crate::ErrorCode::ProtocolMismatch;
        let mut r = Rd::new(src.get(..HEADER_LEN).ok_or(ProtocolMismatch)?);
        if r.u8() != Some(VERSION) {
            return Err(ProtocolMismatch);
        }
        let ftype = match r.u8() {
            Some(1) => FrameType::Handshake,
            Some(2) => FrameType::Transport,
            _ => return Err(ProtocolMismatch),
        };
        let flags = r.u16().ok_or(ProtocolMismatch)?;
        if flags != 0 {
            return Err(ProtocolMismatch);
        }
        let seq = r.u64().ok_or(ProtocolMismatch)?;
        Ok(Self { ftype, flags, seq })
    }
}

/// Inner record types (§11.2).
pub mod rtype {
    pub const HELLO: u8 = 0x01;
    pub const CHAT: u8 = 0x02;
    pub const ACK: u8 = 0x03;
    pub const PING: u8 = 0x04;
    pub const PONG: u8 = 0x05;
    pub const GOODBYE: u8 = 0x06;
    pub const REKEY: u8 = 0x07;
    pub const TYPING: u8 = 0x08;
    pub const READ: u8 = 0x09;
    pub const EDIT: u8 = 0x0A;
    pub const DELETE: u8 = 0x0B;
    pub const REACT: u8 = 0x0C;
    /// In-band ICE restart (§13 T1): `n_cand u8` + IceParams body.
    pub const SIGNAL_OFFER: u8 = 0x10;
    pub const SIGNAL_ANSWER: u8 = 0x11;
    /// Identity transfer (§7.6): `idx u16, total u16, ≤ 12 KiB` of the encrypted key file.
    pub const IDENTITY_CHUNK: u8 = 0x40;
    /// Identity transfer: the receiving device's user confirmed the SAS. Empty body.
    pub const IDENTITY_READY: u8 = 0x41;
    /// Rooms (§14): owner-signed room state (owner → member).
    pub const ROOM_STATE: u8 = 0x30;
    /// Rooms: `from u8, to u8, sealed box`, forwarded by the owner (§14.4).
    pub const ROOM_SIGNAL: u8 = 0x31;
    /// Rooms: a member leaves (member → owner, a proposal). Empty body.
    pub const ROOM_LEAVE: u8 = 0x32;
}

/// Record flags.
pub mod rflags {
    /// Receiver may ignore this record if it does not know the type.
    pub const IGNORABLE: u8 = 1 << 0;
    /// CHAT carries a `ttl_s u32` (self-destruct).
    pub const TTL: u8 = 1 << 1;
    /// CHAT carries `reply_sender u8, reply_seq u64`.
    pub const REPLY: u8 = 1 << 2;
    /// CHAT is a chat-setting notice.
    pub const SETTING: u8 = 1 << 3;
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct Record<'a> {
    pub rtype: u8,
    pub rflags: u8,
    pub body: &'a [u8],
}

pub fn write_record(b: &mut Buf<'_>, rtype: u8, rflags: u8, body: &[u8]) -> Result<(), ()> {
    if body.len() > u16::MAX as usize {
        return Err(());
    }
    b.u8(rtype)?;
    b.u8(rflags)?;
    b.u16(body.len() as u16)?;
    b.put(body)
}

/// Iterates records in a decrypted plaintext. Yields `Err` once on malformed input, then stops.
pub struct Records<'a> {
    r: Rd<'a>,
    failed: bool,
}

impl<'a> Records<'a> {
    pub fn new(plain: &'a [u8]) -> Self {
        Self { r: Rd::new(plain), failed: false }
    }
}

impl<'a> Iterator for Records<'a> {
    type Item = Result<Record<'a>, crate::ErrorCode>;

    fn next(&mut self) -> Option<Self::Item> {
        if self.failed || self.r.remaining() == 0 {
            return None;
        }
        let rec = (|| {
            let rtype = self.r.u8()?;
            let rflags = self.r.u8()?;
            let len = self.r.u16()? as usize;
            let body = self.r.take(len)?;
            Some(Record { rtype, rflags, body })
        })();
        match rec {
            Some(r) => Some(Ok(r)),
            None => {
                self.failed = true;
                Some(Err(crate::ErrorCode::ProtocolMismatch))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn header_roundtrip() {
        let h = Header { ftype: FrameType::Transport, flags: 0, seq: 77 };
        let mut out = [0u8; HEADER_LEN];
        h.write(&mut out).unwrap();
        assert_eq!(Header::read(&out).unwrap(), h);
        out[2] = 1;
        assert!(Header::read(&out).is_err(), "reserved flags must be zero");
    }

    #[test]
    fn records_roundtrip() {
        let mut out = [0u8; 64];
        let mut b = Buf::new(&mut out);
        write_record(&mut b, rtype::CHAT, 0, b"hi").unwrap();
        write_record(&mut b, rtype::ACK, 0, &5u64.to_le_bytes()).unwrap();
        let n = b.len();
        let recs: [Record; 2] = {
            let mut it = Records::new(&out[..n]);
            [it.next().unwrap().unwrap(), it.next().unwrap().unwrap()]
        };
        assert_eq!(recs[0].body, b"hi");
        assert_eq!(recs[1].rtype, rtype::ACK);
        let mut bad = Records::new(&out[..n - 1]);
        bad.next();
        assert!(bad.next().unwrap().is_err());
        assert!(bad.next().is_none());
    }
}
