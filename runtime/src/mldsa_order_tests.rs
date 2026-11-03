//! Real balance, subnet reserve and alpha-share rollback for account-approved orders.
use super::*;
use frame_support::{assert_noop, assert_ok, traits::Get};
use pallet_limit_orders::{AuthorizedOrder, Order, OrderAuthorization, OrderType, VersionedOrder};
use sp_runtime::{Perbill, traits::AccountIdConversion};
use subtensor_runtime_common::AlphaBalance;

#[test]
fn batch_price_failure_reverts_real_assets_approvals_and_nonces() {
    // Two buyers split an odd alpha amount. Their one-unit rounding loss must
    // not exceed the zero-slippage ceiling, even with a perfectly offset batch.
    mldsa::with_profiles(|| {
        let netuid = NetUid::from(42u16);
        SubtensorModule::init_new_network(netuid, 100);
        pallet_subtensor::SubnetMechanism::<Runtime>::insert(netuid, 1);
        pallet_subtensor::SubtokenEnabled::<Runtime>::insert(netuid, true);
        pallet_subtensor::SubnetTAO::<Runtime>::insert(netuid, TaoBalance::new(2_000_000_000_000));
        pallet_subtensor::SubnetAlphaIn::<Runtime>::insert(
            netuid,
            AlphaBalance::new(1_000_000_000_000),
        );
        pallet_subtensor_swap::SwapBalancer::<Runtime>::remove(netuid);
        assert_eq!(
            Swap::current_price(netuid),
            substrate_fixed::types::U64F64::from_num(2)
        );
        let accounts = [91, 92, 93].map(|seed| AccountId::from(pair(seed).public()));
        let intermediary: AccountId = LimitOrdersPalletId::get().into_account_truncating();
        for account in &accounts {
            assert_ok!(SubtensorModule::create_account_if_non_existent(
                account, account
            ));
            let _ = Balances::make_free_balance_be(account, TaoBalance::new(10_000_000_000));
        }
        assert_ok!(SubtensorModule::create_account_if_non_existent(
            &intermediary,
            &LimitOrdersPalletHotkey::get()
        ));
        let _ = Balances::make_free_balance_be(&intermediary, TaoBalance::new(1_000_000_000));
        SubtensorModule::increase_stake_for_hotkey_and_coldkey_on_subnet(
            &accounts[2],
            &accounts[2],
            netuid,
            AlphaBalance::new(3_000_000_001),
        );
        pallet_limit_orders::LimitOrdersEnabled::<Runtime>::put(true);
        for slippage in [0, 1] {
            let nonce = 7u64.saturating_add(slippage as u64);
            let mut batch = Vec::new();
            for (index, owner) in accounts.iter().enumerate() {
                let order = VersionedOrder::V1(Order {
                    signer: owner.clone(),
                    hotkey: owner.clone(),
                    netuid,
                    order_type: if index == 2 {
                        OrderType::StopLoss
                    } else {
                        OrderType::LimitBuy
                    },
                    amount: 3_000_000_001,
                    limit_price: 2_000_000_000,
                    expiry: u64::MAX,
                    fee_rate: Perbill::zero(),
                    fee_recipient: intermediary.clone(),
                    relayer: None,
                    max_slippage: Some(Perbill::from_percent(slippage)),
                    chain_id: ConfigurableChainId::get(),
                    partial_fills_enabled: false,
                });
                assert_ok!(LimitOrders::authorize_order(
                    RuntimeOrigin::signed(owner.clone()),
                    order.clone().with_nonce(nonce).unwrap()
                ));
                batch.push(AuthorizedOrder {
                    order,
                    nonce,
                    partial_fill: None,
                    authorization: OrderAuthorization::AccountApproved,
                });
            }
            let batch = unified_orders(batch.try_into().unwrap());
            if slippage == 0 {
                // The failure occurs during payout, after real TAO and alpha
                // collection. assert_noop compares the complete storage root.
                assert_noop!(
                    LimitOrders::execute_batched_orders(
                        RuntimeOrigin::signed(accounts[0].clone()),
                        netuid,
                        batch
                    ),
                    pallet_limit_orders::Error::<Runtime>::PriceConditionNotMet
                );
                for owner in &accounts {
                    assert!(LimitOrders::approved_order(owner, nonce).is_some());
                    assert_eq!(
                        Balances::free_balance(owner),
                        TaoBalance::new(10_000_000_000)
                    );
                }
                assert_eq!(
                    SubtensorModule::get_stake_for_hotkey_and_coldkey_on_subnet(
                        &accounts[2],
                        &accounts[2],
                        netuid
                    ),
                    AlphaBalance::new(3_000_000_001)
                );
            } else {
                // With new explicitly approved rounding tolerance, the same
                // accounts can settle successfully.
                assert_ok!(LimitOrders::execute_batched_orders(
                    RuntimeOrigin::signed(accounts[0].clone()),
                    netuid,
                    batch
                ));
            }
        }
    });
}

fn orders(
    owner: &AccountId,
    recipient: &AccountId,
    netuid: NetUid,
) -> frame_support::BoundedVec<AuthorizedOrder, pallet_limit_orders::MaxAuthorizedOrders> {
    (0..2)
        .map(|nonce| {
            let order = VersionedOrder::V1(Order {
                signer: owner.clone(),
                hotkey: owner.clone(),
                netuid,
                order_type: OrderType::LimitBuy,
                amount: 1_000_000_000,
                limit_price: u64::MAX,
                expiry: u64::MAX,
                fee_rate: Perbill::from_percent(1),
                fee_recipient: recipient.clone(),
                relayer: None,
                max_slippage: None,
                chain_id: ConfigurableChainId::get(),
                partial_fills_enabled: false,
            });
            AuthorizedOrder {
                nonce,
                order,
                partial_fill: None,
                authorization: OrderAuthorization::AccountApproved,
            }
        })
        .collect::<Vec<_>>()
        .try_into()
        .unwrap()
}

#[test]
fn all_post_quantum_profiles_approve_and_cancel_orders_without_additional_deposits() {
    for profile in 4..=9 {
        mldsa::with_profiles(|| {
            System::set_block_number(1);
            hashed_auth::TestVerificationWeight::set(Weight::from_parts(10_000_000, 0));
            let (owner, sign): (AccountId, Box<dyn Fn(RuntimeCall) -> UncheckedExtrinsic>) =
                match profile {
                    4 => (mldsa::register(81), Box::new(|call| mldsa::sign(81, call))),
                    5 => {
                        let wallet = mldsa::stateless::Wallet::new(81);
                        let _ = Balances::make_free_balance_be(
                            &wallet.account,
                            TaoBalance::new(1_000_000_000_000),
                        );
                        (
                            wallet.account.clone(),
                            Box::new(move |call| wallet.sign(call)),
                        )
                    }
                    _ => {
                        let wallet = falcon::Wallet::new(81, profile);
                        wallet.prepare();
                        (
                            wallet.account.clone(),
                            Box::new(move |call| wallet.sign(call)),
                        )
                    }
                };
            pallet_limit_orders::LimitOrdersEnabled::<Runtime>::put(true);
            let signed = orders(&owner, &owner, NetUid::from(42u16))[0].clone();
            let order = signed.order.with_nonce(signed.nonce).unwrap();
            let reserved = Balances::reserved_balance(&owner);
            let initial_nonce = System::account_nonce(&owner);
            assert_ok!(
                Executive::apply_extrinsic(sign(RuntimeCall::LimitOrders(
                    pallet_limit_orders::Call::authorize_order {
                        order: order.clone()
                    },
                )))
                .unwrap()
            );
            assert!(pallet_limit_orders::ApprovedOrders::<Runtime>::contains_key(&owner));
            assert_ok!(
                Executive::apply_extrinsic(sign(RuntimeCall::LimitOrders(
                    pallet_limit_orders::Call::cancel_order { order },
                )))
                .unwrap()
            );
            assert_eq!(
                LimitOrders::approved_order(&owner, signed.nonce)
                    .unwrap()
                    .status,
                Some(pallet_limit_orders::OrderStatus::Cancelled)
            );
            assert_eq!(Balances::reserved_balance(&owner), reserved);
            assert_eq!(System::account_nonce(&owner), initial_nonce + 2);
            let registered = pallet_hashed_accounts::MlDsaAccounts::<Runtime>::get(&owner);
            if [4, 6, 8].contains(&profile) {
                assert_eq!(registered.unwrap().generation, 2);
            } else {
                assert!(registered.is_none());
            }
        });
    }
}

#[test]
fn restricted_proxy_cannot_create_deferred_order_spending_authority() {
    use alloc::boxed::Box;
    use frame_support::traits::InstanceFilter;

    for protected in [false, true] {
        for proxy_type in [
            ProxyType::NonTransfer,
            ProxyType::NonFungible,
            ProxyType::NonCritical,
        ] {
            mldsa::with_profiles(|| {
                System::set_block_number(1);
                let accounts = [85, 86, 87].map(|identity| {
                    if protected {
                        mldsa::register(identity)
                    } else {
                        let account = AccountId::from(pair(identity).public());
                        let _ = Balances::make_free_balance_be(
                            &account,
                            TaoBalance::new(1_000_000_000_000),
                        );
                        account
                    }
                });
                let [owner, delegate, bridge] = accounts;
                assert_ok!(Proxy::add_proxy_delegate(
                    &owner,
                    delegate.clone(),
                    proxy_type,
                    0
                ));
                assert_ok!(Proxy::add_proxy_delegate(
                    &bridge,
                    delegate.clone(),
                    proxy_type,
                    0
                ));
                assert_ok!(Proxy::add_proxy_delegate(
                    &owner,
                    bridge.clone(),
                    ProxyType::Any,
                    0
                ));
                let netuid = NetUid::from(42u16);
                SubtensorModule::init_new_network(netuid, 100);
                pallet_subtensor::SubnetMechanism::<Runtime>::insert(netuid, 0);
                pallet_subtensor::SubtokenEnabled::<Runtime>::insert(netuid, true);
                pallet_subtensor::SubnetTAO::<Runtime>::insert(
                    netuid,
                    TaoBalance::new(1_000_000_000_000),
                );
                pallet_subtensor::SubnetAlphaIn::<Runtime>::insert(
                    netuid,
                    AlphaBalance::new(1_000_000_000_000),
                );
                assert_ok!(SubtensorModule::create_account_if_non_existent(
                    &owner, &owner
                ));
                pallet_limit_orders::LimitOrdersEnabled::<Runtime>::put(true);
                let signed = orders(&owner, &delegate, netuid)[0].clone();
                let approval =
                    RuntimeCall::LimitOrders(pallet_limit_orders::Call::authorize_order {
                        order: signed.order.clone().with_nonce(signed.nonce).unwrap(),
                    });
                // Every restricted grant denies creation, even those that may
                // relay an order already authorized by its owner.
                for grant in (0..=u8::MAX).filter_map(|i| ProxyType::try_from(i).ok()) {
                    assert_eq!(grant.filter(&approval), grant == ProxyType::Any);
                }
                let balances = [&owner, &delegate].map(Balances::free_balance);
                let reserve = pallet_subtensor::SubnetTAO::<Runtime>::get(netuid);
                let alpha = pallet_subtensor::SubnetAlphaIn::<Runtime>::get(netuid);
                let calls = [
                    (owner.clone(), approval.clone()),
                    (
                        owner.clone(),
                        RuntimeCall::Utility(pallet_utility::Call::batch_all {
                            calls: vec![approval.clone()],
                        }),
                    ),
                    (
                        bridge.clone(),
                        RuntimeCall::Proxy(pallet_proxy::Call::proxy {
                            real: owner.clone().into(),
                            force_proxy_type: Some(ProxyType::Any),
                            call: Box::new(approval.clone()),
                        }),
                    ),
                ];
                for (real, call) in calls {
                    assert_ok!(Proxy::proxy(
                        RuntimeOrigin::signed(delegate.clone()),
                        real.into(),
                        Some(proxy_type),
                        Box::new(call)
                    ));
                    assert_eq!(
                        pallet_proxy::LastCallResult::<Runtime>::get(&owner),
                        Some(Err(frame_system::Error::<Runtime>::CallFiltered.into()))
                    );
                    assert!(!pallet_limit_orders::ApprovedOrders::<Runtime>::contains_key(&owner));
                    assert!(LimitOrders::approved_order(&owner, signed.nonce).is_none());
                    assert_noop!(
                        LimitOrders::execute_orders(
                            RuntimeOrigin::signed(delegate.clone()),
                            unified_orders(vec![signed.clone()].try_into().unwrap()),
                            true
                        ),
                        pallet_limit_orders::Error::<Runtime>::OrderNotApproved
                    );
                    assert_eq!([&owner, &delegate].map(Balances::free_balance), balances);
                    assert_eq!(pallet_subtensor::SubnetTAO::<Runtime>::get(netuid), reserve);
                    assert_eq!(
                        pallet_subtensor::SubnetAlphaIn::<Runtime>::get(netuid),
                        alpha
                    );
                    assert_eq!(
                        SubtensorModule::get_stake_for_hotkey_and_coldkey_on_subnet(
                            &owner, &owner, netuid
                        ),
                        AlphaBalance::ZERO
                    );
                }
                // Full authority still approves the identical terms, and the
                // same relayer/order fixture reaches real economic execution.
                assert_ok!(Proxy::proxy(
                    RuntimeOrigin::signed(bridge),
                    owner.clone().into(),
                    Some(ProxyType::Any),
                    Box::new(approval)
                ));
                assert!(pallet_limit_orders::ApprovedOrders::<Runtime>::contains_key(&owner));
                assert_ok!(LimitOrders::execute_orders(
                    RuntimeOrigin::signed(delegate.clone()),
                    unified_orders(vec![signed].try_into().unwrap()),
                    true
                ));
                assert_eq!(
                    Balances::free_balance(&owner),
                    balances[0].saturating_sub(TaoBalance::new(1_000_000_000))
                );
                assert_eq!(
                    Balances::free_balance(&delegate),
                    balances[1].saturating_add(TaoBalance::new(10_000_000))
                );
                assert!(
                    SubtensorModule::get_stake_for_hotkey_and_coldkey_on_subnet(
                        &owner, &owner, netuid
                    ) > AlphaBalance::ZERO
                );
            });
        }
    }
}

fn unified_orders(
    orders: frame_support::BoundedVec<AuthorizedOrder, pallet_limit_orders::MaxAuthorizedOrders>,
) -> frame_support::BoundedVec<
    pallet_limit_orders::SignedOrder<AccountId>,
    <Runtime as pallet_limit_orders::Config>::MaxOrdersPerBatch,
> {
    orders
        .into_iter()
        .map(|signed| signed.try_into().unwrap())
        .collect::<Vec<_>>()
        .try_into()
        .unwrap()
}

#[test]
fn balance_reaping_removes_approvals_without_locking_an_existential_balance() {
    mldsa::with_profiles(|| {
        System::set_block_number(1);
        pallet_limit_orders::LimitOrdersEnabled::<Runtime>::put(true);
        let owner = AccountId::from(pair(231).public());
        let recipient = AccountId::from(pair(232).public());
        let funding = TaoBalance::new(1_000_000_000);
        let _ = Balances::make_free_balance_be(&owner, funding);
        let signed = orders(&owner, &recipient, NetUid::from(42u16))[0].clone();
        let reserved = Balances::reserved_balance(&owner);
        let providers = System::providers(&owner);
        for nonce in 0..8 {
            assert_ok!(LimitOrders::authorize_order(
                RuntimeOrigin::signed(owner.clone()),
                signed.order.clone().with_nonce(nonce).unwrap()
            ));
        }
        assert_eq!(Balances::reserved_balance(&owner), reserved);
        assert_eq!(System::providers(&owner), providers);
        assert_ok!(Balances::transfer_allow_death(
            RuntimeOrigin::signed(owner.clone()),
            recipient.clone().into(),
            funding
        ));
        assert!(!frame_system::Account::<Runtime>::contains_key(&owner));
        assert!(!pallet_limit_orders::ApprovedOrders::<Runtime>::contains_key(&owner));
        let _ = Balances::make_free_balance_be(&owner, funding);
        assert_noop!(
            LimitOrders::execute_orders(
                RuntimeOrigin::signed(recipient),
                unified_orders(vec![signed].try_into().unwrap()),
                true
            ),
            pallet_limit_orders::Error::<Runtime>::OrderNotApproved
        );
    });
}
