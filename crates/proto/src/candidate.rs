//! Packed ICE candidates (§8.4). Only UDP, component 1, `host` and `srflx` are ever carried.

use crate::buf::{Buf, Rd};
use core::fmt::Write;
use core::net::{Ipv4Addr, Ipv6Addr};
use core::str::FromStr;

#[repr(u8)]
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum Tag {
    HostV4 = 0,
    HostV6 = 1,
    HostMdns = 2,
    SrflxV4 = 3,
    SrflxV6 = 4,
}

impl Tag {
    #[inline]
    pub fn from_u8(v: u8) -> Option<Self> {
        Some(match v {
            0 => Self::HostV4,
            1 => Self::HostV6,
            2 => Self::HostMdns,
            3 => Self::SrflxV4,
            4 => Self::SrflxV6,
            _ => return None,
        })
    }

    #[inline(always)]
    pub const fn addr_len(self) -> usize {
        match self {
            Self::HostV4 | Self::SrflxV4 => 4,
            _ => 16,
        }
    }

    #[inline(always)]
    pub const fn is_srflx(self) -> bool {
        matches!(self, Self::SrflxV4 | Self::SrflxV6)
    }

    #[inline(always)]
    pub const fn is_v6(self) -> bool {
        matches!(self, Self::HostV6 | Self::SrflxV6)
    }

    #[inline(always)]
    pub const fn is_raw_host(self) -> bool {
        matches!(self, Self::HostV4 | Self::HostV6)
    }
}

/// One candidate: 7 B (IPv4) or 19 B (IPv6, mDNS UUID) on the wire.
#[repr(C)]
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct CandidateBin {
    pub tag: Tag,
    pub addr: [u8; 16],
    pub port: u16,
}

impl CandidateBin {
    pub const ZERO: Self = Self { tag: Tag::HostV4, addr: [0; 16], port: 0 };

    #[inline(always)]
    pub const fn wire_len(&self) -> usize {
        1 + self.tag.addr_len() + 2
    }

    pub fn encode(&self, b: &mut Buf<'_>) -> Result<(), ()> {
        b.u8(self.tag as u8)?;
        b.put(&self.addr[..self.tag.addr_len()])?;
        b.u16(self.port)
    }

    pub fn decode(r: &mut Rd<'_>) -> Option<Self> {
        let tag = Tag::from_u8(r.u8()?)?;
        let mut addr = [0u8; 16];
        addr[..tag.addr_len()].copy_from_slice(r.take(tag.addr_len())?);
        let port = r.u16()?;
        if port == 0 {
            return None;
        }
        Some(Self { tag, addr, port })
    }

    /// Parses the address/port/type of an SDP `a=candidate` line into a packed candidate.
    /// `typ` must be `host` or `srflx`; anything else (relay, prflx) is refused (§9.2).
    pub fn from_sdp_parts(addr: &str, port: &str, typ: &str) -> Option<Self> {
        let port = u16::from_str(port).ok().filter(|p| *p != 0)?;
        let srflx = match typ {
            "host" => false,
            "srflx" => true,
            _ => return None,
        };
        let mut out = [0u8; 16];
        let tag = if let Ok(v4) = Ipv4Addr::from_str(addr) {
            out[..4].copy_from_slice(&v4.octets());
            if srflx { Tag::SrflxV4 } else { Tag::HostV4 }
        } else if let Ok(v6) = Ipv6Addr::from_str(addr) {
            out = v6.octets();
            if srflx { Tag::SrflxV6 } else { Tag::HostV6 }
        } else if !srflx {
            out = parse_mdns(addr)?;
            Tag::HostMdns
        } else {
            return None;
        };
        Some(Self { tag, addr: out, port })
    }

    /// Renders `a=candidate:...` (without line ending) using the RFC 8445 priority formula.
    /// `index` makes the foundation and local preference unique.
    pub fn render_sdp(&self, index: usize, b: &mut Buf<'_>) -> Result<(), ()> {
        let (typ, pref): (&str, u32) = if self.tag.is_srflx() { ("srflx", 100) } else { ("host", 126) };
        let local_pref = 65535u32 - index as u32;
        let priority = (pref << 24) | (local_pref << 8) | (256 - 1);
        write!(b, "a=candidate:{typ}{index} 1 udp {priority} ").map_err(|_| ())?;
        self.render_addr(b)?;
        write!(b, " {} typ {typ}", self.port).map_err(|_| ())?;
        if self.tag.is_srflx() {
            write!(b, " raddr 0.0.0.0 rport 0").map_err(|_| ())?;
        }
        Ok(())
    }

    pub fn render_addr(&self, b: &mut Buf<'_>) -> Result<(), ()> {
        match self.tag {
            Tag::HostV4 | Tag::SrflxV4 => {
                let v4 = Ipv4Addr::new(self.addr[0], self.addr[1], self.addr[2], self.addr[3]);
                write!(b, "{v4}").map_err(|_| ())
            }
            Tag::HostV6 | Tag::SrflxV6 => write!(b, "{}", Ipv6Addr::from(self.addr)).map_err(|_| ()),
            Tag::HostMdns => {
                const HEX: &[u8; 16] = b"0123456789abcdef";
                let mut s = [0u8; 36];
                let mut o = 0;
                for (i, byte) in self.addr.iter().enumerate() {
                    if matches!(i, 4 | 6 | 8 | 10) {
                        s[o] = b'-';
                        o += 1;
                    }
                    s[o] = HEX[(byte >> 4) as usize];
                    s[o + 1] = HEX[(byte & 15) as usize];
                    o += 2;
                }
                b.put(&s)?;
                b.put(b".local")
            }
        }
    }
}

/// `xxxxxxxx-xxxx-xxxx-xxxx-xxxxxxxxxxxx.local` → 16 bytes.
fn parse_mdns(s: &str) -> Option<[u8; 16]> {
    let name = s.strip_suffix(".local")?.as_bytes();
    if name.len() != 36 {
        return None;
    }
    let mut out = [0u8; 16];
    let mut n = 0;
    let mut i = 0;
    while i < name.len() {
        if matches!(i, 8 | 13 | 18 | 23) {
            if name[i] != b'-' {
                return None;
            }
            i += 1;
            continue;
        }
        let hi = hex(name[i])?;
        let lo = hex(*name.get(i + 1)?)?;
        out[n] = hi << 4 | lo;
        n += 1;
        i += 2;
    }
    (n == 16).then_some(out)
}

#[inline(always)]
fn hex(c: u8) -> Option<u8> {
    match c {
        b'0'..=b'9' => Some(c - b'0'),
        b'a'..=b'f' => Some(c - b'a' + 10),
        b'A'..=b'F' => Some(c - b'A' + 10),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn render(c: &CandidateBin) -> ([u8; 128], usize) {
        let mut out = [0u8; 128];
        let mut b = Buf::new(&mut out);
        c.render_addr(&mut b).unwrap();
        let n = b.len();
        (out, n)
    }

    #[test]
    fn mdns_roundtrip() {
        let name = "9090b126-3aae-4a3e-b714-5d089ddfbff0.local";
        let c = CandidateBin::from_sdp_parts(name, "41731", "host").unwrap();
        assert_eq!(c.tag, Tag::HostMdns);
        let (out, n) = render(&c);
        assert_eq!(core::str::from_utf8(&out[..n]).unwrap(), name);
        assert_eq!(c.wire_len(), 19);
    }

    #[test]
    fn ipv4_and_v6() {
        let c = CandidateBin::from_sdp_parts("104.28.163.34", "48432", "srflx").unwrap();
        assert_eq!(c.tag, Tag::SrflxV4);
        assert_eq!(c.wire_len(), 7);
        let (out, n) = render(&c);
        assert_eq!(&out[..n], b"104.28.163.34");
        let c6 = CandidateBin::from_sdp_parts("2a09:bac1:6f60::3c3:2d", "5000", "srflx").unwrap();
        assert_eq!(c6.tag, Tag::SrflxV6);
    }

    #[test]
    fn refuses_relay_and_bad() {
        assert!(CandidateBin::from_sdp_parts("1.2.3.4", "1", "relay").is_none());
        assert!(CandidateBin::from_sdp_parts("1.2.3.4", "0", "host").is_none());
        assert!(CandidateBin::from_sdp_parts("x.local", "9", "host").is_none());
    }

    #[test]
    fn wire_roundtrip() {
        let c = CandidateBin::from_sdp_parts("192.0.2.2", "40422", "host").unwrap();
        let mut out = [0u8; 32];
        let mut b = Buf::new(&mut out);
        c.encode(&mut b).unwrap();
        let n = b.len();
        let d = CandidateBin::decode(&mut Rd::new(&out[..n])).unwrap();
        assert_eq!(c, d);
    }
}
