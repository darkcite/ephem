// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
// Copyright 2026 Anton (darkcite)
//! Invite / answer codes (§8.3): fixed little-endian layout, parsed in place.

use crate::buf::{Buf, Rd};
use crate::candidate::CandidateBin;

/// Version of the code format (frames and cards keep [`crate::VERSION`]). 2 (2026-10-03): the
/// responder of the handshake commits to its Noise ephemeral key in its code (security audit
/// F-01; §10.4).
pub const CODE_VERSION: u8 = 2;
pub const MAX_CANDIDATES: usize = 8;
/// Length of an ephemeral-key commitment ([`Code::commit`]).
pub const COMMIT_LEN: usize = 16;
/// Largest encoded code: header + ids + key + commitment + expiry + creds (1+32, 1+32) + fp + 8 × 19.
pub const MAX_CODE_LEN: usize = 4 + 16 + 16 + 32 + COMMIT_LEN + 4 + 33 + 33 + 32 + MAX_CANDIDATES * 19;
/// Smallest valid code: a Tor invite (§28.4), which has no ICE part.
pub const MIN_CODE_LEN: usize = TOR_CODE_LEN;
/// Smallest valid WebRTC code: an invite with minimal credentials and no candidates.
const MIN_RTC_CODE_LEN: usize = 4 + 16 + 32 + (1 + 4) + (1 + 22) + 32;
/// A Tor invite: header, invite_id, room_id, static_pk, onion_pk, commitment, expires_at.
pub const TOR_CODE_LEN: usize = 4 + 16 + 16 + 32 + 32 + COMMIT_LEN + 4;

#[repr(u8)]
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum Kind {
    Invite = 1,
    Answer = 2,
    ResumeInvite = 3,
    ResumeAnswer = 4,
    /// Tor mode (§28.4): one-way invite to the inviter's onion service. No answer code.
    TorInvite = 5,
}

impl Kind {
    fn from_u8(v: u8) -> Option<Self> {
        Some(match v {
            1 => Self::Invite,
            2 => Self::Answer,
            3 => Self::ResumeInvite,
            4 => Self::ResumeAnswer,
            5 => Self::TorInvite,
            _ => return None,
        })
    }

    #[inline(always)]
    pub const fn is_invite(self) -> bool {
        matches!(self, Self::Invite | Self::ResumeInvite | Self::TorInvite)
    }

    /// Codes of the handshake's responder (it sends Noise message 2, so it picks its ephemeral
    /// key last): they carry a commitment to that key.
    #[inline(always)]
    pub const fn commits(self) -> bool {
        matches!(self, Self::Answer | Self::ResumeAnswer | Self::TorInvite)
    }
}

pub mod flags {
    pub const LAN_ONLY: u8 = 1 << 0;
    pub const GROUP: u8 = 1 << 1;
    pub const TRANSFER: u8 = 1 << 2;
    pub const OBSERVER: u8 = 1 << 3;
    pub const KNOWN: u8 = LAN_ONLY | GROUP | TRANSFER | OBSERVER;
}

/// ICE credential (ufrag 4..=32, pwd 22..=32 ICE characters), stored inline.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct Cred {
    pub len: u8,
    pub bytes: [u8; 32],
}

impl Cred {
    pub const EMPTY: Self = Self { len: 0, bytes: [0; 32] };

    pub fn new(s: &[u8]) -> Option<Self> {
        if s.len() > 32 || !s.iter().all(|c| c.is_ascii_alphanumeric() || *c == b'+' || *c == b'/') {
            return None;
        }
        let mut bytes = [0u8; 32];
        bytes[..s.len()].copy_from_slice(s);
        Some(Self { len: s.len() as u8, bytes })
    }

    #[inline(always)]
    pub fn as_bytes(&self) -> &[u8] {
        &self.bytes[..self.len as usize]
    }

    #[inline(always)]
    pub fn as_str(&self) -> &str {
        // Only ICE characters are ever stored (checked in `new`).
        core::str::from_utf8(self.as_bytes()).unwrap_or("")
    }
}

/// Transport parameters shared by both code kinds.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct IceParams {
    pub ufrag: Cred,
    pub pwd: Cred,
    pub fingerprint: [u8; 32],
    pub n_cand: u8,
    pub cands: [CandidateBin; MAX_CANDIDATES],
}

impl IceParams {
    pub const EMPTY: Self = Self {
        ufrag: Cred::EMPTY,
        pwd: Cred::EMPTY,
        fingerprint: [0; 32],
        n_cand: 0,
        cands: [CandidateBin::ZERO; MAX_CANDIDATES],
    };

    #[inline(always)]
    pub fn candidates(&self) -> &[CandidateBin] {
        &self.cands[..self.n_cand as usize]
    }

    pub fn push(&mut self, c: CandidateBin) -> bool {
        if (self.n_cand as usize) >= MAX_CANDIDATES || self.candidates().contains(&c) {
            return false;
        }
        self.cands[self.n_cand as usize] = c;
        self.n_cand += 1;
        true
    }

    /// `ufrag_len ufrag pwd_len pwd dtls_fp candidates…` (the count travels separately).
    pub fn encode_body(&self, b: &mut Buf<'_>) -> Result<(), ()> {
        b.u8(self.ufrag.len)?;
        b.put(self.ufrag.as_bytes())?;
        b.u8(self.pwd.len)?;
        b.put(self.pwd.as_bytes())?;
        b.put(&self.fingerprint)?;
        for c in self.candidates() {
            c.encode(b)?;
        }
        Ok(())
    }

    /// Strict inverse of [`Self::encode_body`] for `n_cand` candidates.
    pub fn decode_body(r: &mut Rd<'_>, n_cand: u8) -> Option<Self> {
        if n_cand as usize > MAX_CANDIDATES {
            return None;
        }
        let ul = r.u8()? as usize;
        if !(4..=32).contains(&ul) {
            return None;
        }
        let ufrag = Cred::new(r.take(ul)?)?;
        let pl = r.u8()? as usize;
        if !(22..=32).contains(&pl) {
            return None;
        }
        let pwd = Cred::new(r.take(pl)?)?;
        let fingerprint = r.arr::<32>()?;
        let mut ice = IceParams { ufrag, pwd, fingerprint, ..IceParams::EMPTY };
        for _ in 0..n_cand {
            ice.push(CandidateBin::decode(r)?);
        }
        (ice.n_cand == n_cand).then_some(ice)
    }
}

/// A decoded code. `room_id` and `expires_at` are zero for answers.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct Code {
    pub kind: Kind,
    pub flags: u8,
    pub invite_id: [u8; 16],
    pub room_id: [u8; 16],
    pub static_pk: [u8; 32],
    /// Tor invites: the inviter's onion service key (its `.onion` address). Zero otherwise.
    pub onion_pk: [u8; 32],
    /// Answers and Tor invites ([`Kind::commits`]): a commitment to the Noise ephemeral key
    /// the sender will use in message 2 (`ephem_crypto::noise::commit`). Without it, someone
    /// who swapped the codes could try ephemeral keys until both sides' safety codes match
    /// (security audit F-01). Zero for other kinds.
    pub commit: [u8; COMMIT_LEN],
    pub expires_at: u32,
    /// WebRTC codes only (empty for a Tor invite).
    pub ice: IceParams,
}

impl Code {
    pub fn encode(&self, out: &mut [u8]) -> Result<usize, ()> {
        let mut b = Buf::new(out);
        b.u8(CODE_VERSION)?;
        b.u8(self.kind as u8)?;
        b.u8(self.flags)?;
        b.u8(self.ice.n_cand)?;
        b.put(&self.invite_id)?;
        if self.kind.is_invite() {
            b.put(&self.room_id)?;
        }
        b.put(&self.static_pk)?;
        if self.kind == Kind::TorInvite {
            b.put(&self.onion_pk)?;
        }
        if self.kind.commits() {
            b.put(&self.commit)?;
        }
        if self.kind.is_invite() {
            b.u32(self.expires_at)?;
        }
        if self.kind != Kind::TorInvite {
            self.ice.encode_body(&mut b)?;
        }
        Ok(b.len())
    }

    /// Strict decoding: unknown version/kind, reserved flag bits, out-of-range lengths and
    /// trailing bytes are all rejected (`E_INVALID_INVITE` / `E_PROTOCOL_MISMATCH`).
    pub fn decode(src: &[u8]) -> Result<Self, crate::ErrorCode> {
        use crate::ErrorCode::{InvalidInvite, ProtocolMismatch};
        // Too short to be any code: garbage, not a code from another version.
        if !(MIN_CODE_LEN..=MAX_CODE_LEN).contains(&src.len()) {
            return Err(InvalidInvite);
        }
        let mut r = Rd::new(src);
        if r.u8().ok_or(InvalidInvite)? != CODE_VERSION {
            return Err(ProtocolMismatch);
        }
        let kind = Kind::from_u8(r.u8().ok_or(InvalidInvite)?).ok_or(InvalidInvite)?;
        let tor = kind == Kind::TorInvite;
        if (tor && src.len() != TOR_CODE_LEN) || (!tor && src.len() < MIN_RTC_CODE_LEN) {
            return Err(InvalidInvite);
        }
        let flags = r.u8().ok_or(InvalidInvite)?;
        if flags & !flags::KNOWN != 0 {
            return Err(InvalidInvite);
        }
        let n_cand = r.u8().ok_or(InvalidInvite)?;
        let invite_id = r.arr::<16>().ok_or(InvalidInvite)?;
        let room_id = if kind.is_invite() { r.arr::<16>().ok_or(InvalidInvite)? } else { [0; 16] };
        let static_pk = r.arr::<32>().ok_or(InvalidInvite)?;
        let onion_pk = if tor { r.arr::<32>().ok_or(InvalidInvite)? } else { [0; 32] };
        let commit = if kind.commits() { r.arr::<COMMIT_LEN>().ok_or(InvalidInvite)? } else { [0; COMMIT_LEN] };
        let expires_at = if kind.is_invite() { r.u32().ok_or(InvalidInvite)? } else { 0 };
        let ice = if tor {
            if n_cand != 0 {
                return Err(InvalidInvite);
            }
            IceParams::EMPTY
        } else {
            IceParams::decode_body(&mut r, n_cand).ok_or(InvalidInvite)?
        };
        if r.remaining() != 0 {
            return Err(InvalidInvite);
        }
        Ok(Self { kind, flags, invite_id, room_id, static_pk, onion_pk, commit, expires_at, ice })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    pub(crate) fn sample(kind: Kind) -> Code {
        let mut ice = IceParams {
            ufrag: Cred::new(b"A/uL").unwrap(),
            pwd: Cred::new(b"55bZhqbovQp1LFLkFt9yUC/k").unwrap(),
            fingerprint: core::array::from_fn(|i| i as u8),
            ..IceParams::EMPTY
        };
        ice.push(CandidateBin::from_sdp_parts("9090b126-3aae-4a3e-b714-5d089ddfbff0.local", "41731", "host").unwrap());
        ice.push(CandidateBin::from_sdp_parts("171.97.169.36", "55298", "srflx").unwrap());
        Code { kind, flags: 0, invite_id: [7; 16], room_id: if kind.is_invite() { [9; 16] } else { [0; 16] }, static_pk: [3; 32], onion_pk: [0; 32], commit: if kind.commits() { [5; COMMIT_LEN] } else { [0; COMMIT_LEN] }, expires_at: if kind.is_invite() { 1_790_000_000 } else { 0 }, ice }
    }

    #[test]
    fn tor_invite_roundtrip_and_strictness() {
        let c = Code { kind: Kind::TorInvite, flags: flags::GROUP, invite_id: [1; 16], room_id: [2; 16], static_pk: [3; 32], onion_pk: [4; 32], commit: [5; COMMIT_LEN], expires_at: 1_790_000_000, ice: IceParams::EMPTY };
        let mut out = [0u8; MAX_CODE_LEN];
        let n = c.encode(&mut out).unwrap();
        assert_eq!(n, TOR_CODE_LEN);
        assert_eq!(n, 120, "§28.4");
        assert_eq!(Code::decode(&out[..n]).unwrap(), c);
        assert!(Code::decode(&out[..n - 1]).is_err(), "short");
        let mut long = out[..n].to_vec();
        long.push(0);
        assert!(Code::decode(&long).is_err(), "trailing byte");
        out[3] = 1;
        assert!(Code::decode(&out[..n]).is_err(), "candidates in a Tor invite");
    }

    #[test]
    fn invite_roundtrip_and_size() {
        let c = sample(Kind::Invite);
        let mut out = [0u8; MAX_CODE_LEN];
        let n = c.encode(&mut out).unwrap();
        // 4 + 16 + 16 + 32 + 4 + (1+4) + (1+24) + 32 + 19 + 7 = 160 (S2: Chrome mDNS invite 153 B + srflx-v4 7 B)
        assert_eq!(n, 160);
        assert_eq!(Code::decode(&out[..n]).unwrap(), c);
    }

    #[test]
    fn answer_roundtrip() {
        let c = sample(Kind::Answer);
        let mut out = [0u8; MAX_CODE_LEN];
        let n = c.encode(&mut out).unwrap();
        assert_eq!(n, 160 - 16 - 4 + COMMIT_LEN, "an answer: no room id or expiry, a commitment");
        assert_eq!(Code::decode(&out[..n]).unwrap(), c);
    }

    #[test]
    fn strictness() {
        let c = sample(Kind::Invite);
        let mut out = [0u8; MAX_CODE_LEN + 1];
        let n = c.encode(&mut out).unwrap();
        assert!(Code::decode(&out[..n + 1]).is_err(), "trailing byte");
        let mut bad = out;
        bad[0] = 1;
        assert_eq!(Code::decode(&bad[..n]), Err(crate::ErrorCode::ProtocolMismatch), "a version-1 code (no commitment)");
        let mut bad = out;
        bad[2] = 0x80;
        assert!(Code::decode(&bad[..n]).is_err(), "reserved flag");
        assert!(Code::decode(&out[..n - 1]).is_err(), "truncated");
        assert_eq!(Code::decode(&[0, 0, 0]), Err(crate::ErrorCode::InvalidInvite), "garbage is not a version mismatch");
    }
}
