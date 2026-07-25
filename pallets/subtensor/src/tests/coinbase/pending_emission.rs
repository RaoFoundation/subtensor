#![allow(
    unused,
    clippy::arithmetic_side_effects,
    clippy::indexing_slicing,
    clippy::panic,
    clippy::unwrap_used,
    clippy::expect_used
)]
//! Pending emission accumulation before and after start.

use super::helpers::*;
use super::prelude::*;

// SKIP_WASM_BUILD=1 RUST_LOG=debug cargo test --package pallet-subtensor --lib -- tests::coinbase::pending_emission::test_pending_emission --exact --show-output --nocapture
#[test]
fn test_pending_emission() {
    new_test_ext(1).execute_with(|| {
        let hotkey = U256::from(1);
        let coldkey = U256::from(2);
        let netuid = add_dynamic_network(&hotkey, &coldkey);
        remove_owner_registration_stake(netuid);
        Tempo::<Test>::insert(netuid, 1);
        FirstEmissionBlockNumber::<Test>::insert(netuid, 0);

        mock::setup_reserves(netuid, 1_000_000.into(), 1.into());
        LastEpochBlock::<Test>::insert(netuid, 0);
        System::set_block_number(10);
        SubtensorModule::run_coinbase(SubtensorModule::mint_tao(0.into()));
        SubnetTAO::<Test>::insert(NetUid::ROOT, TaoBalance::from(1_000_000_000)); // Add root weight.
        System::set_block_number(12);
        SubtensorModule::run_coinbase(SubtensorModule::mint_tao(0.into()));
        SubtensorModule::set_tempo_unchecked(netuid, 10000); // Large number (dont drain)
        SubtensorModule::set_tao_weight(u64::MAX); // Set TAO weight to 1.0

        // Set moving price > 1.0
        SubnetMovingPrice::<Test>::insert(netuid, I96F32::from_num(2));

        // Make sure we are root selling, so we have root alpha divs.
        let root_sell_flag = SubtensorModule::get_network_root_sell_flag(&[netuid]);
        assert!(root_sell_flag, "Root sell flag should be true");

        SubtensorModule::run_coinbase(SubtensorModule::mint_tao(0.into()));
        // 1 TAO / ( 1 + 3 ) = 0.25 * 1 / 2 = 125000000

        assert_abs_diff_eq!(
            u64::from(PendingServerEmission::<Test>::get(netuid)),
            500_000_000,
            epsilon = 1
        ); // 1 / 2.

        assert_abs_diff_eq!(
            u64::from(PendingValidatorEmission::<Test>::get(netuid)),
            500_000_000 - 125000000,
            epsilon = 1
        ); // 1 / 2 - swapped.

        assert_abs_diff_eq!(
            u64::from(PendingRootAlphaDivs::<Test>::get(netuid)),
            125000000,
            epsilon = 1
        ); // 1 / 2 * 0.25 --> (from root_prop)
    });
}

// SKIP_WASM_BUILD=1 RUST_LOG=debug cargo test --package pallet-subtensor --lib -- tests::coinbase::pending_emission::test_pending_emission_start_call_not_done --exact --show-output --nocapture
#[test]
fn test_pending_emission_start_call_not_done() {
    new_test_ext(1).execute_with(|| {
        let validator_coldkey = U256::from(1);
        let validator_hotkey = U256::from(2);
        let subnet_tempo = 10;
        let stake: u64 = 100_000_000_000;
        let root_stake: u64 = 200_000_000_000; // 200 TAO

        // Create root network
        NetworksAdded::<Test>::insert(NetUid::ROOT, true);
        // enabled root
        SubtokenEnabled::<Test>::insert(NetUid::ROOT, true);

        // Add network, register hotkeys, and setup network parameters
        let owner_hotkey = U256::from(10);
        let owner_coldkey = U256::from(11);
        let netuid = add_dynamic_network(&owner_hotkey, &owner_coldkey);
        // Remove FirstEmissionBlockNumber
        FirstEmissionBlockNumber::<Test>::remove(netuid);
        Tempo::<Test>::insert(netuid, subnet_tempo);

        register_ok_neuron(netuid, validator_hotkey, validator_coldkey, 0);
        add_balance_to_coldkey_account(
            &validator_coldkey,
            TaoBalance::from(stake) + ExistentialDeposit::get(),
        );
        SubtensorModule::set_weights_set_rate_limit(netuid, 0);
        step_block(subnet_tempo);
        SubnetOwnerCut::<Test>::set(u16::MAX / 10);
        // There are two validators and three neurons
        MaxAllowedUids::<Test>::set(netuid, 3);
        SubtensorModule::set_max_allowed_validators(netuid, 2);

        // Add stake to validator so it has root stake
        add_balance_to_coldkey_account(&validator_coldkey, root_stake.into());
        // init root
        assert_ok!(SubtensorModule::add_stake(
            RuntimeOrigin::signed(validator_coldkey),
            validator_hotkey,
            NetUid::ROOT,
            root_stake.into()
        ));
        // Set tao weight non zero
        SubtensorModule::set_tao_weight(u64::MAX / 10);

        // Make root sell happen
        // Set moving price > 1.0
        // Set price > 1.0
        let tao = TaoBalance::from(10_000_000_000_u64);
        let alpha = AlphaBalance::from(1_000_000_000_u64);
        SubnetTAO::<Test>::insert(netuid, tao);
        SubnetAlphaIn::<Test>::insert(netuid, alpha);

        SubnetMovingPrice::<Test>::insert(netuid, I96F32::from_num(2));

        // Make sure we are root selling, so we have root alpha divs.
        let root_sell_flag = SubtensorModule::get_network_root_sell_flag(&[netuid]);
        assert!(root_sell_flag, "Root sell flag should be true");

        // !!! Check that the subnet FirstEmissionBlockNumber is None -- no entry
        assert!(FirstEmissionBlockNumber::<Test>::get(netuid).is_none());

        // Run run_coinbase until emissions are accumulated
        step_block(subnet_tempo - 2);

        // Verify that all pending emissions are zero
        assert_eq!(
            PendingServerEmission::<Test>::get(netuid),
            AlphaBalance::ZERO
        );
        assert_eq!(
            PendingValidatorEmission::<Test>::get(netuid),
            AlphaBalance::ZERO
        );
        assert_eq!(
            PendingRootAlphaDivs::<Test>::get(netuid),
            AlphaBalance::ZERO
        );
    });
}
