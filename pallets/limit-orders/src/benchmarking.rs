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
use sp_runtime::{AccountId32, MultiSignature, Perbill, traits::AccountIdConversion};
extern crate alloc;
use crate::{Call, Config, Pallet};
use codec::Encode;

/// Sign a versioned order using the runtime keystore (no `full_crypto` required).
///
/// The key identified by `public` must already be registered in the keystore
/// (e.g. via `sp_io::crypto::sr25519_generate`) before calling this.
fn sign_order<T: crate::Config>(
    public: sp_core::sr25519::Public,
    order: &crate::VersionedOrder<T::AccountId>,
) -> crate::SignedOrder<T::AccountId> {
    let sig = sp_io::crypto::sr25519_sign(
        sp_core::crypto::key_types::ACCOUNT,
        &public,
        &order.encode(),
    )
    .unwrap();
    crate::SignedOrder {
        order: order.clone(),
        signature: MultiSignature::Sr25519(sig),
        partial_fill: None,
    }
}

/// Generate a deterministic sr25519 key for benchmark index `i` and return its
/// public key. The key is inserted into the runtime keystore so it can sign.
fn benchmark_key(i: u32) -> (sp_core::sr25519::Public, AccountId32) {
    let seed = alloc::format!("//BenchSigner{}", i).into_bytes();
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
/// - derives a deterministic sr25519 key via `benchmark_key(i)`,
/// - calls `T::SwapInterface::set_up_acc_for_benchmark` so the account has
///   sufficient balance / stake,
/// - constructs a worst-case `LimitBuy` order (amount = 1 TAO, price = u64::MAX,
///   expiry = u64::MAX, fee 1 %, distinct fee recipient), and
/// - signs it with the generated key.
// Keep per-order execution stable across benchmark repeats. The amount
// is small but non-zero so each valid buy follows the same pool and
// non-zero-fee path without pushing reserves into edge cases.
const BENCHMARK_ORDER_AMOUNT: u64 = 1_000_000;

fn make_benchmark_orders<T: crate::Config>(
    n: u32,
    netuid: NetUid,
) -> alloc::vec::Vec<crate::SignedOrder<T::AccountId>> {
    use subtensor_swap_interface::OrderSwapInterface;

    let mut orders = alloc::vec::Vec::new();

    for i in 0..n {
        let (public, account_id) = benchmark_key(i);
        let account: T::AccountId = account_id.into();
        let fee_recipient: T::AccountId = frame_benchmarking::account("fee_recipient", i, 0);

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

#[benchmarks]
mod benchmarks {
    use super::*;
    use frame_support::traits::Get;
    use subtensor_swap_interface::OrderSwapInterface;

    #[benchmark]
    fn cancel_order() {
        let (public, account_id) = benchmark_key(0);
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

    impl_benchmark_test_suite!(
        Pallet,
        crate::tests::mock::new_test_ext(),
        crate::tests::mock::Test
    );
}
