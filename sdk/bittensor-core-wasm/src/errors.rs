//! CoreError -> JS `Error` mapping. Each variant sets `error.name` so TS
//! callers can branch on the same taxonomy the Python binding exposes as
//! exception classes.

use bittensor_core::CoreError;
use wasm_bindgen::JsValue;

pub fn to_js_err(err: CoreError) -> JsValue {
    let name = match &err {
        CoreError::Keyfile(_) => "KeyfileError",
        CoreError::WrongPassword(_) => "WrongPasswordError",
        CoreError::NotInRuntime(_) => "NotInRuntimeError",
        CoreError::Codec(_) => "CodecError",
        CoreError::Crypto(_) => "CryptoError",
        // Matches the Python binding's LedgerError class. Unreachable today
        // (no `ledger` feature here) but kept aligned for when a WebHID
        // signer backend lands.
        CoreError::Device(_) => "LedgerError",
        CoreError::Rpc(_) => "RpcError",
        CoreError::Policy(_) => "PolicyError",
    };
    let error = js_sys::Error::new(&err.to_string());
    error.set_name(name);
    error.into()
}

pub fn value_err(msg: impl AsRef<str>) -> JsValue {
    let error = js_sys::Error::new(msg.as_ref());
    error.set_name("CodecError");
    error.into()
}
