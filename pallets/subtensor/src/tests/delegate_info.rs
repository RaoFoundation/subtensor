#![allow(clippy::expect_used)]
use super::mock::*;

use codec::Compact;
use frame_support::assert_ok;
use scale_info::prelude::collections::HashMap;
use sp_core::U256;
use substrate_fixed::types::U64F64;
use subtensor_runtime_common::NetUid;

#[test]
fn test_return_per_1000_tao() {
    let take = // 18% take to the Validator
        Compact::<u16>::from((U64F64::from_num(0.18 * u16::MAX as f64)).to_num::<u16>());

    // 10_000 TAO total validator stake
    let total_stake = U64F64::from_num(10_000.0 * 1e9);
    // 1000 TAO emissions per day
    let emissions_per_day = U64F64::from_num(1000.0 * 1e9);

    let return_per_1000 =
        SubtensorModule::return_per_1000_tao_test(take, total_stake, emissions_per_day);

    // We expect 82 TAO per day with 10% of total_stake
    let expected_return_per_1000 = U64F64::from_num(82.0);

    let diff_from_expected: f64 =
        ((return_per_1000 / U64F64::from_num(1e9)) - expected_return_per_1000).to_num::<f64>();

    let eps: f64 = 0.0005e9; // Precision within 0.0005 TAO
    assert!(
        diff_from_expected.abs() <= eps,
        "Difference from expected: {diff_from_expected} is greater than precision: {eps}"
    );
}

#[test]
fn test_get_delegated() {
    new_test_ext(1).execute_with(|| {
        let sn_owner_0 = U256::from(0);
        let sn_owner_1 = U256::from(1);

        // Delegates
        let owner_0 = U256::from(100);
        let owner_1 = U256::from(1 + 100);
        let delegate_0 = U256::from(200);
        let delegate_1 = U256::from(1 + 200);

        // Create 2 networks
        let netuid_0 = add_dynamic_network(&sn_owner_0, &sn_owner_0);
        let netuid_1 = add_dynamic_network(&sn_owner_1, &sn_owner_1);

        // Create delegate hotkey 0 on both networks
        register_ok_neuron(netuid_0, delegate_0, owner_0, 0);
        register_ok_neuron(netuid_1, delegate_0, owner_0, 1);

        // Create delegate hotkey 1 on both networks
        register_ok_neuron(netuid_0, delegate_1, owner_1, 2);
        register_ok_neuron(netuid_1, delegate_1, owner_1, 3);

        // Stake to both hotkeys on both networks with delegatee_0
        let delegatee_0 = U256::from(300);
        let to_stake_0 = vec![
            (netuid_0, Some(delegate_0), 1_000_000_000),
            (netuid_1, Some(delegate_0), 2_000_000_000),
            (netuid_0, Some(delegate_1), 1_000_000_000),
            (netuid_1, Some(delegate_1), 2_000_000_000),
        ];

        // Stake to both hotkeys on only one network with delegatee_1
        let delegatee_1 = U256::from(1 + 300);
        let to_stake_1 = vec![
            (netuid_0, Some(delegate_0), 1_000_000_000),
            (netuid_0, Some(delegate_1), 2_000_000_000),
        ];

        // Stake to both hotkey on either network with delegatee_2
        let delegatee_2 = U256::from(2 + 300);
        let to_stake_2 = vec![
            (netuid_0, Some(delegate_0), 1_000_000_000),
            (netuid_0, None, 0),
            (netuid_1, None, 0),
            (netuid_1, Some(delegate_1), 2_000_000_000),
        ];

        // Stake to one hotkey on one network with delegatee_3
        let delegatee_3 = U256::from(3 + 300);
        let to_stake_3 = vec![
            (netuid_0, Some(delegate_0), 1_000_000_000),
            (netuid_0, None, 0),
            (netuid_1, None, 0),
            (netuid_1, None, 0),
        ];

        // Stake to no hotkeys with delegatee_4
        let delegatee_4 = U256::from(4 + 300);
        let to_stake_4 = vec![
            (netuid_0, None, 0),
            (netuid_0, None, 0),
            (netuid_1, None, 0),
            (netuid_1, None, 0),
        ];

        // Run staking for each delegatee
        let coldkeys = vec![
            delegatee_0,
            delegatee_1,
            delegatee_2,
            delegatee_3,
            delegatee_4,
        ];
        let to_stakes = [to_stake_0, to_stake_1, to_stake_2, to_stake_3, to_stake_4];
        let mut expected_stake_map: HashMap<U256, HashMap<U256, HashMap<NetUid, u64>>> =
            HashMap::new();

        for (i, to_stake) in to_stakes.iter().enumerate() {
            let delegatee = coldkeys.get(i).expect("Delegatee not found");
            for (netuid, delegate, amount) in to_stake {
                let Some(delegate) = delegate else {
                    continue;
                };
                add_balance_to_coldkey_account(delegatee, (*amount + 500_000).into());
                assert_ok!(SubtensorModule::add_stake(
                    RuntimeOrigin::signed(*delegatee),
                    *delegate,
                    *netuid,
                    (*amount).into()
                ));
                let expected_stake = SubtensorModule::get_stake_for_hotkey_and_coldkey_on_subnet(
                    delegate, delegatee, *netuid,
                );
                let stakes = expected_stake_map
                    .entry(*delegatee)
                    .or_default()
                    .entry(*delegate)
                    .or_default();
                stakes.insert(*netuid, expected_stake.into());
            }
        }

        // Check delegated info for each coldkey
        for coldkey in coldkeys {
            let delegated = SubtensorModule::get_delegated(coldkey);

            for (delegate_info, (netuid, staked)) in delegated.iter() {
                if let Some(coldkey_stakes_map) = expected_stake_map.get(&coldkey) {
                    if let Some(expected_under_delegate) =
                        coldkey_stakes_map.get(&delegate_info.delegate_ss58)
                    {
                        if let Some(expected_stake) = expected_under_delegate.get(&netuid.0) {
                            assert_eq!(u64::from(staked.0), *expected_stake);
                        } else {
                            panic!("Netuid {} not found in expected stake map", netuid.0);
                        };
                    } else {
                        panic!(
                            "Delegate {} not found in expected stake map",
                            delegate_info.delegate_ss58
                        );
                    };
                } else {
                    panic!("Coldkey {coldkey} not found in expected stake map");
                }
            }
        }
    });
}
