#![allow(
    dead_code,
    clippy::arithmetic_side_effects,
    clippy::expect_used,
    clippy::unwrap_used
)]

use frame_support::{assert_ok, pallet_prelude::Zero, traits::Hooks};
use frame_system::RawOrigin;
use pallet_subtensor::utils::rate_limiting::TransactionType;
use pallet_subtensor::*;
use share_pool::SafeFloat;
use sp_core::{Get, H256, U256};
use sp_runtime::{BuildStorage, Saturating};
use subtensor_runtime_common::{AlphaBalance, NetUid, TaoBalance, Token};
use subtensor_swap_interface::{Order, SwapHandler};

use super::mock::*;

// Build genesis storage according to the mock runtime.
pub fn new_test_ext(block_number: BlockNumber) -> sp_io::TestExternalities {
    init_logs_for_tests();
    let t = frame_system::GenesisConfig::<Test>::default()
        .build_storage()
        .unwrap();
    let mut ext = sp_io::TestExternalities::new(t);
    ext.execute_with(|| System::set_block_number(block_number));
    ext
}

pub fn test_ext_with_balances(balances: Vec<(U256, u128)>) -> sp_io::TestExternalities {
    init_logs_for_tests();
    let mut t = frame_system::GenesisConfig::<Test>::default()
        .build_storage()
        .unwrap();

    pallet_balances::GenesisConfig::<Test> {
        balances: balances
            .iter()
            .map(|(a, b)| (*a, TaoBalance::from(*b as u64)))
            .collect::<Vec<(U256, TaoBalance)>>(),
        dev_accounts: None,
    }
    .assimilate_storage(&mut t)
    .unwrap();

    t.into()
}

pub fn step_block(n: u16) {
    for _ in 0..n {
        Scheduler::on_finalize(System::block_number());
        Proxy::on_finalize(System::block_number());
        SubtensorModule::on_finalize(System::block_number());
        System::on_finalize(System::block_number());
        System::set_block_number(System::block_number() + 1);
        System::on_initialize(System::block_number());
        SubtensorModule::on_initialize(System::block_number());
        Scheduler::on_initialize(System::block_number());
    }
}

pub fn run_to_block(n: u64) {
    run_to_block_ext(n, false)
}

pub fn run_to_block_ext(n: u64, enable_events: bool) {
    while System::block_number() < n {
        Scheduler::on_finalize(System::block_number());
        SubtensorModule::on_finalize(System::block_number());
        System::on_finalize(System::block_number());
        System::set_block_number(System::block_number() + 1);
        System::on_initialize(System::block_number());
        if !enable_events {
            System::events().iter().for_each(|event| {
                log::info!("Event: {:?}", event.event);
            });
            System::reset_events();
        }
        SubtensorModule::on_initialize(System::block_number());
        Scheduler::on_initialize(System::block_number());
    }
}

pub fn next_block_no_epoch(netuid: NetUid) -> u64 {
    // high tempo to skip automatic epochs in on_initialize
    let high_tempo: u16 = u16::MAX - 1;
    let old_tempo: u16 = SubtensorModule::get_tempo(netuid);

    SubtensorModule::set_tempo_unchecked(netuid, high_tempo);
    let new_block = next_block();
    SubtensorModule::set_tempo_unchecked(netuid, old_tempo);

    new_block
}

pub fn run_to_block_no_epoch(netuid: NetUid, n: u64) {
    // high tempo to skip automatic epochs in on_initialize
    let high_tempo: u16 = u16::MAX - 1;
    let old_tempo: u16 = SubtensorModule::get_tempo(netuid);

    SubtensorModule::set_tempo_unchecked(netuid, high_tempo);
    run_to_block(n);
    SubtensorModule::set_tempo_unchecked(netuid, old_tempo);
}

pub fn step_epochs(count: u16, netuid: NetUid) {
    for _ in 0..count {
        let blocks_to_next_epoch = SubtensorModule::blocks_until_next_auto_epoch(
            netuid,
            SubtensorModule::get_tempo(netuid),
            SubtensorModule::get_current_block_as_u64(),
        );
        log::info!("Blocks to next epoch: {blocks_to_next_epoch:?}");
        step_block(blocks_to_next_epoch as u16);

        assert!(SubtensorModule::should_run_epoch(
            netuid,
            SubtensorModule::get_current_block_as_u64()
        ));
        step_block(1);
    }
}

/// Increments current block by 1, running all hooks associated with doing so, and asserts
/// that the block number was in fact incremented.
///
/// Returns the new block number.
pub fn next_block() -> u64 {
    let mut block = System::block_number();
    block += 1;
    run_to_block(block);
    assert_eq!(System::block_number(), block);
    block
}

pub fn register_ok_neuron(
    netuid: NetUid,
    hotkey_account_id: U256,
    coldkey_account_id: U256,
    start_nonce: u64,
) {
    let block_number: u64 = SubtensorModule::get_current_block_as_u64();
    let (nonce, work): (u64, Vec<u8>) = SubtensorModule::create_work_for_block_number(
        netuid,
        block_number,
        start_nonce,
        &hotkey_account_id,
    );
    let result = SubtensorModule::register(
        <<Test as frame_system::Config>::RuntimeOrigin>::signed(hotkey_account_id),
        netuid,
        block_number,
        nonce,
        work,
        hotkey_account_id,
        coldkey_account_id,
    );
    assert_ok!(result);
    log::info!(
        "Register ok neuron: netuid: {netuid:?}, coldkey: {hotkey_account_id:?}, hotkey: {coldkey_account_id:?}"
    );
}

pub fn add_network(netuid: NetUid, tempo: u16, _modality: u16) {
    SubtensorModule::init_new_network(netuid, tempo);
    SubtensorModule::set_network_registration_allowed(netuid, true);
    SubtensorModule::set_network_pow_registration_allowed(netuid, true);
    FirstEmissionBlockNumber::<Test>::insert(netuid, 1);
    SubtokenEnabled::<Test>::insert(netuid, true);
}

pub fn add_network_without_emission_block(netuid: NetUid, tempo: u16, _modality: u16) {
    SubtensorModule::init_new_network(netuid, tempo);
    SubtensorModule::set_network_registration_allowed(netuid, true);
    SubtensorModule::set_network_pow_registration_allowed(netuid, true);
}

pub fn add_network_disable_subtoken(netuid: NetUid, tempo: u16, _modality: u16) {
    SubtensorModule::init_new_network(netuid, tempo);
    SubtensorModule::set_network_registration_allowed(netuid, true);
    SubtensorModule::set_network_pow_registration_allowed(netuid, true);
    SubtokenEnabled::<Test>::insert(netuid, false);
}

pub fn add_dynamic_network(hotkey: &U256, coldkey: &U256) -> NetUid {
    let netuid = SubtensorModule::get_next_netuid();
    let lock_cost = SubtensorModule::get_network_lock_cost();
    add_balance_to_coldkey_account(coldkey, lock_cost.into());
    TotalIssuance::<Test>::mutate(|total_issuance| {
        *total_issuance = total_issuance.saturating_add(lock_cost);
    });

    assert_ok!(SubtensorModule::register_network(
        RawOrigin::Signed(*coldkey).into(),
        *hotkey
    ));
    NetworkRegistrationAllowed::<Test>::insert(netuid, true);
    NetworkPowRegistrationAllowed::<Test>::insert(netuid, true);
    FirstEmissionBlockNumber::<Test>::insert(netuid, 0);
    SubtokenEnabled::<Test>::insert(netuid, true);
    netuid
}

pub fn add_dynamic_network_without_emission_block(hotkey: &U256, coldkey: &U256) -> NetUid {
    let netuid = SubtensorModule::get_next_netuid();
    let lock_cost = SubtensorModule::get_network_lock_cost();
    add_balance_to_coldkey_account(coldkey, lock_cost.into());
    TotalIssuance::<Test>::mutate(|total_issuance| {
        *total_issuance = total_issuance.saturating_add(lock_cost);
    });

    assert_ok!(SubtensorModule::register_network(
        RawOrigin::Signed(*coldkey).into(),
        *hotkey
    ));
    NetworkRegistrationAllowed::<Test>::insert(netuid, true);
    NetworkPowRegistrationAllowed::<Test>::insert(netuid, true);
    netuid
}

pub fn add_dynamic_network_disable_commit_reveal(hotkey: &U256, coldkey: &U256) -> NetUid {
    let netuid = add_dynamic_network(hotkey, coldkey);
    SubtensorModule::set_commit_reveal_weights_enabled(netuid, false);
    netuid
}

pub fn add_network_disable_commit_reveal(netuid: NetUid, tempo: u16, _modality: u16) {
    add_network(netuid, tempo, _modality);
    SubtensorModule::set_commit_reveal_weights_enabled(netuid, false);
}

// Helper function to set up a neuron with stake
pub fn setup_neuron_with_stake(netuid: NetUid, hotkey: U256, coldkey: U256, stake: TaoBalance) {
    register_ok_neuron(netuid, hotkey, coldkey, stake.into());
    increase_stake_on_coldkey_hotkey_account(&coldkey, &hotkey, stake, netuid);
}

pub fn wait_set_pending_children_cooldown(netuid: NetUid) {
    let cooldown = DefaultPendingCooldown::<Test>::get();
    step_block(cooldown as u16); // Wait for cooldown to pass
    step_epochs(1, netuid); // Run next epoch
}

pub fn wait_and_set_pending_children(netuid: NetUid) {
    let original_block = System::block_number();
    wait_set_pending_children_cooldown(netuid);
    SubtensorModule::do_set_pending_children(netuid);
    System::set_block_number(original_block);
}

pub fn mock_schedule_children(
    coldkey: &U256,
    parent: &U256,
    netuid: NetUid,
    child_vec: &[(u64, U256)],
) {
    // Set minimum stake for setting children
    StakeThreshold::<Test>::put(0);

    // Set initial parent-child relationship
    assert_ok!(SubtensorModule::do_schedule_children(
        RuntimeOrigin::signed(*coldkey),
        *parent,
        netuid,
        child_vec.to_vec()
    ));
}

pub fn mock_set_children(coldkey: &U256, parent: &U256, netuid: NetUid, child_vec: &[(u64, U256)]) {
    mock_schedule_children(coldkey, parent, netuid, child_vec);
    wait_and_set_pending_children(netuid);
}

pub fn mock_set_children_no_epochs(netuid: NetUid, parent: &U256, child_vec: &[(u64, U256)]) {
    let backup_block = SubtensorModule::get_current_block_as_u64();
    PendingChildKeys::<Test>::insert(netuid, parent, (child_vec, 0));
    FirstEmissionBlockNumber::<Test>::insert(netuid, 0);
    let cooldown = PendingChildKeyCooldown::<Test>::get();
    System::set_block_number(cooldown + 1);
    SubtensorModule::do_set_pending_children(netuid);
    System::set_block_number(backup_block);
}

// Helper function to wait for the rate limit
pub fn step_rate_limit(transaction_type: &TransactionType, netuid: NetUid) {
    // Check rate limit
    let limit = transaction_type.rate_limit_on_subnet::<Test>(netuid);

    // Step that many blocks
    step_block(limit as u16);
}

/// Helper function to increase stake on a coldkey-hotkey pair via the public add_stake extrinsic.
pub fn increase_stake_on_coldkey_hotkey_account(
    coldkey: &U256,
    hotkey: &U256,
    tao_staked: TaoBalance,
    netuid: NetUid,
) {
    // Ensure the coldkey has enough balance
    add_balance_to_coldkey_account(coldkey, tao_staked.into());
    assert_ok!(SubtensorModule::add_stake(
        RuntimeOrigin::signed(*coldkey),
        *hotkey,
        netuid,
        tao_staked,
    ));
}

/// Increases the stake on the hotkey account under its owning coldkey.
///
/// # Arguments
/// * `hotkey` - The hotkey account ID.
/// * `increment` - The amount to be incremented.
pub fn increase_stake_on_hotkey_account(hotkey: &U256, increment: TaoBalance, netuid: NetUid) {
    increase_stake_on_coldkey_hotkey_account(
        &SubtensorModule::get_owning_coldkey_for_hotkey(hotkey),
        hotkey,
        increment,
        netuid,
    );
}

pub fn setup_reserves(netuid: NetUid, tao: TaoBalance, alpha: AlphaBalance) {
    SubnetTAO::<Test>::set(netuid, tao);
    SubnetAlphaIn::<Test>::set(netuid, alpha);
}

pub fn swap_tao_to_alpha(netuid: NetUid, tao: TaoBalance) -> (AlphaBalance, u64) {
    if netuid.is_root() {
        return (tao.to_u64().into(), 0);
    }

    let order = GetAlphaForTao::<Test>::with_amount(tao);
    let result = <Test as pallet_subtensor::Config>::SwapInterface::swap(
        netuid.into(),
        order,
        <Test as pallet_subtensor::Config>::SwapInterface::max_price(),
        false,
        true,
    );

    assert_ok!(&result);

    let result = result.unwrap();

    // we don't want to have silent 0 comparisons in tests
    assert!(result.amount_paid_out > AlphaBalance::ZERO);

    (result.amount_paid_out, result.fee_paid.into())
}

pub fn swap_alpha_to_tao_ext(
    netuid: NetUid,
    alpha: AlphaBalance,
    drop_fees: bool,
) -> (TaoBalance, u64) {
    if netuid.is_root() {
        return (alpha.to_u64().into(), 0);
    }

    println!(
        "<Test as pallet_subtensor::Config>::SwapInterface::min_price() = {:?}",
        <Test as pallet_subtensor::Config>::SwapInterface::min_price::<TaoBalance>()
    );

    let order = GetTaoForAlpha::<Test>::with_amount(alpha);
    let result = <Test as pallet_subtensor::Config>::SwapInterface::swap(
        netuid.into(),
        order,
        <Test as pallet_subtensor::Config>::SwapInterface::min_price(),
        drop_fees,
        true,
    );

    assert_ok!(&result);

    let result = result.unwrap();

    // we don't want to have silent 0 comparisons in tests
    assert!(!result.amount_paid_out.is_zero());

    (result.amount_paid_out, result.fee_paid.into())
}

pub fn swap_alpha_to_tao(netuid: NetUid, alpha: AlphaBalance) -> (TaoBalance, u64) {
    swap_alpha_to_tao_ext(netuid, alpha, false)
}

pub fn last_event() -> RuntimeEvent {
    System::events().pop().expect("RuntimeEvent expected").event
}

pub fn assert_last_event<T: frame_system::pallet::Config>(
    generic_event: <T as frame_system::pallet::Config>::RuntimeEvent,
) {
    frame_system::Pallet::<T>::assert_last_event(generic_event.into());
}

pub fn commit_dummy(who: U256, netuid: NetUid) {
    SubtensorModule::set_weights_set_rate_limit(netuid, 0);

    // any 32‑byte value is fine; hash is never opened
    let hash = H256::from_low_u64_be(0xDEAD_BEEF);
    assert_ok!(SubtensorModule::do_commit_weights(
        RuntimeOrigin::signed(who),
        netuid,
        hash
    ));
}

pub fn sf_to_u128(sf: &SafeFloat) -> u128 {
    let alpha_f64: f64 = sf.into();
    alpha_f64 as u128
}

pub fn sf_from_u64(val: u64) -> SafeFloat {
    SafeFloat::from(val)
}
