//! Snowflake client transport for Tor mode (docs/P2P-CHAT.md §28.3, Appendix C.5), sans-IO.
//!
//! The byte stream to the Snowflake bridge is: Tor link protocol → [`smux`] v2 stream →
//! [`kcp`] (reliable, ordered) → [`encap`] packets → Turbotunnel ([`session`]) over WebRTC
//! DataChannels to volunteer proxies. The browser adapter owns the DataChannels and the timer;
//! everything here is deterministic, allocation-free after construction, and tested natively,
//! including against the reference Go snowflake server (`tests/interop.rs`).

pub mod encap;
pub mod kcp;
pub mod session;
pub mod smux;

pub use session::{Error, Session};
