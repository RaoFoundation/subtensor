#![allow(clippy::indexing_slicing)]
#![allow(clippy::unwrap_used)]
#![allow(clippy::arithmetic_side_effects)]
use super::super::mock;
use super::super::mock::*;
use approx::assert_abs_diff_eq;
use frame_support::assert_ok;
use substrate_fixed::types::I96F32;

use crate::*;
use sp_core::U256;
use sp_runtime::PerU16;

// 46: Test dividend distribution with children
// This test verifies the correct distribution of emissions in a chain of parent-child relationships:
// - Sets up a network with three neurons A, B, and C in a chain (A -> B -> C)
// - Establishes parent-child relationships with different stake proportions
// - Adds a childkey take for both B and C
// - Distributes emission across each hotkey using a the helper
// - Checks the emission distribution among A, B, and C
// SKIP_WASM_BUILD=1 RUST_LOG=debug cargo test --package pallet-subtensor --lib -- tests::children::child_dividends::test_dividend_distribution_with_children --exact --show-output
#[test]
fn test_dividend_distribution_with_children() {
    new_test_ext(1).execute_with(|| {
        let netuid = NetUid::from(1);
        add_network(netuid, 1, 0);
        SubtensorModule::set_ck_burn(0);
        mock::setup_reserves(
            netuid,
            1_000_000_000_000_000_u64.into(),
            1_000_000_000_000_000_u64.into(),
        );
        // Set owner cut to 0
        SubtensorModule::set_subnet_owner_cut(0_u16);

        // Define hotkeys and coldkeys
        let hotkey_a: U256 = U256::from(1);
        let hotkey_b: U256 = U256::from(2);
        let hotkey_c: U256 = U256::from(3);
        let coldkey_a: U256 = U256::from(100);
        let coldkey_b: U256 = U256::from(101);
        let coldkey_c: U256 = U256::from(102);

        // Register neurons with decreasing stakes
        register_ok_neuron(netuid, hotkey_a, coldkey_a, 0);
        register_ok_neuron(netuid, hotkey_b, coldkey_b, 0);
        register_ok_neuron(netuid, hotkey_c, coldkey_c, 0);

        // Add initial stakes
        add_balance_to_coldkey_account(&coldkey_a, 1_000.into());
        add_balance_to_coldkey_account(&coldkey_b, 1_000.into());
        add_balance_to_coldkey_account(&coldkey_c, 1_000.into());

        // Swap to alpha
        let total_tao = I96F32::from_num(300_000 + 100_000 + 50_000);
        let (total_alpha, _) = mock::swap_tao_to_alpha(netuid, total_tao.to_num::<u64>().into());
        let total_alpha = I96F32::from_num(total_alpha);

        // Set the stakes directly
        // This avoids needing to swap tao to alpha, impacting the initial stake distribution.
        SubtensorModule::increase_stake_for_hotkey_and_coldkey_on_subnet(
            &hotkey_a,
            &coldkey_a,
            netuid,
            (total_alpha * I96F32::from_num(300_000) / total_tao)
                .saturating_to_num::<u64>()
                .into(),
        );
        SubtensorModule::increase_stake_for_hotkey_and_coldkey_on_subnet(
            &hotkey_b,
            &coldkey_b,
            netuid,
            (total_alpha * I96F32::from_num(100_000) / total_tao)
                .saturating_to_num::<u64>()
                .into(),
        );
        SubtensorModule::increase_stake_for_hotkey_and_coldkey_on_subnet(
            &hotkey_c,
            &coldkey_c,
            netuid,
            (total_alpha * I96F32::from_num(50_000) / total_tao)
                .saturating_to_num::<u64>()
                .into(),
        );

        // Get old stakes
        let stake_a = SubtensorModule::get_total_stake_for_hotkey(&hotkey_a);
        let stake_b = SubtensorModule::get_total_stake_for_hotkey(&hotkey_b);
        let stake_c = SubtensorModule::get_total_stake_for_hotkey(&hotkey_c);

        // Assert initial stake is correct
        let rel_stake_a = I96F32::from_num(stake_a) / total_alpha;
        let rel_stake_b = I96F32::from_num(stake_b) / total_alpha;
        let rel_stake_c = I96F32::from_num(stake_c) / total_alpha;

        log::info!("rel_stake_a: {rel_stake_a:?}"); // 0.6666 -> 2/3
        log::info!("rel_stake_b: {rel_stake_b:?}"); // 0.2222 -> 2/9
        log::info!("rel_stake_c: {rel_stake_c:?}"); // 0.1111 -> 1/9
        let epsilon = I96F32::from_num(0.00001);
        assert!((rel_stake_a - I96F32::from_num(300_000) / total_tao).abs() <= epsilon);
        assert!((rel_stake_b - I96F32::from_num(100_000) / total_tao).abs() <= epsilon);
        assert!((rel_stake_c - I96F32::from_num(50_000) / total_tao).abs() <= epsilon);

        // Set parent-child relationships
        // A -> B (50% of A's stake)
        mock_set_children(&coldkey_a, &hotkey_a, netuid, &[(u64::MAX / 2, hotkey_b)]);

        // B -> C (50% of B's stake)
        mock_set_children(&coldkey_b, &hotkey_b, netuid, &[(u64::MAX / 2, hotkey_c)]);

        // Set CHK take rate to 1/9
        let chk_take: I96F32 = I96F32::from_num(1_f64 / 9_f64);
        let chk_take_u16: u16 = (chk_take * I96F32::from_num(u16::MAX)).saturating_to_num::<u16>();
        ChildkeyTake::<Test>::insert(hotkey_b, netuid, PerU16::from_parts(chk_take_u16));
        ChildkeyTake::<Test>::insert(hotkey_c, netuid, PerU16::from_parts(chk_take_u16));

        // Set the weight of root TAO to be 0%, so only alpha is effective.
        SubtensorModule::set_tao_weight(0);

        let hardcoded_emission: I96F32 = I96F32::from_num(1_000_000); // 1 million (adjust as needed)

        let hotkey_emission =
            SubtensorModule::epoch(netuid, hardcoded_emission.saturating_to_num::<u64>().into());
        log::info!("hotkey_emission: {hotkey_emission:?}");
        let total_emission: I96F32 = hotkey_emission
            .iter()
            .map(|(_, _, emission)| I96F32::from_num(*emission))
            .sum();

        // Verify emissions match expected from CHK arrangements
        let em_eps: I96F32 = I96F32::from_num(1e-4); // 4 decimal places
        // A's pending emission:
        assert!(
            ((I96F32::from_num(hotkey_emission[0].2) / total_emission) -
            I96F32::from_num(2_f64 / 3_f64 * 1_f64 / 2_f64)).abs() // 2/3 * 1/2 = 1/3; 50% -> B
			<= em_eps,
            "A should have pending emission of 1/3 of total emission"
        );
        // B's pending emission:
        assert!(
            ((I96F32::from_num(hotkey_emission[1].2) / total_emission) -
            (I96F32::from_num(2_f64 / 9_f64 * 1_f64 / 2_f64 + 2_f64 / 3_f64 * 1_f64 / 2_f64))).abs() // 2/9 * 1/2 + 2/3 * 1/2; 50% -> C + 50% from A
            <= em_eps,
            "B should have pending emission of 4/9 of total emission"
        );
        // C's pending emission:
        assert!(
            ((I96F32::from_num(hotkey_emission[2].2) / total_emission) -
            (I96F32::from_num(1_f64 / 9_f64 + 1_f64 / 2_f64 * 2_f64 / 9_f64))).abs() // 1/9 + 2/9 * 1/2; 50% from B
            <= em_eps,
            "C should have pending emission of 1/9 of total emission"
        );

        let dividends_a = SubtensorModule::get_parent_child_dividends_distribution(
            &hotkey_a,
            netuid,
            hardcoded_emission.saturating_to_num::<u64>().into(),
        );
        let dividends_b = SubtensorModule::get_parent_child_dividends_distribution(
            &hotkey_b,
            netuid,
            hardcoded_emission.saturating_to_num::<u64>().into(),
        );
        let dividends_c = SubtensorModule::get_parent_child_dividends_distribution(
            &hotkey_c,
            netuid,
            hardcoded_emission.saturating_to_num::<u64>().into(),
        );
        log::info!("dividends_a: {dividends_a:?}");
        log::info!("dividends_b: {dividends_b:?}");
        log::info!("dividends_c: {dividends_c:?}");

        // We expect A to get all of its own emission, as it has no parents.
        assert_eq!(dividends_a.len(), 1);
        assert_eq!(dividends_a[0].0, hotkey_a);
        assert_eq!(
            dividends_a[0].1,
            hardcoded_emission.saturating_to_num::<u64>().into()
        );
        assert_abs_diff_eq!(
            dividends_a
                .iter()
                .map(|(_, emission)| u64::from(*emission))
                .sum::<u64>(),
            hardcoded_emission.saturating_to_num::<u64>(),
            epsilon = (hardcoded_emission / 1000).saturating_to_num::<u64>()
        );

        // We expect B to get a portion of its own emission, and some comission from A, where A gets the rest.
        // B re-delegates 0.5 of its stake to C; And A re-delegates 0.5 of its stake to B.
        let total_stake_b = rel_stake_b * 1 / 2 + rel_stake_a * 1 / 2;
        let expected_b_b: u64 = ((rel_stake_b * 1 / 2) / total_stake_b * hardcoded_emission
            + (rel_stake_a * 1 / 2) / total_stake_b * hardcoded_emission * chk_take)
            .saturating_to_num::<u64>();
        assert_eq!(dividends_b.len(), 2); // A and B
        assert_eq!(dividends_b[1].0, hotkey_b);
        assert_abs_diff_eq!(
            u64::from(dividends_b[1].1),
            expected_b_b,
            epsilon = (hardcoded_emission / 1000).saturating_to_num::<u64>()
        );
        let expected_b_a: u64 = hardcoded_emission.saturating_to_num::<u64>() - expected_b_b;
        assert_eq!(dividends_b[0].0, hotkey_a);
        assert_abs_diff_eq!(
            u64::from(dividends_b[0].1),
            expected_b_a,
            epsilon = (hardcoded_emission / 1000).saturating_to_num::<u64>()
        );
        assert_abs_diff_eq!(
            dividends_b
                .iter()
                .map(|(_, emission)| u64::from(*emission))
                .sum::<u64>(),
            hardcoded_emission.saturating_to_num::<u64>(),
            epsilon = (hardcoded_emission / 1000).saturating_to_num::<u64>()
        );

        // We expect C to get a portion of its own emission, and some comission from B, where B gets the rest.
        let total_stake_c = rel_stake_c + rel_stake_b * 1 / 2;
        let expected_c_c: u64 = (rel_stake_c / total_stake_c * hardcoded_emission
            + (rel_stake_b * 1 / 2) / total_stake_c * hardcoded_emission * chk_take)
            .saturating_to_num::<u64>();
        assert_eq!(dividends_c.len(), 2); // B and C
        assert_eq!(dividends_c[1].0, hotkey_c);
        assert_abs_diff_eq!(
            u64::from(dividends_c[1].1),
            expected_c_c,
            epsilon = (hardcoded_emission / 1000).saturating_to_num::<u64>()
        );
        let expected_c_b: u64 = hardcoded_emission.saturating_to_num::<u64>() - expected_c_c;
        assert_eq!(dividends_c[0].0, hotkey_b);
        assert_abs_diff_eq!(
            u64::from(dividends_c[0].1),
            expected_c_b,
            epsilon = (hardcoded_emission / 1000).saturating_to_num::<u64>()
        );
        assert_abs_diff_eq!(
            dividends_c
                .iter()
                .map(|(_, emission)| u64::from(*emission))
                .sum::<u64>(),
            hardcoded_emission.saturating_to_num::<u64>(),
            epsilon = (hardcoded_emission / 1000).saturating_to_num::<u64>()
        );
    });
}

// 47: Test emission distribution when adding/removing parent-child relationships mid-epoch
// This test verifies the correct distribution of emissions when parent-child relationships change:
// - Sets up a network with three neurons: parent, child1, and child2
// - Establishes initial parent-child relationship between parent and child1
// - Runs first epoch and distributes emissions
// - Changes parent-child relationships to include both child1 and child2
// - Runs second epoch and distributes emissions
// - Checks final emission distribution and stake updates
// - Verifies correct parent-child relationships and stake proportions
// SKIP_WASM_BUILD=1 RUST_LOG=debug cargo test --package pallet-subtensor --lib -- tests::children::child_dividends::test_dynamic_parent_child_relationships --exact --show-output
#[test]
fn test_dynamic_parent_child_relationships() {
    new_test_ext(1).execute_with(|| {
        let netuid = NetUid::from(1);
        SubtensorModule::set_ck_burn(0);
        add_network_disable_commit_reveal(netuid, 1, 0);

        // Define hotkeys and coldkeys
        let parent = U256::from(1);
        let child1 = U256::from(2);
        let child2 = U256::from(3);
        let coldkey_parent = U256::from(100);
        let coldkey_child1 = U256::from(101);
        let coldkey_child2 = U256::from(102);

        // Register neurons with varying stakes
        register_ok_neuron(netuid, parent, coldkey_parent, 0);
        register_ok_neuron(netuid, child1, coldkey_child1, 0);
        register_ok_neuron(netuid, child2, coldkey_child2, 0);

        let chk_take_1 = SubtensorModule::get_childkey_take(&child1, netuid);
        let chk_take_2 = SubtensorModule::get_childkey_take(&child2, netuid);
        log::info!("child take 1: {chk_take_1:?}");
        log::info!("child take 2: {chk_take_2:?}");

        // Add initial stakes
        add_balance_to_coldkey_account(&coldkey_parent, (500_000 + 1_000).into());
        add_balance_to_coldkey_account(&coldkey_child1, (50_000 + 1_000).into());
        add_balance_to_coldkey_account(&coldkey_child2, (30_000 + 1_000).into());

        let reserve = 1_000_000_000_000_u64;
        mock::setup_reserves(netuid, reserve.into(), reserve.into());

        // Swap to alpha
        let total_tao = I96F32::from_num(500_000 + 50_000 + 30_000);
        let (total_alpha, _) = mock::swap_tao_to_alpha(netuid, total_tao.to_num::<u64>().into());
        let total_alpha = I96F32::from_num(total_alpha);
        log::info!("total_alpha: {total_alpha:?}");

        // Set the stakes directly
        // This avoids needing to swap tao to alpha, impacting the initial stake distribution.
        SubtensorModule::increase_stake_for_hotkey_and_coldkey_on_subnet(
            &parent,
            &coldkey_parent,
            netuid,
            (total_alpha * I96F32::from_num(500_000) / total_tao)
                .saturating_to_num::<u64>()
                .into(),
        );
        SubtensorModule::increase_stake_for_hotkey_and_coldkey_on_subnet(
            &child1,
            &coldkey_child1,
            netuid,
            (total_alpha * I96F32::from_num(50_000) / total_tao)
                .saturating_to_num::<u64>()
                .into(),
        );
        SubtensorModule::increase_stake_for_hotkey_and_coldkey_on_subnet(
            &child2,
            &coldkey_child2,
            netuid,
            (total_alpha * I96F32::from_num(30_000) / total_tao)
                .saturating_to_num::<u64>()
                .into(),
        );

        // Get old stakes
        let stake_parent_0 = SubtensorModule::get_stake_for_hotkey_on_subnet(&parent, netuid);
        let stake_child1_0 = SubtensorModule::get_stake_for_hotkey_on_subnet(&child1, netuid);
        let stake_child2_0 = SubtensorModule::get_stake_for_hotkey_on_subnet(&child2, netuid);
        log::info!("stake_parent_0: {stake_parent_0:?}");
        log::info!("stake_child1_0: {stake_child1_0:?}");
        log::info!("stake_child2_0: {stake_child2_0:?}");

        let total_stake_0 = stake_parent_0 + stake_child1_0 + stake_child2_0;

        // Assert initial stake is correct
        let rel_stake_parent_0 = I96F32::from_num(stake_parent_0) / total_alpha;
        let rel_stake_child1_0 = I96F32::from_num(stake_child1_0) / total_alpha;
        let rel_stake_child2_0 = I96F32::from_num(stake_child2_0) / total_alpha;

        log::info!("rel_stake_parent_0: {rel_stake_parent_0:?}");
        log::info!("rel_stake_child1_0: {rel_stake_child1_0:?}");
        log::info!("rel_stake_child2_0: {rel_stake_child2_0:?}");
        let epsilon = I96F32::from_num(0.00001);
        assert!((rel_stake_parent_0 - I96F32::from_num(500_000) / total_tao).abs() <= epsilon);
        assert!((rel_stake_child1_0 - I96F32::from_num(50_000) / total_tao).abs() <= epsilon);
        assert!((rel_stake_child2_0 - I96F32::from_num(30_000) / total_tao).abs() <= epsilon);

        mock_set_children_no_epochs(netuid, &parent, &[(u64::MAX / 2, child1)]);

        step_block(2);

        // Set weights
        let origin = RuntimeOrigin::signed(parent);
        let uids: Vec<u16> = vec![0, 1, 2]; // UIDs for parent, child1, child2
        let values: Vec<u16> = vec![65535, 65535, 65535]; // Set equal weights for all hotkeys
        let version_key = SubtensorModule::get_weights_version_key(netuid);

        // Ensure we can set weights without rate limiting
        SubtensorModule::set_weights_set_rate_limit(netuid, 0);

        assert_ok!(SubtensorModule::set_weights(
            origin,
            netuid,
            uids,
            values,
            version_key
        ));

        // Step blocks to allow for emission distribution
        step_block(11);

        // Get total stake after first payout
        let total_stake_1 = SubtensorModule::get_stake_for_hotkey_on_subnet(&parent, netuid)
            + SubtensorModule::get_stake_for_hotkey_on_subnet(&child1, netuid)
            + SubtensorModule::get_stake_for_hotkey_on_subnet(&child2, netuid);
        log::info!("total_stake_1: {total_stake_1:?}");

        // Change parent-child relationships
        mock_set_children_no_epochs(
            netuid,
            &parent,
            &[(u64::MAX / 4, child1), (u64::MAX / 3, child2)],
        );

        // Step blocks again to allow for emission distribution
        step_block(11);

        // Get total stake after second payout
        let total_stake_2 = SubtensorModule::get_stake_for_hotkey_on_subnet(&parent, netuid)
            + SubtensorModule::get_stake_for_hotkey_on_subnet(&child1, netuid)
            + SubtensorModule::get_stake_for_hotkey_on_subnet(&child2, netuid);
        log::info!("total_stake_2: {total_stake_2:?}");

        // Check final emission distribution
        let stake_parent_2 = SubtensorModule::get_inherited_for_hotkey_on_subnet(&parent, netuid);
        let stake_child1_2 = SubtensorModule::get_inherited_for_hotkey_on_subnet(&child1, netuid);
        let stake_child2_2 = SubtensorModule::get_inherited_for_hotkey_on_subnet(&child2, netuid);
        let total_parent_stake = SubtensorModule::get_stake_for_hotkey_on_subnet(&parent, netuid);
        let total_child1_stake = SubtensorModule::get_stake_for_hotkey_on_subnet(&child1, netuid);
        let total_child2_stake = SubtensorModule::get_stake_for_hotkey_on_subnet(&child2, netuid);

        log::info!("Final stakes:");
        log::info!("Parent stake: {stake_parent_2}");
        log::info!("Child1 stake: {stake_child1_2}");
        log::info!("Child2 stake: {stake_child2_2}");

        // Payout 1
        let payout_1 = total_stake_1 - total_stake_0;
        log::info!("payout_1: {payout_1:?}");

        // Payout 2
        let payout_2 = total_stake_2 - total_stake_1;
        log::info!("payout_2: {payout_2:?}");

        let total_emission = I96F32::from_num(payout_1 + payout_2);

        #[allow(non_snake_case)]
        let TOLERANCE = I96F32::from_num(0.001); // Allow for a small discrepancy due to potential rounding

        // Precise assertions with tolerance
        log::info!("total_emission: {total_emission:?}");
        let expected_parent_stake =
            I96F32::from_num(total_parent_stake) * I96F32::from_num(5) / I96F32::from_num(12);
        assert!(
            (I96F32::from_num(stake_parent_2) - expected_parent_stake).abs()
                / expected_parent_stake
                <= TOLERANCE,
            "Parent stake should be close to {expected_parent_stake:?}, but was {stake_parent_2}"
        );
        // The final relationship leaves the parent with 1 - 1/4 - 1/3 = 5/12
        // of its current direct stake.

        let expected_child1_stake = I96F32::from_num(total_child1_stake)
            + I96F32::from_num(total_parent_stake) / I96F32::from_num(4);
        assert!(
            (I96F32::from_num(stake_child1_2) - expected_child1_stake).abs()
                / expected_child1_stake
                <= TOLERANCE,
            "Child1 stake should be close to {expected_child1_stake:?}, but was {stake_child1_2}"
        );
        // Child1 inherits 1/4 of the parent's current direct stake.

        let expected_child2_stake = I96F32::from_num(total_child2_stake)
            + I96F32::from_num(total_parent_stake) / I96F32::from_num(3);
        assert!(
            (I96F32::from_num(stake_child2_2) - expected_child2_stake).abs()
                / expected_child2_stake
                <= TOLERANCE,
            "Child2 stake should be close to {expected_child2_stake:?}, but was {stake_child2_2}"
        );
        // Child2 inherits 1/3 of the parent's current direct stake.

        // Additional checks for parent-child relationships
        let parent_children: Vec<(u64, U256)> = SubtensorModule::get_children(&parent, netuid);
        assert_eq!(
            parent_children,
            vec![(u64::MAX / 4, child1), (u64::MAX / 3, child2)],
            "Parent should have both children with correct proportions"
        );
        // Parent-child relationship:
        // child1: 1/4 of parent's stake
        // child2: 1/3 of parent's stake

        let child1_parents: Vec<(u64, U256)> = SubtensorModule::get_parents(&child1, netuid);
        assert_eq!(
            child1_parents,
            vec![(u64::MAX / 4, parent)],
            "Child1 should have parent as its parent with correct proportion"
        );
        // Child1-parent relationship:
        // parent: 1/4 of child1's stake

        let child2_parents: Vec<(u64, U256)> = SubtensorModule::get_parents(&child2, netuid);
        assert_eq!(
            child2_parents,
            vec![(u64::MAX / 3, parent)],
            "Child2 should have parent as its parent with correct proportion"
        );
        // Child2-parent relationship:
        // parent: 1/3 of child2's stake

        // Check that child2 has received more stake than child1
        assert!(
            stake_child2_2 > stake_child1_2,
            "Child2 should have received more emission than Child1 due to higher proportion"
        );
        // Child2 stake (874,826) > Child1 stake (778,446)
    });
}

// Test dividend distribution for children with same coldkey Owner
// SKIP_WASM_BUILD=1 RUST_LOG=debug cargo test --package pallet-subtensor --lib -- tests::children::child_dividends::test_dividend_distribution_with_children_same_coldkey_owner --exact --show-output
#[test]
fn test_dividend_distribution_with_children_same_coldkey_owner() {
    new_test_ext(1).execute_with(|| {
        let netuid = NetUid::from(1);
        add_network(netuid, 1, 0);
        // Set SN owner cut to 0
        SubtensorModule::set_subnet_owner_cut(0_u16);
        mock::setup_reserves(
            netuid,
            1_000_000_000_000_u64.into(),
            1_000_000_000_000_u64.into(),
        );

        // Define hotkeys and coldkeys
        let hotkey_a: U256 = U256::from(1);
        let hotkey_b: U256 = U256::from(2);
        let coldkey_a: U256 = U256::from(100); // Only one coldkey

        // Register neurons with decreasing stakes
        register_ok_neuron(netuid, hotkey_a, coldkey_a, 0);
        register_ok_neuron(netuid, hotkey_b, coldkey_a, 0);

        // Add initial stakes
        add_balance_to_coldkey_account(&coldkey_a, 1_000.into());
        add_balance_to_coldkey_account(&coldkey_a, 1_000.into());

        // Swap to alpha
        let total_tao = 300_000 + 100_000;
        let total_alpha = I96F32::from_num(mock::swap_tao_to_alpha(netuid, total_tao.into()).0);
        let total_tao = I96F32::from_num(total_tao);

        // Set the stakes directly
        // This avoids needing to swap tao to alpha, impacting the initial stake distribution.
        SubtensorModule::increase_stake_for_hotkey_and_coldkey_on_subnet(
            &hotkey_a,
            &coldkey_a,
            netuid,
            (total_alpha * I96F32::from_num(300_000) / total_tao)
                .saturating_to_num::<u64>()
                .into(),
        );
        SubtensorModule::increase_stake_for_hotkey_and_coldkey_on_subnet(
            &hotkey_b,
            &coldkey_a,
            netuid,
            (total_alpha * I96F32::from_num(100_000) / total_tao)
                .saturating_to_num::<u64>()
                .into(),
        );

        // Get old stakes
        let stake_a = SubtensorModule::get_total_stake_for_hotkey(&hotkey_a);
        let stake_b = SubtensorModule::get_total_stake_for_hotkey(&hotkey_b);

        // Assert initial stake is correct
        let rel_stake_a = I96F32::from_num(stake_a) / total_alpha;
        let rel_stake_b = I96F32::from_num(stake_b) / total_alpha;

        log::info!("rel_stake_a: {rel_stake_a:?}"); // 0.75 -> 3/4
        log::info!("rel_stake_b: {rel_stake_b:?}"); // 0.25 -> 1/4
        let epsilon = I96F32::from_num(0.0001);
        assert!((rel_stake_a - I96F32::from_num(300_000) / total_tao).abs() <= epsilon);
        assert!((rel_stake_b - I96F32::from_num(100_000) / total_tao).abs() <= epsilon);

        // Set parent-child relationships
        // A -> B (50% of A's stake)
        mock_set_children(&coldkey_a, &hotkey_a, netuid, &[(u64::MAX / 2, hotkey_b)]);

        // Set CHK take rate to 1/9
        let chk_take: I96F32 = I96F32::from_num(1_f64 / 9_f64);
        let chk_take_u16: u16 = (chk_take * I96F32::from_num(u16::MAX)).saturating_to_num::<u16>();
        ChildkeyTake::<Test>::insert(hotkey_b, netuid, PerU16::from_parts(chk_take_u16));

        // Set the weight of root TAO to be 0%, so only alpha is effective.
        SubtensorModule::set_tao_weight(0);

        let hardcoded_emission: I96F32 = I96F32::from_num(1_000_000); // 1 million (adjust as needed)

        let hotkey_emission =
            SubtensorModule::epoch(netuid, hardcoded_emission.saturating_to_num::<u64>().into());
        log::info!("hotkey_emission: {hotkey_emission:?}");
        let total_emission: I96F32 = hotkey_emission
            .iter()
            .map(|(_, _, emission)| I96F32::from_num(*emission))
            .sum();

        // Verify emissions match expected from CHK arrangements
        let em_eps: I96F32 = I96F32::from_num(1e-4); // 4 decimal places
        // A's pending emission:
        assert!(
            ((I96F32::from_num(hotkey_emission[0].2) / total_emission) -
            I96F32::from_num(3_f64 / 4_f64 * 1_f64 / 2_f64)).abs() // 3/4 * 1/2 = 3/8; 50% -> B
			<= em_eps,
            "A should have pending emission of 3/8 of total emission"
        );
        // B's pending emission:
        assert!(
            ((I96F32::from_num(hotkey_emission[1].2) / total_emission) -
            (I96F32::from_num(1_f64 / 4_f64 + 3_f64 / 4_f64 * 1_f64 / 2_f64))).abs() // 1/4 + 3/4 * 1/2 = 5/8; 50% from A
            <= em_eps,
            "B should have pending emission of 5/8 of total emission: {:?}",
            I96F32::from_num(hotkey_emission[1].2) / total_emission
        );

        // Get the distribution of dividends including the Parent/Child relationship.
        let dividends_a = SubtensorModule::get_parent_child_dividends_distribution(
            &hotkey_a,
            netuid,
            hardcoded_emission.saturating_to_num::<u64>().into(),
        );
        let dividends_b = SubtensorModule::get_parent_child_dividends_distribution(
            &hotkey_b,
            netuid,
            hardcoded_emission.saturating_to_num::<u64>().into(),
        );
        log::info!("dividends_a: {dividends_a:?}");
        log::info!("dividends_b: {dividends_b:?}");

        // We expect A should have no impact from B, as they have the same owner.
        assert_eq!(dividends_a.len(), 1);
        assert_eq!(dividends_a[0].0, hotkey_a);
        assert_eq!(
            dividends_a[0].1,
            hardcoded_emission.saturating_to_num::<u64>().into()
        );
        assert_abs_diff_eq!(
            dividends_a
                .iter()
                .map(|(_, emission)| u64::from(*emission))
                .sum::<u64>(),
            hardcoded_emission.saturating_to_num::<u64>(),
            epsilon = (hardcoded_emission / 1000).saturating_to_num::<u64>()
        );

        // Expect only 2 dividends. Parent key A and child key B.
        assert_eq!(dividends_b.len(), 2); // A and B
        assert_eq!(dividends_b[0].0, hotkey_a);
        assert_eq!(dividends_b[1].0, hotkey_b);

        // We expect B's coldkey to have no increase in dividends from A, as they have the same owner.
        // And therefore, B should get no CHK_TAKE.

        // A should also have no decrease because there is no CHK_TAKE.
        let total_stake_b = rel_stake_b + rel_stake_a * 1 / 2;
        let expected_b_b: u64 =
            (rel_stake_b / total_stake_b * hardcoded_emission).saturating_to_num::<u64>();

        assert_abs_diff_eq!(
            u64::from(dividends_b[1].1),
            expected_b_b,
            epsilon = (hardcoded_emission / 1000).saturating_to_num::<u64>(),
        );

        let expected_b_a: u64 =
            ((rel_stake_a * 1 / 2) / total_stake_b * hardcoded_emission).saturating_to_num::<u64>();
        assert_eq!(dividends_b[0].0, hotkey_a);
        assert_abs_diff_eq!(
            u64::from(dividends_b[0].1),
            expected_b_a,
            epsilon = (hardcoded_emission / 1000).saturating_to_num::<u64>()
        );
        assert_abs_diff_eq!(
            dividends_b
                .iter()
                .map(|(_, emission)| u64::from(*emission))
                .sum::<u64>(),
            hardcoded_emission.saturating_to_num::<u64>(),
            epsilon = (hardcoded_emission / 1000).saturating_to_num::<u64>()
        );
    });
}
