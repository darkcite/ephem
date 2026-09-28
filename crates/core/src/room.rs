//! Rooms (§14): owner-controlled, up to 16 members, full mesh of pairwise Noise links.
//!
//! Authority is the **owner-signed room state**: `version`, room id and the member table
//! (`idx`, role, `PeerId`, Ed25519 signing key). Every member verifies the owner's signature with
//! the signing key the owner sent in HELLO on their own Noise link (bound to its `PeerId`), so the
//! member list and roles cannot be forged by anyone else. Roles are enforced by every receiver.
//! There is no group key: every message travels on each pairwise link (owner decision v0.9).
//!
//! Setup path (joins, removals): allocations here are acceptable.

use ephem_crypto::identity::verify;
use ephem_crypto::{Identity, PeerId};
use ephem_proto::ErrorCode;
use ephem_proto::buf::Rd;

pub const MAX_MEMBERS: usize = 16;
/// The owner is always member 0.
pub const OWNER_IDX: u8 = 0;
const DOMAIN: &[u8] = b"ephem-room-state/1";
const SIG_LEN: usize = 64;
const MEMBER_LEN: usize = 1 + 1 + 32 + 32;

#[repr(u8)]
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum RoomRole {
    Owner = 0,
    Member = 1,
    /// Read-only: receivers drop CHAT, EDIT, DELETE, TYPING and REACT from an observer (§11.7).
    Observer = 2,
}

impl RoomRole {
    pub fn from_u8(v: u8) -> Option<Self> {
        Some(match v {
            0 => Self::Owner,
            1 => Self::Member,
            2 => Self::Observer,
            _ => return None,
        })
    }
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct Member {
    pub idx: u8,
    pub role: RoomRole,
    pub peer: PeerId,
    pub sign_pk: [u8; 32],
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RoomState {
    pub room_id: [u8; 16],
    pub version: u32,
    /// Sorted by `idx`; `members[0]` is the owner.
    pub members: Vec<Member>,
}

impl RoomState {
    /// A new room owned by `owner`.
    pub fn new(room_id: [u8; 16], owner: &Identity) -> Self {
        Self { room_id, version: 1, members: vec![Member { idx: OWNER_IDX, role: RoomRole::Owner, peer: owner.peer_id(), sign_pk: owner.sign_pk() }] }
    }

    pub fn member(&self, idx: u8) -> Option<&Member> {
        self.members.iter().find(|m| m.idx == idx)
    }

    pub fn by_peer(&self, peer: &PeerId) -> Option<&Member> {
        self.members.iter().find(|m| m.peer == *peer)
    }

    pub fn owner(&self) -> &Member {
        &self.members[0]
    }

    /// Owner: admits `peer` with `role`; returns its index. Idempotent for a member already in.
    pub fn admit(&mut self, peer: PeerId, sign_pk: [u8; 32], role: RoomRole) -> Result<u8, ErrorCode> {
        if let Some(m) = self.by_peer(&peer) {
            return Ok(m.idx);
        }
        if role == RoomRole::Owner {
            return Err(ErrorCode::NotPermitted);
        }
        let idx = (1..MAX_MEMBERS as u8).find(|i| self.member(*i).is_none()).ok_or(ErrorCode::RoomFull)?;
        self.members.push(Member { idx, role, peer, sign_pk });
        self.members.sort_by_key(|m| m.idx);
        self.version += 1;
        Ok(idx)
    }

    /// Owner: removes a member (never the owner).
    pub fn remove(&mut self, idx: u8) -> Result<(), ErrorCode> {
        if idx == OWNER_IDX {
            return Err(ErrorCode::NotPermitted);
        }
        let i = self.members.iter().position(|m| m.idx == idx).ok_or(ErrorCode::InvalidRoom)?;
        self.members.remove(i);
        self.version += 1;
        Ok(())
    }

    fn encode_unsigned(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(16 + 4 + 1 + self.members.len() * MEMBER_LEN + SIG_LEN);
        out.extend_from_slice(&self.room_id);
        out.extend_from_slice(&self.version.to_le_bytes());
        out.push(self.members.len() as u8);
        for m in &self.members {
            out.push(m.idx);
            out.push(m.role as u8);
            out.extend_from_slice(&m.peer.0);
            out.extend_from_slice(&m.sign_pk);
        }
        out
    }

    fn signed_message(body: &[u8]) -> Vec<u8> {
        let mut msg = Vec::with_capacity(DOMAIN.len() + body.len());
        msg.extend_from_slice(DOMAIN);
        msg.extend_from_slice(body);
        msg
    }

    /// Owner: the ROOM_STATE record body, `state ‖ Ed25519(owner, DOMAIN ‖ state)`.
    pub fn sign(&self, owner: &Identity) -> Vec<u8> {
        debug_assert_eq!(self.owner().peer, owner.peer_id());
        let mut body = self.encode_unsigned();
        let sig = owner.sign(&Self::signed_message(&body));
        body.extend_from_slice(&sig);
        body
    }

    /// Member: decodes and checks a ROOM_STATE received on the link to `owner` (whose signing key
    /// came in that link's HELLO). Strict: unique indices < 16, exactly one owner at index 0
    /// matching the link, known roles, no trailing bytes, valid signature.
    pub fn verify(blob: &[u8], owner: &PeerId, owner_sign_pk: &[u8; 32]) -> Result<Self, ErrorCode> {
        use ErrorCode::InvalidRoom as Bad;
        if blob.len() < SIG_LEN {
            return Err(Bad);
        }
        let (body, sig) = blob.split_at(blob.len() - SIG_LEN);
        let mut sig_arr = [0u8; SIG_LEN];
        sig_arr.copy_from_slice(sig);
        if !verify(owner_sign_pk, &Self::signed_message(body), &sig_arr) {
            return Err(ErrorCode::AuthFailed);
        }
        let mut r = Rd::new(body);
        let room_id = r.arr::<16>().ok_or(Bad)?;
        let version = r.u32().ok_or(Bad)?;
        let n = r.u8().ok_or(Bad)? as usize;
        if n == 0 || n > MAX_MEMBERS {
            return Err(Bad);
        }
        let mut members: Vec<Member> = Vec::with_capacity(n);
        for _ in 0..n {
            let idx = r.u8().ok_or(Bad)?;
            let role = RoomRole::from_u8(r.u8().ok_or(Bad)?).ok_or(Bad)?;
            let peer = PeerId(r.arr::<32>().ok_or(Bad)?);
            let sign_pk = r.arr::<32>().ok_or(Bad)?;
            let owner_slot = idx == OWNER_IDX;
            if idx as usize >= MAX_MEMBERS || owner_slot != (role == RoomRole::Owner) || members.last().is_some_and(|m| m.idx >= idx) || members.iter().any(|m| m.peer == peer) {
                return Err(Bad);
            }
            members.push(Member { idx, role, peer, sign_pk });
        }
        if r.remaining() != 0 || members[0].idx != OWNER_IDX || members[0].peer != *owner || members[0].sign_pk != *owner_sign_pk {
            return Err(Bad);
        }
        Ok(Self { room_id, version, members })
    }
}

/// ROOM_SIGNAL body (§14.4): `from u8, to u8, sealed box`. The owner forwards it from `from` to
/// `to` after checking that `from` is the member on the link it arrived on.
pub fn signal_encode(from: u8, to: u8, sealed: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(2 + sealed.len());
    out.push(from);
    out.push(to);
    out.extend_from_slice(sealed);
    out
}

pub fn signal_decode(body: &[u8]) -> Option<(u8, u8, &[u8])> {
    if body.len() < 2 || body[0] as usize >= MAX_MEMBERS || body[1] as usize >= MAX_MEMBERS {
        return None;
    }
    Some((body[0], body[1], &body[2..]))
}

/// Seal context of a relayed signal: binds the box to the room and to both member indices.
pub fn seal_context(room_id: &[u8; 16], from: u8, to: u8) -> [u8; 18] {
    let mut c = [0u8; 18];
    c[..16].copy_from_slice(room_id);
    c[16] = from;
    c[17] = to;
    c
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn state_admit_sign_verify() {
        let owner = Identity::from_seed(&[1; 32]);
        let (a, b) = (Identity::from_seed(&[2; 32]), Identity::from_seed(&[3; 32]));
        let mut st = RoomState::new([9; 16], &owner);
        assert_eq!(st.admit(a.peer_id(), a.sign_pk(), RoomRole::Member), Ok(1));
        assert_eq!(st.admit(b.peer_id(), b.sign_pk(), RoomRole::Observer), Ok(2));
        assert_eq!(st.admit(a.peer_id(), a.sign_pk(), RoomRole::Member), Ok(1), "idempotent");
        assert_eq!(st.version, 3);
        let blob = st.sign(&owner);
        assert_eq!(RoomState::verify(&blob, &owner.peer_id(), &owner.sign_pk()).unwrap(), st);

        // Signed by someone else, or claimed for another owner.
        let body = st.encode_unsigned();
        let mut forged = body.clone();
        forged.extend_from_slice(&a.sign(&RoomState::signed_message(&body)));
        assert_eq!(RoomState::verify(&forged, &owner.peer_id(), &owner.sign_pk()), Err(ErrorCode::AuthFailed));
        assert!(RoomState::verify(&blob, &a.peer_id(), &owner.sign_pk()).is_err());
        let mut t = blob.clone();
        t[20] ^= 1;
        assert!(RoomState::verify(&t, &owner.peer_id(), &owner.sign_pk()).is_err(), "tampered");

        st.remove(1).unwrap();
        assert_eq!(st.remove(0), Err(ErrorCode::NotPermitted));
        assert_eq!(st.admit(a.peer_id(), a.sign_pk(), RoomRole::Member), Ok(1), "free slot reused");
        for i in 0..13u8 {
            st.admit(PeerId([100 + i; 32]), [0; 32], RoomRole::Member).unwrap();
        }
        assert_eq!(st.members.len(), 16);
        assert_eq!(st.admit(PeerId([200; 32]), [0; 32], RoomRole::Member), Err(ErrorCode::RoomFull));
    }

    #[test]
    fn signal_codec() {
        let b = signal_encode(3, 5, b"box");
        assert_eq!(signal_decode(&b), Some((3, 5, &b"box"[..])));
        assert!(signal_decode(&[16, 1]).is_none());
        assert_ne!(seal_context(&[1; 16], 1, 2), seal_context(&[1; 16], 2, 1));
    }
}
