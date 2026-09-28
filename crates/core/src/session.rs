//! One 1:1 chat (§12) over a sequence of network paths.
//!
//! - A **path** is one RTCPeerConnection: codes → Noise KK → transport. It is disposable (P5).
//! - The **chat** (peer key, `chat_seq` counters, pending ring, timers) survives paths: after
//!   a path is lost the chat is SUSPENDED, and a T3 resume code starts a new path that rebinds
//!   with the same static keys, continues `chat_seq` and resends what was not ACKed (§13).
//!
//! All buffers are fixed and allocated when the chat starts (setup path). The connected path
//! does not allocate; the only setup-time heap use is snow's handshake state and the pending ring.

use crate::messages::{MsgRef, Pending, TTL_CHOICES, Timers};
use crate::room::{MAX_MEMBERS, RoomRole};
use ephem_crypto::noise::{HS_MSG_LEN, Handshake, IK_PAYLOAD_LEN, MAX_HS_MSG_LEN, Transport};
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
/// SUSPENDED grace before the chat is closed (§12).
pub const SUSPEND_GRACE_MS: u64 = 600_000;
/// Rekey period (§10.1): 10 minutes or 2^20 frames, whichever comes first.
pub const REKEY_MS: u64 = 600_000;
pub const REKEY_FRAMES: u64 = 1 << 20;
/// Answerer-side expiry tolerance for clock skew (§8.6).
pub const SKEW_S: u32 = 120;
/// READ is coalesced to at most one per second; TYPING refreshed every 3 s, cleared after 6 s.
pub const READ_EVERY_MS: u64 = 1_000;
pub const TYPING_EVERY_MS: u64 = 3_000;
pub const TYPING_CLEAR_MS: u64 = 6_000;
/// Largest plaintext that fits one frame.
pub const MAX_PLAIN: usize = MAX_FRAME - HEADER_LEN - TAG_LEN;
/// Identity transfer (§7.6): chunk size and the largest key file accepted.
pub const IDENTITY_CHUNK: usize = 12 * 1024;
pub const MAX_IDENTITY: usize = 72 * 1024;
/// Reaction emoji: 1..=32 UTF-8 bytes (one grapheme, checked by the UI) (§11.7).
pub const MAX_REACTION: usize = 32;
/// Nickname sent in HELLO (§7.3 body, §11.2).
pub const MAX_NICK: usize = 32;

/// HELLO capability bits (§11.4).
pub mod caps {
    pub const RESUME: u32 = 1 << 1;
    /// The sender sends read receipts (its privacy setting, reciprocal).
    pub const READ: u32 = 1 << 3;
    /// The sender sends typing notifications (reciprocal).
    pub const TYPING: u32 = 1 << 4;
    /// The sender received the peer's code with the in-app scanner (SAS policy, §10.4).
    pub const SCANNED: u32 = 1 << 8;
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum Role {
    Offerer,
    Answerer,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum State {
    /// Waiting for the local description of the current path.
    Gathering,
    /// Offerer: code shown, waiting for the answer.
    AwaitingAnswer,
    /// Both codes known: ICE/DTLS, then the Noise handshake.
    Connecting,
    Connected,
    /// Path lost after the chat was connected; resume with a T3 code (§13).
    Suspended,
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

/// Per-chat settings. Read receipts and typing are reciprocal (§11.7): off = neither sent nor
/// shown. `nick` is our nickname (saved identities, §7.3), sent in HELLO.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct Settings {
    pub read_receipts: bool,
    pub typing: bool,
    pub nick: [u8; MAX_NICK],
    pub nick_len: u8,
}

impl Settings {
    #[inline]
    pub fn nick(&self) -> &[u8] {
        &self.nick[..self.nick_len as usize]
    }

    /// Sets the nickname; longer than 32 bytes or not UTF-8 is refused.
    pub fn set_nick(&mut self, nick: &[u8]) -> bool {
        if nick.len() > MAX_NICK || core::str::from_utf8(nick).is_err() {
            return false;
        }
        self.nick = [0; MAX_NICK];
        self.nick[..nick.len()].copy_from_slice(nick);
        self.nick_len = nick.len() as u8;
        true
    }
}

impl Default for Settings {
    fn default() -> Self {
        Self { read_receipts: true, typing: true, nick: [0; MAX_NICK], nick_len: 0 }
    }
}

/// This link's place in a room (§14): member indices and roles, fixed by the owner-signed state.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct RoomLink {
    pub me: u8,
    pub peer: u8,
    pub my_role: RoomRole,
    pub peer_role: RoomRole,
}

/// Diagnostics of the current path (§18).
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq)]
pub struct Diag {
    /// Last PING→PONG round trip, ms (0 = none yet).
    pub app_rtt_ms: u32,
    /// Noise rekeys done on this path (the "epoch").
    pub rekeys: u32,
    /// Time until the next scheduled rekey, ms.
    pub rekey_in_ms: u64,
    /// Our messages not acknowledged yet.
    pub pending: u64,
}

/// Output of the core. Slices borrow either the session's TX buffer or the caller's RX frame.
#[derive(Debug, PartialEq, Eq)]
pub enum Event<'a> {
    /// Transmit this frame on the DataChannel.
    Send(&'a [u8]),
    /// Noise handshake done. `resumed` = a later path of the same chat (no new SAS check needed:
    /// the static keys are the ones already pinned).
    Connected { sas: Sas, peer: PeerId, resumed: bool },
    /// Peer HELLO. `sas_optional` = both codes were scanned in person (§10.4); never for an
    /// identity transfer. `sign_pk` is the peer's Ed25519 key (stored with a contact).
    Hello { nick: &'a [u8], sas_optional: bool, sign_pk: [u8; 32] },
    /// Incoming chat message (UTF-8 validated); `msg.sender` is the peer's member index.
    Chat { msg: MsgRef, text: &'a [u8], ttl_s: u32, reply: Option<MsgRef> },
    /// Member `by` changed the self-destruct timer (0 = off).
    Setting { ttl_s: u32, by: u8 },
    /// The peer edited its message.
    Edited { msg: MsgRef, text: &'a [u8] },
    /// A message was deleted for everyone.
    Deleted(MsgRef),
    /// Our messages up to `seq` were delivered (cumulative ACK).
    Delivered { seq: u64 },
    /// Our messages up to `seq` were read (cumulative READ).
    Read { seq: u64 },
    /// Self-destruct timer fired: remove the message everywhere.
    Expired(MsgRef),
    /// Member `by` reacted to a message (empty = reaction removed) (§11.7).
    Reaction { msg: MsgRef, emoji: &'a [u8], by: u8 },
    /// A room record (ROOM_STATE, ROOM_SIGNAL, ROOM_LEAVE) for the room layer (§14).
    Room { rtype: u8, body: &'a [u8] },
    /// In-band ICE restart from the peer (§13 T1): render with [`Session::render_signal`].
    SignalOffer(IceParams),
    SignalAnswer(IceParams),
    /// Identity transfer (§7.6): the receiving device confirmed the SAS.
    PeerReady,
    /// Identity transfer: the key file was sent completely.
    IdentitySent,
    /// Identity transfer: the complete encrypted key file (still passphrase-protected).
    IdentityReceived(&'a [u8]),
    PeerTyping(bool),
    /// No traffic for `DEGRADED_AFTER_MS` (§12).
    Degraded,
    /// Traffic resumed after `Degraded`.
    Alive,
    /// Peer reports its tab is hidden / visible (PING flag, §12).
    PeerHidden(bool),
    /// The path is gone; the chat waits for a resume code (§13 T3).
    Suspended,
    /// Chat closed: by the peer (GOODBYE), by an error, or by expiry.
    Closed(ErrorCode),
}

pub struct Session {
    // ---- current path ----
    role: Role,
    state: State,
    resume: bool,
    privacy: Privacy,
    drop_ipv6: bool,
    invite_id: [u8; 16],
    expires_at: u32,
    code_flags: u8,
    remote_ice: IceParams,
    invite: [u8; MAX_CODE_LEN],
    invite_len: u16,
    answer: [u8; MAX_CODE_LEN],
    answer_len: u16,
    hs: Option<Handshake>,
    tr: Option<Transport>,
    tx: [u8; MAX_FRAME],
    // ---- chat ----
    local: PeerId,
    sign_pk: [u8; 32],
    remote: PeerId,
    room_id: [u8; 16],
    /// PeerIdx of this side and of the peer: 0/1 by the first path's roles in a 1:1 chat, the
    /// room indices in a room (§11.3).
    me_idx: u8,
    peer_idx: u8,
    room: Option<RoomLink>,
    ever_connected: bool,
    settings: Settings,
    scanned: bool,
    peer_caps: u32,
    pending: Pending,
    chat_ttl_s: u32,
    chat_rx: u64,
    own_timers: Timers,
    peer_timers: Timers,
    read_upto: u64,
    read_sent: u64,
    read_sent_ms: u64,
    typing_sent_ms: u64,
    typing_sent: bool,
    peer_typing_until: u64,
    last_rx_ms: u64,
    last_tx_ms: u64,
    rekey_at_ms: u64,
    suspended_at_ms: u64,
    degraded: bool,
    hidden: bool,
    /// The peer's Ed25519 key from HELLO (stored with a contact, §7.5).
    peer_sign_pk: [u8; 32],
    // ---- identity transfer (§7.6) ----
    transfer: bool,
    sas_confirmed: bool,
    peer_ready: bool,
    /// Sender: the key file to send. Receiver: the chunks received so far.
    xfer: Vec<u8>,
    xfer_total: u16,
    xfer_next: u16,
    // ---- diagnostics / renegotiation ----
    app_rtt_ms: u32,
    rekeys: u32,
    /// `o=` version of the next remote description rendered on this path (§13 T1).
    sdp_version: u32,
    // ---- Tor mode (§28.4) ----
    /// This chat runs over Tor streams (Noise IK, one-way invite), not WebRTC.
    tor: bool,
    /// Our onion service key (the host's is in the invite).
    onion_pk: [u8; 32],
    /// The peer's onion service key: from the invite (dialer) or IK message 1 (host).
    peer_onion: [u8; 32],
}

#[inline]
fn session_id(invite_id: &[u8; 16]) -> u64 {
    // SDP `o=` session id must fit in 63 bits.
    let mut b = [0u8; 8];
    b.copy_from_slice(&invite_id[..8]);
    u64::from_le_bytes(b) >> 1
}

/// How an outgoing CHAT is queued: explicit room sequence number, record flags, timer, reply.
#[derive(Copy, Clone)]
struct OutHeader {
    at: Option<u64>,
    rf: u8,
    ttl_s: u32,
    reply: MsgRef,
}

/// A record that does not fit the frame: only reachable through a caller bug.
#[inline]
fn too_large(_: ()) -> ErrorCode {
    ErrorCode::MessageTooLarge
}

impl Session {
    fn blank(role: Role, id: &Identity, privacy: Privacy, drop_ipv6: bool, settings: Settings) -> Self {
        Self {
            role,
            state: State::Gathering,
            resume: false,
            privacy,
            drop_ipv6,
            invite_id: [0; 16],
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
            local: id.peer_id(),
            sign_pk: id.sign_pk(),
            remote: PeerId([0; 32]),
            room_id: [0; 16],
            me_idx: 0,
            peer_idx: 1,
            room: None,
            ever_connected: false,
            settings,
            scanned: false,
            peer_caps: 0,
            pending: Pending::new(),
            chat_ttl_s: 0,
            chat_rx: 0,
            own_timers: Timers::new(),
            peer_timers: Timers::new(),
            read_upto: 0,
            read_sent: 0,
            read_sent_ms: 0,
            typing_sent_ms: 0,
            typing_sent: false,
            peer_typing_until: 0,
            last_rx_ms: 0,
            last_tx_ms: 0,
            rekey_at_ms: 0,
            suspended_at_ms: 0,
            degraded: false,
            hidden: false,
            peer_sign_pk: [0; 32],
            transfer: false,
            sas_confirmed: false,
            peer_ready: false,
            xfer: Vec::new(),
            xfer_total: 0,
            xfer_next: 0,
            app_rtt_ms: 0,
            rekeys: 0,
            sdp_version: 2,
            tor: false,
            onion_pk: [0; 32],
            peer_onion: [0; 32],
        }
    }

    /// Alice: a new chat and its invite. `invite_id` and `room_id` come from the CSPRNG.
    pub fn offerer(id: &Identity, invite_id: [u8; 16], room_id: [u8; 16], expires_at: u32, privacy: Privacy, drop_ipv6: bool, settings: Settings) -> Self {
        let mut s = Self::blank(Role::Offerer, id, privacy, drop_ipv6, settings);
        s.invite_id = invite_id;
        s.room_id = room_id;
        s.expires_at = expires_at;
        s.code_flags = if privacy == Privacy::LanOnly { flags::LAN_ONLY } else { 0 };
        s
    }

    /// New device (§7.6): an invite asking for an identity. This side is the receiver.
    pub fn transfer_receiver(id: &Identity, invite_id: [u8; 16], room_id: [u8; 16], expires_at: u32, privacy: Privacy, drop_ipv6: bool) -> Self {
        let mut s = Self::offerer(id, invite_id, room_id, expires_at, privacy, drop_ipv6, Settings::default());
        s.code_flags |= flags::TRANSFER;
        s.transfer = true;
        s
    }

    /// Bob: a new chat from Alice's invite. `scanned` = the invite came from the in-app scanner.
    pub fn answerer(id: &Identity, invite: &[u8], now_s: u32, privacy: Privacy, drop_ipv6: bool, settings: Settings, scanned: bool) -> Result<Self, ErrorCode> {
        let c = Code::decode(invite)?;
        if c.kind != Kind::Invite {
            return Err(ErrorCode::InvalidInvite);
        }
        Self::check_invite(id, &c, now_s)?;
        // Bob follows Alice's LAN-only request; otherwise his own mode.
        let privacy = if c.flags & flags::LAN_ONLY != 0 { Privacy::LanOnly } else { privacy };
        let mut s = Self::blank(Role::Answerer, id, privacy, drop_ipv6, settings);
        s.remote = PeerId(c.static_pk);
        s.room_id = c.room_id;
        s.scanned = scanned;
        s.transfer = c.flags & flags::TRANSFER != 0;
        s.take_invite(&c, invite);
        Ok(s)
    }

    fn check_invite(id: &Identity, c: &Code, now_s: u32) -> Result<(), ErrorCode> {
        if now_s > c.expires_at.saturating_add(SKEW_S) {
            return Err(ErrorCode::ExpiredInvite);
        }
        if c.static_pk == id.peer_id().0 {
            return Err(ErrorCode::InvalidInvite);
        }
        Ok(())
    }

    fn take_invite(&mut self, c: &Code, raw: &[u8]) {
        self.role = Role::Answerer;
        self.state = State::Gathering;
        self.invite_id = c.invite_id;
        self.expires_at = c.expires_at;
        self.code_flags = c.flags;
        self.remote_ice = c.ice;
        self.invite[..raw.len()].copy_from_slice(raw);
        self.invite_len = raw.len() as u16;
        self.answer_len = 0;
    }

    // ---- queries ----

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

    #[inline(always)]
    pub fn invite_id(&self) -> [u8; 16] {
        self.invite_id
    }

    #[inline(always)]
    pub fn room_id(&self) -> [u8; 16] {
        self.room_id
    }

    /// Makes this link part of a room (§14): fixed member indices and roles; room records are
    /// accepted and roles enforced from now on. Call before the first message.
    pub fn set_room(&mut self, link: RoomLink) {
        debug_assert!((link.me as usize) < MAX_MEMBERS && (link.peer as usize) < MAX_MEMBERS && link.me != link.peer);
        self.me_idx = link.me;
        self.peer_idx = link.peer;
        self.room = Some(link);
        self.code_flags |= flags::GROUP;
        self.encode_tor_invite();
    }

    /// Our next room message on this link continues from `seq` (a link opened mid-room, §14.3).
    pub fn set_seq_base(&mut self, seq: u64) -> bool {
        self.pending.rebase(seq)
    }

    /// Extra invite flags (e.g. `GROUP`, `OBSERVER`) before the offerer builds its code.
    pub fn add_flags(&mut self, f: u8) {
        self.code_flags |= f & flags::KNOWN;
        self.encode_tor_invite();
    }

    #[inline(always)]
    pub fn code_flags(&self) -> u8 {
        self.code_flags
    }

    #[inline(always)]
    pub fn me_idx(&self) -> u8 {
        self.me_idx
    }

    #[inline(always)]
    pub fn peer_idx(&self) -> u8 {
        self.peer_idx
    }

    #[inline(always)]
    pub fn room_link(&self) -> Option<RoomLink> {
        self.room
    }

    #[inline]
    fn peer_observer(&self) -> bool {
        self.room.is_some_and(|r| r.peer_role == RoomRole::Observer)
    }

    #[inline]
    fn me_observer(&self) -> bool {
        self.room.is_some_and(|r| r.my_role == RoomRole::Observer)
    }

    /// This chat is an identity transfer (§7.6), not a conversation.
    #[inline(always)]
    pub fn transfer(&self) -> bool {
        self.transfer
    }

    #[inline(always)]
    pub fn peer_sign_pk(&self) -> [u8; 32] {
        self.peer_sign_pk
    }

    #[inline(always)]
    pub fn sas_confirmed(&self) -> bool {
        self.sas_confirmed
    }

    pub fn diag(&self, now_ms: u64) -> Diag {
        Diag { app_rtt_ms: self.app_rtt_ms, rekeys: self.rekeys, rekey_in_ms: self.rekey_at_ms.saturating_sub(now_ms), pending: self.pending_count() }
    }

    #[inline(always)]
    pub fn ever_connected(&self) -> bool {
        self.ever_connected
    }

    #[inline(always)]
    pub fn chat_ttl(&self) -> u32 {
        self.chat_ttl_s
    }

    /// Number of our messages not yet acknowledged.
    #[inline(always)]
    pub fn pending_count(&self) -> u64 {
        self.pending.last() - self.pending.acked()
    }

    /// This side's code of the current path, for diagnostics (§29.2).
    pub fn local_code(&self) -> &[u8] {
        match self.role {
            Role::Offerer => &self.invite[..self.invite_len as usize],
            Role::Answerer => &self.answer[..self.answer_len as usize],
        }
    }

    // ---- Tor mode (§28.4) ----

    /// Tor mode, the inviter: a new chat and its one-way invite (TOR_INVITE, kind 5) to our
    /// onion service. There is no answer code: incoming streams go to [`Self::tor_accept`].
    pub fn tor_host(id: &Identity, invite_id: [u8; 16], room_id: [u8; 16], expires_at: u32, settings: Settings) -> Self {
        let mut s = Self::blank(Role::Offerer, id, Privacy::Default, false, settings);
        s.tor = true;
        s.onion_pk = id.onion_pk();
        s.invite_id = invite_id;
        s.room_id = room_id;
        s.expires_at = expires_at;
        s.state = State::AwaitingAnswer;
        s.encode_tor_invite();
        s
    }

    /// Re-encodes the host's Tor invite after a flag change.
    fn encode_tor_invite(&mut self) {
        if !self.tor || self.role != Role::Offerer {
            return;
        }
        let code = Code {
            kind: Kind::TorInvite,
            flags: self.code_flags,
            invite_id: self.invite_id,
            room_id: self.room_id,
            static_pk: self.local.0,
            onion_pk: self.onion_pk,
            expires_at: self.expires_at,
            ice: IceParams::EMPTY,
        };
        let n = code.encode(&mut self.invite).expect("a Tor invite fits");
        self.invite_len = n as u16;
    }

    /// Tor mode, the dialer: a chat from a TOR_INVITE. The adapter dials the onion, calls
    /// [`Self::tor_dial`], then [`Self::on_open`] when the stream is up.
    pub fn tor_dialer(id: &Identity, invite: &[u8], now_s: u32, settings: Settings, scanned: bool) -> Result<Self, ErrorCode> {
        let c = Code::decode(invite)?;
        // Identity transfer (§7.6) is a direct-mode feature.
        if c.kind != Kind::TorInvite || c.flags & flags::TRANSFER != 0 {
            return Err(ErrorCode::InvalidInvite);
        }
        Self::check_invite(id, &c, now_s)?;
        let mut s = Self::blank(Role::Answerer, id, Privacy::Default, false, settings);
        s.tor = true;
        s.onion_pk = id.onion_pk();
        s.remote = PeerId(c.static_pk);
        s.peer_onion = c.onion_pk;
        s.room_id = c.room_id;
        s.invite_id = c.invite_id;
        s.expires_at = c.expires_at;
        s.code_flags = c.flags;
        s.scanned = scanned;
        s.invite[..invite.len()].copy_from_slice(invite);
        s.invite_len = invite.len() as u16;
        s.state = State::Gathering;
        Ok(s)
    }

    #[inline(always)]
    pub fn tor(&self) -> bool {
        self.tor
    }

    /// The peer's onion service key (Tor mode; kept with a contact, §28.7).
    #[inline(always)]
    pub fn peer_onion(&self) -> [u8; 32] {
        self.peer_onion
    }

    /// Tor mode, the dialer: prepares a (new) stream to the host: the first one, or a redial
    /// after the stream was lost (§28.5: no recovery ladder, the dialer simply dials again).
    pub fn tor_dial(&mut self, id: &Identity) -> Result<(), ErrorCode> {
        if !self.tor || self.role != Role::Answerer || self.state == State::Closed || self.state == State::Connected {
            return Err(ErrorCode::NotPermitted);
        }
        self.drop_path();
        self.hs = Some(Handshake::ik_initiator(id, &self.remote, &self.invite[..self.invite_len as usize])?);
        self.state = State::Connecting;
        Ok(())
    }

    /// Tor mode, the host: the first frame of an incoming stream. `Ok(true)`: this chat took
    /// the stream (reply sent; the chat is connected); `Ok(false)`: not this chat's (another
    /// invite, or a key other than the pinned peer's), try the next one. Every stream is
    /// authenticated here, before any application data (§28.4).
    pub fn tor_accept(&mut self, id: &Identity, now_ms: u64, frame: &[u8], sink: &mut impl FnMut(Event<'_>)) -> Result<bool, ErrorCode> {
        if !self.tor || self.role != Role::Offerer || self.state == State::Closed {
            return Ok(false);
        }
        if !self.ever_connected && (self.state != State::AwaitingAnswer || (now_ms / 1000) as u32 > self.expires_at) {
            return Ok(false);
        }
        let h = Header::read(frame)?;
        if h.ftype != FrameType::Handshake {
            return Ok(false);
        }
        let mut hs = Handshake::ik_responder(id, &self.invite[..self.invite_len as usize])?;
        let mut payload = [0u8; IK_PAYLOAD_LEN];
        match hs.read_payload(&frame[HEADER_LEN..], &mut payload) {
            Ok(IK_PAYLOAD_LEN) => {}
            _ => return Ok(false),
        }
        let Some(remote) = hs.remote_static() else { return Ok(false) };
        if payload[..16] != self.invite_id || (self.ever_connected && remote != self.remote) || remote == self.local {
            return Ok(false);
        }
        self.drop_path();
        self.remote = remote;
        self.peer_onion.copy_from_slice(&payload[16..]);
        self.hs = Some(hs);
        self.state = State::Connecting;
        self.last_rx_ms = now_ms;
        if let Err(e) = self.complete_handshake(now_ms, sink) {
            self.fail(e, sink);
        }
        Ok(true)
    }

    // ---- rendezvous ----

    /// Renders the remote description (offer for the answerer, answer for the offerer).
    pub fn remote_sdp<'o>(&self, out: &'o mut [u8; MAX_SDP_LEN]) -> Result<&'o str, ErrorCode> {
        let role = match self.role {
            Role::Answerer => sdp::Role::Offer,
            Role::Offerer => sdp::Role::Answer,
        };
        let n = sdp::render_remote(&self.remote_ice, role, session_id(&self.invite_id), 2, out).map_err(|_| ErrorCode::InvalidInvite)?;
        core::str::from_utf8(&out[..n]).map_err(|_| ErrorCode::InvalidInvite)
    }

    /// Renders the peer's in-band re-offer or re-answer (§13 T1) for the current connection.
    /// The DTLS roles of the connection are kept: the path's answerer is the DTLS client.
    pub fn render_signal<'o>(&mut self, ice: &IceParams, offer: bool, out: &'o mut [u8; MAX_SDP_LEN]) -> Result<&'o str, ErrorCode> {
        let role = match (offer, self.role) {
            (true, _) => sdp::Role::Offer,
            (false, Role::Offerer) => sdp::Role::Answer,
            (false, Role::Answerer) => sdp::Role::AnswerPassive,
        };
        self.sdp_version += 1;
        let n = sdp::render_remote(ice, role, session_id(&self.invite_id), self.sdp_version, out).map_err(|_| ErrorCode::InvalidInvite)?;
        core::str::from_utf8(&out[..n]).map_err(|_| ErrorCode::InvalidInvite)
    }

    /// Sends our re-offer or re-answer of an ICE restart over the open channel (§13 T1).
    pub fn signal(&mut self, now_ms: u64, offer: bool, local_sdp: &str, sink: &mut impl FnMut(Event<'_>)) -> Result<(), ErrorCode> {
        if !self.connected() {
            return Err(ErrorCode::PeerOffline);
        }
        let (privacy, drop_ipv6) = (self.privacy, self.drop_ipv6);
        let ice = sdp::parse_local(local_sdp, |c| privacy.keeps(c, drop_ipv6)).ok_or(ErrorCode::IceFailed)?;
        let rt = if offer { rtype::SIGNAL_OFFER } else { rtype::SIGNAL_ANSWER };
        self.send_records(now_ms, sink, |b| {
            let mut body = [0u8; MAX_CODE_LEN];
            let mut w = Buf::new(&mut body);
            w.u8(ice.n_cand)?;
            ice.encode_body(&mut w)?;
            let n = w.len();
            frame::write_record(b, rt, 0, &body[..n])
        })
    }

    /// Builds this side's code from the gathered local description.
    pub fn build_code(&mut self, id: &Identity, local_sdp: &str) -> Result<&[u8], ErrorCode> {
        if self.state != State::Gathering {
            return Err(ErrorCode::NotPermitted);
        }
        let (privacy, drop_ipv6) = (self.privacy, self.drop_ipv6);
        let ice = sdp::parse_local(local_sdp, |c| privacy.keeps(c, drop_ipv6)).ok_or(ErrorCode::IceFailed)?;
        let invite = self.role == Role::Offerer;
        let kind = match (invite, self.resume) {
            (true, false) => Kind::Invite,
            (false, false) => Kind::Answer,
            (true, true) => Kind::ResumeInvite,
            (false, true) => Kind::ResumeAnswer,
        };
        let code = Code {
            kind,
            flags: if invite { self.code_flags } else { 0 },
            invite_id: self.invite_id,
            room_id: if invite { self.room_id } else { [0; 16] },
            static_pk: self.local.0,
            onion_pk: [0; 32],
            expires_at: if invite { self.expires_at } else { 0 },
            ice,
        };
        if invite {
            let n = code.encode(&mut self.invite).map_err(|_| ErrorCode::InvalidInvite)?;
            self.invite_len = n as u16;
            self.state = State::AwaitingAnswer;
            Ok(&self.invite[..n])
        } else {
            let n = code.encode(&mut self.answer).map_err(|_| ErrorCode::InvalidInvite)?;
            self.answer_len = n as u16;
            self.arm_handshake(id)?;
            Ok(&self.answer[..n])
        }
    }

    /// Offerer: applies the answer (or resume answer) to the current invite.
    /// `scanned` = the answer came from the in-app scanner.
    pub fn apply_answer(&mut self, id: &Identity, answer: &[u8], now_s: u32, scanned: bool) -> Result<(), ErrorCode> {
        if self.role != Role::Offerer {
            return Err(ErrorCode::NotPermitted);
        }
        let c = Code::decode(answer)?;
        let want = if self.resume { Kind::ResumeAnswer } else { Kind::Answer };
        if c.kind != want || c.invite_id != self.invite_id {
            return Err(ErrorCode::AnswerMismatch);
        }
        match self.state {
            State::AwaitingAnswer => {}
            State::Gathering => return Err(ErrorCode::NotPermitted),
            _ => return Err(ErrorCode::InviteConsumed),
        }
        if now_s > self.expires_at {
            return Err(ErrorCode::ExpiredInvite);
        }
        if c.static_pk == self.local.0 || (self.ever_connected && c.static_pk != self.remote.0) {
            return Err(ErrorCode::AnswerMismatch);
        }
        self.remote = PeerId(c.static_pk);
        self.remote_ice = c.ice;
        self.answer[..answer.len()].copy_from_slice(answer);
        self.answer_len = answer.len() as u16;
        if !self.ever_connected {
            self.scanned = scanned;
        }
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

    // ---- T3 resume (§13) ----

    /// Starts a new path of this chat with us as offerer; build the RESUME_INVITE next.
    pub fn resume_invite(&mut self, invite_id: [u8; 16], expires_at: u32) -> Result<(), ErrorCode> {
        if !self.ever_connected || self.state == State::Closed {
            return Err(ErrorCode::NotPermitted);
        }
        self.drop_path();
        self.role = Role::Offerer;
        self.resume = true;
        self.state = State::Gathering;
        self.invite_id = invite_id;
        self.expires_at = expires_at;
        self.invite_len = 0;
        self.answer_len = 0;
        Ok(())
    }

    /// Accepts the peer's RESUME_INVITE: it must name this chat (room id) and this peer (key).
    pub fn accept_resume(&mut self, id: &Identity, code: &[u8], now_s: u32) -> Result<(), ErrorCode> {
        let c = Code::decode(code)?;
        if c.kind != Kind::ResumeInvite {
            return Err(ErrorCode::InvalidInvite);
        }
        if !self.ever_connected || self.state == State::Closed || c.room_id != self.room_id {
            return Err(ErrorCode::InvalidRoom);
        }
        if c.static_pk != self.remote.0 {
            return Err(ErrorCode::AuthFailed);
        }
        Self::check_invite(id, &c, now_s)?;
        self.drop_path();
        self.resume = true;
        self.take_invite(&c, code);
        Ok(())
    }

    fn drop_path(&mut self) {
        self.hs = None;
        self.tr = None;
        self.degraded = false;
        self.sdp_version = 2;
    }

    /// The adapter lost the path (DataChannel closed or ICE failed). Before the first connection
    /// this ends the chat with `reason`; afterwards the chat is SUSPENDED and can be resumed.
    pub fn path_lost(&mut self, now_ms: u64, reason: ErrorCode, sink: &mut impl FnMut(Event<'_>)) {
        if self.state == State::Closed || self.state == State::Suspended {
            return;
        }
        if !self.ever_connected {
            self.fail(reason, sink);
            return;
        }
        if self.state == State::Connected {
            self.suspended_at_ms = now_ms;
        }
        self.drop_path();
        self.state = State::Suspended;
        self.peer_typing(false, sink);
        sink(Event::Suspended);
    }

    // ---- transport ----

    /// DataChannel open. The path's offerer sends Noise message 1.
    /// Tor mode: the dialer's stream to the host is up; sends IK message 1 (`invite_id` ‖ our
    /// onion key).
    pub fn on_open(&mut self, now_ms: u64, sink: &mut impl FnMut(Event<'_>)) {
        if self.tor {
            if self.state != State::Connecting || self.role != Role::Answerer {
                return;
            }
            if let Err(e) = self.send_tor_hello(now_ms, sink) {
                self.fail(e, sink);
            }
            return;
        }
        if self.state != State::Connecting || self.role != Role::Offerer {
            return;
        }
        if let Err(e) = self.send_handshake(0, now_ms, sink) {
            self.fail(e, sink);
        }
    }

    fn send_tor_hello(&mut self, now_ms: u64, sink: &mut impl FnMut(Event<'_>)) -> Result<(), ErrorCode> {
        let hs = self.hs.as_mut().ok_or(ErrorCode::CryptoFailed)?;
        let mut payload = [0u8; IK_PAYLOAD_LEN];
        payload[..16].copy_from_slice(&self.invite_id);
        payload[16..].copy_from_slice(&self.onion_pk);
        Header { ftype: FrameType::Handshake, flags: 0, seq: 0 }.write(&mut self.tx).map_err(|_| ErrorCode::CryptoFailed)?;
        let n = hs.write_payload(&payload, &mut self.tx[HEADER_LEN..HEADER_LEN + MAX_HS_MSG_LEN])?;
        self.last_tx_ms = now_ms;
        sink(Event::Send(&self.tx[..HEADER_LEN + n]));
        Ok(())
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
        if !matches!(self.state, State::Connecting | State::Connected) {
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
        self.complete_handshake(now_ms, sink)
    }

    /// After a handshake message was read: the responder replies with message 2, then both
    /// sides derive the transport keys and the chat is connected.
    fn complete_handshake(&mut self, now_ms: u64, sink: &mut impl FnMut(Event<'_>)) -> Result<(), ErrorCode> {
        if !self.hs.as_ref().is_some_and(Handshake::is_finished) {
            // Responder: reply with message 2, which finishes KK and IK.
            self.send_handshake(1, now_ms, sink)?;
        }
        let (tr, sas) = self.hs.take().ok_or(ErrorCode::CryptoFailed)?.finish()?;
        self.tr = Some(tr);
        self.state = State::Connected;
        self.rekey_at_ms = now_ms + REKEY_MS;
        let resumed = self.ever_connected;
        if !resumed {
            self.ever_connected = true;
            if self.room.is_none() {
                self.me_idx = if self.role == Role::Offerer { 0 } else { 1 };
                self.peer_idx = 1 - self.me_idx;
            }
        }
        sink(Event::Connected { sas, peer: self.remote, resumed });
        self.send_hello(now_ms, sink)?;
        self.flush(now_ms, sink)
    }

    /// Resends every unacknowledged CHAT (the receiver drops duplicates) and a pending READ.
    fn flush(&mut self, now_ms: u64, sink: &mut impl FnMut(Event<'_>)) -> Result<(), ErrorCode> {
        for seq in self.pending.acked() + 1..=self.pending.last() {
            self.transmit_slot(seq, now_ms, sink)?;
        }
        self.maybe_send_read(now_ms, sink)
    }

    fn transmit_slot(&mut self, seq: u64, now_ms: u64, sink: &mut impl FnMut(Event<'_>)) -> Result<(), ErrorCode> {
        let Some(slot) = self.pending.get(seq) else { return Ok(()) };
        if slot.deleted {
            return Ok(());
        }
        let tr = self.tr.as_mut().ok_or(ErrorCode::PeerOffline)?;
        let n = {
            let mut b = Buf::new(&mut self.tx[HEADER_LEN..HEADER_LEN + MAX_PLAIN]);
            let (rf, text) = (slot.rflags, slot.text());
            let ttl_len = if rf & rflags::TTL != 0 { 4 } else { 0 };
            let reply_len = if rf & rflags::REPLY != 0 { 9 } else { 0 };
            b.u8(rtype::CHAT).map_err(too_large)?;
            b.u8(rf).map_err(too_large)?;
            b.u16((8 + ttl_len + reply_len + text.len()) as u16).map_err(too_large)?;
            b.u64(seq).map_err(too_large)?;
            if ttl_len != 0 {
                b.u32(slot.ttl_s).map_err(too_large)?;
            }
            if reply_len != 0 {
                b.u8(slot.reply.sender).map_err(too_large)?;
                b.u64(slot.reply.seq).map_err(too_large)?;
            }
            b.put(text).map_err(too_large)?;
            b.len()
        };
        let len = tr.seal(&mut self.tx, n)?;
        self.last_tx_ms = now_ms;
        sink(Event::Send(&self.tx[..len]));
        Ok(())
    }

    fn on_transport(&mut self, now_ms: u64, frame: &mut [u8], sink: &mut impl FnMut(Event<'_>)) -> Result<(), ErrorCode> {
        use ErrorCode::ProtocolMismatch as Bad;
        let range = self.tr.as_mut().ok_or(Bad)?.open(frame)?;
        let plain = &frame[range];
        let (me, peer) = (self.me_idx, self.peer_idx);
        let (in_room, observer) = (self.room.is_some(), self.peer_observer());
        let peer_owner = self.room.is_some_and(|r| r.peer_role == RoomRole::Owner);
        let mut ack_upto = None;
        let mut pong = None;
        let mut rekey = false;
        let mut send_identity = false;
        for rec in Records::new(plain) {
            let rec = rec?;
            let mut r = Rd::new(rec.body);
            match rec.rtype {
                rtype::HELLO => {
                    let (vmin, vmax) = (r.u8().ok_or(Bad)?, r.u8().ok_or(Bad)?);
                    if !(vmin..=vmax).contains(&ephem_proto::VERSION) {
                        return Err(Bad);
                    }
                    self.peer_caps = r.u32().ok_or(Bad)?;
                    let _max_msg = r.u16().ok_or(Bad)?;
                    let sign_pk = r.arr::<32>().ok_or(Bad)?;
                    self.peer_sign_pk = sign_pk;
                    let nl = r.u8().ok_or(Bad)? as usize;
                    let nick = r.take(nl).filter(|n| n.len() <= MAX_NICK && core::str::from_utf8(n).is_ok()).ok_or(Bad)?;
                    // The SAS is always mandatory for an identity transfer (§10.4).
                    let sas_optional = !self.transfer && self.scanned && self.peer_caps & caps::SCANNED != 0;
                    sink(Event::Hello { nick, sas_optional, sign_pk });
                }
                rtype::CHAT => {
                    let seq = r.u64().ok_or(Bad)?;
                    let ttl_s = if rec.rflags & rflags::TTL != 0 { r.u32().ok_or(Bad)? } else { 0 };
                    if !TTL_CHOICES.contains(&ttl_s) {
                        return Err(Bad);
                    }
                    let reply = if rec.rflags & rflags::REPLY != 0 {
                        let who = r.u8().ok_or(Bad)?;
                        let s = r.u64().ok_or(Bad)?;
                        if who as usize >= MAX_MEMBERS {
                            return Err(Bad);
                        }
                        Some(MsgRef { sender: who, seq: s })
                    } else {
                        None
                    };
                    let text = r.take(r.remaining()).unwrap_or(&[]);
                    if text.len() > MAX_TEXT {
                        return Err(ErrorCode::MessageTooLarge);
                    }
                    if core::str::from_utf8(text).is_err() {
                        return Err(Bad);
                    }
                    // Observers are read-only; in a room only the owner sets the timer (§11.7). Such
                    // records are acknowledged (no resends) but dropped.
                    let setting = rec.rflags & rflags::SETTING != 0;
                    let permitted = !observer && (!setting || !in_room || peer_owner);
                    if seq > self.chat_rx {
                        self.chat_rx = seq;
                        if !permitted {
                        } else if setting {
                            self.chat_ttl_s = ttl_s;
                            sink(Event::Setting { ttl_s, by: peer });
                        } else {
                            let msg = MsgRef { sender: peer, seq };
                            if ttl_s != 0
                                && let Some(old) = self.peer_timers.add(msg, ttl_s)
                            {
                                sink(Event::Expired(old));
                            }
                            self.peer_typing(false, sink);
                            sink(Event::Chat { msg, text, ttl_s, reply });
                        }
                    }
                    ack_upto = Some(self.chat_rx);
                }
                rtype::ACK => {
                    let seq = r.u64().ok_or(Bad)?;
                    if !self.pending.ack(seq) {
                        return Err(Bad);
                    }
                    if self.peer_caps & caps::READ == 0 {
                        self.own_timers.start_upto(seq, now_ms);
                    }
                    sink(Event::Delivered { seq });
                }
                rtype::READ => {
                    let seq = r.u64().ok_or(Bad)?;
                    self.own_timers.start_upto(seq, now_ms);
                    if self.settings.read_receipts {
                        sink(Event::Read { seq });
                    }
                }
                rtype::TYPING => {
                    let on = r.u8().ok_or(Bad)? != 0;
                    if self.settings.typing && !observer {
                        self.peer_typing(on, sink);
                        if on {
                            self.peer_typing_until = now_ms + TYPING_CLEAR_MS;
                        }
                    }
                }
                rtype::EDIT => {
                    let seq = r.u64().ok_or(Bad)?;
                    let text = r.take(r.remaining()).unwrap_or(&[]);
                    if text.is_empty() || text.len() > MAX_TEXT || core::str::from_utf8(text).is_err() {
                        return Err(Bad);
                    }
                    if seq <= self.chat_rx && !observer {
                        sink(Event::Edited { msg: MsgRef { sender: peer, seq }, text });
                    }
                }
                rtype::DELETE => {
                    let who = r.u8().ok_or(Bad)?;
                    let seq = r.u64().ok_or(Bad)?;
                    // Only the author deletes for everyone, or the room owner (moderation, §11.7);
                    // anything else, and anything from an observer, is dropped.
                    let own = who == peer && seq <= self.chat_rx;
                    let moderation = peer_owner && (who as usize) < MAX_MEMBERS;
                    if !observer && (own || moderation) {
                        if own {
                            self.peer_timers.remove(seq);
                        }
                        sink(Event::Deleted(MsgRef { sender: who, seq }));
                    }
                }
                rtype::REACT => {
                    let who = r.u8().ok_or(Bad)?;
                    let seq = r.u64().ok_or(Bad)?;
                    let n = r.u8().ok_or(Bad)? as usize;
                    let emoji = r.take(n).filter(|e| e.len() <= MAX_REACTION && core::str::from_utf8(e).is_ok()).ok_or(Bad)?;
                    let msg = MsgRef { sender: who, seq };
                    // In a room a reaction may target any member's message (the room layer knows them).
                    let known = if who == me {
                        seq <= self.pending.last()
                    } else if who == peer {
                        seq <= self.chat_rx
                    } else {
                        in_room && (who as usize) < MAX_MEMBERS
                    };
                    if known && seq != 0 && !observer {
                        sink(Event::Reaction { msg, emoji, by: peer });
                    }
                }
                rtype::ROOM_STATE | rtype::ROOM_SIGNAL | rtype::ROOM_LEAVE => {
                    if self.code_flags & flags::GROUP == 0 {
                        return Err(Bad);
                    }
                    sink(Event::Room { rtype: rec.rtype, body: rec.body });
                }
                rtype::SIGNAL_OFFER | rtype::SIGNAL_ANSWER => {
                    let n = r.u8().ok_or(Bad)?;
                    let ice = IceParams::decode_body(&mut r, n).ok_or(Bad)?;
                    sink(if rec.rtype == rtype::SIGNAL_OFFER { Event::SignalOffer(ice) } else { Event::SignalAnswer(ice) });
                }
                rtype::IDENTITY_READY => {
                    // Only the sending (old) device waits for this.
                    if !self.transfer || self.role_is_receiver() {
                        return Err(ErrorCode::NotPermitted);
                    }
                    self.peer_ready = true;
                    sink(Event::PeerReady);
                    send_identity = self.sas_confirmed && !self.xfer.is_empty();
                }
                rtype::IDENTITY_CHUNK => {
                    // Only on a transfer link, to the receiver, after its user confirmed the SAS.
                    if !self.transfer || !self.role_is_receiver() || !self.sas_confirmed {
                        return Err(ErrorCode::NotPermitted);
                    }
                    let idx = r.u16().ok_or(Bad)?;
                    let total = r.u16().ok_or(Bad)?;
                    let data = r.take(r.remaining()).unwrap_or(&[]);
                    let max_chunks = MAX_IDENTITY.div_ceil(IDENTITY_CHUNK) as u16;
                    if idx != self.xfer_next || total == 0 || total > max_chunks || (idx > 0 && total != self.xfer_total) || data.len() > IDENTITY_CHUNK {
                        return Err(Bad);
                    }
                    if idx == 0 {
                        self.xfer = Vec::with_capacity(total as usize * IDENTITY_CHUNK);
                        self.xfer_total = total;
                    }
                    self.xfer.extend_from_slice(data);
                    self.xfer_next += 1;
                    if self.xfer_next == total {
                        sink(Event::IdentityReceived(&self.xfer));
                    }
                }
                rtype::PING => {
                    let t = r.u64().ok_or(Bad)?;
                    let hidden = r.u8().unwrap_or(0) != 0;
                    if hidden != self.hidden {
                        self.hidden = hidden;
                        sink(Event::PeerHidden(hidden));
                    }
                    pong = Some(t);
                }
                rtype::PONG => {
                    let t = r.u64().ok_or(Bad)?;
                    self.app_rtt_ms = now_ms.saturating_sub(t).min(u32::MAX as u64) as u32;
                }
                rtype::GOODBYE => {
                    r.u16().ok_or(Bad)?;
                    self.close_local();
                    sink(Event::Closed(ErrorCode::PeerOffline));
                    return Ok(());
                }
                rtype::REKEY => rekey = true,
                _ if rec.rflags & rflags::IGNORABLE != 0 => {}
                _ => return Err(Bad),
            }
        }
        if rekey {
            self.tr.as_mut().ok_or(ErrorCode::CryptoFailed)?.rekey_rx();
            self.rekeys += 1;
        }
        if send_identity {
            self.send_identity_chunks(now_ms, sink)?;
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

    /// Peer typing indicator; `peer_typing_until` is the clear deadline (0 = not typing).
    fn peer_typing(&mut self, on: bool, sink: &mut impl FnMut(Event<'_>)) {
        let was = self.peer_typing_until != 0;
        if !on {
            self.peer_typing_until = 0;
        } else if !was {
            self.peer_typing_until = u64::MAX;
        }
        if on != was {
            sink(Event::PeerTyping(on));
        }
    }

    fn send_hello(&mut self, now_ms: u64, sink: &mut impl FnMut(Event<'_>)) -> Result<(), ErrorCode> {
        let sign_pk = self.sign_pk;
        let nick = self.settings.nick;
        let nick_len = self.settings.nick_len as usize;
        let c = caps::RESUME
            | if self.settings.read_receipts { caps::READ } else { 0 }
            | if self.settings.typing { caps::TYPING } else { 0 }
            | if self.scanned { caps::SCANNED } else { 0 };
        self.send_records(now_ms, sink, |b| {
            let mut body = [0u8; 2 + 4 + 2 + 32 + 1 + MAX_NICK];
            let mut w = Buf::new(&mut body);
            w.u8(ephem_proto::VERSION)?;
            w.u8(ephem_proto::VERSION)?;
            w.u32(c)?;
            w.u16(MAX_TEXT as u16)?;
            w.put(&sign_pk)?;
            w.u8(nick_len as u8)?;
            w.put(&nick[..nick_len])?;
            let n = w.len();
            frame::write_record(b, rtype::HELLO, 0, &body[..n])
        })
    }

    /// Builds records into the TX buffer, seals in place and emits `Send`. No allocation.
    fn send_records(&mut self, now_ms: u64, sink: &mut impl FnMut(Event<'_>), fill: impl FnOnce(&mut Buf<'_>) -> Result<(), ()>) -> Result<(), ErrorCode> {
        let tr = self.tr.as_mut().ok_or(ErrorCode::PeerOffline)?;
        let n = {
            let mut b = Buf::new(&mut self.tx[HEADER_LEN..HEADER_LEN + MAX_PLAIN]);
            fill(&mut b).map_err(too_large)?;
            b.len()
        };
        let len = tr.seal(&mut self.tx, n)?;
        self.last_tx_ms = now_ms;
        sink(Event::Send(&self.tx[..len]));
        Ok(())
    }

    #[inline(always)]
    fn connected(&self) -> bool {
        self.state == State::Connected
    }

    // ---- user actions ----

    fn queue(&mut self, now_ms: u64, h: OutHeader, text: &[u8], sink: &mut impl FnMut(Event<'_>)) -> Result<u64, ErrorCode> {
        let OutHeader { at, rf, ttl_s, reply } = h;
        if self.transfer || self.me_observer() {
            return Err(ErrorCode::NotPermitted);
        }
        if !self.ever_connected || self.state == State::Closed {
            return Err(ErrorCode::PeerOffline);
        }
        let seq = match at {
            Some(seq) => self.pending.push_at(seq, rf, ttl_s, reply, text).ok_or(ErrorCode::Backpressure)?.seq,
            None if self.pending.is_full() => return Err(ErrorCode::Backpressure),
            None => self.pending.push(rf, ttl_s, reply, text).seq,
        };
        if self.connected() {
            self.transmit_slot(seq, now_ms, sink)?;
        }
        Ok(seq)
    }

    /// Sends (or queues while the path is down) one chat message with the chat's current
    /// self-destruct timer; returns its `chat_seq`. `at`: a room gives every link the same
    /// sequence number (§14.3); `None` takes the next one of this link.
    pub fn send_chat(&mut self, now_ms: u64, at: Option<u64>, text: &[u8], reply: Option<MsgRef>, sink: &mut impl FnMut(Event<'_>)) -> Result<u64, ErrorCode> {
        if text.is_empty() || text.len() > MAX_TEXT {
            return Err(ErrorCode::MessageTooLarge);
        }
        if core::str::from_utf8(text).is_err() {
            return Err(ErrorCode::ProtocolMismatch);
        }
        let ttl = self.chat_ttl_s;
        let rf = if ttl != 0 { rflags::TTL } else { 0 } | if reply.is_some() { rflags::REPLY } else { 0 };
        let seq = self.queue(now_ms, OutHeader { at, rf, ttl_s: ttl, reply: reply.unwrap_or(MsgRef::NONE) }, text, sink)?;
        if ttl != 0
            && let Some(old) = self.own_timers.add(MsgRef { sender: self.me_idx, seq }, ttl)
        {
            sink(Event::Expired(old));
        }
        self.typing_sent = false;
        Ok(seq)
    }

    /// Sets the chat's self-destruct timer (either person in 1:1, only the owner in a room) and
    /// notifies the peer. `at` as for [`Self::send_chat`].
    pub fn set_ttl(&mut self, now_ms: u64, at: Option<u64>, ttl_s: u32, sink: &mut impl FnMut(Event<'_>)) -> Result<u64, ErrorCode> {
        if !TTL_CHOICES.contains(&ttl_s) || self.room.is_some_and(|r| r.my_role != RoomRole::Owner) {
            return Err(ErrorCode::NotPermitted);
        }
        let rf = rflags::SETTING | if ttl_s != 0 { rflags::TTL } else { 0 };
        let seq = self.queue(now_ms, OutHeader { at, rf, ttl_s, reply: MsgRef::NONE }, &[], sink)?;
        self.chat_ttl_s = ttl_s;
        Ok(seq)
    }

    /// Room member: adopts the room's timer set by the owner on another link (§14.3), so every
    /// link stamps our messages with the same TTL. Nothing is sent.
    pub fn apply_ttl(&mut self, ttl_s: u32) {
        debug_assert!(TTL_CHOICES.contains(&ttl_s));
        self.chat_ttl_s = ttl_s;
    }

    /// Edits one of our messages. A message still pending is rewritten in place, so the peer
    /// only ever receives the final version (§11.3).
    pub fn edit(&mut self, now_ms: u64, seq: u64, text: &[u8], sink: &mut impl FnMut(Event<'_>)) -> Result<(), ErrorCode> {
        if text.is_empty() || text.len() > MAX_TEXT || core::str::from_utf8(text).is_err() {
            return Err(ErrorCode::MessageTooLarge);
        }
        if self.me_observer() {
            return Err(ErrorCode::NotPermitted);
        }
        if seq == 0 || seq > self.pending.last() {
            return Err(ErrorCode::NotPermitted);
        }
        let connected = self.connected();
        if let Some(slot) = self.pending.get_mut(seq) {
            if slot.deleted || slot.rflags & rflags::SETTING != 0 {
                return Err(ErrorCode::NotPermitted);
            }
            let old = slot.len as usize;
            slot.text[..text.len()].copy_from_slice(text);
            if old > text.len() {
                slot.text[text.len()..old].fill(0);
            }
            slot.len = text.len() as u16;
            if !connected {
                return Ok(());
            }
        } else if !connected {
            return Err(ErrorCode::PeerOffline);
        }
        self.send_records(now_ms, sink, |b| {
            b.u8(rtype::EDIT)?;
            b.u8(0)?;
            b.u16((8 + text.len()) as u16)?;
            b.u64(seq)?;
            b.put(text)
        })
    }

    /// Deletes a message. Ours: for everyone (a pending one is simply never sent). Someone
    /// else's: for me only (nothing is sent), or for everyone by the room owner (moderation).
    pub fn delete(&mut self, now_ms: u64, msg: MsgRef, sink: &mut impl FnMut(Event<'_>)) -> Result<(), ErrorCode> {
        if msg.sender != self.me_idx {
            if self.room.is_some_and(|r| r.my_role == RoomRole::Owner) {
                if !self.connected() {
                    return Err(ErrorCode::PeerOffline);
                }
                let mut body = [0u8; 9];
                body[0] = msg.sender;
                body[1..].copy_from_slice(&msg.seq.to_le_bytes());
                return self.send_records(now_ms, sink, |b| frame::write_record(b, rtype::DELETE, 0, &body));
            }
            if msg.sender == self.peer_idx {
                self.peer_timers.remove(msg.seq);
            }
            return Ok(());
        }
        if msg.seq == 0 || msg.seq > self.pending.last() {
            return Err(ErrorCode::NotPermitted);
        }
        let connected = self.connected();
        self.own_timers.remove(msg.seq);
        if let Some(slot) = self.pending.get_mut(msg.seq) {
            slot.deleted = true;
            let len = slot.len as usize;
            slot.text[..len].fill(0);
            slot.len = 0;
            if !connected {
                return Ok(());
            }
        } else if !connected {
            return Err(ErrorCode::PeerOffline);
        }
        let me = self.me_idx;
        self.send_records(now_ms, sink, |b| {
            let mut body = [0u8; 9];
            body[0] = me;
            body[1..].copy_from_slice(&msg.seq.to_le_bytes());
            frame::write_record(b, rtype::DELETE, 0, &body)
        })
    }

    /// Reacts to a message (ours or the peer's); an empty `emoji` removes our reaction.
    pub fn react(&mut self, now_ms: u64, msg: MsgRef, emoji: &[u8], sink: &mut impl FnMut(Event<'_>)) -> Result<(), ErrorCode> {
        if emoji.len() > MAX_REACTION || core::str::from_utf8(emoji).is_err() {
            return Err(ErrorCode::MessageTooLarge);
        }
        let known = if msg.sender == self.me_idx {
            msg.seq <= self.pending.last()
        } else if msg.sender == self.peer_idx {
            msg.seq <= self.chat_rx
        } else {
            self.room.is_some() && (msg.sender as usize) < MAX_MEMBERS
        };
        if self.transfer || self.me_observer() || msg.seq == 0 || !known {
            return Err(ErrorCode::NotPermitted);
        }
        if !self.connected() {
            return Err(ErrorCode::PeerOffline);
        }
        let who = msg.sender;
        self.send_records(now_ms, sink, |b| {
            b.u8(rtype::REACT)?;
            b.u8(0)?;
            b.u16((1 + 8 + 1 + emoji.len()) as u16)?;
            b.u8(who)?;
            b.u64(msg.seq)?;
            b.u8(emoji.len() as u8)?;
            b.put(emoji)
        })
    }

    /// The user confirmed that the SAS matches (§10.4). On an identity transfer (§7.6) the
    /// receiver tells the sender; the sender passes the encrypted key file to send, which goes
    /// out once both sides have confirmed.
    pub fn confirm_sas(&mut self, now_ms: u64, identity: Option<&[u8]>, sink: &mut impl FnMut(Event<'_>)) -> Result<(), ErrorCode> {
        if !self.connected() {
            return Err(ErrorCode::PeerOffline);
        }
        self.sas_confirmed = true;
        if !self.transfer {
            return Ok(());
        }
        if self.role_is_receiver() {
            return self.send_records(now_ms, sink, |b| frame::write_record(b, rtype::IDENTITY_READY, 0, &[]));
        }
        let blob = identity.ok_or(ErrorCode::NotPermitted)?;
        if blob.is_empty() || blob.len() > MAX_IDENTITY {
            return Err(ErrorCode::KeyfileInvalid);
        }
        self.xfer = blob.to_vec();
        if self.peer_ready {
            self.send_identity_chunks(now_ms, sink)?;
        }
        Ok(())
    }

    /// The receiver of a transfer is the device that made the TRANSFER invite (§7.6).
    #[inline]
    fn role_is_receiver(&self) -> bool {
        self.code_flags & flags::TRANSFER != 0 && self.me_idx == 0
    }

    fn send_identity_chunks(&mut self, now_ms: u64, sink: &mut impl FnMut(Event<'_>)) -> Result<(), ErrorCode> {
        let mut blob = core::mem::take(&mut self.xfer);
        let total = blob.len().div_ceil(IDENTITY_CHUNK) as u16;
        for (i, chunk) in blob.chunks(IDENTITY_CHUNK).enumerate() {
            self.send_records(now_ms, sink, |b| {
                b.u8(rtype::IDENTITY_CHUNK)?;
                b.u8(0)?;
                b.u16((4 + chunk.len()) as u16)?;
                b.u16(i as u16)?;
                b.u16(total)?;
                b.put(chunk)
            })?;
        }
        blob.fill(0);
        sink(Event::IdentitySent);
        Ok(())
    }

    /// Sends a room record (§14) on this link.
    pub fn send_room(&mut self, now_ms: u64, rt: u8, body: &[u8], sink: &mut impl FnMut(Event<'_>)) -> Result<(), ErrorCode> {
        if self.code_flags & flags::GROUP == 0 || !matches!(rt, rtype::ROOM_STATE | rtype::ROOM_SIGNAL | rtype::ROOM_LEAVE) {
            return Err(ErrorCode::NotPermitted);
        }
        if !self.connected() {
            return Err(ErrorCode::PeerOffline);
        }
        self.send_records(now_ms, sink, |b| frame::write_record(b, rt, 0, body))
    }

    /// The user has seen the peer's messages up to `seq` (on screen, page visible).
    /// Starts their self-destruct countdowns and sends a coalesced READ if enabled.
    pub fn mark_read(&mut self, now_ms: u64, seq: u64, sink: &mut impl FnMut(Event<'_>)) {
        let seq = seq.min(self.chat_rx);
        if seq <= self.read_upto {
            return;
        }
        self.read_upto = seq;
        self.peer_timers.start_upto(seq, now_ms);
        if let Err(e) = self.maybe_send_read(now_ms, sink) {
            self.fail(e, sink);
        }
    }

    fn maybe_send_read(&mut self, now_ms: u64, sink: &mut impl FnMut(Event<'_>)) -> Result<(), ErrorCode> {
        if !self.settings.read_receipts || !self.connected() || self.read_upto <= self.read_sent || now_ms.saturating_sub(self.read_sent_ms) < READ_EVERY_MS {
            return Ok(());
        }
        let upto = self.read_upto;
        self.send_records(now_ms, sink, |b| frame::write_record(b, rtype::READ, 0, &upto.to_le_bytes()))?;
        self.read_sent = upto;
        self.read_sent_ms = now_ms;
        Ok(())
    }

    /// Composer activity. Sends TYPING at most every 3 s while typing, and 0 when it stops.
    pub fn typing(&mut self, now_ms: u64, active: bool, sink: &mut impl FnMut(Event<'_>)) {
        if !self.settings.typing || !self.connected() || self.me_observer() {
            return;
        }
        let send = if active { !self.typing_sent || now_ms.saturating_sub(self.typing_sent_ms) >= TYPING_EVERY_MS } else { self.typing_sent };
        if !send {
            return;
        }
        let v = active as u8;
        if let Err(e) = self.send_records(now_ms, sink, |b| frame::write_record(b, rtype::TYPING, 0, &[v])) {
            self.fail(e, sink);
            return;
        }
        self.typing_sent = active;
        self.typing_sent_ms = now_ms;
    }

    /// Timer input (driven by the adapter, e.g. every 1 s). `page_hidden` is sent in PING.
    pub fn tick(&mut self, now_ms: u64, page_hidden: bool, sink: &mut impl FnMut(Event<'_>)) {
        self.own_timers.expire(now_ms, |m| sink(Event::Expired(m)));
        self.peer_timers.expire(now_ms, |m| sink(Event::Expired(m)));
        match self.state {
            State::AwaitingAnswer if now_ms / 1000 > self.expires_at as u64 => {
                if self.ever_connected {
                    // An unanswered resume invite: back to waiting for another one.
                    self.state = State::Suspended;
                    sink(Event::Suspended);
                } else {
                    self.fail(ErrorCode::ExpiredInvite, sink);
                }
                return;
            }
            State::Suspended if now_ms.saturating_sub(self.suspended_at_ms) >= SUSPEND_GRACE_MS => {
                self.close_local();
                sink(Event::Closed(ErrorCode::PeerOffline));
                return;
            }
            State::Connected => {}
            _ => return,
        }
        if self.peer_typing_until != 0 && now_ms >= self.peer_typing_until {
            self.peer_typing(false, sink);
        }
        if now_ms.saturating_sub(self.last_rx_ms) >= DEGRADED_AFTER_MS && !self.degraded {
            self.degraded = true;
            sink(Event::Degraded);
        }
        if let Err(e) = self.maybe_send_read(now_ms, sink) {
            self.fail(e, sink);
            return;
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
                self.rekeys += 1;
                self.rekey_at_ms = now_ms + REKEY_MS;
            }
            Ok(()) => {}
            Err(e) => self.fail(e, sink),
        }
    }

    /// Leaves the chat: sends GOODBYE if connected, then wipes keys and pending messages.
    pub fn close(&mut self, now_ms: u64, sink: &mut impl FnMut(Event<'_>)) {
        if self.connected() {
            let code = ErrorCode::PeerOffline.code().to_le_bytes();
            let _ = self.send_records(now_ms, sink, |b| frame::write_record(b, rtype::GOODBYE, 0, &code));
        }
        self.close_local();
    }

    /// Ends the chat because of `e` (reported to the peer when possible).
    pub fn abort(&mut self, e: ErrorCode, sink: &mut impl FnMut(Event<'_>)) {
        self.fail(e, sink);
    }

    fn close_local(&mut self) {
        self.state = State::Closed;
        self.drop_path();
        self.xfer.fill(0);
        self.xfer.clear();
        self.pending.clear();
        self.own_timers = Timers::new();
        self.peer_timers = Timers::new();
        self.tx.fill(0);
    }

    fn fail(&mut self, e: ErrorCode, sink: &mut impl FnMut(Event<'_>)) {
        if self.state == State::Closed {
            return;
        }
        if self.connected() {
            let code = e.code().to_le_bytes();
            let _ = self.send_records(0, sink, |b| frame::write_record(b, rtype::GOODBYE, 0, &code));
        }
        self.close_local();
        sink(Event::Closed(e));
    }
}

#[cfg(test)]
mod tests;
