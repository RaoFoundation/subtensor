//! JSON serialize/deserialize for btwallet-compatible keyfiles.

use std::collections::HashMap;

use pyo3::prelude::*;
use serde_json::json;

use crate::keyfile::key_err;
use crate::{Keypair, CRYPTO_SR25519};

pub fn serialized_keypair_to_keyfile_data(keypair: &Keypair) -> PyResult<Vec<u8>> {
    let mut data: HashMap<&str, serde_json::Value> = HashMap::new();

    let public_key = keypair.public_key_bytes();
    let public_key_str = hex::encode(public_key);
    data.insert("accountId", json!(format!("0x{public_key_str}")));
    data.insert("publicKey", json!(format!("0x{public_key_str}")));

    if let Some(private_key) = keypair.private_key_bytes() {
        let private_key_str = hex::encode(private_key);
        data.insert("privateKey", json!(format!("0x{private_key_str}")));
    }

    data.insert("ss58Address", json!(keypair.ss58_address()));
    data.insert("cryptoType", json!(keypair.crypto_type()));

    serde_json::to_string(&data)
        .map(|json_data| json_data.into_bytes())
        .map_err(|error| key_err(format!("serialization error: {error}")))
}

pub fn deserialize_keypair_from_keyfile_data(keyfile_data: &[u8]) -> PyResult<Keypair> {
    let decoded =
        std::str::from_utf8(keyfile_data).map_err(|_| key_err("failed to decode keyfile data"))?;

    let keyfile_dict: serde_json::Value =
        serde_json::from_str(decoded).map_err(|_| key_err("failed to parse keyfile data"))?;

    let crypto_type = keyfile_dict
        .get("cryptoType")
        .and_then(|value| value.as_u64())
        .map(|value| value as u8)
        .unwrap_or(CRYPTO_SR25519);

    if let Some(secret_phrase) = keyfile_dict
        .get("secretPhrase")
        .and_then(|value| value.as_str())
    {
        return Keypair::create_from_mnemonic(secret_phrase, crypto_type, None);
    }

    if let Some(seed) = keyfile_dict
        .get("secretSeed")
        .and_then(|value| value.as_str())
    {
        let seed = seed.trim_start_matches("0x");
        let seed_bytes =
            hex::decode(seed).map_err(|error| key_err(format!("invalid secret seed: {error}")))?;
        return Keypair::create_from_seed(&seed_bytes, crypto_type);
    }

    if let Some(private_key) = keyfile_dict
        .get("privateKey")
        .and_then(|value| value.as_str())
    {
        return Keypair::create_from_private_key(private_key, crypto_type);
    }

    if let Some(ss58) = keyfile_dict
        .get("ss58Address")
        .and_then(|value| value.as_str())
    {
        return Keypair::py_new(Some(ss58), None, crypto_type, 42);
    }

    Err(key_err("keypair could not be created from keyfile data"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{CRYPTO_ED25519, CRYPTO_SR25519};

    fn test_mnemonic() -> String {
        "abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon about"
            .to_string()
    }

    #[test]
    fn sr25519_keyfile_roundtrip() {
        let original =
            Keypair::create_from_mnemonic(&test_mnemonic(), CRYPTO_SR25519, None).unwrap();
        let data = serialized_keypair_to_keyfile_data(&original).unwrap();
        let restored = deserialize_keypair_from_keyfile_data(&data).unwrap();
        assert_eq!(restored.crypto_type(), CRYPTO_SR25519);
        assert_eq!(restored.ss58_address(), original.ss58_address());
    }

    #[test]
    fn legacy_keyfile_without_crypto_type_defaults_sr25519() {
        let json = r#"{"secretPhrase":"abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon about","ss58Address":"5GrwvaEF5zXb26Fz9rcQpDWS57CtERHpNehXCPcNoHGKutQY"}"#;
        let keypair = deserialize_keypair_from_keyfile_data(json.as_bytes()).unwrap();
        assert_eq!(keypair.crypto_type(), CRYPTO_SR25519);
    }

    #[test]
    fn ed25519_keyfile_roundtrip() {
        let original =
            Keypair::create_from_mnemonic(&test_mnemonic(), CRYPTO_ED25519, None).unwrap();
        let data = serialized_keypair_to_keyfile_data(&original).unwrap();
        let restored = deserialize_keypair_from_keyfile_data(&data).unwrap();
        assert_eq!(restored.crypto_type(), CRYPTO_ED25519);
        assert_eq!(restored.ss58_address(), original.ss58_address());
    }
}
