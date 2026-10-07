//! Dispatch and background-work reference benchmarks using real pool operations.
#![allow(clippy::unwrap_used)]
use super::*;
use frame_benchmarking::v2::*;
use frame_system::RawOrigin;

fn setup<T: Config>() -> (T::AccountId, T::AccountId, NetUid) {
    let owner: T::AccountId = whitelisted_caller();
    let hotkey: T::AccountId = account("lending-user-hotkey", 0, 0);
    let netuid: NetUid = 1.into();
    T::Pool::set_up_netuid_for_benchmark(netuid);
    T::Pool::set_up_acc_for_benchmark(&hotkey, &owner);
    Pallet::<T>::initialize_custody().unwrap();
    let vault = Pallet::<T>::reserve_account(netuid);
    let custody = Pallet::<T>::custody_hotkey().unwrap();
    T::Pool::transfer_tao(&owner, &vault, 400_000_000_000_u64.into()).unwrap();
    let vault_alpha = T::Pool::buy_alpha(
        &vault,
        &custody,
        netuid,
        200_000_000_000_u64.into(),
        u64::MAX.into(),
        false,
    )
    .unwrap();
    T::Pool::buy_alpha(
        &owner,
        &hotkey,
        netuid,
        100_000_000_000_u64.into(),
        u64::MAX.into(),
        false,
    )
    .unwrap();
    Pallet::<T>::fund_reserves(
        netuid,
        200_000_000_000_u64.into(),
        vault_alpha,
        T::Pool::current_alpha_price(netuid),
        true,
    )
    .unwrap();
    Enabled::<T>::put(true);
    (owner, hotkey, netuid)
}

#[benchmarks]
mod benchmarks {
    use super::*;

    #[benchmark]
    fn open() {
        let (owner, hotkey, netuid) = setup::<T>();
        #[extrinsic_call]
        _(
            RawOrigin::Signed(owner.clone()),
            netuid,
            Side::Short,
            16_000_000_000,
            hotkey,
            1,
            0,
        );
        assert!(Positions::<T>::contains_key(owner, netuid));
    }

    #[benchmark(extra)]
    fn increase_short() {
        let (owner, hotkey, netuid) = setup::<T>();
        Pallet::<T>::open(
            RawOrigin::Signed(owner.clone()).into(),
            netuid,
            Side::Short,
            16_000_000_000,
            hotkey.clone(),
            1,
            0,
        )
        .unwrap();
        let before = Positions::<T>::get(&owner, netuid).unwrap();
        frame_system::Pallet::<T>::set_block_number(
            before.last_accrued.saturating_add(T::InterestPeriod::get()),
        );
        #[extrinsic_call]
        open(
            RawOrigin::Signed(owner.clone()),
            netuid,
            Side::Short,
            16_000_000_000,
            hotkey,
            1,
            0,
        );
        let after = Positions::<T>::get(owner, netuid).unwrap();
        assert!(after.principal > before.principal);
        assert_eq!(after.due, before.due);
        assert_eq!(PositionCount::<T>::get(netuid), 1);
    }

    #[benchmark(extra)]
    fn increase_long() {
        let (owner, hotkey, netuid) = setup::<T>();
        // Physically fund enough headroom for two loans in the real and mock pools.
        let vault = Pallet::<T>::reserve_account(netuid);
        T::Pool::transfer_tao(&owner, &vault, 400_000_000_000_u64.into()).unwrap();
        Pallet::<T>::fund_reserves(
            netuid,
            400_000_000_000_u64.into(),
            AlphaBalance::ZERO,
            T::Pool::current_alpha_price(netuid),
            true,
        )
        .unwrap();
        let collateral = T::Pool::quote_buy(netuid, 40_000_000_000_u64.into())
            .unwrap()
            .to_u64();
        Pallet::<T>::open(
            RawOrigin::Signed(owner.clone()).into(),
            netuid,
            Side::Long,
            collateral,
            hotkey.clone(),
            1,
            0,
        )
        .unwrap();
        let before = Positions::<T>::get(&owner, netuid).unwrap();
        let now = before.last_accrued.saturating_add(T::InterestPeriod::get());
        frame_system::Pallet::<T>::set_block_number(now);
        // Fill the bounded admission scan, retaining the growing owner's entry.
        for i in 1..T::MaxPositionsPerSubnet::get() {
            let other: T::AccountId = account("increase-existing-long", i, 0);
            Positions::<T>::insert(
                &other,
                netuid,
                Position {
                    side: Side::Long,
                    hotkey: hotkey.clone(),
                    principal: 1,
                    collateral: 1_000_000_000,
                    proceeds: 0,
                    annual_interest: 1,
                    last_accrued: now,
                    interest_remainder: 1,
                    due: now.saturating_add(T::InterestPeriod::get()),
                },
            );
            OpenByNetuid::<T>::insert(netuid, other, ());
        }
        #[extrinsic_call]
        open(
            RawOrigin::Signed(owner.clone()),
            netuid,
            Side::Long,
            collateral,
            hotkey,
            1,
            0,
        );
        let after = Positions::<T>::get(owner, netuid).unwrap();
        assert!(after.principal > before.principal);
        assert_eq!(after.due, before.due);
        assert_eq!(PositionCount::<T>::get(netuid), 1);
    }

    // Measure the independent long-admission scan in addition to the short's
    // bounded swap quotes. The runtime conservatively composes both envelopes.
    #[benchmark(extra)]
    fn funded_long_admission() {
        let (_, hotkey, netuid) = setup::<T>();
        let now = frame_system::Pallet::<T>::block_number();
        for i in 0..T::MaxPositionsPerSubnet::get() {
            let owner: T::AccountId = account("existing-long", i, 0);
            // Populate every record read by the scan; valuation uses its stored
            // debt and collateral, without a transfer or swap during admission.
            Positions::<T>::insert(
                &owner,
                netuid,
                Position {
                    side: Side::Long,
                    hotkey: hotkey.clone(),
                    principal: 1,
                    collateral: 1_000_000_000,
                    proceeds: 0,
                    annual_interest: 1,
                    last_accrued: now,
                    interest_remainder: 1,
                    due: now.saturating_add(T::InterestPeriod::get()),
                },
            );
            OpenByNetuid::<T>::insert(netuid, owner, ());
        }
        let backing = Vaults::<T>::get(netuid).unwrap().available_tao;
        #[block]
        {
            assert!(Pallet::<T>::funded_long_limit(netuid, 1_000_000_000, backing).unwrap() > 0);
        }
    }

    #[benchmark]
    fn close() {
        let (owner, hotkey, netuid) = setup::<T>();
        Pallet::<T>::open(
            RawOrigin::Signed(owner.clone()).into(),
            netuid,
            Side::Short,
            16_000_000_000,
            hotkey,
            1,
            0,
        )
        .unwrap();
        frame_system::Pallet::<T>::set_block_number(
            frame_system::Pallet::<T>::block_number().saturating_add(T::InterestPeriod::get()),
        );
        #[extrinsic_call]
        _(RawOrigin::Signed(owner.clone()), netuid, false, u64::MAX, 0);
        assert!(!Positions::<T>::contains_key(owner, netuid));
    }

    #[benchmark]
    fn set_enabled() {
        #[extrinsic_call]
        _(RawOrigin::Root, true);
        assert!(Enabled::<T>::get());
    }

    #[benchmark]
    fn collect() {
        let (owner, hotkey, netuid) = setup::<T>();
        let collateral = T::Pool::quote_buy(netuid, 64_000_000_000_u64.into())
            .unwrap()
            .to_u64();
        Pallet::<T>::open(
            RawOrigin::Signed(owner.clone()).into(),
            netuid,
            Side::Long,
            collateral,
            hotkey,
            1,
            0,
        )
        .unwrap();
        let now =
            frame_system::Pallet::<T>::block_number().saturating_add(T::InterestPeriod::get());
        frame_system::Pallet::<T>::set_block_number(now);
        let position = Positions::<T>::get(&owner, netuid).unwrap();
        let (fee, _) = Pallet::<T>::interest_due(&position, now, false).unwrap();
        let fee = fee.min(position.collateral);
        let expected_burn = T::Pool::quote_sell(netuid, fee.into()).unwrap().to_u64();
        assert!(expected_burn > 0);
        #[block]
        {
            let mut position = Positions::<T>::get(&owner, netuid).unwrap();
            Pallet::<T>::charge_interest(&owner, netuid, &mut position, now, false).unwrap();
            Positions::<T>::insert(&owner, netuid, position);
            let mut meter = WeightMeter::with_limit(Weight::MAX);
            Pallet::<T>::convert_pending(now, &mut meter);
        }
        assert_eq!(Vaults::<T>::get(netuid).unwrap().pending_alpha, 0);
        let event: <T as frame_system::Config>::RuntimeEvent = Event::<T>::InterestBurned {
            netuid,
            side: Side::Long,
            tao: expected_burn,
        }
        .into();
        frame_system::Pallet::<T>::assert_last_event(event);
    }

    #[benchmark]
    fn update_reference() {
        let (_, _, netuid) = setup::<T>();
        let now = frame_system::Pallet::<T>::block_number().saturating_add(1_u32.into());
        #[block]
        {
            <Pallet<T> as Hooks<BlockNumberFor<T>>>::on_finalize(now);
        }
        assert_eq!(References::<T>::get(netuid).unwrap().last_updated, now);
    }

    #[benchmark]
    fn settle() {
        let (owner, hotkey, netuid) = setup::<T>();
        Pallet::<T>::open(
            RawOrigin::Signed(owner.clone()).into(),
            netuid,
            Side::Short,
            16_000_000_000,
            hotkey,
            1,
            0,
        )
        .unwrap();
        frame_system::Pallet::<T>::set_block_number(
            frame_system::Pallet::<T>::block_number().saturating_add(T::InterestPeriod::get()),
        );
        Pallet::<T>::start_dissolution(netuid).unwrap();
        let position = Positions::<T>::get(&owner, netuid).unwrap();
        let dissolution = Dissolutions::<T>::get(netuid).unwrap();
        #[block]
        {
            Pallet::<T>::settle_short(netuid, &owner, position, &dissolution).unwrap();
            Pallet::<T>::return_terminal_reserves(netuid).unwrap();
        }
        assert!(!Positions::<T>::contains_key(owner, netuid));
        let vault = Vaults::<T>::get(netuid).unwrap();
        assert_eq!(vault.available_tao, 0);
        assert_eq!(vault.available_alpha, 0);
        assert_eq!(vault.pending_tao, 0);
        assert!(Dissolutions::<T>::get(netuid).unwrap().reserves_returned);
    }

    impl_benchmark_test_suite!(Pallet, crate::tests::ext(), crate::tests::Test);
}
