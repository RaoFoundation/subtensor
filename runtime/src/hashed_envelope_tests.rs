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
        let transactions = [
            legacy(&pair(1), call()),
            UncheckedExtrinsic::new_signed(call(), AccountId::from(ed.public()).into(), Signature::Ed25519(signature), ed_extra.clone()),
            signed(call(), 0, &pair(10)),
            UncheckedExtrinsic::new_signed(call(), Address::Raw(address.encode()), Signature::Ed25519(hashed_signature), ed_extra),
        ];
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
fn ordinary_v4_fee_weight_policy_is_unchanged() {
    ext().execute_with(|| {
        let call = super::call();
        let tx = legacy(&pair(1), call.clone());
        let expected = frame_support::dispatch::DispatchInfo {
            extension_weight: <Runtime as frame_system::Config>::DbWeight::get().reads(1),
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
