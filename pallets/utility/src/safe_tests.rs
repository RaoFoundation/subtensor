#![allow(clippy::unwrap_used, clippy::indexing_slicing)]

use super::*;
use frame_support::{
    assert_ok, derive_impl, parameter_types,
    traits::{Currency, OriginTrait},
};
use sp_runtime::{AccountId32, BuildStorage, traits::IdentityLookup};

type Block = frame_system::mocking::MockBlock<Test>;
frame_support::construct_runtime!(
    pub enum Test {
        System: frame_system,
        Balances: pallet_balances,
        Utility: crate,
    }
);

#[derive_impl(frame_system::config_preludes::TestDefaultConfig)]
impl frame_system::Config for Test {
    type Block = Block;
    type AccountId = AccountId32;
    type Lookup = IdentityLookup<Self::AccountId>;
    type AccountData = pallet_balances::AccountData<u64>;
}

#[derive_impl(pallet_balances::config_preludes::TestDefaultConfig)]
impl pallet_balances::Config for Test {
    type AccountStore = System;
}

// A protected parent must also be able to opt into the separate safe domain.
pub struct ProtectedParent;
impl DerivedAccountPolicy<AccountId32> for ProtectedParent {
    fn requires_protection(_: &AccountId32) -> bool {
        true
    }
}

parameter_types! {
    pub SafeDerivationWeight: Weight = Weight::MAX;
}
impl Config for Test {
    type RuntimeCall = RuntimeCall;
    type PalletsOrigin = OriginCaller;
    type WeightInfo = ();
    type DerivedAccountPolicy = ProtectedParent;
    type SafeDerivationWeight = SafeDerivationWeight;
}

fn ext() -> sp_io::TestExternalities {
    let storage = frame_system::GenesisConfig::<Test>::default()
        .build_storage()
        .unwrap();
    let mut ext = sp_io::TestExternalities::new(storage);
    ext.execute_with(|| System::set_block_number(1));
    ext
}

fn transfer(dest: AccountId32, value: u64) -> Box<RuntimeCall> {
    Box::new(RuntimeCall::Balances(
        pallet_balances::Call::transfer_allow_death { dest, value },
    ))
}

#[test]
fn safe_derivative_preserves_legacy_identity_and_call_encoding() {
    let parent = AccountId32::new([1; 32]);
    let legacy = Utility::derivative_account_id(parent.clone(), 7).unwrap();
    assert_eq!(
        legacy,
        AccountId32::new([
            151, 177, 114, 184, 127, 150, 188, 31, 110, 85, 115, 146, 195, 187, 125, 187, 202, 192,
            125, 66, 101, 52, 26, 214, 24, 42, 17, 61, 203, 157, 124, 61,
        ])
    );
    let child = Utility::safe_derivative_account_id(parent.clone(), 7).unwrap();
    assert_ne!(child, legacy);
    assert!(subtensor_hashed::mldsa::rejects_legacy_public_keys(
        child.as_ref()
    ));
    assert_ne!(
        child,
        Utility::safe_derivative_account_id(parent, 8).unwrap()
    );
    let call = Box::new(RuntimeCall::System(frame_system::Call::remark {
        remark: vec![],
    }));
    let encoded = Call::<Test>::as_derivative {
        index: 7,
        call: call.clone(),
    }
    .encode();
    assert_eq!(encoded, [vec![1, 7, 0], call.encode()].concat());
    assert_eq!(
        Call::<Test>::as_protected_derivative {
            index: 7,
            call: call.clone()
        }
        .encode()[0],
        8
    );
    assert_eq!(
        Call::<Test>::as_safe_derivative { index: 7, call }.encode()[0],
        9
    );
}

#[test]
fn safe_derivative_executes_nested_without_registration_or_losing_filters() {
    ext().execute_with(|| {
        let parent = AccountId32::new([1; 32]);
        let child = Utility::safe_derivative_account_id(parent.clone(), 7).unwrap();
        let grandchild = Utility::safe_derivative_account_id(child.clone(), 9).unwrap();
        let recipient = AccountId32::new([3; 32]);
        let _ = Balances::make_free_balance_be(&grandchild, 10);
        let nested = Box::new(RuntimeCall::Utility(Call::as_safe_derivative {
            index: 9,
            call: transfer(recipient.clone(), 3),
        }));
        let mut filtered = RuntimeOrigin::signed(parent.clone());
        filtered.add_filter(|call| !matches!(call, RuntimeCall::Balances(_)));
        let before = System::events();
        let rejected = Utility::as_safe_derivative(filtered, 7, nested.clone()).unwrap_err();
        assert_eq!(
            rejected.error,
            frame_system::Error::<Test>::CallFiltered.into()
        );
        assert_eq!(rejected.post_info.actual_weight, Some(Weight::MAX));
        assert_eq!(Balances::free_balance(&grandchild), 10);
        assert_eq!(Balances::free_balance(&recipient), 0);
        assert_eq!(System::events(), before);
        assert_ok!(Utility::as_safe_derivative(
            RuntimeOrigin::signed(parent.clone()),
            7,
            nested
        ));
        assert_eq!(Balances::free_balance(&grandchild), 7);
        assert_eq!(Balances::free_balance(&recipient), 3);
        assert_eq!(Balances::reserved_balance(parent), 0);
        assert_eq!(Balances::reserved_balance(child), 0);
    });
}
