//! Stable error codes (§19). `u16` on the wire and in the UI.

#[repr(u16)]
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum ErrorCode {
    InvalidInvite = 0x0001,
    ExpiredInvite = 0x0002,
    InviteConsumed = 0x0003,
    AnswerMismatch = 0x0004,
    InvalidRoom = 0x0010,
    RoomFull = 0x0011,
    RoomDisposed = 0x0012,
    NotOwner = 0x0013,
    AuthFailed = 0x0020,
    CryptoFailed = 0x0021,
    SasRejected = 0x0022,
    DuplicateSession = 0x0023,
    NotAContact = 0x0024,
    IceFailed = 0x0030,
    NoDirectPath = 0x0031,
    RelayRejected = 0x0032,
    ConnectionTimeout = 0x0033,
    NetworkChanged = 0x0034,
    PeerOffline = 0x0035,
    TorUnavailable = 0x0036,
    ProtocolMismatch = 0x0040,
    MessageTooLarge = 0x0041,
    Backpressure = 0x0042,
    NotPermitted = 0x0043,
    BrowserUnsupported = 0x0050,
    KeyfileInvalid = 0x0060,
}

impl ErrorCode {
    #[inline(always)]
    pub const fn code(self) -> u16 {
        self as u16
    }

    /// Stable symbolic name used by the UI (`E_...`).
    pub const fn name(self) -> &'static str {
        match self {
            Self::InvalidInvite => "E_INVALID_INVITE",
            Self::ExpiredInvite => "E_EXPIRED_INVITE",
            Self::InviteConsumed => "E_INVITE_CONSUMED",
            Self::AnswerMismatch => "E_ANSWER_MISMATCH",
            Self::InvalidRoom => "E_INVALID_ROOM",
            Self::RoomFull => "E_ROOM_FULL",
            Self::RoomDisposed => "E_ROOM_DISPOSED",
            Self::NotOwner => "E_NOT_OWNER",
            Self::AuthFailed => "E_AUTH_FAILED",
            Self::CryptoFailed => "E_CRYPTO_FAILED",
            Self::SasRejected => "E_SAS_REJECTED",
            Self::DuplicateSession => "E_DUPLICATE_SESSION",
            Self::NotAContact => "E_NOT_A_CONTACT",
            Self::IceFailed => "E_ICE_FAILED",
            Self::NoDirectPath => "E_NO_DIRECT_PATH",
            Self::RelayRejected => "E_RELAY_REJECTED",
            Self::ConnectionTimeout => "E_CONNECTION_TIMEOUT",
            Self::NetworkChanged => "E_NETWORK_CHANGED",
            Self::PeerOffline => "E_PEER_OFFLINE",
            Self::TorUnavailable => "E_TOR_UNAVAILABLE",
            Self::ProtocolMismatch => "E_PROTOCOL_MISMATCH",
            Self::MessageTooLarge => "E_MESSAGE_TOO_LARGE",
            Self::Backpressure => "E_BACKPRESSURE",
            Self::NotPermitted => "E_NOT_PERMITTED",
            Self::BrowserUnsupported => "E_BROWSER_UNSUPPORTED",
            Self::KeyfileInvalid => "E_KEYFILE_INVALID",
        }
    }
}
