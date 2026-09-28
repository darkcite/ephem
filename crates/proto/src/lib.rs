//! Ephem wire formats (docs/P2P-CHAT.md §8, §11, Appendix A).
//!
//! `no_std`, no allocation: every encoder writes into a caller-provided buffer and every
//! decoder borrows the input. Parsing is in place.
#![no_std]
// Overflow of a caller buffer is the only failure of the encoders: a zero-size error is enough.
#![allow(clippy::result_unit_err)]

pub mod b64url;
pub mod buf;
pub mod candidate;
pub mod code;
pub mod error;
pub mod frame;
pub mod sdp;

pub use error::ErrorCode;

/// Protocol major version carried by every code and frame.
pub const VERSION: u8 = 1;
