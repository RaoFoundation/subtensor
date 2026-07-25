#![allow(unused, clippy::indexing_slicing, clippy::panic, clippy::unwrap_used)]

use approx::assert_abs_diff_eq;
use codec::Encode;
use frame_support::weights::Weight;
use frame_support::{assert_err, assert_noop, assert_ok};
use frame_system::{Config, RawOrigin};
use subtensor_runtime_common::{AlphaBalance, NetUidStorageIndex, TaoBalance, Token};

use super::super::mock::*;
use crate::*;
use share_pool::SafeFloat;
use sp_core::{Get, H160, H256, U256};
use sp_runtime::{PerU16, SaturatedConversion};
use std::collections::BTreeSet;
use substrate_fixed::types::{I96F32, U64F64};

// SKIP_WASM_BUILD=1 RUST_LOG=debug cargo test --package pallet-subtensor --lib -- tests::swap_hotkey_with_subnet::test_swap_hotkey_root_claims_unchanged_if_not_root --exact --nocapture
#[test]
fn test_swap_hotkey_root_claims_unchanged_if_not_root() {
    new_test_ext(1).execute_with(|| {
        let owner_coldkey = U256::from(1001);
        let neuron_hotkey = U256::from(1002);
        let staker_coldkey = U256::from(1003);
        let netuid = add_dynamic_network(&neuron_hotkey, &owner_coldkey);
        let new_hotkey = U256::from(10030);

        add_balance_to_coldkey_account(&owner_coldkey, 20_000_000_000_000_000_u64.into());
        SubtensorModule::set_tao_weight(u64::MAX); // Set TAO weight to 1.0

        let root_stake = 2_000_000_000u64;
        SubtensorModule::increase_stake_for_hotkey_and_coldkey_on_subnet(
            &neuron_hotkey,
            &staker_coldkey,
            NetUid::ROOT,
            root_stake.into(),
        );

        let initial_total_hotkey_alpha = 10_000_000_000u64;
        SubtensorModule::increase_stake_for_hotkey_and_coldkey_on_subnet(
            &neuron_hotkey,
            &staker_coldkey,
            netuid,
            initial_total_hotkey_alpha.into(),
        );

        let validator_stake = SubtensorModule::get_stake_for_hotkey_and_coldkey_on_subnet(
            &neuron_hotkey,
            &staker_coldkey,
            netuid,
        );
        assert_eq!(validator_stake, initial_total_hotkey_alpha.into());

        // Distribute pending root alpha
        let pending_root_alpha = 1_000_000_000u64;
        SubtensorModule::distribute_emission(
            netuid,
            AlphaBalance::ZERO,
            AlphaBalance::ZERO,
            pending_root_alpha.into(),
            AlphaBalance::ZERO,
        );

        assert_ok!(SubtensorModule::claim_root(
            RuntimeOrigin::signed(staker_coldkey),
            BTreeSet::from([netuid])
        ));

        let claimable = RootClaimable::<Test>::get(neuron_hotkey)
            .get(&netuid)
            .copied();

        assert!(claimable.is_some());
        let claimable = claimable.unwrap_or_default();

        assert!(claimable > 0);

        assert!(RootClaimed::<Test>::get((netuid, &neuron_hotkey, &staker_coldkey,)) > 0u128);

        step_block(20);
        assert_ok!(SubtensorModule::perform_hotkey_swap(
            RuntimeOrigin::signed(owner_coldkey),
            &neuron_hotkey,
            &new_hotkey,
            Some(netuid),
            false
        ));

        // Claimable and claimed should stay on old hotkey
        assert_eq!(
            RootClaimable::<Test>::get(neuron_hotkey)
                .get(&netuid)
                .copied(),
            Some(claimable)
        );
        assert!(RootClaimed::<Test>::get((netuid, &neuron_hotkey, &staker_coldkey,)) > 0u128);
    });
}

// SKIP_WASM_BUILD=1 RUST_LOG=debug cargo test --package pallet-subtensor --lib -- tests::swap_hotkey_with_subnet::test_swap_hotkey_root_claims_changed_if_root --exact --nocapture
#[test]
fn test_swap_hotkey_root_claims_changed_if_root() {
    new_test_ext(1).execute_with(|| {
        let owner_coldkey = U256::from(1001);

        let neuron_hotkey = U256::from(1004);
        let neuron_hotkey_new = U256::from(1005);

        let staker_coldkey = U256::from(1006);

        NetworksAdded::<Test>::insert(NetUid::ROOT, true);

        // Use neuron_hotkey as subnet creator so it receives root dividends
        let netuid_1 = add_dynamic_network(&neuron_hotkey, &owner_coldkey);

        add_balance_to_coldkey_account(&owner_coldkey, 20_000_000_000_000_000_u64.into());
        SubtensorModule::set_tao_weight(u64::MAX); // Set TAO weight to 1.0

        let root_stake = 2_000_000_000u64;
        SubtensorModule::increase_stake_for_hotkey_and_coldkey_on_subnet(
            &neuron_hotkey,
            &staker_coldkey,
            NetUid::ROOT,
            root_stake.into(),
        );

        let initial_total_hotkey_alpha = 10_000_000_000u64;
        SubtensorModule::increase_stake_for_hotkey_and_coldkey_on_subnet(
            &neuron_hotkey,
            &owner_coldkey,
            netuid_1,
            initial_total_hotkey_alpha.into(),
        );

        // Distribute pending root alpha
        let pending_root_alpha = 1_000_000_000u64;
        SubtensorModule::distribute_emission(
            netuid_1,
            AlphaBalance::ZERO,
            AlphaBalance::ZERO,
            pending_root_alpha.into(),
            AlphaBalance::ZERO,
        );

        assert_ok!(SubtensorModule::set_root_claim_type(
            RuntimeOrigin::signed(staker_coldkey),
            RootClaimTypeEnum::Keep
        ));
        assert_ok!(SubtensorModule::claim_root(
            RuntimeOrigin::signed(staker_coldkey),
            BTreeSet::from([netuid_1])
        ));

        let claimable = RootClaimable::<Test>::get(neuron_hotkey)
            .get(&netuid_1)
            .copied();
        assert!(claimable.is_some());
        let claimable = claimable.unwrap_or_default();

        assert!(claimable > 0);

        let claimed = RootClaimed::<Test>::get((netuid_1, &neuron_hotkey, &staker_coldkey));
        assert!(claimed > 0u128);

        step_block(20);
        assert_ok!(SubtensorModule::perform_hotkey_swap(
            RuntimeOrigin::signed(owner_coldkey),
            &neuron_hotkey,
            &neuron_hotkey_new,
            Some(NetUid::ROOT),
            false
        ));

        // Claimable and claimed should be transferred to new hotkey
        assert_eq!(
            RootClaimable::<Test>::get(neuron_hotkey_new)
                .get(&netuid_1)
                .copied(),
            Some(claimable)
        );
        assert_eq!(
            RootClaimed::<Test>::get((netuid_1, &neuron_hotkey_new, &staker_coldkey,)),
            claimed
        );
    });
}

// SKIP_WASM_BUILD=1 RUST_LOG=debug cargo test --package pallet-subtensor --lib -- tests::swap_hotkey_with_subnet::test_swap_hotkey_root_claims_changed_if_all_subnets --exact --nocapture
#[test]
fn test_swap_hotkey_root_claims_changed_if_all_subnets() {
    new_test_ext(1).execute_with(|| {
        let owner_coldkey = U256::from(1001);
        let neuron_hotkey = U256::from(1004);
        let neuron_hotkey_new = U256::from(1005);

        let staker_coldkey = U256::from(1006);

        // Ensure ROOT network is registered for all-subnets swap
        SubtokenEnabled::<Test>::insert(NetUid::ROOT, true);
        NetworksAdded::<Test>::insert(NetUid::ROOT, true);

        // Use neuron_hotkey as subnet creator so it receives root dividends
        let netuid_1 = add_dynamic_network(&neuron_hotkey, &owner_coldkey);

        add_balance_to_coldkey_account(&owner_coldkey, 20_000_000_000_000_000_u64.into());
        SubtensorModule::set_tao_weight(u64::MAX); // Set TAO weight to 1.0

        let root_stake = 2_000_000_000u64;
        SubtensorModule::increase_stake_for_hotkey_and_coldkey_on_subnet(
            &neuron_hotkey,
            &staker_coldkey,
            NetUid::ROOT,
            root_stake.into(),
        );

        let initial_total_hotkey_alpha = 10_000_000_000u64;
        SubtensorModule::increase_stake_for_hotkey_and_coldkey_on_subnet(
            &neuron_hotkey,
            &owner_coldkey,
            netuid_1,
            initial_total_hotkey_alpha.into(),
        );

        // Distribute pending root alpha
        let pending_root_alpha = 1_000_000_000u64;
        SubtensorModule::distribute_emission(
            netuid_1,
            AlphaBalance::ZERO,
            AlphaBalance::ZERO,
            pending_root_alpha.into(),
            AlphaBalance::ZERO,
        );

        assert_ok!(SubtensorModule::set_root_claim_type(
            RuntimeOrigin::signed(staker_coldkey),
            RootClaimTypeEnum::Keep
        ));
        assert_ok!(SubtensorModule::claim_root(
            RuntimeOrigin::signed(staker_coldkey),
            BTreeSet::from([netuid_1])
        ));

        let claimable = RootClaimable::<Test>::get(neuron_hotkey)
            .get(&netuid_1)
            .copied();
        assert!(claimable.is_some());
        let claimable = claimable.unwrap_or_default();

        assert!(claimable > 0);

        let claimed = RootClaimed::<Test>::get((netuid_1, &neuron_hotkey, &staker_coldkey));
        assert!(claimed > 0u128);

        step_block(20);
        assert_ok!(SubtensorModule::perform_hotkey_swap(
            RuntimeOrigin::signed(owner_coldkey),
            &neuron_hotkey,
            &neuron_hotkey_new,
            None,
            false
        ));

        // Claimable and claimed should be transferred to new hotkey
        assert_eq!(
            RootClaimable::<Test>::get(neuron_hotkey_new)
                .get(&netuid_1)
                .copied(),
            Some(claimable)
        );
        assert_eq!(
            RootClaimed::<Test>::get((netuid_1, &neuron_hotkey_new, &staker_coldkey,)),
            claimed
        );
    });
}
