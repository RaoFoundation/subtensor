//! Fee baseline guard.
//!
//! For every dispatchable, an `insta` snapshot pins the fee (rao) a 100-byte extrinsic
//! with one unit of every argument is quoted at, under
//! `runtime/tests/snapshots/fee_baseline__dispatchable_fee_matches_snapshot@<case>.snap`.
//! Fees may rise or fall by up to 20% of their snapshot value (inclusive).
//! Changes beyond that range require an explicit snapshot update and review.
//! Signed Ethereum transactions also pin actual gas fees for native transfers
//! and balance-transfer, staking V2 (root add/remove), and proxy precompiles.
//! Transfer/stake principal and reserved proxy deposits are excluded from fees.
//!
//! Regenerate snapshots after a deliberate fee change:
//! `INSTA_UPDATE=always cargo test -p node-subtensor-runtime --test fee_baseline`
//! then review the diff under `runtime/tests/snapshots/` and commit the changed files.
//!
//! By default, running the tests (except in CI) will generate new snapshots for any failing
//! snapshot tests, saved as `runtime/tests/snapshots/*.snap.new`.

#![allow(clippy::expect_used)]

use frame_support::dispatch::GetDispatchInfo;
use insta::assert_snapshot;
use insta::{with_settings, Comparator, Snapshot};
use node_subtensor_runtime::transaction_payment_wrapper::{fee_dispatch_info, FeeWeightDiscount};
use node_subtensor_runtime::{
    BuildStorage, Runtime, RuntimeCall, RuntimeGenesisConfig, System, SystemCall,
    TransactionPayment,
};
use rstest::{fixture, rstest, Context};
use subtensor_runtime_common::{AlphaBalance, MechId, NetUid, TaoBalance, Token};

/// Encoded extrinsic length every pin is quoted at.
const LEN: u32 = 100;
/// Finney has more than `MAX_UNSTAKE_ALL_LEGS` subnets; price bulk unstakes at the cap.
const NETWORKS: u16 = 128;

#[derive(Clone)]
struct FeeToleranceComparator;

fn fee_matches_baseline(reference: &str, current: &str) -> bool {
    match (
        reference.trim().parse::<u64>(),
        current.trim().parse::<u64>(),
    ) {
        (Ok(reference), Ok(current)) => {
            // 20% is one fifth. Widen before multiplying to avoid overflow and
            // compare exactly, without rounding fractional RAO or using floats.
            u128::from(current.abs_diff(reference)).saturating_mul(5) <= u128::from(reference)
        }
        _ => false,
    }
}

impl Comparator for FeeToleranceComparator {
    fn matches(&self, reference: &Snapshot, current: &Snapshot) -> bool {
        match (reference.contents().as_text(), current.contents().as_text()) {
            (Some(reference), Some(current)) => {
                fee_matches_baseline(&reference.to_string(), &current.to_string())
            }
            _ => false,
        }
    }

    fn dyn_clone(&self) -> Box<dyn Comparator> {
        Box::new(self.clone())
    }
}

#[rstest]
#[case("100", "80", true)]
#[case("100", "120", true)]
#[case("100", "79", false)]
#[case("100", "121", false)]
#[case("100", "100", true)]
#[case("6", "5", true)]
#[case("6", "7", true)]
#[case("6", "4", false)]
#[case("6", "8", false)]
#[case("0", "0", true)]
#[case("0", "1", false)]
#[case("18446744073709551615", "14757395258967641292", true)]
#[case("18446744073709551615", "14757395258967641291", false)]
#[case("18446744073709551615", "18446744073709551615", true)]
#[case("0", "18446744073709551615", false)]
#[case("bad", "100", false)]
#[case("100", "bad", false)]
fn fee_tolerance_boundaries(
    #[case] reference: &str,
    #[case] current: &str,
    #[case] expected: bool,
) {
    assert_eq!(fee_matches_baseline(reference, current), expected);
}

#[fixture]
fn new_test_ext() -> sp_io::TestExternalities {
    let mut ext: sp_io::TestExternalities = RuntimeGenesisConfig::default()
        .build_storage()
        .expect("runtime genesis storage builds")
        .into();
    ext.execute_with(|| {
        System::set_block_number(1);
        pallet_subtensor::TotalNetworks::<Runtime>::put(NETWORKS);
    });
    ext
}

/// What the fee wrapper bills for `call`: declared weight minus every discount, then the
/// same `compute_fee` the node's `payment_queryInfo` uses.
fn quoted_fee_rao(call: &RuntimeCall) -> u64 {
    let info = call.get_dispatch_info();
    let fee_info = fee_dispatch_info(&info, Runtime::fee_weight_discount(call, &info));
    TransactionPayment::compute_fee(LEN, &fee_info, TaoBalance::new(0)).to_u64()
}

// Deprecated swap argument types have private fields and no public constructors.
// Decode the original one-valued fixtures until these legacy calls are removed.
fn legacy_tick_index() -> pallet_subtensor_swap::TickIndex {
    codec::Decode::decode(&mut &1i32.to_le_bytes()[..]).expect("valid legacy tick index")
}

fn legacy_position_id() -> pallet_subtensor_swap::PositionId {
    codec::Decode::decode(&mut &1u128.to_le_bytes()[..]).expect("valid legacy position ID")
}

#[rstest]
#[case::system_remark(RuntimeCall::System(SystemCall::remark { remark: vec![1] }))]
#[case::system_set_heap_pages(RuntimeCall::System(SystemCall::set_heap_pages { pages: 1 }))]
#[case::system_set_code(RuntimeCall::System(SystemCall::set_code { code: vec![1] }))]
#[case::system_set_code_without_checks(RuntimeCall::System(SystemCall::set_code_without_checks { code: vec![1] }))]
#[case::system_set_storage(RuntimeCall::System(SystemCall::set_storage { items: vec![(vec![1], vec![1])] }))]
#[case::system_kill_storage(RuntimeCall::System(SystemCall::kill_storage { keys: vec![vec![1]] }))]
#[case::system_kill_prefix(RuntimeCall::System(SystemCall::kill_prefix { prefix: vec![1], subkeys: 1 }))]
#[case::system_remark_with_event(RuntimeCall::System(SystemCall::remark_with_event { remark: vec![1] }))]
#[case::system_authorize_upgrade(RuntimeCall::System(SystemCall::authorize_upgrade { code_hash: [1u8; 32].into() }))]
#[case::system_authorize_upgrade_without_checks(RuntimeCall::System(SystemCall::authorize_upgrade_without_checks { code_hash: [1u8; 32].into() }))]
#[case::system_apply_authorized_upgrade(RuntimeCall::System(SystemCall::apply_authorized_upgrade { code: vec![1] }))]
#[case::timestamp_set(RuntimeCall::Timestamp(pallet_timestamp::Call::set { now: 1 }))]
#[case::grandpa_note_stalled(RuntimeCall::Grandpa(pallet_grandpa::Call::note_stalled { delay: 1, best_finalized_block_number: 1 }))]
#[case::balances_transfer_allow_death(RuntimeCall::Balances(pallet_balances::Call::transfer_allow_death { dest: sp_runtime::MultiAddress::Id([1u8; 32].into()), value: TaoBalance::new(1) }))]
#[case::balances_force_transfer(RuntimeCall::Balances(pallet_balances::Call::force_transfer { source: sp_runtime::MultiAddress::Id([1u8; 32].into()), dest: sp_runtime::MultiAddress::Id([1u8; 32].into()), value: TaoBalance::new(1) }))]
#[case::balances_transfer_keep_alive(RuntimeCall::Balances(pallet_balances::Call::transfer_keep_alive { dest: sp_runtime::MultiAddress::Id([1u8; 32].into()), value: TaoBalance::new(1) }))]
#[case::balances_transfer_all(RuntimeCall::Balances(pallet_balances::Call::transfer_all { dest: sp_runtime::MultiAddress::Id([1u8; 32].into()), keep_alive: false }))]
#[case::balances_force_unreserve(RuntimeCall::Balances(pallet_balances::Call::force_unreserve { who: sp_runtime::MultiAddress::Id([1u8; 32].into()), amount: TaoBalance::new(1) }))]
#[case::balances_upgrade_accounts(RuntimeCall::Balances(pallet_balances::Call::upgrade_accounts { who: vec![[1u8; 32].into()] }))]
#[case::balances_force_set_balance(RuntimeCall::Balances(pallet_balances::Call::force_set_balance { who: sp_runtime::MultiAddress::Id([1u8; 32].into()), new_free: TaoBalance::new(1) }))]
#[case::balances_force_adjust_total_issuance(RuntimeCall::Balances(pallet_balances::Call::force_adjust_total_issuance { direction: pallet_balances::AdjustmentDirection::Increase, delta: TaoBalance::new(1) }))]
#[case::balances_burn(RuntimeCall::Balances(pallet_balances::Call::burn { value: TaoBalance::new(1), keep_alive: false }))]
#[case::subtensor_module_set_weights(RuntimeCall::SubtensorModule(pallet_subtensor::Call::set_weights { netuid: NetUid::from(1u16), dests: vec![1], weights: vec![1], version_key: 1 }))]
#[case::subtensor_module_set_mechanism_weights(RuntimeCall::SubtensorModule(pallet_subtensor::Call::set_mechanism_weights { netuid: NetUid::from(1u16), mecid: MechId::from(1u8), dests: vec![1], weights: vec![1], version_key: 1 }))]
#[case::subtensor_module_batch_set_weights(RuntimeCall::SubtensorModule(pallet_subtensor::Call::batch_set_weights { netuids: vec![codec::Compact(NetUid::from(1u16))], weights: vec![vec![(codec::Compact(1u16), codec::Compact(1u16))]], version_keys: vec![codec::Compact(1u64)] }))]
#[case::subtensor_module_commit_weights(RuntimeCall::SubtensorModule(pallet_subtensor::Call::commit_weights { netuid: NetUid::from(1u16), commit_hash: [1u8; 32].into() }))]
#[case::subtensor_module_commit_mechanism_weights(RuntimeCall::SubtensorModule(pallet_subtensor::Call::commit_mechanism_weights { netuid: NetUid::from(1u16), mecid: MechId::from(1u8), commit_hash: [1u8; 32].into() }))]
#[case::subtensor_module_batch_commit_weights(RuntimeCall::SubtensorModule(pallet_subtensor::Call::batch_commit_weights { netuids: vec![codec::Compact(NetUid::from(1u16))], commit_hashes: vec![[1u8; 32].into()] }))]
#[case::subtensor_module_reveal_weights(RuntimeCall::SubtensorModule(pallet_subtensor::Call::reveal_weights { netuid: NetUid::from(1u16), uids: vec![1], values: vec![1], salt: vec![1], version_key: 1 }))]
#[case::subtensor_module_reveal_mechanism_weights(RuntimeCall::SubtensorModule(pallet_subtensor::Call::reveal_mechanism_weights { netuid: NetUid::from(1u16), mecid: MechId::from(1u8), uids: vec![1], values: vec![1], salt: vec![1], version_key: 1 }))]
#[case::subtensor_module_commit_crv3_mechanism_weights(RuntimeCall::SubtensorModule(pallet_subtensor::Call::commit_crv3_mechanism_weights { netuid: NetUid::from(1u16), mecid: MechId::from(1u8), commit: (vec![1]).try_into().expect("bounded"), reveal_round: 1 }))]
#[case::subtensor_module_batch_reveal_weights(RuntimeCall::SubtensorModule(pallet_subtensor::Call::batch_reveal_weights { netuid: NetUid::from(1u16), uids_list: vec![vec![1]], values_list: vec![vec![1]], salts_list: vec![vec![1]], version_keys: vec![1] }))]
#[case::subtensor_module_decrease_take(RuntimeCall::SubtensorModule(pallet_subtensor::Call::decrease_take { hotkey: [1u8; 32].into(), take: sp_runtime::PerU16::from_parts(1) }))]
#[case::subtensor_module_increase_take(RuntimeCall::SubtensorModule(pallet_subtensor::Call::increase_take { hotkey: [1u8; 32].into(), take: sp_runtime::PerU16::from_parts(1) }))]
#[case::subtensor_module_add_stake(RuntimeCall::SubtensorModule(pallet_subtensor::Call::add_stake { hotkey: [1u8; 32].into(), netuid: NetUid::from(1u16), amount_staked: TaoBalance::new(1) }))]
#[case::subtensor_module_remove_stake(RuntimeCall::SubtensorModule(pallet_subtensor::Call::remove_stake { hotkey: [1u8; 32].into(), netuid: NetUid::from(1u16), amount_unstaked: AlphaBalance::new(1) }))]
#[case::subtensor_module_serve_axon(RuntimeCall::SubtensorModule(pallet_subtensor::Call::serve_axon { netuid: NetUid::from(1u16), version: 1, ip: 1, port: 1, ip_type: 1, protocol: 1, placeholder1: 1, placeholder2: 1 }))]
#[case::subtensor_module_serve_axon_tls(RuntimeCall::SubtensorModule(pallet_subtensor::Call::serve_axon_tls { netuid: NetUid::from(1u16), version: 1, ip: 1, port: 1, ip_type: 1, protocol: 1, placeholder1: 1, placeholder2: 1, certificate: vec![1] }))]
#[case::subtensor_module_serve_prometheus(RuntimeCall::SubtensorModule(pallet_subtensor::Call::serve_prometheus { netuid: NetUid::from(1u16), version: 1, ip: 1, port: 1, ip_type: 1 }))]
#[case::subtensor_module_register(RuntimeCall::SubtensorModule(pallet_subtensor::Call::register { netuid: NetUid::from(1u16), block_number: 1, nonce: 1, work: vec![1], hotkey: [1u8; 32].into(), coldkey: [1u8; 32].into() }))]
#[case::subtensor_module_root_register(RuntimeCall::SubtensorModule(pallet_subtensor::Call::root_register { hotkey: [1u8; 32].into() }))]
#[case::subtensor_module_burned_register(RuntimeCall::SubtensorModule(pallet_subtensor::Call::burned_register { netuid: NetUid::from(1u16), hotkey: [1u8; 32].into() }))]
#[case::subtensor_module_swap_hotkey(RuntimeCall::SubtensorModule(pallet_subtensor::Call::swap_hotkey { hotkey: [1u8; 32].into(), new_hotkey: [1u8; 32].into(), netuid: None }))]
#[case::subtensor_module_swap_hotkey_v2(RuntimeCall::SubtensorModule(pallet_subtensor::Call::swap_hotkey_v2 { hotkey: [1u8; 32].into(), new_hotkey: [1u8; 32].into(), netuid: None, keep_stake: false }))]
#[case::subtensor_module_swap_coldkey(RuntimeCall::SubtensorModule(pallet_subtensor::Call::swap_coldkey { old_coldkey: [1u8; 32].into(), new_coldkey: [1u8; 32].into(), swap_cost: TaoBalance::new(1) }))]
#[case::subtensor_module_set_childkey_take(RuntimeCall::SubtensorModule(pallet_subtensor::Call::set_childkey_take { hotkey: [1u8; 32].into(), netuid: NetUid::from(1u16), take: sp_runtime::PerU16::from_parts(1) }))]
#[case::subtensor_module_sudo_set_tx_childkey_take_rate_limit(RuntimeCall::SubtensorModule(pallet_subtensor::Call::sudo_set_tx_childkey_take_rate_limit { tx_rate_limit: 1 }))]
#[case::subtensor_module_sudo_set_min_childkey_take(RuntimeCall::SubtensorModule(pallet_subtensor::Call::sudo_set_min_childkey_take { take: sp_runtime::PerU16::from_parts(1) }))]
#[case::subtensor_module_sudo_set_max_childkey_take(RuntimeCall::SubtensorModule(pallet_subtensor::Call::sudo_set_max_childkey_take { take: sp_runtime::PerU16::from_parts(1) }))]
#[case::subtensor_module_register_network(RuntimeCall::SubtensorModule(pallet_subtensor::Call::register_network { hotkey: [1u8; 32].into() }))]
#[case::subtensor_module_dissolve_network(RuntimeCall::SubtensorModule(pallet_subtensor::Call::dissolve_network { coldkey: [1u8; 32].into(), netuid: NetUid::from(1u16) }))]
#[case::subtensor_module_set_children(RuntimeCall::SubtensorModule(pallet_subtensor::Call::set_children { hotkey: [1u8; 32].into(), netuid: NetUid::from(1u16), children: vec![(1, [1u8; 32].into())] }))]
#[case::subtensor_module_schedule_swap_coldkey(RuntimeCall::SubtensorModule(pallet_subtensor::Call::schedule_swap_coldkey { new_coldkey: [1u8; 32].into() }))]
#[case::subtensor_module_set_identity(RuntimeCall::SubtensorModule(pallet_subtensor::Call::set_identity { name: vec![1], url: vec![1], github_repo: vec![1], image: vec![1], discord: vec![1], description: vec![1], additional: vec![1] }))]
#[case::subtensor_module_set_subnet_identity(RuntimeCall::SubtensorModule(pallet_subtensor::Call::set_subnet_identity { netuid: NetUid::from(1u16), subnet_name: vec![1], github_repo: vec![1], subnet_contact: vec![1], subnet_url: vec![1], discord: vec![1], description: vec![1], logo_url: vec![1], additional: vec![1] }))]
#[case::subtensor_module_register_network_with_identity(RuntimeCall::SubtensorModule(pallet_subtensor::Call::register_network_with_identity { hotkey: [1u8; 32].into(), identity: None }))]
#[case::subtensor_module_unstake_all(RuntimeCall::SubtensorModule(pallet_subtensor::Call::unstake_all { hotkey: [1u8; 32].into() }))]
#[case::subtensor_module_unstake_all_alpha(RuntimeCall::SubtensorModule(pallet_subtensor::Call::unstake_all_alpha { hotkey: [1u8; 32].into() }))]
#[case::subtensor_module_move_stake(RuntimeCall::SubtensorModule(pallet_subtensor::Call::move_stake { origin_hotkey: [1u8; 32].into(), destination_hotkey: [1u8; 32].into(), origin_netuid: NetUid::from(1u16), destination_netuid: NetUid::from(1u16), alpha_amount: AlphaBalance::new(1) }))]
#[case::subtensor_module_transfer_stake(RuntimeCall::SubtensorModule(pallet_subtensor::Call::transfer_stake { destination_coldkey: [1u8; 32].into(), hotkey: [1u8; 32].into(), origin_netuid: NetUid::from(1u16), destination_netuid: NetUid::from(1u16), alpha_amount: AlphaBalance::new(1) }))]
#[case::subtensor_module_swap_stake(RuntimeCall::SubtensorModule(pallet_subtensor::Call::swap_stake { hotkey: [1u8; 32].into(), origin_netuid: NetUid::from(1u16), destination_netuid: NetUid::from(1u16), alpha_amount: AlphaBalance::new(1) }))]
#[case::subtensor_module_add_stake_limit(RuntimeCall::SubtensorModule(pallet_subtensor::Call::add_stake_limit { hotkey: [1u8; 32].into(), netuid: NetUid::from(1u16), amount_staked: TaoBalance::new(1), limit_price: TaoBalance::new(1), allow_partial: false }))]
#[case::subtensor_module_remove_stake_limit(RuntimeCall::SubtensorModule(pallet_subtensor::Call::remove_stake_limit { hotkey: [1u8; 32].into(), netuid: NetUid::from(1u16), amount_unstaked: AlphaBalance::new(1), limit_price: TaoBalance::new(1), allow_partial: false }))]
#[case::subtensor_module_swap_stake_limit(RuntimeCall::SubtensorModule(pallet_subtensor::Call::swap_stake_limit { hotkey: [1u8; 32].into(), origin_netuid: NetUid::from(1u16), destination_netuid: NetUid::from(1u16), alpha_amount: AlphaBalance::new(1), limit_price: TaoBalance::new(1), allow_partial: false }))]
#[case::subtensor_module_move_stake_limit(RuntimeCall::SubtensorModule(pallet_subtensor::Call::move_stake_limit { origin_hotkey: [1u8; 32].into(), destination_hotkey: [1u8; 32].into(), origin_netuid: NetUid::from(1u16), destination_netuid: NetUid::from(1u16), alpha_amount: AlphaBalance::new(1), limit_price: TaoBalance::new(1), allow_partial: false }))]
#[case::subtensor_module_try_associate_hotkey(RuntimeCall::SubtensorModule(pallet_subtensor::Call::try_associate_hotkey { hotkey: [1u8; 32].into() }))]
#[case::subtensor_module_start_call(RuntimeCall::SubtensorModule(pallet_subtensor::Call::start_call { netuid: NetUid::from(1u16) }))]
#[case::subtensor_module_associate_evm_key(RuntimeCall::SubtensorModule(pallet_subtensor::Call::associate_evm_key { netuid: NetUid::from(1u16), evm_key: [1u8; 20].into(), block_number: 1, signature: [1u8; 65].into() }))]
#[case::subtensor_module_recycle_alpha(RuntimeCall::SubtensorModule(pallet_subtensor::Call::recycle_alpha { hotkey: [1u8; 32].into(), amount: AlphaBalance::new(1), netuid: NetUid::from(1u16) }))]
#[case::subtensor_module_burn_alpha(RuntimeCall::SubtensorModule(pallet_subtensor::Call::burn_alpha { hotkey: [1u8; 32].into(), amount: AlphaBalance::new(1), netuid: NetUid::from(1u16) }))]
#[case::subtensor_module_set_pending_childkey_cooldown(RuntimeCall::SubtensorModule(pallet_subtensor::Call::set_pending_childkey_cooldown { cooldown: 1 }))]
#[case::subtensor_module_remove_stake_full_limit(RuntimeCall::SubtensorModule(pallet_subtensor::Call::remove_stake_full_limit { hotkey: [1u8; 32].into(), netuid: NetUid::from(1u16), limit_price: None }))]
#[case::subtensor_module_register_leased_network(RuntimeCall::SubtensorModule(pallet_subtensor::Call::register_leased_network { emissions_share: sp_runtime::Percent::from_parts(1), end_block: None }))]
#[case::subtensor_module_terminate_lease(RuntimeCall::SubtensorModule(pallet_subtensor::Call::terminate_lease { lease_id: 1, hotkey: [1u8; 32].into() }))]
#[case::subtensor_module_update_symbol(RuntimeCall::SubtensorModule(pallet_subtensor::Call::update_symbol { netuid: NetUid::from(1u16), symbol: vec![1] }))]
#[case::subtensor_module_commit_timelocked_weights(RuntimeCall::SubtensorModule(pallet_subtensor::Call::commit_timelocked_weights { netuid: NetUid::from(1u16), commit: (vec![1]).try_into().expect("bounded"), reveal_round: 1, commit_reveal_version: 1 }))]
#[case::subtensor_module_set_coldkey_auto_stake_hotkey(RuntimeCall::SubtensorModule(pallet_subtensor::Call::set_coldkey_auto_stake_hotkey { netuid: NetUid::from(1u16), hotkey: [1u8; 32].into() }))]
#[case::subtensor_module_commit_timelocked_mechanism_weights(RuntimeCall::SubtensorModule(pallet_subtensor::Call::commit_timelocked_mechanism_weights { netuid: NetUid::from(1u16), mecid: MechId::from(1u8), commit: (vec![1]).try_into().expect("bounded"), reveal_round: 1, commit_reveal_version: 1 }))]
#[case::subtensor_module_root_dissolve_network(RuntimeCall::SubtensorModule(pallet_subtensor::Call::root_dissolve_network { netuid: NetUid::from(1u16) }))]
#[case::subtensor_module_claim_root(RuntimeCall::SubtensorModule(pallet_subtensor::Call::claim_root { subnets: std::collections::BTreeSet::from([NetUid::from(1u16)]) }))]
#[case::subtensor_module_claim_root_with_hotkey(RuntimeCall::SubtensorModule(pallet_subtensor::Call::claim_root_with_hotkey { hotkey: [1u8; 32].into() }))]
#[case::subtensor_module_stake_into_basket(RuntimeCall::SubtensorModule(pallet_subtensor::Call::stake_into_basket { hotkey: [1u8; 32].into(), amount_staked: TaoBalance::new(1) }))]
#[case::subtensor_module_swap_basket(RuntimeCall::SubtensorModule(pallet_subtensor::Call::swap_basket { hotkey: [1u8; 32].into(), origin_netuid: NetUid::from(1u16), destination_netuid: NetUid::from(1u16), amount: AlphaBalance::new(1), min_amount_out: 1 }))]
#[case::subtensor_module_sudo_set_root_claim_threshold(RuntimeCall::SubtensorModule(pallet_subtensor::Call::sudo_set_root_claim_threshold { netuid: NetUid::from(1u16), new_value: 1 }))]
#[case::subtensor_module_announce_coldkey_swap(RuntimeCall::SubtensorModule(pallet_subtensor::Call::announce_coldkey_swap { new_coldkey_hash: [1u8; 32].into() }))]
#[case::subtensor_module_swap_coldkey_announced(RuntimeCall::SubtensorModule(pallet_subtensor::Call::swap_coldkey_announced { new_coldkey: [1u8; 32].into() }))]
#[case::subtensor_module_dispute_coldkey_swap(RuntimeCall::SubtensorModule(pallet_subtensor::Call::dispute_coldkey_swap {}))]
#[case::subtensor_module_reset_coldkey_swap(RuntimeCall::SubtensorModule(pallet_subtensor::Call::reset_coldkey_swap { coldkey: [1u8; 32].into() }))]
#[case::subtensor_module_enable_voting_power_tracking(RuntimeCall::SubtensorModule(pallet_subtensor::Call::enable_voting_power_tracking { netuid: NetUid::from(1u16) }))]
#[case::subtensor_module_disable_voting_power_tracking(RuntimeCall::SubtensorModule(pallet_subtensor::Call::disable_voting_power_tracking { netuid: NetUid::from(1u16) }))]
#[case::subtensor_module_sudo_set_voting_power_ema_alpha(RuntimeCall::SubtensorModule(pallet_subtensor::Call::sudo_set_voting_power_ema_alpha { netuid: NetUid::from(1u16), alpha: 1 }))]
#[case::subtensor_module_add_stake_burn(RuntimeCall::SubtensorModule(pallet_subtensor::Call::add_stake_burn { hotkey: [1u8; 32].into(), netuid: NetUid::from(1u16), amount: TaoBalance::new(1), limit: None }))]
#[case::subtensor_module_clear_coldkey_swap_announcement(RuntimeCall::SubtensorModule(pallet_subtensor::Call::clear_coldkey_swap_announcement {}))]
#[case::subtensor_module_register_limit(RuntimeCall::SubtensorModule(pallet_subtensor::Call::register_limit { netuid: NetUid::from(1u16), hotkey: [1u8; 32].into(), limit_price: 1 }))]
#[case::subtensor_module_set_auto_parent_delegation_enabled(RuntimeCall::SubtensorModule(pallet_subtensor::Call::set_auto_parent_delegation_enabled { hotkey: [1u8; 32].into(), enabled: false }))]
#[case::subtensor_module_lock_stake(RuntimeCall::SubtensorModule(pallet_subtensor::Call::lock_stake { hotkey: [1u8; 32].into(), netuid: NetUid::from(1u16), amount: AlphaBalance::new(1) }))]
#[case::subtensor_module_move_lock(RuntimeCall::SubtensorModule(pallet_subtensor::Call::move_lock { destination_hotkey: [1u8; 32].into(), netuid: NetUid::from(1u16) }))]
#[case::subtensor_module_set_perpetual_lock(RuntimeCall::SubtensorModule(pallet_subtensor::Call::set_perpetual_lock { netuid: NetUid::from(1u16), enabled: false }))]
#[case::subtensor_module_set_tempo(RuntimeCall::SubtensorModule(pallet_subtensor::Call::set_tempo { netuid: NetUid::from(1u16), tempo: 1 }))]
#[case::subtensor_module_set_activity_cutoff_factor(RuntimeCall::SubtensorModule(pallet_subtensor::Call::set_activity_cutoff_factor { netuid: NetUid::from(1u16), factor_milli: 1 }))]
#[case::subtensor_module_trigger_epoch(RuntimeCall::SubtensorModule(pallet_subtensor::Call::trigger_epoch { netuid: NetUid::from(1u16) }))]
#[case::subtensor_module_set_reject_locked_alpha(RuntimeCall::SubtensorModule(pallet_subtensor::Call::set_reject_locked_alpha { enabled: false }))]
#[case::subtensor_module_transfer_stake_and_hotkey(RuntimeCall::SubtensorModule(pallet_subtensor::Call::transfer_stake_and_hotkey { destination_coldkey: [1u8; 32].into(), origin_hotkey: [1u8; 32].into(), destination_hotkey: [1u8; 32].into(), origin_netuid: NetUid::from(1u16), destination_netuid: NetUid::from(1u16), alpha_amount: AlphaBalance::new(1) }))]
#[case::subtensor_module_add_collateral(RuntimeCall::SubtensorModule(pallet_subtensor::Call::add_collateral { netuid: NetUid::from(1u16), hotkey: [1u8; 32].into(), alpha: AlphaBalance::new(1), limit_price: TaoBalance::new(1) }))]
#[case::subtensor_module_set_min_collateral(RuntimeCall::SubtensorModule(pallet_subtensor::Call::set_min_collateral { netuid: NetUid::from(1u16), hotkey: [1u8; 32].into(), min_locked: AlphaBalance::new(1) }))]
#[case::utility_batch(RuntimeCall::Utility(pallet_subtensor_utility::Call::batch { calls: vec![RuntimeCall::System(SystemCall::remark { remark: vec![1] })] }))]
#[case::utility_as_derivative(RuntimeCall::Utility(pallet_subtensor_utility::Call::as_derivative { index: 1, call: Box::new(RuntimeCall::System(SystemCall::remark { remark: vec![1] })) }))]
#[case::utility_batch_all(RuntimeCall::Utility(pallet_subtensor_utility::Call::batch_all { calls: vec![RuntimeCall::System(SystemCall::remark { remark: vec![1] })] }))]
#[case::utility_dispatch_as(RuntimeCall::Utility(pallet_subtensor_utility::Call::dispatch_as { as_origin: Box::new(frame_system::RawOrigin::Root.into()), call: Box::new(RuntimeCall::System(SystemCall::remark { remark: vec![1] })) }))]
#[case::utility_force_batch(RuntimeCall::Utility(pallet_subtensor_utility::Call::force_batch { calls: vec![RuntimeCall::System(SystemCall::remark { remark: vec![1] })] }))]
#[case::utility_with_weight(RuntimeCall::Utility(pallet_subtensor_utility::Call::with_weight { call: Box::new(RuntimeCall::System(SystemCall::remark { remark: vec![1] })), weight: frame_support::weights::Weight::from_parts(1, 1) }))]
#[case::utility_if_else(RuntimeCall::Utility(pallet_subtensor_utility::Call::if_else { main: Box::new(RuntimeCall::System(SystemCall::remark { remark: vec![1] })), fallback: Box::new(RuntimeCall::System(SystemCall::remark { remark: vec![1] })) }))]
#[case::utility_dispatch_as_fallible(RuntimeCall::Utility(pallet_subtensor_utility::Call::dispatch_as_fallible { as_origin: Box::new(frame_system::RawOrigin::Root.into()), call: Box::new(RuntimeCall::System(SystemCall::remark { remark: vec![1] })) }))]
#[case::sudo_sudo(RuntimeCall::Sudo(pallet_sudo::Call::sudo { call: Box::new(RuntimeCall::System(SystemCall::remark { remark: vec![1] })) }))]
#[case::sudo_sudo_unchecked_weight(RuntimeCall::Sudo(pallet_sudo::Call::sudo_unchecked_weight { call: Box::new(RuntimeCall::System(SystemCall::remark { remark: vec![1] })), weight: frame_support::weights::Weight::from_parts(1, 1) }))]
#[case::sudo_set_key(RuntimeCall::Sudo(pallet_sudo::Call::set_key { new: sp_runtime::MultiAddress::Id([1u8; 32].into()) }))]
#[case::sudo_sudo_as(RuntimeCall::Sudo(pallet_sudo::Call::sudo_as { who: sp_runtime::MultiAddress::Id([1u8; 32].into()), call: Box::new(RuntimeCall::System(SystemCall::remark { remark: vec![1] })) }))]
#[case::sudo_remove_key(RuntimeCall::Sudo(pallet_sudo::Call::remove_key {}))]
#[case::multisig_as_multi_threshold_1(RuntimeCall::Multisig(pallet_multisig::Call::as_multi_threshold_1 { other_signatories: vec![[1u8; 32].into()], call: Box::new(RuntimeCall::System(SystemCall::remark { remark: vec![1] })) }))]
#[case::multisig_as_multi(RuntimeCall::Multisig(pallet_multisig::Call::as_multi { threshold: 1, other_signatories: vec![[1u8; 32].into()], maybe_timepoint: None, call: Box::new(RuntimeCall::System(SystemCall::remark { remark: vec![1] })), max_weight: frame_support::weights::Weight::from_parts(1, 1) }))]
#[case::multisig_approve_as_multi(RuntimeCall::Multisig(pallet_multisig::Call::approve_as_multi { threshold: 1, other_signatories: vec![[1u8; 32].into()], maybe_timepoint: None, call_hash: [1u8; 32], max_weight: frame_support::weights::Weight::from_parts(1, 1) }))]
#[case::multisig_cancel_as_multi(RuntimeCall::Multisig(pallet_multisig::Call::cancel_as_multi { threshold: 1, other_signatories: vec![[1u8; 32].into()], timepoint: pallet_multisig::Timepoint { height: 1, index: 1 }, call_hash: [1u8; 32] }))]
#[case::multisig_poke_deposit(RuntimeCall::Multisig(pallet_multisig::Call::poke_deposit { threshold: 1, other_signatories: vec![[1u8; 32].into()], call_hash: [1u8; 32] }))]
#[case::preimage_note_preimage(RuntimeCall::Preimage(pallet_preimage::Call::note_preimage { bytes: vec![1] }))]
#[case::preimage_unnote_preimage(RuntimeCall::Preimage(pallet_preimage::Call::unnote_preimage { hash: [1u8; 32].into() }))]
#[case::preimage_request_preimage(RuntimeCall::Preimage(pallet_preimage::Call::request_preimage { hash: [1u8; 32].into() }))]
#[case::preimage_unrequest_preimage(RuntimeCall::Preimage(pallet_preimage::Call::unrequest_preimage { hash: [1u8; 32].into() }))]
#[case::preimage_ensure_updated(RuntimeCall::Preimage(pallet_preimage::Call::ensure_updated { hashes: vec![[1u8; 32].into()] }))]
#[case::scheduler_schedule(RuntimeCall::Scheduler(pallet_scheduler::Call::schedule { when: 1, maybe_periodic: None, priority: 1, call: Box::new(RuntimeCall::System(SystemCall::remark { remark: vec![1] })) }))]
#[case::scheduler_cancel(RuntimeCall::Scheduler(pallet_scheduler::Call::cancel { when: 1, index: 1 }))]
#[case::scheduler_schedule_named(RuntimeCall::Scheduler(pallet_scheduler::Call::schedule_named { id: [1u8; 32], when: 1, maybe_periodic: None, priority: 1, call: Box::new(RuntimeCall::System(SystemCall::remark { remark: vec![1] })) }))]
#[case::scheduler_cancel_named(RuntimeCall::Scheduler(pallet_scheduler::Call::cancel_named { id: [1u8; 32] }))]
#[case::scheduler_schedule_after(RuntimeCall::Scheduler(pallet_scheduler::Call::schedule_after { after: 1, maybe_periodic: None, priority: 1, call: Box::new(RuntimeCall::System(SystemCall::remark { remark: vec![1] })) }))]
#[case::scheduler_schedule_named_after(RuntimeCall::Scheduler(pallet_scheduler::Call::schedule_named_after { id: [1u8; 32], after: 1, maybe_periodic: None, priority: 1, call: Box::new(RuntimeCall::System(SystemCall::remark { remark: vec![1] })) }))]
#[case::scheduler_set_retry(RuntimeCall::Scheduler(pallet_scheduler::Call::set_retry { task: (1, 1), retries: 1, period: 1 }))]
#[case::scheduler_set_retry_named(RuntimeCall::Scheduler(pallet_scheduler::Call::set_retry_named { id: [1u8; 32], retries: 1, period: 1 }))]
#[case::scheduler_cancel_retry(RuntimeCall::Scheduler(pallet_scheduler::Call::cancel_retry { task: (1, 1) }))]
#[case::scheduler_cancel_retry_named(RuntimeCall::Scheduler(pallet_scheduler::Call::cancel_retry_named { id: [1u8; 32] }))]
#[case::proxy_proxy(RuntimeCall::Proxy(pallet_subtensor_proxy::Call::proxy { real: sp_runtime::MultiAddress::Id([1u8; 32].into()), force_proxy_type: None, call: Box::new(RuntimeCall::System(SystemCall::remark { remark: vec![1] })) }))]
#[case::proxy_add_proxy(RuntimeCall::Proxy(pallet_subtensor_proxy::Call::add_proxy { delegate: sp_runtime::MultiAddress::Id([1u8; 32].into()), proxy_type: subtensor_runtime_common::ProxyType::Any, delay: 1 }))]
#[case::proxy_remove_proxy(RuntimeCall::Proxy(pallet_subtensor_proxy::Call::remove_proxy { delegate: sp_runtime::MultiAddress::Id([1u8; 32].into()), proxy_type: subtensor_runtime_common::ProxyType::Any, delay: 1 }))]
#[case::proxy_remove_proxies(RuntimeCall::Proxy(pallet_subtensor_proxy::Call::remove_proxies {}))]
#[case::proxy_create_pure(RuntimeCall::Proxy(pallet_subtensor_proxy::Call::create_pure { proxy_type: subtensor_runtime_common::ProxyType::Any, delay: 1, index: 1 }))]
#[case::proxy_kill_pure(RuntimeCall::Proxy(pallet_subtensor_proxy::Call::kill_pure { spawner: sp_runtime::MultiAddress::Id([1u8; 32].into()), proxy_type: subtensor_runtime_common::ProxyType::Any, index: 1, height: 1, ext_index: 1 }))]
#[case::proxy_announce(RuntimeCall::Proxy(pallet_subtensor_proxy::Call::announce { real: sp_runtime::MultiAddress::Id([1u8; 32].into()), call_hash: [1u8; 32].into() }))]
#[case::proxy_remove_announcement(RuntimeCall::Proxy(pallet_subtensor_proxy::Call::remove_announcement { real: sp_runtime::MultiAddress::Id([1u8; 32].into()), call_hash: [1u8; 32].into() }))]
#[case::proxy_reject_announcement(RuntimeCall::Proxy(pallet_subtensor_proxy::Call::reject_announcement { delegate: sp_runtime::MultiAddress::Id([1u8; 32].into()), call_hash: [1u8; 32].into() }))]
#[case::proxy_proxy_announced(RuntimeCall::Proxy(pallet_subtensor_proxy::Call::proxy_announced { delegate: sp_runtime::MultiAddress::Id([1u8; 32].into()), real: sp_runtime::MultiAddress::Id([1u8; 32].into()), force_proxy_type: None, call: Box::new(RuntimeCall::System(SystemCall::remark { remark: vec![1] })) }))]
#[case::proxy_poke_deposit(RuntimeCall::Proxy(pallet_subtensor_proxy::Call::poke_deposit {}))]
#[case::proxy_set_real_pays_fee(RuntimeCall::Proxy(pallet_subtensor_proxy::Call::set_real_pays_fee { delegate: sp_runtime::MultiAddress::Id([1u8; 32].into()), pays_fee: false }))]
#[case::commitments_set_commitment(RuntimeCall::Commitments(pallet_commitments::Call::set_commitment { netuid: NetUid::from(1u16), info: Box::new(pallet_commitments::CommitmentInfo { fields: (vec![pallet_commitments::Data::None]).try_into().expect("bounded") }) }))]
#[case::commitments_set_max_space(RuntimeCall::Commitments(pallet_commitments::Call::set_max_space { new_limit: 1 }))]
#[case::admin_utils_swap_authorities(RuntimeCall::AdminUtils(pallet_admin_utils::Call::swap_authorities { new_authorities: (vec![sp_core::sr25519::Public::from_raw([1u8; 32]).into()]).try_into().expect("bounded") }))]
#[case::admin_utils_sudo_set_default_take(RuntimeCall::AdminUtils(pallet_admin_utils::Call::sudo_set_default_take { default_take: sp_runtime::PerU16::from_parts(1) }))]
#[case::admin_utils_sudo_set_tx_rate_limit(RuntimeCall::AdminUtils(pallet_admin_utils::Call::sudo_set_tx_rate_limit { tx_rate_limit: 1 }))]
#[case::admin_utils_sudo_set_serving_rate_limit(RuntimeCall::AdminUtils(pallet_admin_utils::Call::sudo_set_serving_rate_limit { netuid: NetUid::from(1u16), serving_rate_limit: 1 }))]
#[case::admin_utils_sudo_set_min_difficulty(RuntimeCall::AdminUtils(pallet_admin_utils::Call::sudo_set_min_difficulty { netuid: NetUid::from(1u16), min_difficulty: 1 }))]
#[case::admin_utils_sudo_set_max_difficulty(RuntimeCall::AdminUtils(pallet_admin_utils::Call::sudo_set_max_difficulty { netuid: NetUid::from(1u16), max_difficulty: 1 }))]
#[case::admin_utils_sudo_set_weights_version_key(RuntimeCall::AdminUtils(pallet_admin_utils::Call::sudo_set_weights_version_key { netuid: NetUid::from(1u16), weights_version_key: 1 }))]
#[case::admin_utils_sudo_set_weights_set_rate_limit(RuntimeCall::AdminUtils(pallet_admin_utils::Call::sudo_set_weights_set_rate_limit { netuid: NetUid::from(1u16), weights_set_rate_limit: 1 }))]
#[case::admin_utils_sudo_set_adjustment_interval(RuntimeCall::AdminUtils(pallet_admin_utils::Call::sudo_set_adjustment_interval { netuid: NetUid::from(1u16), adjustment_interval: 1 }))]
#[case::admin_utils_sudo_set_adjustment_alpha(RuntimeCall::AdminUtils(pallet_admin_utils::Call::sudo_set_adjustment_alpha { netuid: NetUid::from(1u16), adjustment_alpha: 1 }))]
#[case::admin_utils_sudo_set_immunity_period(RuntimeCall::AdminUtils(pallet_admin_utils::Call::sudo_set_immunity_period { netuid: NetUid::from(1u16), immunity_period: 1 }))]
#[case::admin_utils_sudo_set_min_allowed_weights(RuntimeCall::AdminUtils(pallet_admin_utils::Call::sudo_set_min_allowed_weights { netuid: NetUid::from(1u16), min_allowed_weights: 1 }))]
#[case::admin_utils_sudo_set_max_allowed_uids(RuntimeCall::AdminUtils(pallet_admin_utils::Call::sudo_set_max_allowed_uids { netuid: NetUid::from(1u16), max_allowed_uids: 1 }))]
#[case::admin_utils_sudo_set_kappa(RuntimeCall::AdminUtils(pallet_admin_utils::Call::sudo_set_kappa { netuid: NetUid::from(1u16), kappa: 1 }))]
#[case::admin_utils_sudo_set_rho(RuntimeCall::AdminUtils(pallet_admin_utils::Call::sudo_set_rho { netuid: NetUid::from(1u16), rho: 1 }))]
#[case::admin_utils_sudo_set_activity_cutoff(RuntimeCall::AdminUtils(pallet_admin_utils::Call::sudo_set_activity_cutoff { netuid: NetUid::from(1u16), activity_cutoff: 1 }))]
#[case::admin_utils_sudo_set_activity_cutoff_factor(RuntimeCall::AdminUtils(pallet_admin_utils::Call::sudo_set_activity_cutoff_factor { netuid: NetUid::from(1u16), factor_milli: 1 }))]
#[case::admin_utils_sudo_set_network_registration_allowed(RuntimeCall::AdminUtils(pallet_admin_utils::Call::sudo_set_network_registration_allowed { netuid: NetUid::from(1u16), registration_allowed: false }))]
#[case::admin_utils_sudo_set_network_pow_registration_allowed(RuntimeCall::AdminUtils(pallet_admin_utils::Call::sudo_set_network_pow_registration_allowed { netuid: NetUid::from(1u16), registration_allowed: false }))]
#[case::admin_utils_sudo_set_target_registrations_per_interval(RuntimeCall::AdminUtils(pallet_admin_utils::Call::sudo_set_target_registrations_per_interval { netuid: NetUid::from(1u16), target_registrations_per_interval: 1 }))]
#[case::admin_utils_sudo_set_min_burn(RuntimeCall::AdminUtils(pallet_admin_utils::Call::sudo_set_min_burn { netuid: NetUid::from(1u16), min_burn: TaoBalance::new(1) }))]
#[case::admin_utils_sudo_set_max_burn(RuntimeCall::AdminUtils(pallet_admin_utils::Call::sudo_set_max_burn { netuid: NetUid::from(1u16), max_burn: TaoBalance::new(1) }))]
#[case::admin_utils_sudo_set_difficulty(RuntimeCall::AdminUtils(pallet_admin_utils::Call::sudo_set_difficulty { netuid: NetUid::from(1u16), difficulty: 1 }))]
#[case::admin_utils_sudo_set_max_allowed_validators(RuntimeCall::AdminUtils(pallet_admin_utils::Call::sudo_set_max_allowed_validators { netuid: NetUid::from(1u16), max_allowed_validators: 1 }))]
#[case::admin_utils_sudo_set_bonds_moving_average(RuntimeCall::AdminUtils(pallet_admin_utils::Call::sudo_set_bonds_moving_average { netuid: NetUid::from(1u16), bonds_moving_average: 1 }))]
#[case::admin_utils_sudo_set_bonds_penalty(RuntimeCall::AdminUtils(pallet_admin_utils::Call::sudo_set_bonds_penalty { netuid: NetUid::from(1u16), bonds_penalty: 1 }))]
#[case::admin_utils_sudo_set_max_registrations_per_block(RuntimeCall::AdminUtils(pallet_admin_utils::Call::sudo_set_max_registrations_per_block { netuid: NetUid::from(1u16), max_registrations_per_block: 1 }))]
#[case::admin_utils_sudo_set_subnet_owner_cut(RuntimeCall::AdminUtils(pallet_admin_utils::Call::sudo_set_subnet_owner_cut { subnet_owner_cut: 1 }))]
#[case::admin_utils_sudo_set_network_rate_limit(RuntimeCall::AdminUtils(pallet_admin_utils::Call::sudo_set_network_rate_limit { rate_limit: 1 }))]
#[case::admin_utils_sudo_set_tempo(RuntimeCall::AdminUtils(pallet_admin_utils::Call::sudo_set_tempo { netuid: NetUid::from(1u16), tempo: 1 }))]
#[case::admin_utils_sudo_set_total_issuance(RuntimeCall::AdminUtils(pallet_admin_utils::Call::sudo_set_total_issuance { total_issuance: TaoBalance::new(1) }))]
#[case::admin_utils_sudo_set_network_immunity_period(RuntimeCall::AdminUtils(pallet_admin_utils::Call::sudo_set_network_immunity_period { immunity_period: 1 }))]
#[case::admin_utils_sudo_set_network_min_lock_cost(RuntimeCall::AdminUtils(pallet_admin_utils::Call::sudo_set_network_min_lock_cost { lock_cost: TaoBalance::new(1) }))]
#[case::admin_utils_sudo_set_subnet_limit(RuntimeCall::AdminUtils(pallet_admin_utils::Call::sudo_set_subnet_limit { max_subnets: 1 }))]
#[case::admin_utils_sudo_set_lock_reduction_interval(RuntimeCall::AdminUtils(pallet_admin_utils::Call::sudo_set_lock_reduction_interval { interval: 1 }))]
#[case::admin_utils_sudo_set_rao_recycled(RuntimeCall::AdminUtils(pallet_admin_utils::Call::sudo_set_rao_recycled { netuid: NetUid::from(1u16), rao_recycled: TaoBalance::new(1) }))]
#[case::admin_utils_sudo_set_stake_threshold(RuntimeCall::AdminUtils(pallet_admin_utils::Call::sudo_set_stake_threshold { min_stake: 1 }))]
#[case::admin_utils_sudo_set_nominator_min_required_stake(RuntimeCall::AdminUtils(pallet_admin_utils::Call::sudo_set_nominator_min_required_stake { min_stake: 1 }))]
#[case::admin_utils_sudo_set_tx_delegate_take_rate_limit(RuntimeCall::AdminUtils(pallet_admin_utils::Call::sudo_set_tx_delegate_take_rate_limit { tx_rate_limit: 1 }))]
#[case::admin_utils_sudo_set_min_delegate_take(RuntimeCall::AdminUtils(pallet_admin_utils::Call::sudo_set_min_delegate_take { take: sp_runtime::PerU16::from_parts(1) }))]
#[case::admin_utils_sudo_set_min_childkey_take_per_subnet(RuntimeCall::AdminUtils(pallet_admin_utils::Call::sudo_set_min_childkey_take_per_subnet { netuid: NetUid::from(1u16), take: sp_runtime::PerU16::from_parts(1) }))]
#[case::admin_utils_sudo_set_commit_reveal_weights_enabled(RuntimeCall::AdminUtils(pallet_admin_utils::Call::sudo_set_commit_reveal_weights_enabled { netuid: NetUid::from(1u16), enabled: false }))]
#[case::admin_utils_sudo_set_liquid_alpha_enabled(RuntimeCall::AdminUtils(pallet_admin_utils::Call::sudo_set_liquid_alpha_enabled { netuid: NetUid::from(1u16), enabled: false }))]
#[case::admin_utils_sudo_set_alpha_values(RuntimeCall::AdminUtils(pallet_admin_utils::Call::sudo_set_alpha_values { netuid: NetUid::from(1u16), alpha_low: 1, alpha_high: 1 }))]
#[case::admin_utils_sudo_set_liquid_alpha_consensus_mode(RuntimeCall::AdminUtils(pallet_admin_utils::Call::sudo_set_liquid_alpha_consensus_mode { netuid: NetUid::from(1u16), mode: pallet_subtensor::ConsensusMode::Current }))]
#[case::admin_utils_sudo_set_dissolve_network_schedule_duration(RuntimeCall::AdminUtils(pallet_admin_utils::Call::sudo_set_dissolve_network_schedule_duration { duration: 1 }))]
#[case::admin_utils_sudo_set_commit_reveal_weights_interval(RuntimeCall::AdminUtils(pallet_admin_utils::Call::sudo_set_commit_reveal_weights_interval { netuid: NetUid::from(1u16), interval: 1 }))]
#[case::admin_utils_sudo_set_evm_chain_id(RuntimeCall::AdminUtils(pallet_admin_utils::Call::sudo_set_evm_chain_id { chain_id: 1 }))]
#[case::admin_utils_schedule_grandpa_change(RuntimeCall::AdminUtils(pallet_admin_utils::Call::schedule_grandpa_change { next_authorities: vec![(sp_core::ed25519::Public::from_raw([1u8; 32]).into(), 1)], in_blocks: 1, forced: None }))]
#[case::admin_utils_sudo_set_toggle_transfer(RuntimeCall::AdminUtils(pallet_admin_utils::Call::sudo_set_toggle_transfer { netuid: NetUid::from(1u16), toggle: false }))]
#[case::admin_utils_sudo_set_recycle_or_burn(RuntimeCall::AdminUtils(pallet_admin_utils::Call::sudo_set_recycle_or_burn { netuid: NetUid::from(1u16), recycle_or_burn: pallet_subtensor::RecycleOrBurnEnum::Burn }))]
#[case::admin_utils_sudo_toggle_evm_precompile(RuntimeCall::AdminUtils(pallet_admin_utils::Call::sudo_toggle_evm_precompile { precompile_id: pallet_admin_utils::PrecompileEnum::BalanceTransfer, enabled: false }))]
#[case::admin_utils_sudo_set_subnet_moving_alpha(RuntimeCall::AdminUtils(pallet_admin_utils::Call::sudo_set_subnet_moving_alpha { alpha: substrate_fixed::types::I96F32::from_bits(1) }))]
#[case::admin_utils_sudo_set_ema_price_halving_period(RuntimeCall::AdminUtils(pallet_admin_utils::Call::sudo_set_ema_price_halving_period { netuid: NetUid::from(1u16), ema_halving: 1 }))]
#[case::admin_utils_sudo_set_alpha_sigmoid_steepness(RuntimeCall::AdminUtils(pallet_admin_utils::Call::sudo_set_alpha_sigmoid_steepness { netuid: NetUid::from(1u16), steepness: 1 }))]
#[case::admin_utils_sudo_set_yuma3_enabled(RuntimeCall::AdminUtils(pallet_admin_utils::Call::sudo_set_yuma3_enabled { netuid: NetUid::from(1u16), enabled: false }))]
#[case::admin_utils_sudo_set_bonds_reset_enabled(RuntimeCall::AdminUtils(pallet_admin_utils::Call::sudo_set_bonds_reset_enabled { netuid: NetUid::from(1u16), enabled: false }))]
#[case::admin_utils_sudo_set_sn_owner_hotkey(RuntimeCall::AdminUtils(pallet_admin_utils::Call::sudo_set_sn_owner_hotkey { netuid: NetUid::from(1u16), hotkey: [1u8; 32].into() }))]
#[case::admin_utils_sudo_set_subtoken_enabled(RuntimeCall::AdminUtils(pallet_admin_utils::Call::sudo_set_subtoken_enabled { netuid: NetUid::from(1u16), subtoken_enabled: false }))]
#[case::admin_utils_sudo_set_commit_reveal_version(RuntimeCall::AdminUtils(pallet_admin_utils::Call::sudo_set_commit_reveal_version { version: 1 }))]
#[case::admin_utils_sudo_set_owner_immune_neuron_limit(RuntimeCall::AdminUtils(pallet_admin_utils::Call::sudo_set_owner_immune_neuron_limit { netuid: NetUid::from(1u16), immune_neurons: 1 }))]
#[case::admin_utils_sudo_set_ck_burn(RuntimeCall::AdminUtils(pallet_admin_utils::Call::sudo_set_ck_burn { burn: 1 }))]
#[case::admin_utils_sudo_set_admin_freeze_window(RuntimeCall::AdminUtils(pallet_admin_utils::Call::sudo_set_admin_freeze_window { window: 1 }))]
#[case::admin_utils_sudo_set_owner_hparam_rate_limit(RuntimeCall::AdminUtils(pallet_admin_utils::Call::sudo_set_owner_hparam_rate_limit { epochs: 1 }))]
#[case::admin_utils_sudo_set_mechanism_count(RuntimeCall::AdminUtils(pallet_admin_utils::Call::sudo_set_mechanism_count { netuid: NetUid::from(1u16), mechanism_count: MechId::from(1u8) }))]
#[case::admin_utils_sudo_set_mechanism_emission_split(RuntimeCall::AdminUtils(pallet_admin_utils::Call::sudo_set_mechanism_emission_split { netuid: NetUid::from(1u16), maybe_split: None }))]
#[case::admin_utils_sudo_trim_to_max_allowed_uids(RuntimeCall::AdminUtils(pallet_admin_utils::Call::sudo_trim_to_max_allowed_uids { netuid: NetUid::from(1u16), max_n: 1 }))]
#[case::admin_utils_sudo_set_min_allowed_uids(RuntimeCall::AdminUtils(pallet_admin_utils::Call::sudo_set_min_allowed_uids { netuid: NetUid::from(1u16), min_allowed_uids: 1 }))]
#[case::admin_utils_sudo_set_tao_flow_cutoff(RuntimeCall::AdminUtils(pallet_admin_utils::Call::sudo_set_tao_flow_cutoff { flow_cutoff: substrate_fixed::types::I64F64::from_bits(1) }))]
#[case::admin_utils_sudo_set_tao_flow_normalization_exponent(RuntimeCall::AdminUtils(pallet_admin_utils::Call::sudo_set_tao_flow_normalization_exponent { exponent: substrate_fixed::types::U64F64::from_bits(1) }))]
#[case::admin_utils_sudo_set_emission_bar_quantile(RuntimeCall::AdminUtils(pallet_admin_utils::Call::sudo_set_emission_bar_quantile { quantile: substrate_fixed::types::U64F64::from_bits(1) }))]
#[case::admin_utils_sudo_set_emission_bar_rank(RuntimeCall::AdminUtils(pallet_admin_utils::Call::sudo_set_emission_bar_rank { rank: 1 }))]
#[case::admin_utils_sudo_set_emission_gate_exponent(RuntimeCall::AdminUtils(pallet_admin_utils::Call::sudo_set_emission_gate_exponent { exponent: substrate_fixed::types::U64F64::from_bits(1) }))]
#[case::admin_utils_sudo_set_tao_flow_smoothing_factor(RuntimeCall::AdminUtils(pallet_admin_utils::Call::sudo_set_tao_flow_smoothing_factor { smoothing_factor: 1 }))]
#[case::admin_utils_sudo_set_net_tao_flow_enabled(RuntimeCall::AdminUtils(pallet_admin_utils::Call::sudo_set_net_tao_flow_enabled { enabled: false }))]
#[case::admin_utils_sudo_set_max_mechanism_count(RuntimeCall::AdminUtils(pallet_admin_utils::Call::sudo_set_max_mechanism_count { max_mechanism_count: MechId::from(1u8) }))]
#[case::admin_utils_sudo_set_min_non_immune_uids(RuntimeCall::AdminUtils(pallet_admin_utils::Call::sudo_set_min_non_immune_uids { netuid: NetUid::from(1u16), min: 1 }))]
#[case::admin_utils_sudo_set_start_call_delay(RuntimeCall::AdminUtils(pallet_admin_utils::Call::sudo_set_start_call_delay { delay: 1 }))]
#[case::admin_utils_sudo_set_coldkey_swap_announcement_delay(RuntimeCall::AdminUtils(pallet_admin_utils::Call::sudo_set_coldkey_swap_announcement_delay { duration: 1 }))]
#[case::admin_utils_sudo_set_coldkey_swap_reannouncement_delay(RuntimeCall::AdminUtils(pallet_admin_utils::Call::sudo_set_coldkey_swap_reannouncement_delay { duration: 1 }))]
#[case::admin_utils_sudo_set_burn_half_life(RuntimeCall::AdminUtils(pallet_admin_utils::Call::sudo_set_burn_half_life { netuid: NetUid::from(1u16), burn_half_life: 1 }))]
#[case::admin_utils_sudo_set_burn_increase_mult(RuntimeCall::AdminUtils(pallet_admin_utils::Call::sudo_set_burn_increase_mult { netuid: NetUid::from(1u16), burn_increase_mult: substrate_fixed::types::U64F64::from_bits(1) }))]
#[case::admin_utils_sudo_set_owner_cut_enabled(RuntimeCall::AdminUtils(pallet_admin_utils::Call::sudo_set_owner_cut_enabled { netuid: NetUid::from(1u16), enabled: false }))]
#[case::admin_utils_sudo_set_owner_cut_auto_lock_enabled(RuntimeCall::AdminUtils(pallet_admin_utils::Call::sudo_set_owner_cut_auto_lock_enabled { netuid: NetUid::from(1u16), enabled: false }))]
#[case::admin_utils_sudo_set_subnet_emission_enabled(RuntimeCall::AdminUtils(pallet_admin_utils::Call::sudo_set_subnet_emission_enabled { netuid: NetUid::from(1u16), enabled: false }))]
#[case::admin_utils_sudo_set_collateral_lock_share(RuntimeCall::AdminUtils(pallet_admin_utils::Call::sudo_set_collateral_lock_share { netuid: NetUid::from(1u16), lock_share: 1 }))]
#[case::admin_utils_sudo_set_collateral_drain_ratio(RuntimeCall::AdminUtils(pallet_admin_utils::Call::sudo_set_collateral_drain_ratio { netuid: NetUid::from(1u16), drain_ratio: substrate_fixed::types::U64F64::from_bits(1) }))]
#[case::admin_utils_sudo_set_basket_concentration_cap(RuntimeCall::AdminUtils(pallet_admin_utils::Call::sudo_set_basket_concentration_cap { cap: 1 }))]
#[case::admin_utils_sudo_set_basket_trading_enabled(RuntimeCall::AdminUtils(pallet_admin_utils::Call::sudo_set_basket_trading_enabled { enabled: false }))]
#[case::admin_utils_sudo_set_basket_trading_frozen(RuntimeCall::AdminUtils(pallet_admin_utils::Call::sudo_set_basket_trading_frozen { hotkey: [1u8; 32].into(), frozen: false }))]
#[case::admin_utils_sudo_set_basket_daily_turnover_cap(RuntimeCall::AdminUtils(pallet_admin_utils::Call::sudo_set_basket_daily_turnover_cap { cap: 1 }))]
#[case::admin_utils_sudo_set_basket_liquidity_cap(RuntimeCall::AdminUtils(pallet_admin_utils::Call::sudo_set_basket_liquidity_cap { cap: 1 }))]
#[case::admin_utils_sudo_set_max_epochs_per_block(RuntimeCall::AdminUtils(pallet_admin_utils::Call::sudo_set_max_epochs_per_block { max_epochs_per_block: 1 }))]
#[case::safe_mode_enter(RuntimeCall::SafeMode(pallet_safe_mode::Call::enter {}))]
#[case::safe_mode_force_enter(RuntimeCall::SafeMode(pallet_safe_mode::Call::force_enter {}))]
#[case::safe_mode_extend(RuntimeCall::SafeMode(pallet_safe_mode::Call::extend {}))]
#[case::safe_mode_force_extend(RuntimeCall::SafeMode(pallet_safe_mode::Call::force_extend {}))]
#[case::safe_mode_force_exit(RuntimeCall::SafeMode(pallet_safe_mode::Call::force_exit {}))]
#[case::safe_mode_force_slash_deposit(RuntimeCall::SafeMode(pallet_safe_mode::Call::force_slash_deposit { account: [1u8; 32].into(), block: 1 }))]
#[case::safe_mode_release_deposit(RuntimeCall::SafeMode(pallet_safe_mode::Call::release_deposit { account: [1u8; 32].into(), block: 1 }))]
#[case::safe_mode_force_release_deposit(RuntimeCall::SafeMode(pallet_safe_mode::Call::force_release_deposit { account: [1u8; 32].into(), block: 1 }))]
#[case::evm_withdraw(RuntimeCall::EVM(pallet_evm::Call::withdraw { address: [1u8; 20].into(), value: TaoBalance::new(1) }))]
#[case::evm_call(RuntimeCall::EVM(pallet_evm::Call::call { source: [1u8; 20].into(), target: [1u8; 20].into(), input: vec![1], value: sp_core::U256([1u64; 4]), gas_limit: 1, max_fee_per_gas: sp_core::U256([1u64; 4]), max_priority_fee_per_gas: None, nonce: None, access_list: vec![([1u8; 20].into(), vec![[1u8; 32].into()])], authorization_list: vec![ethereum::AuthorizationListItem { chain_id: 1, address: [1u8; 20].into(), nonce: sp_core::U256([1u64; 4]), signature: ethereum::eip7702::MalleableTransactionSignature { odd_y_parity: false, r: [1u8; 32].into(), s: [1u8; 32].into() } }] }))]
#[case::evm_create(RuntimeCall::EVM(pallet_evm::Call::create { source: [1u8; 20].into(), init: vec![1], value: sp_core::U256([1u64; 4]), gas_limit: 1, max_fee_per_gas: sp_core::U256([1u64; 4]), max_priority_fee_per_gas: None, nonce: None, access_list: vec![([1u8; 20].into(), vec![[1u8; 32].into()])], authorization_list: vec![ethereum::AuthorizationListItem { chain_id: 1, address: [1u8; 20].into(), nonce: sp_core::U256([1u64; 4]), signature: ethereum::eip7702::MalleableTransactionSignature { odd_y_parity: false, r: [1u8; 32].into(), s: [1u8; 32].into() } }] }))]
#[case::evm_create2(RuntimeCall::EVM(pallet_evm::Call::create2 { source: [1u8; 20].into(), init: vec![1], salt: [1u8; 32].into(), value: sp_core::U256([1u64; 4]), gas_limit: 1, max_fee_per_gas: sp_core::U256([1u64; 4]), max_priority_fee_per_gas: None, nonce: None, access_list: vec![([1u8; 20].into(), vec![[1u8; 32].into()])], authorization_list: vec![ethereum::AuthorizationListItem { chain_id: 1, address: [1u8; 20].into(), nonce: sp_core::U256([1u64; 4]), signature: ethereum::eip7702::MalleableTransactionSignature { odd_y_parity: false, r: [1u8; 32].into(), s: [1u8; 32].into() } }] }))]
#[case::evm_set_whitelist(RuntimeCall::EVM(pallet_evm::Call::set_whitelist { new: vec![[1u8; 20].into()] }))]
#[case::evm_disable_whitelist(RuntimeCall::EVM(pallet_evm::Call::disable_whitelist { disabled: false }))]
#[case::base_fee_set_base_fee_per_gas(RuntimeCall::BaseFee(pallet_base_fee::Call::set_base_fee_per_gas { fee: sp_core::U256([1u64; 4]) }))]
#[case::base_fee_set_elasticity(RuntimeCall::BaseFee(pallet_base_fee::Call::set_elasticity { elasticity: sp_runtime::Permill::from_parts(1) }))]
#[case::drand_write_pulse(RuntimeCall::Drand(pallet_drand::Call::write_pulse { pulses_payload: pallet_drand::types::PulsesPayload { block_number: 1, pulses: vec![pallet_drand::types::Pulse { round: 1, randomness: (vec![1]).try_into().expect("bounded"), signature: (vec![1]).try_into().expect("bounded") }], public: sp_runtime::MultiSigner::Ed25519([1u8; 32].into()) }, signature: None }))]
#[case::drand_set_beacon_config(RuntimeCall::Drand(pallet_drand::Call::set_beacon_config { config_payload: pallet_drand::types::BeaconConfigurationPayload { block_number: 1, config: pallet_drand::types::BeaconConfiguration { public_key: (vec![1]).try_into().expect("bounded"), period: 1, genesis_time: 1, hash: (vec![1]).try_into().expect("bounded"), group_hash: (vec![1]).try_into().expect("bounded"), scheme_id: (vec![1]).try_into().expect("bounded"), metadata: pallet_drand::types::Metadata { beacon_id: (vec![1]).try_into().expect("bounded") } }, public: sp_runtime::MultiSigner::Ed25519([1u8; 32].into()) }, signature: None }))]
#[case::drand_set_oldest_stored_round(RuntimeCall::Drand(pallet_drand::Call::set_oldest_stored_round { oldest_round: 1 }))]
#[case::crowdloan_create(RuntimeCall::Crowdloan(pallet_crowdloan::Call::create { deposit: TaoBalance::new(1), min_contribution: TaoBalance::new(1), cap: TaoBalance::new(1), end: 1, call: None, target_address: None }))]
#[case::crowdloan_contribute(RuntimeCall::Crowdloan(pallet_crowdloan::Call::contribute { crowdloan_id: 1, amount: TaoBalance::new(1) }))]
#[case::crowdloan_withdraw(RuntimeCall::Crowdloan(pallet_crowdloan::Call::withdraw { crowdloan_id: 1 }))]
#[case::crowdloan_finalize(RuntimeCall::Crowdloan(pallet_crowdloan::Call::finalize { crowdloan_id: 1 }))]
#[case::crowdloan_refund(RuntimeCall::Crowdloan(pallet_crowdloan::Call::refund { crowdloan_id: 1 }))]
#[case::crowdloan_dissolve(RuntimeCall::Crowdloan(pallet_crowdloan::Call::dissolve { crowdloan_id: 1 }))]
#[case::crowdloan_update_min_contribution(RuntimeCall::Crowdloan(pallet_crowdloan::Call::update_min_contribution { crowdloan_id: 1, new_min_contribution: TaoBalance::new(1) }))]
#[case::crowdloan_update_end(RuntimeCall::Crowdloan(pallet_crowdloan::Call::update_end { crowdloan_id: 1, new_end: 1 }))]
#[case::crowdloan_update_cap(RuntimeCall::Crowdloan(pallet_crowdloan::Call::update_cap { crowdloan_id: 1, new_cap: TaoBalance::new(1) }))]
#[case::crowdloan_set_max_contribution(RuntimeCall::Crowdloan(pallet_crowdloan::Call::set_max_contribution { crowdloan_id: 1, new_max_contribution: None }))]
#[case::swap_set_fee_rate(RuntimeCall::Swap(pallet_subtensor_swap::Call::set_fee_rate { netuid: NetUid::from(1u16), rate: 1 }))]
#[case::swap_toggle_user_liquidity(RuntimeCall::Swap(pallet_subtensor_swap::Call::toggle_user_liquidity { netuid: NetUid::from(1u16), enable: false }))]
#[case::swap_add_liquidity(RuntimeCall::Swap(pallet_subtensor_swap::Call::add_liquidity { hotkey: [1u8; 32].into(), netuid: NetUid::from(1u16), tick_low: legacy_tick_index(), tick_high: legacy_tick_index(), liquidity: 1 }))]
#[case::swap_remove_liquidity(RuntimeCall::Swap(pallet_subtensor_swap::Call::remove_liquidity { hotkey: [1u8; 32].into(), netuid: NetUid::from(1u16), position_id: legacy_position_id() }))]
#[case::swap_modify_position(RuntimeCall::Swap(pallet_subtensor_swap::Call::modify_position { hotkey: [1u8; 32].into(), netuid: NetUid::from(1u16), position_id: legacy_position_id(), liquidity_delta: 1 }))]
#[case::swap_disable_lp(RuntimeCall::Swap(pallet_subtensor_swap::Call::disable_lp {}))]
#[case::contracts_call_old_weight(RuntimeCall::Contracts(pallet_contracts::Call::call_old_weight { dest: sp_runtime::MultiAddress::Id([1u8; 32].into()), value: TaoBalance::new(1), gas_limit: 1, storage_deposit_limit: None, data: vec![1] }))]
#[case::contracts_instantiate_with_code_old_weight(RuntimeCall::Contracts(pallet_contracts::Call::instantiate_with_code_old_weight { value: TaoBalance::new(1), gas_limit: 1, storage_deposit_limit: None, code: vec![1], data: vec![1], salt: vec![1] }))]
#[case::contracts_instantiate_old_weight(RuntimeCall::Contracts(pallet_contracts::Call::instantiate_old_weight { value: TaoBalance::new(1), gas_limit: 1, storage_deposit_limit: None, code_hash: [1u8; 32].into(), data: vec![1], salt: vec![1] }))]
#[case::contracts_upload_code(RuntimeCall::Contracts(pallet_contracts::Call::upload_code { code: vec![1], storage_deposit_limit: None, determinism: pallet_contracts::Determinism::Enforced }))]
#[case::contracts_remove_code(RuntimeCall::Contracts(pallet_contracts::Call::remove_code { code_hash: [1u8; 32].into() }))]
#[case::contracts_set_code(RuntimeCall::Contracts(pallet_contracts::Call::set_code { dest: sp_runtime::MultiAddress::Id([1u8; 32].into()), code_hash: [1u8; 32].into() }))]
#[case::contracts_call(RuntimeCall::Contracts(pallet_contracts::Call::call { dest: sp_runtime::MultiAddress::Id([1u8; 32].into()), value: TaoBalance::new(1), gas_limit: frame_support::weights::Weight::from_parts(1, 1), storage_deposit_limit: None, data: vec![1] }))]
#[case::contracts_instantiate_with_code(RuntimeCall::Contracts(pallet_contracts::Call::instantiate_with_code { value: TaoBalance::new(1), gas_limit: frame_support::weights::Weight::from_parts(1, 1), storage_deposit_limit: None, code: vec![1], data: vec![1], salt: vec![1] }))]
#[case::contracts_instantiate(RuntimeCall::Contracts(pallet_contracts::Call::instantiate { value: TaoBalance::new(1), gas_limit: frame_support::weights::Weight::from_parts(1, 1), storage_deposit_limit: None, code_hash: [1u8; 32].into(), data: vec![1], salt: vec![1] }))]
#[case::contracts_migrate(RuntimeCall::Contracts(pallet_contracts::Call::migrate { weight_limit: frame_support::weights::Weight::from_parts(1, 1) }))]
#[case::mev_shield_announce_next_key(RuntimeCall::MevShield(pallet_shield::Call::announce_next_key { enc_key: None }))]
#[case::mev_shield_submit_encrypted(RuntimeCall::MevShield(pallet_shield::Call::submit_encrypted { ciphertext: (vec![1]).try_into().expect("bounded") }))]
#[case::mev_shield_store_encrypted(RuntimeCall::MevShield(pallet_shield::Call::store_encrypted { encrypted_call: (vec![1]).try_into().expect("bounded") }))]
#[case::mev_shield_set_max_pending_extrinsics_number(RuntimeCall::MevShield(pallet_shield::Call::set_max_pending_extrinsics_number { value: 1 }))]
#[case::mev_shield_set_on_initialize_weight(RuntimeCall::MevShield(pallet_shield::Call::set_on_initialize_weight { value: 1 }))]
#[case::mev_shield_set_stored_extrinsic_lifetime(RuntimeCall::MevShield(pallet_shield::Call::set_stored_extrinsic_lifetime { value: 1 }))]
#[case::mev_shield_set_max_extrinsic_weight(RuntimeCall::MevShield(pallet_shield::Call::set_max_extrinsic_weight { value: 1 }))]
#[case::limit_orders_execute_orders(RuntimeCall::LimitOrders(pallet_limit_orders::Call::execute_orders { orders: (vec![pallet_limit_orders::SignedOrder { order: pallet_limit_orders::VersionedOrder::V1(pallet_limit_orders::Order { signer: [1u8; 32].into(), hotkey: [1u8; 32].into(), netuid: NetUid::from(1u16), order_type: pallet_limit_orders::OrderType::LimitBuy, amount: 1, limit_price: 1, expiry: 1, fee_rate: sp_runtime::Perbill::from_parts(1), fee_recipient: [1u8; 32].into(), relayer: None, max_slippage: None, chain_id: 1, partial_fills_enabled: false }), signature: sp_runtime::MultiSignature::Ed25519([1u8; 64].into()), partial_fill: None }]).try_into().expect("bounded"), should_fail: false }))]
#[case::limit_orders_execute_batched_orders(RuntimeCall::LimitOrders(pallet_limit_orders::Call::execute_batched_orders { netuid: NetUid::from(1u16), orders: (vec![pallet_limit_orders::SignedOrder { order: pallet_limit_orders::VersionedOrder::V1(pallet_limit_orders::Order { signer: [1u8; 32].into(), hotkey: [1u8; 32].into(), netuid: NetUid::from(1u16), order_type: pallet_limit_orders::OrderType::LimitBuy, amount: 1, limit_price: 1, expiry: 1, fee_rate: sp_runtime::Perbill::from_parts(1), fee_recipient: [1u8; 32].into(), relayer: None, max_slippage: None, chain_id: 1, partial_fills_enabled: false }), signature: sp_runtime::MultiSignature::Ed25519([1u8; 64].into()), partial_fill: None }]).try_into().expect("bounded") }))]
#[case::limit_orders_cancel_order(RuntimeCall::LimitOrders(pallet_limit_orders::Call::cancel_order { order: pallet_limit_orders::VersionedOrder::V1(pallet_limit_orders::Order { signer: [1u8; 32].into(), hotkey: [1u8; 32].into(), netuid: NetUid::from(1u16), order_type: pallet_limit_orders::OrderType::LimitBuy, amount: 1, limit_price: 1, expiry: 1, fee_rate: sp_runtime::Perbill::from_parts(1), fee_recipient: [1u8; 32].into(), relayer: None, max_slippage: None, chain_id: 1, partial_fills_enabled: false }) }))]
#[case::limit_orders_set_pallet_status(RuntimeCall::LimitOrders(pallet_limit_orders::Call::set_pallet_status { enabled: false }))]
#[case::limit_orders_prune_linked_output(RuntimeCall::LimitOrders(pallet_limit_orders::Call::prune_linked_output { order_id: [1u8; 32].into() }))]

fn dispatchable_fee_matches_snapshot(
    #[context] ctx: Context,
    #[case] call: RuntimeCall,
    mut new_test_ext: sp_io::TestExternalities,
) {
    new_test_ext.execute_with(|| {
        let fee = quoted_fee_rao(&call);
        with_settings!(
            {
                comparator => Box::new(FeeToleranceComparator),
                description => ctx.description.unwrap_or_default(),
                snapshot_suffix => ctx.description.unwrap_or_default(),
             },
             {
                assert_snapshot!(fee)
            }
        );
    })
}

#[derive(Clone, Copy)]
enum EvmFeeCase {
    BalanceTransfer,
    BalancePrecompile,
    AddStake,
    RemoveStake,
    AddProxy,
}

#[rstest]
#[case::no_tip(EvmFeeCase::BalanceTransfer, 0)]
#[case::priority_tip(EvmFeeCase::BalanceTransfer, 2_000_000_000)]
#[case::balance_precompile(EvmFeeCase::BalancePrecompile, 0)]
#[case::add_stake(EvmFeeCase::AddStake, 0)]
#[case::remove_stake(EvmFeeCase::RemoveStake, 0)]
#[case::add_proxy(EvmFeeCase::AddProxy, 0)]
fn evm_transaction_fee_matches_snapshot(
    #[context] ctx: Context,
    #[case] case: EvmFeeCase,
    #[case] priority_fee_per_gas: u64,
    mut new_test_ext: sp_io::TestExternalities,
) {
    use frame_support::traits::Get;
    use node_subtensor_runtime::{Balances, Executive, RuntimeOrigin, UncheckedExtrinsic};
    use pallet_evm::{AddressMapping, FeeCalculator};
    use precompile_utils::solidity::encode_with_selector;
    use sp_core::{ecdsa, Pair, H160, H256, U256};
    use subtensor_precompiles::{
        BalanceTransferPrecompile, PrecompileExt, ProxyPrecompile, StakingPrecompileV2,
    };
    use subtensor_runtime_common::ProxyType;

    let fee_rao = new_test_ext.execute_with(|| {
        let pair = ecdsa::Pair::from_seed(&[7u8; 32]);
        let seed_signature = pair.sign_prehashed(&[0u8; 32]);
        let public = sp_io::crypto::secp256k1_ecdsa_recover(&seed_signature.0, &[0u8; 32])
            .ok().expect("valid sender key");
        let sender = H160::from_slice(&sp_io::hashing::keccak_256(&public)[12..]);
        let sender_account = <Runtime as pallet_evm::Config>::AddressMapping::into_account_id(sender);
        Balances::force_set_balance(RuntimeOrigin::root(), sender_account.clone().into(), TaoBalance::new(1_000_000_000_000))
            .expect("fund sender");
        let destination = H256::repeat_byte(0x33);
        let destination_account: subtensor_runtime_common::AccountId = destination.0.into();
        let hotkey = H256::repeat_byte(0x44);
        let hotkey_account: subtensor_runtime_common::AccountId = hotkey.0.into();
        let amount = 1_000_000_000u64;
        let netuid = NetUid::from(0u16);
        if matches!(case, EvmFeeCase::AddStake | EvmFeeCase::RemoveStake) {
            pallet_subtensor::Pallet::<Runtime>::init_new_network(netuid, 360);
            pallet_subtensor::SubtokenEnabled::<Runtime>::insert(netuid, true);
            pallet_subtensor::Pallet::<Runtime>::create_account_if_non_existent(&sender_account, &hotkey_account)
                .expect("set up staking hotkey");
            if matches!(case, EvmFeeCase::RemoveStake) {
                pallet_subtensor::Pallet::<Runtime>::add_stake(RuntimeOrigin::signed(sender_account.clone()), hotkey_account.clone(), netuid, TaoBalance::new(2_000_000_000))
                    .expect("seed root stake for removal");
            }
        }
        let stake_before = pallet_subtensor::Pallet::<Runtime>::get_stake_for_hotkey_and_coldkey_on_subnet(&hotkey_account, &sender_account, netuid).to_u64();
        let destination_before = Balances::free_balance(&destination_account).to_u64();
        let selector = |signature: &str| u32::from_be_bytes(sp_io::hashing::keccak_256(signature.as_bytes())[..4].try_into().expect("four selector bytes"));
        let (receiver, input, value_rao, economic_debit) = match case {
            EvmFeeCase::BalanceTransfer => (H160::repeat_byte(0x22), vec![], amount, i128::from(amount)),
            EvmFeeCase::BalancePrecompile => (H160::from_low_u64_be(BalanceTransferPrecompile::<Runtime>::INDEX), encode_with_selector(selector("transfer(bytes32)"), (destination,)), amount, i128::from(amount)),
            EvmFeeCase::AddStake => (H160::from_low_u64_be(StakingPrecompileV2::<Runtime>::INDEX), encode_with_selector(selector("addStake(bytes32,uint256,uint256)"), (hotkey, U256::from(amount), U256::zero())), 0, i128::from(amount)),
            EvmFeeCase::RemoveStake => (H160::from_low_u64_be(StakingPrecompileV2::<Runtime>::INDEX), encode_with_selector(selector("removeStake(bytes32,uint256,uint256)"), (hotkey, U256::from(amount), U256::zero())), 0, i128::from(amount).checked_neg().expect("unstaking proceeds fit")),
            EvmFeeCase::AddProxy => (H160::from_low_u64_be(ProxyPrecompile::<Runtime>::INDEX), encode_with_selector(selector("addProxy(bytes32,uint8,uint32)"), (destination, 0u8, 0u32)), 0, 0),
        };
        // Larger than the expected consumption so the check exercises gas refunds.
        let gas_limit = U256::from(5_000_000);
        let decimals = U256::from(1_000_000_000);
        let (base_fee, _) = <Runtime as pallet_evm::Config>::FeeCalculator::min_gas_price();
        // Subtensor's vendored EVM runner currently overrides the requested
        // priority fee to None (runner/stack.rs), so both cases pay base fee.
        let gas_price = base_fee;
        let message = ethereum::EIP1559TransactionMessage {
            chain_id: <Runtime as pallet_evm::Config>::ChainId::get(),
            nonce: U256::zero(),
            max_priority_fee_per_gas: U256::from(priority_fee_per_gas),
            max_fee_per_gas: U256::from(100_000_000_000u64),
            gas_limit,
            action: ethereum::TransactionAction::Call(receiver),
            value: U256::from(value_rao)
                .checked_mul(decimals)
                .expect("value fits"),
            input,
            access_list: vec![],
        };
        let hash = message.hash();
        let signature = pair.sign_prehashed(hash.as_fixed_bytes());
        let transaction = ethereum::TransactionV3::EIP1559(ethereum::EIP1559Transaction {
            chain_id: message.chain_id,
            nonce: message.nonce,
            max_priority_fee_per_gas: message.max_priority_fee_per_gas,
            max_fee_per_gas: message.max_fee_per_gas,
            gas_limit: message.gas_limit,
            action: message.action,
            value: message.value,
            input: message.input,
            access_list: message.access_list,
            signature: ethereum::eip2930::TransactionSignature::new(
                signature.0[64] != 0,
                H256::from_slice(&signature.0[..32]),
                H256::from_slice(&signature.0[32..64]),
            )
            .expect("valid Ethereum signature"),
        });
        let sender_before = Balances::free_balance(&sender_account).to_u64()
            .checked_add(Balances::reserved_balance(&sender_account).to_u64()).expect("total sender balance fits");
        let receiver_account = <Runtime as pallet_evm::Config>::AddressMapping::into_account_id(receiver);
        let receiver_before = Balances::free_balance(&receiver_account).to_u64();
        let extrinsic =
            UncheckedExtrinsic::new_bare(RuntimeCall::Ethereum(pallet_ethereum::Call::transact {
                transaction,
            }));
        Executive::apply_extrinsic(extrinsic)
            .expect("signed Ethereum transaction is valid")
            .expect("Ethereum transaction dispatch succeeds");

        // Receipts enter Pending during execution; CurrentReceipts is populated
        // only when the Ethereum block is finalized.
        assert_eq!(pallet_ethereum::Pending::<Runtime>::count(), 1);
        let (_, _, receipt) = pallet_ethereum::Pending::<Runtime>::get(0)
            .expect("Ethereum transaction produces a receipt");
        let receipt = match receipt {
            pallet_ethereum::Receipt::EIP1559(receipt) => Some(receipt),
            _ => None,
        }
        .expect("expected EIP-1559 receipt");
        assert_eq!(receipt.status_code, 1, "EVM transaction succeeds");
        assert!(receipt.used_gas > U256::zero() && receipt.used_gas < gas_limit);
        match case {
            EvmFeeCase::BalanceTransfer => assert_eq!(Balances::free_balance(&receiver_account).to_u64().checked_sub(receiver_before), Some(amount)),
            EvmFeeCase::BalancePrecompile => assert_eq!(Balances::free_balance(&destination_account).to_u64().checked_sub(destination_before), Some(amount)),
            EvmFeeCase::AddStake => assert_eq!(pallet_subtensor::Pallet::<Runtime>::get_stake_for_hotkey_and_coldkey_on_subnet(&hotkey_account, &sender_account, netuid).to_u64().checked_sub(stake_before), Some(amount)),
            EvmFeeCase::RemoveStake => assert_eq!(stake_before.checked_sub(pallet_subtensor::Pallet::<Runtime>::get_stake_for_hotkey_and_coldkey_on_subnet(&hotkey_account, &sender_account, netuid).to_u64()), Some(amount)),
            EvmFeeCase::AddProxy => {
                let (proxies, deposit) = pallet_subtensor_proxy::Proxies::<Runtime>::get(&sender_account);
                assert_eq!(proxies.len(), 1);
                let proxy = proxies.first().expect("proxy added");
                assert_eq!(proxy.delegate, destination_account);
                assert_eq!(proxy.proxy_type, ProxyType::Any);
                assert_eq!(proxy.delay, 0);
                assert!(deposit.to_u64() > 0);
                assert_eq!(Balances::reserved_balance(&sender_account), deposit);
            }
        }
        // Count reserved proxy deposits as owned balance; exclude stake and transfer
        // principal (or unstaking proceeds) from the actual transaction fee.
        let sender_after = Balances::free_balance(&sender_account).to_u64()
            .checked_add(Balances::reserved_balance(&sender_account).to_u64()).expect("total sender balance fits");
        let fee_rao = u64::try_from(i128::from(sender_before).checked_sub(i128::from(sender_after))
            .and_then(|debit| debit.checked_sub(economic_debit)).expect("fee debit fits"))
            .expect("actual fee is nonnegative and fits RAO");
        let expected_fee = receipt
            .used_gas
            .checked_mul(gas_price)
            .expect("gas fee fits")
            .checked_div(decimals)
            .expect("nonzero decimal scale");
        assert_eq!(
            U256::from(fee_rao),
            expected_fee,
            "charge actual gas and refund unused gas"
        );
        fee_rao
    });
    with_settings!({
        comparator => Box::new(FeeToleranceComparator),
        description => ctx.description.unwrap_or_default(),
        snapshot_suffix => ctx.description.unwrap_or_default(),
    }, {
        assert_snapshot!(fee_rao);
    });
}
