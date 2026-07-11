//! wasm-bindgen bindings for `bittensor-core`. No logic lives here:
//! constructors, method forwarding, error mapping, and JS-object
//! materialization only — the same role `bittensor-core-py` plays for Python.
//!
//! The TS shell owns all I/O (WebSocket JSON-RPC, `fetch`); this module owns
//! everything whose right answer is defined by the chain: crypto, SCALE,
//! extrinsic payloads, metadata digests. Host-only surfaces (keyfiles,
//! Ledger, drand fetching) are deliberately absent — the crate builds
//! `bittensor-core` without its `host` feature.

use wasm_bindgen::prelude::*;

mod digest;
mod errors;
mod keys;
mod runtime;
mod timelock;
mod values;

/// This binding crate's version (mirrors the Python binding's
/// `__core_version__`, which likewise reports the binding, not the core).
#[wasm_bindgen(js_name = coreVersion)]
pub fn core_version() -> String {
    env!("CARGO_PKG_VERSION").to_string()
}
