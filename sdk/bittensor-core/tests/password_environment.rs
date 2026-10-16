//! Verify wallet-compatible password encoding without global environment writes.

use std::error::Error;

use bittensor_core::{
    keyfile_env::{decode_password_from_environment, encode_password_for_environment},
    CoreError,
};

#[test]
fn encoding_matches_legacy_ascii_unicode_and_empty_vectors() -> Result<(), Box<dyn Error>> {
    for (name, password, expected) in [
        ("BT_PW_TEST", "native-test", "LDUrOSE6eTE2JzY="),
        ("WALLET_α", "pässwörd\0\n", "J4LoPzYjnHjDM0FG"),
        ("A", "", ""),
        ("A", "a\0\n", "IEFL"),
    ] {
        assert_eq!(encode_password_for_environment(name, password)?, expected);
        assert_eq!(decode_password_from_environment(name, expected)?, password);
    }
    Ok(())
}

#[test]
fn empty_encoding_key_retains_the_legacy_error() {
    assert!(matches!(
        encode_password_for_environment("", "password"),
        Err(CoreError::Crypto(message)) if message == "env var name must not be empty"
    ));
}

#[test]
fn encoding_does_not_create_or_replace_a_process_variable() -> Result<(), Box<dyn Error>> {
    let name = format!("BT_PW_PURE_ENCODING_REGRESSION_{}", std::process::id());
    let before = std::env::var_os(&name);
    let value = encode_password_for_environment(&name, "test-password")?;
    assert!(!value.is_empty());
    assert_eq!(std::env::var_os(&name), before);
    // Encoding is defined by UTF-8 bytes, independently of OS variable syntax.
    assert!(!encode_password_for_environment("not=an\0os-variable", "secret")?.is_empty());
    Ok(())
}

#[test]
fn decoder_retains_whitespace_and_error_boundaries() -> Result<(), Box<dyn Error>> {
    assert_eq!(
        decode_password_from_environment("BT_PW_TEST", " \nLDUrOSE6eTE2JzY=\t ")?,
        "native-test"
    );
    assert!(matches!(decode_password_from_environment("", ""),
        Err(CoreError::Crypto(message)) if message == "env var name must not be empty"));
    assert!(matches!(decode_password_from_environment("A", "!!!"),
        Err(CoreError::Keyfile(message)) if message == "invalid base64 in wallet password env var"));
    assert!(matches!(decode_password_from_environment("A", "vg=="),
        Err(CoreError::Keyfile(message)) if message == "invalid wallet password env var: corrupt UTF-8"));
    Ok(())
}

#[test]
fn malformed_suffix_after_valid_blocks_retains_the_decoder_error() -> Result<(), Box<dyn Error>> {
    let name = "BT_PW_TEST";
    let password = "password long enough to cover several valid decoding blocks";
    let encoded = encode_password_for_environment(name, password)?;
    assert_eq!(decode_password_from_environment(name, &encoded)?, password);
    assert!(matches!(
        decode_password_from_environment(name, &format!("{encoded}!!!!")),
        Err(CoreError::Keyfile(message)) if message == "invalid base64 in wallet password env var"
    ));
    Ok(())
}

#[test]
fn guarded_preallocation_covers_partial_decoding_without_growth() -> Result<(), Box<dyn Error>> {
    use base64::{engine::general_purpose::STANDARD, Engine as _};
    let encoded = encode_password_for_environment(
        "BT_PW_TEST",
        "password long enough to cover several valid decoding blocks",
    )?;
    let malformed = format!("{encoded}!!!!");
    let mut partial = zeroize::Zeroizing::new(Vec::with_capacity(base64::decoded_len_estimate(
        malformed.len(),
    )));
    let pointer = partial.as_ptr();
    let capacity = partial.capacity();
    assert!(STANDARD.decode_vec(&malformed, &mut partial).is_err());
    // Establish the dependency's late-error path and sizing contract, on which
    // the guard-before-decoding implementation relies.
    assert!(partial.iter().any(|byte| *byte != 0));
    assert_eq!(partial.as_ptr(), pointer);
    assert_eq!(partial.capacity(), capacity);
    Ok(())
}
