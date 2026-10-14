//! Pure environment password compatibility, without changing process variables.

use crate::CoreError;
use base64::{engine::general_purpose, Engine as _};
use zeroize::Zeroizing;

fn xor(data: &[u8], name: &str) -> Result<Zeroizing<Vec<u8>>, CoreError> {
    if name.is_empty() {
        return Err(CoreError::Crypto("env var name must not be empty".into()));
    }
    Ok(Zeroizing::new(
        data.iter()
            .zip(name.bytes().cycle())
            .map(|(byte, key)| byte ^ key)
            .collect(),
    ))
}

/// Return the wallet-compatible XOR/base64 value without reading or changing
/// the process environment. The encoding key must be nonempty; its UTF-8
/// bytes are used exactly as in the legacy wallet format.
pub fn encode_password_for_environment(name: &str, password: &str) -> Result<String, CoreError> {
    Ok(general_purpose::STANDARD.encode(xor(password.as_bytes(), name)?.as_slice()))
}

/// Decode a caller-supplied environment value without consulting process state.
pub fn decode_password_from_environment(name: &str, value: &str) -> Result<String, CoreError> {
    if name.is_empty() {
        return Err(CoreError::Crypto("env var name must not be empty".into()));
    }
    let value = value.trim();
    // Install the guard before decoding: malformed suffixes can follow valid
    // blocks that have already written reversible password bytes. Reserving the
    // decoder's full bound prevents growth from leaving a prior allocation.
    let mut bytes = Zeroizing::new(Vec::with_capacity(base64::decoded_len_estimate(
        value.len(),
    )));
    general_purpose::STANDARD
        .decode_vec(value, &mut bytes)
        .map_err(|_| CoreError::Keyfile("invalid base64 in wallet password env var".into()))?;
    let decoded = xor(&bytes, name)?;
    std::str::from_utf8(&decoded)
        .map(str::to_owned)
        .map_err(|_| CoreError::Keyfile("invalid wallet password env var: corrupt UTF-8".into()))
}
