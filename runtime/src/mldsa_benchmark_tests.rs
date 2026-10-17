//! Benchmark fixture validity; these tests produce no performance measurements.
#![cfg(feature = "runtime-benchmarks")]
use super::*;
use frame_support::assert_ok;
use pallet_hashed_accounts::{DerivedAuthorities, DerivedDescriptor, OnRegister};
use subtensor_hashed::mldsa;

fn payer() -> AccountId {
    let payer = AccountId::new([93; 32]);
    let _ = Balances::make_free_balance_be(&payer, TaoBalance::new(1_000_000_000_000_000));
    payer
}

fn protected_parent() -> AccountId {
    <hashed_auth::HashedDerivedPolicy as pallet_utility::DerivedAccountPolicy<AccountId>>::setup_protected_benchmark(
        &AccountId::new([92; 32]),
    ).unwrap()
}

#[test]
fn mldsa_registration_benchmark_exercises_all_delegates_announcements_and_alias_sweep() {
    ext().execute_with(|| {
        System::set_block_number(1);
        let payer = payer();
        let descriptor = mldsa::Descriptor {
            version: 2,
            algorithm: 4,
            initial_commitment: mldsa::commitment(&mldsa::public_key_from_seed(&[91; 32]).unwrap()),
        };
        let account = AccountId::new(mldsa::account_id(&descriptor));
        assert_ok!(hashed_auth::OnHashedRegistered::setup_benchmark_mldsa(
            &account,
            &payer,
            &descriptor
        ));
        let delegates = Proxy::proxies(account.clone()).0;
        assert_eq!(delegates.len(), MaxProxies::get() as usize);
        for delegate in &delegates {
            assert_eq!(
                Proxy::announcements(delegate.delegate.clone()).0.len(),
                MaxPending::get() as usize
            );
            assert!(DerivedAuthorities::<Runtime>::contains_key(
                &delegate.delegate
            ));
        }
        let before = Balances::free_balance(&account);
        assert_ok!(HashedAccounts::register(
            RuntimeOrigin::signed(payer),
            descriptor.into()
        ));
        assert_eq!(
            Balances::free_balance(&account),
            before.saturating_add(TaoBalance::new(1_000_000_000))
        );
        assert!(HashedAccounts::is_protected(&account));
    });
}

#[test]
fn sponsored_derived_benchmark_decodes_near_limit_envelope_and_checks_maximum_graph() {
    ext().execute_with(|| {
        System::set_block_number(1);
        let payer = payer();
        let descriptor = DerivedDescriptor::Derivative {
            parent: protected_parent(),
            index: 42,
        };
        let account = descriptor.account_id();
        assert_ok!(hashed_auth::OnHashedRegistered::setup_benchmark_derived(
            &account,
            &account,
            &descriptor
        ));
        assert_ok!(hashed_auth::OnHashedRegistered::setup_benchmark_sponsored(
            &account,
            &payer,
            &descriptor
        ));
        let encoded = System::extrinsic_data(0);
        assert!((65_533..=65_536).contains(&encoded.len()));
        assert_ok!(HashedAccounts::register_derived_sponsored(
            RuntimeOrigin::signed(account.clone()),
            descriptor,
            payer
        ));
        let record = DerivedAuthorities::<Runtime>::get(account).unwrap();
        assert_eq!((record.depth, record.nodes), (16, 1024));
    });
}

#[test]
fn protected_child_benchmarks_start_from_valid_maximum_parent_graph() {
    for pure in [false, true] {
        ext().execute_with(|| {
            System::set_block_number(1);
            let marker = AccountId::new([94; 32]);
            let parent = if pure {
                <hashed_auth::HashedProxyPolicy as pallet_proxy::ProxyAccountPolicy<AccountId>>::setup_protected_benchmark(&marker).unwrap()
            } else {
                <hashed_auth::HashedDerivedPolicy as pallet_utility::DerivedAccountPolicy<AccountId>>::setup_protected_benchmark(&marker).unwrap()
            };
            let prior = DerivedAuthorities::<Runtime>::get(&parent).unwrap();
            assert_eq!((prior.depth, prior.nodes), (15, 1023));
            let child = if pure {
                let child = Proxy::pure_account(&parent, &ProxyType::Any, 0, Some((1, 0))).unwrap();
                assert_ok!(Proxy::create_protected_pure(RuntimeOrigin::signed(parent.clone()), ProxyType::Any, 0, 0));
                child
            } else {
                let child = Utility::derivative_account_id(parent.clone(), 0).unwrap();
                assert_ok!(Utility::as_protected_derivative(RuntimeOrigin::signed(parent.clone()), 0,
                    Box::new(RuntimeCall::System(SystemCall::remark { remark: Vec::new() }))));
                child
            };
            let record = DerivedAuthorities::<Runtime>::get(child).unwrap();
            assert_eq!((record.depth, record.nodes), (16, 1024));
            assert_eq!(record.pq_dependencies.as_slice(), &[parent]);
        });
    }
}

#[test]
fn mixed_order_benchmark_fixtures_execute_with_real_runtime_policies() {
    for netted in [false, true] {
        // Mixed, maximum mixed and all-PQ corners. Every order's fulfilled
        // status is asserted by the actual benchmark verification function.
        for (n, p) in [
            (8, 1),
            (8, 8),
            (LimitOrdersMaxOrdersPerBatch::get(), 1),
            (LimitOrdersMaxOrdersPerBatch::get(), 8),
        ] {
            let mut ext = ext();
            ext.register_extension(sp_keystore::KeystoreExt::new(
                sp_keystore::testing::MemoryKeystore::new(),
            ));
            ext.execute_with(|| {
                System::set_block_number(1);
                assert_ok!(
                    pallet_limit_orders::benchmarking::verify_mixed_order_fixture::<Runtime>(
                        n, p, netted
                    )
                );
            });
        }
    }
}
