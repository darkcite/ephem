//! Ephem cryptography (docs/P2P-CHAT.md §7.1, §10).
//!
//! - [`identity`]: 32-byte seed → X25519 static (Noise) + Ed25519 signing key, `PeerId`, handle.
//! - [`noise`]: Noise_KK handshake (snow, setup only) and the in-place transport cipher
//!   (ChaCha20-Poly1305 over the raw split keys, frame header as AAD, nonce = frame `seq`).
//! - [`sas`]: short authentication string from the handshake hash.
//! - [`keyfile`]: encrypted identity key file (Argon2id + XChaCha20-Poly1305).

pub mod contacts;
pub mod identity;
pub mod keyfile;
pub mod noise;
pub mod sas;

pub use identity::{Identity, PeerId};

/// Fills `out` from the platform CSPRNG (`crypto.getRandomValues` in the browser).
/// A missing CSPRNG is fatal: nothing in this app is safe without it.
#[inline]
pub fn random(out: &mut [u8]) {
    getrandom::fill(out).expect("CSPRNG unavailable");
}
