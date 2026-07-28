//! RFC-0078 merkleized-metadata: the digest signed via `CheckMetadataHash`
//! and the type proof hardware wallets need to decode a transaction on-device.
//!
//! Binds Parity's `merkleized-metadata` crate (the reference implementation
//! whose fixtures match polkadot-js `merkleizeMetadata`) to raw
//! `MetadataVersioned` bytes — the same blob the transport already downloads
//! and caches. Two entry points:
//!
//! - [`metadata_digest`]: the 32-byte hash a `MetadataVerifyingSigner` returns
//!   from its `metadata_digest` hook; the transport signs it into the payload
//!   and flips the mode byte.
//! - [`generate_extrinsic_proof`]: the SCALE-encoded proof blob the Ledger
//!   generic app expects appended to the signature payload (the same
//!   `MetadataProof { proof, extrinsic, extra_info }` wire shape Zondax's
//!   `ledger-polkadot-generic-api` service produces).

use codec::{Decode, Encode};
use frame_metadata::{RuntimeMetadata, RuntimeMetadataPrefixed};
use merkleized_metadata::{
    generate_metadata_digest, generate_proof_for_extrinsic_parts, types::ExtrinsicMetadata,
    ExtraInfo, FrameMetadataPrepared, Proof, SignedExtrinsicData,
};
use subtensor_macros::freeze_struct;

use crate::error::CoreError;

/// The chain constants that go into the digest alongside the metadata.
///
/// They must match what the runtime baked into its `RUNTIME_METADATA_HASH`
/// (`substrate-wasm-builder`'s `enable_metadata_hash(token_symbol, decimals)`
/// plus the runtime's spec name/version and ss58 prefix) or `CheckMetadataHash`
/// rejects the extrinsic with `BadProof`.
#[derive(Debug, Clone)]
pub struct ChainInfo {
    pub spec_version: u32,
    pub spec_name: String,
    pub base58_prefix: u16,
    pub decimals: u8,
    pub token_symbol: String,
}

impl ChainInfo {
    fn extra_info(&self) -> ExtraInfo {
        ExtraInfo {
            spec_version: self.spec_version,
            spec_name: self.spec_name.clone(),
            base58_prefix: self.base58_prefix,
            decimals: self.decimals,
            token_symbol: self.token_symbol.clone(),
        }
    }
}

/// `ExtraInfo` in the SCALE wire shape the Ledger generic app decodes.
///
/// The upstream crate's `ExtraInfo` doesn't derive `Encode`; the field order
/// here is pinned by the app's parser (spec_version, spec_name, base58_prefix,
/// decimals, token_symbol — the RFC-0078 `MetadataDigest::V1` tail order).
#[freeze_struct("edb2a68250293f5")]
#[derive(Encode)]
struct ExtraInfoEncoded {
    spec_version: u32,
    spec_name: String,
    base58_prefix: u16,
    decimals: u8,
    token_symbol: String,
}

impl From<ExtraInfo> for ExtraInfoEncoded {
    fn from(info: ExtraInfo) -> Self {
        Self {
            spec_version: info.spec_version,
            spec_name: info.spec_name,
            base58_prefix: info.base58_prefix,
            decimals: info.decimals,
            token_symbol: info.token_symbol,
        }
    }
}

/// The proof blob wire shape the Ledger generic app expects: the merkle proof
/// of the types the extrinsic touches, the extrinsic metadata, and the chain
/// constants — SCALE-encoded in this order.
#[freeze_struct("e1038cfe3b767760")]
#[derive(Encode)]
struct MetadataProof {
    proof: Proof,
    extrinsic: ExtrinsicMetadata,
    extra_info: ExtraInfoEncoded,
}

/// Decode a raw `MetadataVersioned` blob (magic `meta` + version byte +
/// V14/V15 payload — exactly what `state_getMetadata` /
/// `Metadata_metadata_at_version` return, unwrapped).
fn decode_metadata(metadata_bytes: &[u8]) -> Result<RuntimeMetadata, CoreError> {
    let prefixed = RuntimeMetadataPrefixed::decode(&mut &metadata_bytes[..])
        .map_err(|e| CoreError::Codec(format!("cannot decode runtime metadata: {e}")))?;
    Ok(prefixed.1)
}

/// The RFC-0078 metadata digest: the 32-byte hash `CheckMetadataHash` signs.
pub fn metadata_digest(metadata_bytes: &[u8], info: &ChainInfo) -> Result<[u8; 32], CoreError> {
    let metadata = decode_metadata(metadata_bytes)?;
    let digest =
        generate_metadata_digest(&metadata, info.extra_info()).map_err(CoreError::Codec)?;
    Ok(digest.hash())
}

/// The SCALE-encoded metadata proof for one extrinsic, in the wire shape the
/// Ledger generic app decodes on-device (appended after the signature payload).
///
/// - `call_data`: the SCALE-encoded call.
/// - `included_in_extrinsic`: the signed-extension bytes that travel in the
///   extrinsic (era, nonce, tip, mode byte...), in declaration order.
/// - `included_in_signed_data`: the implied bytes both sides agree on
///   (spec/tx version, genesis hash, era block hash, metadata hash...).
pub fn generate_extrinsic_proof(
    call_data: &[u8],
    included_in_extrinsic: &[u8],
    included_in_signed_data: &[u8],
    metadata_bytes: &[u8],
    info: &ChainInfo,
) -> Result<Vec<u8>, CoreError> {
    let metadata = decode_metadata(metadata_bytes)?;
    let signed_ext_data = SignedExtrinsicData {
        included_in_extrinsic,
        included_in_signed_data,
    };
    let proof = generate_proof_for_extrinsic_parts(call_data, Some(signed_ext_data), &metadata)
        .map_err(CoreError::Codec)?;
    let extrinsic = FrameMetadataPrepared::prepare(&metadata)
        .map_err(CoreError::Codec)?
        .as_type_information()
        .map_err(CoreError::Codec)?
        .extrinsic_metadata;
    let metadata_proof = MetadataProof {
        proof,
        extrinsic,
        extra_info: info.extra_info().into(),
    };
    Ok(metadata_proof.encode())
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::indexing_slicing)]

    use super::*;

    /// The Bittensor mainnet/localnet chain constants (runtime/build.rs bakes
    /// TAO / 9 decimals; ss58 prefix 42; spec name "node-subtensor").
    fn chain_info(spec_version: u32) -> ChainInfo {
        ChainInfo {
            spec_version,
            spec_name: "node-subtensor".into(),
            base58_prefix: 42,
            decimals: 9,
            token_symbol: "TAO".into(),
        }
    }

    /// The golden fixture recorded from localnet by the Python test suite.
    fn golden() -> serde_json::Value {
        let path = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../python/tests/fixtures/golden.json"
        );
        let raw = std::fs::read_to_string(path).expect("golden.json fixture exists");
        serde_json::from_str(&raw).unwrap()
    }

    fn golden_metadata_v15() -> Vec<u8> {
        let golden = golden();
        let hex_str = golden["metadata"]["v15_hex"]
            .as_str()
            .expect("golden.json has metadata.v15_hex");
        let raw = hex::decode(hex_str.trim_start_matches("0x")).unwrap();
        // The fixture stores the raw `Metadata_metadata_at_version` response:
        // an SCALE `Option<OpaqueMetadata>` wrapping the MetadataVersioned blob.
        Option::<Vec<u8>>::decode(&mut &raw[..])
            .unwrap()
            .expect("fixture metadata is Some")
    }

    /// Cross-implementation vector: the same metadata + chain info fed to
    /// polkadot-api's `merkleizeMetadata` (the JS implementation wallets use)
    /// produces this digest. The JS library validates the extra info against
    /// the metadata's own System.Version, so this pins spec name, version,
    /// prefix, decimals, and symbol all at once.
    #[test]
    fn digest_matches_polkadot_js_merkleize_metadata() {
        let golden = golden();
        let spec_version = golden["network"]["spec_version"]
            .as_u64()
            .and_then(|value| u32::try_from(value).ok())
            .expect("golden.json network.spec_version fits u32");
        let metadata = golden_metadata_v15();
        let digest = metadata_digest(&metadata, &chain_info(spec_version)).unwrap();
        assert_eq!(
            hex::encode(digest),
            "b5c88dea6d1920f1b4ced91e632b1b1db2db5c79a01e51ea383848b61f37d8a8"
        );
    }

    #[test]
    fn digest_is_deterministic_and_spec_version_sensitive() {
        let metadata = golden_metadata_v15();
        let digest_a = metadata_digest(&metadata, &chain_info(1)).unwrap();
        let digest_b = metadata_digest(&metadata, &chain_info(1)).unwrap();
        assert_eq!(digest_a, digest_b);
        // The spec version is part of the digest: a different runtime version
        // must produce a different hash.
        let digest_c = metadata_digest(&metadata, &chain_info(2)).unwrap();
        assert_ne!(digest_a, digest_c);
        assert_ne!(digest_a, [0u8; 32]);
    }

    #[test]
    fn rejects_garbage_metadata() {
        let result = metadata_digest(&[0u8; 16], &chain_info(1));
        assert!(matches!(result, Err(CoreError::Codec(_))));
    }

    /// The Ledger proof blob, cross-checked two ways.
    ///
    /// Byte-exact against the pinned vector (recorded from this crate — the
    /// same `generate_proof_for_extrinsic_parts` call Zondax's
    /// `ledger-polkadot-generic-api` service ships to devices), and
    /// envelope-compatible with polkadot-api's `getProofForExtrinsicParts`:
    /// the JS proof carries fewer leaves (it omits the address/signature
    /// types that only appear in the full extrinsic), so the merkle sections
    /// differ, but both must agree on the trailing
    /// `ExtrinsicMetadata ++ ExtraInfo` wire encoding the device parses. The
    /// tree contents themselves are cross-checked by the digest vector above.
    #[test]
    fn extrinsic_proof_matches_pinned_vectors() {
        let path = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/fixtures/ledger_proof_vector.json"
        );
        let raw = std::fs::read_to_string(path).expect("ledger proof vector fixture exists");
        let vector: serde_json::Value = serde_json::from_str(&raw).unwrap();
        let field =
            |name: &str| -> Vec<u8> { hex::decode(vector[name].as_str().unwrap()).unwrap() };
        let metadata = golden_metadata_v15();
        let spec_version = match &vector["spec_version"] {
            serde_json::Value::Number(number) => number
                .to_string()
                .parse()
                .expect("fixture spec_version fits u32"),
            value => panic!("fixture spec_version {value} is not an integer"),
        };
        let proof = generate_extrinsic_proof(
            &field("call_data_hex"),
            &field("included_in_extrinsic_hex"),
            &field("included_in_signed_data_hex"),
            &metadata,
            &chain_info(spec_version),
        )
        .unwrap();
        assert_eq!(
            hex::encode(&proof),
            vector["rust_proof_hex"].as_str().unwrap()
        );
        // ExtrinsicMetadata ++ ExtraInfo: everything after the merkle proof.
        // ExtraInfo alone is spec_version(4) + spec_name + prefix(2) +
        // decimals(1) + symbol; the shared tail across both implementations
        // must cover at least that plus the signed-extension list.
        let js_proof = field("polkadot_js_proof_hex");
        let tail_len = 512.min(js_proof.len()).min(proof.len());
        assert_eq!(
            proof.get(proof.len() - tail_len..),
            js_proof.get(js_proof.len() - tail_len..),
        );
    }
}
