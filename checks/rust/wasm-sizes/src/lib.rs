#![allow(unused)]
// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
// Copyright 2026 Anton (darkcite)
static mut OUT: [u8; 4096] = [0; 4096];

#[unsafe(no_mangle)]
pub extern "C" fn probe(n: u32) -> u32 {
    let mut acc = n;
    #[cfg(feature = "noise")]
    {
        let p: snow::params::NoiseParams = "Noise_KK_25519_ChaChaPoly_BLAKE2s".parse().unwrap();
        let kp = snow::Builder::new(p.clone()).generate_keypair().unwrap();
        let kp2 = snow::Builder::new(p.clone()).generate_keypair().unwrap();
        let mut i = snow::Builder::new(p.clone()).local_private_key(&kp.private).unwrap()
            .remote_public_key(&kp2.public).unwrap().prologue(b"p2pchat/1").unwrap().build_initiator().unwrap();
        let mut r = snow::Builder::new(p).local_private_key(&kp2.private).unwrap()
            .remote_public_key(&kp.public).unwrap().prologue(b"p2pchat/1").unwrap().build_responder().unwrap();
        let mut b1 = [0u8; 256]; let mut b2 = [0u8; 256];
        let l = i.write_message(&[], &mut b1).unwrap(); r.read_message(&b1[..l], &mut b2).unwrap();
        let l = r.write_message(&[], &mut b1).unwrap(); i.read_message(&b1[..l], &mut b2).unwrap();
        acc ^= i.get_handshake_hash()[0] as u32;
    }
    #[cfg(feature = "keyfile")]
    {
        use argon2::{Argon2, Params, Algorithm, Version};
        let a = Argon2::new(Algorithm::Argon2id, Version::V0x13, Params::new(19 * 1024, 2, 1, Some(32)).unwrap());
        let mut k = [0u8; 32];
        a.hash_password_into(b"pass", b"saltsaltsaltsalt", &mut k).unwrap();
        use chacha20poly1305::{XChaCha20Poly1305, KeyInit, AeadInPlace};
        let c = XChaCha20Poly1305::new((&k).into());
        let mut buf = [0u8; 64];
        let _ = c.encrypt_in_place_detached((&[0u8; 24]).into(), b"", &mut buf);
        let hk = hkdf::SimpleHkdf::<blake2::Blake2s256>::new(None, &k);
        let mut o = [0u8; 32]; hk.expand(b"p2pchat/ed25519", &mut o).unwrap();
        let sk = ed25519_dalek::SigningKey::from_bytes(&o);
        use ed25519_dalek::Signer;
        let s = sk.sign(b"x");
        let xs = x25519_dalek::StaticSecret::from(o);
        acc ^= s.to_bytes()[0] as u32 ^ x25519_dalek::PublicKey::from(&xs).as_bytes()[0] as u32;
    }
    #[cfg(feature = "qr")]
    {
        let code = qrcode::QrCode::with_error_correction_level(&[7u8; 190][..], qrcode::EcLevel::M).unwrap();
        acc ^= code.width() as u32;
        let img = image::GrayImage::new(64, 64);
        let mut p = rqrr::PreparedImage::prepare(img);
        acc ^= p.detect_grids().len() as u32;
    }
    #[cfg(feature = "mls")]
    {
        use openmls::prelude::*;
        let provider = openmls_rust_crypto::OpenMlsRustCrypto::default();
        let cs = Ciphersuite::MLS_128_DHKEMX25519_AES128GCM_SHA256_Ed25519;
        let sig = openmls_basic_credential::SignatureKeyPair::new(cs.signature_algorithm()).unwrap();
        let cred = CredentialWithKey { credential: BasicCredential::new(b"a".to_vec()).into(), signature_key: sig.public().into() };
        let g = MlsGroup::builder().ciphersuite(cs).build(&provider, &sig, cred).unwrap();
        acc ^= g.epoch().as_u64() as u32;
    }
    acc
}
