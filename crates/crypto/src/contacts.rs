// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
// Copyright 2026 Anton (darkcite)
//! Contacts (§7.5): a fixed-capacity table that lives in RAM while signed in and on disk only
//! inside the encrypted key file (TLV 0x01). Saved identities only. Setup/UI path: allocations
//! here happen on sign-in, on edits and on save, never while chatting.

use crate::identity::PeerId;
use ephem_proto::buf::Rd;

pub const MAX_CONTACTS: usize = 256;
pub const MAX_NICK: usize = 32;
/// Key-file TLV types (§7.3).
pub const TLV_CONTACTS: u8 = 0x01;
pub const TLV_CARD: u8 = 0x04;
/// Settings sections the adapter keeps as opaque UTF-8 in the key file's other sections
/// (Appendix F): custom Tor bridge lines, the channels followed.
pub const TLV_TOR_BRIDGES: u8 = 0x05;
pub const TLV_FOLLOWS: u8 = 0x06;

/// The value of section `t` in a TLV area (`others` of [`Contacts::from_tlv`]).
pub fn section(tlv: &[u8], t: u8) -> Option<&[u8]> {
    let mut r = Rd::new(tlv);
    while r.remaining() > 0 {
        let (k, len) = (r.u8()?, r.u16()? as usize);
        let v = r.take(len)?;
        if k == t {
            return Some(v);
        }
    }
    None
}

/// Replaces (or with an empty `value`, removes) section `t` in a TLV area; the other sections
/// keep their order. `false` if `value` does not fit a section (64 KiB).
pub fn set_section(tlv: &mut Vec<u8>, t: u8, value: &[u8]) -> bool {
    if value.len() > u16::MAX as usize {
        return false;
    }
    let mut out = Vec::with_capacity(tlv.len() + 3 + value.len());
    let mut r = Rd::new(tlv);
    while r.remaining() > 0 {
        let (Some(k), Some(len)) = (r.u8(), r.u16()) else { break };
        let Some(v) = r.take(len as usize) else { break };
        if k != t {
            out.push(k);
            out.extend_from_slice(&len.to_le_bytes());
            out.extend_from_slice(v);
        }
    }
    if !value.is_empty() {
        out.push(t);
        out.extend_from_slice(&(value.len() as u16).to_le_bytes());
        out.extend_from_slice(value);
    }
    tlv.fill(0);
    *tlv = out;
    true
}
/// Default lifetime of a new contact card (§7.5).
pub const CARD_TTL_S: u32 = 30 * 24 * 3600;

pub mod cflags {
    /// The SAS was compared with this peer.
    pub const VERIFIED: u8 = 1 << 0;
    pub const HAS_ONION: u8 = 1 << 1;
    pub const HAS_SIGN: u8 = 1 << 2;
    /// Added from their contact card, whose secret opens the first Tor dial (§28.4 case 3).
    pub const FROM_CARD: u8 = 1 << 3;
    pub const KNOWN: u8 = VERIFIED | HAS_ONION | HAS_SIGN | FROM_CARD;
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct Contact {
    pub peer_id: PeerId,
    pub flags: u8,
    pub onion_pk: [u8; 32],
    pub sign_pk: [u8; 32],
    /// The secret of the card this contact was added from (flag `FROM_CARD`).
    pub card_secret: [u8; 16],
    pub added_at: u32,
    nick_len: u8,
    nick: [u8; MAX_NICK],
}

impl Contact {
    #[inline]
    pub fn nick(&self) -> &[u8] {
        &self.nick[..self.nick_len as usize]
    }

    #[inline(always)]
    pub fn verified(&self) -> bool {
        self.flags & cflags::VERIFIED != 0
    }

    fn set_nick(&mut self, nick: &[u8]) -> bool {
        if nick.len() > MAX_NICK || !ephem_proto::card::valid_nick(nick) {
            return false;
        }
        self.nick = [0; MAX_NICK];
        self.nick[..nick.len()].copy_from_slice(nick);
        self.nick_len = nick.len() as u8;
        true
    }

    fn encode(&self, out: &mut Vec<u8>) {
        out.extend_from_slice(&self.peer_id.0);
        out.push(self.flags);
        if self.flags & cflags::HAS_ONION != 0 {
            out.extend_from_slice(&self.onion_pk);
        }
        if self.flags & cflags::HAS_SIGN != 0 {
            out.extend_from_slice(&self.sign_pk);
        }
        if self.flags & cflags::FROM_CARD != 0 {
            out.extend_from_slice(&self.card_secret);
        }
        out.extend_from_slice(&self.added_at.to_le_bytes());
        out.push(self.nick_len);
        out.extend_from_slice(self.nick());
    }

    fn decode(r: &mut Rd<'_>) -> Option<Self> {
        let peer_id = PeerId(r.arr::<32>()?);
        let flags = r.u8()?;
        if flags & !cflags::KNOWN != 0 {
            return None;
        }
        let onion_pk = if flags & cflags::HAS_ONION != 0 { r.arr::<32>()? } else { [0; 32] };
        let sign_pk = if flags & cflags::HAS_SIGN != 0 { r.arr::<32>()? } else { [0; 32] };
        let card_secret = if flags & cflags::FROM_CARD != 0 { r.arr::<16>()? } else { [0; 16] };
        let added_at = r.u32()?;
        let n = r.u8()? as usize;
        let mut c = Contact { peer_id, flags, onion_pk, sign_pk, card_secret, added_at, nick_len: 0, nick: [0; MAX_NICK] };
        let nick = r.take(n)?;
        // A name saved by an older version may break today's rules (F-04): keep the contact and
        // drop the name (the handle is shown instead) rather than refuse the whole key file.
        if !c.set_nick(nick) && core::str::from_utf8(nick).is_err() {
            return None;
        }
        Some(c)
    }
}

/// Why a contact operation was refused.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum ContactError {
    Full,
    BadNick,
    NotFound,
}

/// Our own contact card's secret (TLV 0x04): whoever holds a card with it may dial us once.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct OwnCard {
    pub secret: [u8; 16],
    /// Unix seconds; 0 = never.
    pub expires_at: u32,
}

impl OwnCard {
    #[inline(always)]
    pub fn live(&self, now_s: u32) -> bool {
        self.expires_at == 0 || now_s <= self.expires_at
    }
}

#[derive(Default)]
pub struct Contacts {
    list: Vec<Contact>,
    /// Our current contact card (none until the user shows one).
    pub card: Option<OwnCard>,
}

impl Contacts {
    pub fn new() -> Self {
        Self { list: Vec::with_capacity(MAX_CONTACTS), card: None }
    }

    /// Adds a contact from their card (§7.5): unverified, with their onion key and the card's
    /// secret. An existing contact keeps its verification and nickname; it only gains the
    /// onion key if it had none.
    pub fn add_from_card(&mut self, peer: PeerId, onion_pk: [u8; 32], secret: [u8; 16], nick: &[u8], now_s: u32) -> Result<(), ContactError> {
        if let Some(c) = self.list.iter_mut().find(|c| c.peer_id == peer) {
            if c.flags & cflags::HAS_ONION == 0 {
                c.onion_pk = onion_pk;
                c.flags |= cflags::HAS_ONION;
            }
            return Ok(());
        }
        if self.list.len() >= MAX_CONTACTS {
            return Err(ContactError::Full);
        }
        let mut c = Contact {
            peer_id: peer,
            flags: cflags::HAS_ONION | cflags::FROM_CARD,
            onion_pk,
            sign_pk: [0; 32],
            card_secret: secret,
            added_at: now_s,
            nick_len: 0,
            nick: [0; MAX_NICK],
        };
        if !c.set_nick(nick) {
            return Err(ContactError::BadNick);
        }
        self.list.push(c);
        Ok(())
    }

    /// The contact now knows us (a connection went through): later dials are contact dials,
    /// so the card's secret is no longer needed (and may have been reset since).
    pub fn card_used(&mut self, peer: &PeerId) {
        if let Some(c) = self.list.iter_mut().find(|c| c.peer_id == *peer) {
            c.flags &= !cflags::FROM_CARD;
            c.card_secret = [0; 16];
        }
    }

    #[inline]
    pub fn list(&self) -> &[Contact] {
        &self.list
    }

    pub fn get(&self, peer: &PeerId) -> Option<&Contact> {
        self.list.iter().find(|c| c.peer_id == *peer)
    }

    /// A verified contact whose nickname equals `nick` (ASCII case-insensitive) but whose key is
    /// not `peer`: someone calling themselves by that name (impersonation warning, §7.5).
    pub fn impersonated(&self, nick: &[u8], peer: &PeerId) -> Option<&Contact> {
        if nick.is_empty() {
            return None;
        }
        self.list.iter().find(|c| c.verified() && c.peer_id != *peer && c.nick().eq_ignore_ascii_case(nick))
    }

    /// Adds a contact, or updates its nickname and keys. `verified` only ever turns on here.
    pub fn save(&mut self, peer: PeerId, sign_pk: Option<[u8; 32]>, verified: bool, nick: &[u8], now_s: u32) -> Result<(), ContactError> {
        if let Some(c) = self.list.iter_mut().find(|c| c.peer_id == peer) {
            if !c.set_nick(nick) {
                return Err(ContactError::BadNick);
            }
            if let Some(k) = sign_pk {
                c.sign_pk = k;
                c.flags |= cflags::HAS_SIGN;
            }
            if verified {
                c.flags |= cflags::VERIFIED;
            }
            return Ok(());
        }
        if self.list.len() >= MAX_CONTACTS {
            return Err(ContactError::Full);
        }
        let mut c = Contact {
            peer_id: peer,
            flags: if verified { cflags::VERIFIED } else { 0 },
            onion_pk: [0; 32],
            sign_pk: [0; 32],
            card_secret: [0; 16],
            added_at: now_s,
            nick_len: 0,
            nick: [0; MAX_NICK],
        };
        if !c.set_nick(nick) {
            return Err(ContactError::BadNick);
        }
        if let Some(k) = sign_pk {
            c.sign_pk = k;
            c.flags |= cflags::HAS_SIGN;
        }
        self.list.push(c);
        Ok(())
    }

    /// Stores the contact's onion service key (learned in a Tor chat, §28.7).
    pub fn set_onion(&mut self, peer: &PeerId, onion_pk: [u8; 32]) -> Result<(), ContactError> {
        let c = self.list.iter_mut().find(|c| c.peer_id == *peer).ok_or(ContactError::NotFound)?;
        c.onion_pk = onion_pk;
        c.flags |= cflags::HAS_ONION;
        Ok(())
    }

    pub fn set_verified(&mut self, peer: &PeerId) -> Result<(), ContactError> {
        let c = self.list.iter_mut().find(|c| c.peer_id == *peer).ok_or(ContactError::NotFound)?;
        c.flags |= cflags::VERIFIED;
        Ok(())
    }

    pub fn remove(&mut self, peer: &PeerId) -> Result<(), ContactError> {
        let i = self.list.iter().position(|c| c.peer_id == *peer).ok_or(ContactError::NotFound)?;
        self.list.remove(i);
        Ok(())
    }

    /// The key-file TLV area: CONTACTS and CARD (if any) followed by `others` (unknown
    /// sections, kept).
    pub fn to_tlv(&self, others: &[u8]) -> Vec<u8> {
        let mut out = Vec::new();
        if let Some(k) = self.card {
            out.push(TLV_CARD);
            out.extend_from_slice(&20u16.to_le_bytes());
            out.extend_from_slice(&k.secret);
            out.extend_from_slice(&k.expires_at.to_le_bytes());
        }
        if !self.list.is_empty() {
            let mut v = Vec::with_capacity(2 + self.list.len() * 134);
            v.extend_from_slice(&(self.list.len() as u16).to_le_bytes());
            for c in &self.list {
                c.encode(&mut v);
            }
            out.push(TLV_CONTACTS);
            out.extend_from_slice(&(v.len() as u16).to_le_bytes());
            out.extend_from_slice(&v);
        }
        out.extend_from_slice(others);
        out
    }

    /// Splits a key-file TLV area into the contacts and the other sections (kept verbatim).
    /// A malformed area is `None` (`E_KEYFILE_INVALID`).
    pub fn from_tlv(tlv: &[u8]) -> Option<(Self, Vec<u8>)> {
        let mut contacts = Self::new();
        let mut others = Vec::new();
        let mut r = Rd::new(tlv);
        while r.remaining() > 0 {
            let t = r.u8()?;
            let len = r.u16()? as usize;
            let value = r.take(len)?;
            if t == TLV_CARD {
                let mut v = Rd::new(value);
                let card = OwnCard { secret: v.arr::<16>()?, expires_at: v.u32()? };
                if v.remaining() != 0 || contacts.card.is_some() {
                    return None;
                }
                contacts.card = Some(card);
                continue;
            }
            if t != TLV_CONTACTS {
                others.push(t);
                others.extend_from_slice(&(len as u16).to_le_bytes());
                others.extend_from_slice(value);
                continue;
            }
            let mut v = Rd::new(value);
            let n = v.u16()? as usize;
            if n > MAX_CONTACTS || !contacts.list.is_empty() {
                return None;
            }
            for _ in 0..n {
                let c = Contact::decode(&mut v)?;
                if contacts.get(&c.peer_id).is_some() {
                    return None;
                }
                contacts.list.push(c);
            }
            if v.remaining() != 0 {
                return None;
            }
        }
        Some((contacts, others))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Cards (§7.5): our card's secret and contacts added from cards survive the key file;
    /// a card contact loses its secret once it has connected.
    #[test]
    fn cards_in_the_key_file() {
        let mut c = Contacts::new();
        let (carol, dave) = (PeerId([7; 32]), PeerId([8; 32]));
        c.card = Some(OwnCard { secret: [5; 16], expires_at: 0 });
        c.add_from_card(carol, [9; 32], [6; 16], "Кэрол".as_bytes(), 50).unwrap();
        c.save(dave, None, true, b"Dave", 51).unwrap();
        c.add_from_card(dave, [4; 32], [3; 16], b"ignored", 52).unwrap();
        let d = *c.get(&dave).unwrap();
        assert!(d.verified() && d.nick() == b"Dave" && d.flags & cflags::FROM_CARD == 0, "an existing contact keeps its state");
        assert_eq!(d.onion_pk, [4; 32], "but gains the onion key");
        let (back, kept) = Contacts::from_tlv(&c.to_tlv(&[])).unwrap();
        assert!(kept.is_empty());
        assert_eq!(back.card, c.card);
        assert_eq!(back.list(), c.list());
        let k = back.get(&carol).unwrap();
        assert!(!k.verified() && k.flags & cflags::FROM_CARD != 0 && k.card_secret == [6; 16] && k.onion_pk == [9; 32]);
        c.card_used(&carol);
        assert_eq!(c.get(&carol).unwrap().flags & cflags::FROM_CARD, 0);
        assert!(OwnCard { secret: [0; 16], expires_at: 10 }.live(10) && !OwnCard { secret: [0; 16], expires_at: 10 }.live(11));
        let mut two = c.to_tlv(&[]);
        two.extend_from_slice(&[TLV_CARD, 20, 0]);
        two.extend_from_slice(&[0; 20]);
        assert!(Contacts::from_tlv(&two).is_none(), "two cards");
    }

    #[test]
    fn save_verify_impersonation_roundtrip() {
        let mut c = Contacts::new();
        let (alice, bob, eve) = (PeerId([1; 32]), PeerId([2; 32]), PeerId([3; 32]));
        c.save(alice, Some([9; 32]), false, "Alice".as_bytes(), 100).unwrap();
        c.save(bob, None, true, b"Bob", 101).unwrap();
        assert!(!c.get(&alice).unwrap().verified());
        c.set_verified(&alice).unwrap();
        c.save(alice, None, false, b"Alice W", 200).unwrap();
        c.set_onion(&alice, [7; 32]).unwrap();
        assert_eq!(c.set_onion(&PeerId([0xee; 32]), [7; 32]), Err(ContactError::NotFound));
        let a = c.get(&alice).unwrap();
        assert!(a.verified(), "verified never turns off by a save");
        assert_eq!((a.nick(), a.sign_pk, a.added_at), (&b"Alice W"[..], [9; 32], 100));
        assert!(c.impersonated(b"bob", &eve).is_some(), "Eve calling herself Bob");
        assert!(c.impersonated(b"Bob", &bob).is_none(), "the real Bob");
        assert_eq!(c.save(eve, None, false, &[0xff], 1), Err(ContactError::BadNick));

        let others = [0x7fu8, 2, 0, 7, 7];
        let tlv = c.to_tlv(&others);
        let (back, kept) = Contacts::from_tlv(&tlv).unwrap();
        assert_eq!(back.list(), c.list());
        assert_eq!(kept, others, "unknown sections kept verbatim");
        assert!(Contacts::from_tlv(&tlv[..tlv.len() - 1]).is_none(), "truncated");
        c.remove(&bob).unwrap();
        assert_eq!(c.remove(&bob), Err(ContactError::NotFound));
    }

    #[test]
    fn sections() {
        let mut t = vec![0x7fu8, 1, 0, 9];
        assert!(set_section(&mut t, TLV_TOR_BRIDGES, b"snowflake x"));
        assert!(set_section(&mut t, TLV_FOLLOWS, b"[]"));
        assert_eq!(section(&t, TLV_TOR_BRIDGES), Some(&b"snowflake x"[..]));
        assert!(set_section(&mut t, TLV_TOR_BRIDGES, b"y"));
        assert_eq!((section(&t, TLV_TOR_BRIDGES), section(&t, 0x7f)), (Some(&b"y"[..]), Some(&[9u8][..])));
        assert!(set_section(&mut t, TLV_TOR_BRIDGES, b""));
        assert_eq!(section(&t, TLV_TOR_BRIDGES), None);
        assert_eq!(t[..4], [0x7f, 1, 0, 9], "other sections keep their order");
        assert!(!set_section(&mut t, TLV_FOLLOWS, &vec![0; 70_000]));
        let (_, kept) = Contacts::from_tlv(&Contacts::new().to_tlv(&t)).unwrap();
        assert_eq!(kept, t, "settings travel as other sections");
    }

    #[test]
    fn capacity() {
        let mut c = Contacts::new();
        for i in 0..MAX_CONTACTS {
            let mut id = [0u8; 32];
            id[..2].copy_from_slice(&(i as u16).to_le_bytes());
            c.save(PeerId(id), Some([1; 32]), true, &[b'x'; 32], 0).unwrap();
        }
        assert_eq!(c.save(PeerId([0xff; 32]), None, false, b"", 0), Err(ContactError::Full));
        let tlv = c.to_tlv(&[]);
        assert!(tlv.len() <= u16::MAX as usize + 3, "fits one TLV");
        assert_eq!(Contacts::from_tlv(&tlv).unwrap().0.list().len(), MAX_CONTACTS);
    }
}
