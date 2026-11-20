//! Differential acceptance checks against the pinned core's public verifier.

use std::error::Error;

use bittensor_core::keys::verify_strict;
use bittensor_core::keys::{self, Keypair, CRYPTO_ED25519, CRYPTO_SR25519};

#[test]
fn raw_signatures_verify_and_wrapped_signatures_are_rejected() -> Result<(), Box<dyn Error>> {
    let payload = b"btauth/1\nPOST\n/submit?task=1\nbody\n123\nsender\n-";
    for scheme in [CRYPTO_SR25519, CRYPTO_ED25519] {
        let key = Keypair::from_seed(&[7; 32], scheme)?;
        let address = key.ss58_address();
        let signature = key.sign(payload)?;
        assert!(verify_strict(payload, &signature, &address, scheme)?);
        assert!(keys::verify(payload, &signature, &address, scheme)?);
        assert!(!verify_strict(
            b"changed message",
            &signature,
            &address,
            scheme
        )?);
        let wrong_key = Keypair::from_seed(&[8; 32], scheme)?;
        assert!(!verify_strict(
            payload,
            &signature,
            &wrong_key.ss58_address(),
            scheme
        )?);
        let wrapped = [
            b"<Bytes>".as_slice(),
            payload.as_slice(),
            b"</Bytes>".as_slice(),
        ]
        .concat();
        let wrapped_signature = key.sign(&wrapped)?;
        assert!(keys::verify(payload, &wrapped_signature, &address, scheme)?);
        assert!(!verify_strict(
            payload,
            &wrapped_signature,
            &address,
            scheme
        )?);
        assert!(verify_strict(
            &wrapped,
            &wrapped_signature,
            &address,
            scheme
        )?);
    }
    Ok(())
}

#[test]
fn malformed_encoding_matches_the_core_error_boundary() -> Result<(), Box<dyn Error>> {
    for scheme in [CRYPTO_SR25519, CRYPTO_ED25519] {
        let key = Keypair::from_seed(&[9; 32], scheme)?;
        let address = key.ss58_address();
        for signature in [vec![0; 63], vec![0; 65]] {
            let strict = verify_strict(b"message", &signature, &address, scheme)
                .err()
                .ok_or("strict accepted wrong length")?;
            let upstream = keys::verify(b"message", &signature, &address, scheme)
                .err()
                .ok_or("core accepted wrong length")?;
            assert_eq!(strict.to_string(), upstream.to_string());
        }
        assert!(!verify_strict(b"message", &[0; 64], &address, scheme)?);
        assert!(verify_strict(b"message", &[0; 64], "bad SS58", scheme).is_err());
    }
    let address = keys::ss58_from_public([255; 32], 42);
    assert!(!verify_strict(
        b"message",
        &[0; 64],
        &address,
        CRYPTO_SR25519
    )?);
    let address = Keypair::from_seed(&[9; 32], CRYPTO_ED25519)?.ss58_address();
    assert!(verify_strict(b"message", &[0; 64], &address, 255).is_err());
    Ok(())
}
