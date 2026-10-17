//! Benchmarks for Limit Orders Pallet
#![allow(
    clippy::arithmetic_side_effects,
    clippy::indexing_slicing,
    clippy::unwrap_used
)]
use crate::{NetUid, OrderType, Orders};
use frame_benchmarking::v2::*;
use frame_system::RawOrigin;
use sp_core::{Get, H256};
use sp_runtime::{AccountId32, Perbill, traits::AccountIdConversion};
extern crate alloc;
use crate::{Call, Config, Pallet};

/// Sign a versioned order using the runtime keystore (no `full_crypto` required).
///
/// The key identified by `public` must already be registered in the keystore
/// (e.g. via `sp_io::crypto::sr25519_generate`) before calling this.
///
/// The order is signed in the **human-readable ("clear-signing") form** on
/// purpose: it is the worst case for `is_order_valid`, which tries
/// `verify_order` (raw) and `verify_wrapped` (hash) first and only succeeds on
/// the final `verify_readable`. Signing this form forces all three signature
/// verifications to run, so the measured weight reflects the true worst case.
fn sign_order<T: crate::Config>(
    public: sp_core::sr25519::Public,
    order: &crate::VersionedOrder<T::AccountId>,
) -> crate::SignedOrder<T::AccountId> {
    // Mirror the on-chain check in `verify_readable`: the signed message is the
    // `<Bytes>…</Bytes>`-wrapped canonical readable rendering of the order, hashed
    // when it exceeds Ledger's raw-signing limit (which it always does in practice)
    // exactly as the device does before signing.
    let msg = crate::pallet::Pallet::<T>::render_order(order);
    let payload = [b"<Bytes>".as_slice(), &msg, b"</Bytes>".as_slice()].concat();
    let signed_bytes = if payload.len() > crate::LEDGER_MAX_SIGN_SIZE {
        sp_core::hashing::blake2_256(&payload).to_vec()
    } else {
        payload
    };
    let sig =
        sp_io::crypto::sr25519_sign(sp_core::crypto::key_types::ACCOUNT, &public, &signed_bytes)
            .unwrap();
    crate::SignedOrder {
        order: order.clone(),
        signature: crate::OrderSignature::Sr25519(sig),
        partial_fill: None,
    }
}

/// Generate a deterministic sr25519 key for benchmark index `i` and return its
/// public key. The key is inserted into the runtime keystore so it can sign.
fn benchmark_key(namespace: &str, i: u32) -> (sp_core::sr25519::Public, AccountId32) {
    let seed = alloc::format!("//BenchSigner//{}//{}", namespace, i).into_bytes();
    let public = sp_io::crypto::sr25519_generate(sp_core::crypto::key_types::ACCOUNT, Some(seed));
    let account = AccountId32::from(public);
    (public, account)
}

pub fn order_id<T: crate::Config>(order: &crate::VersionedOrder<T::AccountId>) -> H256 {
    crate::pallet::Pallet::<T>::derive_order_id(order)
}

/// Build `n` signed benchmark orders for `netuid`, one per distinct signer.
///
/// For each index `i` in `0..n` the function:
/// - derives a deterministic sr25519 key in the classical signer namespace,
/// - calls `T::SwapInterface::set_up_acc_for_benchmark` so the account has
///   sufficient balance / stake,
/// - constructs a worst-case `LimitBuy` order (amount = 1 TAO, price = u64::MAX,
///   expiry = u64::MAX, fee 1 %, distinct fee recipient), and
/// - signs it with the generated key.
// Keep per-order execution stable across benchmark repeats. Use one TAO
// so every order clears the pallet/subtensor minimum amount checks while
// avoiding the reserve-draining edge cases caused by very large orders.
const BENCHMARK_ORDER_AMOUNT: u64 = 1_000_000_000;

fn make_benchmark_orders<T: crate::Config>(
    n: u32,
    netuid: NetUid,
) -> alloc::vec::Vec<crate::SignedOrder<T::AccountId>> {
    make_benchmark_orders_in_namespace::<T>(n, netuid, "classical")
}

fn make_benchmark_orders_in_namespace<T: crate::Config>(
    n: u32,
    netuid: NetUid,
    namespace: &'static str,
) -> alloc::vec::Vec<crate::SignedOrder<T::AccountId>> {
    use subtensor_swap_interface::OrderSwapInterface;

    let mut orders = alloc::vec::Vec::new();

    for i in 0..n {
        let (public, account_id) = benchmark_key(namespace, i);
        let account: T::AccountId = account_id.into();
        let fee_recipient: T::AccountId = frame_benchmarking::account(namespace, i, 1);

        T::SwapInterface::set_up_acc_for_benchmark(&account, &account);
        T::SwapInterface::set_up_acc_for_benchmark(&fee_recipient, &fee_recipient);

        let order = crate::VersionedOrder::V1(crate::Order {
            signer: account.clone(),
            hotkey: account.clone(),
            netuid,
            order_type: OrderType::LimitBuy,
            amount: BENCHMARK_ORDER_AMOUNT,
            limit_price: u64::MAX,
            expiry: u64::MAX,
            fee_rate: Perbill::from_percent(1),
            fee_recipient,
            relayer: None,
            max_slippage: None,
            chain_id: T::ChainId::get(),
            partial_fills_enabled: false,
        });
        orders.push(sign_order::<T>(public, &order));
    }

    orders
}

/// Exercise maximum relayer bytes and linked-provider reads/writes as well as
/// cold verification. Input/provider records are setup outside measurement.
fn make_benchmark_authorized_orders<T: crate::Config>(
    n: u32,
    netuid: NetUid,
) -> (
    frame_support::BoundedVec<crate::AuthorizedOrder, crate::MaxAuthorizedOrders>,
    alloc::vec::Vec<H256>,
) {
    use crate::OrderCapabilities;
    use codec::Encode;
    let mut entries = alloc::vec::Vec::new();
    let mut ids = alloc::vec::Vec::new();
    // Capability setup permanently protects these owners in the real runtime.
    // They must never overlap with the classical orders in a mixed batch.
    for (i, legacy) in make_benchmark_orders_in_namespace::<T>(n, netuid, "mldsa")
        .into_iter()
        .enumerate()
    {
        let old = legacy.order.view();
        let provider = H256(sp_core::hashing::blake2_256(
            &(b"v2/benchmark/provider", i as u64).encode(),
        ));
        crate::LinkedOutputs::<T>::insert(
            provider,
            crate::LinkedOutput {
                signer: old.signer.clone(),
                asset: crate::LinkedAsset::Tao,
                total: BENCHMARK_ORDER_AMOUNT,
                expires_at: u64::MAX,
            },
        );
        let mut relayers = alloc::vec::Vec::new();
        relayers.push(frame_benchmarking::account("caller", 0, 0));
        for r in 1..10 {
            relayers.push(frame_benchmarking::account("relayer", r, 0));
        }
        let order = crate::VersionedOrder::V2(crate::OrderV2 {
            signer: old.signer,
            hotkey: old.hotkey,
            netuid: old.netuid,
            order_type: old.order_type,
            amount: crate::OrderAmount::LinkedPercentage {
                provider,
                pct: Perbill::one(),
            },
            limit_price: old.limit_price,
            expiry: old.expiry,
            fee_rate: old.fee_rate,
            fee_recipient: old.fee_recipient,
            relayer: Some(relayers.try_into().unwrap()),
            max_slippage: Some(Perbill::one()),
            chain_id: old.chain_id,
            partial_fills_enabled: false,
            has_linked_order: true,
        });
        let mut seed = [0u8; 32];
        seed[..8].copy_from_slice(&(i as u64).to_le_bytes());
        let public_key = subtensor_hashed::mldsa::public_key_from_seed(&seed).unwrap();
        T::Capabilities::setup_benchmark(order.signer(), 1, &public_key, &order.view()).unwrap();
        let id = Pallet::<T>::derive_authorized_order_id(0, &order);
        crate::Orders::<T>::remove(id);
        crate::OrderNonces::<T>::remove(order.signer(), 0);
        let payload = Pallet::<T>::order_signing_payload(1, 0, &order);
        let signature =
            subtensor_hashed::mldsa::sign_message(&seed, &payload, crate::ORDER_CONTEXT, &[0; 32])
                .unwrap();
        entries.push(crate::AuthorizedOrder {
            nonce: 0,
            order,
            partial_fill: None,
            authorization: crate::OrderAuthorization::MlDsa(crate::MlDsaOrderProof {
                epoch: 1,
                public_key,
                signature: signature.to_vec().try_into().unwrap(),
            }),
        });
        ids.push(id);
    }
    (entries.try_into().unwrap(), ids)
}

#[benchmarks]
mod benchmarks {
    use super::*;
    use frame_support::traits::Get;
    use subtensor_swap_interface::OrderSwapInterface;

    #[benchmark]
    fn cancel_order() {
        let (public, account_id) = benchmark_key("classical", 0);
        let account: T::AccountId = account_id.into();

        let order = crate::VersionedOrder::V1(crate::Order {
            signer: account.clone(),
            hotkey: account.clone(),
            netuid: NetUid::from(1u16),
            order_type: OrderType::LimitBuy,
            amount: 1_000,
            limit_price: 2_000_000_000,
            expiry: 1_000_000_000,
            fee_rate: Perbill::zero(),
            fee_recipient: account.clone(),
            relayer: None,
            max_slippage: None,
            chain_id: T::ChainId::get(),
            partial_fills_enabled: false,
        });
        let signed = sign_order::<T>(public, &order);

        #[extrinsic_call]
        _(RawOrigin::Signed(account.clone()), signed.order.clone());

        let id = order_id::<T>(&signed.order);
        assert_eq!(Orders::<T>::get(id), Some(crate::OrderStatus::Cancelled));
    }

    #[benchmark]
    fn set_pallet_status() {
        #[extrinsic_call]
        _(RawOrigin::Root, false);

        assert_eq!(crate::LimitOrdersEnabled::<T>::get(), false);
    }

    #[benchmark]
    fn prune_linked_output() {
        let (_, account_id) = benchmark_key("classical", 0);
        let account: T::AccountId = account_id.into();
        let order_id = H256::repeat_byte(0x11);
        crate::LinkedOutputs::<T>::insert(
            order_id,
            crate::LinkedOutput {
                signer: account.clone(),
                asset: crate::LinkedAsset::Tao,
                total: 1_000,
                expires_at: u64::MAX,
            },
        );

        #[extrinsic_call]
        _(RawOrigin::Signed(account), order_id);

        assert!(crate::LinkedOutputs::<T>::get(order_id).is_none());
    }

    /// Worst case: `n` valid orders each with a distinct signer (coldkey/hotkey)
    /// and a distinct fee recipient. The benchmark runs in all-or-nothing mode
    /// and verifies every order is fulfilled, so silently skipped or stale orders
    /// cannot produce cheaper/noisy measurements across repeats.
    #[benchmark]
    fn execute_orders(n: Linear<1, { T::MaxOrdersPerBatch::get() }>) {
        let netuid = NetUid::from(1u16);
        crate::LimitOrdersEnabled::<T>::set(true);
        T::SwapInterface::set_up_netuid_for_benchmark(netuid);

        let orders = make_benchmark_orders::<T>(n, netuid);
        let order_ids = orders
            .iter()
            .map(|signed| order_id::<T>(&signed.order))
            .collect::<alloc::vec::Vec<_>>();

        // Benchmark externalities are reused across samples/repeats. Remove any
        // terminal status left by an earlier run so every sample measures the same
        // successful execution path, rather than the cheaper already-processed path.
        for id in &order_ids {
            Orders::<T>::remove(id);
        }

        let bounded_orders: frame_support::BoundedVec<_, T::MaxOrdersPerBatch> =
            frame_support::BoundedVec::try_from(orders).unwrap();
        let caller: T::AccountId = frame_benchmarking::account("caller", 0, 0);

        frame_system::Pallet::<T>::reset_events();

        #[extrinsic_call]
        _(RawOrigin::Signed(caller), bounded_orders, true);

        for id in order_ids {
            assert_eq!(Orders::<T>::get(id), Some(crate::OrderStatus::Fulfilled));
        }
    }

    /// Worst case: `n` buy orders each with a distinct signer and fee recipient,
    /// maximising asset-collection reads, pro-rata distribution writes, and the
    /// number of unique fee-transfer recipients in `collect_fees`.
    #[benchmark]
    fn execute_batched_orders(n: Linear<1, { T::MaxOrdersPerBatch::get() }>) {
        let netuid = NetUid::from(1u16);
        crate::LimitOrdersEnabled::<T>::set(true);
        T::SwapInterface::set_up_netuid_for_benchmark(netuid);

        // Set up the pallet intermediary so the net pool swap and alpha
        // distribution transfers succeed.
        let pallet_acct: T::AccountId = T::PalletId::get().into_account_truncating();
        let pallet_hotkey: T::AccountId = T::PalletHotkey::get();
        T::SwapInterface::set_up_acc_for_benchmark(&pallet_hotkey, &pallet_acct);

        let orders = make_benchmark_orders::<T>(n, netuid);

        let bounded_orders: frame_support::BoundedVec<_, T::MaxOrdersPerBatch> =
            frame_support::BoundedVec::try_from(orders).unwrap();
        let caller: T::AccountId = frame_benchmarking::account("caller", 0, 0);

        #[extrinsic_call]
        _(RawOrigin::Signed(caller), netuid, bounded_orders);
    }

    #[benchmark]
    fn authorize_order() {
        crate::LimitOrdersEnabled::<T>::set(true);
        let netuid = NetUid::from(1u16);
        T::SwapInterface::set_up_netuid_for_benchmark(netuid);
        let order = make_benchmark_authorized_orders::<T>(1, netuid)
            .0
            .into_iter()
            .next()
            .unwrap()
            .order;
        let signer = order.signer().clone();
        let id = Pallet::<T>::derive_authorized_order_id(0, &order);
        crate::Orders::<T>::remove(id);
        crate::OrderNonces::<T>::remove(&signer, 0);

        #[extrinsic_call]
        _(RawOrigin::Signed(signer), order.with_nonce(0).unwrap());

        assert!(crate::ApprovedOrders::<T>::contains_key(id));
    }

    #[benchmark]
    fn cancel_authenticated_order() {
        let netuid = NetUid::from(1u16);
        T::SwapInterface::set_up_netuid_for_benchmark(netuid);
        let order = make_benchmark_authorized_orders::<T>(1, netuid)
            .0
            .into_iter()
            .next()
            .unwrap()
            .order;
        let signer = order.signer().clone();
        let id = Pallet::<T>::derive_authorized_order_id(0, &order);
        crate::Orders::<T>::insert(id, crate::OrderStatus::PartiallyFilled(1));
        crate::OrderNonces::<T>::insert(&signer, 0, id);
        crate::ApprovedOrders::<T>::insert(id, Pallet::<T>::order_approval_commitment(0, &order));

        #[extrinsic_call]
        cancel_order(RawOrigin::Signed(signer), order.with_nonce(0).unwrap());

        assert_eq!(
            crate::Orders::<T>::get(id),
            Some(crate::OrderStatus::Cancelled)
        );
        assert!(!crate::ApprovedOrders::<T>::contains_key(id));
    }

    /// Every proof is valid and performs portable ML-DSA verification followed
    /// by a real swap and capability budget mutation. Includes the full bounded
    /// proof length; no mocked signature or guessed verification weight.
    #[benchmark]
    fn execute_authenticated_orders(
        n: Linear<8, { T::MaxOrdersPerBatch::get() }>,
        p: Linear<1, 8>,
    ) {
        let netuid = NetUid::from(1u16);
        crate::LimitOrdersEnabled::<T>::set(true);
        T::SwapInterface::set_up_netuid_for_benchmark(netuid);
        let (orders, _) = make_benchmark_authorized_orders::<T>(p, netuid);
        let mut combined: alloc::vec::Vec<_> = orders
            .into_iter()
            .map(|signed| crate::SignedOrder::try_from(signed).unwrap())
            .collect();
        combined.extend(make_benchmark_orders::<T>(n.saturating_sub(p), netuid));
        let ids: alloc::vec::Vec<_> = combined
            .iter()
            .map(|signed| Pallet::<T>::derive_order_id(&signed.order))
            .collect();
        let orders: frame_support::BoundedVec<_, T::MaxOrdersPerBatch> =
            combined.try_into().unwrap();
        let caller: T::AccountId = frame_benchmarking::account("caller", 0, 0);
        frame_system::Pallet::<T>::reset_events();

        #[extrinsic_call]
        execute_orders(RawOrigin::Signed(caller), orders, true);

        for id in ids {
            assert_eq!(
                crate::Orders::<T>::get(id),
                Some(crate::OrderStatus::Fulfilled)
            );
        }
    }

    #[benchmark]
    fn execute_authenticated_batched_orders(
        n: Linear<8, { T::MaxOrdersPerBatch::get() }>,
        p: Linear<1, 8>,
    ) {
        let netuid = NetUid::from(1u16);
        crate::LimitOrdersEnabled::<T>::set(true);
        T::SwapInterface::set_up_netuid_for_benchmark(netuid);
        let pallet_acct: T::AccountId = T::PalletId::get().into_account_truncating();
        let pallet_hotkey = T::PalletHotkey::get();
        T::SwapInterface::set_up_acc_for_benchmark(&pallet_hotkey, &pallet_acct);
        let (orders, _) = make_benchmark_authorized_orders::<T>(p, netuid);
        let mut combined: alloc::vec::Vec<_> = orders
            .into_iter()
            .map(|signed| crate::SignedOrder::try_from(signed).unwrap())
            .collect();
        combined.extend(make_benchmark_orders::<T>(n.saturating_sub(p), netuid));
        let ids: alloc::vec::Vec<_> = combined
            .iter()
            .map(|signed| Pallet::<T>::derive_order_id(&signed.order))
            .collect();
        let orders: frame_support::BoundedVec<_, T::MaxOrdersPerBatch> =
            combined.try_into().unwrap();
        let caller: T::AccountId = frame_benchmarking::account("caller", 0, 0);
        frame_system::Pallet::<T>::reset_events();

        #[extrinsic_call]
        execute_batched_orders(RawOrigin::Signed(caller), netuid, orders);

        for id in ids {
            assert_eq!(
                crate::Orders::<T>::get(id),
                Some(crate::OrderStatus::Fulfilled)
            );
        }
    }

    /// Execute setup, dispatch and verification only: no timing or weight output.
    pub fn verify_mixed_order_fixture<T: Config>(
        n: u32,
        p: u32,
        netted: bool,
    ) -> Result<(), frame_benchmarking::BenchmarkError> {
        use frame_benchmarking::{BenchmarkParameter, BenchmarkingSetup};
        let components = [(BenchmarkParameter::n, n), (BenchmarkParameter::p, p)];
        if netted {
            <execute_authenticated_batched_orders as BenchmarkingSetup<T>>::unit_test_instance(
                &execute_authenticated_batched_orders,
                &components,
            )
        } else {
            <execute_authenticated_orders as BenchmarkingSetup<T>>::unit_test_instance(
                &execute_authenticated_orders,
                &components,
            )
        }
    }

    impl_benchmark_test_suite!(
        Pallet,
        crate::tests::mock::new_test_ext(),
        crate::tests::mock::Test
    );
}

pub use benchmarks::verify_mixed_order_fixture;
