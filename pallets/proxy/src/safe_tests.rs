#![allow(clippy::unwrap_used, clippy::indexing_slicing)]

use super::*;
use frame::deps::{
    sp_io,
    sp_runtime::{AccountId32, traits::IdentityLookup},
};
use frame::testing_prelude::*;
use pallet_subtensor_utility as utility;

type Block = frame_system::mocking::MockBlock<Test>;
construct_runtime!(
    pub enum Test {
        System: frame_system,
        Balances: pallet_balances,
        Proxy: crate,
        Utility: utility,
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
impl utility::Config for Test {
    type RuntimeCall = RuntimeCall;
    type PalletsOrigin = OriginCaller;
    type WeightInfo = ();
    type DerivedAccountPolicy = ();
    type SafeDerivationWeight = ();
}

#[derive(
    Clone,
    Copy,
    Default,
    PartialEq,
    Eq,
    PartialOrd,
    Ord,
    Encode,
    Decode,
    DecodeWithMemTracking,
    Debug,
    MaxEncodedLen,
    TypeInfo,
)]
pub enum ProxyType {
    #[default]
    Any,
    UtilityOrProxy,
}
impl InstanceFilter<RuntimeCall> for ProxyType {
    fn filter(&self, call: &RuntimeCall) -> bool {
        *self == Self::Any || matches!(call, RuntimeCall::Utility(_) | RuntimeCall::Proxy(_))
    }
    fn is_superset(&self, other: &Self) -> bool {
        *self == Self::Any || self == other
    }
}

// Safe creation is explicit and is available even to protected parents.
pub struct ProtectedParent;
impl Contains<(AccountId32, AccountId32)> for ProtectedParent {
    fn contains(_: &(AccountId32, AccountId32)) -> bool {
        true
    }
}
impl ProxyAccountPolicy<AccountId32> for ProtectedParent {
    fn requires_protection(_: &AccountId32) -> bool {
        true
    }
}
impl Config for Test {
    type RuntimeCall = RuntimeCall;
    type Currency = Balances;
    type AccountPolicy = ProtectedParent;
    type ProxyType = ProxyType;
    type ProxyDepositBase = ConstU64<1>;
    type ProxyDepositFactor = ConstU64<1>;
    type MaxProxies = ConstU32<4>;
    type WeightInfo = ();
    type SafeDerivationWeight = ();
    type MaxPending = ConstU32<4>;
    type CallHasher = BlakeTwo256;
    type AnnouncementDepositBase = ConstU64<1>;
    type AnnouncementDepositFactor = ConstU64<1>;
    type BlockNumberProvider = System;
}

fn account(n: u8) -> AccountId32 {
    AccountId32::new([n; 32])
}
fn ext() -> sp_io::TestExternalities {
    let mut storage = frame_system::GenesisConfig::<Test>::default()
        .build_storage()
        .unwrap();
    pallet_balances::GenesisConfig::<Test> {
        balances: vec![(account(1), 100), (account(2), 100), (account(3), 100)],
        ..Default::default()
    }
    .assimilate_storage(&mut storage)
    .unwrap();
    let mut ext = sp_io::TestExternalities::new(storage);
    ext.execute_with(|| System::set_block_number(1));
    ext
}
fn kill(spawner: AccountId32, height: u64, ext_index: u32) -> Box<RuntimeCall> {
    Box::new(RuntimeCall::Proxy(Call::kill_safe_pure {
        spawner,
        proxy_type: ProxyType::Any,
        index: 7,
        height,
        ext_index,
    }))
}

#[test]
fn safe_pure_preserves_legacy_derivation_and_call_bytes() {
    ext().execute_with(|| {
        let parent = account(1);
        let legacy = Proxy::pure_account(&parent, &ProxyType::Any, 7, None).unwrap();
        assert_eq!(
            legacy,
            AccountId32::new([
                127, 213, 69, 74, 56, 92, 228, 66, 210, 7, 92, 2, 22, 112, 104, 103, 167, 55, 154,
                146, 183, 240, 241, 21, 202, 118, 227, 147, 47, 143, 120, 171,
            ])
        );
        let safe = Proxy::safe_pure_account(&parent, &ProxyType::Any, 7, None).unwrap();
        assert_ne!(safe, legacy);
        assert!(subtensor_hashed::mldsa::rejects_legacy_public_keys(
            safe.as_ref()
        ));
        assert_ne!(
            safe,
            Proxy::safe_pure_account(&parent, &ProxyType::Any, 7, Some((1, 1))).unwrap()
        );
        assert_ne!(
            safe,
            Proxy::safe_pure_account(&parent, &ProxyType::Any, 7, Some((2, 0))).unwrap()
        );
        let create = Call::<Test>::create_pure {
            proxy_type: ProxyType::Any,
            delay: 0,
            index: 7,
        };
        assert_eq!(
            create.encode(),
            [vec![4], (ProxyType::Any, 0u64, 7u16).encode()].concat()
        );
        let old_kill = Call::<Test>::kill_pure {
            spawner: parent.clone(),
            proxy_type: ProxyType::Any,
            index: 7,
            height: 1,
            ext_index: 0,
        };
        assert_eq!(
            old_kill.encode(),
            [
                vec![5],
                (
                    parent,
                    ProxyType::Any,
                    7u16,
                    codec::Compact(1u64),
                    codec::Compact(0u32)
                )
                    .encode()
            ]
            .concat()
        );
        assert_eq!(
            Call::<Test>::create_protected_pure {
                proxy_type: ProxyType::Any,
                delay: 0,
                index: 7
            }
            .encode()[0],
            12
        );
        assert_eq!(
            Call::<Test>::create_safe_pure {
                proxy_type: ProxyType::Any,
                delay: 0,
                index: 7
            }
            .encode()[0],
            13
        );
        assert_eq!(
            Call::<Test>::kill_safe_pure {
                spawner: account(1),
                proxy_type: ProxyType::Any,
                index: 7,
                height: 1,
                ext_index: 0
            }
            .encode()[0],
            14
        );
    });
}

#[test]
fn safe_pure_lifecycle_refunds_only_proxy_deposit_and_does_not_recreate_old_address() {
    ext().execute_with(|| {
        System::set_extrinsic_index(3);
        let parent = account(1);
        let pure = Proxy::safe_pure_account(&parent, &ProxyType::Any, 7, None).unwrap();
        assert_ok!(Proxy::create_safe_pure(
            RuntimeOrigin::signed(parent.clone()),
            ProxyType::Any,
            0,
            7
        ));
        assert_eq!(Balances::reserved_balance(&parent), 2);
        assert_eq!(Proxies::<Test>::get(&pure).1, 2);
        assert_noop!(
            Proxy::create_safe_pure(RuntimeOrigin::signed(parent.clone()), ProxyType::Any, 0, 7),
            Error::<Test>::Duplicate
        );
        assert_noop!(
            Proxy::kill_safe_pure(
                RuntimeOrigin::signed(pure.clone()),
                parent.clone(),
                ProxyType::Any,
                7,
                1,
                4
            ),
            Error::<Test>::NoPermission
        );
        assert_noop!(
            Proxy::kill_pure(
                RuntimeOrigin::signed(pure.clone()),
                parent.clone(),
                ProxyType::Any,
                7,
                1,
                3
            ),
            Error::<Test>::NoPermission
        );
        let _ = Balances::make_free_balance_be(&pure, 10);
        assert_ok!(Proxy::proxy(
            RuntimeOrigin::signed(parent.clone()),
            pure.clone(),
            None,
            kill(parent.clone(), 1, 3)
        ));
        assert!(!Proxies::<Test>::contains_key(&pure));
        assert_eq!(Balances::reserved_balance(&parent), 0);
        assert_eq!(Balances::free_balance(&parent), 100);
        assert_eq!(Balances::free_balance(&pure), 10);
        System::set_block_number(2);
        System::set_extrinsic_index(0);
        let next = Proxy::safe_pure_account(&parent, &ProxyType::Any, 7, None).unwrap();
        assert_ne!(next, pure);
        assert_ok!(Proxy::create_safe_pure(
            RuntimeOrigin::signed(parent.clone()),
            ProxyType::Any,
            0,
            7
        ));
        assert_noop!(
            Proxy::proxy(
                RuntimeOrigin::signed(parent),
                pure,
                None,
                Box::new(RuntimeCall::System(frame_system::Call::remark {
                    remark: vec![]
                }))
            ),
            Error::<Test>::NotProxy
        );
        assert!(Proxies::<Test>::contains_key(next));
    });
}

#[test]
fn safe_children_and_nested_proxies_keep_restricted_origin_filters() {
    ext().execute_with(|| {
        let parent = account(1);
        let delegate = account(2);
        let recipient = account(4);
        assert_ok!(Proxy::create_safe_pure(
            RuntimeOrigin::signed(parent.clone()),
            ProxyType::Any,
            0,
            7
        ));
        let pure = Proxy::safe_pure_account(&parent, &ProxyType::Any, 7, None).unwrap();
        let _ = Balances::make_free_balance_be(&pure, 100);
        assert_ok!(Proxy::add_proxy(
            RuntimeOrigin::signed(pure.clone()),
            delegate.clone(),
            ProxyType::UtilityOrProxy,
            0
        ));
        let child = Utility::safe_derivative_account_id(pure.clone(), 5).unwrap();
        let _ = Balances::make_free_balance_be(&child, 10);
        let transfer = Box::new(RuntimeCall::Balances(
            pallet_balances::Call::transfer_allow_death {
                dest: recipient.clone(),
                value: 3,
            },
        ));
        let derivative = Box::new(RuntimeCall::Utility(utility::Call::as_safe_derivative {
            index: 5,
            call: transfer.clone(),
        }));
        assert_ok!(Proxy::proxy(
            RuntimeOrigin::signed(delegate.clone()),
            pure.clone(),
            None,
            derivative
        ));
        assert_eq!(
            LastCallResult::<Test>::get(&pure),
            Some(Err(frame_system::Error::<Test>::CallFiltered.into()))
        );
        assert_eq!(Balances::free_balance(&child), 10);
        assert_eq!(Balances::free_balance(&recipient), 0);

        // The outer restricted filter also survives a nested proxy dispatch.
        assert_ok!(Proxy::create_safe_pure(
            RuntimeOrigin::signed(pure.clone()),
            ProxyType::Any,
            0,
            8
        ));
        let nested = Proxy::safe_pure_account(&pure, &ProxyType::Any, 8, None).unwrap();
        let _ = Balances::make_free_balance_be(&nested, 10);
        let nested_call = Box::new(RuntimeCall::Proxy(Call::proxy {
            real: nested.clone(),
            force_proxy_type: None,
            call: transfer.clone(),
        }));
        assert_ok!(Proxy::proxy(
            RuntimeOrigin::signed(delegate),
            pure.clone(),
            None,
            nested_call
        ));
        assert_eq!(
            LastCallResult::<Test>::get(&nested),
            Some(Err(frame_system::Error::<Test>::CallFiltered.into()))
        );
        assert_eq!(Balances::free_balance(&nested), 10);
        assert_ok!(Proxy::proxy(
            RuntimeOrigin::signed(parent),
            pure.clone(),
            None,
            Box::new(RuntimeCall::Proxy(Call::proxy {
                real: nested,
                force_proxy_type: None,
                call: transfer
            }))
        ));
        assert_eq!(Balances::free_balance(recipient), 3);
    });
}

#[test]
fn restricted_direct_and_announced_proxies_cannot_kill_safe_pure() {
    ext().execute_with(|| {
        let parent = account(1);
        let delegate = account(2);
        assert_ok!(Proxy::create_safe_pure(
            RuntimeOrigin::signed(parent.clone()),
            ProxyType::Any,
            0,
            7
        ));
        let pure = Proxy::safe_pure_account(&parent, &ProxyType::Any, 7, None).unwrap();
        let _ = Balances::make_free_balance_be(&pure, 100);
        assert_ok!(Proxy::add_proxy(
            RuntimeOrigin::signed(pure.clone()),
            delegate.clone(),
            ProxyType::UtilityOrProxy,
            0
        ));
        let call = kill(parent.clone(), 1, 0);
        assert_ok!(Proxy::proxy(
            RuntimeOrigin::signed(delegate.clone()),
            pure.clone(),
            None,
            call.clone()
        ));
        assert_eq!(
            LastCallResult::<Test>::get(&pure),
            Some(Err(frame_system::Error::<Test>::CallFiltered.into()))
        );
        assert!(Proxies::<Test>::contains_key(&pure));
        assert_eq!(Balances::reserved_balance(&parent), 2);
        assert_ok!(Proxy::remove_proxy(
            RuntimeOrigin::signed(pure.clone()),
            delegate.clone(),
            ProxyType::UtilityOrProxy,
            0
        ));
        assert_ok!(Proxy::add_proxy(
            RuntimeOrigin::signed(pure.clone()),
            delegate.clone(),
            ProxyType::UtilityOrProxy,
            2
        ));
        assert_ok!(Proxy::announce(
            RuntimeOrigin::signed(delegate.clone()),
            pure.clone(),
            BlakeTwo256::hash_of(&call)
        ));
        System::set_block_number(3);
        assert_ok!(Proxy::proxy_announced(
            RuntimeOrigin::signed(account(3)),
            delegate,
            pure.clone(),
            None,
            call
        ));
        assert_eq!(
            LastCallResult::<Test>::get(&pure),
            Some(Err(frame_system::Error::<Test>::CallFiltered.into()))
        );
        assert!(Proxies::<Test>::contains_key(pure));
        assert_eq!(Balances::reserved_balance(parent), 2);
    });
}
