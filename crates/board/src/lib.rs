// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
// Copyright 2026 Anton (darkcite)
//! Boards (docs/BOARDS.md, Appendix G): a 4chan-like channel type. Anyone holding the link
//! posts anonymously; the owner's tab is the single writer that numbers, bumps, prunes,
//! moderates and signs the result as one IPNS record over dag-cbor blocks.
//!
//! - [`post`]: what a poster signs (`s`), with one signing prefix per kind (G.4).
//! - [`board`]: the owner's model and its blocks (G.5).
//! - [`verify`]: a reader checks a record and its blocks (G.5.1 reader rules, G.5.3).
//! - [`pow`], [`submit`], [`pipeline`]: the proof of work, the submit format and the host's
//!   intake (G.6, G.8), sans-IO.
//! - [`gateway`], [`host`]: the board's onion as bytes (G.6.1, G.11.1) and the single writer
//!   that publishes (G.3, G.5.3), sans-IO; the browser's loops live in `crates/channel-web`.
//!
//! Text only (v1); images are v2 (G.7). The channel crate's encoders (`cbor`, `cid`, `car`,
//! `ipns`) are reused unchanged; `ephem_channel::channel` is not touched.

pub mod board;
pub mod gateway;
pub mod host;
pub mod pipeline;
pub mod post;
pub mod pow;
pub mod submit;
pub mod verify;

/// Limits (G.5.2): defaults equal the maxima unless noted.
pub mod limits {
    /// Post body, UTF-8 bytes.
    pub const BODY: usize = 2_000;
    /// Thread subject, UTF-8 bytes.
    pub const SUBJECT: usize = 100;
    /// Live threads (10 pages × 15, B4).
    pub const THREADS: usize = 150;
    /// Replies that still bump their thread.
    pub const BUMP_LIMIT: usize = 300;
    /// Posts in a thread (OP included); then it is locked.
    pub const THREAD_POSTS: usize = 500;
    /// Posts per chunk block; a full chunk never changes unless a post in it is deleted.
    pub const CHUNK: usize = 64;
    /// Catalog buckets (`no mod 10`); readers sort by bump.
    pub const BUCKETS: usize = 10;
    /// Characters of the OP body in the catalog, in bytes (cut on a UTF-8 boundary).
    pub const EXCERPT: usize = 140;
    /// Archived (text-only) threads and how long they stay.
    pub const ARCHIVE: usize = 256;
    pub const ARCHIVE_S: u64 = 7 * 24 * 3600;
    /// Deletion-list entries and how long they stay (longer than any record's validity).
    pub const DELS: usize = 4_096;
    pub const DELS_S: u64 = 30 * 24 * 3600;
    /// Moderation log entries.
    pub const MODLOG: usize = 1_024;
    /// Prune protection (G.8): a thread this young, or bumped this recently, is never pruned to
    /// make room for a new thread; the new thread is refused instead.
    pub const PROTECT_AGE_S: u64 = 30 * 60;
    pub const PROTECT_BUMP_S: u64 = 10 * 60;
    /// The encrypted owner-state block (bans, filters, efforts, held posts), padded by the caller.
    pub const OWN: usize = 64 * 1024;
    /// Record validity (R1) and TTL.
    pub const VALIDITY_S: u64 = 72 * 3600;
    pub const TTL_NS: u64 = 60_000_000_000;
    /// A record whose sequence is further ahead of the reader's clock is refused (G.5.3).
    pub const FUTURE_MS: u64 = 3_600_000;
    pub const TITLE: usize = 128;
    pub const ABOUT: usize = 1_024;
    pub const RULES: usize = 2_048;
    pub const MIRRORS: usize = 8;
    pub const SEE_ALSO: usize = 16;
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum BoardError {
    /// Text over a limit, or a field out of range.
    TooLong,
    /// The thread or post does not exist (or is pruned).
    NotFound,
    /// The thread is locked, or the board refuses this kind of post now.
    Refused,
    /// No room: the board is full of protected threads.
    Busy,
    /// The same body was posted in this thread recently.
    Duplicate,
    /// A signature does not verify.
    BadSignature,
    /// A record that does not verify (`ephem_channel::ipns`), or one too far in the future.
    Record,
    /// A block is missing, malformed, or breaks a reader rule.
    Invalid,
}
