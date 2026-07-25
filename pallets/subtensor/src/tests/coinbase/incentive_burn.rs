#![allow(
    unused,
    clippy::arithmetic_side_effects,
    clippy::indexing_slicing,
    clippy::panic,
    clippy::unwrap_used,
    clippy::expect_used
)]
//! Incentive burn to subnet owner / burn-key sorting.

use super::helpers::*;
use super::prelude::*;

// // SKIP_WASM_BUILD=1 RUST_LOG=debug cargo test --package pallet-subtensor --lib -- tests::coinbase::test_get_root_children_with_weights --exact --show-output --nocapture
// #[test]
// fn test_get_root_children_with_weights() {
//     new_test_ext(1).execute_with(|| {
//         // Init netuid 1
//         let alpha = NetUid::from(1);
//         add_network(NetUid::ROOT, 1, 0);
//         add_network(alpha, 1, 0);
//         // Set TAO weight to 1.
//         SubtensorModule::set_tao_weight(u64::MAX); // Set TAO weight to 1.
//                                                    // Create keys.
//         let cold = U256::from(0);
//         let alice = U256::from(1);
//         let bob = U256::from(2);
//         // Register Alice and Bob to the root network and alpha subnet.
//         register_ok_neuron(alpha, alice, cold, 0);
//         register_ok_neuron(alpha, bob, cold, 0);
//         assert_ok!(SubtensorModule::root_register(
//             RuntimeOrigin::signed(cold).clone(),
//             alice,
//         ));
//         assert_ok!(SubtensorModule::root_register(
//             RuntimeOrigin::signed(cold).clone(),
//             bob,
//         ));
//         // Add stake for Alice and Bob on root.
//         let alice_root_stake = AlphaBalance::from(1_000_000_000);
//         SubtensorModule::increase_stake_for_hotkey_and_coldkey_on_subnet(
//             &alice,
//             &cold,
//             NetUid::ROOT,
//             alice_root_stake,
//         );
//         let bob_root_stake = AlphaBalance::from(1_000_000_000);
//         SubtensorModule::increase_stake_for_hotkey_and_coldkey_on_subnet(
//             &bob,
//             &cold,
//             NetUid::ROOT,
//             alice_root_stake,
//         );
//         // Add stake for Alice and Bob on netuid.
//         let alice_alpha_stake = AlphaBalance::from(1_000_000_000);
//         SubtensorModule::increase_stake_for_hotkey_and_coldkey_on_subnet(
//             &alice,
//             &cold,
//             alpha,
//             alice_alpha_stake,
//         );
//         let bob_alpha_stake = AlphaBalance::from(1_000_000_000);
//         SubtensorModule::increase_stake_for_hotkey_and_coldkey_on_subnet(
//             &bob,
//             &cold,
//             alpha,
//             bob_alpha_stake,
//         );
//         // Set Bob as 100% child of Alice on root.
//         mock_set_children_no_epochs(alpha, &alice, &[(u64::MAX, bob)]);

//         // Set Bob childkey take to zero.
//         ChildkeyTake::<Test>::insert(bob, alpha, 0);
//         Delegates::<Test>::insert(alice, 0);
//         Delegates::<Test>::insert(bob, 0);

//         // Set weights on the subnet.
//         assert_ok!(SubtensorModule::set_weights(
//             RuntimeOrigin::signed(alice),
//             alpha,
//             vec![0, 1],
//             vec![1, 1],
//             0,
//         ));
//         assert_ok!(SubtensorModule::set_weights(
//             RuntimeOrigin::signed(bob),
//             alpha,
//             vec![0, 1],
//             vec![1, 1],
//             0,
//         ));

//         // Lets drain!
//         let pending_alpha = AlphaBalance::from(1_000_000_000);
//         SubtensorModule::distribute_emission(alpha, pending_alpha, 0, 0.into(), 0.into());

//         // Alice and Bob make the same amount.
//         close(
//             AlphaDividendsPerSubnet::<Test>::get(alpha, alice),
//             pending_alpha / 2,
//             10,
//         );
//         close(
//             AlphaDividendsPerSubnet::<Test>::get(alpha, bob),
//             pending_alpha / 2,
//             10,
//         );
//     });
// }

// SKIP_WASM_BUILD=1 RUST_LOG=debug cargo test --package pallet-subtensor --lib -- tests::coinbase::incentive_burn::test_incentive_to_subnet_owner_is_burned --exact --show-output --nocapture
#[test]
fn test_incentive_to_subnet_owner_is_burned() {
    new_test_ext(1).execute_with(|| {
        let subnet_owner_ck = U256::from(0);
        let subnet_owner_hk = U256::from(1);

        let other_ck = U256::from(2);
        let other_hk = U256::from(3);
        Owner::<Test>::insert(other_hk, other_ck);

        let netuid = add_dynamic_network(&subnet_owner_hk, &subnet_owner_ck);
        remove_owner_registration_stake(netuid);

        let pending_tao: u64 = 1_000_000_000;
        let pending_alpha = AlphaBalance::ZERO; // None to valis
        let owner_cut = AlphaBalance::ZERO;
        let mut incentives: BTreeMap<U256, AlphaBalance> = BTreeMap::new();

        // Give incentive to other_hk
        incentives.insert(other_hk, 10_000_000.into());

        // Give incentives to subnet_owner_hk
        incentives.insert(subnet_owner_hk, 10_000_000.into());

        // Verify stake before
        let subnet_owner_stake_before =
            SubtensorModule::get_stake_for_hotkey_on_subnet(&subnet_owner_hk, netuid);
        assert_eq!(subnet_owner_stake_before, 0.into());
        let other_stake_before = SubtensorModule::get_stake_for_hotkey_on_subnet(&other_hk, netuid);
        assert_eq!(other_stake_before, 0.into());

        // Distribute dividends and incentives
        SubtensorModule::distribute_dividends_and_incentives(
            netuid,
            owner_cut,
            incentives,
            BTreeMap::new(),
            BTreeMap::new(),
        );

        // Verify stake after
        let subnet_owner_stake_after =
            SubtensorModule::get_stake_for_hotkey_on_subnet(&subnet_owner_hk, netuid);
        assert_eq!(subnet_owner_stake_after, 0.into());
        let other_stake_after = SubtensorModule::get_stake_for_hotkey_on_subnet(&other_hk, netuid);
        assert!(other_stake_after > 0.into());
    });
}

// SKIP_WASM_BUILD=1 RUST_LOG=debug cargo test --package pallet-subtensor --lib -- tests::coinbase::incentive_burn::test_incentive_to_subnet_owners_hotkey_is_burned --exact --show-output --nocapture
#[test]
fn test_incentive_to_subnet_owners_hotkey_is_burned() {
    new_test_ext(1).execute_with(|| {
        let subnet_owner_ck = U256::from(0);
        let subnet_owner_hk = U256::from(1);

        // Other hk owned by owner
        let other_hk = U256::from(3);
        Owner::<Test>::insert(other_hk, subnet_owner_ck);
        OwnedHotkeys::<Test>::insert(subnet_owner_ck, vec![subnet_owner_hk, other_hk]);

        let netuid = add_dynamic_network(&subnet_owner_hk, &subnet_owner_ck);
        remove_owner_registration_stake(netuid);
        Uids::<Test>::insert(netuid, other_hk, 1);

        // Set the burn key limit to 2
        ImmuneOwnerUidsLimit::<Test>::insert(netuid, 2);

        let pending_tao: u64 = 1_000_000_000;
        let pending_alpha = AlphaBalance::ZERO; // None to valis
        let owner_cut = AlphaBalance::ZERO;
        let mut incentives: BTreeMap<U256, AlphaBalance> = BTreeMap::new();

        // Give incentive to other_hk
        incentives.insert(other_hk, 10_000_000.into());

        // Give incentives to subnet_owner_hk
        incentives.insert(subnet_owner_hk, 10_000_000.into());

        // Verify stake before
        let subnet_owner_stake_before =
            SubtensorModule::get_stake_for_hotkey_on_subnet(&subnet_owner_hk, netuid);
        assert_eq!(subnet_owner_stake_before, 0.into());
        let other_stake_before = SubtensorModule::get_stake_for_hotkey_on_subnet(&other_hk, netuid);
        assert_eq!(other_stake_before, 0.into());

        // Distribute dividends and incentives
        SubtensorModule::distribute_dividends_and_incentives(
            netuid,
            owner_cut,
            incentives,
            BTreeMap::new(),
            BTreeMap::new(),
        );

        // Verify stake after
        let subnet_owner_stake_after =
            SubtensorModule::get_stake_for_hotkey_on_subnet(&subnet_owner_hk, netuid);
        assert_eq!(subnet_owner_stake_after, 0.into());
        let other_stake_after = SubtensorModule::get_stake_for_hotkey_on_subnet(&other_hk, netuid);
        assert_eq!(other_stake_after, 0.into());
    });
}

// Test that if number of sn owner hotkeys is greater than ImmuneOwnerUidsLimit, then the ones with
// higher BlockAtRegistration are used to burn
// SKIP_WASM_BUILD=1 RUST_LOG=debug cargo test --package pallet-subtensor --lib -- tests::coinbase::incentive_burn::test_burn_key_sorting --exact --show-output --nocapture
#[test]
fn test_burn_key_sorting() {
    new_test_ext(1).execute_with(|| {
        let subnet_owner_ck = U256::from(0);
        let subnet_owner_hk = U256::from(1);

        // Other hk owned by owner
        let other_hk_1 = U256::from(3);
        let other_hk_2 = U256::from(4);
        let other_hk_3 = U256::from(5);
        Owner::<Test>::insert(other_hk_1, subnet_owner_ck);
        Owner::<Test>::insert(other_hk_2, subnet_owner_ck);
        Owner::<Test>::insert(other_hk_3, subnet_owner_ck);
        OwnedHotkeys::<Test>::insert(
            subnet_owner_ck,
            vec![subnet_owner_hk, other_hk_1, other_hk_2, other_hk_3],
        );

        let netuid = add_dynamic_network(&subnet_owner_hk, &subnet_owner_ck);
        remove_owner_registration_stake(netuid);

        // Set block of registration and UIDs for other hotkeys
        // HK1 has block of registration 2
        // HK2 and HK3 have the same block of registration 1, so they are sorted by UID
        // Set HK2 UID = 3 and HK3 UID = 2 so that HK3 is burned and HK2 is not
        // Summary: HK1 and HK3 should be burned, HK2 should be not.
        // Let's test it now.
        BlockAtRegistration::<Test>::insert(netuid, 1, 2);
        BlockAtRegistration::<Test>::insert(netuid, 3, 1);
        BlockAtRegistration::<Test>::insert(netuid, 2, 1);
        Uids::<Test>::insert(netuid, other_hk_1, 1);
        Uids::<Test>::insert(netuid, other_hk_2, 3);
        Uids::<Test>::insert(netuid, other_hk_3, 2);

        let pending_tao: u64 = 1_000_000_000;
        let pending_alpha = AlphaBalance::ZERO; // None to valis
        let owner_cut = AlphaBalance::ZERO;
        let mut incentives: BTreeMap<U256, AlphaBalance> = BTreeMap::new();

        // Give incentive to hotkeys
        incentives.insert(other_hk_1, 10_000_000.into());
        incentives.insert(other_hk_2, 10_000_000.into());
        incentives.insert(other_hk_3, 10_000_000.into());

        // Give incentives to subnet_owner_hk
        incentives.insert(subnet_owner_hk, 10_000_000.into());

        // Distribute dividends and incentives
        SubtensorModule::distribute_dividends_and_incentives(
            netuid,
            owner_cut,
            incentives,
            BTreeMap::new(),
            BTreeMap::new(),
        );

        // SN owner is burned
        let subnet_owner_stake_after =
            SubtensorModule::get_stake_for_hotkey_on_subnet(&subnet_owner_hk, netuid);
        assert_eq!(subnet_owner_stake_after, 0.into());

        // No burn limits, all HKs should be burned
        let other_stake_after_1 =
            SubtensorModule::get_stake_for_hotkey_on_subnet(&other_hk_1, netuid);
        let other_stake_after_2 =
            SubtensorModule::get_stake_for_hotkey_on_subnet(&other_hk_2, netuid);
        let other_stake_after_3 =
            SubtensorModule::get_stake_for_hotkey_on_subnet(&other_hk_3, netuid);
        assert_eq!(other_stake_after_1, 0.into());
        assert_eq!(other_stake_after_2, 0.into());
        assert_eq!(other_stake_after_3, 0.into());
    });
}
