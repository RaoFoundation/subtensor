//! Runtime call inventory for proxy filters.
//!
//! Keep this file boring: one generated group per call-bearing runtime pallet.
//! Proxy-specific policy belongs in `mod.rs`.

use frame_system::Call as SystemCall;
use pallet_admin_utils::Call as AdminUtilsCall;
use pallet_balances::Call as BalancesCall;
use pallet_base_fee::Call as BaseFeeCall;
use pallet_commitments::Call as CommitmentsCall;
use pallet_contracts::Call as ContractsCall;
use pallet_crowdloan::Call as CrowdloanCall;
use pallet_drand::Call as DrandCall;
use pallet_ethereum::Call as EthereumCall;
use pallet_evm::Call as EvmCall;
use pallet_grandpa::Call as GrandpaCall;
use pallet_limit_orders::Call as LimitOrdersCall;
use pallet_multisig::Call as MultisigCall;
use pallet_preimage::Call as PreimageCall;
use pallet_safe_mode::Call as SafeModeCall;
use pallet_scheduler::Call as SchedulerCall;
use pallet_shield::Call as MevShieldCall;
use pallet_subtensor::Call as SubtensorCall;
use pallet_subtensor_proxy::Call as ProxyCall;
use pallet_subtensor_swap::Call as SwapCall;
use pallet_subtensor_utility::Call as UtilityCall;
use pallet_sudo::Call as SudoCall;
use pallet_timestamp::Call as TimestampCall;
use subtensor_macros::call_filter_group;
use subtensor_runtime_common::{SMALL_ALPHA_TRANSFER_LIMIT, SMALL_TRANSFER_LIMIT};

call_filter_group!(
    SystemCalls,
    [
        RuntimeCall::System(SystemCall::remark),
        RuntimeCall::System(SystemCall::remark_with_event),
        RuntimeCall::System(SystemCall::set_heap_pages),
        RuntimeCall::System(SystemCall::set_code),
        RuntimeCall::System(SystemCall::set_code_without_checks),
        RuntimeCall::System(SystemCall::set_storage),
        RuntimeCall::System(SystemCall::kill_storage),
        RuntimeCall::System(SystemCall::kill_prefix),
        RuntimeCall::System(SystemCall::authorize_upgrade),
        RuntimeCall::System(SystemCall::authorize_upgrade_without_checks),
        RuntimeCall::System(SystemCall::apply_authorized_upgrade),
    ]
);

call_filter_group!(
    TimestampCalls,
    [RuntimeCall::Timestamp(TimestampCall::set),]
);

call_filter_group!(
    GrandpaCalls,
    [
        RuntimeCall::Grandpa(GrandpaCall::report_equivocation),
        RuntimeCall::Grandpa(GrandpaCall::report_equivocation_unsigned),
        RuntimeCall::Grandpa(GrandpaCall::note_stalled),
    ]
);

call_filter_group!(
    SafeModeCalls,
    [
        RuntimeCall::SafeMode(SafeModeCall::enter),
        RuntimeCall::SafeMode(SafeModeCall::force_enter),
        RuntimeCall::SafeMode(SafeModeCall::extend),
        RuntimeCall::SafeMode(SafeModeCall::force_extend),
        RuntimeCall::SafeMode(SafeModeCall::force_exit),
        RuntimeCall::SafeMode(SafeModeCall::force_slash_deposit),
        RuntimeCall::SafeMode(SafeModeCall::release_deposit),
        RuntimeCall::SafeMode(SafeModeCall::force_release_deposit),
    ]
);

call_filter_group!(
    EthereumCalls,
    [RuntimeCall::Ethereum(EthereumCall::transact),]
);

call_filter_group!(
    EvmCalls,
    [
        RuntimeCall::EVM(EvmCall::withdraw),
        RuntimeCall::EVM(EvmCall::call),
        RuntimeCall::EVM(EvmCall::create),
        RuntimeCall::EVM(EvmCall::create2),
        RuntimeCall::EVM(EvmCall::set_whitelist),
        RuntimeCall::EVM(EvmCall::disable_whitelist),
    ]
);

call_filter_group!(
    BaseFeeCalls,
    [
        RuntimeCall::BaseFee(BaseFeeCall::set_base_fee_per_gas),
        RuntimeCall::BaseFee(BaseFeeCall::set_elasticity),
    ]
);

call_filter_group!(
    DrandCalls,
    [
        RuntimeCall::Drand(DrandCall::write_pulse),
        RuntimeCall::Drand(DrandCall::set_beacon_config),
        RuntimeCall::Drand(DrandCall::set_oldest_stored_round),
    ]
);

call_filter_group!(
    CrowdloanCalls,
    [
        RuntimeCall::Crowdloan(CrowdloanCall::create),
        RuntimeCall::Crowdloan(CrowdloanCall::contribute),
        RuntimeCall::Crowdloan(CrowdloanCall::withdraw),
        RuntimeCall::Crowdloan(CrowdloanCall::finalize),
        RuntimeCall::Crowdloan(CrowdloanCall::refund),
        RuntimeCall::Crowdloan(CrowdloanCall::dissolve),
        RuntimeCall::Crowdloan(CrowdloanCall::update_min_contribution),
        RuntimeCall::Crowdloan(CrowdloanCall::update_end),
        RuntimeCall::Crowdloan(CrowdloanCall::update_cap),
        RuntimeCall::Crowdloan(CrowdloanCall::set_max_contribution),
    ]
);

call_filter_group!(
    SwapCalls,
    [
        RuntimeCall::Swap(SwapCall::toggle_user_liquidity),
        RuntimeCall::Swap(SwapCall::add_liquidity),
        RuntimeCall::Swap(SwapCall::remove_liquidity),
        RuntimeCall::Swap(SwapCall::modify_position),
        RuntimeCall::Swap(SwapCall::disable_lp),
        RuntimeCall::Swap(SwapCall::set_fee_rate),
    ]
);

call_filter_group!(
    ContractsCalls,
    [
        RuntimeCall::Contracts(ContractsCall::call_old_weight),
        RuntimeCall::Contracts(ContractsCall::instantiate_with_code_old_weight),
        RuntimeCall::Contracts(ContractsCall::instantiate_old_weight),
        RuntimeCall::Contracts(ContractsCall::upload_code),
        RuntimeCall::Contracts(ContractsCall::remove_code),
        RuntimeCall::Contracts(ContractsCall::set_code),
        RuntimeCall::Contracts(ContractsCall::call),
        RuntimeCall::Contracts(ContractsCall::instantiate_with_code),
        RuntimeCall::Contracts(ContractsCall::instantiate),
        RuntimeCall::Contracts(ContractsCall::migrate),
    ]
);

call_filter_group!(
    MevShieldCalls,
    [
        RuntimeCall::MevShield(MevShieldCall::announce_next_key),
        RuntimeCall::MevShield(MevShieldCall::submit_encrypted),
        RuntimeCall::MevShield(MevShieldCall::store_encrypted),
        RuntimeCall::MevShield(MevShieldCall::set_max_pending_extrinsics_number),
        RuntimeCall::MevShield(MevShieldCall::set_on_initialize_weight),
        RuntimeCall::MevShield(MevShieldCall::set_stored_extrinsic_lifetime),
        RuntimeCall::MevShield(MevShieldCall::set_max_extrinsic_weight),
    ]
);

call_filter_group!(
    LimitOrdersCalls,
    [
        RuntimeCall::LimitOrders(LimitOrdersCall::execute_orders),
        RuntimeCall::LimitOrders(LimitOrdersCall::execute_batched_orders),
        RuntimeCall::LimitOrders(LimitOrdersCall::cancel_order),
        RuntimeCall::LimitOrders(LimitOrdersCall::set_pallet_status),
    ]
);

call_filter_group!(
    UtilityCalls,
    [
        RuntimeCall::Utility(UtilityCall::batch),
        RuntimeCall::Utility(UtilityCall::as_derivative),
        RuntimeCall::Utility(UtilityCall::batch_all),
        RuntimeCall::Utility(UtilityCall::dispatch_as),
        RuntimeCall::Utility(UtilityCall::force_batch),
        RuntimeCall::Utility(UtilityCall::with_weight),
        RuntimeCall::Utility(UtilityCall::if_else),
        RuntimeCall::Utility(UtilityCall::dispatch_as_fallible),
    ]
);

call_filter_group!(
    SudoCalls,
    [
        RuntimeCall::Sudo(SudoCall::sudo),
        RuntimeCall::Sudo(SudoCall::sudo_unchecked_weight),
        RuntimeCall::Sudo(SudoCall::set_key),
        RuntimeCall::Sudo(SudoCall::sudo_as),
        RuntimeCall::Sudo(SudoCall::remove_key),
    ]
);

call_filter_group!(
    MultisigCalls,
    [
        RuntimeCall::Multisig(MultisigCall::as_multi_threshold_1),
        RuntimeCall::Multisig(MultisigCall::as_multi),
        RuntimeCall::Multisig(MultisigCall::approve_as_multi),
        RuntimeCall::Multisig(MultisigCall::cancel_as_multi),
        RuntimeCall::Multisig(MultisigCall::poke_deposit),
    ]
);

call_filter_group!(
    PreimageCalls,
    [
        RuntimeCall::Preimage(PreimageCall::note_preimage),
        RuntimeCall::Preimage(PreimageCall::unnote_preimage),
        RuntimeCall::Preimage(PreimageCall::request_preimage),
        RuntimeCall::Preimage(PreimageCall::unrequest_preimage),
        RuntimeCall::Preimage(PreimageCall::ensure_updated),
    ]
);

call_filter_group!(
    SchedulerCalls,
    [
        RuntimeCall::Scheduler(SchedulerCall::schedule),
        RuntimeCall::Scheduler(SchedulerCall::cancel),
        RuntimeCall::Scheduler(SchedulerCall::schedule_named),
        RuntimeCall::Scheduler(SchedulerCall::cancel_named),
        RuntimeCall::Scheduler(SchedulerCall::schedule_after),
        RuntimeCall::Scheduler(SchedulerCall::schedule_named_after),
        RuntimeCall::Scheduler(SchedulerCall::set_retry),
        RuntimeCall::Scheduler(SchedulerCall::set_retry_named),
        RuntimeCall::Scheduler(SchedulerCall::cancel_retry),
        RuntimeCall::Scheduler(SchedulerCall::cancel_retry_named),
    ]
);

call_filter_group!(
    ProxyCalls,
    [
        RuntimeCall::Proxy(ProxyCall::proxy),
        RuntimeCall::Proxy(ProxyCall::add_proxy),
        RuntimeCall::Proxy(ProxyCall::remove_proxy),
        RuntimeCall::Proxy(ProxyCall::remove_proxies),
        RuntimeCall::Proxy(ProxyCall::create_pure),
        RuntimeCall::Proxy(ProxyCall::kill_pure),
        RuntimeCall::Proxy(ProxyCall::announce),
        RuntimeCall::Proxy(ProxyCall::remove_announcement),
        RuntimeCall::Proxy(ProxyCall::reject_announcement),
        RuntimeCall::Proxy(ProxyCall::proxy_announced),
        RuntimeCall::Proxy(ProxyCall::poke_deposit),
        RuntimeCall::Proxy(ProxyCall::set_real_pays_fee),
    ]
);

call_filter_group!(
    CommitmentsCalls,
    [
        RuntimeCall::Commitments(CommitmentsCall::set_commitment),
        RuntimeCall::Commitments(CommitmentsCall::set_max_space),
    ]
);

// Ordinary balance transfers — moving your own free balance.
call_filter_group!(
    BalanceTransferCalls,
    [
        RuntimeCall::Balances(BalancesCall::transfer_keep_alive),
        RuntimeCall::Balances(BalancesCall::transfer_allow_death),
        RuntimeCall::Balances(BalancesCall::transfer_all),
    ]
);

// Privileged, root-only balance operations (force transfer/unreserve, burn,
// issuance adjustment).
call_filter_group!(
    BalanceMaintenanceCalls,
    [
        RuntimeCall::Balances(BalancesCall::force_transfer),
        RuntimeCall::Balances(BalancesCall::force_unreserve),
        RuntimeCall::Balances(BalancesCall::upgrade_accounts),
        RuntimeCall::Balances(BalancesCall::force_set_balance),
        RuntimeCall::Balances(BalancesCall::force_adjust_total_issuance),
        RuntimeCall::Balances(BalancesCall::burn),
    ]
);

// Managing your own stake: add, remove, and move between hotkeys/subnets.
call_filter_group!(
    StakeManagementCalls,
    [
        RuntimeCall::SubtensorModule(SubtensorCall::add_stake),
        RuntimeCall::SubtensorModule(SubtensorCall::add_stake_limit),
        RuntimeCall::SubtensorModule(SubtensorCall::remove_stake),
        RuntimeCall::SubtensorModule(SubtensorCall::remove_stake_limit),
        RuntimeCall::SubtensorModule(SubtensorCall::remove_stake_full_limit),
        RuntimeCall::SubtensorModule(SubtensorCall::unstake_all),
        RuntimeCall::SubtensorModule(SubtensorCall::unstake_all_alpha),
        RuntimeCall::SubtensorModule(SubtensorCall::move_stake),
        RuntimeCall::SubtensorModule(SubtensorCall::swap_stake),
        RuntimeCall::SubtensorModule(SubtensorCall::swap_stake_limit),
    ]
);

// Moving staked value to another coldkey — the stake analogue of a transfer.
call_filter_group!(
    StakeTransferCalls,
    [RuntimeCall::SubtensorModule(SubtensorCall::transfer_stake),]
);

// Permissionless proof-of-work registration (costs no TAO).
call_filter_group!(
    PowRegistrationCalls,
    [
        RuntimeCall::SubtensorModule(SubtensorCall::register),
        RuntimeCall::SubtensorModule(SubtensorCall::register_limit),
    ]
);

// Registration paid by burning TAO (spends value, unlike POW registration).
call_filter_group!(
    BurnedRegistrationCalls,
    [RuntimeCall::SubtensorModule(SubtensorCall::burned_register),]
);

// Registration into the root subnet.
call_filter_group!(
    RootRegistrationCalls,
    [RuntimeCall::SubtensorModule(SubtensorCall::root_register),]
);

// Development/testnet faucet. This call only exists when the runtime is built
// with `pow-faucet`, so the group is empty in production-feature builds.
#[cfg(feature = "pow-faucet")]
call_filter_group!(
    FaucetCalls,
    [RuntimeCall::SubtensorModule(SubtensorCall::faucet),]
);

#[cfg(not(feature = "pow-faucet"))]
pub(super) struct FaucetCalls;

#[cfg(not(feature = "pow-faucet"))]
impl frame_support::traits::Contains<crate::RuntimeCall> for FaucetCalls {
    fn contains(_call: &crate::RuntimeCall) -> bool {
        false
    }
}

#[cfg(not(feature = "pow-faucet"))]
impl subtensor_runtime_common::CallFilterMetadata for FaucetCalls {
    fn call_infos() -> ::alloc::vec::Vec<subtensor_runtime_common::CallInfo> {
        ::alloc::vec::Vec::new()
    }
}

// Rotating a neuron's hotkey.
call_filter_group!(
    HotkeySwapCalls,
    [
        RuntimeCall::SubtensorModule(SubtensorCall::swap_hotkey),
        RuntimeCall::SubtensorModule(SubtensorCall::swap_hotkey_v2),
    ]
);

// Rotating a coldkey — the full account-takeover surface.
call_filter_group!(
    ColdkeySwapCalls,
    [
        RuntimeCall::SubtensorModule(SubtensorCall::schedule_swap_coldkey),
        RuntimeCall::SubtensorModule(SubtensorCall::swap_coldkey),
        RuntimeCall::SubtensorModule(SubtensorCall::announce_coldkey_swap),
        RuntimeCall::SubtensorModule(SubtensorCall::swap_coldkey_announced),
        RuntimeCall::SubtensorModule(SubtensorCall::clear_coldkey_swap_announcement),
        RuntimeCall::SubtensorModule(SubtensorCall::dispute_coldkey_swap),
        RuntimeCall::SubtensorModule(SubtensorCall::reset_coldkey_swap),
    ]
);

// Dissolving a subnet — irreversible network destruction.
call_filter_group!(
    CriticalNetworkCalls,
    [
        RuntimeCall::SubtensorModule(SubtensorCall::dissolve_network),
        RuntimeCall::SubtensorModule(SubtensorCall::root_dissolve_network),
    ]
);

// Delegating a hotkey's work to child keys.
call_filter_group!(
    ChildKeyCalls,
    [
        RuntimeCall::SubtensorModule(SubtensorCall::set_children),
        RuntimeCall::SubtensorModule(SubtensorCall::set_childkey_take),
    ]
);

// Claiming accumulated root dividends.
call_filter_group!(
    RootClaimCalls,
    [RuntimeCall::SubtensorModule(SubtensorCall::claim_root),]
);

// Selecting how root dividends are claimed (a staking-side setting).
call_filter_group!(
    RootClaimTypeCalls,
    [RuntimeCall::SubtensorModule(
        SubtensorCall::set_root_claim_type
    ),]
);

// A subnet's public identity and token symbol.
call_filter_group!(
    SubnetIdentityCalls,
    [
        RuntimeCall::SubtensorModule(SubtensorCall::set_subnet_identity),
        RuntimeCall::SubtensorModule(SubtensorCall::update_symbol),
    ]
);

// Starting a subnet's emission schedule (start_call).
call_filter_group!(
    SubnetActivationCalls,
    [RuntimeCall::SubtensorModule(SubtensorCall::start_call),]
);

// Residual pallet-subtensor calls that no proxy needs to grant on their own:
// weights, serving, delegate-take, alpha lock/burn/preferences, network
// registration, childkey admin, account association, tempo control, voting
// power, root-claim admin, and lease teardown.
call_filter_group!(
    SubtensorCommonCalls,
    [
        RuntimeCall::SubtensorModule(SubtensorCall::set_weights),
        RuntimeCall::SubtensorModule(SubtensorCall::set_mechanism_weights),
        RuntimeCall::SubtensorModule(SubtensorCall::batch_set_weights),
        RuntimeCall::SubtensorModule(SubtensorCall::commit_weights),
        RuntimeCall::SubtensorModule(SubtensorCall::commit_mechanism_weights),
        RuntimeCall::SubtensorModule(SubtensorCall::batch_commit_weights),
        RuntimeCall::SubtensorModule(SubtensorCall::commit_crv3_mechanism_weights),
        RuntimeCall::SubtensorModule(SubtensorCall::commit_timelocked_weights),
        RuntimeCall::SubtensorModule(SubtensorCall::commit_timelocked_mechanism_weights),
        RuntimeCall::SubtensorModule(SubtensorCall::reveal_weights),
        RuntimeCall::SubtensorModule(SubtensorCall::reveal_mechanism_weights),
        RuntimeCall::SubtensorModule(SubtensorCall::batch_reveal_weights),
        RuntimeCall::SubtensorModule(SubtensorCall::add_stake_burn),
        RuntimeCall::SubtensorModule(SubtensorCall::lock_stake),
        RuntimeCall::SubtensorModule(SubtensorCall::move_lock),
        RuntimeCall::SubtensorModule(SubtensorCall::set_perpetual_lock),
        RuntimeCall::SubtensorModule(SubtensorCall::set_reject_locked_alpha),
        RuntimeCall::SubtensorModule(SubtensorCall::recycle_alpha),
        RuntimeCall::SubtensorModule(SubtensorCall::burn_alpha),
        RuntimeCall::SubtensorModule(SubtensorCall::register_network),
        RuntimeCall::SubtensorModule(SubtensorCall::register_network_with_identity),
        RuntimeCall::SubtensorModule(SubtensorCall::register_leased_network),
        RuntimeCall::SubtensorModule(SubtensorCall::decrease_take),
        RuntimeCall::SubtensorModule(SubtensorCall::increase_take),
        RuntimeCall::SubtensorModule(SubtensorCall::serve_axon),
        RuntimeCall::SubtensorModule(SubtensorCall::serve_axon_tls),
        RuntimeCall::SubtensorModule(SubtensorCall::serve_prometheus),
        RuntimeCall::SubtensorModule(SubtensorCall::set_identity),
        RuntimeCall::SubtensorModule(SubtensorCall::try_associate_hotkey),
        RuntimeCall::SubtensorModule(SubtensorCall::associate_evm_key),
        RuntimeCall::SubtensorModule(SubtensorCall::set_coldkey_auto_stake_hotkey),
        RuntimeCall::SubtensorModule(SubtensorCall::set_pending_childkey_cooldown),
        RuntimeCall::SubtensorModule(SubtensorCall::set_auto_parent_delegation_enabled),
        RuntimeCall::SubtensorModule(SubtensorCall::sudo_set_tx_childkey_take_rate_limit),
        RuntimeCall::SubtensorModule(SubtensorCall::sudo_set_min_childkey_take),
        RuntimeCall::SubtensorModule(SubtensorCall::sudo_set_max_childkey_take),
        RuntimeCall::SubtensorModule(SubtensorCall::terminate_lease),
        RuntimeCall::SubtensorModule(SubtensorCall::set_tempo),
        RuntimeCall::SubtensorModule(SubtensorCall::set_activity_cutoff_factor),
        RuntimeCall::SubtensorModule(SubtensorCall::trigger_epoch),
        RuntimeCall::SubtensorModule(SubtensorCall::sudo_set_num_root_claims),
        RuntimeCall::SubtensorModule(SubtensorCall::sudo_set_root_claim_threshold),
        RuntimeCall::SubtensorModule(SubtensorCall::enable_voting_power_tracking),
        RuntimeCall::SubtensorModule(SubtensorCall::disable_voting_power_tracking),
        RuntimeCall::SubtensorModule(SubtensorCall::sudo_set_voting_power_ema_alpha),
    ]
);

// Subnet parameters a subnet owner may set directly (the admin-utils calls
// guarded by `ensure_sn_owner_or_root`). These are the genuine owner/lease
// management surface, as opposed to the root-only `RootConfigCalls`.
call_filter_group!(
    SubnetManagementCalls,
    [
        RuntimeCall::AdminUtils(AdminUtilsCall::sudo_set_serving_rate_limit),
        RuntimeCall::AdminUtils(AdminUtilsCall::sudo_set_max_difficulty),
        RuntimeCall::AdminUtils(AdminUtilsCall::sudo_set_weights_version_key),
        RuntimeCall::AdminUtils(AdminUtilsCall::sudo_set_adjustment_alpha),
        RuntimeCall::AdminUtils(AdminUtilsCall::sudo_set_immunity_period),
        RuntimeCall::AdminUtils(AdminUtilsCall::sudo_set_min_allowed_weights),
        RuntimeCall::AdminUtils(AdminUtilsCall::sudo_set_rho),
        RuntimeCall::AdminUtils(AdminUtilsCall::sudo_set_activity_cutoff),
        RuntimeCall::AdminUtils(AdminUtilsCall::sudo_set_min_burn),
        RuntimeCall::AdminUtils(AdminUtilsCall::sudo_set_max_burn),
        RuntimeCall::AdminUtils(AdminUtilsCall::sudo_set_bonds_moving_average),
        RuntimeCall::AdminUtils(AdminUtilsCall::sudo_set_bonds_penalty),
        RuntimeCall::AdminUtils(AdminUtilsCall::sudo_set_min_childkey_take_per_subnet),
        RuntimeCall::AdminUtils(AdminUtilsCall::sudo_set_commit_reveal_weights_enabled),
        RuntimeCall::AdminUtils(AdminUtilsCall::sudo_set_liquid_alpha_enabled),
        RuntimeCall::AdminUtils(AdminUtilsCall::sudo_set_alpha_values),
        RuntimeCall::AdminUtils(AdminUtilsCall::sudo_set_commit_reveal_weights_interval),
        RuntimeCall::AdminUtils(AdminUtilsCall::sudo_set_toggle_transfer),
        RuntimeCall::AdminUtils(AdminUtilsCall::sudo_set_recycle_or_burn),
        RuntimeCall::AdminUtils(AdminUtilsCall::sudo_set_alpha_sigmoid_steepness),
        RuntimeCall::AdminUtils(AdminUtilsCall::sudo_set_yuma3_enabled),
        RuntimeCall::AdminUtils(AdminUtilsCall::sudo_set_bonds_reset_enabled),
        RuntimeCall::AdminUtils(AdminUtilsCall::sudo_set_owner_immune_neuron_limit),
        RuntimeCall::AdminUtils(AdminUtilsCall::sudo_set_mechanism_count),
        RuntimeCall::AdminUtils(AdminUtilsCall::sudo_set_mechanism_emission_split),
        RuntimeCall::AdminUtils(AdminUtilsCall::sudo_trim_to_max_allowed_uids),
        RuntimeCall::AdminUtils(AdminUtilsCall::sudo_set_max_allowed_uids),
        RuntimeCall::AdminUtils(AdminUtilsCall::sudo_set_burn_half_life),
        RuntimeCall::AdminUtils(AdminUtilsCall::sudo_set_burn_increase_mult),
        RuntimeCall::AdminUtils(AdminUtilsCall::sudo_set_owner_cut_enabled),
        RuntimeCall::AdminUtils(AdminUtilsCall::sudo_set_owner_cut_auto_lock_enabled),
    ]
);

// Admin parameters that require root (set via sudo / governance). A subnet
// owner cannot call these; they reach the broad proxies only as inert grants
// (the dispatch's `ensure_root` rejects a proxy's signed origin). Includes the
// two deprecated extrinsics that always error.
call_filter_group!(
    RootConfigCalls,
    [
        RuntimeCall::AdminUtils(AdminUtilsCall::swap_authorities),
        RuntimeCall::AdminUtils(AdminUtilsCall::schedule_grandpa_change),
        RuntimeCall::AdminUtils(AdminUtilsCall::sudo_set_default_take),
        RuntimeCall::AdminUtils(AdminUtilsCall::sudo_set_tx_rate_limit),
        RuntimeCall::AdminUtils(AdminUtilsCall::sudo_set_min_difficulty),
        RuntimeCall::AdminUtils(AdminUtilsCall::sudo_set_weights_set_rate_limit),
        RuntimeCall::AdminUtils(AdminUtilsCall::sudo_set_adjustment_interval),
        RuntimeCall::AdminUtils(AdminUtilsCall::sudo_set_kappa),
        RuntimeCall::AdminUtils(AdminUtilsCall::sudo_set_network_registration_allowed),
        RuntimeCall::AdminUtils(AdminUtilsCall::sudo_set_network_pow_registration_allowed),
        RuntimeCall::AdminUtils(AdminUtilsCall::sudo_set_target_registrations_per_interval),
        RuntimeCall::AdminUtils(AdminUtilsCall::sudo_set_difficulty),
        RuntimeCall::AdminUtils(AdminUtilsCall::sudo_set_max_allowed_validators),
        RuntimeCall::AdminUtils(AdminUtilsCall::sudo_set_max_registrations_per_block),
        RuntimeCall::AdminUtils(AdminUtilsCall::sudo_set_subnet_owner_cut),
        RuntimeCall::AdminUtils(AdminUtilsCall::sudo_set_network_rate_limit),
        RuntimeCall::AdminUtils(AdminUtilsCall::sudo_set_tempo),
        RuntimeCall::AdminUtils(AdminUtilsCall::sudo_set_total_issuance),
        RuntimeCall::AdminUtils(AdminUtilsCall::sudo_set_network_immunity_period),
        RuntimeCall::AdminUtils(AdminUtilsCall::sudo_set_network_min_lock_cost),
        RuntimeCall::AdminUtils(AdminUtilsCall::sudo_set_subnet_limit),
        RuntimeCall::AdminUtils(AdminUtilsCall::sudo_set_lock_reduction_interval),
        RuntimeCall::AdminUtils(AdminUtilsCall::sudo_set_rao_recycled),
        RuntimeCall::AdminUtils(AdminUtilsCall::sudo_set_stake_threshold),
        RuntimeCall::AdminUtils(AdminUtilsCall::sudo_set_nominator_min_required_stake),
        RuntimeCall::AdminUtils(AdminUtilsCall::sudo_set_tx_delegate_take_rate_limit),
        RuntimeCall::AdminUtils(AdminUtilsCall::sudo_set_min_delegate_take),
        RuntimeCall::AdminUtils(AdminUtilsCall::sudo_set_dissolve_network_schedule_duration),
        RuntimeCall::AdminUtils(AdminUtilsCall::sudo_set_evm_chain_id),
        RuntimeCall::AdminUtils(AdminUtilsCall::sudo_toggle_evm_precompile),
        RuntimeCall::AdminUtils(AdminUtilsCall::sudo_set_subnet_moving_alpha),
        RuntimeCall::AdminUtils(AdminUtilsCall::sudo_set_ema_price_halving_period),
        RuntimeCall::AdminUtils(AdminUtilsCall::sudo_set_subtoken_enabled),
        RuntimeCall::AdminUtils(AdminUtilsCall::sudo_set_commit_reveal_version),
        RuntimeCall::AdminUtils(AdminUtilsCall::sudo_set_ck_burn),
        RuntimeCall::AdminUtils(AdminUtilsCall::sudo_set_admin_freeze_window),
        RuntimeCall::AdminUtils(AdminUtilsCall::sudo_set_owner_hparam_rate_limit),
        RuntimeCall::AdminUtils(AdminUtilsCall::sudo_set_min_allowed_uids),
        RuntimeCall::AdminUtils(AdminUtilsCall::sudo_set_min_non_immune_uids),
        RuntimeCall::AdminUtils(AdminUtilsCall::sudo_set_tao_flow_cutoff),
        RuntimeCall::AdminUtils(AdminUtilsCall::sudo_set_tao_flow_normalization_exponent),
        RuntimeCall::AdminUtils(AdminUtilsCall::sudo_set_tao_flow_smoothing_factor),
        RuntimeCall::AdminUtils(AdminUtilsCall::sudo_set_net_tao_flow_enabled),
        RuntimeCall::AdminUtils(AdminUtilsCall::sudo_set_max_mechanism_count),
        RuntimeCall::AdminUtils(AdminUtilsCall::sudo_set_start_call_delay),
        RuntimeCall::AdminUtils(AdminUtilsCall::sudo_set_coldkey_swap_announcement_delay),
        RuntimeCall::AdminUtils(AdminUtilsCall::sudo_set_coldkey_swap_reannouncement_delay),
        RuntimeCall::AdminUtils(AdminUtilsCall::sudo_set_subnet_emission_enabled),
        RuntimeCall::AdminUtils(AdminUtilsCall::sudo_set_max_epochs_per_block),
    ]
);

// Rotating a subnet's owner key — an ownership-takeover vector, deliberately
// kept out of the Owner proxy.
call_filter_group!(
    OwnerKeyCalls,
    [RuntimeCall::AdminUtils(
        AdminUtilsCall::sudo_set_sn_owner_hotkey
    ),]
);

// ============================================================================
// Conditional, proxy-specific groups
//
// These carry amount / nested-call constraints, so they overlap the inventory
// groups above (e.g. `transfer_keep_alive` is also in `BalanceTransferCalls`).
// They are deliberately excluded from `AllCalls` to keep that a non-overlapping
// partition. Generating them with the same macro keeps the executable filter
// and the client-facing metadata in lockstep.
// ============================================================================

// `SmallTransfer`: amount-bounded balance and stake transfers.
call_filter_group!(SmallTransferCalls, [
    RuntimeCall::Balances(BalancesCall::transfer_keep_alive)
        where value < SMALL_TRANSFER_LIMIT,
    RuntimeCall::Balances(BalancesCall::transfer_allow_death)
        where value < SMALL_TRANSFER_LIMIT,
    RuntimeCall::SubtensorModule(SubtensorCall::transfer_stake)
        where alpha_amount < SMALL_ALPHA_TRANSFER_LIMIT,
]);

// `SudoUncheckedSetCode`: a single sudo call, only when it wraps
// `System::set_code`.
call_filter_group!(SudoSetCodeCalls, [
    RuntimeCall::Sudo(SudoCall::sudo_unchecked_weight)
        where nested(call) == RuntimeCall::System(SystemCall::set_code),
]);

// Full inventory of every runtime call, used only by the coverage test that
// checks it against `RuntimeCall` metadata. Nested in three blocks so the
// flattened tuple stays within the `CallFilterMetadata` tuple-impl arity;
// `call_infos()` recurses regardless.
// Infrastructure pallets granted wholesale to the broad proxies, excluding
// `SudoCalls` (which `NonCritical` denies). Shared by the proxy policy in
// `mod.rs` and by the `WholesalePalletCalls` inventory below so the list lives
// in one place.
pub(super) type InfraCommonCalls = (
    SystemCalls,
    TimestampCalls,
    GrandpaCalls,
    UtilityCalls,
    MultisigCalls,
    PreimageCalls,
    SchedulerCalls,
    ProxyCalls,
    CommitmentsCalls,
    SafeModeCalls,
    EthereumCalls,
    EvmCalls,
    BaseFeeCalls,
    DrandCalls,
    CrowdloanCalls,
    SwapCalls,
    ContractsCalls,
    MevShieldCalls,
    LimitOrdersCalls,
);

#[cfg(test)]
pub(super) type AllCalls = (
    WholesalePalletCalls,
    SubtensorSplitCalls,
    AdminUtilsSplitCalls,
);

// Pallets every granting proxy grants in full.
#[cfg(test)]
type WholesalePalletCalls = (InfraCommonCalls, SudoCalls);

// Balances + pallet-subtensor, split by proxy membership.
#[cfg(test)]
type SubtensorSplitCalls = (
    BalanceTransferCalls,
    BalanceMaintenanceCalls,
    StakeManagementCalls,
    StakeTransferCalls,
    PowRegistrationCalls,
    BurnedRegistrationCalls,
    RootRegistrationCalls,
    FaucetCalls,
    HotkeySwapCalls,
    ColdkeySwapCalls,
    CriticalNetworkCalls,
    ChildKeyCalls,
    RootClaimCalls,
    RootClaimTypeCalls,
    SubnetIdentityCalls,
    SubnetActivationCalls,
    SubtensorCommonCalls,
);

// admin-utils, split for the Owner and SubnetLeaseBeneficiary proxies.
#[cfg(test)]
type AdminUtilsSplitCalls = (SubnetManagementCalls, RootConfigCalls, OwnerKeyCalls);

#[cfg(test)]
mod tests {
    use super::*;
    use crate::RuntimeCall;
    use alloc::{collections::BTreeSet, format, string::String, vec::Vec};
    use frame_support::traits::GetCallMetadata;
    use subtensor_runtime_common::{CallFilterMetadata, CallInfo};

    #[test]
    fn all_call_groups_cover_runtime_call_metadata() {
        let groups_call_infos = AllCalls::call_infos();
        let groups_call_names = call_names_from_group_infos(&groups_call_infos);
        let runtime_call_names = call_names_from_runtime_metadata();
        let duplicate_group_calls = duplicate_call_names(&groups_call_infos);

        let missing_from_groups = runtime_call_names
            .difference(&groups_call_names)
            .cloned()
            .collect::<Vec<_>>();
        let extra_in_groups = groups_call_names
            .difference(&runtime_call_names)
            .cloned()
            .collect::<Vec<_>>();

        assert!(
            missing_from_groups.is_empty()
                && extra_in_groups.is_empty()
                && duplicate_group_calls.is_empty(),
            "AllCalls inventory does not match RuntimeCall metadata.\n\
             call_infos: {}\n\
             runtime calls: {}\n\
             missing from AllCalls ({}):\n{}\n\
             extra in AllCalls ({}):\n{}\n\
             duplicates in AllCalls ({}):\n{}",
            groups_call_names.len(),
            runtime_call_names.len(),
            missing_from_groups.len(),
            format_call_list(&missing_from_groups),
            extra_in_groups.len(),
            format_call_list(&extra_in_groups),
            duplicate_group_calls.len(),
            format_call_list(&duplicate_group_calls),
        );
    }

    fn call_names_from_group_infos(call_infos: &[CallInfo]) -> BTreeSet<String> {
        call_infos.iter().map(format_call_info).collect()
    }

    fn call_names_from_runtime_metadata() -> BTreeSet<String> {
        RuntimeCall::get_module_names()
            .iter()
            .flat_map(|module| {
                RuntimeCall::get_call_names(module)
                    .iter()
                    .map(move |call| format!("{}::{}", module, call))
            })
            .collect()
    }

    fn duplicate_call_names(call_infos: &[CallInfo]) -> Vec<String> {
        let mut seen = BTreeSet::new();
        let mut duplicates = Vec::new();

        for call_info in call_infos {
            let call_name = format_call_info(call_info);
            if !seen.insert(call_name.clone()) {
                duplicates.push(call_name);
            }
        }

        duplicates
    }

    fn format_call_info(call_info: &CallInfo) -> String {
        format!(
            "{}::{}",
            String::from_utf8_lossy(&call_info.pallet_name),
            String::from_utf8_lossy(&call_info.call_name)
        )
    }

    fn format_call_list(calls: &[String]) -> String {
        if calls.is_empty() {
            return "  <none>".into();
        }

        calls
            .iter()
            .map(|call| format!("  {}", call))
            .collect::<Vec<_>>()
            .join("\n")
    }
}
