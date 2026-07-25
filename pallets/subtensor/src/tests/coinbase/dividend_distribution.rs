#![allow(
    unused,
    clippy::arithmetic_side_effects,
    clippy::indexing_slicing,
    clippy::panic,
    clippy::unwrap_used,
    clippy::expect_used
)]
//! Dividend and incentive distribution math helpers.

use super::helpers::*;
use super::prelude::*;

#[test]
fn test_calculate_dividend_distribution_totals() {
    new_test_ext(1).execute_with(|| {
        let mut stake_map: BTreeMap<U256, (AlphaBalance, AlphaBalance)> = BTreeMap::new();
        let mut dividends: BTreeMap<U256, U96F32> = BTreeMap::new();

        let pending_validator_alpha = AlphaBalance::from(183_123_567_452_u64);
        let pending_root_alpha = AlphaBalance::from(837_120_949_872_u64);
        let tao_weight: U96F32 = U96F32::from_num(0.18); // 18%

        let hotkeys = [U256::from(0), U256::from(1)];

        // Stake map and dividends shouldn't matter for this test.
        stake_map.insert(hotkeys[0], (4_859_302.into(), 2_342_352.into()));
        stake_map.insert(hotkeys[1], (23_423.into(), 859_273.into()));
        dividends.insert(hotkeys[0], 77_783_738_u64.into());
        dividends.insert(hotkeys[1], 19_283_940_u64.into());

        let (alpha_dividends, root_alpha_dividends) =
            SubtensorModule::calculate_dividend_distribution(
                pending_validator_alpha,
                pending_root_alpha,
                tao_weight,
                stake_map,
                dividends,
            );

        // Verify the total of each dividends type is close to the inputs.
        let total_alpha_dividends = alpha_dividends.values().sum::<U96F32>();
        let total_root_alpha_dividends = root_alpha_dividends.values().sum::<U96F32>();

        assert_abs_diff_eq!(
            total_alpha_dividends.to_num::<u64>(),
            u64::from(pending_validator_alpha),
            epsilon = 1_000
        );
        assert_abs_diff_eq!(
            total_root_alpha_dividends.to_num::<u64>(),
            pending_root_alpha.to_u64(),
            epsilon = 1_000
        );
    });
}

#[test]
fn test_calculate_dividend_distribution_total_only_tao() {
    new_test_ext(1).execute_with(|| {
        let mut stake_map: BTreeMap<U256, (AlphaBalance, AlphaBalance)> = BTreeMap::new();
        let mut dividends: BTreeMap<U256, U96F32> = BTreeMap::new();

        let pending_validator_alpha = AlphaBalance::ZERO;
        let pending_root_alpha = AlphaBalance::from(837_120_949_872_u64);
        let tao_weight: U96F32 = U96F32::from_num(0.18); // 18%

        let hotkeys = [U256::from(0), U256::from(1)];

        // Stake map and dividends shouldn't matter for this test.
        stake_map.insert(hotkeys[0], (4_859_302.into(), 2_342_352.into()));
        stake_map.insert(hotkeys[1], (23_423.into(), 859_273.into()));
        dividends.insert(hotkeys[0], 77_783_738_u64.into());
        dividends.insert(hotkeys[1], 19_283_940_u64.into());

        let (alpha_dividends, root_alpha_dividends) =
            SubtensorModule::calculate_dividend_distribution(
                pending_validator_alpha,
                pending_root_alpha,
                tao_weight,
                stake_map,
                dividends,
            );

        // Verify the total of each dividends type is close to the inputs.
        let total_alpha_dividends = alpha_dividends.values().sum::<U96F32>();
        let total_root_alpha_dividends = root_alpha_dividends.values().sum::<U96F32>();

        assert_abs_diff_eq!(
            total_alpha_dividends.to_num::<u64>(),
            u64::from(pending_validator_alpha),
            epsilon = 1_000
        );
        assert_abs_diff_eq!(
            total_root_alpha_dividends.to_num::<u64>(),
            pending_root_alpha.to_u64(),
            epsilon = 1_000
        );
    });
}

#[test]
fn test_calculate_dividend_distribution_total_no_tao_weight() {
    new_test_ext(1).execute_with(|| {
        let mut stake_map: BTreeMap<U256, (AlphaBalance, AlphaBalance)> = BTreeMap::new();
        let mut dividends: BTreeMap<U256, U96F32> = BTreeMap::new();

        let pending_validator_alpha = AlphaBalance::from(183_123_567_452_u64);
        let pending_tao = TaoBalance::ZERO; // If tao weight is 0, then only alpha dividends should be input.
        let tao_weight: U96F32 = U96F32::from_num(0.0); // 0%

        let hotkeys = [U256::from(0), U256::from(1)];

        // Stake map and dividends shouldn't matter for this test.
        stake_map.insert(hotkeys[0], (4_859_302.into(), 2_342_352.into()));
        stake_map.insert(hotkeys[1], (23_423.into(), 859_273.into()));
        dividends.insert(hotkeys[0], 77_783_738_u64.into());
        dividends.insert(hotkeys[1], 19_283_940_u64.into());

        let (alpha_dividends, tao_dividends) = SubtensorModule::calculate_dividend_distribution(
            pending_validator_alpha,
            //   pending_tao,
            AlphaBalance::ZERO,
            tao_weight,
            stake_map,
            dividends,
        );

        // Verify the total of each dividends type is close to the inputs.
        let total_alpha_dividends = alpha_dividends.values().sum::<U96F32>();
        let total_tao_dividends = tao_dividends.values().sum::<U96F32>();

        assert_abs_diff_eq!(
            total_alpha_dividends.to_num::<u64>(),
            u64::from(pending_validator_alpha),
            epsilon = 1_000
        );
        assert_abs_diff_eq!(
            total_tao_dividends.to_num::<u64>(),
            pending_tao.to_u64(),
            epsilon = 1_000
        );
    });
}

#[test]
fn test_calculate_dividend_distribution_total_only_alpha() {
    new_test_ext(1).execute_with(|| {
        let mut stake_map: BTreeMap<U256, (AlphaBalance, AlphaBalance)> = BTreeMap::new();
        let mut dividends: BTreeMap<U256, U96F32> = BTreeMap::new();

        let pending_validator_alpha = AlphaBalance::from(183_123_567_452_u64);
        let pending_tao = TaoBalance::ZERO;
        let tao_weight: U96F32 = U96F32::from_num(0.18); // 18%

        let hotkeys = [U256::from(0), U256::from(1)];

        // Stake map and dividends shouldn't matter for this test.
        stake_map.insert(hotkeys[0], (4_859_302.into(), 2_342_352.into()));
        stake_map.insert(hotkeys[1], (23_423.into(), 859_273.into()));
        dividends.insert(hotkeys[0], 77_783_738_u64.into());
        dividends.insert(hotkeys[1], 19_283_940_u64.into());

        let (alpha_dividends, tao_dividends) = SubtensorModule::calculate_dividend_distribution(
            pending_validator_alpha,
            //   pending_tao,
            AlphaBalance::ZERO,
            tao_weight,
            stake_map,
            dividends,
        );

        // Verify the total of each dividends type is close to the inputs.
        let total_alpha_dividends = alpha_dividends.values().sum::<U96F32>();
        let total_tao_dividends = tao_dividends.values().sum::<U96F32>();

        assert_abs_diff_eq!(
            total_alpha_dividends.to_num::<u64>(),
            u64::from(pending_validator_alpha),
            epsilon = 1_000
        );
        assert_abs_diff_eq!(
            total_tao_dividends.to_num::<u64>(),
            pending_tao.to_u64(),
            epsilon = 1_000
        );
    });
}

#[test]
fn test_calculate_dividend_and_incentive_distribution() {
    new_test_ext(1).execute_with(|| {
        let sn_owner_hk = U256::from(0);
        let sn_owner_ck = U256::from(1);
        let netuid = add_dynamic_network(&sn_owner_hk, &sn_owner_ck);

        // Register a single neuron.
        let hotkey = U256::from(1);
        let coldkey = U256::from(2);
        register_ok_neuron(netuid, hotkey, coldkey, 0);
        // Give non-zero alpha
        SubtensorModule::increase_stake_for_hotkey_and_coldkey_on_subnet(
            &hotkey,
            &coldkey,
            netuid,
            1.into(),
        );

        let pending_alpha = AlphaBalance::from(123_456_789);
        let pending_validator_alpha = pending_alpha / 2.into(); // Pay half to validators.
        let pending_tao = TaoBalance::ZERO;
        let pending_swapped = 0; // Only alpha output.
        let tao_weight: U96F32 = U96F32::from_num(0.0); // 0%

        // Hotkey, Incentive, Dividend
        let hotkey_emission = vec![(hotkey, pending_alpha / 2.into(), pending_alpha / 2.into())];

        let (incentives, (alpha_dividends, tao_dividends)) =
            SubtensorModule::calculate_dividend_and_incentive_distribution(
                netuid,
                //   pending_tao,
                AlphaBalance::ZERO,
                pending_validator_alpha,
                hotkey_emission,
                tao_weight,
            );

        let incentives_total = incentives.values().copied().map(u64::from).sum::<u64>();
        let dividends_total = alpha_dividends.values().sum::<U96F32>().to_num::<u64>();

        assert_abs_diff_eq!(
            dividends_total + incentives_total,
            u64::from(pending_alpha),
            epsilon = 2
        );
    });
}

#[test]
fn test_calculate_dividend_and_incentive_distribution_all_to_validators() {
    new_test_ext(1).execute_with(|| {
        let sn_owner_hk = U256::from(0);
        let sn_owner_ck = U256::from(1);
        let netuid = add_dynamic_network(&sn_owner_hk, &sn_owner_ck);

        // Register a single neuron.
        let hotkey = U256::from(1);
        let coldkey = U256::from(2);
        register_ok_neuron(netuid, hotkey, coldkey, 0);
        // Give non-zero alpha
        SubtensorModule::increase_stake_for_hotkey_and_coldkey_on_subnet(
            &hotkey,
            &coldkey,
            netuid,
            1.into(),
        );

        let pending_alpha = AlphaBalance::from(123_456_789);
        let pending_validator_alpha = pending_alpha; // Pay all to validators.
        let pending_tao = TaoBalance::ZERO;
        let tao_weight: U96F32 = U96F32::from_num(0.0); // 0%

        // Hotkey, Incentive, Dividend
        let hotkey_emission = vec![(hotkey, 0.into(), pending_alpha)];

        let (incentives, (alpha_dividends, tao_dividends)) =
            SubtensorModule::calculate_dividend_and_incentive_distribution(
                netuid,
                //   pending_tao,
                AlphaBalance::ZERO,
                pending_validator_alpha,
                hotkey_emission,
                tao_weight,
            );

        let incentives_total = incentives.values().copied().map(u64::from).sum::<u64>();
        let dividends_total = alpha_dividends.values().sum::<U96F32>().to_num::<u64>();

        assert_eq!(
            AlphaBalance::from(dividends_total + incentives_total),
            pending_alpha
        );
    });
}

#[test]
fn test_calculate_dividends_and_incentives() {
    new_test_ext(1).execute_with(|| {
        let sn_owner_hk = U256::from(0);
        let sn_owner_ck = U256::from(1);
        let netuid = add_dynamic_network(&sn_owner_hk, &sn_owner_ck);

        // Register a single neuron.
        let hotkey = U256::from(1);
        let coldkey = U256::from(2);
        register_ok_neuron(netuid, hotkey, coldkey, 0);
        // Give non-zero alpha
        SubtensorModule::increase_stake_for_hotkey_and_coldkey_on_subnet(
            &hotkey,
            &coldkey,
            netuid,
            1.into(),
        );

        let divdends = AlphaBalance::from(123_456_789);
        let incentive = AlphaBalance::from(683_051_923);
        let total_emission = divdends + incentive;

        // Hotkey, Incentive, Dividend
        let hotkey_emission = vec![(hotkey, incentive, divdends)];

        let (incentives, dividends) =
            SubtensorModule::calculate_dividends_and_incentives(netuid, hotkey_emission);

        let incentives_total = incentives
            .values()
            .copied()
            .fold(AlphaBalance::ZERO, |acc, x| acc + x);
        let dividends_total =
            AlphaBalance::from(dividends.values().sum::<U96F32>().to_num::<u64>());

        assert_eq!(dividends_total + incentives_total, total_emission);
    });
}

#[test]
fn test_calculate_dividends_and_incentives_only_validators() {
    new_test_ext(1).execute_with(|| {
        let sn_owner_hk = U256::from(0);
        let sn_owner_ck = U256::from(1);
        let netuid = add_dynamic_network(&sn_owner_hk, &sn_owner_ck);

        // Register a single neuron.
        let hotkey = U256::from(1);
        let coldkey = U256::from(2);
        register_ok_neuron(netuid, hotkey, coldkey, 0);
        // Give non-zero alpha
        SubtensorModule::increase_stake_for_hotkey_and_coldkey_on_subnet(
            &hotkey,
            &coldkey,
            netuid,
            1.into(),
        );

        let divdends = AlphaBalance::from(123_456_789);
        let incentive = AlphaBalance::ZERO;

        // Hotkey, Incentive, Dividend
        let hotkey_emission = vec![(hotkey, incentive, divdends)];

        let (incentives, dividends) =
            SubtensorModule::calculate_dividends_and_incentives(netuid, hotkey_emission);

        let incentives_total = incentives
            .values()
            .copied()
            .fold(AlphaBalance::ZERO, |acc, x| acc + x);
        let dividends_total =
            AlphaBalance::from(dividends.values().sum::<U96F32>().to_num::<u64>());

        assert_eq!(dividends_total, divdends);
        assert_eq!(incentives_total, AlphaBalance::ZERO);
    });
}

#[test]
fn test_calculate_dividends_and_incentives_only_miners() {
    new_test_ext(1).execute_with(|| {
        let sn_owner_hk = U256::from(0);
        let sn_owner_ck = U256::from(1);
        let netuid = add_dynamic_network(&sn_owner_hk, &sn_owner_ck);

        // Register a single neuron.
        let hotkey = U256::from(1);
        let coldkey = U256::from(2);
        register_ok_neuron(netuid, hotkey, coldkey, 0);
        // Give non-zero alpha
        SubtensorModule::increase_stake_for_hotkey_and_coldkey_on_subnet(
            &hotkey,
            &coldkey,
            netuid,
            1.into(),
        );

        let divdends = AlphaBalance::ZERO;
        let incentive = AlphaBalance::from(123_456_789);

        // Hotkey, Incentive, Dividend
        let hotkey_emission = vec![(hotkey, incentive, divdends)];

        let (incentives, dividends) =
            SubtensorModule::calculate_dividends_and_incentives(netuid, hotkey_emission);

        let incentives_total = incentives
            .values()
            .copied()
            .fold(AlphaBalance::ZERO, |acc, x| acc + x);
        let dividends_total =
            AlphaBalance::from(dividends.values().sum::<U96F32>().to_num::<u64>());

        assert_eq!(incentives_total, incentive);
        assert_eq!(dividends_total, divdends);
    });
}
