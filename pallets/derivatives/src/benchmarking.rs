//! Benchmarks for `pallet_derivatives`.
//!
//! Both position benchmarks use an alpha cushion: it is the heavier deposit path at open, and
//! at close it is the only path on which every swap can fire.
#![allow(clippy::arithmetic_side_effects, clippy::unwrap_used)]

use frame_benchmarking::v2::*;
use frame_system::RawOrigin;
use subtensor_runtime_common::{NetUid, TaoBalance};
use subtensor_swap_interface::OrderSwapInterface;

use crate::*;

/// TAO the owner turns into the alpha cushion.
const CUSHION_TAO: u64 = 10_000_000_000;
/// TAO a whale trades to move the pool price against the position.
const WHALE_TAO: u64 = 300_000_000_000;
/// Smaller price move for `roll`: the position must lose, but keep enough cushion to reopen.
const ROLL_WHALE_TAO: u64 = 100_000_000_000;

fn setup<T: Config>() -> (T::AccountId, NetUid) {
    let netuid = NetUid::from(1u16);
    T::Pool::set_up_pool_for_benchmark(netuid);
    Pallet::<T>::claim_hotkey();

    let owner: T::AccountId = frame_benchmarking::account("owner", 0, 0);
    T::Pool::set_up_acc_for_benchmark(&owner, &owner);
    (owner, netuid)
}

/// Alpha staked at `(owner, owner)`, bought from the pool.
fn alpha_cushion<T: Config>(owner: &T::AccountId, netuid: NetUid) -> Deposit<T::AccountId> {
    let amount =
        T::Pool::buy_alpha_internal(owner, owner, netuid, TaoBalance::from(CUSHION_TAO)).unwrap();
    Deposit::Alpha {
        hotkey: owner.clone(),
        amount,
    }
}

/// Fill the `MAX_EXPIRY_SHIFT - 1` queues a new position probes first, so `schedule_expiry`
/// walks every one of them before it finds room.
fn fill_expiry_queues<T: Config>(owner: &T::AccountId, netuid: NetUid) {
    let now = frame_system::Pallet::<T>::block_number();
    let mut at = now.saturating_add(Params::<T>::get().lifetime_blocks);
    for _ in 1..settle::MAX_EXPIRY_SHIFT {
        Expiring::<T>::mutate(at, |queue| {
            while queue.try_push((owner.clone(), netuid, Side::Short)).is_ok() {}
        });
        at.saturating_inc();
    }
}

#[benchmarks]
mod benchmarks {
    use super::*;

    /// Worst case: an alpha cushion (the validated stake transfer) and a full expiry window.
    #[benchmark]
    fn open() {
        let (owner, netuid) = setup::<T>();
        let deposit = alpha_cushion::<T>(&owner, netuid);
        fill_expiry_queues::<T>(&owner, netuid);

        #[extrinsic_call]
        _(
            RawOrigin::Signed(owner.clone()),
            netuid,
            Side::Long,
            deposit,
        );

        let position = Positions::<T>::get(&owner, (netuid, Side::Long)).unwrap();
        let nominal = position
            .opened_at
            .saturating_add(Params::<T>::get().lifetime_blocks);
        assert_eq!(
            position.expires_at,
            nominal.saturating_add((settle::MAX_EXPIRY_SHIFT - 1).into())
        );
    }

    /// Worst case: a long with an alpha cushion, closed after the price fell and after its
    /// hotkey was swapped away. Every swap fires: sell the proceeds, sell cushion alpha for the
    /// debt gap, sell cushion alpha for the fee, then sell what is left of the cushion because
    /// it can no longer go back in kind.
    #[benchmark]
    fn close() {
        let (owner, netuid) = setup::<T>();
        let whale: T::AccountId = frame_benchmarking::account("whale", 0, 0);
        T::Pool::set_up_acc_for_benchmark(&whale, &whale);

        // Whale buys first so its later sale drops the price below the open.
        let whale_alpha =
            T::Pool::buy_alpha_internal(&whale, &whale, netuid, TaoBalance::from(WHALE_TAO))
                .unwrap();
        let deposit = alpha_cushion::<T>(&owner, netuid);
        Pallet::<T>::do_open(owner.clone(), netuid, Side::Long, deposit).unwrap();
        T::Pool::sell_alpha_internal(&whale, &whale, netuid, whale_alpha).unwrap();
        T::Pool::forget_hotkey_for_benchmark(&owner);

        #[extrinsic_call]
        _(
            RawOrigin::Signed(owner.clone()),
            owner.clone(),
            netuid,
            Side::Long,
        );

        assert!(!Positions::<T>::contains_key(&owner, (netuid, Side::Long)));
        assert_eq!(Footprint::<T>::get(netuid, Side::Long), 0);
    }

    /// Worst case: the `close` path with the cushion coming back in kind (sell proceeds, sell
    /// cushion alpha for the debt gap and the fee, return the rest as stake), then the `open`
    /// path with an alpha cushion plus top-up and a full expiry window.
    #[benchmark]
    fn roll() {
        let (owner, netuid) = setup::<T>();
        let whale: T::AccountId = frame_benchmarking::account("whale", 0, 0);
        T::Pool::set_up_acc_for_benchmark(&whale, &whale);

        let whale_alpha =
            T::Pool::buy_alpha_internal(&whale, &whale, netuid, TaoBalance::from(ROLL_WHALE_TAO))
                .unwrap();
        let deposit = alpha_cushion::<T>(&owner, netuid);
        Pallet::<T>::do_open(owner.clone(), netuid, Side::Long, deposit).unwrap();
        T::Pool::sell_alpha_internal(&whale, &whale, netuid, whale_alpha).unwrap();
        let top_up = alpha_cushion::<T>(&owner, netuid);
        // One block later, so the slot the old position frees in its own queue is not one the
        // new expiry probes.
        let now = frame_system::Pallet::<T>::block_number().saturating_add(1u32.into());
        frame_system::Pallet::<T>::set_block_number(now);
        fill_expiry_queues::<T>(&owner, netuid);

        #[extrinsic_call]
        _(
            RawOrigin::Signed(owner.clone()),
            netuid,
            Side::Long,
            Some(top_up),
        );

        let after = Positions::<T>::get(&owner, (netuid, Side::Long)).unwrap();
        let nominal = now.saturating_add(Params::<T>::get().lifetime_blocks);
        assert_eq!(
            after.expires_at,
            nominal.saturating_add((settle::MAX_EXPIRY_SHIFT - 1).into())
        );
        assert!(matches!(after.cushion, Deposit::Alpha { .. }));
    }

    #[benchmark]
    fn sudo_set_params() {
        let mut params = Params::<T>::get();
        params.shorts_enabled = false;

        #[extrinsic_call]
        _(RawOrigin::Root, params.clone());

        assert_eq!(Params::<T>::get(), params);
    }

    impl_benchmark_test_suite!(
        Pallet,
        crate::tests::mock::new_test_ext(),
        crate::tests::mock::Test
    );
}
