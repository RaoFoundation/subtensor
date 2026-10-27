#![allow(clippy::unwrap_used)]
use super::*;
use crate::tests::{
    Balances, HashedAccounts, RejectRegistration, RuntimeOrigin, System, Test, new_test_ext,
    sponsor,
};
use frame_support::{assert_noop, assert_ok, traits::Currency};
use sp_runtime::DispatchError;

fn fixture(byte: u8) -> (subtensor_hashed::mldsa::Descriptor, AccountId32, [u8; 1952]) {
    let public = [byte; 1952];
    let descriptor = subtensor_hashed::mldsa::Descriptor {
        version: 2,
        algorithm: 4,
        initial_commitment: subtensor_hashed::mldsa::key_commitment(&public),
    };
    let account = AccountId32::new(subtensor_hashed::mldsa::account_id(&descriptor));
    (descriptor, account, public)
}

#[test]
fn master_switch_preserves_registered_ms_state_for_resume() {
    new_test_ext().execute_with(|| {
        let (descriptor, account, public) = fixture(2);
        let next = [9; 64];
        let register =
            || HashedAccounts::register(RuntimeOrigin::signed(sponsor()), descriptor.into());
        crate::tests::Enabled::set(false);
        assert_noop!(register(), Error::<Test>::Disabled);
        assert!(!MlDsaAccounts::<Test>::contains_key(&account));
        crate::tests::Enabled::set(true);
        assert_ok!(register());
        let derived = DerivedDescriptor::Derivative {
            parent: account.clone(),
            index: 1,
        };
        assert_ok!(HashedAccounts::register_derived_with_sponsor(
            &sponsor(),
            derived.clone()
        ));
        let authorize = || {
            HashedAccounts::check_mldsa_authorization(
                &account,
                descriptor.algorithm,
                &public,
                0,
                next,
            )
        };
        let ticket = authorize().unwrap();
        let before = MlDsaAccounts::<Test>::get(&account);
        let derived_before = DerivedAuthorities::<Test>::get(derived.account_id());
        crate::tests::Enabled::set(false);
        assert_noop!(register(), Error::<Test>::Disabled);
        assert_noop!(authorize(), Error::<Test>::Disabled);
        // A ticket validated before a toggle must fail during preparation.
        assert_noop!(
            HashedAccounts::advance_mldsa(&ticket),
            Error::<Test>::Disabled
        );
        assert_noop!(
            HashedAccounts::check_registered(RuntimeOrigin::signed(sponsor()), descriptor.into()),
            Error::<Test>::Disabled
        );
        assert_noop!(
            HashedAccounts::register_derived_with_sponsor(&sponsor(), derived.clone()),
            Error::<Test>::Disabled
        );
        assert_noop!(
            HashedAccounts::register_derived_with_sponsor(
                &sponsor(),
                DerivedDescriptor::Derivative {
                    parent: account.clone(),
                    index: 2
                }
            ),
            Error::<Test>::Disabled
        );
        assert_noop!(
            HashedAccounts::check_derived(RuntimeOrigin::signed(sponsor()), derived.clone()),
            Error::<Test>::Disabled
        );
        assert_eq!(MlDsaAccounts::<Test>::get(&account), before);
        assert_eq!(
            DerivedAuthorities::<Test>::get(derived.account_id()),
            derived_before
        );
        for protected in [&account, &derived.account_id()] {
            assert!(HashedAccounts::is_protected(protected));
            assert_eq!(
                EvmAliases::<Test>::get(HashedAccounts::evm_alias(protected)),
                Some(protected.clone())
            );
        }
        crate::tests::Enabled::set(true);
        assert_ok!(authorize());
        let events = System::events();
        assert_ok!(HashedAccounts::advance_mldsa(&ticket));
        assert_eq!(MlDsaAccounts::<Test>::get(&account).unwrap().generation, 1);
        assert_eq!(
            MlDsaAccounts::<Test>::get(&account).unwrap().commitment,
            next
        );
        assert_eq!(System::events(), events);
    });
}

fn register(byte: u8) -> AccountId32 {
    let (descriptor, account, _) = fixture(byte);
    assert_ok!(HashedAccounts::register(
        RuntimeOrigin::signed(sponsor()),
        descriptor.into()
    ));
    account
}

fn multisig(mut members: Vec<AccountId32>, threshold: u16) -> DerivedDescriptor {
    members.sort();
    DerivedDescriptor::Multisig {
        threshold,
        signatories: members.try_into().unwrap(),
    }
}

#[test]
fn disabling_never_erases_zero_balance_account_protection() {
    new_test_ext().execute_with(|| {
        let (descriptor, account, _) = fixture(2);
        assert_ok!(HashedAccounts::register(
            RuntimeOrigin::signed(sponsor()),
            descriptor.into()
        ));
        crate::tests::Enabled::set(false);
        let _ = Balances::make_free_balance_be(&account, 0);
        assert!(HashedAccounts::is_protected(&account));
        assert!(!HashedAccounts::is_registered(&account));
        assert!(HashedAccounts::authority_meets(
            &account,
            SecurityFloor::PqRequired
        ));
        assert_eq!(
            EvmAliases::<Test>::get(HashedAccounts::evm_alias(&account)),
            Some(account.clone())
        );
        assert_eq!(System::providers(&account), 1);
    });
}

#[test]
fn mldsa_ticket_is_read_only_single_use_and_rechecks_both_state_fields() {
    new_test_ext().execute_with(|| {
        let (descriptor, account, public) = fixture(2);
        assert_ok!(HashedAccounts::register(
            RuntimeOrigin::signed(sponsor()),
            descriptor.into()
        ));
        let before = sp_io::storage::root(sp_runtime::StateVersion::V1);
        let ticket = HashedAccounts::check_mldsa_authorization(
            &account,
            subtensor_hashed::mldsa::ALGORITHM,
            &public,
            0,
            [9; 64],
        )
        .unwrap();
        assert_eq!(sp_io::storage::root(sp_runtime::StateVersion::V1), before);
        assert_ok!(HashedAccounts::advance_mldsa(&ticket));
        assert_noop!(
            HashedAccounts::advance_mldsa(&ticket),
            Error::<Test>::WrongGeneration
        );
        assert_eq!(
            MlDsaAccounts::<Test>::get(&account).unwrap().commitment,
            [9; 64]
        );
        assert_eq!(System::account_nonce(&account), 1); // independent counter
        assert_ok!(HashedAccounts::register(
            RuntimeOrigin::signed(AccountId32::new([88; 32])),
            descriptor.into()
        ));
        assert_eq!(MlDsaAccounts::<Test>::get(&account).unwrap().generation, 1);
        assert_eq!(Balances::reserved_balance(sponsor()), 20);
        MlDsaAccounts::<Test>::mutate(&account, |record| record.as_mut().unwrap().generation = 0);
        assert_noop!(
            HashedAccounts::advance_mldsa(&ticket),
            Error::<Test>::WrongCommitment
        );
    });
}

#[test]
fn mldsa_rejects_invalid_state_commitments_and_exhaustion() {
    new_test_ext().execute_with(|| {
        let (_, account, public) = fixture(2);
        register(2);
        assert_noop!(
            HashedAccounts::check_mldsa_authorization(
                &account,
                subtensor_hashed::mldsa::ALGORITHM,
                &public,
                1,
                [9; 64]
            ),
            Error::<Test>::WrongGeneration
        );
        assert_noop!(
            HashedAccounts::check_mldsa_authorization(
                &account,
                subtensor_hashed::mldsa::ALGORITHM,
                &[3; 1952],
                0,
                [9; 64]
            ),
            Error::<Test>::WrongCommitment
        );
        assert_noop!(
            HashedAccounts::check_mldsa_authorization(
                &account,
                subtensor_hashed::mldsa::ALGORITHM,
                &public,
                0,
                [0; 64]
            ),
            Error::<Test>::InvalidNextCommitment
        );
        assert_noop!(
            HashedAccounts::check_mldsa_authorization(
                &account,
                subtensor_hashed::mldsa::ALGORITHM,
                &public,
                0,
                subtensor_hashed::mldsa::key_commitment(&public)
            ),
            Error::<Test>::InvalidNextCommitment
        );
        MlDsaAccounts::<Test>::mutate(&account, |record| {
            record.as_mut().unwrap().generation = u64::MAX
        });
        assert_noop!(
            HashedAccounts::check_mldsa_authorization(
                &account,
                subtensor_hashed::mldsa::ALGORITHM,
                &public,
                u64::MAX,
                [9; 64]
            ),
            Error::<Test>::GenerationExhausted
        );
    });
}

#[test]
fn registration_conflicts_and_hook_failures_do_not_leave_partial_state() {
    new_test_ext().execute_with(|| {
        let (descriptor, account, _) = fixture(2);
        RejectRegistration::set(true);
        assert_noop!(
            HashedAccounts::register(RuntimeOrigin::signed(sponsor()), descriptor.into()),
            DispatchError::Other("incompatible authority")
        );
        RejectRegistration::set(false);
        assert_eq!(Balances::reserved_balance(sponsor()), 0);
        assert_eq!(System::providers(&account), 0);
        assert!(EvmAliases::<Test>::get(HashedAccounts::evm_alias(&account)).is_none());
        Accounts::<Test>::insert(
            &account,
            AccountRecord {
                descriptor: Descriptor {
                    version: 1,
                    scheme: Scheme::Sr25519,
                    initial_commitment: [1; 32],
                },
                generation: 0,
                commitment: [1; 32],
            },
        );
        assert_noop!(
            HashedAccounts::register(RuntimeOrigin::signed(sponsor()), descriptor.into()),
            Error::<Test>::AuthenticationKindConflict
        );
    });
}

#[test]
fn mixed_multisig_requires_a_pq_member_in_every_threshold_and_preserves_address() {
    new_test_ext().execute_with(|| {
        let first = register(2);
        let second = register(3);
        let classical = AccountId32::new([44; 32]);
        let descriptor = multisig(vec![first.clone(), second, classical.clone()], 2);
        let DerivedDescriptor::Multisig {
            threshold,
            signatories,
        } = &descriptor
        else {
            unreachable!()
        };
        assert_eq!(
            descriptor.account_id(),
            AccountId32::new(sp_io::hashing::blake2_256(
                &(b"modlpy/utilisuba", signatories.as_slice(), *threshold).encode()
            ))
        );
        let account = descriptor.account_id();
        assert_ok!(HashedAccounts::register_derived(
            RuntimeOrigin::signed(sponsor()),
            descriptor.clone()
        ));
        assert!(HashedAccounts::authority_meets(
            &account,
            SecurityFloor::PqRequired
        ));
        assert_eq!(
            HashedAccounts::authentication_kind(&account),
            AuthenticationKind::DerivedPq
        );
        assert!(!HashedAccounts::is_registered(&account));
        assert!(HashedAccounts::mldsa_accounts(&account).is_none());
        assert_eq!(
            EvmAliases::<Test>::get(HashedAccounts::evm_alias(&account)),
            Some(account.clone())
        );
        assert_ok!(HashedAccounts::register_derived(
            RuntimeOrigin::signed(classical.clone()),
            descriptor
        ));
        assert_eq!(Balances::reserved_balance(sponsor()), 70);
        let unsafe_group = multisig(vec![first, classical, AccountId32::new([45; 32])], 2);
        assert_noop!(
            HashedAccounts::register_derived(RuntimeOrigin::signed(sponsor()), unsafe_group),
            Error::<Test>::InvalidDerivedAuthority
        );
    });
}

#[test]
fn derivative_graph_is_bounded_and_disabled_switch_preserves_all_aliases() {
    new_test_ext().execute_with(|| {
        let mut parent = register(2);
        for index in 0..u16::from(MAX_DERIVED_DEPTH) {
            let descriptor = DerivedDescriptor::Derivative { parent, index };
            parent = descriptor.account_id();
            assert_ok!(HashedAccounts::register_derived(
                RuntimeOrigin::signed(sponsor()),
                descriptor
            ));
        }
        let excessive = DerivedDescriptor::Derivative {
            parent: parent.clone(),
            index: 100,
        };
        assert_noop!(
            HashedAccounts::register_derived(RuntimeOrigin::signed(sponsor()), excessive),
            Error::<Test>::DerivedAuthorityTooComplex
        );
        crate::tests::Enabled::set(false);
        assert!(HashedAccounts::is_protected(&parent));
        assert_eq!(
            EvmAliases::<Test>::get(HashedAccounts::evm_alias(&parent)),
            Some(parent)
        );
    });
}

#[test]
fn pure_proxy_derivation_does_not_length_prefix_encoded_proxy_type() {
    let who = AccountId32::new([12; 32]);
    let descriptor = DerivedDescriptor::PureProxy {
        spawner: who.clone(),
        proxy_type: vec![3].try_into().unwrap(),
        index: 9,
        height: 17,
        extrinsic_index: 2,
    };
    let expected = (b"modlpy/proxy____", &who, 17u32, 2u32, 3u8, 9u16)
        .using_encoded(sp_io::hashing::blake2_256);
    assert_eq!(descriptor.account_id(), AccountId32::new(expected));
}

#[test]
fn sponsored_derived_registration_requires_owner_and_payer_consent_and_never_double_charges() {
    new_test_ext().execute_with(|| {
        let first = register(2);
        let second = register(3);
        let descriptor = multisig(vec![first.clone(), second], 2);
        let owner = descriptor.account_id();
        let payer = sponsor();
        let before = Balances::reserved_balance(&payer);
        assert_noop!(
            HashedAccounts::register_derived_sponsored(
                RuntimeOrigin::signed(first),
                descriptor.clone(),
                payer.clone()
            ),
            Error::<Test>::InvalidDerivedAuthority
        );
        crate::tests::RejectSponsorship::set(true);
        assert_noop!(
            HashedAccounts::register_derived_sponsored(
                RuntimeOrigin::signed(owner.clone()),
                descriptor.clone(),
                payer.clone()
            ),
            DispatchError::Other("sponsor did not consent")
        );
        assert!(!HashedAccounts::is_protected(&owner));
        assert_eq!(Balances::reserved_balance(&payer), before);
        crate::tests::RejectSponsorship::set(false);
        assert_ok!(HashedAccounts::register_derived_sponsored(
            RuntimeOrigin::signed(owner.clone()),
            descriptor.clone(),
            payer.clone()
        ));
        assert!(HashedAccounts::is_protected(&owner));
        assert_eq!(Balances::free_balance(&owner), 0);
        assert_eq!(
            Balances::reserved_balance(&payer),
            before.saturating_add(30)
        );
        crate::tests::RejectSponsorship::set(true);
        assert_ok!(HashedAccounts::register_derived_sponsored(
            RuntimeOrigin::signed(owner),
            descriptor,
            AccountId32::new([99; 32])
        ));
        assert_eq!(
            Balances::reserved_balance(&payer),
            before.saturating_add(30)
        );
    });
}

#[test]
fn later_weak_member_protection_cannot_expand_an_existing_security_proof() {
    new_test_ext().execute_with(|| {
        let first = register(2);
        let second = register(3);
        let mut future_parent = first.clone();
        let mut future_chain = Vec::new();
        for index in 0u16..u16::from(MAX_DERIVED_DEPTH) {
            let descriptor = DerivedDescriptor::Derivative {
                parent: future_parent,
                index,
            };
            future_parent = descriptor.account_id();
            future_chain.push(descriptor);
        }
        let initially_weak = future_parent;
        let descriptor = multisig(
            vec![first.clone(), second.clone(), initially_weak.clone()],
            2,
        );
        let composite = descriptor.account_id();
        assert_ok!(HashedAccounts::register_derived(
            RuntimeOrigin::signed(sponsor()),
            descriptor
        ));
        let before = DerivedAuthorities::<Test>::get(&composite).unwrap();
        assert_eq!((before.depth, before.nodes), (1, 4));
        assert_eq!(before.pq_dependencies.len(), 2);
        assert!(before.pq_dependencies.contains(&first));
        assert!(before.pq_dependencies.contains(&second));
        assert!(!before.pq_dependencies.contains(&initially_weak));

        for descriptor in future_chain {
            assert_ok!(HashedAccounts::register_derived(
                RuntimeOrigin::signed(sponsor()),
                descriptor
            ));
        }
        assert_eq!(
            DerivedAuthorities::<Test>::get(&initially_weak)
                .unwrap()
                .depth,
            MAX_DERIVED_DEPTH
        );
        assert_eq!(DerivedAuthorities::<Test>::get(&composite).unwrap(), before);
        // The optional member now has depth16. A new child still depends on
        // composite's original depth1 proof, not an expanded depth17 graph.
        let child = DerivedDescriptor::Derivative {
            parent: composite,
            index: 99,
        };
        let child_id = child.account_id();
        assert_ok!(HashedAccounts::register_derived(
            RuntimeOrigin::signed(sponsor()),
            child
        ));
        assert_eq!(DerivedAuthorities::<Test>::get(child_id).unwrap().depth, 2);
    });
}

#[test]
fn fixed_profile_authenticates_without_registration_or_authority_state() {
    new_test_ext().execute_with(|| {
        let (descriptor, account, public) = (0u8..=u8::MAX)
            .find_map(|seed| {
                let public = subtensor_hashed::mldsa::public_key_from_seed(&[seed; 32]).unwrap();
                let descriptor = subtensor_hashed::mldsa::Descriptor {
                    version: 2,
                    algorithm: subtensor_hashed::mldsa::FIXED_ALGORITHM,
                    initial_commitment: subtensor_hashed::mldsa::key_commitment(&public),
                };
                let account = subtensor_hashed::mldsa::account_id(&descriptor);
                subtensor_hashed::mldsa::rejects_legacy_public_keys(&account).then_some((
                    descriptor,
                    AccountId32::new(account),
                    public,
                ))
            })
            .unwrap();
        let deposit_before = Balances::reserved_balance(sponsor());
        assert_noop!(
            HashedAccounts::register(RuntimeOrigin::signed(sponsor()), descriptor.into()),
            Error::<Test>::UnsupportedDescriptor
        );
        assert_eq!(Balances::reserved_balance(sponsor()), deposit_before);
        assert!(!HashedAccounts::authority_meets(
            &account,
            SecurityFloor::PqRequired
        ));
        let initial = descriptor.initial_commitment;
        for (profile, generation, next) in [(4, 0, initial), (5, 0, [9; 64]), (5, 1, initial)] {
            assert!(
                HashedAccounts::check_mldsa_authorization(
                    &account, profile, &public, generation, next
                )
                .is_err()
            );
        }
        let proof =
            HashedAccounts::check_mldsa_authorization(&account, 5, &public, 0, initial).unwrap();
        crate::tests::Enabled::set(false);
        assert_noop!(
            HashedAccounts::check_mldsa_authorization(&account, 5, &public, 0, initial),
            Error::<Test>::Disabled
        );
        assert_noop!(
            HashedAccounts::advance_mldsa(&proof),
            Error::<Test>::Disabled
        );
        crate::tests::Enabled::set(true);
        assert_ok!(HashedAccounts::advance_mldsa(&proof));
        System::inc_account_nonce(&account);
        assert_ok!(HashedAccounts::advance_mldsa(&proof));
        assert_eq!(System::account(&account).sufficients, 1);
        assert!(!MlDsaAccounts::<Test>::contains_key(&account));
    });
}
