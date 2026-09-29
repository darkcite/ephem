// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
// Copyright 2026 Anton (darkcite)
//! Public channels (docs/P2P-CHAT.md §27, Appendix D): the data model, IPFS-native.
//!
//! Sans-IO and independent of the browser: blocks are dag-cbor (a deterministic CBOR subset),
//! addressed by CIDv1 (SHA2-256), shipped as CAR v1 files, and the channel's current root is
//! named by a signed IPNS V2 record whose key is the channel key. Everything a reader receives
//! is verified here: block hashes against their CIDs, the record's signature and validity, the
//! manifest's and every post's signature.
//!
//! Setup/UI path (a post is written by a human; a channel is read once per visit): owned
//! buffers are fine here, unlike the chat hot path.

pub mod car;
pub mod cbor;
pub mod channel;
pub mod cid;
pub mod gateway;
pub mod ipns;
pub mod time;
pub mod varint;

pub use channel::{Channel, ChannelError, Manifest, Post, View};
pub use car::{read as read_car, write as write_car};
pub use cid::Cid;
