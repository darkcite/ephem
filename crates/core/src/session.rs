//! One pairwise link (§12): rendezvous codes → Noise KK → records.
//!
//! All buffers are inline and fixed (§22): the session is allocated once when the link starts
//! (setup path); the connected path never allocates. The only setup-time heap use is inside
//! snow's handshake state.

use ephem_crypto::noise::{HS_MSG_LEN, Handshake, Transport};
use ephem_crypto::sas::Sas;
use ephem_crypto::{Identity, PeerId};
use ephem_proto::ErrorCode;
use ephem_proto::buf::{Buf, Rd};
use ephem_proto::candidate::CandidateBin;
use ephem_proto::code::{Code, IceParams, Kind, MAX_CODE_LEN, flags};
use ephem_proto::frame::{self, FrameType, HEADER_LEN, Header, MAX_FRAME, MAX_TEXT, Records, TAG_LEN, rflags, rtype};
use ephem_proto::sdp::{self, MAX_SDP_LEN};

/// Idle interval after which a PING is sent (§12).
pub const PING_IDLE_MS: u64 = 15_000;
/// Silence after which the link is DEGRADED (§12, E8: 120 s, not 30 s).
pub const DEGRADED_AFTER_MS: u64 = 120_000;
/// Rekey period (§10.1): 10 minutes or 2^20 frames, whichever comes first.
pub const REKEY_MS: u64 = 600_000;
pub const REKEY_FRAMES: u64 = 1 << 20;
/// Answerer-side expiry tolerance for clock skew (§8.6).
pub const SKEW_S: u32 = 120;
/// HELLO capability bits (§11.4) implemented by this build.
pub const CAPS: u32 = 0;
/// Largest plaintext that fits one frame.
pub const MAX_PLAIN: usize = MAX_FRAME - HEADER_LEN - TAG_LEN;

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum Role {
    Offerer,
    Answerer,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum State {
    /// Offerer: waiting for the local offer to be gathered.
    Gathering,
    /// Offerer: invite shown, waiting for the answer.
    AwaitingAnswer,
    /// Both codes known, WebRTC connecting / Noise handshaking.
    Connecting,
    Connected,
    Closed,
}

/// Candidate privacy mode (§9.4).
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum Privacy {
    LanOnly,
    Default,
    MaxConnectivity,
}

impl Privacy {
    pub fn from_u8(v: u8) -> Self {
        match v {
            0 => Self::LanOnly,
            2 => Self::MaxConnectivity,
            _ => Self::Default,
        }
    }

    #[inline]
    pub fn keeps(self, c: &CandidateBin, drop_ipv6: bool) -> bool {
        if drop_ipv6 && c.tag.is_v6() {
            return false;
        }
        match self {
            Self::LanOnly => !c.tag.is_srflx() && !c.tag.is_raw_host(),
            Self::Default => !c.tag.is_raw_host(),
            Self::MaxConnectivity => true,
        }
    }
}

/// Output of the core. Slices borrow either the session's TX buffer or the caller's RX frame.
#[derive(Debug, PartialEq, Eq)]
pub enum Event<'a> {
    /// Transmit this frame on the DataChannel.
    Send(&'a [u8]),
    /// Noise handshake done: show the SAS (§10.4).
    Connected { sas: Sas, peer: PeerId },
    /// Peer HELLO received.
    Hello { sign_pk: [u8; 32], nick: &'a [u8] },
    /// Incoming chat message (UTF-8 validated).
    Chat { seq: u64, text: &'a [u8] },
    /// Our messages up to `seq` were delivered (cumulative ACK).
    Delivered { seq: u64 },
    /// No traffic for `DEGRADED_AFTER_MS` (§12).
    Degraded,
    /// Traffic resumed after `Degraded`.
    Alive,
    /// Peer reports its tab is hidden / visible (PING flag, §12).
    PeerHidden(bool),
    /// Link closed: by the peer (GOODBYE), by a protocol error, or by expiry.
    Closed(ErrorCode),
}

pub struct Session {
    role: Role,
    state: State,
    privacy: Privacy,
    drop_ipv6: bool,
    local: PeerId,
    sign_pk: [u8; 32],
    remote: PeerId,
    invite_id: [u8; 16],
    room_id: [u8; 16],
    expires_at: u32,
    code_flags: u8,
    /// Remote ICE parameters, rendered into SDP on demand.
    remote_ice: IceParams,
    invite: [u8; MAX_CODE_LEN],
    invite_len: u16,
    answer: [u8; MAX_CODE_LEN],
    answer_len: u16,
    hs: Option<Handshake>,
    tr: Option<Transport>,
    tx: [u8; MAX_FRAME],
    chat_tx: u64,
    chat_rx: u64,
    last_rx_ms: u64,
    last_tx_ms: u64,
    rekey_at_ms: u64,
    degraded: bool,
    hidden: bool,
}

#[inline]
fn session_id(invite_id: &[u8; 16]) -> u64 {
    // SDP `o=` session id must fit in 63 bits.
    u64::from_le_bytes([invite_id[0], invite_id[1], invite_id[2], invite_id[3], invite_id[4], invite_id[5], invite_id[6], invite_id[7]]) >> 1
}

impl Session {
    fn blank(role: Role, id: &Identity, privacy: Privacy, drop_ipv6: bool) -> Self {
        Self {
            role,
            state: State::Gathering,
            privacy,
            drop_ipv6,
            local: id.peer_id(),
            sign_pk: id.sign_pk(),
            remote: PeerId([0; 32]),
            invite_id: [0; 16],
            room_id: [0; 16],
            expires_at: 0,
            code_flags: 0,
            remote_ice: IceParams::EMPTY,
            invite: [0; MAX_CODE_LEN],
            invite_len: 0,
            answer: [0; MAX_CODE_LEN],
            answer_len: 0,
            hs: None,
            tr: None,
            tx: [0; MAX_FRAME],
            chat_tx: 0,
            chat_rx: 0,
            last_rx_ms: 0,
            last_tx_ms: 0,
            rekey_at_ms: 0,
            degraded: false,
            hidden: false,
        }
    }

    /// Alice: a new invite. `invite_id` and `room_id` come from the CSPRNG.
    pub fn offerer(id: &Identity, invite_id: [u8; 16], room_id: [u8; 16], expires_at: u32, privacy: Privacy, drop_ipv6: bool) -> Self {
        let mut s = Self::blank(Role::Offerer, id, privacy, drop_ipv6);
        s.invite_id = invite_id;
        s.room_id = room_id;
        s.expires_at = expires_at;
        s.code_flags = if privacy == Privacy::LanOnly { flags::LAN_ONLY } else { 0 };
        s
    }

    /// Bob: parses Alice's invite. Then render the offer with [`Self::remote_sdp`].
    pub fn answerer(id: &Identity, invite: &[u8], now_s: u32, privacy: Privacy, drop_ipv6: bool) -> Result<Self, ErrorCode> {
        let c = Code::decode(invite)?;
        if c.kind != Kind::Invite {
            return Err(ErrorCode::InvalidInvite);
        }
        if now_s > c.expires_at.saturating_add(SKEW_S) {
            return Err(ErrorCode::ExpiredInvite);
        }
        if c.static_pk == id.peer_id().0 {
            return Err(ErrorCode::InvalidInvite);
        }
        // Bob follows Alice's LAN-only request; otherwise his own mode.
        let privacy = if c.flags & flags::LAN_ONLY != 0 { Privacy::LanOnly } else { privacy };
        let mut s = Self::blank(Role::Answerer, id, privacy, drop_ipv6);
        s.remote = PeerId(c.static_pk);
        s.invite_id = c.invite_id;
        s.room_id = c.room_id;
        s.expires_at = c.expires_at;
        s.code_flags = c.flags;
        s.remote_ice = c.ice;
        s.invite[..invite.len()].copy_from_slice(invite);
        s.invite_len = invite.len() as u16;
        Ok(s)
    }

    #[inline(always)]
    pub fn state(&self) -> State {
        self.state
    }

    #[inline(always)]
    pub fn role(&self) -> Role {
        self.role
    }

    #[inline(always)]
    pub fn privacy(&self) -> Privacy {
        self.privacy
    }

    #[inline(always)]
    pub fn remote(&self) -> PeerId {
        self.remote
    }

    #[inline(always)]
    pub fn expires_at(&self) -> u32 {
        self.expires_at
    }

    /// Renders the remote description (offer for Bob, answer for Alice) into `out` (Appendix A).
    pub fn remote_sdp<'o>(&self, out: &'o mut [u8; MAX_SDP_LEN]) -> Result<&'o str, ErrorCode> {
        let role = match self.role {
            Role::Answerer => sdp::Role::Offer,
            Role::Offerer => sdp::Role::Answer,
        };
        let n = sdp::render_remote(&self.remote_ice, role, session_id(&self.invite_id), out).map_err(|_| ErrorCode::InvalidInvite)?;
        core::str::from_utf8(&out[..n]).map_err(|_| ErrorCode::InvalidInvite)
    }

    /// Builds this side's code from the gathered local description.
    /// Alice → INVITE (then waits for the answer). Bob → ANSWER (then the handshake is armed).
    pub fn build_code(&mut self, id: &Identity, local_sdp: &str) -> Result<&[u8], ErrorCode> {
        if self.state != State::Gathering {
            return Err(ErrorCode::NotPermitted);
        }
        let (privacy, drop_ipv6) = (self.privacy, self.drop_ipv6);
        let ice = sdp::parse_local(local_sdp, |c| privacy.keeps(c, drop_ipv6)).ok_or(ErrorCode::IceFailed)?;
        let kind = match self.role {
            Role::Offerer => Kind::Invite,
            Role::Answerer => Kind::Answer,
        };
        let code = Code {
            kind,
            flags: if kind == Kind::Invite { self.code_flags } else { 0 },
            invite_id: self.invite_id,
            room_id: if kind == Kind::Invite { self.room_id } else { [0; 16] },
            static_pk: self.local.0,
            expires_at: if kind == Kind::Invite { self.expires_at } else { 0 },
            ice,
        };
        match self.role {
            Role::Offerer => {
                let n = code.encode(&mut self.invite).map_err(|_| ErrorCode::InvalidInvite)?;
                self.invite_len = n as u16;
                self.state = State::AwaitingAnswer;
                Ok(&self.invite[..n])
            }
            Role::Answerer => {
                let n = code.encode(&mut self.answer).map_err(|_| ErrorCode::InvalidInvite)?;
                self.answer_len = n as u16;
                self.arm_handshake(id)?;
                Ok(&self.answer[..n])
            }
        }
    }

    /// Alice: applies Bob's answer. Then render it with [`Self::remote_sdp`].
    pub fn apply_answer(&mut self, id: &Identity, answer: &[u8], now_s: u32) -> Result<(), ErrorCode> {
        if self.role != Role::Offerer {
            return Err(ErrorCode::NotPermitted);
        }
        let c = Code::decode(answer)?;
        if c.kind != Kind::Answer || c.invite_id != self.invite_id {
            return Err(ErrorCode::AnswerMismatch);
        }
        match self.state {
            State::AwaitingAnswer => {}
            State::Gathering => return Err(ErrorCode::NotPermitted),
            _ => return Err(ErrorCode::InviteConsumed),
        }
        if now_s > self.expires_at {
            self.state = State::Closed;
            return Err(ErrorCode::ExpiredInvite);
        }
        if c.static_pk == self.local.0 {
            return Err(ErrorCode::AnswerMismatch);
        }
        self.remote = PeerId(c.static_pk);
        self.remote_ice = c.ice;
        self.answer[..answer.len()].copy_from_slice(answer);
        self.answer_len = answer.len() as u16;
        self.arm_handshake(id)
    }

    fn arm_handshake(&mut self, id: &Identity) -> Result<(), ErrorCode> {
        let hs = Handshake::new(
            id,
            &self.remote,
            self.role == Role::Offerer,
            &self.invite[..self.invite_len as usize],
            &self.answer[..self.answer_len as usize],
        )?;
        self.hs = Some(hs);
        self.state = State::Connecting;
        Ok(())
    }

    /// DataChannel open. The initiator (offerer) sends Noise message 1.
    pub fn on_open(&mut self, now_ms: u64, sink: &mut impl FnMut(Event<'_>)) {
        if self.state != State::Connecting || self.role != Role::Offerer {
            return;
        }
        if let Err(e) = self.send_handshake(0, now_ms, sink) {
            self.fail(e, sink);
        }
    }

    fn send_handshake(&mut self, seq: u64, now_ms: u64, sink: &mut impl FnMut(Event<'_>)) -> Result<(), ErrorCode> {
        let hs = self.hs.as_mut().ok_or(ErrorCode::CryptoFailed)?;
        Header { ftype: FrameType::Handshake, flags: 0, seq }.write(&mut self.tx).map_err(|_| ErrorCode::CryptoFailed)?;
        let n = hs.write(&mut self.tx[HEADER_LEN..HEADER_LEN + HS_MSG_LEN])?;
        self.last_tx_ms = now_ms;
        sink(Event::Send(&self.tx[..HEADER_LEN + n]));
        Ok(())
    }

    /// A frame arrived on the DataChannel. Decrypted in place inside `frame` (§11.6).
    pub fn on_frame(&mut self, now_ms: u64, frame: &mut [u8], sink: &mut impl FnMut(Event<'_>)) {
        if self.state == State::Closed {
            return;
        }
        if let Err(e) = self.on_frame_inner(now_ms, frame, sink) {
            self.fail(e, sink);
        }
    }

    fn on_frame_inner(&mut self, now_ms: u64, frame: &mut [u8], sink: &mut impl FnMut(Event<'_>)) -> Result<(), ErrorCode> {
        if frame.len() > MAX_FRAME {
            return Err(ErrorCode::MessageTooLarge);
        }
        let h = Header::read(frame)?;
        self.last_rx_ms = now_ms;
        if self.degraded {
            self.degraded = false;
            sink(Event::Alive);
        }
        match h.ftype {
            FrameType::Handshake => self.on_handshake(now_ms, &frame[HEADER_LEN..], sink),
            FrameType::Transport => self.on_transport(now_ms, frame, sink),
        }
    }

    fn on_handshake(&mut self, now_ms: u64, msg: &[u8], sink: &mut impl FnMut(Event<'_>)) -> Result<(), ErrorCode> {
        if self.state != State::Connecting {
            return Err(ErrorCode::ProtocolMismatch);
        }
        let hs = self.hs.as_mut().ok_or(ErrorCode::ProtocolMismatch)?;
        if hs.is_my_turn() {
            return Err(ErrorCode::ProtocolMismatch);
        }
        hs.read(msg)?;
        if !hs.is_finished() {
            // Responder: reply with message 2, which finishes KK.
            self.send_handshake(1, now_ms, sink)?;
        }
        let (tr, sas) = self.hs.take().ok_or(ErrorCode::CryptoFailed)?.finish()?;
        self.tr = Some(tr);
        self.state = State::Connected;
        self.rekey_at_ms = now_ms + REKEY_MS;
        sink(Event::Connected { sas, peer: self.remote });
        self.send_hello(now_ms, sink)
    }

    fn on_transport(&mut self, now_ms: u64, frame: &mut [u8], sink: &mut impl FnMut(Event<'_>)) -> Result<(), ErrorCode> {
        let range = self.tr.as_mut().ok_or(ErrorCode::ProtocolMismatch)?.open(frame)?;
        let plain = &frame[range];
        let mut ack_upto = None;
        let mut pong = None;
        let mut rekey = false;
        for rec in Records::new(plain) {
            let rec = rec?;
            let mut r = Rd::new(rec.body);
            match rec.rtype {
                rtype::HELLO => {
                    let (vmin, vmax) = (r.u8().ok_or(ErrorCode::ProtocolMismatch)?, r.u8().ok_or(ErrorCode::ProtocolMismatch)?);
                    if !(vmin..=vmax).contains(&ephem_proto::VERSION) {
                        return Err(ErrorCode::ProtocolMismatch);
                    }
                    let _caps = r.u32().ok_or(ErrorCode::ProtocolMismatch)?;
                    let _max_msg = r.u16().ok_or(ErrorCode::ProtocolMismatch)?;
                    let sign_pk = r.arr::<32>().ok_or(ErrorCode::ProtocolMismatch)?;
                    let nl = r.u8().ok_or(ErrorCode::ProtocolMismatch)? as usize;
                    let nick = r.take(nl).filter(|n| n.len() <= 32 && core::str::from_utf8(n).is_ok()).ok_or(ErrorCode::ProtocolMismatch)?;
                    sink(Event::Hello { sign_pk, nick });
                }
                rtype::CHAT => {
                    let seq = r.u64().ok_or(ErrorCode::ProtocolMismatch)?;
                    if rec.rflags & rflags::TTL != 0 {
                        r.u32().ok_or(ErrorCode::ProtocolMismatch)?;
                    }
                    if rec.rflags & rflags::REPLY != 0 {
                        r.take(9).ok_or(ErrorCode::ProtocolMismatch)?;
                    }
                    let text = r.take(r.remaining()).unwrap_or(&[]);
                    if text.len() > MAX_TEXT {
                        return Err(ErrorCode::MessageTooLarge);
                    }
                    if core::str::from_utf8(text).is_err() {
                        return Err(ErrorCode::ProtocolMismatch);
                    }
                    if seq > self.chat_rx {
                        self.chat_rx = seq;
                        sink(Event::Chat { seq, text });
                    }
                    ack_upto = Some(self.chat_rx);
                }
                rtype::ACK => sink(Event::Delivered { seq: r.u64().ok_or(ErrorCode::ProtocolMismatch)? }),
                rtype::PING => {
                    let t = r.u64().ok_or(ErrorCode::ProtocolMismatch)?;
                    let hidden = r.u8().unwrap_or(0) != 0;
                    if hidden != self.hidden {
                        self.hidden = hidden;
                        sink(Event::PeerHidden(hidden));
                    }
                    pong = Some(t);
                }
                rtype::PONG => {}
                rtype::GOODBYE => {
                    r.u16().ok_or(ErrorCode::ProtocolMismatch)?;
                    self.close_local();
                    sink(Event::Closed(ErrorCode::PeerOffline));
                    return Ok(());
                }
                rtype::REKEY => rekey = true,
                _ if rec.rflags & rflags::IGNORABLE != 0 => {}
                _ => return Err(ErrorCode::ProtocolMismatch),
            }
        }
        if rekey {
            self.tr.as_mut().ok_or(ErrorCode::CryptoFailed)?.rekey_rx();
        }
        if ack_upto.is_some() || pong.is_some() {
            self.send_records(now_ms, sink, |b| {
                if let Some(s) = ack_upto {
                    frame::write_record(b, rtype::ACK, 0, &s.to_le_bytes())?;
                }
                if let Some(t) = pong {
                    frame::write_record(b, rtype::PONG, 0, &t.to_le_bytes())?;
                }
                Ok(())
            })?;
        }
        Ok(())
    }

    fn send_hello(&mut self, now_ms: u64, sink: &mut impl FnMut(Event<'_>)) -> Result<(), ErrorCode> {
        let sign_pk = self.sign_pk;
        self.send_records(now_ms, sink, |b| {
            let mut body = [0u8; 2 + 4 + 2 + 32 + 1];
            let mut w = Buf::new(&mut body);
            w.u8(ephem_proto::VERSION)?;
            w.u8(ephem_proto::VERSION)?;
            w.u32(CAPS)?;
            w.u16(MAX_TEXT as u16)?;
            w.put(&sign_pk)?;
            w.u8(0)?;
            frame::write_record(b, rtype::HELLO, 0, &body)
        })
    }

    /// Builds records into the TX buffer, seals in place and emits `Send`. No allocation.
    fn send_records(
        &mut self,
        now_ms: u64,
        sink: &mut impl FnMut(Event<'_>),
        fill: impl FnOnce(&mut Buf<'_>) -> Result<(), ()>,
    ) -> Result<(), ErrorCode> {
        let tr = self.tr.as_mut().ok_or(ErrorCode::PeerOffline)?;
        let n = {
            let mut b = Buf::new(&mut self.tx[HEADER_LEN..HEADER_LEN + MAX_PLAIN]);
            fill(&mut b).map_err(|_| ErrorCode::MessageTooLarge)?;
            b.len()
        };
        let len = tr.seal(&mut self.tx, n)?;
        self.last_tx_ms = now_ms;
        sink(Event::Send(&self.tx[..len]));
        Ok(())
    }

    /// Sends one chat message; returns its `chat_seq`.
    pub fn send_chat(&mut self, now_ms: u64, text: &[u8], sink: &mut impl FnMut(Event<'_>)) -> Result<u64, ErrorCode> {
        if self.state != State::Connected {
            return Err(ErrorCode::PeerOffline);
        }
        if text.is_empty() || text.len() > MAX_TEXT {
            return Err(ErrorCode::MessageTooLarge);
        }
        if core::str::from_utf8(text).is_err() {
            return Err(ErrorCode::ProtocolMismatch);
        }
        let seq = self.chat_tx + 1;
        self.send_records(now_ms, sink, |b| {
            b.u8(rtype::CHAT)?;
            b.u8(0)?;
            b.u16((8 + text.len()) as u16)?;
            b.u64(seq)?;
            b.put(text)
        })?;
        self.chat_tx = seq;
        Ok(seq)
    }

    /// Timer input (driven by the adapter, e.g. every 1 s). `page_hidden` is sent in PING.
    pub fn tick(&mut self, now_ms: u64, page_hidden: bool, sink: &mut impl FnMut(Event<'_>)) {
        match self.state {
            State::AwaitingAnswer if now_ms / 1000 > self.expires_at as u64 => {
                self.fail(ErrorCode::ExpiredInvite, sink);
                return;
            }
            State::Connected => {}
            _ => return,
        }
        if now_ms.saturating_sub(self.last_rx_ms) >= DEGRADED_AFTER_MS && !self.degraded {
            self.degraded = true;
            sink(Event::Degraded);
        }
        let tx_seq = self.tr.as_ref().map_or(0, Transport::tx_seq);
        let rekey = now_ms >= self.rekey_at_ms || (tx_seq > 0 && tx_seq & (REKEY_FRAMES - 1) == 0);
        let ping = now_ms.saturating_sub(self.last_tx_ms) >= PING_IDLE_MS;
        if !rekey && !ping {
            return;
        }
        let res = self.send_records(now_ms, sink, |b| {
            if ping {
                let mut body = [0u8; 9];
                body[..8].copy_from_slice(&now_ms.to_le_bytes());
                body[8] = page_hidden as u8;
                frame::write_record(b, rtype::PING, 0, &body)?;
            }
            if rekey {
                frame::write_record(b, rtype::REKEY, 0, &[])?;
            }
            Ok(())
        });
        match res {
            Ok(()) if rekey => {
                if let Some(tr) = self.tr.as_mut() {
                    tr.rekey_tx();
                }
                self.rekey_at_ms = now_ms + REKEY_MS;
            }
            Ok(()) => {}
            Err(e) => self.fail(e, sink),
        }
    }

    /// Leaves the chat: sends GOODBYE if connected, then zeroizes keys.
    pub fn close(&mut self, now_ms: u64, sink: &mut impl FnMut(Event<'_>)) {
        if self.state == State::Connected {
            let code = ErrorCode::PeerOffline.code().to_le_bytes();
            let _ = self.send_records(now_ms, sink, |b| frame::write_record(b, rtype::GOODBYE, 0, &code));
        }
        self.close_local();
    }

    fn close_local(&mut self) {
        self.state = State::Closed;
        self.hs = None;
        self.tr = None; // Transport zeroizes on drop.
        self.tx.fill(0);
    }

    fn fail(&mut self, e: ErrorCode, sink: &mut impl FnMut(Event<'_>)) {
        if self.state == State::Closed {
            return;
        }
        if e != ErrorCode::ExpiredInvite {
            let code = e.code().to_le_bytes();
            let _ = self.send_records(0, sink, |b| frame::write_record(b, rtype::GOODBYE, 0, &code));
        }
        self.close_local();
        sink(Event::Closed(e));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const NOW_S: u32 = 1_790_000_000;
    const NOW_MS: u64 = NOW_S as u64 * 1000;

    fn local_sdp(ufrag: &str, fp: u8, mdns: &str, port: u16) -> ([u8; 1024], usize) {
        use core::fmt::Write;
        let mut out = [0u8; 1024];
        let mut b = Buf::new(&mut out);
        write!(b, "v=0\r\na=candidate:1 1 udp 2122260223 {mdns}.local {port} typ host\r\n\
            a=candidate:2 1 udp 2122260223 192.168.1.5 {port} typ host\r\n\
            a=candidate:3 1 udp 1686052607 203.0.113.9 {port} typ srflx raddr 0.0.0.0 rport 0\r\n\
            a=ice-ufrag:{ufrag}\r\na=ice-pwd:55bZhqbovQp1LFLkFt9yUC/k\r\na=fingerprint:sha-256 ").unwrap();
        for i in 0..32 {
            write!(b, "{}{:02X}", if i > 0 { ":" } else { "" }, fp).unwrap();
        }
        b.put(b"\r\n").unwrap();
        let n = b.len();
        (out, n)
    }

    /// Frames in flight between the two sides (copied, as the network would).
    struct Wire {
        frames: [[u8; MAX_FRAME]; 8],
        lens: [usize; 8],
        n: usize,
    }

    impl Wire {
        fn new() -> Box<Self> {
            Box::new(Self { frames: [[0; MAX_FRAME]; 8], lens: [0; 8], n: 0 })
        }
    }

    #[derive(Default)]
    struct Seen {
        sas: Option<Sas>,
        chats: u32,
        last_text: [u8; 16],
        delivered: u64,
        hello: bool,
        closed: Option<ErrorCode>,
    }

    fn collect<'w>(wire: &'w mut Wire, seen: &'w mut Seen) -> impl FnMut(Event<'_>) + 'w {
        move |e| match e {
            Event::Send(f) => {
                wire.frames[wire.n][..f.len()].copy_from_slice(f);
                wire.lens[wire.n] = f.len();
                wire.n += 1;
            }
            Event::Connected { sas, .. } => seen.sas = Some(sas),
            Event::Hello { .. } => seen.hello = true,
            Event::Chat { text, .. } => {
                seen.chats += 1;
                seen.last_text = [0; 16];
                seen.last_text[..text.len()].copy_from_slice(text);
            }
            Event::Delivered { seq } => seen.delivered = seq,
            Event::Closed(e) => seen.closed = Some(e),
            _ => {}
        }
    }

    /// Delivers every frame queued by `from` to `to`.
    fn pump(from: &mut Wire, to: &mut Session, wire_back: &mut Wire, seen: &mut Seen) {
        for i in 0..from.n {
            let len = from.lens[i];
            let mut sink = collect(wire_back, seen);
            to.on_frame(NOW_MS, &mut from.frames[i][..len], &mut sink);
        }
        from.n = 0;
    }

    struct Pair {
        a: Box<Session>,
        b: Box<Session>,
        wa: Box<Wire>,
        wb: Box<Wire>,
        sa: Seen,
        sb: Seen,
    }

    fn connect(privacy: Privacy) -> Pair {
        let alice = Identity::from_seed(&[1; 32]);
        let bob = Identity::from_seed(&[2; 32]);
        let mut a = Box::new(Session::offerer(&alice, [7; 16], [9; 16], NOW_S + 300, privacy, false));
        let (sdp_a, na) = local_sdp("AAAA", 0xAA, "9090b126-3aae-4a3e-b714-5d089ddfbff0", 40000);
        let invite: [u8; MAX_CODE_LEN] = {
            let c = a.build_code(&alice, core::str::from_utf8(&sdp_a[..na]).unwrap()).unwrap();
            let mut x = [0u8; MAX_CODE_LEN];
            x[..c.len()].copy_from_slice(c);
            x
        };
        let invite_len = a.invite_len as usize;
        let mut b = Box::new(Session::answerer(&bob, &invite[..invite_len], NOW_S, Privacy::Default, false).unwrap());
        let mut sdp = [0u8; MAX_SDP_LEN];
        assert!(b.remote_sdp(&mut sdp).unwrap().contains("a=setup:actpass"));
        let (sdp_b, nb) = local_sdp("BBBB", 0xBB, "1111b126-3aae-4a3e-b714-5d089ddfbff0", 50000);
        let answer: ([u8; MAX_CODE_LEN], usize) = {
            let c = b.build_code(&bob, core::str::from_utf8(&sdp_b[..nb]).unwrap()).unwrap();
            let mut x = [0u8; MAX_CODE_LEN];
            x[..c.len()].copy_from_slice(c);
            (x, c.len())
        };
        a.apply_answer(&alice, &answer.0[..answer.1], NOW_S + 10).unwrap();
        assert_eq!(a.apply_answer(&alice, &answer.0[..answer.1], NOW_S + 10), Err(ErrorCode::InviteConsumed));
        let r = a.remote_sdp(&mut sdp).unwrap();
        assert!(r.contains("a=setup:active") && r.contains("a=ice-ufrag:BBBB"));
        let mut p = Pair { a, b, wa: Wire::new(), wb: Wire::new(), sa: Seen::default(), sb: Seen::default() };
        // DataChannel opens on both sides; Alice initiates.
        p.b.on_open(NOW_MS, &mut collect(&mut p.wb, &mut p.sb));
        p.a.on_open(NOW_MS, &mut collect(&mut p.wa, &mut p.sa));
        assert_eq!(p.wa.n, 1);
        p.settle();
        p
    }

    impl Pair {
        fn settle(&mut self) {
            for _ in 0..4 {
                pump(&mut self.wa, &mut self.b, &mut self.wb, &mut self.sb);
                pump(&mut self.wb, &mut self.a, &mut self.wa, &mut self.sa);
            }
        }
    }

    #[test]
    fn full_flow() {
        let mut p = connect(Privacy::Default);
        assert_eq!(p.a.state(), State::Connected);
        assert_eq!(p.b.state(), State::Connected);
        assert!(p.sa.sas.is_some() && p.sa.sas == p.sb.sas, "SAS equal on both sides");
        assert!(p.sa.hello && p.sb.hello);

        let seq = p.a.send_chat(NOW_MS, "héllo".as_bytes(), &mut collect(&mut p.wa, &mut p.sa)).unwrap();
        p.settle();
        assert_eq!(p.sb.chats, 1);
        assert_eq!(&p.sb.last_text[..6], "héllo".as_bytes());
        assert_eq!(p.sa.delivered, seq);

        p.b.send_chat(NOW_MS, b"hi", &mut collect(&mut p.wb, &mut p.sb)).unwrap();
        p.settle();
        assert_eq!(p.sa.chats, 1);

        // Idle → PING/PONG; rekey after 10 min keeps both sides in sync.
        p.a.tick(NOW_MS + REKEY_MS, false, &mut collect(&mut p.wa, &mut p.sa));
        p.settle();
        p.a.send_chat(NOW_MS, b"after rekey", &mut collect(&mut p.wa, &mut p.sa)).unwrap();
        p.settle();
        assert_eq!(p.sb.chats, 2);
        assert!(p.sb.closed.is_none());

        p.a.close(NOW_MS, &mut collect(&mut p.wa, &mut p.sa));
        p.settle();
        assert_eq!(p.sb.closed, Some(ErrorCode::PeerOffline));
        assert_eq!(p.b.state(), State::Closed);
    }

    #[test]
    fn tamper_closes() {
        let mut p = connect(Privacy::Default);
        p.a.send_chat(NOW_MS, b"x", &mut collect(&mut p.wa, &mut p.sa)).unwrap();
        p.wa.frames[0][HEADER_LEN] ^= 1;
        p.settle();
        assert_eq!(p.sb.closed, Some(ErrorCode::CryptoFailed));
        assert_eq!(p.b.state(), State::Closed);
    }

    #[test]
    fn privacy_filter_and_expiry() {
        let alice = Identity::from_seed(&[1; 32]);
        let bob = Identity::from_seed(&[2; 32]);
        let (sdp_a, na) = local_sdp("AAAA", 0xAA, "9090b126-3aae-4a3e-b714-5d089ddfbff0", 40000);
        let sdp_a = core::str::from_utf8(&sdp_a[..na]).unwrap();
        for (mode, want) in [(Privacy::LanOnly, 1), (Privacy::Default, 2), (Privacy::MaxConnectivity, 3)] {
            let mut a = Session::offerer(&alice, [7; 16], [9; 16], NOW_S + 300, mode, false);
            let c = Code::decode(a.build_code(&alice, sdp_a).unwrap()).unwrap();
            assert_eq!(c.ice.n_cand, want, "{mode:?}");
            assert_eq!(c.flags & flags::LAN_ONLY != 0, mode == Privacy::LanOnly);
        }
        let mut a = Session::offerer(&alice, [7; 16], [9; 16], NOW_S + 300, Privacy::Default, false);
        let inv = a.build_code(&alice, sdp_a).unwrap();
        let mut inv_copy = [0u8; MAX_CODE_LEN];
        inv_copy[..inv.len()].copy_from_slice(inv);
        let n = inv.len();
        assert_eq!(
            Session::answerer(&bob, &inv_copy[..n], NOW_S + 300 + SKEW_S + 1, Privacy::Default, false).err(),
            Some(ErrorCode::ExpiredInvite)
        );
        assert_eq!(
            Session::answerer(&alice, &inv_copy[..n], NOW_S, Privacy::Default, false).err(),
            Some(ErrorCode::InvalidInvite),
            "own invite"
        );
        let mut seen = Seen::default();
        let mut w = Wire::new();
        a.tick(NOW_MS + 301_000, false, &mut collect(&mut w, &mut seen));
        assert_eq!(seen.closed, Some(ErrorCode::ExpiredInvite));
        assert_eq!(w.n, 0, "nothing sent before a channel exists");
    }
}
