//! Staking-proxy add/remove via chain-extension function ids.

use super::*;

#[test]
fn add_proxy_success_creates_proxy_relationship() {
    mock::new_test_ext(1).execute_with(|| {
        let delegator = U256::from(6001);
        let delegate = U256::from(6002);

        add_balance_to_coldkey_account(&delegator, 1_000_000_000.into());

        assert_eq!(
            pallet_subtensor_proxy::Proxies::<mock::Test>::get(delegator)
                .0
                .len(),
            0
        );

        let mut env = MockEnv::new(FunctionId::AddProxyV1, delegator, delegate.encode());

        let ret = SubtensorChainExtension::<mock::Test>::dispatch(&mut env).unwrap();
        assert_extension_success(ret);

        let proxies = pallet_subtensor_proxy::Proxies::<mock::Test>::get(delegator).0;
        assert_eq!(proxies.len(), 1);
        if let Some(proxy) = proxies.first() {
            assert_eq!(proxy.delegate, delegate);
            assert_eq!(
                proxy.proxy_type,
                subtensor_runtime_common::ProxyType::Staking
            );
            assert_eq!(proxy.delay, 0u64);
        } else {
            panic!("proxies should contain one element");
        }
    });
}

#[test]
fn remove_proxy_success_removes_proxy_relationship() {
    mock::new_test_ext(1).execute_with(|| {
        let delegator = U256::from(7001);
        let delegate = U256::from(7002);

        add_balance_to_coldkey_account(&delegator, 1_000_000_000.into());

        let mut add_env = MockEnv::new(FunctionId::AddProxyV1, delegator, delegate.encode());
        let ret = SubtensorChainExtension::<mock::Test>::dispatch(&mut add_env).unwrap();
        assert_extension_success(ret);

        let proxies_before = pallet_subtensor_proxy::Proxies::<mock::Test>::get(delegator).0;
        assert_eq!(proxies_before.len(), 1);

        let mut remove_env = MockEnv::new(FunctionId::RemoveProxyV1, delegator, delegate.encode());
        let ret = SubtensorChainExtension::<mock::Test>::dispatch(&mut remove_env).unwrap();
        assert_extension_success(ret);

        let proxies_after = pallet_subtensor_proxy::Proxies::<mock::Test>::get(delegator).0;
        assert_eq!(proxies_after.len(), 0);
    });
}
