//! Custom Subtensor runtime APIs consumed by the node JSON-RPC layer and indexers.
//!
//! Trait and method **names are frozen** (Tier C): clients and `decl_runtime_apis!`
//! versioning depend on them. Prefer docs and call-site clarity over renames.
//!
//! # Where implementations live
//!
//! - Runtime wiring: `runtime/src/lib.rs` (`impl …RuntimeApi<Block> for Runtime`)
//! - Pallet builders: [`pallet_subtensor::rpc_info`] (delegate / neuron / subnet /
//!   stake / metagraph views), plus staking lock / coinbase helpers for a few queries
//!
//! # Related crate
//!
//! JSON-RPC method strings and SCALE-encoding wrappers:
//! `subtensor-custom-rpc` (`pallets/subtensor/rpc`).

#![cfg_attr(not(feature = "std"), no_std)]
extern crate alloc;
use alloc::collections::BTreeMap;
use alloc::vec::Vec;
use codec::Compact;
use pallet_subtensor::rpc_info::{
    delegate_info::DelegateInfo,
    dynamic_info::DynamicInfo,
    metagraph::{Metagraph, SelectiveMetagraph},
    neuron_info::{NeuronInfo, NeuronInfoLite},
    show_subnet::SubnetState,
    stake_info::{StakeAvailability, StakeInfo},
    subnet_info::{
        SubnetHyperparams, SubnetHyperparamsV2, SubnetHyperparamsV3, SubnetInfo, SubnetInfov2,
    },
};
use pallet_subtensor::staking::lock::LockState;
use sp_runtime::AccountId32;
use substrate_fixed::types::U64F64;
use subtensor_runtime_common::{
    AlphaBalance, MechId, NetUid, ProxyFilterInfo, ProxyTypeInfo, TaoBalance,
};

sp_api::decl_runtime_apis! {
    /// Delegate (validator hotkey) RPC views: take, nominators, registrations, returns.
    pub trait DelegateInfoRuntimeApi {
        /// All hotkeys currently in `Delegates`, each with a full nominator list.
        fn get_delegates() -> Vec<DelegateInfo<AccountId32>>;
        /// One delegate hotkey, or `None` if it is not in `Delegates`.
        fn get_delegate( delegate_account: AccountId32 ) -> Option<DelegateInfo<AccountId32>>;
        /// Delegates that `delegatee_account` (coldkey) has stake on, with `(netuid, alpha)` per row.
        fn get_delegated( delegatee_account: AccountId32 ) -> Vec<(DelegateInfo<AccountId32>, (Compact<NetUid>, Compact<AlphaBalance>))>;
    }

    /// Per-uid neuron views for a subnet (full and lite).
    pub trait NeuronInfoRuntimeApi {
        /// Full [`NeuronInfo`] for every uid on `netuid`.
        fn get_neurons(netuid: NetUid) -> Vec<NeuronInfo<AccountId32>>;
        /// Full [`NeuronInfo`] for one uid, or `None` if unset / out of range.
        fn get_neuron(netuid: NetUid, uid: u16) -> Option<NeuronInfo<AccountId32>>;
        /// Lite [`NeuronInfoLite`] rows for every uid on `netuid`.
        fn get_neurons_lite(netuid: NetUid) -> Vec<NeuronInfoLite<AccountId32>>;
        /// Lite [`NeuronInfoLite`] for one uid, or `None` if unset / out of range.
        fn get_neuron_lite(netuid: NetUid, uid: u16) -> Option<NeuronInfoLite<AccountId32>>;
    }

    /// Subnet metadata, hyperparams, dynamic pool state, and metagraph snapshots.
    pub trait SubnetInfoRuntimeApi {
        /// Legacy [`SubnetInfo`] for one subnet.
        fn get_subnet_info(netuid: NetUid) -> Option<SubnetInfo<AccountId32>>;
        /// Legacy [`SubnetInfo`] for all netuids (sparse: `None` gaps allowed).
        fn get_subnets_info() -> Vec<Option<SubnetInfo<AccountId32>>>;
        /// [`SubnetInfov2`] for one subnet.
        fn get_subnet_info_v2(netuid: NetUid) -> Option<SubnetInfov2<AccountId32>>;
        /// [`SubnetInfov2`] for all netuids (sparse: `None` gaps allowed).
        fn get_subnets_info_v2() -> Vec<Option<SubnetInfov2<AccountId32>>>;
        #[deprecated(note = "Use `get_subnet_hyperparams_v3` instead.")]
        fn get_subnet_hyperparams(netuid: NetUid) -> Option<SubnetHyperparams>;
        #[deprecated(note = "Use `get_subnet_hyperparams_v3` instead.")]
        fn get_subnet_hyperparams_v2(netuid: NetUid) -> Option<SubnetHyperparamsV2>;
        /// Current subnet hyperparameters (`SubnetHyperparamsV3`).
        #[api_version(2)]
        fn get_subnet_hyperparams_v3(netuid: NetUid) -> Option<SubnetHyperparamsV3>;
        /// [`DynamicInfo`] for every subnet (sparse).
        fn get_all_dynamic_info() -> Vec<Option<DynamicInfo<AccountId32>>>;
        /// Root mechanism metagraph for every subnet (sparse).
        fn get_all_metagraphs() -> Vec<Option<Metagraph<AccountId32>>>;
        /// Root mechanism [`Metagraph`] for one subnet.
        fn get_metagraph(netuid: NetUid) -> Option<Metagraph<AccountId32>>;
        /// All mechanisms' metagraphs across subnets (sparse).
        fn get_all_mechagraphs() -> Vec<Option<Metagraph<AccountId32>>>;
        /// [`Metagraph`] for one `(netuid, mecid)` mechanism.
        fn get_mechagraph(netuid: NetUid, mecid: MechId) -> Option<Metagraph<AccountId32>>;
        /// [`DynamicInfo`] for one subnet.
        fn get_dynamic_info(netuid: NetUid) -> Option<DynamicInfo<AccountId32>>;
        /// [`SubnetState`] show-subnet snapshot for one netuid.
        fn get_subnet_state(netuid: NetUid) -> Option<SubnetState<AccountId32>>;
        /// Partial root metagraph: only columns listed in `metagraph_indexes`.
        fn get_selective_metagraph(netuid: NetUid, metagraph_indexes: Vec<u16>) -> Option<SelectiveMetagraph<AccountId32>>;
        /// Hotkey selected for coldkey auto-stake on `netuid`, if any.
        fn get_coldkey_auto_stake_hotkey(coldkey: AccountId32, netuid: NetUid) -> Option<AccountId32>;
        /// Partial mechanism metagraph for `(netuid, subid)`; `subid` is a [`MechId`].
        fn get_selective_mechagraph(netuid: NetUid, subid: MechId, metagraph_indexes: Vec<u16>) -> Option<SelectiveMetagraph<AccountId32>>;
        /// Netuid that would be pruned next under current immunity / emission rules, if any.
        fn get_subnet_to_prune() -> Option<NetUid>;
        /// Subnet's on-chain account id (treasury / subnet key), if the subnet exists.
        fn get_subnet_account_id(netuid: NetUid) -> Option<AccountId32>;
        /// Absolute block when the next tempo epoch starts for `netuid`.
        fn get_next_epoch_start_block(netuid: NetUid) -> Option<u64>;
        /// Network-wide TAO emission for the current block (rao).
        fn get_block_emission() -> TaoBalance;
    }

    /// Stake positions, fees, coldkey locks, and hotkey conviction queries.
    pub trait StakeInfoRuntimeApi {
        /// All stake positions owned by one coldkey.
        fn get_stake_info_for_coldkey( coldkey_account: AccountId32 ) -> Vec<StakeInfo<AccountId32>>;
        /// Stake positions for many coldkeys: `(coldkey, positions)`.
        fn get_stake_info_for_coldkeys( coldkey_accounts: Vec<AccountId32> ) -> Vec<(AccountId32, Vec<StakeInfo<AccountId32>>)>;
        /// Single `(hotkey, coldkey, netuid)` stake row, if present.
        fn get_stake_info_for_hotkey_coldkey_netuid( hotkey_account: AccountId32, coldkey_account: AccountId32, netuid: NetUid ) -> Option<StakeInfo<AccountId32>>;
        /// Per-coldkey, per-netuid stake availability; `netuids == None` means all subnets.
        fn get_stake_availability_for_coldkeys( coldkey_accounts: Vec<AccountId32>, netuids: Option<Vec<NetUid>> ) -> BTreeMap<AccountId32, BTreeMap<NetUid, StakeAvailability>>;
        /// Fee (rao) to move `amount` between optional origin/destination stake endpoints.
        fn get_stake_fee( origin: Option<(AccountId32, NetUid)>, origin_coldkey_account: AccountId32, destination: Option<(AccountId32, NetUid)>, destination_coldkey_account: AccountId32, amount: u64 ) -> u64;
        /// Coldkey lock state on `netuid`, if a lock exists.
        fn get_coldkey_lock(coldkey: AccountId32, netuid: NetUid) -> Option<LockState>;
        /// Hotkey conviction score on `netuid` (`U64F64`).
        fn get_hotkey_conviction(hotkey: AccountId32, netuid: NetUid) -> U64F64;
        /// Hotkey with the highest conviction on `netuid`, if any.
        fn get_most_convicted_hotkey_on_subnet(netuid: NetUid) -> Option<AccountId32>;
    }

    /// Cost (TAO / rao) to register a new subnet at the current block.
    pub trait SubnetRegistrationRuntimeApi {
        /// Lock cost required to create a new subnet (rao).
        fn get_network_registration_cost() -> TaoBalance;
    }

    /// Proxy type catalog and which calls each proxy type may dispatch.
    pub trait ProxyFilterRuntimeApi {
        /// All registered proxy type descriptors.
        fn get_proxy_types() -> Vec<ProxyTypeInfo>;
        /// Filter rules; `proxy_types == None` returns filters for every type.
        fn get_proxy_filters(proxy_types: Option<Vec<u8>>) -> Vec<ProxyFilterInfo>;
    }
}
