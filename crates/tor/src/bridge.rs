//! Tor bridge lines (docs/P2P-CHAT.md Appendix F.2): the Tor Browser / torrc format, parsed into
//! the Snowflake setup a browser page can run.
//!
//! A page has no raw TCP or UDP, so only `snowflake` lines are usable; every other line is
//! rejected with its reason ([`Problem`]), never dropped silently. Options a browser cannot honour
//! (domain fronting, uTLS) are reported as notes and ignored.
//!
//! Several lines merge: brokers, bridges and STUN servers are each the union, in order of first
//! appearance (a Snowflake broker matches a proxy to any of the listed bridges). Runs once, when
//! Tor starts or the setting is checked; the returned strings are its only allocations.

use crate::net::BRIDGE_ADDRS;

/// Why a line is not used, or what in it is ignored.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum Problem {
    // Errors: the line is not used.
    PlainBridge,
    Obfs4,
    Meek,
    WebTunnel,
    OtherTransport,
    NoFingerprint,
    BadFingerprint,
    FingerprintMismatch,
    NoBroker,
    BadBroker,
    AmpCacheOnly,
    NoStun,
    // Notes: the line is used, this option is not.
    FrontIgnored,
    UtlsIgnored,
    AmpCacheIgnored,
    TurnIgnored,
    BadStunIgnored,
    UnknownOption,
    TooManyBridges,
}

impl Problem {
    pub fn is_error(self) -> bool {
        (self as u8) <= (Problem::NoStun as u8)
    }

    pub fn reason(self) -> &'static str {
        match self {
            Problem::PlainBridge => "a plain bridge (IP:port) needs a direct TCP connection, which a web page cannot open",
            Problem::Obfs4 => "obfs4 needs a direct TCP connection, which a web page cannot open",
            Problem::Meek => "meek needs domain fronting, which a web page cannot do",
            Problem::WebTunnel => "WebTunnel is not usable from a web page (the browser adds WebSocket framing the bridge does not expect)",
            Problem::OtherTransport => "this transport is not usable from a web page; only snowflake bridges are",
            Problem::NoFingerprint => "no bridge fingerprint",
            Problem::BadFingerprint => "the fingerprint must be 40 hexadecimal characters",
            Problem::FingerprintMismatch => "the line has two different fingerprints",
            Problem::NoBroker => "no broker: the line needs url=https://…",
            Problem::BadBroker => "the broker (url=) must be an https:// address",
            Problem::AmpCacheOnly => "AMP cache rendezvous is not supported yet; add a url= broker",
            Problem::NoStun => "no usable STUN server (ice=stun:…)",
            Problem::FrontIgnored => "domain fronting (front=/fronts=) is ignored: browsers cannot do it; the url= address is used as is",
            Problem::UtlsIgnored => "utls options are ignored: the browser's own TLS is used",
            Problem::AmpCacheIgnored => "ampcache= is ignored (not supported yet)",
            Problem::TurnIgnored => "TURN servers are ignored: Ephem never relays through TURN",
            Problem::BadStunIgnored => "an ice= entry that is not stun:host:port is ignored",
            Problem::UnknownOption => "an unknown option is ignored",
            Problem::TooManyBridges => "more than 4 different bridges: the extra ones are ignored",
        }
    }
}

/// What a set of lines configures, and what was wrong with them.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Bridges {
    pub brokers: Vec<String>,
    pub fingerprints: Vec<String>,
    pub ice: Vec<String>,
    /// `(line number from 1, problem)`, in order.
    pub problems: Vec<(u32, Problem)>,
}

impl Bridges {
    /// Usable: at least one bridge, one broker and one STUN server.
    pub fn usable(&self) -> bool {
        !self.fingerprints.is_empty() && !self.brokers.is_empty() && !self.ice.is_empty()
    }

    /// The lines that were used, with no error (for a status line).
    pub fn errors(&self) -> usize {
        self.problems.iter().filter(|(_, p)| p.is_error()).count()
    }
}

fn push_unique(v: &mut Vec<String>, s: &str) {
    if !v.iter().any(|x| x.eq_ignore_ascii_case(s)) {
        v.push(s.to_owned());
    }
}

fn fingerprint(s: &str) -> Option<&str> {
    (s.len() == 40 && s.bytes().all(|b| b.is_ascii_hexdigit())).then_some(s)
}

/// `https://…`, or `http://` to the loopback address (the offline lab's broker only).
fn broker(url: &str) -> bool {
    let host = |rest: &str| rest.split(['/', '?', '#']).next().unwrap_or("").to_owned();
    if let Some(rest) = url.strip_prefix("https://") {
        let h = host(rest);
        return !h.is_empty() && !h.contains('@') && !h.contains(char::is_whitespace);
    }
    url.strip_prefix("http://").map(host).is_some_and(|h| h.starts_with("127.0.0.1:") || h.starts_with("localhost:") || h == "127.0.0.1" || h == "localhost")
}

/// `stun:host:port` (the form the Snowflake client takes).
fn stun(s: &str) -> bool {
    s.strip_prefix("stun:").and_then(|r| r.rsplit_once(':')).is_some_and(|(h, p)| !h.is_empty() && !h.contains('/') && p.parse::<u16>().is_ok_and(|p| p > 0))
}

/// Parses bridge lines (one per line; `Bridge ` prefixes, blank lines and `#` comments allowed).
pub fn parse(text: &str) -> Bridges {
    let mut out = Bridges::default();
    for (n, raw) in text.lines().enumerate() {
        let n = n as u32 + 1;
        let line = raw.trim();
        let line = line.strip_prefix("Bridge ").or_else(|| line.strip_prefix("bridge ")).unwrap_or(line).trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if let Err(p) = parse_line(line, n, &mut out) {
            out.problems.push((n, p));
        }
    }
    out
}

fn parse_line(line: &str, n: u32, out: &mut Bridges) -> Result<(), Problem> {
    let mut words = line.split_ascii_whitespace();
    let transport = words.next().unwrap_or("");
    if transport.contains(':') || transport.contains('.') {
        return Err(Problem::PlainBridge);
    }
    match transport.to_ascii_lowercase().as_str() {
        "snowflake" => {}
        "obfs4" | "obfs3" | "scramblesuit" => return Err(Problem::Obfs4),
        "meek" | "meek_lite" => return Err(Problem::Meek),
        "webtunnel" => return Err(Problem::WebTunnel),
        _ => return Err(Problem::OtherTransport),
    }
    let mut fp: Option<&str> = None;
    let (mut url, mut amp) = (None, false);
    let mut notes = [false; 3]; // front, utls, unknown option
    let (mut stuns, mut turn, mut bad_stun) = (Vec::new(), false, false);
    for w in words {
        let Some((k, v)) = w.split_once('=') else {
            // The placeholder address, then the fingerprint (both optional in torrc).
            if w.contains(':') {
                continue;
            }
            let f = fingerprint(w).ok_or(Problem::BadFingerprint)?;
            if fp.is_some_and(|x| !x.eq_ignore_ascii_case(f)) {
                return Err(Problem::FingerprintMismatch);
            }
            fp = Some(f);
            continue;
        };
        match k {
            "fingerprint" => {
                let f = fingerprint(v).ok_or(Problem::BadFingerprint)?;
                if fp.is_some_and(|x| !x.eq_ignore_ascii_case(f)) {
                    return Err(Problem::FingerprintMismatch);
                }
                fp = Some(f);
            }
            "url" => url = Some(v),
            "ampcache" => amp = true,
            "front" | "fronts" => notes[0] = true,
            "utls-imitate" | "utls-nosni" | "utls" => notes[1] = true,
            "ice" => {
                for s in v.split(',').filter(|s| !s.is_empty()) {
                    if s.starts_with("turn:") || s.starts_with("turns:") {
                        turn = true;
                    } else if stun(s) {
                        stuns.push(s);
                    } else {
                        bad_stun = true;
                    }
                }
            }
            _ => notes[2] = true,
        }
    }
    let fp = fp.ok_or(Problem::NoFingerprint)?;
    let url = match url {
        Some(u) if broker(u) => u,
        Some(_) => return Err(Problem::BadBroker),
        None if amp => return Err(Problem::AmpCacheOnly),
        None => return Err(Problem::NoBroker),
    };
    if stuns.is_empty() {
        return Err(Problem::NoStun);
    }
    let fp_upper = fp.to_ascii_uppercase();
    if !out.fingerprints.contains(&fp_upper) {
        if out.fingerprints.len() < BRIDGE_ADDRS.len() {
            out.fingerprints.push(fp_upper);
        } else {
            out.problems.push((n, Problem::TooManyBridges));
        }
    }
    push_unique(&mut out.brokers, url);
    for s in stuns {
        push_unique(&mut out.ice, s);
    }
    let flagged = [(notes[0], Problem::FrontIgnored), (notes[1], Problem::UtlsIgnored), (amp, Problem::AmpCacheIgnored), (turn, Problem::TurnIgnored), (bad_stun, Problem::BadStunIgnored), (notes[2], Problem::UnknownOption)];
    for (on, p) in flagged {
        if on {
            out.problems.push((n, p));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    // As Tor Browser ships them (13.5+), one per line.
    const TOR_BROWSER: &str = "snowflake 192.0.2.3:80 2B280B23E1107BB62ABFC40DDCC8824814F80A72 fingerprint=2B280B23E1107BB62ABFC40DDCC8824814F80A72 url=https://1098762253.rsc.cdn77.org/ fronts=www.cdn77.com,www.phpmyadmin.net ice=stun:stun.antisip.com:3478,stun:stun.epygi.com:3478,stun:stun.uls.co.za:3478,stun:stun.voipgate.com:3478,stun:stun.mixvoip.com:3478,stun:stun.nextcloud.com:3478,stun:stun.bethesda.net:3478,stun:stun.nextcloud.com:443 utls-imitate=hellorandomizedalpn
snowflake 192.0.2.4:80 8838024498816A039FCBBAB14E6F40A0843051FA fingerprint=8838024498816A039FCBBAB14E6F40A0843051FA url=https://1098762253.rsc.cdn77.org/ fronts=www.cdn77.com,www.phpmyadmin.net ice=stun:stun.antisip.com:3478,stun:stun.epygi.com:3478 utls-imitate=hellorandomizedalpn";

    #[test]
    fn tor_browser_lines() {
        let b = parse(TOR_BROWSER);
        assert!(b.usable());
        assert_eq!(b.fingerprints, ["2B280B23E1107BB62ABFC40DDCC8824814F80A72", "8838024498816A039FCBBAB14E6F40A0843051FA"]);
        assert_eq!(b.brokers, ["https://1098762253.rsc.cdn77.org/"]);
        assert_eq!(b.ice.len(), 8, "the union, without duplicates");
        assert_eq!(b.errors(), 0);
        assert!(b.problems.contains(&(1, Problem::FrontIgnored)) && b.problems.contains(&(2, Problem::UtlsIgnored)));
    }

    #[test]
    fn other_transports_rejected_with_reason() {
        let text = "Bridge obfs4 198.51.100.1:443 0123456789ABCDEF0123456789ABCDEF01234567 cert=x iat-mode=0
webtunnel [2001:db8::1]:443 0123456789ABCDEF0123456789ABCDEF01234567 url=https://example.com/x ver=0.0.1
meek_lite 192.0.2.20:80 url=https://meek.example/ front=ajax.example
198.51.100.7:9001 0123456789ABCDEF0123456789ABCDEF01234567
conjure 143.110.214.222:80 url=https://registration.refraction.network/api
# a comment

snowflake 192.0.2.3:80 0123456789ABCDEF0123456789ABCDEF01234567 ampcache=https://cdn.ampproject.org/ ice=stun:s.example:3478
snowflake 192.0.2.3:80 0123 url=https://b.example/ ice=stun:s.example:3478
snowflake 192.0.2.3:80 url=https://b.example/ ice=stun:s.example:3478
snowflake 192.0.2.3:80 0123456789ABCDEF0123456789ABCDEF01234567 url=http://b.example/ ice=stun:s.example:3478
snowflake 192.0.2.3:80 0123456789ABCDEF0123456789ABCDEF01234567 url=https://b.example/ ice=turn:t.example:3478
snowflake 192.0.2.3:80 0123456789ABCDEF0123456789ABCDEF01234567 fingerprint=1123456789ABCDEF0123456789ABCDEF01234567 url=https://b.example/ ice=stun:s.example:3478";
        let b = parse(text);
        assert!(!b.usable());
        let want = [
            (1, Problem::Obfs4),
            (2, Problem::WebTunnel),
            (3, Problem::Meek),
            (4, Problem::PlainBridge),
            (5, Problem::OtherTransport),
            (8, Problem::AmpCacheOnly),
            (9, Problem::BadFingerprint),
            (10, Problem::NoFingerprint),
            (11, Problem::BadBroker),
            (12, Problem::NoStun),
            (13, Problem::FingerprintMismatch),
        ];
        assert_eq!(b.problems, want);
        assert!(want.iter().all(|(_, p)| p.is_error() && !p.reason().is_empty()));
    }

    #[test]
    fn lab_and_limits() {
        // The offline lab: http to loopback is accepted, http elsewhere is not (above).
        let fp = "68B505B9955C1E9A1AC28A0B2BD851050222A3F5";
        let b = parse(&format!("snowflake 192.0.2.3:80 {fp} url=http://127.0.0.1:59999/ ice=stun:127.0.0.1:3478\nsnowflake 192.0.2.3:80 {fp} url=http://127.0.0.1:18080/ ice=stun:127.0.0.1:3478,turn:x:1,bogus"));
        assert!(b.usable());
        assert_eq!(b.fingerprints, [fp]);
        assert_eq!(b.brokers, ["http://127.0.0.1:59999/", "http://127.0.0.1:18080/"], "brokers tried in order");
        assert_eq!(b.problems, [(2, Problem::TurnIgnored), (2, Problem::BadStunIgnored)]);
        // At most BRIDGE_ADDRS bridges.
        let many: String = (0..6).map(|i| format!("snowflake {i:040X} url=https://b.example/ ice=stun:s:1\n")).collect();
        let b = parse(&many);
        assert_eq!(b.fingerprints.len(), BRIDGE_ADDRS.len());
        assert_eq!(b.problems.iter().filter(|(_, p)| *p == Problem::TooManyBridges).count(), 6 - BRIDGE_ADDRS.len());
        assert!(!Problem::TooManyBridges.is_error());
    }

    #[test]
    fn never_panics() {
        for s in ["", "snowflake", "snowflake =", "snowflake url=", "snowflake ice=,,,", "Bridge ", "\u{0}\u{ff}", "snowflake 192.0.2.3:80 fingerprint= url=https:// ice=stun::"] {
            let _ = parse(s);
        }
        assert!(!broker("https://") && !broker("https://u@h/") && broker("https://h/") && broker("http://localhost:1/"));
        assert!(stun("stun:h:1") && !stun("stun:h") && !stun("stun::1") && !stun("stun:h:0"));
    }
}
