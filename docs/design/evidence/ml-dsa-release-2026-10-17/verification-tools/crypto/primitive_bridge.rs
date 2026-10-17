//! Test-only FFI around the exact production primitive; never linked into the node.
use std::{ptr, slice};
use subtensor_hashed::mldsa;
#[no_mangle]
pub unsafe extern "C" fn local_keygen(seed: *const u8, output: *mut u8) -> i32 {
    let seed: &[u8; 32] = slice::from_raw_parts(seed, 32).try_into().unwrap();
    match mldsa::public_key_from_seed(seed) {
        Ok(key) => {
            ptr::copy_nonoverlapping(key.as_ptr(), output, key.len());
            0
        }
        Err(_) => -1,
    }
}
#[no_mangle]
pub unsafe extern "C" fn local_sign(
    seed: *const u8,
    message: *const u8,
    message_len: usize,
    context: *const u8,
    context_len: usize,
    random: *const u8,
    output: *mut u8,
) -> i32 {
    let seed: &[u8; 32] = slice::from_raw_parts(seed, 32).try_into().unwrap();
    let random: &[u8; 32] = slice::from_raw_parts(random, 32).try_into().unwrap();
    match mldsa::sign_message(
        seed,
        slice::from_raw_parts(message, message_len),
        slice::from_raw_parts(context, context_len),
        random,
    ) {
        Ok(sig) => {
            ptr::copy_nonoverlapping(sig.as_ptr(), output, sig.len());
            0
        }
        Err(_) => -1,
    }
}
#[no_mangle]
pub unsafe extern "C" fn local_verify(
    key: *const u8,
    message: *const u8,
    message_len: usize,
    context: *const u8,
    context_len: usize,
    signature: *const u8,
) -> i32 {
    let key: &mldsa::PublicKeyBytes = slice::from_raw_parts(key, mldsa::PUBLIC_KEY_LEN)
        .try_into()
        .unwrap();
    let signature: &mldsa::SignatureBytes = slice::from_raw_parts(signature, mldsa::SIGNATURE_LEN)
        .try_into()
        .unwrap();
    if mldsa::verify_message(
        key,
        slice::from_raw_parts(message, message_len),
        slice::from_raw_parts(context, context_len),
        signature,
    ) {
        0
    } else {
        -1
    }
}
