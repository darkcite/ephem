//! Ephem Tor mode (docs/P2P-CHAT.md §28, Appendix C.5): arti inside the web app, reaching Tor
//! only through Snowflake.
//!
//! - [`stream`]: the Snowflake session as the byte stream arti takes for a TCP connection.
//! - [`net`]: the transport guard: the only address that can be "dialled" is the bridge.
//! - [`tls`]: TLS for Tor channels (rustls + ring, Tor's certificate policy).
//! - [`config`]: arti configuration (bridge, in-memory storage, lab network).
//! - `web` (wasm32 only): browser runtime, Snowflake carrier (broker + DataChannels) and the
//!   JS API.

pub mod config;
pub mod net;
pub mod stream;
pub mod tls;
