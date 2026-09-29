//! Ephem Tor mode (docs/P2P-CHAT.md §28, Appendix C.5): arti inside the web app, reaching Tor
//! only through Snowflake.
//!
//! - [`stream`]: the Snowflake session as the byte stream arti takes for a TCP connection.
//! - [`net`]: the transport guard: the only address that can be "dialled" is the bridge.
//! - [`tls`]: TLS for Tor channels (rustls + ring, Tor's certificate policy).
//! - [`bridge`]: Tor bridge lines (Tor Browser format) → the Snowflake setup (Appendix F.2).
//! - [`config`]: arti configuration (bridge, in-memory storage, lab network).
//! - `web` (wasm32 only): browser runtime, Snowflake carrier (broker + DataChannels) and the
//!   JS API.

pub mod bridge;
pub mod config;
pub mod net;
pub mod stream;
pub mod tls;

#[cfg(all(target_arch = "wasm32", target_os = "unknown"))]
pub mod web;
