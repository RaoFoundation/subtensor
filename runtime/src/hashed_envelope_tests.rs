//! Standard-v4 client compatibility and transaction-boundary authorization.
use super::*;
use frame_support::assert_ok;
use sp_runtime::traits::ExtrinsicCall;

#[test]
fn direct_call_authenticates_rotates_and_rejects_replay() {
    ext().execute_with(|| {
        setup();
        let tx = signed(call(), 0, &pair(10));
        assert_eq!(ExtrinsicCall::call(&tx), &call());
        let bytes = tx.encode();
        let decoded = UncheckedExtrinsic::decode(&mut &bytes[..]).unwrap();
        let legacy = LegacyUnchecked::decode(&mut &bytes[..]).unwrap();
        assert!(matches!(
            legacy.0.preamble,
            sp_runtime::generic::Preamble::Signed(Address::Raw(_), Signature::Sr25519(_), _)
        ));
        assert_eq!(legacy.encode(), bytes);
        let opaque: sp_runtime::OpaqueExtrinsic = decoded.clone().into();
        assert_eq!(opaque.encode(), bytes);
        assert_ok!(Executive::apply_extrinsic(decoded).unwrap());
        assert_eq!(HashedAccounts::accounts(account()).unwrap().generation, 1);
        assert!(
            Executive::apply_extrinsic(UncheckedExtrinsic::decode(&mut &bytes[..]).unwrap())
                .is_err()
        );
    });
}

#[test]
fn bare_general_and_ordinary_signature_cannot_impersonate_hashed_account() {
    ext().execute_with(|| {
        setup();
        let before = signing_state();
        assert!(Executive::apply_extrinsic(UncheckedExtrinsic::new_bare(call())).is_err());
        let general = UncheckedExtrinsic::Legacy(
            sp_runtime::generic::UncheckedExtrinsic {
                preamble: sp_runtime::generic::Preamble::General(0, extra(0).into()),
                function: call(),
            }
            .into(),
        );
        assert!(Executive::apply_extrinsic(general).is_err());
        let ordinary = SignedPayload::new(call(), extra(0))
            .unwrap()
            .using_encoded(|p| pair(10).sign(p));
        let mut tx = signed(call(), 0, &pair(10));
        let UncheckedExtrinsic::Legacy(xt) = &mut tx;
        let sp_runtime::generic::Preamble::Signed(_, sig, _) = &mut xt.0.preamble else {
            panic!("signed")
        };
        *sig = Signature::Sr25519(ordinary);
        assert!(Executive::apply_extrinsic(tx).is_err());
        assert_eq!(signing_state(), before);
    });
}

#[test]
fn raw_signer_rejects_unknown_versions_trailing_bytes_and_oversized_lengths() {
    ext().execute_with(|| {
        setup();
        let tx = signed(call(), 0, &pair(10));
        for bad in [
            alloc::vec![],
            alloc::vec![0;105],
            alloc::vec![0;107],
            alloc::vec![0;106],
        ] {
            let mut changed = tx.clone();
            let UncheckedExtrinsic::Legacy(xt) = &mut changed;
            let sp_runtime::generic::Preamble::Signed(address, _, _) = &mut xt.0.preamble else {
                panic!("signed")
            };
            *address = Address::Raw(bad);
            assert!(UncheckedExtrinsic::decode(&mut &changed.encode()[..]).is_err());
            assert!(Executive::apply_extrinsic(changed).is_err());
        }
        let mut body = alloc::vec![0x84, 2];
        codec::Compact(u32::MAX).encode_to(&mut body);
        let mut encoded = codec::Compact(body.len() as u32).encode();
        encoded.extend_from_slice(&body);
        assert!(UncheckedExtrinsic::decode(&mut &encoded[..]).is_err());
    });
}

#[test]
fn export_client_envelope_fixtures() {
    ext().execute_with(|| {
        setup();
        let ed = sp_core::ed25519::Pair::from_seed(&[23; 32]);
        let ed_extra = extra(0);
        let signature = SignedPayload::new(call(), ed_extra.clone()).unwrap().using_encoded(|p| ed.sign(p));
        let address = subtensor_hashed::SignerAddress {
            version: 1, account: [45;32], scheme: Scheme::Ed25519, generation: 7,
            next_commitment: [46;32], public_key: ed.public().0,
        };
        let hashed_signature = ed.sign(&hashed_extrinsic::signing_hash(&address, &call(), &ed_extra).unwrap());
        let ecdsa = sp_core::ecdsa::Pair::from_seed(&[24; 32]);
        let ecdsa_signature = SignedPayload::new(call(), ed_extra.clone()).unwrap().using_encoded(|p| ecdsa.sign(p));
        let ecdsa_account = sp_runtime::traits::IdentifyAccount::into_account(sp_runtime::MultiSigner::from(ecdsa.public()));
        let transactions = [
            legacy(&pair(1), call()),
            { mldsa::register(42); mldsa::sign(42, call()) },
            UncheckedExtrinsic::new_signed(call(), AccountId::from(ed.public()).into(), Signature::Ed25519(signature), ed_extra.clone()),
            signed(call(), 0, &pair(10)),
            UncheckedExtrinsic::new_signed(call(), Address::Raw(address.encode()), Signature::Ed25519(hashed_signature), ed_extra),
            UncheckedExtrinsic::new_signed(call(), ecdsa_account.into(), Signature::Ecdsa(ecdsa_signature), extra(0)),
            UncheckedExtrinsic::new_bare(call()),
        ];
        // Export a proof checked by the runtime, not just synthetic opaque bytes.
        assert!(transactions[1].clone().check(&ChainContext::default()).is_ok());
        let bytes = transactions.iter().map(|tx| {
            let bytes = tx.encode();
            assert_eq!(UncheckedExtrinsic::decode(&mut &bytes[..]).unwrap().encode(), bytes);
            format!("0x{}", hex::encode(bytes))
        }).collect::<Vec<_>>();
        if let Some(path) = std::env::var_os("ACCOUNT_ENVELOPE_FIXTURES") {
            let metadata = [14,15,16].map(|version| (version.to_string(), format!("0x{}", hex::encode(&*hashed_extrinsic::metadata_at_version(version).unwrap()))));
            let value = serde_json::json!({"metadata": metadata.into_iter().collect::<std::collections::BTreeMap<_,_>>(), "extrinsics": bytes,
                "specVersion": VERSION.spec_version, "transactionVersion": VERSION.transaction_version});
            std::fs::write(path, serde_json::to_vec(&value).unwrap()).unwrap();
        }
    });
}

/// Legacy Polkadot.js rejects fixed arrays above 2048 while constructing a
/// call decoder, even if that transaction uses another enum variant. Large
/// proofs must use bounded vectors and validate their exact length separately.
#[test]
fn all_metadata_versions_respect_legacy_client_fixed_array_limit() {
    for version in [14, 15, 16] {
        let encoded = hashed_extrinsic::metadata_at_version(version).unwrap();
        let metadata = frame_metadata::RuntimeMetadataPrefixed::decode(&mut &encoded[..]).unwrap();

        macro_rules! profiles {
            ($metadata:expr) => {
                &$metadata
                    .pallets
                    .iter()
                    .find(|p| p.name == "HashedAccounts")
                    .unwrap()
                    .constants
                    .iter()
                    .find(|c| c.name == "MldsaAccountProfiles")
                    .unwrap()
                    .value
            };
        }
        let advertised = match &metadata.1 {
            frame_metadata::RuntimeMetadata::V14(data) => profiles!(data),
            frame_metadata::RuntimeMetadata::V15(data) => profiles!(data),
            frame_metadata::RuntimeMetadata::V16(data) => profiles!(data),
            _ => panic!("unexpected metadata version"),
        };
        assert_eq!(advertised.as_slice(), &[4, 5]);
        let registry = match metadata.1 {
            frame_metadata::RuntimeMetadata::V14(metadata) => metadata.types,
            frame_metadata::RuntimeMetadata::V15(metadata) => metadata.types,
            frame_metadata::RuntimeMetadata::V16(metadata) => metadata.types,
            _ => panic!("unexpected metadata version"),
        };
        for portable in registry.types {
            if let scale_info::TypeDef::Array(array) = &portable.ty.type_def {
                assert!(
                    array.len <= 2048,
                    "metadata v{version} type {} exposes an unsupported fixed array: {:?}",
                    portable.id,
                    portable.ty
                );
            }
        }
    }
}

#[test]
fn account_metadata_uses_scheme_names_and_preserves_event_and_call_indices() {
    for version in [14, 15, 16] {
        let encoded = hashed_extrinsic::metadata_at_version(version).unwrap();
        let metadata = frame_metadata::RuntimeMetadataPrefixed::decode(&mut &encoded[..]).unwrap();
        macro_rules! check {
            ($data:expr) => {{
                let data = $data;
                let pallet = data
                    .pallets
                    .iter()
                    .find(|p| p.name == "HashedAccounts")
                    .unwrap();
                let storage = &pallet.storage.as_ref().unwrap().entries;
                assert!(storage.iter().any(|entry| entry.name == "MlDsaAccounts"));
                assert!(storage.iter().all(|entry| !matches!(
                    entry.name.as_str(),
                    "MlDsaRegistrationEnabled" | "MlDsaAuthorizationEnabled"
                )));
                let admin = data
                    .pallets
                    .iter()
                    .find(|p| p.name == "AdminUtils")
                    .unwrap();
                assert!(
                    admin
                        .storage
                        .as_ref()
                        .unwrap()
                        .entries
                        .iter()
                        .any(|entry| entry.name == "HashedAccountsEnabled")
                );
                assert!(storage.iter().all(|entry| !entry.name.contains("V2")));
                assert!(
                    pallet
                        .constants
                        .iter()
                        .any(|c| c.name == "MlDsaRegistrationDeposit")
                );
                assert!(pallet.constants.iter().all(|c| !c.name.contains("V2")));
                let scale_info::TypeDef::Variant(events) = &data
                    .types
                    .resolve(pallet.event.as_ref().unwrap().ty.id)
                    .unwrap()
                    .type_def
                else {
                    panic!("event enum")
                };
                assert_eq!(
                    events
                        .variants
                        .iter()
                        .map(|v| (v.index, v.name.as_str()))
                        .collect::<Vec<_>>(),
                    vec![
                        (0, "Registered"),
                        (1, "Rotated"),
                        (2, "RegisteredMlDsa"),
                        (3, "RotatedMlDsa"),
                        (4, "DerivedProtected"),
                        (5, "CapabilityInstalled"),
                        (6, "CapabilityRevoked"),
                        (8, "AuthorizedFixed"),
                    ]
                );
                let scale_info::TypeDef::Variant(calls) = &data
                    .types
                    .resolve(pallet.calls.as_ref().unwrap().ty.id)
                    .unwrap()
                    .type_def
                else {
                    panic!("call enum")
                };
                assert!(calls.variants.iter().all(|v| !v.name.contains("v2")));
                let mut call_indices = calls
                    .variants
                    .iter()
                    .map(|v| (v.index, v.name.as_str()))
                    .collect::<Vec<_>>();
                call_indices.sort_unstable();
                assert_eq!(
                    call_indices,
                    vec![
                        (0, "register"),
                        (1, "check_registered"),
                        (4, "register_derived"),
                        (5, "check_derived"),
                        (7, "install_capability"),
                        (8, "revoke_capability"),
                        (9, "register_derived_sponsored"),
                    ]
                );
                let orders = data
                    .pallets
                    .iter()
                    .find(|p| p.name == "LimitOrders")
                    .unwrap();
                for name in ["ApprovedOrders", "OrderNonces"] {
                    assert!(
                        orders
                            .storage
                            .as_ref()
                            .unwrap()
                            .entries
                            .iter()
                            .any(|e| e.name == name)
                    );
                }
            }};
        }
        match metadata.1 {
            frame_metadata::RuntimeMetadata::V14(data) => check!(data),
            frame_metadata::RuntimeMetadata::V15(data) => check!(data),
            frame_metadata::RuntimeMetadata::V16(data) => check!(data),
            _ => panic!("unexpected metadata version"),
        }
    }
}

#[test]
fn python_sdk_v4_vectors_execute_rotate_and_preserve_account_identity() {
    let fixture: serde_json::Value =
        serde_json::from_str(include_str!("../tests/fixtures/hashed-python-v4.json")).unwrap();
    assert_eq!(fixture["spec_version"], VERSION.spec_version);
    assert_eq!(fixture["transaction_version"], VERSION.transaction_version);
    for wallet in fixture["wallets"].as_array().unwrap() {
        ext().execute_with(|| {
            let descriptor = Descriptor::decode(
                &mut &hex::decode(wallet["descriptor"].as_str().unwrap()).unwrap()[..],
            )
            .unwrap();
            let account = super::composable_accounts::register(descriptor);
            assert_eq!(
                hex::encode(AsRef::<[u8]>::as_ref(&account)),
                wallet["account"].as_str().unwrap()
            );
            let recipient = AccountId::new([99; 32]);
            for (index, wire) in wallet["extrinsics"].as_array().unwrap().iter().enumerate() {
                let bytes = hex::decode(wire.as_str().unwrap()).unwrap();
                let xt = UncheckedExtrinsic::decode(&mut &bytes[..]).unwrap();
                assert_eq!(xt.encode(), bytes);
                let replay = xt.clone();
                let result = Executive::apply_extrinsic(xt).unwrap();
                if index < 2 {
                    assert_ok!(result);
                } else {
                    assert!(result.is_err());
                }
                assert_eq!(
                    HashedAccounts::accounts(&account).unwrap().generation,
                    (index + 1) as u64
                );
                assert_eq!(System::account_nonce(&account), (index + 2) as u32);
                assert!(Executive::apply_extrinsic(replay).is_err());
            }
            assert_eq!(
                Balances::free_balance(&recipient),
                TaoBalance::new(1_000_000_000)
            );
            assert!(Balances::free_balance(&account) < TaoBalance::new(9_000_000_000));
        });
    }
}

#[test]
fn ordinary_v4_fee_weight_accounts_for_all_protected_namespaces() {
    ext().execute_with(|| {
        let call = super::call();
        let tx = legacy(&pair(1), call.clone());
        let expected = frame_support::dispatch::DispatchInfo {
            extension_weight: <Runtime as frame_system::Config>::DbWeight::get().reads(3),
            ..call.get_dispatch_info()
        };
        assert_eq!(tx.get_dispatch_info(), expected);
        assert_eq!(
            tx.check(&ChainContext::default())
                .unwrap()
                .get_dispatch_info(),
            expected
        );
    });
}
