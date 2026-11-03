#![allow(clippy::unwrap_used)]

use frame_support::assert_ok;
use sp_core::U256;
use substrate_fixed::types::I96F32;
use subtensor_runtime_common::{AlphaBalance, TaoBalance};

use super::mock::*;
use crate::*;

#[test]
fn collateral_purchase_locks_exactly_the_requested_alpha() {
    new_test_ext(1).execute_with(|| {
        let owner_coldkey = U256::from(1001);
        let owner_hotkey = U256::from(1002);
        let coldkey = U256::from(1003);
        let hotkey = U256::from(1004);
        let netuid = add_dynamic_network(&owner_hotkey, &owner_coldkey);
        assert_ok!(SubtensorModule::create_account_if_non_existent(
            &coldkey, &hotkey
        ));
        setup_reserves(
            netuid,
            TaoBalance::from(1_000_000_000_000u64),
            AlphaBalance::from(2_000_000_000_000u64),
        );
        SubnetMovingPrice::<Test>::insert(netuid, I96F32::from_num(1));

        let requested = AlphaBalance::from(100_000_000_000u64);
        add_balance_to_coldkey_account(&coldkey, 200_000_000_000u64.into());
        assert_ok!(SubtensorModule::add_collateral(
            RuntimeOrigin::signed(coldkey),
            netuid,
            hotkey,
            requested,
            TaoBalance::MAX,
        ));

        let stake =
            SubtensorModule::get_stake_for_hotkey_and_coldkey_on_subnet(&hotkey, &coldkey, netuid);
        assert!(
            stake > requested,
            "stale quote should overbuy at the live spot price"
        );
        assert_eq!(
            SubtensorModule::get_miner_collateral_locked(netuid, &hotkey, &coldkey),
            requested,
        );
        assert_eq!(
            ColdkeyMinerCollateral::<Test>::get(netuid, coldkey),
            requested
        );
        assert_eq!(
            SubtensorModule::available_to_unstake_from_hotkey(&coldkey, &hotkey, netuid),
            stake.saturating_sub(requested),
        );
    });
}

#[test]
fn collateral_purchase_at_fair_price_locks_the_live_fill() {
    new_test_ext(1).execute_with(|| {
        let owner_coldkey = U256::from(2001);
        let owner_hotkey = U256::from(2002);
        let coldkey = U256::from(2003);
        let hotkey = U256::from(2004);
        let netuid = add_dynamic_network(&owner_hotkey, &owner_coldkey);
        assert_ok!(SubtensorModule::create_account_if_non_existent(
            &coldkey, &hotkey
        ));
        setup_reserves(
            netuid,
            TaoBalance::from(1_000_000_000_000u64),
            AlphaBalance::from(2_000_000_000_000u64),
        );
        SubnetMovingPrice::<Test>::insert(netuid, I96F32::from_num(0.5));

        let requested = AlphaBalance::from(100_000_000u64);
        add_balance_to_coldkey_account(&coldkey, 1_000_000_000u64.into());
        assert_ok!(SubtensorModule::add_collateral(
            RuntimeOrigin::signed(coldkey),
            netuid,
            hotkey,
            requested,
            TaoBalance::MAX,
        ));

        let stake =
            SubtensorModule::get_stake_for_hotkey_and_coldkey_on_subnet(&hotkey, &coldkey, netuid);
        let locked = SubtensorModule::get_miner_collateral_locked(netuid, &hotkey, &coldkey);
        assert!(!locked.is_zero());
        assert!(locked < requested, "fees and price impact should underfill");
        assert_eq!(locked, stake);
        assert_eq!(ColdkeyMinerCollateral::<Test>::get(netuid, coldkey), locked);
    });
}

#[test]
fn collateral_top_up_uses_free_stake_without_buying_tao() {
    new_test_ext(1).execute_with(|| {
        let owner_coldkey = U256::from(3001);
        let owner_hotkey = U256::from(3002);
        let coldkey = U256::from(3003);
        let hotkey = U256::from(3004);
        let netuid = add_dynamic_network(&owner_hotkey, &owner_coldkey);
        assert_ok!(SubtensorModule::create_account_if_non_existent(
            &coldkey, &hotkey
        ));
        let available = AlphaBalance::from(200_000_000u64);
        let requested = AlphaBalance::from(100_000_000u64);
        SubtensorModule::increase_stake_for_hotkey_and_coldkey_on_subnet(
            &hotkey, &coldkey, netuid, available,
        );

        assert_ok!(SubtensorModule::add_collateral(
            RuntimeOrigin::signed(coldkey),
            netuid,
            hotkey,
            requested,
            TaoBalance::MAX,
        ));

        assert_eq!(
            SubtensorModule::get_miner_collateral_locked(netuid, &hotkey, &coldkey),
            requested,
        );
        assert_eq!(
            SubtensorModule::available_to_unstake_from_hotkey(&coldkey, &hotkey, netuid),
            available.saturating_sub(requested),
        );
    });
}
