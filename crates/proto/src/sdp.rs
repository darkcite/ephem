//! SDP ↔ minimal fields (Appendix A). Proven in every browser pair by checkpoint S1.

use crate::buf::Buf;
use crate::candidate::CandidateBin;
use crate::code::{Cred, IceParams};
use core::fmt::Write;

/// Upper bound of a rendered remote SDP (template ≈ 450 B + 8 candidates × ≈ 110 B).
pub const MAX_SDP_LEN: usize = 2048;

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum Role {
    /// Rendering the peer's offer (`a=setup:actpass`).
    Offer,
    /// Rendering the peer's answer (`a=setup:active`).
    Answer,
}

/// Renders a remote description from the minimal fields. `session_id` comes from the invite id.
pub fn render_remote(ice: &IceParams, role: Role, session_id: u64, out: &mut [u8]) -> Result<usize, ()> {
    let mut b = Buf::new(out);
    let setup = match role {
        Role::Offer => "actpass",
        Role::Answer => "active",
    };
    write!(
        b,
        "v=0\r\no=- {session_id} 2 IN IP4 127.0.0.1\r\ns=-\r\nt=0 0\r\na=group:BUNDLE 0\r\n\
         a=extmap-allow-mixed\r\na=msid-semantic: WMS\r\n\
         m=application 9 UDP/DTLS/SCTP webrtc-datachannel\r\nc=IN IP4 0.0.0.0\r\n\
         a=ice-ufrag:{}\r\na=ice-pwd:{}\r\na=ice-options:trickle\r\na=fingerprint:sha-256 ",
        ice.ufrag.as_str(),
        ice.pwd.as_str()
    )
    .map_err(|_| ())?;
    const HEX: &[u8; 16] = b"0123456789ABCDEF";
    for (i, byte) in ice.fingerprint.iter().enumerate() {
        if i > 0 {
            b.u8(b':')?;
        }
        b.put(&[HEX[(byte >> 4) as usize], HEX[(byte & 15) as usize]])?;
    }
    write!(b, "\r\na=setup:{setup}\r\na=mid:0\r\na=sctp-port:5000\r\na=max-message-size:262144\r\n").map_err(|_| ())?;
    for (i, c) in ice.candidates().iter().enumerate() {
        c.render_sdp(i, &mut b)?;
        b.put(b"\r\n")?;
    }
    b.put(b"a=end-of-candidates\r\n")?;
    Ok(b.len())
}

/// Extracts ufrag, pwd, SHA-256 fingerprint and every UDP component-1 host/srflx candidate
/// from a local description. `keep` decides which candidates enter the code (privacy mode, §9.4).
pub fn parse_local(sdp: &str, keep: impl Fn(&CandidateBin) -> bool) -> Option<IceParams> {
    let mut ice = IceParams::EMPTY;
    let mut have_fp = false;
    for line in sdp.split('\n') {
        let line = line.trim_end_matches('\r');
        if let Some(v) = line.strip_prefix("a=ice-ufrag:") {
            ice.ufrag = Cred::new(v.as_bytes())?;
        } else if let Some(v) = line.strip_prefix("a=ice-pwd:") {
            ice.pwd = Cred::new(v.as_bytes())?;
        } else if let Some(v) = line.strip_prefix("a=fingerprint:") {
            let (alg, hex) = v.split_once(' ')?;
            if alg.eq_ignore_ascii_case("sha-256") {
                ice.fingerprint = parse_fp(hex)?;
                have_fp = true;
            }
        } else if let Some(v) = line.strip_prefix("a=candidate:")
            && let Some(c) = parse_candidate(v)
            && keep(&c)
        {
            ice.push(c);
        }
    }
    (have_fp && (4..=32).contains(&ice.ufrag.len) && (22..=32).contains(&ice.pwd.len)).then_some(ice)
}

/// `<foundation> <component> <transport> <priority> <addr> <port> typ <type> ...`
fn parse_candidate(v: &str) -> Option<CandidateBin> {
    let mut it = v.split(' ');
    let _foundation = it.next()?;
    if it.next()? != "1" {
        return None;
    }
    if !it.next()?.eq_ignore_ascii_case("udp") {
        return None;
    }
    let _priority = it.next()?;
    let addr = it.next()?;
    let port = it.next()?;
    if it.next()? != "typ" {
        return None;
    }
    CandidateBin::from_sdp_parts(addr, port, it.next()?)
}

fn parse_fp(hex: &str) -> Option<[u8; 32]> {
    let mut out = [0u8; 32];
    let mut n = 0;
    for part in hex.trim().split(':') {
        let b = part.as_bytes();
        if b.len() != 2 || n >= 32 {
            return None;
        }
        out[n] = u8::from_str_radix(part, 16).ok()?;
        n += 1;
    }
    (n == 32).then_some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    // Real Chromium offer captured by checkpoint S1 (docs/P2P-CHAT.md §24.2).
    const CHROME_OFFER: &str = "v=0\r\no=- 8714843556912643240 2 IN IP4 127.0.0.1\r\ns=-\r\nt=0 0\r\na=group:BUNDLE 0\r\na=extmap-allow-mixed\r\na=msid-semantic: WMS\r\nm=application 41731 UDP/DTLS/SCTP webrtc-datachannel\r\nc=IN IP4 192.0.2.2\r\na=candidate:2075442524 1 udp 2122260223 192.0.2.2 41731 typ host generation 0 network-id 1\r\na=candidate:91971524 1 tcp 1518280447 192.0.2.2 9 typ host tcptype active generation 0 network-id 1\r\na=candidate:1 1 udp 2122260223 9090b126-3aae-4a3e-b714-5d089ddfbff0.local 50000 typ host\r\na=ice-ufrag:A/uL\r\na=ice-pwd:55bZhqbovQp1LFLkFt9yUC/k\r\na=ice-options:trickle\r\na=fingerprint:sha-256 61:30:1F:D5:F2:26:33:A5:6B:99:A4:95:B7:68:5A:AB:D6:59:04:2A:3B:B9:31:74:71:7E:96:05:42:4D:27:8A\r\na=setup:actpass\r\na=mid:0\r\na=sctp-port:5000\r\na=max-message-size:262144\r\n";

    #[test]
    fn parse_real_offer_and_filter() {
        let all = parse_local(CHROME_OFFER, |_| true).unwrap();
        assert_eq!(all.ufrag.as_str(), "A/uL");
        assert_eq!(all.pwd.len, 24);
        assert_eq!(all.fingerprint[0], 0x61);
        assert_eq!(all.n_cand, 2, "TCP candidate skipped");
        let no_raw = parse_local(CHROME_OFFER, |c| !c.tag.is_raw_host()).unwrap();
        assert_eq!(no_raw.n_cand, 1, "raw host IP filtered (S4)");
    }

    #[test]
    fn render_then_reparse() {
        let ice = parse_local(CHROME_OFFER, |_| true).unwrap();
        let mut out = [0u8; MAX_SDP_LEN];
        let n = render_remote(&ice, Role::Offer, 42, &mut out).unwrap();
        let sdp = core::str::from_utf8(&out[..n]).unwrap();
        assert!(sdp.contains("a=setup:actpass\r\n"));
        assert!(sdp.contains("a=fingerprint:sha-256 61:30:1F:"));
        assert!(sdp.contains("9090b126-3aae-4a3e-b714-5d089ddfbff0.local 50000 typ host"));
        let back = parse_local(sdp, |_| true).unwrap();
        assert_eq!(back, ice);
    }
}
