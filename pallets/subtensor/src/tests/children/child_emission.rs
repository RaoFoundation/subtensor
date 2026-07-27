#![allow(clippy::indexing_slicing)]
#![allow(clippy::unwrap_used)]
#![allow(clippy::arithmetic_side_effects)]
use super::super::mock;
use super::super::mock::*;
use approx::assert_abs_diff_eq;
use substrate_fixed::types::I96F32;
use subtensor_runtime_common::{AlphaBalance, TaoBalance};

use crate::*;
use sp_core::U256;
use sp_runtime::PerU16;
use subtensor_swap_interface::SwapHandler;

// 44: Test with a chain of parent-child relationships (e.g., A -> B -> C)
// This test verifies the correct distribution of emissions in a chain of parent-child relationships:
// - Sets up a network with three neurons A, B, and C in a chain (A -> B -> C)
// - Establishes parent-child relationships with different stake proportions
// - Sets weights for all neurons
// - Runs an epoch with a hardcoded emission value
// - Checks the emission distribution among A, B, and C
// - Verifies that all parties received emissions and the total stake increased correctly
// SKIP_WASM_BUILD=1 RUST_LOG=debug cargo test --package pallet-subtensor --lib -- tests::children::child_emission::test_parent_child_chain_emission --exact --show-output
#[test]
fn test_parent_child_chain_emission() {
    new_test_ext(1).execute_with(|| {
        let subnet_owner_coldkey = U256::from(1001);
        let subnet_owner_hotkey = U256::from(1002);
        let netuid = add_dynamic_network(&subnet_owner_hotkey, &subnet_owner_coldkey);
        remove_owner_registration_stake(netuid);
        SubtensorModule::set_ck_burn(0);
        Tempo::<Test>::insert(netuid, 1);

        // Setup large LPs to prevent slippage
        SubnetTAO::<Test>::insert(netuid, TaoBalance::from(1_000_000_000_000_000_u64));
        SubnetAlphaIn::<Test>::insert(netuid, AlphaBalance::from(1_000_000_000_000_000_u64));

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
        let stake_a = 300_000_000_000_u64;
        let stake_b = 100_000_000_000_u64;
        let stake_c = 50_000_000_000_u64;
        let total_tao: I96F32 = I96F32::from_num(stake_a + stake_b + stake_c);
        let total_alpha: I96F32 = I96F32::from_num(
            SubtensorModule::swap_tao_for_alpha(
                netuid,
                total_tao.to_num::<u64>().into(),
                <Test as Config>::SwapInterface::max_price(),
                false,
            )
            .unwrap()
            .amount_paid_out,
        );

        // Set the stakes directly
        // This avoids needing to swap tao to alpha, impacting the initial stake distribution.
        SubtensorModule::increase_stake_for_hotkey_and_coldkey_on_subnet(
            &hotkey_a,
            &coldkey_a,
            netuid,
            (total_alpha * I96F32::from_num(stake_a) / total_tao)
                .saturating_to_num::<u64>()
                .into(),
        );
        SubtensorModule::increase_stake_for_hotkey_and_coldkey_on_subnet(
            &hotkey_b,
            &coldkey_b,
            netuid,
            (total_alpha * I96F32::from_num(stake_b) / total_tao)
                .saturating_to_num::<u64>()
                .into(),
        );
        SubtensorModule::increase_stake_for_hotkey_and_coldkey_on_subnet(
            &hotkey_c,
            &coldkey_c,
            netuid,
            (total_alpha * I96F32::from_num(stake_c) / total_tao)
                .saturating_to_num::<u64>()
                .into(),
        );

        // Get old stakes
        let stake_a = SubtensorModule::get_total_stake_for_hotkey(&hotkey_a);
        let stake_b = SubtensorModule::get_total_stake_for_hotkey(&hotkey_b);
        let stake_c = SubtensorModule::get_total_stake_for_hotkey(&hotkey_c);

        let _total_stake: I96F32 = I96F32::from_num(stake_a + stake_b + stake_c);

        // Assert initial stake is correct
        let rel_stake_a = I96F32::from_num(stake_a) / total_tao;
        let rel_stake_b = I96F32::from_num(stake_b) / total_tao;
        let rel_stake_c = I96F32::from_num(stake_c) / total_tao;

        log::info!("rel_stake_a: {rel_stake_a:?}"); // 0.6666 -> 2/3
        log::info!("rel_stake_b: {rel_stake_b:?}"); // 0.2222 -> 2/9
        log::info!("rel_stake_c: {rel_stake_c:?}"); // 0.1111 -> 1/9
        assert!((rel_stake_a - I96F32::from_num(stake_a) / total_tao).abs() < 0.001);
        assert!((rel_stake_b - I96F32::from_num(stake_b) / total_tao).abs() < 0.001);
        assert!((rel_stake_c - I96F32::from_num(stake_c) / total_tao).abs() < 0.001);

        // Set parent-child relationships
        // A -> B (50% of A's stake)
        mock_set_children_no_epochs(netuid, &hotkey_a, &[(u64::MAX / 2, hotkey_b)]);

        // B -> C (50% of B's stake)
        mock_set_children_no_epochs(netuid, &hotkey_b, &[(u64::MAX / 2, hotkey_c)]);

        // Get old stakes after children are scheduled
        let stake_a_old = SubtensorModule::get_total_stake_for_hotkey(&hotkey_a);
        let stake_b_old = SubtensorModule::get_total_stake_for_hotkey(&hotkey_b);
        let stake_c_old = SubtensorModule::get_total_stake_for_hotkey(&hotkey_c);

        let total_stake_old: I96F32 =
            I96F32::from_num((stake_a_old + stake_b_old + stake_c_old).to_u64());
        log::info!("Old stake for hotkey A: {stake_a_old:?}");
        log::info!("Old stake for hotkey B: {stake_b_old:?}");
        log::info!("Old stake for hotkey C: {stake_c_old:?}");
        log::info!("Total old stake: {total_stake_old:?}");

        // Set CHK take rate to 1/9
        let chk_take: I96F32 = I96F32::from_num(1_f64 / 9_f64);
        let chk_take_u16: u16 = (chk_take * I96F32::from_num(u16::MAX)).saturating_to_num::<u16>();
        ChildkeyTake::<Test>::insert(hotkey_b, netuid, PerU16::from_parts(chk_take_u16));
        ChildkeyTake::<Test>::insert(hotkey_c, netuid, PerU16::from_parts(chk_take_u16));

        // Set the weight of root TAO to be 0%, so only alpha is effective.
        SubtensorModule::set_tao_weight(0);

        let emission = SubtensorModule::get_block_emission();

        // Set pending emission to 0
        PendingValidatorEmission::<Test>::insert(netuid, AlphaBalance::ZERO);
        PendingServerEmission::<Test>::insert(netuid, AlphaBalance::ZERO);

        // To trigger the epoch, block should be > tempo. So we advance it before
        System::set_block_number(2);

        // Run epoch with emission value
        let emission_value = u64::from(emission.peek());
        SubtensorModule::run_coinbase(emission);

        // Log new stake
        let stake_a_new = SubtensorModule::get_total_stake_for_hotkey(&hotkey_a);
        let stake_b_new = SubtensorModule::get_total_stake_for_hotkey(&hotkey_b);
        let stake_c_new = SubtensorModule::get_total_stake_for_hotkey(&hotkey_c);
        let total_stake_new = I96F32::from_num((stake_a_new + stake_b_new + stake_c_new).to_u64());
        log::info!("Stake for hotkey A: {stake_a_new:?}");
        log::info!("Stake for hotkey B: {stake_b_new:?}");
        log::info!("Stake for hotkey C: {stake_c_new:?}");

        let stake_inc_a = stake_a_new - stake_a_old;
        let stake_inc_b = stake_b_new - stake_b_old;
        let stake_inc_c = stake_c_new - stake_c_old;
        let total_stake_inc: I96F32 = total_stake_new - total_stake_old;
        log::info!("Stake increase for hotkey A: {stake_inc_a:?}");
        log::info!("Stake increase for hotkey B: {stake_inc_b:?}");
        log::info!("Stake increase for hotkey C: {stake_inc_c:?}");
        log::info!("Total stake increase: {total_stake_inc:?}");
        let rel_stake_inc_a = I96F32::from_num(stake_inc_a) / total_stake_inc;
        let rel_stake_inc_b = I96F32::from_num(stake_inc_b) / total_stake_inc;
        let rel_stake_inc_c = I96F32::from_num(stake_inc_c) / total_stake_inc;
        log::info!("rel_stake_inc_a: {rel_stake_inc_a:?}");
        log::info!("rel_stake_inc_b: {rel_stake_inc_b:?}");
        log::info!("rel_stake_inc_c: {rel_stake_inc_c:?}");

        // Verify the final stake distribution
        let stake_inc_eps = I96F32::from_num(1e-4); // 4 decimal places

        // Each child has chk_take take
        let expected_a = I96F32::from_num(2_f64 / 3_f64)
            * (I96F32::from_num(1_f64) - (I96F32::from_num(1_f64 / 2_f64) * chk_take));
        assert!(
            (rel_stake_inc_a - expected_a).abs() // B's take on 50% CHK
            <= stake_inc_eps,
            "A should have {expected_a:?} of total stake increase; {rel_stake_inc_a:?}"
        );
        let expected_b = I96F32::from_num(2_f64 / 9_f64)
            * (I96F32::from_num(1_f64) - (I96F32::from_num(1_f64 / 2_f64) * chk_take))
            + I96F32::from_num(2_f64 / 3_f64) * (I96F32::from_num(1_f64 / 2_f64) * chk_take);
        assert!(
            (rel_stake_inc_b - expected_b).abs() // C's take on 50% CHK + take from A
            <= stake_inc_eps,
            "B should have {expected_b:?} of total stake increase; {rel_stake_inc_b:?}"
        );
        let expected_c = I96F32::from_num(1_f64 / 9_f64)
            + (I96F32::from_num(2_f64 / 9_f64) * I96F32::from_num(1_f64 / 2_f64) * chk_take);
        assert!(
            (rel_stake_inc_c - expected_c).abs() // B's take on 50% CHK
            <= stake_inc_eps,
            "C should have {expected_c:?} of total stake increase; {rel_stake_inc_c:?}"
        );

        let hotkeys = [hotkey_a, hotkey_b, hotkey_c];
        let mut total_stake_now = AlphaBalance::ZERO;
        for (hotkey, netuid, stake) in TotalHotkeyAlpha::<Test>::iter() {
            if hotkeys.contains(&hotkey) {
                total_stake_now += stake;
            } else {
                log::info!("hotkey: {hotkey:?}, netuid: {netuid:?}, stake: {stake:?}");
            }
        }
        log::info!("total_stake_now: {total_stake_now:?}, total_stake_new: {total_stake_new:?}");

        assert_abs_diff_eq!(
            total_stake_inc.to_num::<u64>(),
            emission_value,
            epsilon = emission_value / 1000,
        );
    });
}

// 45: Test *epoch* with a chain of parent-child relationships (e.g., A -> B -> C)
// This test verifies the correct distribution of emissions in a chain of parent-child relationships:
// - Sets up a network with three neurons A, B, and C in a chain (A -> B -> C)
// - Establishes parent-child relationships with different stake proportions
// - Sets weights for all neurons
// - Runs an epoch with a hardcoded emission value
// - Checks the emission distribution among A, B, and C
// SKIP_WASM_BUILD=1 RUST_LOG=debug cargo test --package pallet-subtensor --lib -- tests::children::child_emission::test_parent_child_chain_epoch --exact --show-output
#[test]
fn test_parent_child_chain_epoch() {
    new_test_ext(1).execute_with(|| {
        let netuid = NetUid::from(1);
        add_network(netuid, 1, 0);
        SubtensorModule::set_ck_burn(0);
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

        mock::setup_reserves(
            netuid,
            1_000_000_000_000_u64.into(),
            1_000_000_000_000_u64.into(),
        );

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

        assert!(rel_stake_a > I96F32::from_num(0));
        assert!(rel_stake_b > I96F32::from_num(0));
        assert!(rel_stake_c > I96F32::from_num(0));

        // because of the fee we allow slightly higher range
        let epsilon = I96F32::from_num(0.00001);
        assert!((rel_stake_a - (I96F32::from_num(300_000) / total_tao)).abs() <= epsilon);
        assert!((rel_stake_b - (I96F32::from_num(100_000) / total_tao)).abs() <= epsilon);
        assert!((rel_stake_c - (I96F32::from_num(50_000) / total_tao)).abs() <= epsilon);

        // Set parent-child relationships
        // A -> B (50% of A's stake)
        mock_set_children(&coldkey_a, &hotkey_a, netuid, &[(u64::MAX / 2, hotkey_b)]);

        // B -> C (50% of B's stake)
        mock_set_children(&coldkey_b, &hotkey_b, netuid, &[(u64::MAX / 2, hotkey_c)]);

        // Set CHK take rate to 1/9
        let chk_take = I96F32::from_num(1_f64 / 9_f64);
        let chk_take_u16: u16 = (chk_take * I96F32::from_num(u16::MAX)).saturating_to_num::<u16>();
        ChildkeyTake::<Test>::insert(hotkey_b, netuid, PerU16::from_parts(chk_take_u16));
        ChildkeyTake::<Test>::insert(hotkey_c, netuid, PerU16::from_parts(chk_take_u16));

        // Set the weight of root TAO to be 0%, so only alpha is effective.
        SubtensorModule::set_tao_weight(0);

        let hardcoded_emission = I96F32::from_num(1_000_000); // 1 million (adjust as needed)

        let hotkey_emission =
            SubtensorModule::epoch(netuid, hardcoded_emission.saturating_to_num::<u64>().into());
        log::info!("hotkey_emission: {hotkey_emission:?}");
        let total_emission: I96F32 = hotkey_emission
            .iter()
            .map(|(_, _, emission)| I96F32::from_num(*emission))
            .sum();

        // Verify emissions match expected from CHK arrangements
        let em_eps = I96F32::from_num(1e-4); // 4 decimal places
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
    });
}
