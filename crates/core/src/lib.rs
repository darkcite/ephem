// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
// Copyright 2026 Anton (darkcite)
//! Ephem sans-IO core (docs/P2P-CHAT.md §6.2, §11, §12, §13).
//!
//! No browser, no clock, no randomness of its own: the adapter feeds inputs (`now_ms`, local
//! SDP, received frames, user actions) and receives [`Event`]s through a generic sink.
//! Deterministic, so it is tested natively by running two sessions against each other.

pub mod messages;
pub mod room;
pub mod session;

pub use messages::{MsgRef, TTL_CHOICES};
pub use room::{Member, RoomRole, RoomState};
pub use session::{Diag, Event, Privacy, Role, RoomLink, Session, Settings, State};
