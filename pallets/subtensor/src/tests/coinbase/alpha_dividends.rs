#![allow(
    unused,
    clippy::arithmetic_side_effects,
    clippy::indexing_slicing,
    clippy::panic,
    clippy::unwrap_used,
    clippy::expect_used
)]
//! Alpha dividend collateral release and take-floor capture.

use super::helpers::*;
use super::prelude::*;

// SKIP_WASM_BUILD=1 cargo test -p pallet-subtensor --lib alpha_dividends -- --nocapture
#[test]
fn test_alpha_dividends_release_collateral_on_full_emission() {
    new_test_ext(1).execute_with(|| {
        let owner_ck = U256::from(10);
        let owner_hk = U256::from(11);
        let sn_owner_ck = U256::from(0);
        let sn_owner_hk = U256::from(1);

        Owner::<Test>::insert(owner_hk, owner_ck);
        OwnedHotkeys::<Test>::insert(owner_ck, vec![owner_hk]);

        let netuid = add_dynamic_network(&sn_owner_hk, &sn_owner_ck);
        Uids::<Test>::insert(netuid, owner_hk, 1);
        Delegates::<Test>::insert(owner_hk, PerU16::from_parts(u16::MAX / 2));

        let locked: AlphaBalance = 5_000_000u64.into();
        MinerCollateral::<Test>::insert(
            (netuid, owner_hk, owner_ck),
            MinerCollateralState {
                locked,
                drain_ratio: U64F64::from_num(1),
                min_locked: AlphaBalance::ZERO,
                earned: AlphaBalance::ZERO,
            },
        );
        ColdkeyMinerCollateral::<Test>::insert(netuid, owner_ck, locked);
        SubtensorModule::increase_stake_for_hotkey_and_coldkey_on_subnet(
            &owner_hk,
            &owner_ck,
            netuid,
            1_000_000u64.into(),
        );

        let dividend: U96F32 = U96F32::from_num(1_000_000u64);
        let mut alpha_dividends: BTreeMap<U256, U96F32> = BTreeMap::new();
        alpha_dividends.insert(owner_hk, dividend);

        SubtensorModule::distribute_dividends_and_incentives(
            netuid,
            AlphaBalance::ZERO,
            BTreeMap::new(),
            alpha_dividends,
            BTreeMap::new(),
        );

        let alpha_take = SubtensorModule::get_hotkey_take_float(&owner_hk).saturating_mul(dividend);
        let nominator: AlphaBalance = dividend
            .saturating_sub(alpha_take)
            .saturating_to_num::<u64>()
            .into();
        let state =
            MinerCollateral::<Test>::get((netuid, owner_hk, owner_ck)).expect("still locked");
        assert_eq!(state.locked, locked.saturating_sub(1_000_000u64.into()));
        assert_eq!(state.earned, 1_000_000u64.into());
        assert_eq!(
            AlphaDividendsPerSubnet::<Test>::get(netuid, owner_hk),
            nominator
        );
    });
}

#[test]
fn test_alpha_dividends_floor_capture_only_from_take() {
    new_test_ext(1).execute_with(|| {
        let owner_ck = U256::from(20);
        let owner_hk = U256::from(21);
        let sn_owner_ck = U256::from(0);
        let sn_owner_hk = U256::from(1);

        Owner::<Test>::insert(owner_hk, owner_ck);
        OwnedHotkeys::<Test>::insert(owner_ck, vec![owner_hk]);

        let netuid = add_dynamic_network(&sn_owner_hk, &sn_owner_ck);
        Uids::<Test>::insert(netuid, owner_hk, 1);
        Delegates::<Test>::insert(owner_hk, PerU16::from_parts(u16::MAX / 2));

        let locked: AlphaBalance = 100u64.into();
        let min_locked: AlphaBalance = 10_000_000u64.into();
        MinerCollateral::<Test>::insert(
            (netuid, owner_hk, owner_ck),
            MinerCollateralState {
                locked,
                drain_ratio: U64F64::from_num(1),
                min_locked,
                earned: AlphaBalance::ZERO,
            },
        );
        ColdkeyMinerCollateral::<Test>::insert(netuid, owner_ck, locked);
        SubtensorModule::increase_stake_for_hotkey_and_coldkey_on_subnet(
            &owner_hk,
            &owner_ck,
            netuid,
            1_000_000u64.into(),
        );
        let owner_stake_before = SubtensorModule::get_stake_for_hotkey_and_coldkey_on_subnet(
            &owner_hk, &owner_ck, netuid,
        );

        let dividend: U96F32 = U96F32::from_num(1_000_000u64);
        let alpha_take = SubtensorModule::get_hotkey_take_float(&owner_hk).saturating_mul(dividend);
        let take: AlphaBalance = alpha_take.saturating_to_num::<u64>().into();
        let nominator: AlphaBalance = dividend
            .saturating_sub(alpha_take)
            .saturating_to_num::<u64>()
            .into();
        let mut alpha_dividends: BTreeMap<U256, U96F32> = BTreeMap::new();
        alpha_dividends.insert(owner_hk, dividend);

        SubtensorModule::distribute_dividends_and_incentives(
            netuid,
            AlphaBalance::ZERO,
            BTreeMap::new(),
            alpha_dividends,
            BTreeMap::new(),
        );

        let state =
            MinerCollateral::<Test>::get((netuid, owner_hk, owner_ck)).expect("still locked");
        assert_eq!(state.locked, locked.saturating_add(take));
        assert_eq!(state.earned, 1_000_000u64.into());
        assert_eq!(
            AlphaDividendsPerSubnet::<Test>::get(netuid, owner_hk),
            nominator
        );

        let owner_stake_after = SubtensorModule::get_stake_for_hotkey_and_coldkey_on_subnet(
            &owner_hk, &owner_ck, netuid,
        );
        let gained = owner_stake_after.saturating_sub(owner_stake_before);
        assert!(
            gained >= 999_999u64.into() && gained <= 1_000_000u64.into(),
            "unexpected owner stake gain: {gained:?}"
        );
    });
}
