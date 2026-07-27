//! Tests for commitments pallet: data type info.

use super::*;

#[test]
fn manual_data_type_info() {
    let mut registry = scale_info::Registry::new();
    let type_id = registry.register_type(&scale_info::meta_type::<Data>());
    let registry: scale_info::PortableRegistry = registry.into();
    let type_info = registry.resolve(type_id.id).expect("Expected not to panic");

    let check_type_info = |data: &Data| {
        let variant_name = match data {
            Data::None => "None".to_string(),
            Data::BlakeTwo256(_) => "BlakeTwo256".to_string(),
            Data::Sha256(_) => "Sha256".to_string(),
            Data::Keccak256(_) => "Keccak256".to_string(),
            Data::ShaThree256(_) => "ShaThree256".to_string(),
            Data::Raw(bytes) => format!("Raw{}", bytes.len()),
            Data::TimelockEncrypted { .. } => "TimelockEncrypted".to_string(),
            Data::ResetBondsFlag => "ResetBondsFlag".to_string(),
            Data::BigRaw(_) => "BigRaw".to_string(),
        };
        if let scale_info::TypeDef::Variant(variant) = &type_info.type_def {
            let variant = variant
                .variants
                .iter()
                .find(|v| v.name == variant_name)
                .unwrap_or_else(|| panic!("Expected to find variant {variant_name}"));

            let encoded = data.encode();
            assert_eq!(encoded[0], variant.index);

            // For variants with fields, check the encoded length matches expected field lengths
            if !variant.fields.is_empty() {
                let expected_len = match data {
                    Data::None => 0,
                    Data::Raw(bytes) => bytes.len() as u32,
                    Data::BigRaw(bytes) => bytes.len() as u32,
                    Data::BlakeTwo256(_)
                    | Data::Sha256(_)
                    | Data::Keccak256(_)
                    | Data::ShaThree256(_) => 32,
                    Data::TimelockEncrypted {
                        encrypted,
                        reveal_round,
                    } => {
                        // Calculate length: encrypted (length prefixed) + reveal_round (u64)
                        let encrypted_len = encrypted.encode().len() as u32; // Includes length prefix
                        let reveal_round_len = reveal_round.encode().len() as u32; // Typically 8 bytes
                        encrypted_len + reveal_round_len
                    }
                    Data::ResetBondsFlag => 0,
                };
                assert_eq!(
                    encoded.len() as u32 - 1, // Subtract variant byte
                    expected_len,
                    "Encoded length mismatch for variant {variant_name}"
                );
            } else {
                assert_eq!(
                    encoded.len() as u32 - 1,
                    0,
                    "Expected no fields for {variant_name}"
                );
            }
        } else {
            panic!("Should be a variant type");
        }
    };

    let mut data = vec![
        Data::None,
        Data::BlakeTwo256(Default::default()),
        Data::Sha256(Default::default()),
        Data::Keccak256(Default::default()),
        Data::ShaThree256(Default::default()),
        Data::ResetBondsFlag,
    ];

    // Add Raw instances for all possible sizes
    for n in 0..128 {
        data.push(Data::Raw(
            vec![0u8; n as usize]
                .try_into()
                .expect("Expected not to panic"),
        ));
    }

    // Add a TimelockEncrypted instance
    data.push(Data::TimelockEncrypted {
        encrypted: vec![0u8; 64].try_into().expect("Expected not to panic"),
        reveal_round: 12345,
    });

    for d in data.iter() {
        check_type_info(d);
    }
}
