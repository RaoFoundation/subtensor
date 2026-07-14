//! Bindings for `bittensor_core::digest` — RFC-0078 merkleized metadata.

use bittensor_core::digest::{self, ChainInfo};
use pyo3::prelude::*;
use pyo3::types::PyBytes;

use crate::errors::to_py_err;

/// The Bittensor chain constants baked into the runtime's metadata hash
/// (`runtime/build.rs` `enable_metadata_hash("TAO", 9)`, ss58 prefix 42).
/// Callers targeting another chain pass their own values.
const DEFAULT_BASE58_PREFIX: u16 = 42;
const DEFAULT_DECIMALS: u8 = 9;
const DEFAULT_TOKEN_SYMBOL: &str = "TAO";

#[allow(clippy::too_many_arguments)]
fn chain_info(
    spec_version: u32,
    spec_name: &str,
    base58_prefix: u16,
    decimals: u8,
    token_symbol: &str,
) -> ChainInfo {
    ChainInfo {
        spec_version,
        spec_name: spec_name.to_owned(),
        base58_prefix,
        decimals,
        token_symbol: token_symbol.to_owned(),
    }
}

/// The RFC-0078 metadata digest: the 32-byte hash signed via the
/// ``CheckMetadataHash`` extension.
///
/// Args:
///     metadata_bytes (bytes): Raw ``MetadataVersioned`` blob (magic ``meta``
///         + version byte + V14/V15 payload).
///     spec_version (int): The runtime's spec version.
///     spec_name (str): The runtime's spec name (e.g. ``node-subtensor``).
///     base58_prefix (int): ss58 address prefix. Defaults to 42.
///     decimals (int): Decimals of the primary token. Defaults to 9.
///     token_symbol (str): Primary token symbol. Defaults to ``TAO``.
///
/// Returns:
///     bytes: The 32-byte digest.
#[pyfunction]
#[pyo3(signature = (metadata_bytes, spec_version, spec_name, base58_prefix=DEFAULT_BASE58_PREFIX, decimals=DEFAULT_DECIMALS, token_symbol=DEFAULT_TOKEN_SYMBOL))]
fn metadata_digest(
    py: Python,
    metadata_bytes: Vec<u8>,
    spec_version: u32,
    spec_name: &str,
    base58_prefix: u16,
    decimals: u8,
    token_symbol: &str,
) -> PyResult<Py<PyBytes>> {
    let info = chain_info(
        spec_version,
        spec_name,
        base58_prefix,
        decimals,
        token_symbol,
    );
    let hash = digest::metadata_digest(&metadata_bytes, &info).map_err(to_py_err)?;
    Ok(PyBytes::new(py, &hash).into())
}

/// The SCALE-encoded metadata proof the Ledger generic app decodes on-device
/// (appended after the signature payload when signing).
///
/// Args:
///     call_data (bytes): The SCALE-encoded call.
///     included_in_extrinsic (bytes): Signed-extension bytes that travel in
///         the extrinsic (era, nonce, tip, mode...), in declaration order.
///     included_in_signed_data (bytes): The implied bytes both sides sign
///         (spec/tx version, genesis hash, era block hash, metadata hash...).
///     metadata_bytes (bytes): Raw ``MetadataVersioned`` blob.
///     spec_version (int): The runtime's spec version.
///     spec_name (str): The runtime's spec name.
///     base58_prefix (int): ss58 address prefix. Defaults to 42.
///     decimals (int): Decimals of the primary token. Defaults to 9.
///     token_symbol (str): Primary token symbol. Defaults to ``TAO``.
///
/// Returns:
///     bytes: The SCALE-encoded proof blob.
#[allow(clippy::too_many_arguments)]
#[pyfunction]
#[pyo3(signature = (call_data, included_in_extrinsic, included_in_signed_data, metadata_bytes, spec_version, spec_name, base58_prefix=DEFAULT_BASE58_PREFIX, decimals=DEFAULT_DECIMALS, token_symbol=DEFAULT_TOKEN_SYMBOL))]
fn generate_extrinsic_proof(
    py: Python,
    call_data: Vec<u8>,
    included_in_extrinsic: Vec<u8>,
    included_in_signed_data: Vec<u8>,
    metadata_bytes: Vec<u8>,
    spec_version: u32,
    spec_name: &str,
    base58_prefix: u16,
    decimals: u8,
    token_symbol: &str,
) -> PyResult<Py<PyBytes>> {
    let info = chain_info(
        spec_version,
        spec_name,
        base58_prefix,
        decimals,
        token_symbol,
    );
    let proof = digest::generate_extrinsic_proof(
        &call_data,
        &included_in_extrinsic,
        &included_in_signed_data,
        &metadata_bytes,
        &info,
    )
    .map_err(to_py_err)?;
    Ok(PyBytes::new(py, &proof).into())
}

pub fn register(module: &Bound<'_, PyModule>) -> PyResult<()> {
    module.add_function(wrap_pyfunction!(metadata_digest, module)?)?;
    module.add_function(wrap_pyfunction!(generate_extrinsic_proof, module)?)?;
    Ok(())
}
