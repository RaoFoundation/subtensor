#![cfg(feature = "host")]

use bittensor_core::keyfiles::{read_keypair_from_keyfile, save_keypair_pair_to_keyfiles};
use bittensor_core::keys::{
    Keypair, CRYPTO_ED25519, CRYPTO_FALCON1024, CRYPTO_FALCON1024_HASHED, CRYPTO_FALCON512,
    CRYPTO_FALCON512_HASHED, CRYPTO_HASHED, CRYPTO_HASHED_ED25519, CRYPTO_MLDSA65,
    CRYPTO_MLDSA65_FIXED, CRYPTO_SR25519,
};
use std::error::Error;
use std::fs;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

struct TestDirectory(PathBuf);

impl TestDirectory {
    fn new() -> Self {
        static SEQUENCE: AtomicU64 = AtomicU64::new(0);
        Self(std::env::temp_dir().join(format!(
            "bt-keyfile-pair-identity-{}-{}",
            std::process::id(),
            SEQUENCE.fetch_add(1, Ordering::Relaxed),
        )))
    }
}

impl Drop for TestDirectory {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

// PROTECTED_REGRESSION: a keyfile pair must not advertise a receiving account controlled by another private key; changes require explicit human approval.
#[test]
fn different_accounts_are_rejected_before_creating_or_replacing_keyfiles(
) -> Result<(), Box<dyn Error>> {
    let directory = TestDirectory::new();
    let private_path = directory.0.join("coldkey");
    let public_path = directory.0.join("coldkeypub.txt");
    let alice = Keypair::from_uri("//Alice", CRYPTO_SR25519)?;
    let bob = Keypair::from_uri("//Bob", CRYPTO_SR25519)?;
    let public = bob.public_only()?;
    let result = save_keypair_pair_to_keyfiles(
        &alice,
        &private_path,
        None,
        &public,
        &public_path,
        false,
        true,
    );
    assert!(
        result.is_err(),
        "different receiving and signing accounts were accepted"
    );
    assert!(
        !directory.0.exists(),
        "rejection must precede directory creation"
    );

    save_keypair_pair_to_keyfiles(
        &alice,
        &private_path,
        None,
        &alice.public_only()?,
        &public_path,
        false,
        true,
    )?;
    let original_private = fs::read(&private_path)?;
    let original_public = fs::read(&public_path)?;
    let original_count = fs::read_dir(&directory.0)?.count();
    let result = save_keypair_pair_to_keyfiles(
        &alice,
        &private_path,
        None,
        &public,
        &public_path,
        true,
        true,
    );
    assert!(result.is_err());
    assert_eq!(fs::read(&private_path)?, original_private);
    assert_eq!(fs::read(&public_path)?, original_public);
    assert_eq!(fs::read_dir(&directory.0)?.count(), original_count);
    Ok(())
}

// PROTECTED_REGRESSION: equal public bytes must not allow a keyfile pair to claim the wrong signing scheme; changes require explicit human approval.
#[test]
fn equal_public_bytes_with_different_crypto_are_rejected_before_writing(
) -> Result<(), Box<dyn Error>> {
    let directory = TestDirectory::new();
    let private = Keypair::from_seed(&[1; 32], CRYPTO_SR25519)?;
    let public = Keypair::new(None, Some(&private.public_key_bytes()), CRYPTO_ED25519, 42)?;
    assert_eq!(private.public_key_bytes(), public.public_key_bytes());
    assert_ne!(private.crypto_type(), public.crypto_type());
    let result = save_keypair_pair_to_keyfiles(
        &private,
        &directory.0.join("coldkey"),
        None,
        &public,
        &directory.0.join("coldkeypub.txt"),
        false,
        true,
    );
    assert!(result.is_err(), "different signing schemes were accepted");
    assert!(!directory.0.exists());
    Ok(())
}

#[test]
fn matching_pairs_preserve_all_supported_private_key_profiles() -> Result<(), Box<dyn Error>> {
    for crypto in [
        CRYPTO_SR25519,
        CRYPTO_ED25519,
        CRYPTO_HASHED,
        CRYPTO_HASHED_ED25519,
        CRYPTO_MLDSA65,
        CRYPTO_MLDSA65_FIXED,
        CRYPTO_FALCON512,
        CRYPTO_FALCON512_HASHED,
        CRYPTO_FALCON1024,
        CRYPTO_FALCON1024_HASHED,
    ] {
        let directory = TestDirectory::new();
        let private_path = directory.0.join("coldkey");
        let public_path = directory.0.join("coldkeypub.txt");
        let private = Keypair::from_seed(&[3; 32], crypto)?;
        let public = if crypto == CRYPTO_SR25519 {
            // SS58 display prefixes do not change the account identity.
            let public = Keypair::new(None, Some(&private.public_key_bytes()), crypto, 0)?;
            assert_ne!(public.ss58_address(), private.ss58_address());
            public
        } else {
            private.public_only()?
        };
        save_keypair_pair_to_keyfiles(
            &private,
            &private_path,
            None,
            &public,
            &public_path,
            false,
            true,
        )?;
        let recovered_private = read_keypair_from_keyfile(&private_path, None)?;
        let recovered_public = read_keypair_from_keyfile(&public_path, None)?;
        for recovered in [&recovered_private, &recovered_public] {
            assert_eq!(recovered.public_key_bytes(), private.public_key_bytes());
            assert_eq!(recovered.crypto_type(), crypto);
        }
        assert!(recovered_private.has_private_key());
        assert!(!recovered_public.has_private_key());
    }
    Ok(())
}
