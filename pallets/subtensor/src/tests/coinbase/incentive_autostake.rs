#![allow(
    unused,
    clippy::arithmetic_side_effects,
    clippy::indexing_slicing,
    clippy::panic,
    clippy::unwrap_used,
    clippy::expect_used
)]
//! Incentive autostake destination vs hotkey fallback.

use super::helpers::*;
use super::prelude::*;

#[test]
fn test_incentive_is_autostaked_to_owner_destination() {
    new_test_ext(1).execute_with(|| {
        let subnet_owner_ck = U256::from(0);
        let subnet_owner_hk = U256::from(1);

        let miner_ck = U256::from(10);
        let miner_hk = U256::from(11);
        let dest_hk = U256::from(12);

        Owner::<Test>::insert(miner_hk, miner_ck);
        Owner::<Test>::insert(dest_hk, miner_ck);
        OwnedHotkeys::<Test>::insert(miner_ck, vec![miner_hk, dest_hk]);

        let netuid = add_dynamic_network(&subnet_owner_hk, &subnet_owner_ck);

        Uids::<Test>::insert(netuid, miner_hk, 1);
        Uids::<Test>::insert(netuid, dest_hk, 2);

        // Set autostake destination for the miner's coldkey
        assert_ok!(SubtensorModule::set_coldkey_auto_stake_hotkey(
            RuntimeOrigin::signed(miner_ck),
            netuid,
            dest_hk,
        ));

        assert_eq!(
            SubtensorModule::get_stake_for_hotkey_on_subnet(&miner_hk, netuid),
            0.into()
        );
        assert_eq!(
            SubtensorModule::get_stake_for_hotkey_on_subnet(&dest_hk, netuid),
            0.into()
        );

        // Distribute an incentive to the miner hotkey
        let mut incentives: BTreeMap<U256, AlphaBalance> = BTreeMap::new();
        let incentive: AlphaBalance = 10_000_000u64.into();
        incentives.insert(miner_hk, incentive);

        SubtensorModule::distribute_dividends_and_incentives(
            netuid,
            AlphaBalance::ZERO, // owner_cut
            incentives,
            BTreeMap::new(), // alpha_dividends
            BTreeMap::new(), // tao_dividends
        );

        // Expect the stake to land on the destination hotkey (not the original miner hotkey)
        assert_eq!(
            SubtensorModule::get_stake_for_hotkey_on_subnet(&miner_hk, netuid),
            0.into()
        );
        assert_eq!(
            SubtensorModule::get_stake_for_hotkey_on_subnet(&dest_hk, netuid),
            incentive
        );
    });
}

#[test]
fn test_incentive_goes_to_hotkey_when_no_autostake_destination() {
    new_test_ext(1).execute_with(|| {
        let subnet_owner_ck = U256::from(0);
        let subnet_owner_hk = U256::from(1);

        let miner_ck = U256::from(20);
        let miner_hk = U256::from(21);

        Owner::<Test>::insert(miner_hk, miner_ck);
        OwnedHotkeys::<Test>::insert(miner_ck, vec![miner_hk]);

        let netuid = add_dynamic_network(&subnet_owner_hk, &subnet_owner_ck);

        Uids::<Test>::insert(netuid, miner_hk, 1);

        assert_eq!(
            SubtensorModule::get_stake_for_hotkey_on_subnet(&miner_hk, netuid),
            0.into()
        );

        // Distribute an incentive to the miner hotkey
        let mut incentives: BTreeMap<U256, AlphaBalance> = BTreeMap::new();
        let incentive: AlphaBalance = 5_000_000u64.into();
        incentives.insert(miner_hk, incentive);

        SubtensorModule::distribute_dividends_and_incentives(
            netuid,
            AlphaBalance::ZERO, // owner_cut
            incentives,
            BTreeMap::new(), // alpha_dividends
            BTreeMap::new(), // tao_dividends
        );

        // With no autostake destination, the incentive should be staked to the original hotkey
        assert_eq!(
            SubtensorModule::get_stake_for_hotkey_on_subnet(&miner_hk, netuid),
            incentive
        );
    });
}
