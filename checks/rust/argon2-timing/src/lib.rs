// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
// Copyright 2026 Anton (darkcite)
#[unsafe(no_mangle)]
pub extern "C" fn kdf(m_kib: u32, t: u32) -> u32 {
    use argon2::{Argon2, Params, Algorithm, Version};
    let a = Argon2::new(Algorithm::Argon2id, Version::V0x13, Params::new(m_kib, t, 1, Some(32)).unwrap());
    let mut k = [0u8; 32];
    a.hash_password_into(b"correct horse battery", b"saltsaltsaltsalt", &mut k).unwrap();
    k[0] as u32
}
