//! Exhaustive metadata-to-Executive envelope coverage. Minimal arguments exercise
//! authentication and dispatch, not every operation's successful business state.
use super::*;
use fp_self_contained::SelfContainedCall;
use scale_info::{PortableRegistry, TypeDef, TypeDefPrimitive};
use sp_core::ed25519;
use sp_runtime::traits::TransactionExtension;
use subtensor_hashed::mldsa;

/// Construct finite canonical values from metadata, trying other enum variants
/// when a recursive branch cannot terminate. Unknown or uninhabited types are
/// returned explicitly rather than silently omitted from the coverage report.
fn minimal_value(registry: &PortableRegistry, id: u32, path: &mut Vec<u32>) -> Option<Vec<u8>> {
    if path.contains(&id) || path.len() >= 64 {
        return None;
    }
    path.push(id);
    let result = (|| {
        let ty = &registry.resolve(id)?.type_def;
        let mut fields = |ids: Vec<u32>| -> Option<Vec<u8>> {
            let mut bytes = Vec::new();
            for id in ids {
                bytes.extend(minimal_value(registry, id, path)?);
            }
            Some(bytes)
        };
        match ty {
            TypeDef::Composite(composite) => {
                fields(composite.fields.iter().map(|f| f.ty.id).collect())
            }
            TypeDef::Tuple(tuple) => fields(tuple.fields.iter().map(|f| f.id).collect()),
            TypeDef::Array(array) => {
                let element = minimal_value(registry, array.type_param.id, path)?;
                Some(element.repeat(usize::try_from(array.len).ok()?))
            }
            TypeDef::Sequence(_) | TypeDef::BitSequence(_) => Some(vec![0]),
            TypeDef::Compact(_) => Some(vec![4]),
            TypeDef::Variant(variants) => variants.variants.iter().find_map(|variant| {
                let mut bytes = vec![variant.index];
                for field in &variant.fields {
                    bytes.extend(minimal_value(registry, field.ty.id, path)?);
                }
                Some(bytes)
            }),
            TypeDef::Primitive(primitive) => {
                let len = match primitive {
                    TypeDefPrimitive::Bool | TypeDefPrimitive::U8 | TypeDefPrimitive::I8 => 1,
                    TypeDefPrimitive::U16 | TypeDefPrimitive::I16 => 2,
                    TypeDefPrimitive::U32 | TypeDefPrimitive::I32 | TypeDefPrimitive::Char => 4,
                    TypeDefPrimitive::U64 | TypeDefPrimitive::I64 => 8,
                    TypeDefPrimitive::U128 | TypeDefPrimitive::I128 => 16,
                    TypeDefPrimitive::U256 | TypeDefPrimitive::I256 => 32,
                    TypeDefPrimitive::Str => return Some(vec![0]),
                };
                let mut bytes = vec![0; len];
                bytes[0] = 1;
                Some(bytes)
            }
        }
    })();
    path.pop();
    result
}

#[derive(Clone, Copy, Debug)]
enum Mode {
    Sr,
    Ed,
    HashedMs,
    FixedMs,
}

fn signed(call: RuntimeCall, mode: Mode) -> (AccountId, UncheckedExtrinsic) {
    let key = [91; 32];
    let (account, ms) = match mode {
        Mode::Sr => (AccountId::from(pair(91).public()), None),
        Mode::Ed => (
            AccountId::new(ed25519::Pair::from_seed(&key).public().0),
            None,
        ),
        Mode::HashedMs | Mode::FixedMs => {
            pallet_admin_utils::HashedAccountsEnabled::<Runtime>::put(true);

            hashed_auth::TestVerificationWeight::set(Weight::from_parts(10_000_000, 0));
            let public_key = mldsa::public_key_from_seed(&key).unwrap();
            let descriptor = mldsa::Descriptor {
                version: mldsa::VERSION,
                algorithm: if matches!(mode, Mode::FixedMs) {
                    mldsa::FIXED_ALGORITHM
                } else {
                    mldsa::ALGORITHM
                },
                initial_commitment: mldsa::commitment(&public_key),
            };
            let account = AccountId::new(mldsa::account_id(&descriptor));
            let sponsor = AccountId::from(pair(70).public());
            let _ =
                Balances::make_free_balance_be(&sponsor, TaoBalance::new(1_000_000_000_000_000));
            frame_support::assert_ok!(HashedAccounts::register(
                RuntimeOrigin::signed(sponsor),
                descriptor.into()
            ));
            (account, Some((descriptor, public_key)))
        }
    };
    let _ = Balances::make_free_balance_be(&account, TaoBalance::new(1_000_000_000_000_000));
    let extra = extra(System::account_nonce(&account));
    let xt = if let Some((descriptor, public_key)) = ms {
        let next_commitment = if matches!(mode, Mode::FixedMs) {
            descriptor.initial_commitment
        } else {
            mldsa::commitment(&mldsa::public_key_from_seed(&[92; 32]).unwrap())
        };
        let header = mldsa::Header {
            account: *account.as_ref(),
            version: mldsa::VERSION,
            algorithm: descriptor.algorithm,
            generation: 0,
            public_key,
            next_commitment,
        };
        let payload = (&call, &extra, extra.implicit().unwrap()).encode();
        let address = header.sign(&key, &payload, &[0; 32]).unwrap();
        UncheckedExtrinsic::new_signed(
            call,
            Address::Raw(address.encode()),
            Signature::Ed25519(ed25519::Signature::from_raw(mldsa::CARRIER_SIGNATURE)),
            extra,
        )
    } else {
        let payload = SignedPayload::new(call.clone(), extra.clone()).unwrap();
        let signature = match mode {
            Mode::Sr => Signature::Sr25519(payload.using_encoded(|bytes| pair(91).sign(bytes))),
            _ => Signature::Ed25519(
                payload.using_encoded(|bytes| ed25519::Pair::from_seed(&key).sign(bytes)),
            ),
        };
        UncheckedExtrinsic::new_signed(call, account.clone().into(), signature, extra)
    };
    (account, xt)
}

#[test]
fn every_metadata_call_has_explicit_envelope_and_dispatch_coverage() {
    let encoded = hashed_extrinsic::metadata_at_version(15).unwrap();
    let metadata = frame_metadata::RuntimeMetadataPrefixed::decode(&mut &encoded[..]).unwrap();
    let frame_metadata::RuntimeMetadata::V15(metadata) = metadata.1 else {
        panic!("metadata15 unavailable");
    };
    let mut unresolved = Vec::new();
    let mut covered = 0usize;
    let mut self_contained = 0usize;
    let mut uninhabited = Vec::new();
    for pallet in &metadata.pallets {
        let Some(calls) = &pallet.calls else { continue };
        let TypeDef::Variant(calls) = &metadata.types.resolve(calls.ty.id).unwrap().type_def else {
            panic!("call metadata is not an enum");
        };
        for variant in &calls.variants {
            let name = format!("{}.{}", pallet.name, variant.name);
            // Grandpa equivocation reports are intentionally uncallable in this
            // runtime: KeyOwnerProof is Void, an enum with no inhabitants.
            // Assert that metadata proves this instead of silently skipping it.
            if variant.fields.iter().any(|field| {
                matches!(
                    &metadata.types.resolve(field.ty.id).unwrap().type_def,
                    TypeDef::Variant(variants) if variants.variants.is_empty()
                )
            }) {
                uninhabited.push(name.clone());
                println!("MS_MATRIX {name} uninhabited-call-type");
                continue;
            }
            let mut encoded = vec![pallet.index, variant.index];
            let mut supported = true;
            for field in &variant.fields {
                match minimal_value(&metadata.types, field.ty.id, &mut Vec::new()) {
                    Some(value) => encoded.extend(value),
                    None => {
                        supported = false;
                        break;
                    }
                }
            }
            // Ethereum encodes its transaction using a custom codec. Supply a
            // valid typed legacy transaction so all native signing modes prove
            // that the self-contained ECDSA route cannot be wrapped as signed.
            if name == "Ethereum.transact" {
                encoded = RuntimeCall::Ethereum(pallet_ethereum::Call::transact {
                    transaction: ethereum::TransactionV3::Legacy(ethereum::LegacyTransaction {
                        nonce: sp_core::U256::zero(),
                        gas_price: sp_core::U256::from(1_000_000_000_u64),
                        gas_limit: sp_core::U256::from(21_000),
                        action: ethereum::TransactionAction::Call(sp_core::H160::repeat_byte(2)),
                        value: sp_core::U256::zero(),
                        input: Vec::new(),
                        signature: ethereum::legacy::TransactionSignature::new(
                            27,
                            sp_core::H256::repeat_byte(1),
                            sp_core::H256::repeat_byte(1),
                        )
                        .unwrap(),
                    }),
                })
                .encode();
                supported = true;
            }
            let mut input = encoded.as_slice();
            let call = RuntimeCall::decode(&mut input);
            let Ok(call) = call else {
                unresolved.push(format!("{name}: custom decoding or finite value required"));
                continue;
            };
            if !supported || !input.is_empty() || call.encode() != encoded {
                unresolved.push(format!("{name}: noncanonical or recursive metadata"));
                continue;
            }
            // The pinned upstream contracts pallet consumes migration_noop on
            // the no-migration branch even if the caller supplies a smaller
            // limit (migration.rs:399). Its WeightMeter debug assertion then
            // panics. Exercise the dispatch with the sufficient documented
            // budget; keep that preexisting weight issue in the release report.
            let call = match call {
                RuntimeCall::Contracts(pallet_contracts::Call::migrate { .. }) => {
                    RuntimeCall::Contracts(pallet_contracts::Call::migrate {
                        weight_limit: <<Runtime as pallet_contracts::Config>::WeightInfo as pallet_contracts::WeightInfo>::migration_noop(),
                    })
                }
                call => call,
            };
            covered += 1;
            let ethereum = call.is_self_contained();
            self_contained += <usize as From<bool>>::from(ethereum);
            let mut outcomes = Vec::new();
            for mode in [Mode::Sr, Mode::Ed, Mode::HashedMs, Mode::FixedMs] {
                ext().execute_with(|| {
                    System::set_block_number(1);
                    pallet_admin_utils::HashedAccountsEnabled::<Runtime>::put(true);

                    let (account, tx) = signed(call.clone(), mode);
                    let protected = matches!(mode, Mode::HashedMs | Mode::FixedMs);
                    let checked = tx.clone().check(&ChainContext::default());
                    if ethereum {
                        assert!(
                            checked.is_err(),
                            "{name} {mode:?}: signed Ethereum wrapper accepted"
                        );
                        println!("MS_MATRIX {name} {mode:?} self-contained-signature-required");
                        return;
                    }
                    assert!(
                        checked.is_ok(),
                        "{name} {mode:?}: signature/envelope rejected"
                    );
                    let before = System::account_nonce(&account);
                    let validity = frame_support::storage::with_transaction_unchecked(|| {
                        frame_support::storage::TransactionOutcome::Rollback(
                            Executive::validate_transaction(
                                sp_runtime::transaction_validity::TransactionSource::External,
                                tx.clone(),
                                System::parent_hash(),
                            ),
                        )
                    });
                    // BlockBuilder rolls back an extrinsic rejected by Executive,
                    // including a signed mandatory call whose dispatch failure is
                    // classified as BadMandatory after extension preparation.
                    let result = frame_support::storage::with_transaction_unchecked(|| {
                        let result = Executive::apply_extrinsic(tx);
                        if result.is_ok() {
                            frame_support::storage::TransactionOutcome::Commit(result)
                        } else {
                            frame_support::storage::TransactionOutcome::Rollback(result)
                        }
                    });
                    if protected {
                        let generation =
                            HashedAccounts::mldsa_accounts(&account).unwrap().generation;
                        assert_eq!(
                            generation,
                            <u64 as From<bool>>::from(result.is_ok()),
                            "{name} {mode:?}"
                        );
                        assert_eq!(
                            System::account_nonce(&account),
                            before + <u32 as From<bool>>::from(result.is_ok()),
                            "{name} {mode:?}"
                        );
                    }
                    println!("MS_MATRIX {name} {mode:?} {result:?} pool={validity:?}");
                    outcomes.push((result, validity.map(|_| ())));
                });
            }
            if !ethereum {
                let [sr, ed, hashed_ms, fixed_ms] = outcomes.as_slice() else {
                    panic!("{name}: missing wallet result");
                };
                assert_eq!(sr, ed, "{name}: ordinary SR/ED outcome differs");
                assert_eq!(hashed_ms, fixed_ms, "{name}: MS profile outcome differs");
                assert_eq!(sr.1, hashed_ms.1, "{name}: pool validity differs");
                // These are the only expected dispatch differences for the
                // common minimal fixture. Enforce their exact authority errors
                // rather than allowing a whole pallet to escape parity checks.
                let expected: Option<(sp_runtime::DispatchResult, sp_runtime::DispatchResult)> =
                    match name.as_str() {
                        "Utility.as_derivative" => Some((
                            Ok(()),
                            Err(sp_runtime::DispatchError::Other(
                                "ProtectedDerivativeRequired",
                            )),
                        )),
                        "Utility.as_protected_derivative" => Some((
                            Err(sp_runtime::DispatchError::Other("PqParentRequired")),
                            Ok(()),
                        )),
                        "Proxy.add_proxy" | "Proxy.create_pure" => Some((
                            Ok(()),
                            Err(
                                crate::pallet_proxy::Error::<Runtime>::AccountPolicyViolation
                                    .into(),
                            ),
                        )),
                        "Proxy.set_real_pays_fee" => Some((
                            Err(crate::pallet_proxy::Error::<Runtime>::NotProxy.into()),
                            Err(
                                crate::pallet_proxy::Error::<Runtime>::AccountPolicyViolation
                                    .into(),
                            ),
                        )),
                        "Proxy.create_protected_pure" => Some((
                            Err(
                                crate::pallet_proxy::Error::<Runtime>::AccountPolicyViolation
                                    .into(),
                            ),
                            Ok(()),
                        )),
                        "HashedAccounts.install_capability" => Some((
                            Err(pallet_hashed_accounts::Error::<Runtime>::NotRegistered.into()),
                            Err(pallet_hashed_accounts::Error::<Runtime>::CapabilityExpired.into()),
                        )),
                        "HashedAccounts.revoke_capability" => Some((
                            Err(pallet_hashed_accounts::Error::<Runtime>::NotRegistered.into()),
                            Ok(()),
                        )),
                        _ => None,
                    };
                if let Some((classical, protected)) = expected {
                    assert_eq!(sr.0, Ok(classical), "{name}: classical policy changed");
                    assert_eq!(
                        hashed_ms.0,
                        Ok(protected),
                        "{name}: protected policy changed"
                    );
                } else {
                    assert_eq!(sr.0, hashed_ms.0, "{name}: dispatch parity failed");
                }
            }
        }
    }
    println!(
        "MS_MATRIX covered={covered} self_contained={self_contained} uninhabited={uninhabited:?} unresolved={unresolved:?}"
    );
    // New calls must never silently disappear from the report. A custom codec
    // needs an explicit fixture before this matrix can be called exhaustive.
    assert!(
        unresolved.is_empty(),
        "unresolved call fixtures: {unresolved:?}"
    );
    assert!(covered > 300, "unexpectedly incomplete runtime metadata");
}

#[test]
#[cfg(feature = "pow-faucet")]
fn malformed_faucet_work_is_a_dispatch_error_for_every_signing_mode() {
    for mode in [Mode::Sr, Mode::Ed, Mode::HashedMs, Mode::FixedMs] {
        for len in [0, 1, 31, 33, 64] {
            ext().execute_with(|| {
                System::set_block_number(1);
                let (account, tx) = signed(
                    RuntimeCall::SubtensorModule(pallet_subtensor::Call::faucet {
                        block_number: 1,
                        nonce: 0,
                        work: vec![0; len],
                    }),
                    mode,
                );
                let before = System::account_nonce(&account);
                assert_eq!(
                    Executive::apply_extrinsic(tx).unwrap(),
                    Err(pallet_subtensor::Error::<Runtime>::InvalidSeal.into()),
                    "{mode:?}: work length {len}"
                );
                assert_eq!(System::account_nonce(&account), before + 1);
                if matches!(mode, Mode::HashedMs | Mode::FixedMs) {
                    assert_eq!(
                        HashedAccounts::mldsa_accounts(account).unwrap().generation,
                        1
                    );
                }
                assert!(!System::events().iter().any(|event| matches!(
                    event.event,
                    RuntimeEvent::SubtensorModule(pallet_subtensor::Event::Faucet(..))
                )));
            });
        }
    }
}

#[test]
fn disclosed_unregistered_ms_address_can_be_blocked_by_hostile_hotkey_association() {
    super::mldsa::with_profiles(|| {
        System::set_block_number(1);

        hashed_auth::TestVerificationWeight::set(Weight::from_parts(10_000_000, 0));
        let descriptor = super::mldsa::descriptor(74);
        let victim = AccountId::new(mldsa::account_id(&descriptor));
        // An ordinary transfer to bare SS58 supplies no descriptor registration
        // guard. This public transaction discloses the target without its key.
        let amount = TaoBalance::new(100_000_000_000);
        let (_, funding) = signed(
            RuntimeCall::Balances(pallet_balances::Call::transfer_keep_alive {
                dest: victim.clone().into(),
                value: amount,
            }),
            Mode::Ed,
        );
        let public_funding_bytes = funding.encode();
        frame_support::assert_ok!(Executive::apply_extrinsic(funding).unwrap());
        assert_eq!(Balances::free_balance(&victim), amount);

        // The attacker learns the AccountId from the already included payment,
        // not by guessing its hash, seeing a descriptor, or racing that payment.
        let observed_call = UncheckedExtrinsic::decode(&mut public_funding_bytes.as_slice())
            .unwrap()
            .into_call();
        let observed_recipient = match observed_call {
            RuntimeCall::Balances(pallet_balances::Call::transfer_keep_alive {
                dest: Address::Id(recipient),
                ..
            }) => recipient,
            _ => panic!("funding fixture must disclose a bare destination AccountId"),
        };
        let (attacker, claim) = signed(
            RuntimeCall::SubtensorModule(pallet_subtensor::Call::try_associate_hotkey {
                hotkey: observed_recipient,
            }),
            Mode::Sr,
        );
        frame_support::assert_ok!(Executive::apply_extrinsic(claim).unwrap());
        assert_eq!(pallet_subtensor::Owner::<Runtime>::get(&victim), attacker);
        assert!(!HashedAccounts::is_protected(&victim));
        let (_, registration) = signed(
            RuntimeCall::HashedAccounts(pallet_hashed_accounts::Call::register {
                descriptor: descriptor.into(),
            }),
            Mode::Ed,
        );
        assert_eq!(
            Executive::apply_extrinsic(registration).unwrap(),
            Err(sp_runtime::DispatchError::Other(
                "MlDsaHotkeyRequiresPqOwner"
            ))
        );
        assert!(!HashedAccounts::is_protected(&victim));
        assert_eq!(Balances::free_balance(&victim), amount);

        // Possession of the genuine initial MS key cannot bypass the missing
        // registry record, even for a transfer recovering only native TAO.
        let call = RuntimeCall::Balances(pallet_balances::Call::transfer_keep_alive {
            dest: attacker.clone().into(),
            value: TaoBalance::new(1_000_000_000),
        });
        let extra = extra(System::account_nonce(&victim));
        let seed = super::mldsa::seed(74, 0);
        let header = mldsa::Header {
            account: *victim.as_ref(),
            version: mldsa::VERSION,
            algorithm: descriptor.algorithm,
            generation: 0,
            public_key: mldsa::public_key_from_seed(&seed).unwrap(),
            next_commitment: mldsa::commitment(
                &mldsa::public_key_from_seed(&super::mldsa::seed(74, 1)).unwrap(),
            ),
        };
        let address = header
            .sign(
                &seed,
                &(&call, &extra, extra.implicit().unwrap()).encode(),
                &[0; 32],
            )
            .unwrap();
        let recovery = UncheckedExtrinsic::new_signed(
            call,
            Address::Raw(address.encode()),
            Signature::Ed25519(ed25519::Signature::from_raw(mldsa::CARRIER_SIGNATURE)),
            extra,
        );
        assert!(matches!(
            recovery.check(&ChainContext::default()),
            Err(
                sp_runtime::transaction_validity::TransactionValidityError::Invalid(
                    InvalidTransaction::BadProof
                )
            )
        ));
        assert_eq!(pallet_subtensor::Owner::<Runtime>::get(&victim), attacker);
        assert_eq!(Balances::free_balance(&victim), amount);
    });
}

#[test]
fn registered_ms_address_rejects_hostile_weak_owner_and_remains_spendable() {
    super::mldsa::with_profiles(|| {
        let victim = super::mldsa::register(74);
        let record = HashedAccounts::mldsa_accounts(&victim).unwrap();
        let (attacker, claim) = signed(
            RuntimeCall::SubtensorModule(pallet_subtensor::Call::try_associate_hotkey {
                hotkey: victim.clone(),
            }),
            Mode::Sr,
        );
        assert_eq!(
            Executive::apply_extrinsic(claim).unwrap(),
            Err(pallet_subtensor::Error::<Runtime>::HotkeyOwnerPolicyViolation.into())
        );
        assert!(!pallet_subtensor::Owner::<Runtime>::contains_key(&victim));
        assert_eq!(HashedAccounts::mldsa_accounts(&victim).unwrap(), record);

        let before = Balances::free_balance(&attacker);
        let amount = TaoBalance::new(1_000_000_000);
        let transfer = super::mldsa::sign(
            74,
            RuntimeCall::Balances(pallet_balances::Call::transfer_keep_alive {
                dest: attacker.clone().into(),
                value: amount,
            }),
        );
        frame_support::assert_ok!(Executive::apply_extrinsic(transfer).unwrap());
        assert_eq!(Balances::free_balance(&attacker), before + amount);
        assert_eq!(
            HashedAccounts::mldsa_accounts(&victim).unwrap().generation,
            1
        );
    });
}

#[test]
fn atomic_ms_registration_payment_does_not_fund_a_hostile_association() {
    super::mldsa::with_profiles(|| {
        System::set_block_number(1);

        hashed_auth::TestVerificationWeight::set(Weight::from_parts(10_000_000, 0));
        let descriptor = super::mldsa::descriptor(74);
        let victim = AccountId::new(mldsa::account_id(&descriptor));
        let (attacker, claim) = signed(
            RuntimeCall::SubtensorModule(pallet_subtensor::Call::try_associate_hotkey {
                hotkey: victim.clone(),
            }),
            Mode::Sr,
        );
        frame_support::assert_ok!(Executive::apply_extrinsic(claim).unwrap());
        let reserved = Balances::reserved_balance(&attacker);
        let (_, payment) = signed(
            RuntimeCall::Utility(pallet_utility::Call::batch_all {
                calls: vec![
                    RuntimeCall::HashedAccounts(pallet_hashed_accounts::Call::register {
                        descriptor: descriptor.into(),
                    }),
                    RuntimeCall::Balances(pallet_balances::Call::transfer_keep_alive {
                        dest: victim.clone().into(),
                        value: TaoBalance::new(1_000_000_000),
                    }),
                ],
            }),
            Mode::Sr,
        );
        assert_eq!(
            Executive::apply_extrinsic(payment).unwrap(),
            Err(sp_runtime::DispatchError::Other(
                "MlDsaHotkeyRequiresPqOwner"
            ))
        );
        assert_eq!(Balances::free_balance(&victim), TaoBalance::new(0));
        assert_eq!(Balances::reserved_balance(&attacker), reserved);
        assert_eq!(pallet_subtensor::Owner::<Runtime>::get(&victim), attacker);
        assert!(!HashedAccounts::is_protected(&victim));
        assert!(
            pallet_hashed_accounts::EvmAliases::<Runtime>::get(HashedAccounts::evm_alias(&victim))
                .is_none()
        );
    });
}
