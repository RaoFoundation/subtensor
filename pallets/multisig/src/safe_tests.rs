//! Safe-address regressions use real AccountId32 encodings; legacy upstream
//! tests retain their original u64 runtime and golden behavior.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::indexing_slicing)]

use super::*;
use crate as pallet_multisig;
use frame::{deps::sp_runtime::AccountId32, prelude::*, runtime::prelude::*, testing_prelude::*};

type Block = frame_system::mocking::MockBlockU32<Test>;

construct_runtime!(
    pub struct Test {
        System: frame_system,
        Balances: pallet_balances,
        Multisig: pallet_multisig,
    }
);

#[derive_impl(frame_system::config_preludes::TestDefaultConfig)]
impl frame_system::Config for Test {
    type Block = Block;
    type AccountId = AccountId32;
    type Lookup = frame::deps::sp_runtime::traits::IdentityLookup<AccountId32>;
    type AccountData = pallet_balances::AccountData<u64>;
}

#[derive_impl(pallet_balances::config_preludes::TestDefaultConfig)]
impl pallet_balances::Config for Test {
    type ReserveIdentifier = [u8; 8];
    type AccountStore = System;
}

parameter_types! {
    pub static DepositBase: u64 = 1;
    pub static DepositFactor: u64 = 1;
    // A small nonzero test weight checks that refunds retain the derivation charge.
    pub static DerivationWeight: Weight = Weight::from_parts(123, 456);
}

impl Config for Test {
    type RuntimeEvent = RuntimeEvent;
    type RuntimeCall = RuntimeCall;
    type Currency = Balances;
    type DepositBase = DepositBase;
    type DepositFactor = DepositFactor;
    type MaxSignatories = ConstU32<4>;
    type WeightInfo = ();
    type SafeDerivationWeight = DerivationWeight;
    type BlockNumberProvider = System;
}

fn account(value: u8) -> AccountId32 {
    AccountId32::new([value; 32])
}
fn members() -> Vec<AccountId32> {
    vec![account(1), account(2), account(3)]
}

pub fn new_test_ext() -> TestState {
    let mut storage = frame_system::GenesisConfig::<Test>::default()
        .build_storage()
        .unwrap();
    pallet_balances::GenesisConfig::<Test> {
        balances: (1..=5).map(|n| (account(n), 100)).collect(),
        ..Default::default()
    }
    .assimilate_storage(&mut storage)
    .unwrap();
    let mut ext = TestState::new(storage);
    ext.execute_with(|| {
        DepositBase::set(1);
        DepositFactor::set(1);
        DerivationWeight::set(Weight::from_parts(123, 456));
        System::set_block_number(1);
    });
    ext
}

fn transfer(amount: u64) -> Box<RuntimeCall> {
    Box::new(RuntimeCall::Balances(
        pallet_balances::Call::transfer_allow_death {
            dest: account(9),
            value: amount,
        },
    ))
}

#[test]
fn safe_full_width_address_matches_shared_helper_and_legacy_is_unchanged() {
    let signatories = members();
    let legacy = Multisig::multi_account_id(&signatories, 2);
    assert_eq!(
        legacy,
        AccountId32::new([
            63, 253, 32, 235, 151, 186, 253, 111, 90, 248, 208, 38, 205, 17, 16, 31, 21, 83, 250,
            156, 180, 184, 163, 125, 23, 33, 49, 79, 187, 180, 143, 197
        ])
    );
    assert_eq!(
        legacy,
        AccountId32::new(blake2_256(
            &(b"modlpy/utilisuba", &signatories, 2u16).encode()
        ))
    );
    let safe = Multisig::safe_multi_account_id(&signatories, 2).unwrap();
    assert_eq!(
        safe,
        AccountId32::new([
            172, 65, 87, 122, 63, 188, 134, 21, 202, 232, 106, 72, 162, 86, 241, 135, 139, 225,
            234, 107, 76, 89, 48, 29, 190, 28, 238, 185, 3, 36, 96, 46
        ])
    );
    assert_ne!(safe, legacy);
    let expected = subtensor_hashed::safe::safe_derived_account(
        subtensor_hashed::safe::MULTISIG_DOMAIN,
        &(signatories.as_slice(), 2u16),
    )
    .unwrap();
    assert_eq!(safe, AccountId32::new(expected));
    assert!(subtensor_hashed::mldsa::rejects_legacy_public_keys(
        safe.as_ref()
    ));
    // Upstream's u64 mock must never silently truncate a supposedly safe account.
    assert_eq!(
        crate::Pallet::<crate::tests::Test>::safe_multi_account_id(&[1, 2, 3], 2),
        Err(crate::Error::<crate::tests::Test>::InvalidSafeAccountId.into())
    );
}

#[test]
fn every_legacy_call_encoding_remains_the_original_index_and_fields() {
    let others = vec![account(2), account(3)];
    let when = Timepoint {
        height: 7u32,
        index: 2,
    };
    let hash = [9; 32];
    let weight = Weight::from_parts(17, 23);
    let call = transfer(11);
    let pairs = [
        (
            Call::<Test>::as_multi_threshold_1 {
                other_signatories: others.clone(),
                call: call.clone(),
            }
            .encode(),
            (0u8, &others, &call).encode(),
        ),
        (
            Call::<Test>::as_multi {
                threshold: 2,
                other_signatories: others.clone(),
                maybe_timepoint: Some(when),
                call: call.clone(),
                max_weight: weight,
            }
            .encode(),
            (1u8, 2u16, &others, Some(when), &call, weight).encode(),
        ),
        (
            Call::<Test>::approve_as_multi {
                threshold: 2,
                other_signatories: others.clone(),
                maybe_timepoint: Some(when),
                call_hash: hash,
                max_weight: weight,
            }
            .encode(),
            (2u8, 2u16, &others, Some(when), hash, weight).encode(),
        ),
        (
            Call::<Test>::cancel_as_multi {
                threshold: 2,
                other_signatories: others.clone(),
                timepoint: when,
                call_hash: hash,
            }
            .encode(),
            (3u8, 2u16, &others, when, hash).encode(),
        ),
        (
            Call::<Test>::poke_deposit {
                threshold: 2,
                other_signatories: others.clone(),
                call_hash: hash,
            }
            .encode(),
            (4u8, 2u16, &others, hash).encode(),
        ),
    ];
    for (actual, expected) in pairs {
        assert_eq!(actual, expected);
    }
}

#[test]
fn safe_three_of_three_requires_full_threshold_and_refunds_only_after_execution() {
    new_test_ext().execute_with(|| {
        let safe = Multisig::safe_multi_account_id(&members(), 3).unwrap();
        assert_ok!(Balances::transfer_allow_death(
            RuntimeOrigin::signed(account(4)),
            safe.clone(),
            30
        ));
        let call = transfer(30);
        let hash = blake2_256(&call.encode());
        let when = Multisig::timepoint();
        let opened = Multisig::as_multi_safe(
            RuntimeOrigin::signed(account(1)),
            3,
            vec![account(2), account(3)],
            None,
            call.clone(),
            Weight::MAX,
        )
        .unwrap();
        assert_eq!(
            opened.actual_weight,
            Some(
                <() as WeightInfo>::as_multi_create(2, call.encoded_size() as u32)
                    .saturating_add(DerivationWeight::get())
            )
        );
        assert_eq!(Balances::reserved_balance(account(1)), 4);
        assert_ok!(Multisig::approve_as_multi_safe(
            RuntimeOrigin::signed(account(2)),
            3,
            vec![account(1), account(3)],
            Some(when),
            hash,
            Weight::zero()
        ));
        assert_eq!(Balances::free_balance(account(9)), 0);
        assert_eq!(
            Multisigs::<Test>::get(&safe, hash).unwrap().approvals.len(),
            2
        );
        assert_noop!(
            Multisig::as_multi_safe(
                RuntimeOrigin::signed(account(2)),
                3,
                vec![account(1), account(3)],
                Some(when),
                call.clone(),
                Weight::MAX
            ),
            Error::<Test>::AlreadyApproved
        );
        assert_ok!(Multisig::as_multi_safe(
            RuntimeOrigin::signed(account(3)),
            3,
            vec![account(1), account(2)],
            Some(when),
            call,
            Weight::MAX
        ));
        assert_eq!(Balances::free_balance(account(9)), 30);
        assert_eq!(Balances::reserved_balance(account(1)), 0);
        assert!(!Multisigs::<Test>::contains_key(safe, hash));
    });
}

#[test]
fn legacy_approvals_cancel_and_poke_cannot_touch_safe_operation() {
    new_test_ext().execute_with(|| {
        let safe = Multisig::safe_multi_account_id(&members(), 2).unwrap();
        let legacy = Multisig::multi_account_id(&members(), 2);
        let call = transfer(10);
        let hash = blake2_256(&call.encode());
        let when = Multisig::timepoint();
        assert_ok!(Multisig::approve_as_multi_safe(
            RuntimeOrigin::signed(account(1)),
            2,
            vec![account(2), account(3)],
            None,
            hash,
            Weight::zero()
        ));
        assert_noop!(
            Multisig::cancel_as_multi(
                RuntimeOrigin::signed(account(1)),
                2,
                vec![account(2), account(3)],
                when,
                hash
            ),
            Error::<Test>::NotFound
        );
        assert_noop!(
            Multisig::poke_deposit(
                RuntimeOrigin::signed(account(1)),
                2,
                vec![account(2), account(3)],
                hash
            ),
            Error::<Test>::NotFound
        );
        assert_ok!(Multisig::approve_as_multi(
            RuntimeOrigin::signed(account(2)),
            2,
            vec![account(1), account(3)],
            None,
            hash,
            Weight::zero()
        ));
        assert_eq!(
            Multisigs::<Test>::get(&safe, hash)
                .unwrap()
                .approvals
                .as_slice(),
            &[account(1)]
        );
        assert_eq!(
            Multisigs::<Test>::get(&legacy, hash)
                .unwrap()
                .approvals
                .as_slice(),
            &[account(2)]
        );
        assert_noop!(
            Multisig::cancel_as_multi_safe(
                RuntimeOrigin::signed(account(2)),
                2,
                vec![account(1), account(3)],
                when,
                hash
            ),
            Error::<Test>::NotOwner
        );
        assert_ok!(Multisig::cancel_as_multi_safe(
            RuntimeOrigin::signed(account(1)),
            2,
            vec![account(2), account(3)],
            when,
            hash
        ));
        assert_eq!(Balances::reserved_balance(account(1)), 0);
        assert!(Multisigs::<Test>::contains_key(legacy, hash));
    });
}

#[test]
fn safe_deposit_adjustments_and_cancellation_preserve_depositor_rules() {
    new_test_ext().execute_with(|| {
        let safe = Multisig::safe_multi_account_id(&members(), 2).unwrap();
        let hash = [1; 32];
        assert_ok!(Multisig::approve_as_multi_safe(
            RuntimeOrigin::signed(account(1)),
            2,
            vec![account(2), account(3)],
            None,
            hash,
            Weight::zero()
        ));
        DepositBase::set(5);
        assert_noop!(
            Multisig::poke_deposit_safe(
                RuntimeOrigin::signed(account(2)),
                2,
                vec![account(1), account(3)],
                hash
            ),
            Error::<Test>::NotOwner
        );
        assert_ok!(Multisig::poke_deposit_safe(
            RuntimeOrigin::signed(account(1)),
            2,
            vec![account(2), account(3)],
            hash
        ));
        assert_eq!(Balances::reserved_balance(account(1)), 7);
        assert_eq!(Multisigs::<Test>::get(&safe, hash).unwrap().deposit, 7);
        DepositBase::set(1);
        assert_ok!(Multisig::poke_deposit_safe(
            RuntimeOrigin::signed(account(1)),
            2,
            vec![account(2), account(3)],
            hash
        ));
        assert_eq!(Balances::reserved_balance(account(1)), 3);
        assert_ok!(Multisig::cancel_as_multi_safe(
            RuntimeOrigin::signed(account(1)),
            2,
            vec![account(2), account(3)],
            Multisig::timepoint(),
            hash
        ));
        assert_eq!(Balances::reserved_balance(account(1)), 0);
    });
}

#[test]
fn safe_threshold_one_uses_safe_origin_and_reports_inner_failure() {
    new_test_ext().execute_with(|| {
        let safe = Multisig::safe_multi_account_id(&members(), 1).unwrap();
        assert_ok!(Balances::transfer_allow_death(
            RuntimeOrigin::signed(account(4)),
            safe.clone(),
            20
        ));
        assert_ok!(Multisig::as_multi_threshold_1_safe(
            RuntimeOrigin::signed(account(1)),
            vec![account(2), account(3)],
            transfer(20)
        ));
        assert_eq!(Balances::free_balance(account(9)), 20);
        assert!(Multisig::as_multi_threshold_1_safe(
            RuntimeOrigin::signed(account(2)),
            vec![account(1), account(3)],
            transfer(1)
        )
        .is_err());
        assert_eq!(Balances::reserved_balance(account(1)), 0);
        assert_eq!(Multisigs::<Test>::iter().count(), 0);
    });
}

#[test]
fn safe_completion_checks_inner_weight_and_reports_failed_dispatch_before_refund() {
    new_test_ext().execute_with(|| {
        let safe = Multisig::safe_multi_account_id(&members(), 2).unwrap();
        let call = transfer(10);
        let hash = blake2_256(&call.encode());
        let when = Multisig::timepoint();
        assert_ok!(Multisig::approve_as_multi_safe(
            RuntimeOrigin::signed(account(1)),
            2,
            vec![account(2), account(3)],
            None,
            hash,
            Weight::zero(),
        ));
        assert_noop!(
            Multisig::as_multi_safe(
                RuntimeOrigin::signed(account(2)),
                2,
                vec![account(1), account(3)],
                Some(when),
                call.clone(),
                Weight::zero(),
            ),
            Error::<Test>::MaxWeightTooLow
        );
        assert_eq!(Balances::reserved_balance(account(1)), 3);
        assert_eq!(
            Multisigs::<Test>::get(&safe, hash)
                .unwrap()
                .approvals
                .as_slice(),
            &[account(1)]
        );
        assert_ok!(Multisig::as_multi_safe(
            RuntimeOrigin::signed(account(2)),
            2,
            vec![account(1), account(3)],
            Some(when),
            call,
            Weight::MAX,
        ));
        assert!(matches!(
            System::events().last().unwrap().event,
            RuntimeEvent::Multisig(Event::MultisigExecuted { result: Err(_), .. })
        ));
        assert_eq!(Balances::free_balance(account(9)), 0);
        assert_eq!(Balances::reserved_balance(account(1)), 0);
        assert!(!Multisigs::<Test>::contains_key(safe, hash));
    });
}

#[test]
fn malformed_manifests_and_wrong_timepoint_leave_state_unchanged() {
    new_test_ext().execute_with(|| {
        for (threshold, others, expected) in [
            (
                4,
                vec![account(2), account(3)],
                Error::<Test>::ThresholdTooHigh,
            ),
            (
                2,
                vec![account(3), account(2)],
                Error::<Test>::SignatoriesOutOfOrder,
            ),
            (
                2,
                vec![account(2), account(2)],
                Error::<Test>::SignatoriesOutOfOrder,
            ),
            (
                2,
                vec![account(1), account(2)],
                Error::<Test>::SenderInSignatories,
            ),
            (2, vec![], Error::<Test>::TooFewSignatories),
        ] {
            assert_noop!(
                Multisig::approve_as_multi_safe(
                    RuntimeOrigin::signed(account(1)),
                    threshold,
                    others,
                    None,
                    [0; 32],
                    Weight::zero()
                ),
                expected
            );
        }
        assert_eq!(Balances::reserved_balance(account(1)), 0);
        assert_eq!(Multisigs::<Test>::iter().count(), 0);
        assert_ok!(Multisig::approve_as_multi_safe(
            RuntimeOrigin::signed(account(1)),
            2,
            vec![account(2), account(3)],
            None,
            [0; 32],
            Weight::zero()
        ));
        assert_noop!(
            Multisig::approve_as_multi_safe(
                RuntimeOrigin::signed(account(2)),
                2,
                vec![account(1), account(3)],
                Some(Timepoint {
                    height: 2,
                    index: 0
                }),
                [0; 32],
                Weight::zero()
            ),
            Error::<Test>::WrongTimepoint
        );
    });
}

#[test]
fn maximum_derivation_weight_cannot_be_refunded_away() {
    new_test_ext().execute_with(|| {
        DerivationWeight::set(Weight::MAX);
        let call = Call::<Test>::as_multi_safe {
            threshold: 2,
            other_signatories: vec![account(2), account(3)],
            maybe_timepoint: None,
            call: transfer(1),
            max_weight: Weight::zero(),
        };
        assert_eq!(call.get_dispatch_info().call_weight, Weight::MAX);
        let result = Multisig::as_multi_safe(
            RuntimeOrigin::signed(account(1)),
            2,
            vec![account(2), account(3)],
            None,
            transfer(1),
            Weight::zero(),
        )
        .unwrap();
        assert_eq!(result.actual_weight, Some(Weight::MAX));
    });
}
