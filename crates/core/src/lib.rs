//! Ephem sans-IO core (docs/P2P-CHAT.md §6.2, §11, §12).
//!
//! No browser, no clock, no randomness of its own: the adapter feeds inputs (`now_ms`, local
//! SDP, received frames) and receives [`Event`]s through a generic sink. Deterministic, so it
//! is tested natively by running two sessions against each other.

pub mod session;

pub use session::{Event, Privacy, Role, Session, State};
