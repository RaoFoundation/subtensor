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
        assert_ok!(HashedAccounts::do_install_capability(
            &account,
            1,
            descriptor.initial_commitment,
            10,
            trading_scope()
        ));
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
        assert_noop!(
            HashedAccounts::do_install_capability(&account, 2, [5; 64], 10, trading_scope()),
            Error::<Test>::Disabled
        );
        assert_noop!(
            HashedAccounts::checked_capability(&account, CapabilityRole::Order, 1, &public),
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
        // Revocation only removes authority and remains available during a pause.
        assert_ok!(HashedAccounts::do_revoke_capability(
            &account,
            CapabilityRole::Order
        ));
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

fn trading_scope() -> CapabilityScope {
    CapabilityScope::Trading(TradingScope {
        netuid: 1,
        hotkey: AccountId32::new([30; 32]),
        fee_recipient: AccountId32::new([31; 32]),
        max_fee_perbill: 100,
        max_tao_per_order: 40,
        max_alpha_per_order: 20,
        total_tao: 60,
        total_alpha: 30,
    })
}

#[test]
fn capabilities_bind_signed_owners_and_have_independent_monotonic_epochs() {
    new_test_ext().execute_with(|| {
        let owner = register(2);
        let public = [22; 1952];
        let commitment = subtensor_hashed::mldsa::commitment(&public);
        assert_noop!(
            HashedAccounts::install_capability(
                RuntimeOrigin::none(),
                1,
                commitment,
                10,
                trading_scope()
            ),
            sp_runtime::DispatchError::BadOrigin
        );
        assert_ok!(HashedAccounts::install_capability(
            RuntimeOrigin::signed(sponsor()),
            1,
            commitment,
            10,
            trading_scope()
        ));
        assert!(
            HashedAccounts::checked_capability(&sponsor(), CapabilityRole::Order, 1, &public)
                .is_ok()
        );
        assert_ok!(HashedAccounts::revoke_capability(
            RuntimeOrigin::signed(sponsor()),
            CapabilityRole::Order
        ));

        assert_ok!(HashedAccounts::install_capability(
            RuntimeOrigin::signed(owner.clone()),
            1,
            commitment,
            10,
            trading_scope()
        ));
        assert!(
            HashedAccounts::checked_capability(&owner, CapabilityRole::Order, 1, &public).is_ok()
        );
        assert_noop!(
            HashedAccounts::install_capability(
                RuntimeOrigin::signed(owner.clone()),
                1,
                commitment,
                10,
                trading_scope()
            ),
            Error::<Test>::CapabilityEpochMismatch
        );
        assert_noop!(
            HashedAccounts::checked_capability(&owner, CapabilityRole::Order, 1, &[23; 1952]),
            Error::<Test>::WrongCommitment
        );
        assert_ok!(HashedAccounts::revoke_capability(
            RuntimeOrigin::signed(owner.clone()),
            CapabilityRole::Order
        ));
        assert_eq!(
            CapabilityEpochs::<Test>::get(&owner, CapabilityRole::Order),
            2
        );
        assert_noop!(
            HashedAccounts::checked_capability(&owner, CapabilityRole::Order, 1, &public),
            Error::<Test>::CapabilityMissing
        );
        assert_ok!(HashedAccounts::install_capability(
            RuntimeOrigin::signed(owner.clone()),
            3,
            commitment,
            10,
            trading_scope()
        ));
        assert_noop!(
            HashedAccounts::checked_capability(&owner, CapabilityRole::Order, 1, &public),
            Error::<Test>::CapabilityEpochMismatch
        );
        assert_eq!(
            CapabilityEpochs::<Test>::get(&owner, CapabilityRole::Message),
            0
        );
    });
}

#[test]
fn capability_expiry_and_epoch_exhaustion_fail_closed_but_allow_revocation() {
    new_test_ext().execute_with(|| {
        let owner = register(2);
        let public = [22; 1952];
        let commitment = subtensor_hashed::mldsa::commitment(&public);
        assert_noop!(
            HashedAccounts::install_capability(
                RuntimeOrigin::signed(owner.clone()),
                1,
                commitment,
                1,
                trading_scope()
            ),
            Error::<Test>::CapabilityExpired
        );
        assert_ok!(HashedAccounts::install_capability(
            RuntimeOrigin::signed(owner.clone()),
            1,
            commitment,
            10,
            trading_scope()
        ));
        System::set_block_number(10);
        assert!(
            HashedAccounts::checked_capability(&owner, CapabilityRole::Order, 1, &public).is_ok()
        );
        System::set_block_number(11);
        assert_noop!(
            HashedAccounts::checked_capability(&owner, CapabilityRole::Order, 1, &public),
            Error::<Test>::CapabilityExpired
        );
        CapabilityEpochs::<Test>::insert(&owner, CapabilityRole::Order, u64::MAX);
        assert_ok!(HashedAccounts::revoke_capability(
            RuntimeOrigin::signed(owner.clone()),
            CapabilityRole::Order
        ));
        assert!(ActiveCapabilities::<Test>::get(&owner, CapabilityRole::Order).is_none());
        assert_eq!(
            CapabilityEpochs::<Test>::get(&owner, CapabilityRole::Order),
            u64::MAX
        );
        assert_noop!(
            HashedAccounts::install_capability(
                RuntimeOrigin::signed(owner),
                u64::MAX,
                commitment,
                100,
                trading_scope()
            ),
            Error::<Test>::GenerationExhausted
        );
    });
}

#[test]
fn trading_capability_enforces_asset_recipient_fee_and_separate_total_budgets() {
    new_test_ext().execute_with(|| {
        let owner = register(2);
        let public = [22; 1952];
        assert_ok!(HashedAccounts::install_capability(
            RuntimeOrigin::signed(owner.clone()),
            1,
            subtensor_hashed::mldsa::commitment(&public),
            100,
            trading_scope()
        ));
        let hotkey = AccountId32::new([30; 32]);
        let fee_recipient = AccountId32::new([31; 32]);
        let check = |netuid, hot, recipient, fee, buy, full, fill| {
            HashedAccounts::check_order_capability(
                &owner, 1, &public, netuid, hot, recipient, fee, buy, full, fill,
            )
        };
        assert_ok!(check(1, &hotkey, &fee_recipient, 100, true, 40, 30));
        assert_noop!(
            check(2, &hotkey, &fee_recipient, 100, true, 40, 30),
            Error::<Test>::CapabilityScopeViolation
        );
        assert_noop!(
            check(1, &owner, &fee_recipient, 100, true, 40, 30),
            Error::<Test>::CapabilityScopeViolation
        );
        assert_noop!(
            check(1, &hotkey, &owner, 100, true, 40, 30),
            Error::<Test>::CapabilityScopeViolation
        );
        assert_noop!(
            check(1, &hotkey, &fee_recipient, 101, true, 40, 30),
            Error::<Test>::CapabilityScopeViolation
        );
        assert_noop!(
            check(1, &hotkey, &fee_recipient, 100, true, 41, 30),
            Error::<Test>::CapabilityScopeViolation
        );
        assert_ok!(HashedAccounts::consume_order_capability(
            &owner, 1, true, 30
        ));
        assert_ok!(check(1, &hotkey, &fee_recipient, 100, true, 40, 30));
        assert_ok!(HashedAccounts::consume_order_capability(
            &owner, 1, true, 30
        ));
        assert_noop!(
            check(1, &hotkey, &fee_recipient, 100, true, 1, 1),
            Error::<Test>::CapabilityScopeViolation
        );
        assert_ok!(check(1, &hotkey, &fee_recipient, 100, false, 20, 20));
        assert_ok!(HashedAccounts::consume_order_capability(
            &owner, 1, false, 20
        ));
        assert_noop!(
            HashedAccounts::consume_order_capability(&owner, 1, false, 11),
            Error::<Test>::CapabilityScopeViolation
        );
        let record = ActiveCapabilities::<Test>::get(&owner, CapabilityRole::Order).unwrap();
        assert_eq!((record.spent_tao, record.spent_alpha), (60, 20));
    });
}

#[test]
fn message_capability_does_not_authorize_trading() {
    new_test_ext().execute_with(|| {
        let owner = register(2);
        let public = [22; 1952];
        assert_ok!(HashedAccounts::install_capability(
            RuntimeOrigin::signed(owner.clone()),
            1,
            subtensor_hashed::mldsa::commitment(&public),
            100,
            CapabilityScope::Message {
                audience: b"subnet.example".to_vec().try_into().unwrap()
            }
        ));
        assert!(
            HashedAccounts::checked_capability(&owner, CapabilityRole::Message, 1, &public).is_ok()
        );
        assert_noop!(
            HashedAccounts::check_order_capability(
                &owner, 1, &public, 1, &owner, &owner, 0, true, 1, 1
            ),
            Error::<Test>::CapabilityMissing
        );
    });
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
