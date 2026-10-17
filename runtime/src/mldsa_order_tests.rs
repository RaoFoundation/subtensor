//! Real balance, subnet reserve and alpha-share rollback for detached order batches.
use super::*;
use frame_support::{assert_noop, assert_ok, traits::Get};
use pallet_hashed_accounts::{ActiveCapabilities, CapabilityRole, CapabilityScope, TradingScope};
use pallet_limit_orders::{
    AuthorizedOrder, MlDsaOrderProof, Order, OrderAuthorization, OrderStatus, OrderType,
    VersionedOrder,
};
use sp_runtime::{Perbill, traits::AccountIdConversion};
use subtensor_hashed::mldsa as mldsa_crypto;
use subtensor_runtime_common::AlphaBalance;

fn grant(
    owner: &AccountId,
    public: &mldsa_crypto::PublicKeyBytes,
    recipient: &AccountId,
    netuid: NetUid,
    epoch: u64,
    total: u64,
) {
    assert_ok!(
        Executive::apply_extrinsic(mldsa::sign(
            81,
            RuntimeCall::HashedAccounts(pallet_hashed_accounts::Call::install_capability {
                expected_epoch: epoch,
                key_commitment: mldsa_crypto::commitment(public),
                expires_at: 100,
                scope: CapabilityScope::Trading(TradingScope {
                    netuid: <u16 as From<NetUid>>::from(netuid),
                    hotkey: owner.clone(),
                    fee_recipient: recipient.clone(),
                    max_fee_perbill: 10_000_000,
                    max_tao_per_order: 1_000_000_000,
                    max_alpha_per_order: 0,
                    total_tao: total,
                    total_alpha: 0,
                }),
            })
        ))
        .unwrap()
    );
}

fn orders(
    owner: &AccountId,
    recipient: &AccountId,
    netuid: NetUid,
    epoch: u64,
    seed: &[u8; 32],
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
            let payload = LimitOrders::order_signing_payload(epoch, nonce, &order);
            AuthorizedOrder {
                nonce,
                order,
                partial_fill: None,
                authorization: OrderAuthorization::MlDsa(MlDsaOrderProof {
                    epoch,
                    public_key: mldsa_crypto::public_key_from_seed(seed).unwrap(),
                    signature: mldsa_crypto::sign_message(
                        seed,
                        &payload,
                        pallet_limit_orders::ORDER_CONTEXT,
                        &[0; 32],
                    )
                    .unwrap()
                    .to_vec()
                    .try_into()
                    .unwrap(),
                }),
            }
        })
        .collect::<Vec<_>>()
        .try_into()
        .unwrap()
}

#[test]
fn aggregate_capability_failure_reverts_real_assets_fees_reserves_and_order_state() {
    mldsa::with_profiles(|| {
        let owner = mldsa::register(81);
        let recipient = AccountId::new([82; 32]);
        let relayer = AccountId::from(pair(83).public());
        let netuid = NetUid::from(42u16);
        SubtensorModule::init_new_network(netuid, 100);
        pallet_subtensor::SubnetMechanism::<Runtime>::insert(netuid, 0);
        pallet_subtensor::SubtokenEnabled::<Runtime>::insert(netuid, true);
        pallet_subtensor::SubnetTAO::<Runtime>::insert(netuid, TaoBalance::new(1_000_000_000_000));
        pallet_subtensor::SubnetAlphaIn::<Runtime>::insert(
            netuid,
            AlphaBalance::new(1_000_000_000_000),
        );
        assert_ok!(SubtensorModule::create_account_if_non_existent(
            &owner, &owner
        ));
        let intermediary: AccountId = LimitOrdersPalletId::get().into_account_truncating();
        let intermediary_hotkey = LimitOrdersPalletHotkey::get();
        assert_ok!(SubtensorModule::create_account_if_non_existent(
            &intermediary,
            &intermediary_hotkey
        ));
        for account in [&recipient, &intermediary, &relayer] {
            let _ = Balances::make_free_balance_be(account, TaoBalance::new(1_000_000_000));
        }
        pallet_limit_orders::LimitOrdersEnabled::<Runtime>::put(true);
        let seed = [84; 32];
        let public = mldsa_crypto::public_key_from_seed(&seed).unwrap();
        grant(&owner, &public, &recipient, netuid, 1, 1_500_000_000);
        let batch = orders(&owner, &recipient, netuid, 1, &seed);
        // A classical owner shares the same netted transaction. Its real funds
        // must roll back if a later ML-DSA aggregate budget fails.
        let classical_pair = pair(85);
        let classical_owner = AccountId::from(classical_pair.public());
        let _ = Balances::make_free_balance_be(&classical_owner, TaoBalance::new(2_000_000_000));
        assert_ok!(SubtensorModule::create_account_if_non_existent(
            &classical_owner,
            &classical_owner
        ));
        let mut classical_terms = batch[0].order.clone();
        if let VersionedOrder::V1(terms) = &mut classical_terms {
            terms.signer = classical_owner.clone();
            terms.hotkey = classical_owner.clone();
            terms.fee_rate = Perbill::zero();
        }
        let classical_id = LimitOrders::derive_order_id(&classical_terms);
        let classical = pallet_limit_orders::SignedOrder {
            signature: sp_runtime::MultiSignature::Sr25519(
                classical_pair.sign(&classical_terms.encode()),
            )
            .into(),
            order: classical_terms,
            partial_fill: None,
        };
        let classical_before = Balances::free_balance(&classical_owner);
        let ids: Vec<_> = batch
            .iter()
            .map(|signed| LimitOrders::derive_authorized_order_id(signed.nonce, &signed.order))
            .collect();
        let balances = [&owner, &recipient, &intermediary].map(Balances::free_balance);
        let reserve = pallet_subtensor::SubnetTAO::<Runtime>::get(netuid);
        let alpha_reserve = pallet_subtensor::SubnetAlphaIn::<Runtime>::get(netuid);
        let capability = ActiveCapabilities::<Runtime>::get(&owner, CapabilityRole::Order).unwrap();
        assert_eq!(
            SubtensorModule::get_stake_for_hotkey_and_coldkey_on_subnet(&owner, &owner, netuid),
            AlphaBalance::ZERO
        );

        let mut mixed = unified_orders(batch);
        mixed.try_insert(0, classical.clone()).unwrap();
        assert_ok!(AdminUtils::sudo_set_hashed_accounts_enabled(
            RuntimeOrigin::root(),
            false
        ));
        assert_noop!(
            LimitOrders::execute_batched_orders(
                RuntimeOrigin::signed(relayer.clone()),
                netuid,
                mixed.clone()
            ),
            pallet_hashed_accounts::Error::<Runtime>::Disabled
        );
        assert_ok!(AdminUtils::sudo_set_hashed_accounts_enabled(
            RuntimeOrigin::root(),
            true
        ));
        // Each order fits individually; only the post-fill aggregate counter
        // fails. assert_noop also checks the entire storage root, covering real
        // balances, issuance, reserves, share pools, events and linked state.
        assert_noop!(
            LimitOrders::execute_batched_orders(
                RuntimeOrigin::signed(relayer.clone()),
                netuid,
                mixed
            ),
            pallet_hashed_accounts::Error::<Runtime>::CapabilityScopeViolation
        );
        assert_eq!(Balances::free_balance(&classical_owner), classical_before);
        assert_eq!(
            SubtensorModule::get_stake_for_hotkey_and_coldkey_on_subnet(
                &classical_owner,
                &classical_owner,
                netuid
            ),
            AlphaBalance::ZERO
        );
        assert!(!pallet_limit_orders::Orders::<Runtime>::contains_key(
            classical_id
        ));
        assert_eq!(
            [&owner, &recipient, &intermediary].map(Balances::free_balance),
            balances
        );
        assert_eq!(pallet_subtensor::SubnetTAO::<Runtime>::get(netuid), reserve);
        assert_eq!(
            pallet_subtensor::SubnetAlphaIn::<Runtime>::get(netuid),
            alpha_reserve
        );
        assert_eq!(
            ActiveCapabilities::<Runtime>::get(&owner, CapabilityRole::Order),
            Some(capability)
        );
        assert_eq!(
            SubtensorModule::get_stake_for_hotkey_and_coldkey_on_subnet(&owner, &owner, netuid),
            AlphaBalance::ZERO
        );
        assert_eq!(
            SubtensorModule::get_stake_for_hotkey_and_coldkey_on_subnet(
                &intermediary_hotkey,
                &intermediary,
                netuid
            ),
            AlphaBalance::ZERO
        );
        for (nonce, id) in ids.iter().enumerate() {
            assert!(!pallet_limit_orders::OrderNonces::<Runtime>::contains_key(
                &owner,
                nonce as u64
            ));
            assert!(!pallet_limit_orders::Orders::<Runtime>::contains_key(id));
        }

        // A larger explicitly approved replacement grant and fresh signatures
        // prove the same fixture reaches successful economic execution.
        grant(&owner, &public, &recipient, netuid, 2, 2_000_000_000);
        let owner_before = Balances::free_balance(&owner);
        let mut mixed = unified_orders(orders(&owner, &recipient, netuid, 2, &seed));
        mixed.try_insert(0, classical).unwrap();
        assert_ok!(LimitOrders::execute_batched_orders(
            RuntimeOrigin::signed(relayer),
            netuid,
            mixed
        ));
        assert_eq!(
            Balances::free_balance(&classical_owner),
            classical_before.saturating_sub(TaoBalance::new(1_000_000_000))
        );
        assert_eq!(
            SubtensorModule::get_stake_for_hotkey_and_coldkey_on_subnet(
                &classical_owner,
                &classical_owner,
                netuid
            ),
            AlphaBalance::new(1_000_000_000)
        );
        assert_eq!(
            pallet_limit_orders::Orders::<Runtime>::get(classical_id),
            Some(OrderStatus::Fulfilled)
        );
        assert_eq!(
            Balances::free_balance(&owner),
            owner_before.saturating_sub(TaoBalance::new(2_000_000_000))
        );
        assert_eq!(
            Balances::free_balance(&recipient),
            balances[1].saturating_add(TaoBalance::new(20_000_000))
        );
        assert_eq!(Balances::free_balance(&intermediary), balances[2]);
        assert_eq!(
            SubtensorModule::get_stake_for_hotkey_and_coldkey_on_subnet(&owner, &owner, netuid),
            AlphaBalance::new(1_980_000_000)
        );
        assert_eq!(
            ActiveCapabilities::<Runtime>::get(&owner, CapabilityRole::Order)
                .unwrap()
                .spent_tao,
            2_000_000_000
        );
        for id in ids {
            assert_eq!(
                pallet_limit_orders::Orders::<Runtime>::get(id),
                Some(OrderStatus::Fulfilled)
            );
        }
    });
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
                let mut signed = orders(&owner, &delegate, netuid, 0, &[88; 32])[0].clone();
                signed.authorization = OrderAuthorization::AccountApproved;
                let id = LimitOrders::derive_authorized_order_id(signed.nonce, &signed.order);
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
                    assert!(!pallet_limit_orders::ApprovedOrders::<Runtime>::contains_key(id));
                    assert!(!pallet_limit_orders::OrderNonces::<Runtime>::contains_key(
                        &owner,
                        signed.nonce
                    ));
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
                assert!(pallet_limit_orders::ApprovedOrders::<Runtime>::contains_key(id));
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
