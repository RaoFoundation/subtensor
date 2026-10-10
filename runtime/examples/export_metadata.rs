//! Export native-runtime metadata for SDK generation and protocol inspection.
//!
//! `SKIP_WASM_BUILD=1 cargo run -p node-subtensor-runtime --example export_metadata -- /tmp/subtensor-metadata.scale`
//! This exports the current production configuration, without running a node.
//! The pre-push gate still checks bindings against a freshly built release node.

use node_subtensor_runtime::{VERSION, hashed_extrinsic};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let output = std::env::args_os().nth(1).ok_or(
        "usage: cargo run -p node-subtensor-runtime --example export_metadata -- <output.scale> [version=15]",
    )?;
    let version = std::env::args()
        .nth(2)
        .map(|value| value.parse::<u32>())
        .transpose()?
        .unwrap_or(15);
    let metadata = hashed_extrinsic::metadata_at_version(version)
        .ok_or("runtime does not support the requested metadata version")?;
    std::fs::write(&output, &*metadata)?;
    println!(
        "Exported metadata V{version}: spec_version={}, transaction_version={}, ss58_format=42",
        VERSION.spec_version, VERSION.transaction_version,
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::indexing_slicing)]
    use codec::Decode;
    use frame_metadata::{RuntimeMetadata, RuntimeMetadataPrefixed};

    use super::*;

    fn metadata(version: u32) -> RuntimeMetadata {
        let bytes = hashed_extrinsic::metadata_at_version(version).unwrap();
        RuntimeMetadataPrefixed::decode(&mut &bytes[..]).unwrap().1
    }

    #[test]
    fn legacy_metadata_keeps_legacy_extension_layout() {
        let RuntimeMetadata::V14(v14) = metadata(14) else {
            panic!("expected V14")
        };
        let RuntimeMetadata::V15(v15) = metadata(15) else {
            panic!("expected V15")
        };
        let v14_extensions: Vec<_> = v14
            .extrinsic
            .signed_extensions
            .iter()
            .map(|ext| ext.identifier.as_str())
            .collect();
        let v15_extensions: Vec<_> = v15
            .extrinsic
            .signed_extensions
            .iter()
            .map(|ext| ext.identifier.as_str())
            .collect();
        assert_eq!(v14_extensions, v15_extensions);
        assert!(!v15_extensions.contains(&"AuthorizeHashedAccount"));
        assert_eq!(v14.extrinsic.version, 4);
        assert_eq!(v15.extrinsic.version, 4);
        assert!(
            v15.pallets
                .iter()
                .any(|pallet| pallet.name == "HashedAccounts")
        );
    }

    #[test]
    fn v16_advertises_both_pipelines_with_a_typed_hashed_proof() {
        let RuntimeMetadata::V16(v16) = metadata(16) else {
            panic!("expected V16")
        };
        assert!(v16.extrinsic.versions.contains(&4));
        assert!(v16.extrinsic.versions.contains(&5));
        let pipelines = &v16.extrinsic.transaction_extensions_by_version;
        let legacy = pipelines.get(&0).unwrap();
        let hashed = pipelines.get(&1).unwrap();
        assert_eq!(&hashed[1..], legacy);
        let authorization = &v16.extrinsic.transaction_extensions[hashed[0].0 as usize];
        assert_eq!(authorization.identifier, "AuthorizeHashedAccount");
        let ty = v16.types.resolve(authorization.ty.id).unwrap();
        let scale_info::TypeDef::Composite(fields) = &ty.type_def else {
            panic!("authorization must contain the account and proof")
        };
        assert!(
            fields
                .fields
                .iter()
                .any(|field| field.name.as_deref() == Some("account"))
        );
        let proof = fields
            .fields
            .iter()
            .find(|field| field.name.as_deref() == Some("proof"))
            .unwrap();
        let proof = v16.types.resolve(proof.ty.id).unwrap();
        let scale_info::TypeDef::Composite(proof) = &proof.type_def else {
            panic!("proof must advertise its wire fields")
        };
        assert_eq!(
            proof
                .fields
                .iter()
                .filter_map(|field| field.name.as_deref())
                .collect::<Vec<_>>(),
            ["generation", "public_key", "next_commitment", "signature"]
        );
        assert!(v16.types.resolve(authorization.implicit.id).is_some());
        assert!(!legacy.iter().any(|index| index == &hashed[0]));
    }
}
